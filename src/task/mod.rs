pub mod scheduler;

use core::{cell::UnsafeCell, sync::atomic::{AtomicU8, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed}}};

use alloc::{collections::BTreeMap, sync::{Arc, Weak}, vec::Vec};

use crate::{CpuInfo, mm::{MEM_get_kernel_addresspace, PhyAddr, VirtAddr, get_hhdm, physical::PHYSICAL_MEMORY_ALLOCATOR}, println, lock::spinlock::SpinLock, task::ThreadState::{READY, WAITING}};

unsafe extern "C" {
    fn context_switch(cr3: u64, old_rsp_ptr: *mut u64, new_rsp: u64);
}

type PID = usize;
type TID = usize;

pub struct Process {
    pid: PID,
    addr_space: PhyAddr,
    threads: SpinLock<Vec<Weak<Thread>>>
}

impl Process {
    fn new_idle(id: PID) -> Self {
        Self {
            pid: id,
            addr_space: PhyAddr(MEM_get_kernel_addresspace()),
            threads: SpinLock::new(Vec::new())
        }
    }

    fn new (id: PID) -> Result<Self, &'static str>{
        let new_addr_space = PHYSICAL_MEMORY_ALLOCATOR
        .lock()
        .alloc_page()
        .ok_or("out of memory")?;

        let kernel_addr_space = MEM_get_kernel_addresspace();
        unsafe {
            let PhyAddr(addr_space) = new_addr_space;
            let addr_space = addr_space as *mut u8;
            let kernel_addr_space = kernel_addr_space as *mut u8;

            core::ptr::copy_nonoverlapping(kernel_addr_space, addr_space, crate::mm::get_pagesize() as usize);
        }

        Ok(Self { 
            pid: id,
            addr_space: new_addr_space,
            threads: SpinLock::new(Vec::new())
        })
    }

    fn add_thread(&self, thread: Weak<Thread>) {
        self.threads.lock().push(thread);
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // todo destroy address space correctly here by deallocating each level
        PHYSICAL_MEMORY_ALLOCATOR.lock().free_page(self.addr_space);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TaskContext {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rbp: u64,
    rip: u64,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq)]
pub enum ThreadState {
    RUNNING,
    READY,
    WAITING,
    DEAD,
}

impl ThreadState {
    fn from_u8(state: u8) -> Self {
        const RUNNING: u8 = ThreadState::RUNNING as u8;
        const READY: u8 = ThreadState::READY as u8;
        const WAITING: u8 = ThreadState::WAITING as u8;
        const DEAD: u8 = ThreadState::DEAD as u8;
        match state {
            RUNNING     => ThreadState::RUNNING,
            READY       => ThreadState::READY,
            DEAD        => ThreadState::DEAD,
            WAITING     => ThreadState::WAITING,
            _ => unreachable!("unknown thread state")
        }
    }
}

pub struct Thread {
    tid: TID,
    state: AtomicU8,
    process: Arc<Process>,
    rsp: UnsafeCell<u64>,
    kernel_stack: VirtAddr,
    entry_point: VirtAddr,
}

impl Thread {
    pub fn create_idle(stack: VirtAddr, idle_proc: Arc<Process>, id: TID) -> Self {

        Self { tid: id, state: AtomicU8::new(ThreadState::RUNNING as u8), process: idle_proc, rsp: UnsafeCell::new(0), kernel_stack: stack, entry_point: VirtAddr(0) }
    }

    pub fn new(state: ThreadState, context: &TaskContext, process: Arc<Process>, id: TID) -> Result<Self, &'static str> {

        let PhyAddr(stack) = PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page().ok_or("out of memory")?;
        let kernel_stack = VirtAddr(stack + get_hhdm());

        let stack_context_ptr = {
            let kernel_stack_top = kernel_stack.0 + crate::mm::get_pagesize();
            (kernel_stack_top - core::mem::size_of::<TaskContext>() as u64) as *mut TaskContext
        };
        let stack_context = unsafe { &mut *stack_context_ptr };
        
        *stack_context = *context;

        Ok(Self { 
            tid: id,
            state: AtomicU8::new(state as u8), 
            process, 
            rsp: UnsafeCell::new(stack_context_ptr as u64), 
            kernel_stack, 
            entry_point: VirtAddr(context.rip)
        })
    }

    fn get_state(&self) -> ThreadState {
        ThreadState::from_u8(self.state.load(Relaxed))
    }

    fn set_state(&self, state: ThreadState) {
        self.state.store(state as u8, Relaxed);
    }

    fn test_and_set_state(&self, current: ThreadState, new: ThreadState) -> ThreadState {
        let state = match self.state.compare_exchange(current as u8, new as u8, AcqRel, Acquire) {
            Ok(success) => success,
            Err(failed) => failed 
        };

        ThreadState::from_u8(state)
    }
}

impl Drop for Thread {
    fn drop(&mut self) {
        let kernel_stack = PhyAddr(self.kernel_stack.0 - get_hhdm());
        PHYSICAL_MEMORY_ALLOCATOR.lock().free_page(kernel_stack);
        println!("thread id: {} is dying !!", self.tid);
    }
}

static NEXT_PID: AtomicUsize = AtomicUsize::new(0);
static NEXT_TID: AtomicUsize = AtomicUsize::new(0);

fn alloc_pid() -> PID {
    NEXT_PID.fetch_add(1, Relaxed)
}

