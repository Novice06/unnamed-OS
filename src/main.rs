#![no_std]
#![no_main]

pub mod mm;
pub mod spinlock;
pub mod display;

use core::panic::PanicInfo;
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

unsafe extern "C" {
    fn hcf() -> !;
    fn SERIAL_putc(c: u8);

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