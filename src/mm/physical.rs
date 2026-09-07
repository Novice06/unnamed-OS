use core::{panic, sync::atomic::Ordering};
use crate::{mm::PhyAddr, println, spinlock::SpinLock};
use super::LimineMemMapEntry;

struct Bitmap {
    data: &'static mut [u8]
}
unsafe impl Send for Bitmap {}
unsafe impl Sync for Bitmap {}

impl Bitmap {
    fn from_memory_map(mem_map: &[LimineMemMapEntry], limine_hhdm_offset: u64, total_page_number: u32) -> Self {

        let bitmap_length = (total_page_number + 7) / 8;

        // we need to find the bitmap address within the memory map
        let bitmap_region = mem_map
        .iter()
        .filter(|region| region.etype == super::LIMINE_MEMMAP_USABLE)
        .find(|region| region.length >= bitmap_length as u64)
        .expect("there is no way we didnt find any suitable region for the bitmap");

        let mut bitmap = Bitmap { 
            data: unsafe {
                core::slice::from_raw_parts_mut((bitmap_region.base + limine_hhdm_offset) as *mut u8, bitmap_length as usize)
            }
        };
        

        println!("bitmap base: 0x{:x} length {}", bitmap_region.base, bitmap_length);

        // initially we mark the whole physical address space as used
        bitmap.data.fill(0xFF);

        // then first we map free regions
        let available_regions = mem_map
        .iter()
        .filter(|region| region.etype == super::LIMINE_MEMMAP_USABLE);
        for region in available_regions {
            for addr in (region.base..region.base + region.length).step_by(0x1000) {
                bitmap.clear(Bitmap::addr_to_index(addr));
            }
        }

        // then we map reserved regions, that way we handle perfectly overlaping blocks
        let used_regions = mem_map
        .iter()
        .filter(|region| region.etype != super::LIMINE_MEMMAP_USABLE);
        for region in used_regions {
            for addr in (region.base..region.base + region.length).step_by(0x1000) {
                
                let index = Bitmap::addr_to_index(addr);

                if index > total_page_number as usize {break;} // some address are just MMIO and not part of the physical memory
                
                bitmap.set(index);
            }
        }

        // after all that we need to map the bitmap region as used
        for addr in (bitmap_region.base..bitmap_region.base + bitmap_length as u64).step_by(0x1000) {
            bitmap.set(Bitmap::addr_to_index(addr));
        }

        bitmap
    }

    fn set(&mut self, index: usize) {

        let byte = index / 8;
        let bit = index % 8;

        self.data[byte] |= 1 << bit;
    }

    fn clear(&mut self, index: usize) {

        let byte = index / 8;
        let bit = index % 8;

        self.data[byte] &= !(1 << bit);
    }

    fn is_used(&self, index: usize) -> bool{

        let byte = index / 8;
        let bit = index % 8;

        self.data[byte] & (1 << bit) != 0
    }

    fn addr_to_index(addr: u64) -> usize {
        (addr / 0x1000) as usize
    }

    fn index_to_addr(index: usize) -> u64 {
        index as u64 * 0x1000
    }

    fn search_free_ranges(&self, count: usize, total_page_number: u32) -> Option<usize> {
        let mut range: usize = 0;

        for index in 0..total_page_number as usize {
            if !self.is_used(index) {
                range += 1;
            } else {
                range = 0;
            }

            if range >= count {
                return Some(index + 1 - range);
            }
        }

        None
    }

}

struct FreePage {
    next: *mut FreePage,
    back: *mut FreePage
}

struct FreeList {
    head: *mut FreePage
}

unsafe impl Send for FreeList {}
unsafe impl Sync for FreeList {}

impl FreeList {
    fn from_bitmap(bitmap: &Bitmap, total_pages: u32) -> Self {
        let mut list = FreeList {
            head: core::ptr::null_mut()
        };

        let hhdm = crate::mm::HHDM_OFFSET.load(Ordering::Relaxed);

        // initializing the head
        let first_free = bitmap.search_free_ranges(1, total_pages).unwrap();
        list.head = (Bitmap::index_to_addr(first_free) + hhdm) as *mut FreePage;
        unsafe {
            (*list.head).back = core::ptr::null_mut();
            (*list.head).next = core::ptr::null_mut();
        };

        let mut tail= list.head;
        
        for index in first_free+1..=total_pages as usize {
            if bitmap.is_used(index) {continue;}

            unsafe {
                let new_tail = (Bitmap::index_to_addr(index) + hhdm) as *mut FreePage;
                (*new_tail).next = core::ptr::null_mut();
                (*new_tail).back = (*tail).back;

                (*tail).next = new_tail;
                tail = new_tail;
            }
        }

        list
    }

