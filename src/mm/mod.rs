struct Bitmap {
    data: *mut u8,
    length: u32,
}
unsafe impl Send for Bitmap {}
unsafe impl Sync for Bitmap {}

impl Bitmap {
    fn new(mem_map: &mut [LimineMemMapEntry], total_page_number: u32) -> Self {

        let length = total_page_number / 8;

        // we need to find the bitmap address within the memory map
        let bitmap_region = mem_map
        .iter()
        .filter(|region| region.etype == 0)
        .find(|region| region.length >= length as u64)
        .expect("there is no way we didnt find any suitable region for the bitmap");

        let mut bitmap = Bitmap { data: bitmap_region.base as *mut u8, length };

        // first we map free regions
        let available_regions = mem_map
        .iter()
        .filter(|region| region.etype == 0);
        for region in available_regions {
            for addr in (region.base..region.base + region.length).step_by(0x1000) {
                bitmap.clear(Bitmap::addr_to_index(addr));
            }
        }

        // then we map reserved regions, that way we handle perfectly overlaping blocks
        let used_regions = mem_map
        .iter()
        .filter(|region| region.etype != 0);
        for region in used_regions {
            for addr in (region.base..region.base + region.length).step_by(0x1000) {
                bitmap.set(Bitmap::addr_to_index(addr));
            }
        }

        // after all that we need to map the bitmap region as used
        for addr in (bitmap_region.base..bitmap_region.base + bitmap_region.length).step_by(0x1000) {
            bitmap.set(Bitmap::addr_to_index(addr));
        }

        bitmap
    }

    fn set(&mut self, index: usize) {
        let bitmap = unsafe {
            core::slice::from_raw_parts_mut(self.data, self.length as usize)
        };

        let byte = index / 8;
        let bit = index % 8;

        bitmap[byte] |= 1 << bit;
    }

    fn clear(&mut self, index: usize) {
        let bitmap = unsafe {
            core::slice::from_raw_parts_mut(self.data, self.length as usize)
        };

        let byte = index / 8;
        let bit = index % 8;

        bitmap[byte] &= !(1 << bit);
    }

    fn get(&self, index: usize) -> bool{
        let bitmap = unsafe {
            core::slice::from_raw_parts(self.data, self.length as usize)
        };

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
            if self.get(index) {
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
            free_pages: 0,
            used_pages: 0,
        }
    }
}

pub static mut PHYSICAL_MEMORY_ALLOCATOR: PhysMemAllocator = PhysMemAllocator::new();

#[repr(C)]
pub struct LimineMemMapEntry {
    base: u64,
    length: u64,
    etype: u64,
}

#[repr(C)]
pub struct LimineMemMap {
    revision: u64,
    count: u64,
    entries: *mut LimineMemMapEntry,
}

#[unsafe(no_mangle)]
pub extern "C" fn PHYSMEM_init(
    mem_map: LimineMemMap,
    limine_hhdm_offset: u64
) {

    let mem_map_entries: &mut [LimineMemMapEntry] = unsafe {
        core::slice::from_raw_parts_mut(mem_map.entries, mem_map.count as usize)
    };

    let memory_size : u64 = mem_map_entries
    .iter()
    .map(|entry| entry.length)
    .sum();

    unsafe {
        PHYSICAL_MEMORY_ALLOCATOR.total_pages = (memory_size / 0x1000) as u32;
        PHYSICAL_MEMORY_ALLOCATOR.bitmap = Some(Bitmap::new(mem_map_entries, PHYSICAL_MEMORY_ALLOCATOR.total_pages));
    };
}