fn alloc_tid() -> TID {
    NEXT_TID.fetch_add(1, Relaxed)
}

struct TaskManager {
    processes: SpinLock<BTreeMap<PID, Arc<Process>>>,
    threads: SpinLock<BTreeMap<TID, Arc<Thread>>>,
    reaper_thread_id: AtomicUsize,
    dead_threads: SpinLock<Vec<Arc<Thread>>>,
}

impl TaskManager {
    const fn new() -> Self{
        Self { 
            processes: SpinLock::new(BTreeMap::new()), 
            threads: SpinLock::new(BTreeMap::new()),
            reaper_thread_id: AtomicUsize::new(0xDEADBEEF),
            dead_threads: SpinLock::new(Vec::new())
        }
    }

    fn add_process(&self, process: Process, id: PID) -> Arc<Process> {
        
        let process_arc = Arc::new(process);
        
        self.processes.lock().insert(id, process_arc.clone());

        process_arc
    }

    fn add_thread(&self, thread: Thread, id: TID) -> Arc<Thread> {
        let thread_arc = Arc::new(thread);

        self.threads.lock().insert(id, thread_arc.clone());

        thread_arc
    }

    fn reap_thread(&self, thread: Arc<Thread>) {
        self.dead_threads.lock().push(thread);
        let thread_reaper = TASK_MANAGER.get_reaper_thread().expect("reaper thread not found");
        
        if thread_reaper.test_and_set_state(WAITING, READY) == ThreadState::WAITING {
            scheduler::schedule_thread(thread_reaper);
        }
    }

    fn remove_thread(&self, id: TID) -> Option<Arc<Thread>> {
        self.threads.lock().remove(&id)
    }

    fn remove_process(&self, id: PID) -> Option<Arc<Process>> {
        self.processes.lock().remove(&id)
    }

    fn get_process(&self, id: PID) -> Option<Arc<Process>> {
        self.processes.lock().get(&id).cloned()
    }

    fn get_thread(&self, id: TID) -> Option<Arc<Thread>> {
        self.threads.lock().get(&id).cloned()
    }

    fn get_reaper_thread(&self) -> Option<Arc<Thread>> {
        let id: TID = self.reaper_thread_id.load(Relaxed);
        if id == 0xDEADBEEF {
            return None;
        }

        self.get_thread(id)
    }
}

static TASK_MANAGER: TaskManager = TaskManager::new();

fn reaper_thread() {
    loop {
        let dead_batch: Vec<Arc<Thread>> = TASK_MANAGER.dead_threads.lock().drain(..).collect();
        if dead_batch.is_empty() {
            CpuInfo::get_current().current_thread.as_ref().unwrap().set_state(WAITING);
            scheduler::yield_task();
            continue;
        }
    }
}

pub fn exit_thread() {
    {
        let current_thread = CpuInfo::get_current().current_thread.as_ref().unwrap();

        current_thread.process.threads.lock().retain(|weak_thread| {
            weak_thread.upgrade().map_or(false, |this_thread| !Arc::ptr_eq(&this_thread, &current_thread))
        });

        if current_thread.process.threads.lock().is_empty() {
            TASK_MANAGER.remove_process(current_thread.process.pid);
        }

        current_thread.set_state(ThreadState::DEAD);
        TASK_MANAGER.remove_thread(current_thread.tid);
    }

    scheduler::yield_task();
}

fn spawn_thread(entry_point: VirtAddr) {

    let context = TaskContext {rip: entry_point.0, ..Default::default()};

    let current_process = CpuInfo::get_current()
    .current_thread
    .as_ref()
    .unwrap()
    .process.clone();
    let thread_arc = {
        let tid = alloc_tid();
        let thread = Thread::new(ThreadState::READY, &context, current_process.clone(),tid).unwrap_or_else(|e| panic!("{e}"));

        TASK_MANAGER.add_thread(thread, tid)
    };
    current_process.add_thread(Arc::downgrade(&thread_arc));

    scheduler::schedule_thread(thread_arc);
}

#[unsafe(no_mangle)]
pub extern "C" fn TASK_init_idle_thread() {
    let cpu = CpuInfo::get_current();

    let idle_process = TASK_MANAGER.get_process(0).expect("idle process not found!");
    let idle_thread_arc = {
        let tid = alloc_tid();
        let idle_thread = Thread::create_idle(cpu.own_stack, idle_process.clone(), tid);

        TASK_MANAGER.add_thread(idle_thread, tid)    // do we really need to put the idle thread in the task manager? idk!
    };

    idle_process.add_thread(Arc::downgrade(&idle_thread_arc));

    cpu.current_thread = Some(idle_thread_arc);
}

#[unsafe(no_mangle)]
pub extern "C" fn TASK_create_idle_process() {    // this function will be called once at the start thus the pid will be 0
    let pid = alloc_pid();
    let idle_proc = TASK_MANAGER.add_process(Process::new_idle(pid), pid);

    let tid = alloc_tid();
    let context = TaskContext {rip: reaper_thread as *const u64 as u64, ..Default::default()};
    let thread_reaper = Thread::new(ThreadState::WAITING, &context, idle_proc, tid).unwrap_or_else(|e| panic!("{e}"));
    
    TASK_MANAGER.add_thread(thread_reaper, tid);
    TASK_MANAGER.reaper_thread_id.store(tid, Relaxed);
}