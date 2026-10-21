//! Small scheduler stress test. It only uses the tasking API:
//! spawn_thread, yield_task, wake_thread, the SLEEPING protocol, thread exit.
//!
//! Phases:
//!   1. yield storm : context switches + mutual exclusion of the irq-save lock
//!   2. ring        : sleep/wake handshake with several tokens in flight
//!   3. churn       : spawn/exit storm (also spawning from non-driver threads)
//!   4. hogs        : preemption really happens (checked if PREEMPTION)
//!   5. chaos       : random mix, sleepers woken by helper threads while they
//!                    are going to sleep (the lost-wakeup window)
//! The whole sequence is repeated ROUNDS times: races are probabilistic.

use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use core::sync::atomic::{
    AtomicBool, AtomicIsize, AtomicUsize,
    Ordering::{AcqRel, Acquire, Relaxed, Release},
};

use crate::{
    lock::{crit_sec::CriticalGuard, spinlock::SpinLockIrqSave},
    mm::VirtAddr,
    println, CpuInfo,
};

use super::{
    scheduler::{wake_thread, yield_task},
    Thread, ThreadState,
};

// ------------------------------------------------------------------ config

const NCPUS: usize = 4; // must match qemu -smp
const PREEMPTION: bool = true;
const ROUNDS: usize = 10;

const YIELD_WORKERS: usize = 8;
const YIELD_ITERS: usize = 20_000;

const RING_N: usize = 12;
const RING_TOKENS: usize = 3;
const RING_BUDGET: isize = 200_000;

const CHURN_ROUNDS: usize = 200;
const CHURN_BURST: usize = 8;

const HOG_ITERS: u64 = 200_000_000; // must last several ticks

const CHAOS_WORKERS: usize = 10;
const CHAOS_ITERS: usize = 3_000;

const MAX_W: usize = 16;
const STALL_LIMIT: usize = 30_000_000; // watchdog yields without progress
const WAIT_LIMIT: usize = 50_000_000; // driver polling iterations

const _: () = assert!(RING_N <= MAX_W && CHAOS_WORKERS <= MAX_W);

// ----------------------------------------------------------------- helpers

static SEQ: AtomicUsize = AtomicUsize::new(0); // worker index dispenser
static PROGRESS: AtomicUsize = AtomicUsize::new(0);
static SPAWNED: AtomicUsize = AtomicUsize::new(0);
static EXITED: AtomicUsize = AtomicUsize::new(0);
static WATCHDOG_STOP: AtomicBool = AtomicBool::new(false);
static CPU_HITS: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
static GUARDED: SpinLockIrqSave<u64> = SpinLockIrqSave::new(0);
static PHASE: SpinLockIrqSave<&'static str> = SpinLockIrqSave::new("boot");

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

fn cpu_id() -> usize {
    (  core::arch::x86_64::__cpuid(1).ebx >> 24) as usize
}

/// cpuid is slow in a VM: only sample it from time to time
fn hit() {
    CPU_HITS[cpu_id() & 7].fetch_add(1, Relaxed);
}

fn spin(n: u64) {
    for _ in 0..n {
        core::hint::spin_loop();
    }
}

fn current_thread() -> Arc<Thread> {
    CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().cloned().unwrap())
}

fn spawn(f: fn()) {
    super::spawn_thread(VirtAddr(f as usize as u64));
}

/// every thread spawned this way MUST call `finish()` as its last action
fn spawn_counted(f: fn()) {
    SPAWNED.fetch_add(1, Relaxed);
    spawn(f);
}

fn finish() {
    EXITED.fetch_add(1, Relaxed);
    PROGRESS.fetch_add(1, Relaxed);
}

fn wait_all_exited() {
    let mut spins = 0usize;
    while EXITED.load(Relaxed) != SPAWNED.load(Relaxed) {
        yield_task();
        spins += 1;
        if spins > WAIT_LIMIT {
            panic!(
                "[stress] threads never exited in phase '{}': spawned={} exited={}",
                *PHASE.lock(),
                SPAWNED.load(Relaxed),
                EXITED.load(Relaxed)
            );
        }
    }
}

