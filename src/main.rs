#![no_std]
#![no_main]

extern crate alloc;

pub mod smp;
pub mod mm;
pub mod lock;
pub mod display;
pub mod task;
pub mod vfs;
pub mod syscall;

use core::panic::PanicInfo;

use crate::smp::CpuInfo;

/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        disable_interrupts();
        error!("RUST KERNEL PANIC");
        error!("on CPU id: {}", CpuInfo::get_current().processor_id);

        if let Some(location) = info.location() {
            error!("Location: {}:{}:{}", location.file(), location.line(), location.column());
        }

        error!("Message: {}", info.message());

        smp::send_ipi_allbutself(smp::IpiAction::HALT, false);

        hcf();
    };
}

#[repr(C)]
pub struct TrapFrame {
    // In the reverse order they are pushed:
    pub ds: u64, // Data segment pushed by us

    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,

    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,

    pub interrupt: u64,
    pub error: u64,

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

type ISRHandler = extern "C" fn(regs: *mut TrapFrame);

unsafe extern "C" {
    fn hcf() -> !;
    fn SERIAL_putc(c: u8);
    fn switch_pdbr(pdbr: u64);
    fn get_pdbr() -> u64;
    fn get_rflags() -> u64;
    fn enable_interrupts();
    fn disable_interrupts();
    fn lapic_send_eoi();

    fn ISR_registerNewHandler(interrupt: i32, handler: ISRHandler);

    static kernel_start: core::ffi::c_uchar;
    static kernel_write_allowed_start: core::ffi::c_uchar;
    static kernel_end: core::ffi::c_uchar;
}

#[unsafe(no_mangle)]
pub extern "C" fn draw_framebuffer(
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
) {

    info!("hello from rust!, framebuffer at 0x{:x}", address as u64);

    let fb_ptr = address as *mut u32;

    for y in 0..height {
        for x in 0..width {
            let nx = x * 255 / width;
            let ny = y * 255 / height;

            let pixel = (ny << 8) | nx;

            let offset = y * (pitch / 4) + x;

            unsafe {
                core::ptr::write_volatile(
                    fb_ptr.add(offset),
                    pixel as u32,
                );
            }
        }
    }

    panic!("just panicking for no reason");
}