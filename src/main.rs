#![no_std]
#![no_main]

use core::panic::PanicInfo;
/// This function is called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe {
        hcf();
    };
}

pub mod mm;

unsafe extern "C" {
    fn hcf() -> !;
}

#[unsafe(no_mangle)]
pub extern "C" fn draw_framebuffer(
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
) {
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