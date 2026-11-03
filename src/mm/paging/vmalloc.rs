use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use alloc::vec::Vec;

use crate::{lock::spinlock::SpinLockIrqSave, mm::{VirtAddr, paging}};

const MAX_CAPACITY: usize = 0x1000 / core::mem::size_of::<FreeBlock>();

struct FreeBlock {
    start: usize,
    count: usize,
}

pub struct VmallocManager {
    start_addr: AtomicU64,
    total_block: AtomicUsize,
    free_blocks: SpinLockIrqSave<Vec<FreeBlock>>,
}

impl VmallocManager {
    /// size in KB
    pub fn init(&self, start: VirtAddr, size: usize){
        self.start_addr.store(start.0, Ordering::Relaxed);
        self.total_block.store(size / 4, Ordering::Relaxed); // 4 Ko par bloc
        *self.free_blocks.lock() = Vec::with_capacity(MAX_CAPACITY);
        self.free_blocks.lock().push(FreeBlock { start: 0, count: size / 4 });
    }
    
    pub fn alloc_blocks(&self, num: usize) -> Option<VirtAddr> {

        let block_num = {
            let mut free_blocks = self.free_blocks.lock();
            if free_blocks.is_empty() {
                return None;   
            }
            
            let best_idx = free_blocks
                .iter()
                .enumerate()
                .filter(|(_, b)| b.count >= num)
                .min_by_key(|(_, b)| b.count)
                .map(|(idx, _)| idx)?;

            let block = &mut free_blocks[best_idx];
            let start = block.start;

            if block.count == num {
                free_blocks.remove(best_idx);
            } else {
                block.start += num;
                block.count -= num;
            }
            start
        };

        let ret_addr = VirtAddr(self.start_addr.load(Ordering::Relaxed) + ((block_num as u64) << 12));
        
        paging::alloc_pages_in_pdbr(ret_addr, num as u64, paging::PAGE_DEFAULT);
        
        Some(ret_addr)
    }

    pub fn free_blocks(&self, addr: VirtAddr, count: usize) {
        // the order is important here !
        super::dealloc_pages_in_pdbr(addr, count as u64);

        let mut free_blocks = self.free_blocks.lock();
        
        if free_blocks.len() >= MAX_CAPACITY {
            panic!(
                "exceeded the maximum capacity for free blocks
                cannot allocate further from the heap because multiple page allocation relies on vmalloc 
                it would result in a loop heap -> vmalloc -> heap"
            );
        }

        let new_start = ((addr.0 - self.start_addr.load(Ordering::Relaxed)) >> 12) as usize;
        let new_block = FreeBlock {
            start: new_start,
            count,
        };

        let idx = match free_blocks.binary_search_by(|b| b.start.cmp(&new_block.start)) {
            Ok(_) => panic!("vmalloc double free detected!"),
            Err(i) => i,
        };

        free_blocks.insert(idx, new_block);

        if idx + 1 < free_blocks.len() {
            let next = &free_blocks[idx + 1];
            if free_blocks[idx].start + free_blocks[idx].count == next.start {
                let next_count = next.count;
                free_blocks[idx].count += next_count;
                free_blocks.remove(idx + 1);
            }
        }

        if idx > 0 {
            let prev = &free_blocks[idx - 1];
            if prev.start + prev.count == free_blocks[idx].start {
                let curr_count = free_blocks[idx].count;
                free_blocks[idx - 1].count += curr_count;
                free_blocks.remove(idx);
            }
        }
    }
}

pub static VMALLOC: VmallocManager = VmallocManager { 
    start_addr: AtomicU64::new(0), 
    total_block: AtomicUsize::new(0), 
    free_blocks: SpinLockIrqSave::new(Vec::new())
};

pub fn init(start_addr: VirtAddr, end_addr: VirtAddr) {
    let size_kb = (end_addr.0 - start_addr.0) >> 10;
    VMALLOC.init(start_addr, size_kb as usize);
}