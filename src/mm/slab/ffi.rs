use core::sync::atomic::Ordering::Relaxed;

use crate::mm::{HHDM_OFFSET, PhyAddr, physical::PHYSICAL_MEMORY_ALLOCATOR, slab::{SLAB, SlabHeader, size_to_class_index}};

const LARGE_MAGIC: u64 = 0x1A2B_3C4D_5E6F_7081;

#[repr(C, align(16))]
struct LargeHeader {
    magic: u64,
    num_pages: usize,
    size: usize,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn kmalloc(size: usize) -> *mut u8{
    if size == 0 {
        return core::ptr::null_mut();
    }

    let max_size = size.max(16);    // it is a standard to align pointer at minimum by 16 in C
    
    if let Some(class_index) = size_to_class_index(max_size) {
        return SLAB[class_index].lock().alloc();
    }

    let header_size = core::mem::size_of::<LargeHeader>();
    let num_pages = (max_size + header_size + 4095) / 4096;
    if num_pages > 1 {todo!("time to implement multiple page allocator")}

    let hhdm = HHDM_OFFSET.load(Relaxed);
    let PhyAddr(page) = PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page().expect("kmalloc: out of memory");
    let virt_page = page + hhdm;

    let header = unsafe {
        &mut *(virt_page as *mut LargeHeader)
    };

    header.magic = LARGE_MAGIC;
    header.num_pages = num_pages;
    header.size = size;

    (virt_page + header_size as u64) as *mut u8
}


#[unsafe(no_mangle)]
unsafe extern "C" fn kfree(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }

    let page_addr = (ptr as u64) & !0xFFF;
    let slab_header = unsafe {
        &mut *(page_addr as *mut SlabHeader)
    };

    if slab_header.header_magic == super::HEADER_MAGIC {
        let class_index = size_to_class_index(slab_header.slab_class).unwrap();
        SLAB[class_index].lock().free(ptr, slab_header);
        return;
    }

    let large_header = unsafe {
        &mut *(page_addr as *mut LargeHeader)
    };

    if large_header.magic == LARGE_MAGIC {
        if large_header.num_pages > 1 {todo!("time to implement multiple page allocator")}

        let hhdm = HHDM_OFFSET.load(Relaxed);
        let phys_page = page_addr - hhdm;

        PHYSICAL_MEMORY_ALLOCATOR.lock().free_page(PhyAddr(phys_page));
        return;
    }

    panic!("kfree: pointer 0x{:x} is corrupted or double freed!", ptr as usize);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn krealloc(ptr: *mut u8, new_size: usize) -> *mut u8 {
    if ptr.is_null() {
        return unsafe {
            kmalloc(new_size)
        };
    }

    if new_size == 0 {
        unsafe {
            kfree(ptr);
        };
        return core::ptr::null_mut();
    }

    let page_addr = (ptr as u64) & !0xFFF;
    let slab_header = unsafe {
        &mut *(page_addr as *mut SlabHeader)
    };

    let old_size = if slab_header.header_magic == super::HEADER_MAGIC {
        slab_header.slab_class
    } else {
        let large_header = unsafe {
            &mut *(page_addr as *mut LargeHeader)
        };

        if large_header.magic == LARGE_MAGIC {
            large_header.size
        } else {
            panic!("krealloc: invalid pointer 0x{:x}", ptr as usize);
        }
    };

    if old_size >= new_size {
        return ptr;
    }

    unsafe {
        let new_ptr = kmalloc(new_size);
        if !new_ptr.is_null() {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, old_size.min(new_size));
        }

        kfree(ptr);
        new_ptr
    }
}