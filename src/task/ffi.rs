use crate::{mm::VirtAddr, task::{exit_thread, scheduler::yield_task, spawn_thread}};

#[unsafe(no_mangle)]
unsafe extern "C" fn TASK_spawn_thread(entry_point: *mut u8) {
    spawn_thread(VirtAddr(entry_point as u64));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn TASK_exit_thread(code: i32) {
    exit_thread(code);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn TASK_yield() {
    yield_task();
}