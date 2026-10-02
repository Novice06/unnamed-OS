use alloc::{boxed::Box, collections::VecDeque};

use crate::{CpuInfo, mm::{PhyAddr, VirtAddr, get_hhdm, physical::PHYSICAL_MEMORY_ALLOCATOR}, println, spinlock::SpinLock, task::{TaskContext, Thread, context_switch}};


static RUNQUEUE: SpinLock<VecDeque<Box<Thread>>> = SpinLock::new(VecDeque::new());


fn yield_task() {
    let Some(next_thread) = RUNQUEUE.lock().pop_front() else {
        return;
    };

    let this_cpu = CpuInfo::get_current();

    // unsafe {
    //     crate::disable_interrupts();
    // }
    
    let mut old_thread = this_cpu.current_thread.take().unwrap();

    let PhyAddr(cr3) = next_thread.process.addr_space;
    let new_rsp = next_thread.rsp;

    let old_rsp_ptr = &mut (*old_thread).rsp as *mut u64;

    match this_cpu.idle_thread {
        Some(_) => RUNQUEUE.lock().push_back(old_thread),
        None => this_cpu.idle_thread = Some(old_thread)
    }

    this_cpu.current_thread = Some(next_thread);

    // unsafe {
    //     crate::enable_interrupts();
    // }

    unsafe {
        context_switch(cr3, old_rsp_ptr, new_rsp);
    }

}

fn spawn_thread(entry_point: VirtAddr) {

    let PhyAddr(stack) = PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page().expect("out of memory");
    let kernel_stack = VirtAddr(stack + get_hhdm());

    let kernel_stack_top = kernel_stack.0 + 4096;
    let context_ptr = (kernel_stack_top - core::mem::size_of::<TaskContext>() as u64) as *mut TaskContext;
    let context = unsafe { &mut *context_ptr };

    context.r12 = 0;
    context.r13 = 0;
    context.r14 = 0;
    context.r15 = 0;
    context.rbp = 0;
    context.rbx = 0;
    context.rip = entry_point.0;

    RUNQUEUE
    .lock()
    .push_back(
        Box::new(Thread::new(entry_point, context_ptr as u64, kernel_stack))
    );
}

type Task = fn() -> !;

fn TaskA() -> ! {
    loop {
        println!("TaskA running");
        yield_task();
    }
}

fn TaskB() -> ! {
    loop {
        println!("TaskB running");
        yield_task();
    }
}

#[unsafe(no_mangle)]
extern "C" fn TASK_test() {

    let taska_ptr: Task = TaskA;
    let taskb_ptr: Task = TaskB;

    println!("size of context {}", core::mem::size_of::<TaskContext>());
    spawn_thread(VirtAddr(taska_ptr as u64));
    spawn_thread(VirtAddr(taskb_ptr as u64));
    yield_task();
}