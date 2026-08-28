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

        let bitmap_length = (total_page_number + 7) / 8;

        // we need to find the bitmap address within the memory map
        let bitmap_region = mem_map
        .iter()
        .filter(|region| region.etype == super::LIMINE_MEMMAP_USABLE)
        .find(|region| region.length >= bitmap_length as u64)
        .expect("there is no way we didnt find any suitable region for the bitmap");

        let mut bitmap = Bitmap { data: (bitmap_region.base + limine_hhdm_offset) as *mut u8, length: bitmap_length };

        println!("bitmap base: 0x{:x} length {}", bitmap_region.base, bitmap_length);

        // initially we mark the whole physical address space as used
        bitmap.sliced_bitmap_mut().fill(0xFF);

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
    next: *mut FreePage
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

        // initializing the head
        let first_free = bitmap.search_free_ranges(1, total_pages).unwrap();
        list.head = Bitmap::index_to_addr(first_free) as *mut FreePage;
        unsafe {(*list.head).next = core::ptr::null_mut()};

        let mut tail= list.head;
        
        for index in first_free+1..=total_pages as usize {
            if bitmap.is_used(index) {continue;}

            unsafe {
                let new_tail = Bitmap::index_to_addr(index) as *mut FreePage;
                (*new_tail).next = core::ptr::null_mut();

                (*tail).next = new_tail;
                tail = new_tail;
            }
        }

        list
    }

    fn get(&mut self) -> *mut u8 {
        let ret = self.head;

        if !self.head.is_null() {
            unsafe {
                self.head = (*self.head).next;
            }
        }

        ret as *mut u8
    }

    fn put(&mut self, addr: *mut u8) {
        let head = addr as *mut FreePage;
        unsafe {(*head).next = self.head}

        self.head = head;

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

    fn alloc_page(&mut self) -> *mut u8 {
        if let Some(free_list) = self.free_list.as_mut() {
            if self.free_pages <= 0 {return core::ptr::null_mut();}

            // let free_list = self.free_list.as_mut().unwrap();
            let bitmap = self.bitmap.as_mut().unwrap();

            loop {
                let addr = free_list.get();
                if bitmap.is_used(Bitmap::addr_to_index(addr as u64)) {
                    // this means that we probably allocated this page using only the bitmap
                    // or somehow a used page got placed in the free list by error (I'm only a human after all)
                    continue;   // anyway retry !! but seriously I need to panic here or atleast fallback to only use the bitmap
                }

                self.used_pages += 1;
                self.free_pages -= 1;

                bitmap.set(Bitmap::addr_to_index(addr as u64)); // set this page as used
                break addr;
            }
        } else {
            let free_index = self.bitmap
            .as_ref()
            .unwrap()
            .search_free_ranges(1, self.total_pages);

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
    }

    fn free_page(&mut self, addr: *mut u8) {

        // self.free_list
        // .as_mut()
        // .unwrap()
        // .put(addr);

        if let Some(free_list) = self.free_list.as_mut() {
            free_list.put(addr);
        }
        
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
    // allocator.free_list = Some(FreeList::from_bitmap(allocator.bitmap.as_ref().unwrap(), allocator.total_pages));

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

    let page = allocator.alloc_page();
    println!("test allocation: 0x{:x}", page as u64);
    println!("allocator, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);

    allocator.free_page(page);
    println!("allocator after free, total pages {}, free pages {}, used pages {}", allocator.total_pages, allocator.free_pages, allocator.used_pages);
}