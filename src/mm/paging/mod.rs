use core::{panic, sync::atomic::{Ordering::{self, Relaxed}}};
use alloc::vec::Vec;

use crate::{kernel_end, kernel_start, kernel_write_allowed_start, lock::spinlock::SpinLockIrqSave, mm::{HHDM_OFFSET, LimineExecutableAddr, PhyAddr, VirtAddr, physical}, smp};
use super::{LimineMemMapEntry};

pub mod vmalloc;

pub const PAGE_PRESENT: u8              = 1 << 0;
pub const PAGE_WRITABLE: u8             = 1 << 1;
pub const PAGE_USER: u8                 = 1 << 2;
pub const PAGE_WRITE_THROUGH_CACHE: u8  = 1 << 3;
pub const PAGE_DISABLE_CACHE: u8        = 1 << 4;
pub const PAGE_GLOBAL: u8               = 1 << 5;
pub const PAGE_NO_EXECUTE: u8           = 1 << 6;

pub const PAGE_DEFAULT: u8              = PAGE_PRESENT | PAGE_WRITABLE;

static PAGING_LOCK: SpinLockIrqSave<()> = SpinLockIrqSave::new(());

struct PageEntry {
    entries: *mut u64
}

impl PageEntry {
    fn from(VirtAddr(addr): VirtAddr) -> Self {

        Self { 
            entries: addr as *mut u64
        }
    }

    fn from_zeroed(addr: VirtAddr) -> Self {
        let entry = Self::from(addr);
        unsafe {entry.entries.write_bytes(0, 512)};

        entry
    }

    fn from_previous_level(&mut self, index: usize, force_map: bool) -> Option<Self> {

        let hhdm = crate::mm::HHDM_OFFSET.load(Ordering::Relaxed);
        let mut page_was_present = false;

        let addr = if self.is_present(index) {
            page_was_present = true;
            self.get(index) & 0x000FFFFFFFFFF000
        } else if force_map {
            let PhyAddr(addr) = super::physical::PHYSICAL_MEMORY_ALLOCATOR
            .lock()
            .alloc_page()?;

            self.set(index, addr, PAGE_PRESENT | PAGE_USER | PAGE_WRITABLE);

            addr
        } else {
            return None;
        };

        let addr = VirtAddr(addr + hhdm);
        Some(
            if page_was_present {
                Self::from(addr)
            }else {
                Self::from_zeroed(addr)
            }
        )
    }

    fn get(&self, index: usize) -> u64 {
        if index >= 512 {panic!("index out of bound !")}

        unsafe {*self.entries.add(index)}
    }

    fn set(&mut self, index: usize, phys_addr: u64, flags: u8) {
        if index >= 512 {panic!("index out of bound !")}

        unsafe {
            core::ptr::write_volatile(self.entries.add(index), Self::flags_to_attr(flags) | (phys_addr & 0x000FFFFFFFFFF000));
        }
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
        (self.get(index) & 1) != 0
    }

    fn is_mapped(pml4_addr: VirtAddr, VirtAddr(addr): VirtAddr) -> bool {
        let mut pml4 = PageEntry::from(pml4_addr);

        let Some(mut pml3) = pml4.from_previous_level(((addr >> 39) & 0x1FF) as usize, false) else {
            return false;
        };
        let Some(mut pml2) = pml3.from_previous_level(((addr >> 30) & 0x1FF) as usize, false) else {
            return false;
        };
        let Some(pml1) = pml2.from_previous_level(((addr >> 21) & 0x1FF) as usize, false) else {
            return false;
        };

        pml1.is_present(((addr >> 12) & 0x1FF) as usize)
    }

    fn get_physical(&self, index: usize) -> Option<PhyAddr> {
        if !self.is_present(index) {
            return None;
        }
        let entry = self.get(index);
        Some(PhyAddr(entry & 0x000FFFFFFFFFF000))
    }

    fn clear_entry(&mut self, index: usize) {
        if index < 512 {
            unsafe {
                core::ptr::write_volatile(self.entries.add(index), 0);
            }
        }
    }

    fn is_empty(&self) -> bool {
        for i in 0..512 {
            if self.is_present(i) {
                return false;
            }
        }
        true
    }

