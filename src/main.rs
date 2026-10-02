#![no_std]
#![no_main]

extern crate alloc;

pub mod mm;
pub mod spinlock;
pub mod display;
pub mod task;

use core::panic::PanicInfo;

use alloc::boxed::Box;

use crate::{mm::VirtAddr, task::Thread};

/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        println!("RUST KERNEL PANIC");

        if let Some(location) = info.location() {
            println!("Location: {}:{}:{}", location.file(), location.line(), location.column());
        }

        println!("Message: {}", info.message());

        hcf();
    };
}

#[repr(C)] // because we want some member of our struct to be accessible from C
pub struct CpuInfo {
    pub self_ptr: *mut CpuInfo,
    pub lapic_id: u32,
    pub processor_id: u32,
    pub own_stack: VirtAddr,
    
    // rust specific members
    pub current_thread: Option<Box<Thread>>,
    pub idle_thread: Option<Box<Thread>>,
}

impl CpuInfo {
    fn get_current() -> &'static mut CpuInfo {
        unsafe {
            let cpu: *mut CpuInfo;
            core::arch::asm!("mov {}, gs:0", out(reg) cpu);
            &mut *cpu
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn CPU_init(lapic_id: u32, processor_id: u32, stack_top: u64) -> *mut CpuInfo {
    let cpu_box = Box::new(CpuInfo {
        self_ptr: core::ptr::null_mut(),
        lapic_id,
        processor_id,
        own_stack: VirtAddr(stack_top),
        current_thread: None,
        idle_thread: None,
    });

    let cpu_ptr = Box::into_raw(cpu_box);
    unsafe {
        (*cpu_ptr).self_ptr = cpu_ptr;
    }

    cpu_ptr
}

unsafe extern "C" {
    fn hcf() -> !;
    fn SERIAL_putc(c: u8);
    fn switch_pdbr(pdbr: u64);
    fn get_pdbr() -> u64;
    fn enable_interrupts();
    fn disable_interrupts();

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

    println!("hello from rust!, framebuffer at 0x{:x}", address as u64);

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
}