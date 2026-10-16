
#[unsafe(no_mangle)]
unsafe extern "C" fn VFS_init(initrd: *mut u8, initrd_size: u32) {
    let cpio_archive = if !initrd.is_null() {
        Some(unsafe {
            core::slice::from_raw_parts(initrd, initrd_size as usize)
        })
    } else {
        None
    };

    super::init(cpio_archive);
}