// --------------------------------------------------------------- semaphore
//
// "State change under the lock" protocol:
//   wait : under the lock { token ? take it : enqueue self + state=SLEEPING },
//          then (lock released) yield, then re-test (wake-ups may be spurious
//          or stolen by a thread that never slept)
//   post : under the lock { count++ ; pop one waiter }, then wake_thread()

struct SemState {
    count: usize,
    waiters: VecDeque<Arc<Thread>>,
}

pub struct Semaphore {
    state: SpinLockIrqSave<SemState>,
}

impl Semaphore {
    const fn new() -> Self {
        Self {
            state: SpinLockIrqSave::new(SemState { count: 0, waiters: VecDeque::new() }),
        }
    }

    fn reset(&self) {
        let mut s = self.state.lock();
        assert!(s.waiters.is_empty(), "semaphore reset with sleepers still queued");
        s.count = 0;
    }

    fn wait(&self) {
        loop {
            let mut s = self.state.lock();
            if s.count > 0 {
                s.count -= 1;
                return;
            }

            let me = current_thread();
            s.waiters.push_back(Arc::clone(&me));
            me.set_state(ThreadState::SLEEPING); // under the lock: this is the whole point
            drop(s);

            yield_task();
        }
    }

    fn post(&self) {
        let waiter = {
            let mut s = self.state.lock();
            s.count += 1;
            s.waiters.pop_front()
        };

        if let Some(w) = waiter {
            wake_thread(w);
        }
    }
}

static DONE: Semaphore = Semaphore::new();

fn begin_phase(name: &'static str) {
    *PHASE.lock() = name;
    DONE.reset();
    SEQ.store(0, Relaxed);
    println!("[stress] {name} ...");
}

fn end_phase(name: &str) {
    wait_all_exited();
    println!("[stress] {name} ok");
}

// ------------------------------------------------------- phase 1: yield storm

fn yield_worker() {
    let me = SEQ.fetch_add(1, Relaxed);

    for i in 0..YIELD_ITERS {
        {
            // non-atomic read-modify-write with a widened window: any hole in
            // the lock (or an interrupt leaking in) loses an update
            let mut g = GUARDED.lock();
            let v = *g;
            core::hint::black_box(v);
            *g = v + 1;
        }

        if i % 64 == 0 {
            hit();
        }
        if i % 3 == 0 {
            spin((me as u64 * 7) % 64);
        }

        PROGRESS.fetch_add(1, Relaxed);
        yield_task();
    }

    DONE.post();
    finish();
}

fn phase_yield() {
    begin_phase("yield storm");
    *GUARDED.lock() = 0;

    for _ in 0..YIELD_WORKERS {
        spawn_counted(yield_worker);
    }
    for _ in 0..YIELD_WORKERS {
        DONE.wait();
    }

    let got = *GUARDED.lock();
    assert_eq!(
        got,
        (YIELD_WORKERS * YIELD_ITERS) as u64,
        "lost update inside SpinLockIrqSave"
    );

    end_phase("yield storm");
}

// ------------------------------------------------------------ phase 2: ring

static RING: [Semaphore; MAX_W] = [const { Semaphore::new() }; MAX_W];
static BUDGET: AtomicIsize = AtomicIsize::new(0);
static STEPS: AtomicUsize = AtomicUsize::new(0);
static STOP: AtomicBool = AtomicBool::new(false);

fn ring_worker() {
    let me = SEQ.fetch_add(1, Relaxed);
    let next = (me + 1) % RING_N;

    loop {
        RING[me].wait();
        if STOP.load(Acquire) {
            break;
        }

        let left = BUDGET.fetch_sub(1, AcqRel);
        if left >= 1 {
            STEPS.fetch_add(1, Relaxed);
            PROGRESS.fetch_add(1, Relaxed);
        }

        if left <= 1 {
            // budget exhausted: flush everybody so nobody sleeps forever
            STOP.store(true, Release);
            for s in &RING[..RING_N] {
                s.post();
            }
            break;
        }

        RING[next].post();
    }

    DONE.post();
    finish();
}

