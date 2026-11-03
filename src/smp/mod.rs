use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::{boxed::Box, sync::Arc, vec::Vec};

use crate::{error, lock::{crit_sec::CriticalGuard, spinlock::SpinLockIrqSave}, mm::VirtAddr, smp::IpiAction::{HALT, TlbFlush}, task::Thread};

const IPI_VECTOR: u8 = 0x40;

#[repr(C)]
struct LapicIds
{
    pointer: *const u8,
    size: u32,
}

unsafe extern "C" {
    static LAPIC_IDS: LapicIds;
    
    fn get_cpu(lapic_id: u8) -> *const ();
    fn LAPIC_send_ipi(vector: u8, target: u8);
}

fn get_lapic_ids() -> &'static [u8] {
    unsafe {
        core::slice::from_raw_parts(LAPIC_IDS.pointer, LAPIC_IDS.size as usize)
    }
}

#[derive(Clone, Copy)]
pub enum IpiAction {
    TlbFlush{start: VirtAddr, pages: usize},
    HALT,
}

struct Completion {remaining: AtomicUsize}
impl Completion {
    fn im_done(&self) {
        let _ = self.remaining.fetch_sub(1, Ordering::AcqRel);
    }
}
struct IpiMessage {action: IpiAction, done: Option<*const Completion>}

#[repr(C)] // because we want some member of our struct to be accessible from C
pub struct CpuInfo {
    pub self_ptr: *mut CpuInfo,
    pub lapic_id: u32,
    pub processor_id: u32,
    pub lapic_ticks_in_1ms: u32,
    pub own_stack: VirtAddr,
    
    // rust specific members
    pub previous_thread: Option<Arc<Thread>>,
    pub current_thread: Option<Arc<Thread>>,
    pub idle_thread: Option<Arc<Thread>>,
    ipi_queue: SpinLockIrqSave<Vec<IpiMessage>>,
}

impl CpuInfo {
    const MAX_IPI_MESSAGE: usize = 0x1000 / core::mem::size_of::<IpiMessage>();

    pub fn get_current() -> &'static mut CpuInfo {
        unsafe {
            let cpu: *mut CpuInfo;
            core::arch::asm!("mov {}, gs:0", out(reg) cpu);
            &mut *cpu
        }
    }

    fn get_this(lapic_id: u8) -> &'static CpuInfo {
        unsafe {
            let cpu = get_cpu(lapic_id) as *const CpuInfo;
            & *cpu
        }
    }

    fn push_ipi_message(&self, message: IpiMessage) {
        loop {
            {
                let mut queue = self.ipi_queue.lock();
                if queue.len() < Self::MAX_IPI_MESSAGE {
                    queue.push(message);
                    break;
                }
            }

            core::hint::spin_loop();
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn CPU_init(lapic_id: u32, processor_id: u32, stack_top: u64) -> *mut CpuInfo {
    let cpu_box = Box::new(CpuInfo {
        self_ptr: core::ptr::null_mut(),
        lapic_id,
        processor_id,
        lapic_ticks_in_1ms: 0,
        own_stack: VirtAddr(stack_top),
        previous_thread: None,
        current_thread: None,
        idle_thread: None,
        ipi_queue: SpinLockIrqSave::new(Vec::with_capacity(CpuInfo::MAX_IPI_MESSAGE)),
    });

    let cpu_ptr = Box::into_raw(cpu_box);
    unsafe {
        (*cpu_ptr).self_ptr = cpu_ptr;
    }

    cpu_ptr
}
#[unsafe(no_mangle)]
pub extern "C" fn SMP_init_IPI() {
    unsafe { crate::ISR_registerNewHandler(IPI_VECTOR as i32, ipi_handler) }
}

extern "C" fn ipi_handler(_regs: *mut crate::Registers) {
    let mut ipi_queue = CpuInfo::get_current().ipi_queue.lock();
    for ipi_message in ipi_queue.iter() {
        match ipi_message.action {
            TlbFlush { start, pages } => {
                if pages > 64 {
                    unsafe {
                        core::arch::asm!(
                            "mov rax, cr3",
                            "mov cr3, rax",
                            out("rax") _,
                            options(nostack, preserves_flags)
                        );
                    }

                } else {
                    let VirtAddr(start) = start;
                    for page in 0..pages {
                        unsafe {
                            let addr = start + page as u64 * 0x1000;
                            core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
                        };
                    }
                }
            },

            HALT => {
                error!("received a halt signal HALTING");
                unsafe {crate::hcf()};
            }
        }

        if let Some(acknowledge) = ipi_message.done { 
            unsafe { & *acknowledge }.im_done();
        }
    }

    ipi_queue.clear();
    unsafe { crate::lapic_send_eoi() };
}

pub fn send_ipi_allbutself(action: IpiAction, with_completion: bool) {

    let completion = Completion {remaining: AtomicUsize::new(get_lapic_ids().len() - 1)};
    
    CriticalGuard::with(|| {
        let my_lapic = CpuInfo::get_current().lapic_id as u8;

        for lapic in get_lapic_ids().iter().filter(|id| **id != my_lapic) {
            let this_cpu = CpuInfo::get_this(*lapic);
            this_cpu.push_ipi_message(IpiMessage { action, done: Some(core::ptr::from_ref(&completion)) });
        }

        // another loop here for sending ipi because `LAPIC_send_ipi()` goes in a busy loop waiting for Delivery status to be cleared
        // and if le target cpu get an ipi with an empty ipi queue (if our ipi message was already processed) then well, its harmless
        for lapic in get_lapic_ids().iter().filter(|id| **id != my_lapic) {
            unsafe { LAPIC_send_ipi(IPI_VECTOR, *lapic) };
        }
    });

    if with_completion {
        loop {
            if completion.remaining.load(Ordering::Acquire) <= 0 { break; }
            core::hint::spin_loop();
        }
    }
}

pub fn send_broadcast_ipi(action: IpiAction, with_completion: bool) {
    let completion = Completion {remaining: AtomicUsize::new(get_lapic_ids().len())};
    
    CriticalGuard::with(|| {

        for lapic in get_lapic_ids().iter() {
            let this_cpu = CpuInfo::get_this(*lapic);
            this_cpu.push_ipi_message(IpiMessage { action, done: Some(core::ptr::from_ref(&completion)) });
        }

        for lapic in get_lapic_ids().iter() {
            unsafe { LAPIC_send_ipi(IPI_VECTOR, *lapic) };
        }
    });

    if with_completion {
        loop {
            if completion.remaining.load(Ordering::Acquire) <= 0 { break; }
            core::hint::spin_loop();
        }
    }
}