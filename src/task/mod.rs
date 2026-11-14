pub mod scheduler;
mod ffi;

use core::{cell::UnsafeCell, sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering::{self, AcqRel, Acquire, Relaxed, Release}}};

use alloc::{collections::BTreeMap, sync::{Arc, Weak}, vec::Vec};

use crate::{CpuInfo, lock::{crit_sec::CriticalGuard, mutex::Mutex, spinlock::SpinLockIrqSave}, mm::{MEM_get_kernel_addresspace, PhyAddr, VirtAddr, get_hhdm, physical::PHYSICAL_MEMORY_ALLOCATOR, virtual_area::AddressSpace}, task::{ThreadState::SLEEPING, scheduler::{thread_start, wake_thread}}, vfs::Vnode};

unsafe extern "C" {
    fn context_switch(cr3: u64, old_rsp_ptr: *mut u64, new_rsp: u64);

    static KERNEL_STACK_PAGES: u32;
}

type PID = usize;
type TID = usize;

#[derive(Debug)]
pub enum TaskError {
    OutOfMemory,
    ProcessNotMySon,    // hee heee !
    ProcessIsChildless,
    ProcessStillRunning,
}

pub struct OpenFile {
    pub node: Arc<dyn Vnode>,
    pub flags: u32,
    pub offset: Mutex<u64>,
}

impl OpenFile {
    pub fn new(node: Arc<dyn Vnode>, flags: u32) -> Self {
        Self { node, flags, offset: Mutex::new(0) }
    }
}

pub struct FileDescriptor {
    pub file: Arc<OpenFile>,
    pub is_cloexec: bool,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq)]
pub enum ProcessState {
    ALIVE,
    ZOMBIE,
    DEAD,
}

impl ProcessState {
    fn from_u8(state: u8) -> Self {
        const ALIVE: u8 = ProcessState::ALIVE as u8;
        const ZOMBIE: u8 = ProcessState::ZOMBIE as u8;
        const DEAD: u8 = ProcessState::DEAD as u8;
        match state {
            ALIVE     => ProcessState::ALIVE,
            ZOMBIE       => ProcessState::ZOMBIE,
            DEAD        => ProcessState::DEAD,
            _ => unreachable!("unknown process state")
        }
    }
}

pub struct Process {
    pub pid: PID,
    ppid: AtomicUsize,
    state: AtomicU8,
    addr_space: Mutex<AddressSpace>,
    cr3: PhyAddr,
    children: SpinLockIrqSave<Vec<Arc<Process>>>,
    threads: SpinLockIrqSave<Vec<Weak<Thread>>>,
    status: AtomicU32,
    pub resources: SpinLockIrqSave<[Option<FileDescriptor>; Self::MAX_RESOURCES]>,
    pub current_dir: Arc<dyn Vnode>,
}

impl Process {
    pub const MAX_RESOURCES: usize = 128;
    
    fn new_idle(id: PID) -> Self {
        let kernel_cr3 = MEM_get_kernel_addresspace();
        let pml4 = MEM_get_kernel_addresspace() + get_hhdm();

        Self {
            pid: id,
            ppid: AtomicUsize::new(0),
            state: AtomicU8::new(ProcessState::ALIVE as u8),
            addr_space: Mutex::new(AddressSpace::new(VirtAddr(pml4))),
            cr3: PhyAddr(kernel_cr3),
            children: SpinLockIrqSave::new(Vec::new()),
            threads: SpinLockIrqSave::new(Vec::new()),
            status: AtomicU32::new(0),
            resources: SpinLockIrqSave::new(core::array::from_fn(|_| None)),
            current_dir: crate::vfs::ROOT_FILESYSTEM.lock().as_ref().expect("VFS not initialized").root(),
        }
    }