    fn destroy(self, parent: &mut Self, idx: usize) -> Result<(), Self> {

        if !self.is_empty() {
            return Err(self);
        }

        let self_addr = parent.get_physical(idx).unwrap();
        physical::PHYSICAL_MEMORY_ALLOCATOR.lock().free_page(self_addr);
        parent.clear_entry(idx);

        Ok(())
    }
}

// uint64_t pml4 = (virt >> 39) & 0x1FF;
// uint64_t pml3 = (virt >> 30) & 0x1FF;
// uint64_t pml2 = (virt >> 21) & 0x1FF;
// uint64_t pml1 = (virt >> 12) & 0x1FF;
// uint64_t offset = virt & 0xFFF;


fn map_pages(pml4_addr: VirtAddr, VirtAddr(virt): VirtAddr, PhyAddr(phys): PhyAddr, num_pages: u64, flags: u8)
{
    // assume we posess the lock !

    let mut pml4 = PageEntry::from(pml4_addr);

    for i in 0..num_pages {
        let addr = virt + 0x1000 * i;
        let mut pml3 = pml4.from_previous_level(((addr >> 39) & 0x1FF) as usize, true).expect("cant allocate pml4");
        let mut pml2 = pml3.from_previous_level(((addr >> 30) & 0x1FF) as usize, true).expect("cant allocate pml2");
        let mut pml1 = pml2.from_previous_level(((addr >> 21) & 0x1FF) as usize, true).expect("cant allocate pml1");

        pml1.set(((addr >> 12) & 0x1FF) as usize, phys + 0x1000 * i, flags);
    }
}

fn unmap_pages(pml4_addr: VirtAddr, VirtAddr(virt): VirtAddr, num_pages: u64) -> Vec<PhyAddr> {
    
    // ironically this function need a heap since its uses a vector to collect frames
    // therefore one should use this function for a very specific reason
    // TODO: impl a similar function that directly deallocate physical frame

    // we suppose the caller has the lock

    let mut pml4 = PageEntry::from(pml4_addr);
    let mut frames = Vec::new();

    for i in 0..num_pages {
        let addr = virt + 0x1000 * i;
        
        let pml4_idx = ((addr >> 39) & 0x1FF) as usize;
        let pml3_idx = ((addr >> 30) & 0x1FF) as usize;
        let pml2_idx = ((addr >> 21) & 0x1FF) as usize;
        let pml1_idx = ((addr >> 12) & 0x1FF) as usize;

        let Some(mut pml3) = pml4.from_previous_level(pml4_idx, false) else { continue; };
        let Some(mut pml2) = pml3.from_previous_level(pml3_idx, false) else { continue; };
        let Some(mut pml1) = pml2.from_previous_level(pml2_idx, false) else { continue; };

        if let Some(phys) = pml1.get_physical(pml1_idx) {
            frames.push(phys);
            pml1.clear_entry(pml1_idx);
        }

        // ohh mygod this gonna take forever !!
        // TODO: find a way to destroy levels in batches
        let _ = pml1.destroy(&mut pml2, pml2_idx);
        let _ = pml2.destroy(&mut pml3, pml3_idx);
    }

    frames
}

pub fn map_mmio(PhyAddr(phys): PhyAddr, num_pages: u64) -> VirtAddr{
    let _guard = PAGING_LOCK.lock();

    let hhdm = HHDM_OFFSET.load(Relaxed);
    let pml4_addr = unsafe {
        crate::get_pdbr()
    };
    let pml4_virt = VirtAddr(pml4_addr + hhdm);

    map_pages(
        pml4_virt, 
        VirtAddr(phys + hhdm), 
        PhyAddr(phys), 
        num_pages,
        PAGE_PRESENT | PAGE_WRITABLE | PAGE_DISABLE_CACHE | PAGE_NO_EXECUTE
    );

    VirtAddr(phys + hhdm)
}

pub fn alloc_pages(pml4_addr: VirtAddr, VirtAddr(virt): VirtAddr, num_pages: u64, flags: u8)
{
    let _guard = PAGING_LOCK.lock();

    for i in 0..num_pages {
        let current_virt = VirtAddr(virt + 0x1000 * i);

        if PageEntry::is_mapped(pml4_addr, current_virt) {continue;}

        let frame = super::physical::PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page().expect("out of memory");
        map_pages(
            pml4_addr,
            current_virt,
            frame,
            1,
            flags
        );
    }
}

