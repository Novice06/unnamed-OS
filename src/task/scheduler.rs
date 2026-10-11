use alloc::{collections::VecDeque, sync::Arc};

use crate::{CpuInfo, mm::{PhyAddr, VirtAddr}, println, spinlock::SpinLock, task::{TASK_MANAGER, TaskContext, Thread, ThreadState, context_switch}};


static RUNQUEUE: SpinLock<VecDeque<Arc<Thread>>> = SpinLock::new(VecDeque::new());

pub fn schedule_thread(thread: Arc<Thread>) {
    thread.set_state(ThreadState::READY);
    RUNQUEUE.lock().push_back(thread);
}

pub fn yield_task() {
    let this_cpu = CpuInfo::get_current();

    let next_thread = if let Some(thread) = RUNQUEUE.lock().pop_front() {
        thread
    } else if this_cpu.current_thread.as_ref().unwrap().get_state() != ThreadState::RUNNING {
        this_cpu.idle_thread.take().unwrap()
    }
    else {
        return;
    };

    let old_thread = this_cpu.current_thread.take().unwrap();

    let PhyAddr(cr3) = next_thread.process.addr_space;
    let new_rsp = unsafe {
        *next_thread.rsp.get()
    };

    let old_rsp_ptr = old_thread.rsp.get();

    match this_cpu.idle_thread {
        Some(_) => {
            if old_thread.get_state() == ThreadState::RUNNING {
                old_thread.set_state(ThreadState::READY);
                RUNQUEUE.lock().push_back(old_thread);
            } else if old_thread.get_state() == ThreadState::DEAD {
                TASK_MANAGER.reap_thread(old_thread);
            }
        },
        None => this_cpu.idle_thread = Some(old_thread)
    }

    next_thread.set_state(ThreadState::RUNNING);
    this_cpu.current_thread = Some(next_thread);

    // unsafe {
    //     crate::enable_interrupts();
    // }

    unsafe {
        context_switch(cr3, old_rsp_ptr, new_rsp);
    }

}

type Task = fn() -> !;

fn task_a() -> ! {
    for _ in 0..5 {
        println!("TaskA running");
        yield_task();
    }
    super::exit_thread();
    loop {
        
    }
}

fn task_b() -> ! {
    for _ in 0..5 {
        println!("TaskB running");
        yield_task();
    }
    super::exit_thread();
    loop {
        
    }
}

#[unsafe(no_mangle)]
extern "C" fn TASK_test() {

    let taska_ptr: Task = task_a;
    let taskb_ptr: Task = task_b;

    println!("size of context {}", core::mem::size_of::<TaskContext>());
    super::spawn_thread(VirtAddr(taska_ptr as u64));
    super::spawn_thread(VirtAddr(taskb_ptr as u64));
    yield_task();
}