    fn _new (id: PID, pid: PID, current_dir: Arc<dyn Vnode>) -> Result<Self, TaskError>{
        let new_addr_space = PHYSICAL_MEMORY_ALLOCATOR
        .lock()
        .alloc_page()
        .ok_or(TaskError::OutOfMemory)?;

        let kernel_cr3 = MEM_get_kernel_addresspace();
        unsafe {
            let PhyAddr(addr_space) = new_addr_space;
            let addr_space = addr_space + get_hhdm();
            let kernel_addr_space = kernel_cr3 + get_hhdm();

            // higher half
            core::ptr::copy_nonoverlapping((kernel_addr_space + 256 * 8) as *mut u8, (addr_space + 256 * 8) as *mut u8, 256 * 8);
        }

        Ok(Self { 
            pid: id,
            ppid: AtomicUsize::new(pid),
            state: AtomicU8::new(ProcessState::ALIVE as u8),
            addr_space: Mutex::new(AddressSpace::new(VirtAddr(kernel_cr3 + get_hhdm()))),
            cr3: PhyAddr(kernel_cr3),
            children: SpinLockIrqSave::new(Vec::new()),
            threads: SpinLockIrqSave::new(Vec::new()),
            status: AtomicU32::new(0),
            resources: SpinLockIrqSave::new(core::array::from_fn(|_| None)),
            current_dir
        })
    }

    pub fn get_state(&self) -> ProcessState {
        ProcessState::from_u8(self.state.load(Acquire))
    }

    pub fn set_state(&self, state: ProcessState) {
        self.state.store(state as u8, Release);
    }

    fn test_and_set_state(&self, current: ProcessState, new: ProcessState) -> ProcessState {
        let state = match self.state.compare_exchange(current as u8, new as u8, AcqRel, Acquire) {
            Ok(success) => success,
            Err(failed) => failed 
        };

        ProcessState::from_u8(state)
    }

    fn add_thread(&self, thread: Weak<Thread>) {
        self.threads.lock().push(thread);
    }

    // fn add_child(&self, process: Arc<Process>) {
    //     self.children.lock().push(process);
    // }

    pub fn alloc_resource(&self, res: FileDescriptor) -> Option<usize> {
        let mut resources = self.resources.lock();
        let index = resources.iter().position(|r| r.is_none())?;

        resources[index] = Some(res);
        Some(index)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.addr_space.try_lock().expect("impossible !!").clean();
        PHYSICAL_MEMORY_ALLOCATOR.lock().free_page(self.cr3);
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
    NEW,
    RUNNING,
    READY,
    SLEEPING,
    WAITING,
    DEAD,
}

impl ThreadState {
    fn from_u8(state: u8) -> Self {
        const NEW: u8 = ThreadState::NEW as u8;
        const RUNNING: u8 = ThreadState::RUNNING as u8;
        const READY: u8 = ThreadState::READY as u8;
        const SLEEPING: u8 = ThreadState::SLEEPING as u8;
        const WAITING: u8 = ThreadState::WAITING as u8;
        const DEAD: u8 = ThreadState::DEAD as u8;
        match state {
            NEW         => ThreadState::NEW,
            RUNNING     => ThreadState::RUNNING,
            READY       => ThreadState::READY,
            DEAD        => ThreadState::DEAD,
            SLEEPING    => ThreadState::SLEEPING,
            WAITING     => ThreadState::WAITING,
            _ => unreachable!("unknown thread state")
        }
    }
}

pub struct Thread {
    tid: TID,
    state: AtomicU8,
    pub process: Arc<Process>,
    rsp: UnsafeCell<u64>,
    kernel_stack: VirtAddr,
    entry_point: VirtAddr,
}

impl Thread {

    const KERNEL_STACK_PAGES: usize = 32;

    pub fn create_idle(stack: VirtAddr, idle_proc: Arc<Process>, id: TID) -> Self {

        Self { tid: id, state: AtomicU8::new(ThreadState::RUNNING as u8), process: idle_proc, rsp: UnsafeCell::new(0), kernel_stack: stack, entry_point: VirtAddr(0) }
    }