fn phase_ring() {
    begin_phase("sleep/wake ring");

    for s in &RING[..RING_N] {
        s.reset();
    }
    BUDGET.store(RING_BUDGET, Relaxed);
    STEPS.store(0, Relaxed);
    STOP.store(false, Relaxed);

    for k in 0..RING_TOKENS {
        RING[k * RING_N / RING_TOKENS].post();
    }
    for _ in 0..RING_N {
        spawn_counted(ring_worker);
    }
    for _ in 0..RING_N {
        DONE.wait();
    }

    assert_eq!(
        STEPS.load(Relaxed),
        RING_BUDGET as usize,
        "ring lost or duplicated a wake-up"
    );

    end_phase("sleep/wake ring");
}

// ----------------------------------------------------------- phase 3: churn

fn churn_grandchild() {
    if rdtsc() % 3 == 0 {
        yield_task();
    }
    finish();
}

fn churn_child() {
    let mut rng = Rng::new(rdtsc());

    for _ in 0..(rng.next_u64() % 4) {
        yield_task();
    }
    if rng.next_u64() % 4 == 0 {
        spawn_counted(churn_grandchild); // spawn from a non-driver thread
    }

    finish();
}

fn phase_churn() {
    begin_phase("spawn/exit churn");

    for _ in 0..CHURN_ROUNDS {
        for _ in 0..CHURN_BURST {
            spawn_counted(churn_child);
        }
        yield_task();
    }

    end_phase("spawn/exit churn");
}

// ------------------------------------------------------------- phase 4: hogs

static HOGS_RUNNING: AtomicUsize = AtomicUsize::new(0);
static HB_DURING_HOGS: AtomicUsize = AtomicUsize::new(0);
static HB_STOP: AtomicBool = AtomicBool::new(false);

fn hog() {
    HOGS_RUNNING.fetch_add(1, AcqRel);

    let mut x = 0u64;
    for i in 0..HOG_ITERS {
        x = core::hint::black_box(x.wrapping_add(i));
        if i % 1_000_000 == 0 {
            PROGRESS.fetch_add(1, Relaxed);
        }
    }

    HOGS_RUNNING.fetch_sub(1, AcqRel);
    DONE.post();
    finish();
}

fn heartbeat() {
    while !HB_STOP.load(Relaxed) {
        PROGRESS.fetch_add(1, Relaxed);
        // counted only while EVERY cpu runs a hog: if we run now, a hog was preempted
        if HOGS_RUNNING.load(Acquire) == NCPUS {
            HB_DURING_HOGS.fetch_add(1, Relaxed);
        }
        yield_task();
    }
    finish();
}

fn phase_hog() {
    begin_phase("cpu hogs");

    HOGS_RUNNING.store(0, Relaxed);
    HB_DURING_HOGS.store(0, Relaxed);
    HB_STOP.store(false, Relaxed);

    spawn_counted(heartbeat);
    for _ in 0..NCPUS {
        spawn_counted(hog);
    }
    for _ in 0..NCPUS {
        DONE.wait();
    }
    HB_STOP.store(true, Relaxed);

    let hb = HB_DURING_HOGS.load(Relaxed);
    println!("[stress]   heartbeat ran {hb} times while every cpu was hogged");
    if PREEMPTION {
        assert!(hb > 0, "preemption enabled but a hogged cpu was never preempted");
    }

    end_phase("cpu hogs");
}

// ------------------------------------------------------------ phase 5: chaos

static PRIV: [Semaphore; MAX_W] = [const { Semaphore::new() }; MAX_W];
static JOBS: SpinLockIrqSave<Vec<usize>> = SpinLockIrqSave::new(Vec::new());
static CHAOS_EXPECT: AtomicUsize = AtomicUsize::new(0);

/// wakes a sleeper after a random delay, usually from another cpu
fn chaos_helper() {
    let mut rng = Rng::new(rdtsc());
    let job = JOBS.lock().pop();

    spin(rng.next_u64() % 600);
    if let Some(i) = job {
        PRIV[i].post();
    }

    finish();
}

