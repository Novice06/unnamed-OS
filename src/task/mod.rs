pub mod scheduler;

use alloc::{boxed::Box, sync::Arc};

use crate::{CpuInfo, mm::{MEM_get_kernel_addresspace, PhyAddr, VirtAddr}};

unsafe extern "C" {
    fn context_switch(cr3: u64, old_rsp_ptr: *mut u64, new_rsp: u64);
}

pub struct Process {
    addr_space: PhyAddr,
}

impl Process {
    fn new_idle() -> Self {
        Process {
            addr_space: PhyAddr(MEM_get_kernel_addresspace()),
        }
    }
}

#[repr(C)]
pub struct TaskContext {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rbp: u64,
    rip: u64,
}

pub struct Thread {
    process: Arc<Process>,
    rsp: u64,
    kernel_stack: VirtAddr,
    entry_point: VirtAddr,
}

impl Thread {
    pub fn new_idle(stack: VirtAddr) -> Self {
        Self {
            process: Arc::new(Process::new_idle()),
            rsp: 0,
            kernel_stack: stack,
            entry_point: VirtAddr(0),
        }
    }

    pub fn new(entry_point: VirtAddr, rsp: u64, kernel_stack: VirtAddr) -> Self {
        let current_thread = CpuInfo::get_current().current_thread.as_ref().unwrap();
        Self { process: Arc::clone(&current_thread.process), rsp, kernel_stack, entry_point }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn TASK_init() {
    let cpu = CpuInfo::get_current();

    let idle_thread = Thread::new_idle(cpu.own_stack);

    cpu.current_thread = Some(Box::new(idle_thread));
}