pub fn dealloc_pages(pml4_addr: VirtAddr, virt: VirtAddr, num_pages: u64) {
    
    let frames = {
        let _guard = PAGING_LOCK.lock();
        unmap_pages(pml4_addr, virt, num_pages)
    };

    // sending an ipi to invalidate tlb including my self to prevent a case where before sending the ipi
    // we get preempted and another cpu send the ipi and then again we get preempted (for some reason idk)
    // so the cpu that sent the ipi never gets to tlb flush !
    crate::smp::send_broadcast_ipi(smp::IpiAction::TlbFlush { start: virt, pages: num_pages as usize }, true);

    let mut pmm = super::physical::PHYSICAL_MEMORY_ALLOCATOR.lock();
    for frame in frames {
        pmm.free_page(frame);
    }
}

pub fn alloc_pages_in_pdbr(addr: VirtAddr, num_pages: u64, flags: u8) {
    let pml4_addr = unsafe { VirtAddr(crate::get_pdbr() + HHDM_OFFSET.load(Relaxed)) };

    alloc_pages(pml4_addr, addr, num_pages, flags);
}

pub fn dealloc_pages_in_pdbr(virt: VirtAddr, num_pages: u64) {
    let pml4_addr = unsafe { VirtAddr(crate::get_pdbr() + HHDM_OFFSET.load(Relaxed)) };

    dealloc_pages(pml4_addr, virt, num_pages);
}

pub fn init(mem_map_entries: &[LimineMemMapEntry], executable_addr: LimineExecutableAddr) {

    let hhdm = crate::mm::HHDM_OFFSET.load(Ordering::Relaxed);

    let pml4_addr_phys = super::physical::PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page().expect("out of memory");
    let pml4_addr_virt = VirtAddr(pml4_addr_phys.0 + hhdm);

    // map section to hhdm
    for entry in mem_map_entries {
        if 
        [
            super::LIMINE_MEMMAP_USABLE, 
            super::LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE, 
            super::LIMINE_MEMMAP_EXECUTABLE_AND_MODULES, 
            super::LIMINE_MEMMAP_FRAMEBUFFER,
            super::_LIMINE_MEMMAP_ACPI_RECLAIMABLE,
        ].contains(&entry.etype) {

            let flags = if entry.etype == super::LIMINE_MEMMAP_FRAMEBUFFER {
                PAGE_PRESENT | PAGE_WRITABLE | PAGE_DISABLE_CACHE
            } else {
                PAGE_PRESENT | PAGE_WRITABLE
            };

            map_pages(
                pml4_addr_virt,
                VirtAddr(entry.base + hhdm), 
                PhyAddr(entry.base), 
                entry.length / 0x1000, 
                flags
            );
        }
    }

    // map kernel
    let length_non_writable = unsafe {
        (core::ptr::from_ref(&kernel_write_allowed_start) as u64 - core::ptr::from_ref(&kernel_start) as u64 + 0xFFF) / 0x1000
    };

    map_pages(
        pml4_addr_virt,
        VirtAddr(executable_addr.virtual_base), 
        PhyAddr(executable_addr.physical_base), 
        length_non_writable,
        PAGE_PRESENT | PAGE_GLOBAL
    );

    let length_writable = unsafe {
        (core::ptr::from_ref(&kernel_end) as u64 - core::ptr::from_ref(&kernel_write_allowed_start) as u64 + 0xFFF) / 0x1000
    };
    map_pages(
        pml4_addr_virt,
        VirtAddr(executable_addr.virtual_base + (length_non_writable * 0x1000)), 
        PhyAddr(executable_addr.physical_base + (length_non_writable * 0x1000)), 
        length_writable,
        PAGE_PRESENT | PAGE_WRITABLE | PAGE_GLOBAL
    );

    // giving 512 Gio of virt memory to vmalloc
    const VMALLOC_START: u64 = 0xFFFF_C000_0000_0000;
    const VMALLOC_END: u64 = 0xFFFF_C080_0000_0000;

    //map 3rd level for now its just 1 level which will cover the entire 512Gio addr space
    PageEntry::from(pml4_addr_virt).from_previous_level(384, true).unwrap();

    vmalloc::init(VirtAddr(VMALLOC_START), VirtAddr(VMALLOC_END));

    super::KERNEL_ADDR_SPACE.store(pml4_addr_phys.0, Relaxed);
    unsafe { crate::switch_pdbr(pml4_addr_phys.0) };

}