fn chaos_worker() {
    let me = SEQ.fetch_add(1, Relaxed);
    let mut rng = Rng::new(rdtsc() ^ ((me as u64 + 1) << 40));

    for i in 0..CHAOS_ITERS {
        match rng.next_u64() % 7 {
            0 => yield_task(),
            1 => spin(rng.next_u64() % 4_000),
            2 | 3 => {
                // 1 job pushed <=> 1 helper spawned, so this sleep is always
                // matched by exactly one wake-up, racing with our own sleep
                JOBS.lock().push(me);
                spawn_counted(chaos_helper);
                spin(rng.next_u64() % 300);
                PRIV[me].wait();
            }
            4 => spawn_counted(churn_grandchild),
            5 => {
                let mut g = GUARDED.lock();
                let v = *g;
                core::hint::black_box(v);
                *g = v + 1;
                CHAOS_EXPECT.fetch_add(1, Relaxed);
            }
            _ => {
                yield_task();
                yield_task();
            }
        }

        if i % 64 == 0 {
            hit();
        }
        PROGRESS.fetch_add(1, Relaxed);
    }

    DONE.post();
    finish();
}

fn phase_chaos() {
    begin_phase("chaos");

    for s in &PRIV[..CHAOS_WORKERS] {
        s.reset();
    }
    *GUARDED.lock() = 0;
    CHAOS_EXPECT.store(0, Relaxed);
    JOBS.lock().clear();

    for _ in 0..CHAOS_WORKERS {
        spawn_counted(chaos_worker);
    }
    for _ in 0..CHAOS_WORKERS {
        DONE.wait();
    }

    wait_all_exited();

    let got = *GUARDED.lock() as usize;
    assert_eq!(got, CHAOS_EXPECT.load(Relaxed), "lost update during chaos");
    assert!(JOBS.lock().is_empty(), "a sleeper was woken without its helper running");

    println!("[stress] chaos ok");
}

// ------------------------------------------------------------------ driver

fn watchdog() {
    let mut last = PROGRESS.load(Relaxed);
    let mut stalled = 0usize;

    while !WATCHDOG_STOP.load(Relaxed) {
        yield_task();

        let now = PROGRESS.load(Relaxed);
        if now != last {
            last = now;
            stalled = 0;
            continue;
        }

        stalled += 1;
        if stalled > STALL_LIMIT {
            panic!(
                "[stress] watchdog: no progress in phase '{}' (spawned={} exited={}): deadlock or lost wake-up",
                *PHASE.lock(),
                SPAWNED.load(Relaxed),
                EXITED.load(Relaxed)
            );
        }
    }
}

fn driver() {
    println!("[stress] start: ncpus={NCPUS} preemption={PREEMPTION} rounds={ROUNDS}");

    spawn(watchdog);

    for round in 1..=ROUNDS {
        println!("[stress] ===== round {round}/{ROUNDS} =====");
        phase_yield();
        phase_ring();
        phase_churn();
        phase_hog();
        phase_chaos();
    }

    for c in 0..NCPUS.min(8) {
        let h = CPU_HITS[c].load(Relaxed);
        println!("[stress] cpu {c}: {h} samples");
        if h == 0 {
            println!("[stress] WARNING: cpu {c} never ran a worker");
        }
    }

    println!(
        "[stress] ALL PHASES PASSED (spawned {}, exited {})",
        SPAWNED.load(Relaxed),
        EXITED.load(Relaxed)
    );
    WATCHDOG_STOP.store(true, Relaxed);
}

// ------------------------------------------------------------- entry points

/// run by the BSP idle thread (and usable by every AP)
pub fn idle_loop() -> ! {
    loop {
        yield_task();
        if PREEMPTION {
            // sti only takes effect after the next instruction: no window
            // where an interrupt slips in between sti and hlt
            unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)) };
        } else {
            core::hint::spin_loop();
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn TASK_stress() {
    spawn(driver);
    idle_loop()
}