    pub fn new(entry_point: VirtAddr, context: &TaskContext, process: Arc<Process>, id: TID) -> Result<Self, TaskError> {

        let kernel_stack = crate::mm::paging::vmalloc::VMALLOC.alloc_blocks(Self::KERNEL_STACK_PAGES).ok_or(TaskError::OutOfMemory)?;

        let stack_context_ptr = {
            let kernel_stack_top = kernel_stack.0 + crate::mm::get_pagesize();
            (kernel_stack_top - core::mem::size_of::<TaskContext>() as u64) as *mut TaskContext
        };
        let stack_context = unsafe { &mut *stack_context_ptr };
        
        *stack_context = *context;
        stack_context.rip = thread_start as *const u8 as u64;

        Ok(Self { 
            tid: id,
            state: AtomicU8::new(ThreadState::NEW as u8), 
            process, 
            rsp: UnsafeCell::new(stack_context_ptr as u64), 
            kernel_stack, 
            entry_point
        })
    }

    pub fn get_state(&self) -> ThreadState {
        ThreadState::from_u8(self.state.load(Acquire))
    }

    pub fn set_state(&self, state: ThreadState) {
        self.state.store(state as u8, Release);
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
        crate::mm::paging::vmalloc::VMALLOC.free_blocks(self.kernel_stack, Self::KERNEL_STACK_PAGES);
        // println!("thread id: {} is dying !!", self.tid);
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
    processes: SpinLockIrqSave<BTreeMap<PID, Arc<Process>>>,
    threads: SpinLockIrqSave<BTreeMap<TID, Arc<Thread>>>,
    reaper_thread_id: AtomicUsize,
    dead_threads: SpinLockIrqSave<Vec<Arc<Thread>>>,
}

impl TaskManager {
    const fn new() -> Self{
        Self { 
            processes: SpinLockIrqSave::new(BTreeMap::new()), 
            threads: SpinLockIrqSave::new(BTreeMap::new()),
            reaper_thread_id: AtomicUsize::new(0xDEADBEEF),
            dead_threads: SpinLockIrqSave::new(Vec::new())
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
        
        wake_thread(thread_reaper);
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
        {
            let _dead_batch: Vec<Arc<Thread>> = TASK_MANAGER.dead_threads.lock().drain(..).collect();
        }   // die here

        let mut is_empty = false;
        {
            let dead_thread = TASK_MANAGER.dead_threads.lock();
            if dead_thread.is_empty() {
                CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().set_state(SLEEPING));
                is_empty = true;
            }
        }

        if is_empty { scheduler::yield_task(); }
    }
}

pub fn wait_anychildproc() -> Result<(PID, u32), TaskError> {
    let current = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());
    let mut children = current.children.lock();

    if children.is_empty() {
        return Err(TaskError::ProcessIsChildless);
    }

    let Some(zombie_idx) = children
    .iter()
    .position(|proc| proc.test_and_set_state(ProcessState::ZOMBIE, ProcessState::DEAD) == ProcessState::ZOMBIE) else {
        return Err(TaskError::ProcessStillRunning);
    };
    

    let zombie = children.remove(zombie_idx);
    drop(children);

    TASK_MANAGER.remove_process(zombie.pid).expect("corruption");
    Ok((zombie.pid, zombie.status.load(Acquire)))
}

pub fn wait_process(id: PID) -> Result<(PID, u32), TaskError> {
    let current = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());
    let mut children = current.children.lock();

    let child_idx = children.iter().position(|proc| proc.pid == id).ok_or(TaskError::ProcessNotMySon)?.clone();

    if children[child_idx].test_and_set_state(ProcessState::ZOMBIE, ProcessState::DEAD) != ProcessState::ZOMBIE {
        return Err(TaskError::ProcessStillRunning);
    }

