use crate::{error, vfs::cpio::{self, CpioError}};


#[unsafe(no_mangle)]
unsafe extern "C" fn VFS_init() {
    super::init();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VFS_initrd(initrd: *mut u8, initrd_size: u32) {
    let root = super::ROOT_FILESYSTEM.lock().as_ref().unwrap().root();

    let cpio_archive = if !initrd.is_null() {
        unsafe {
            core::slice::from_raw_parts(initrd, initrd_size as usize)
        }
    } else {
        return;
    };

    match cpio::unpack_cpio_in(&root, cpio_archive) {
        Ok(_) => (),
        Err(err) => {
            match err {
                CpioError::Vfs(_) => error!("there was a VFS error while unpacking the cpio archive"),
                _ => error!("Invalid cpio Archive"),
            }
        }
    }
}