    fn get(&mut self) -> Option<PhyAddr> {

        if self.head.is_null() {
            return None;
        }

        let hhdm = crate::mm::HHDM_OFFSET.load(Ordering::Relaxed);
        let ret = self.head;

        unsafe {
            self.head = (*self.head).next;
            (*self.head).back = core::ptr::null_mut();
        }

        Some(PhyAddr(ret as u64 - hhdm))
    }

    fn put(&mut self, PhyAddr(addr): PhyAddr) {
        let hhdm = crate::mm::HHDM_OFFSET.load(Ordering::Relaxed);

        let new_head = (addr + hhdm) as *mut FreePage;

        unsafe {
            (*new_head).next = self.head;
            (*new_head).back = core::ptr::null_mut();
        }

        self.head = new_head;

    }
}

pub struct PhysMemAllocator {
    bitmap: Option<Bitmap>,
    free_list: Option<FreeList>,
    total_pages: u32,
    free_pages: u32,
    used_pages: u32,
}

impl PhysMemAllocator {
    const fn new() -> Self {
        Self {
            bitmap: None,
            free_list: None,
            total_pages: 0,
            free_pages: 0,
            used_pages: 0,
        }
    }

    pub fn alloc_page(&mut self) -> Option<PhyAddr> {

        if self.free_pages <= 0 {return None;}
        let bitmap = self.bitmap.as_mut().unwrap();

        if let Some(mut free_list) = self.free_list.take() {

            let addr = free_list.get()?;
            if !bitmap.is_used(Bitmap::addr_to_index(addr.0)) {
                self.used_pages += 1;
                self.free_pages -= 1;

                bitmap.set(Bitmap::addr_to_index(addr.0)); // set this page as used
                self.free_list = Some(free_list);   // put the list back before returning.

                return Some(addr);
            }

            // this means that we probably allocated this page using only the bitmap and didnt correct the list
            // or somehow a used page got placed in the free list by error (I'm only a human after all)
            // anyway fallback to only use the bitmap because its corrupted now
            // we could try to to rebuild the free list but that would probably take too long if we have a lot of memory
            self.free_list = None;
        } 

        // fall back to bitmap

        let free_index = bitmap.search_free_ranges(1, self.total_pages);

        if let Some(index) = free_index {
            bitmap.set(index);

            self.used_pages += 1;
            self.free_pages -= 1;

            Some(PhyAddr(Bitmap::index_to_addr(index)))
        } else {
            None
        }
        
    }

    pub fn free_page(&mut self, addr: PhyAddr) {
        let bitmap = self.bitmap.as_mut().unwrap();

        if !bitmap.is_used(Bitmap::addr_to_index(addr.0)) {
            panic!("tried to free an unused page!")
        }

        if let Some(free_list) = self.free_list.as_mut() {
            free_list.put(addr);
        }
        
        bitmap.clear(Bitmap::addr_to_index(addr.0));

        self.free_pages += 1;
        self.used_pages -= 1;
    }
}

pub static PHYSICAL_MEMORY_ALLOCATOR: SpinLock<PhysMemAllocator> = SpinLock::new(PhysMemAllocator::new());

pub fn init(memory_size: u32, mem_map_entries: &[LimineMemMapEntry], limine_hhdm_offset: u64) {
    let mut allocator = PHYSICAL_MEMORY_ALLOCATOR.lock();
    allocator.total_pages = (memory_size / 0x1000) as u32;
    allocator.bitmap = Some(Bitmap::from_memory_map(mem_map_entries, limine_hhdm_offset,allocator.total_pages));
    allocator.free_list = Some(FreeList::from_bitmap(allocator.bitmap.as_ref().unwrap(), allocator.total_pages));

    for index in 0..allocator.total_pages {
        let bitmap = allocator.bitmap.as_ref().unwrap();
        if bitmap.is_used(index as usize) {
            allocator.used_pages += 1;
        } else {
            allocator.free_pages += 1;
        }
    }

    println!("allocator, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);

    let PhyAddr(page1) = allocator.alloc_page().expect("out of memory");
    println!("test allocation: 0x{:x}", page1);
    println!("allocator, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);

    let PhyAddr(page2) = allocator.alloc_page().expect("out of memory");
    println!("test allocation: 0x{:x}", page2 as u64);
    println!("allocator, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);

    allocator.free_page(PhyAddr(page2));
    println!("allocator after free, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);

    let PhyAddr(page2) = allocator.alloc_page().expect("out of memory");
    println!("test re-allocation: 0x{:x}", page2 as u64);
    println!("allocator, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);

    allocator.free_page(PhyAddr(page2));
    allocator.free_page(PhyAddr(page1));
}