    let child = children.remove(child_idx);
    drop(children); // to re-enable interrupt

    TASK_MANAGER.remove_process(child.pid).expect("corruption");
    Ok((child.pid, child.status.load(Acquire)))
}

pub fn exit_thread(code: i32) -> ! {
    {
        let current_thread = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().cloned().unwrap());

        let zero_thread_left = {
            let mut this_process_threads = current_thread.process.threads.lock();
            this_process_threads.retain(|weak_thread| {
                weak_thread.upgrade().map_or(false, |this_thread| !Arc::ptr_eq(&this_thread, &current_thread))
            });

            this_process_threads.is_empty()
        };

        if zero_thread_left {
            if current_thread.process.pid <= 1 {
                panic!("attempted to kill init");
            }

            let this_process = current_thread.process.clone();
            let init = TASK_MANAGER.get_process(1).expect("no init found");

            let children = core::mem::take(&mut *this_process.children.lock());
            
            // init adoption
            for child in &children {
                child.ppid.store(1, Relaxed);
            }

            init.children.lock().extend(children);

            let status = this_process.status.load(Ordering::Relaxed);
            this_process.status.store(((code as u32 & 0xff) << 8) | status, Ordering::Relaxed);
            this_process.set_state(ProcessState::ZOMBIE);
        }

        TASK_MANAGER.remove_thread(current_thread.tid);
        current_thread.set_state(ThreadState::DEAD);
    }

    scheduler::yield_task();

    unreachable!("shouldnt get here !!");
}

pub fn spawn_thread(entry_point: VirtAddr) {

    let context = TaskContext {..Default::default()};

    let current_process = CriticalGuard::with(|| {
        CpuInfo::get_current()
        .current_thread
        .as_ref()
        .unwrap()
        .process.clone()
    });
    let thread_arc = {
        let tid = alloc_tid();
        let thread = Thread::new(
            entry_point,
            &context,
            current_process.clone(),tid
        ).unwrap();

        TASK_MANAGER.add_thread(thread, tid)
    };
    current_process.add_thread(Arc::downgrade(&thread_arc));

    match scheduler::schedule_thread(thread_arc) {
        Ok(_) => (),
        Err(_) => panic!("failed to schedule a new thread"),
    };
}

#[unsafe(no_mangle)]
pub extern "C" fn TASK_init_idle_thread() {
    let cpu = CpuInfo::get_current();

    let idle_process = TASK_MANAGER.get_process(0).expect("idle process not found!");
    let idle_thread_arc = {
        let tid = alloc_tid();
        let stack_bottom = unsafe {
            cpu.kernel_stack_top.0 - (KERNEL_STACK_PAGES as u64 * 0x1000)
        };
        let idle_thread = Thread::create_idle(VirtAddr(stack_bottom), idle_process.clone(), tid);

        TASK_MANAGER.add_thread(idle_thread, tid)    // do we really need to put the idle thread in the task manager? idk!
    };

    idle_process.add_thread(Arc::downgrade(&idle_thread_arc));

    cpu.idle_thread = Some(idle_thread_arc.clone());
    cpu.current_thread = Some(idle_thread_arc);
}

#[unsafe(no_mangle)]
pub extern "C" fn TASK_create_idle_process() {    // this function will be called once at the start thus the pid will be 0
    let pid = alloc_pid();
    let idle_proc = TASK_MANAGER.add_process(Process::new_idle(pid), pid);

    let tid = alloc_tid();
    let context = TaskContext {..Default::default()};
    let thread_reaper = Thread::new(
        VirtAddr(reaper_thread as *const u8 as u64), 
        &context, idle_proc, 
        tid
    ).unwrap();
    thread_reaper.set_state(ThreadState::WAITING);
    
    TASK_MANAGER.add_thread(thread_reaper, tid);
    TASK_MANAGER.reaper_thread_id.store(tid, Relaxed);
}