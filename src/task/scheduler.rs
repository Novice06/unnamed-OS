use alloc::{collections::VecDeque, sync::Arc};

use crate::{CpuInfo, ISR_registerNewHandler, enable_interrupts, lock::{crit_sec::CriticalGuard, spinlock::SpinLockIrqSave}, mm::{PhyAddr, VirtAddr}, task::{TASK_MANAGER, Thread, ThreadState::{self, READY}, context_switch}};


static RUNQUEUE: SpinLockIrqSave<VecDeque<Arc<Thread>>> = SpinLockIrqSave::new(VecDeque::new());

/// shedule a new thread only use this function to schedule a new thread
pub fn schedule_thread(thread: Arc<Thread>) -> Result<(), Arc<Thread>> {
    if thread.test_and_set_state(ThreadState::NEW, ThreadState::READY) != ThreadState::NEW {
        return Err(thread);
    }

    RUNQUEUE.lock().push_back(thread);
    Ok(())
}
/// schedule a thread that wasnt already running or ready(meaning in the RUNQUEUE)
pub fn wake_thread(thread: Arc<Thread>) {
    loop {
        match thread.get_state() {
            ThreadState::WAITING => {
                if thread.test_and_set_state(ThreadState::WAITING, ThreadState::READY) != ThreadState::WAITING {
                    continue;
                }

                RUNQUEUE.lock().push_back(thread);
                return;
            },
            ThreadState::SLEEPING => {
                if thread.test_and_set_state(ThreadState::SLEEPING, ThreadState::RUNNING) != ThreadState::SLEEPING {
                    continue;
                }

                return;
            },
            _ => return,
        }
    }
}

pub fn yield_task() {

    let _guard = CriticalGuard::enter_critical_section();   // very important

    let this_cpu = CpuInfo::get_current();

    let next_thread = if let Some(thread) = RUNQUEUE.lock().pop_front() {
        thread
    } else if this_cpu.current_thread.as_ref().unwrap().get_state() != ThreadState::RUNNING {
        this_cpu.idle_thread.as_ref().cloned().expect("no idle thread found")
    }
    else {
        return;
    };

    let previous_thread = this_cpu.current_thread.take().unwrap();

    let PhyAddr(cr3) = next_thread.process.cr3;
    let new_rsp = unsafe {
        *next_thread.rsp.get()
    };
    let old_rsp_ptr = previous_thread.rsp.get();

    this_cpu.previous_thread = Some(previous_thread);

    next_thread.set_state(ThreadState::RUNNING);
    this_cpu.current_thread = Some(next_thread);

    unsafe {
        context_switch(cr3, old_rsp_ptr, new_rsp);
    }

    finish_context_switch();
}

fn finish_context_switch() {
    let this_cpu = CpuInfo::get_current();
    let previous_thread = this_cpu.previous_thread.take().expect("impossible !! we're finishing a context switch without a previous task?");


    // if its not the idle thread
    if !Arc::ptr_eq(&previous_thread, this_cpu.idle_thread.as_ref().unwrap()) {

        match previous_thread.get_state() {
            ThreadState::RUNNING => {
                previous_thread.set_state(READY);
                RUNQUEUE.lock().push_back(previous_thread);
            },

            ThreadState::DEAD => TASK_MANAGER.reap_thread(previous_thread),

            ThreadState::SLEEPING => {
                if previous_thread.test_and_set_state(ThreadState::SLEEPING, ThreadState::WAITING) != ThreadState::SLEEPING {
                    previous_thread.set_state(READY);
                    RUNQUEUE.lock().push_back(previous_thread);
                }
            },

            _ => (),
        }
    }
}

pub fn thread_start() -> ! {
    CriticalGuard::with(finish_context_switch);

    unsafe { enable_interrupts(); }

    let VirtAddr(entry_point_ptr) = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().entry_point);
    let entry_point: fn() = unsafe {
        core::mem::transmute(entry_point_ptr)
    };

    entry_point();

    super::exit_thread(-1);
}

extern "C" fn preempt(_regs: *mut crate::Registers) {
    unsafe { crate::lapic_send_eoi() };
    yield_task();
}

#[unsafe(no_mangle)]
extern "C" fn TASK_init_scheduler() {
    unsafe { ISR_registerNewHandler(0x20, preempt) }
}