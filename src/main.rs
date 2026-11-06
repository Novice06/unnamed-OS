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

#[repr(C, packed)]
struct Registers {
    // in the reverse order they are pushed:
    ds: u64,                                      // data segment pushed by us
    rdi: u64, rsi: u64, rbp: u64, rbx: u64, rdx: u64, rcx: u64, rax: u64, // pusha
    interrupt: u64, error: u64,                   // we push interrupt, error is pushed automatically (or our dummy)
    rip: u64, cs: u64, rflags: u64, rsp: u64, ss: u64, // pushed automatically by CPU
}

type ISRHandler = extern "C" fn(regs: *mut Registers);

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