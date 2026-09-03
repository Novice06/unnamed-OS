use core::sync::atomic::{AtomicU64, Ordering};

use crate::{kernel_end, kernel_start, kernel_write_allowed_start, mm::LimineExecutableAddr, println};
use super::{LimineMemMapEntry};

pub const PAGE_PRESENT: u8              = 1 << 0;
pub const PAGE_WRITABLE: u8             = 1 << 1;
pub const PAGE_USER: u8                 = 1 << 2;
pub const PAGE_WRITE_THROUGH_CACHE: u8  = 1 << 3;
pub const PAGE_DISABLE_CACHE: u8        = 1 << 4;
pub const PAGE_GLOBAL: u8               = 1 << 5;
pub const PAGE_NO_EXECUTE: u8           = 1 << 6;

struct PageEntry<'a> {
    entries: &'a mut [u64]
}

impl PageEntry<'_> {
    fn from_raw(raw: *mut u64) -> Self {
        let entries=  unsafe {
            core::slice::from_raw_parts_mut(raw, 512)
        };

        Self { 
            entries
        }
    }

    fn from_zeroed(raw: *mut u64) -> Self {
        let entry = Self::from_raw(raw);
        entry.entries.fill(0);

        entry
    }

    fn from_previous_level(&mut self, index: usize, force_map: bool, flags: u8) -> Option<Self> {

        let hhdm = HHDM_OFFSET.load(Ordering::Relaxed);
        let mut page_was_present = false;

        let addr = if self.is_present(index) {
            page_was_present = true;
            self.get(index) & 0x000FFFFFFFFFF000
        } else if force_map {
            let addr = super::physical::PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page();
            self.set(index, addr as u64, flags);

            addr as u64
        } else {
            return None;
        };

        let addr = addr + hhdm;
        Some(
            if page_was_present {
                Self::from_raw(addr as *mut u64)
            }else {
                Self::from_zeroed(addr as *mut u64)
            }
        )
    }

    fn get(&self, index: usize) -> u64 {
        self.entries[index]
    }

    fn set(&mut self, index: usize, phys_addr: u64, flags: u8) {
        self.entries[index] = Self::flags_to_attr(flags) | (phys_addr & 0x000FFFFFFFFFF000);
    }

    fn flags_to_attr(flags: u8) -> u64 {
        let mut attribute: u64 = 0;
        if (flags & PAGE_PRESENT)               != 0 {attribute |= 1 << 0}
        if (flags & PAGE_WRITABLE)              != 0 {attribute |= 1 << 1}
        if (flags & PAGE_USER)                  != 0 {attribute |= 1 << 2}
        if (flags & PAGE_WRITE_THROUGH_CACHE)   != 0 {attribute |= 1 << 3}
        if (flags & PAGE_DISABLE_CACHE)         != 0 {attribute |= 1 << 4}
        if (flags & PAGE_GLOBAL)                != 0 {attribute |= 1 << 8}
        if (flags & PAGE_NO_EXECUTE)            != 0 {attribute |= 1 << 63}

        attribute
    }

    fn is_present(&self, index: usize) -> bool {
        (self.entries[index] & 1) != 0
    }
}

static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);

// uint64_t pml4 = (virt >> 39) & 0x1FF;
// uint64_t pml3 = (virt >> 30) & 0x1FF;
// uint64_t pml2 = (virt >> 21) & 0x1FF;
// uint64_t pml1 = (virt >> 12) & 0x1FF;
// uint64_t offset = virt & 0xFFF;


pub fn map_pages(pml4_addr: *mut u64, virt: u64, phys: u64, num_pages: u64, flags: u8)
{
    let mut pml4 = PageEntry::from_raw(pml4_addr);

    for i in 0..num_pages {
        let addr = virt + 0x1000 * i;
        let mut pml3 = pml4.from_previous_level(((addr >> 39) & 0x1FF) as usize, true, flags).expect("cant allocate pml4");
        let mut pml2 = pml3.from_previous_level(((addr >> 30) & 0x1FF) as usize, true, flags).expect("cant allocate pml2");
        let mut pml1 = pml2.from_previous_level(((addr >> 21) & 0x1FF) as usize, true, flags).expect("cant allocate pml1");

        pml1.set(((addr >> 12) & 0x1FF) as usize, phys + 0x1000 * i, flags);
    }
}

pub fn alloc_pages(pml4_addr: *mut u64, virt: u64, num_pages: u64, flags: u8)
{
    for i in 0..num_pages {
        let frame = super::physical::PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page() as u64;
        map_pages(
            pml4_addr,
            virt + 0x1000 * i,
            frame,
            1,
            flags
        );
    }
}

pub fn init(limine_hhdm_offset: u64, mem_map_entries: &[LimineMemMapEntry], executable_addr: LimineExecutableAddr) -> u64 {
    HHDM_OFFSET.store(limine_hhdm_offset, Ordering::Relaxed); // use the same hhdm as limine

    let hhdm = HHDM_OFFSET.load(Ordering::Relaxed);

    let pml4_addr = super::physical::PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page() as u64 + hhdm;

    // map section to hhdm
    for entry in mem_map_entries {
        if 
        [
            super::LIMINE_MEMMAP_USABLE, 
            super::LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE, 
            super::LIMINE_MEMMAP_EXECUTABLE_AND_MODULES, 
            super::LIMINE_MEMMAP_FRAMEBUFFER
        ].contains(&entry.etype) {

            let flags = if entry.etype == super::LIMINE_MEMMAP_FRAMEBUFFER {
                PAGE_PRESENT | PAGE_WRITABLE | PAGE_DISABLE_CACHE
            } else {
                PAGE_PRESENT | PAGE_WRITABLE
            };

            map_pages(
                pml4_addr as *mut u64, 
                entry.base + hhdm, 
                entry.base, 
                entry.length / 0x1000, 
                flags
            );
        }
    }

    println!("mapping hhdm was a success!");

    // map kernel
    let length_non_writable = unsafe {
        println!("start {:x} end {:x}", core::ptr::from_ref(&kernel_start) as u64, core::ptr::from_ref(&kernel_write_allowed_start) as u64);
        (core::ptr::from_ref(&kernel_write_allowed_start) as u64 - core::ptr::from_ref(&kernel_start) as u64 + 0xFFF) / 0x1000
    };

    map_pages(
        pml4_addr as *mut u64,
        executable_addr.virtual_base, 
        executable_addr.physical_base, 
        length_non_writable,
        PAGE_PRESENT | PAGE_GLOBAL
    );

    let length_writable = unsafe {
        (core::ptr::from_ref(&kernel_end) as u64 - core::ptr::from_ref(&kernel_write_allowed_start) as u64 + 0xFFF) / 0x1000
    };
    map_pages(
        pml4_addr as *mut u64,
        executable_addr.virtual_base + (length_non_writable * 0x1000), 
        executable_addr.physical_base + (length_non_writable * 0x1000), 
        length_writable,
        PAGE_PRESENT | PAGE_WRITABLE | PAGE_GLOBAL
    );

    // map a new stack for the kernel
    let kernel_size = length_non_writable + length_writable;
    let kernel_virtual_end = executable_addr.virtual_base + (kernel_size * 0x1000);
    alloc_pages(
        pml4_addr as *mut u64,
        kernel_virtual_end,
        8, // 32 kb
        PAGE_PRESENT | PAGE_WRITABLE | PAGE_GLOBAL
    );

    kernel_virtual_end + 8 * 0x1000 // return the stack top

}