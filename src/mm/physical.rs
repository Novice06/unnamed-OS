use crate::{println, spinlock::SpinLock};
use super::LimineMemMapEntry;

struct Bitmap {
    data: *mut u8,
    length: u32,
}
unsafe impl Send for Bitmap {}
unsafe impl Sync for Bitmap {}

impl Bitmap {
    fn new(mem_map: &[LimineMemMapEntry], limine_hhdm_offset: u64, total_page_number: u32) -> Self {

        let length = (total_page_number + 7) / 8;

        // we need to find the bitmap address within the memory map
        let bitmap_region = mem_map
        .iter()
        .filter(|region| region.etype == super::LIMINE_MEMMAP_USABLE)
        .find(|region| region.length >= length as u64)
        .expect("there is no way we didnt find any suitable region for the bitmap");

        let mut bitmap = Bitmap { data: (bitmap_region.base + limine_hhdm_offset) as *mut u8, length };

        println!("bitmap base: 0x{:x} length {}", bitmap_region.base, length);

        // initially we mark the whole physical address space as used
        bitmap.sliced_bitmap_mut().fill(0);

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
        for addr in (bitmap_region.base..bitmap_region.base + length as u64).step_by(0x1000) {
            bitmap.set(Bitmap::addr_to_index(addr));
        }

        bitmap
    }

    fn sliced_bitmap(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.data, self.length as usize)
        }
    }

    fn sliced_bitmap_mut(&self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.data, self.length as usize)
        }
    }

    fn set(&mut self, index: usize) {
        let bitmap = self.sliced_bitmap_mut();

        let byte = index / 8;
        let bit = index % 8;

        bitmap[byte] |= 1 << bit;
    }

    fn clear(&mut self, index: usize) {
        let bitmap = self.sliced_bitmap_mut();

        let byte = index / 8;
        let bit = index % 8;

        bitmap[byte] &= !(1 << bit);
    }

    fn is_used(&self, index: usize) -> bool{
        let bitmap = self.sliced_bitmap();

        let byte = index / 8;
        let bit = index % 8;

        bitmap[byte] & (1 << bit) != 0
    }

    fn addr_to_index(addr: u64) -> usize {
        (addr / 0x1000) as usize
    }

    fn index_to_addr(index: usize) -> u64 {
        index as u64 * 0x1000
    }

    fn search_free_ranges(&self, count: usize) -> Option<usize> {
        let mut range: usize = 0;

        for index in 0..self.length as usize {
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

pub struct PhysMemAllocator {
    bitmap: Option<Bitmap>,
    total_pages: u32,
    free_pages: u32,
    used_pages: u32,
}

impl PhysMemAllocator {
    const fn new() -> Self {
        Self {
            bitmap: None,
            total_pages: 0,
            free_pages: 0,
            used_pages: 0,
        }
    }

    fn alloc_page(&mut self) -> *mut u8 {
        let free_index = self.bitmap
        .as_ref()
        .unwrap()
        .search_free_ranges(1);

        if let Some(index) = free_index {
            self.bitmap
            .as_mut()
            .unwrap()
            .set(index);

            self.used_pages += 1;
            self.free_pages -= 1;

            Bitmap::index_to_addr(index) as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }

    fn free_page(&mut self, addr: *mut u8) {
        self.bitmap
        .as_mut()
        .unwrap()
        .clear(Bitmap::addr_to_index(addr as u64));

        self.free_pages += 1;
        self.used_pages -= 1;
    }
}

pub static PHYSICAL_MEMORY_ALLOCATOR: SpinLock<PhysMemAllocator> = SpinLock::new(PhysMemAllocator::new());

pub fn init(memory_size: u32, mem_map_entries: &[LimineMemMapEntry], limine_hhdm_offset: u64) {
    let mut allocator = PHYSICAL_MEMORY_ALLOCATOR.lock();
    allocator.total_pages = (memory_size / 0x1000) as u32;
    allocator.bitmap = Some(Bitmap::new(mem_map_entries, limine_hhdm_offset,allocator.total_pages));

    for index in 0..allocator.total_pages {
        let bitmap = allocator.bitmap.as_ref().unwrap();
        if bitmap.is_used(index as usize) {
            allocator.used_pages += 1;
        } else {
            allocator.free_pages += 1;
        }
    }

    println!("allocator, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);

    let page = allocator.alloc_page();
    println!("test allocation: 0x{:x}", page as u64);
    println!("allocator, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);

    allocator.free_page(page);
    println!("allocator after free, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);
}