use core::{alloc::GlobalAlloc, sync::atomic::Ordering::Relaxed};
use crate::{mm::{HHDM_OFFSET, PhyAddr, VirtAddr, physical::PHYSICAL_MEMORY_ALLOCATOR}, spinlock::SpinLock};

mod ffi;

const HEADER_MAGIC: u64 = 0xBADA55_C0DE_BABE;

#[repr(C)]
struct SlabHeader {
    next: *mut SlabHeader,
    back: *mut SlabHeader,
    free_list: *mut u8,
    used_slots: usize,
    slab_class: usize,
    header_magic: u64,
}

impl SlabHeader {
    fn from_page(VirtAddr(page): VirtAddr, class: usize, total_slot: usize) -> *mut Self{
        let header = page as *mut SlabHeader;
        let header_ref = unsafe {
            &mut *header
        };

        header_ref.next = core::ptr::null_mut();
        header_ref.back = core::ptr::null_mut();
        header_ref.slab_class = class;
        header_ref.used_slots = 0;
        header_ref.header_magic = HEADER_MAGIC;

        header_ref.free_list = (page + 0x1000 - class as u64) as *mut u8;    // start at the end that way will will always be aligned
        let mut tail = header_ref.free_list;

        for _ in 0..total_slot -1 {
            unsafe {
                let new_tail = tail.sub(class);
                (tail as *mut *mut u8).write(new_tail);
                tail = new_tail;
                (tail as *mut *mut u8).write(core::ptr::null_mut());
            }
        }

        header
    }

    fn pop_slot(&mut self) -> Option<*mut u8>{

        if self.free_list.is_null(){
            None
        } else {
            let ret = self.free_list;
            unsafe {
                self.free_list = *(self.free_list as *mut *mut u8);
            };

            self.used_slots += 1;
            Some(ret)
        }   
    }

    fn push_slot(&mut self, slot: *mut u8) {
        unsafe {
            (slot as *mut *mut u8).write(self.free_list);
        };

        self.used_slots -= 1;
        self.free_list = slot;
    }
}

struct SlabClass {
    full_list: *mut SlabHeader,
    partial_list: *mut SlabHeader,
    class_size: usize,
    total_slot: usize,
}
unsafe impl Sync for SlabClass {}
unsafe impl Send for SlabClass {}
impl SlabClass {
    const fn new(class_index: usize) -> Self {
        let class_size = 1usize << (class_index + 3);    // classes : 8, 16, 32, 64, 128, 256, 512, 1024, 2048
        let usable = 4096 - core::mem::size_of::<SlabHeader>();
        let slots = usable / class_size;
        Self { full_list: core::ptr::null_mut(), partial_list: core::ptr::null_mut(), class_size: class_size, total_slot: slots }
    }

    fn refill(&mut self) {

        let PhyAddr(page) = PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page().expect("out of memory");
        let hhdm = HHDM_OFFSET.load(Relaxed);
        let header = SlabHeader::from_page(VirtAddr(page + hhdm), self.class_size, self.total_slot);

        unsafe {
            Self::push_to_list(&mut self.partial_list, header);
        };
    }

    unsafe fn push_to_list(list: &mut *mut SlabHeader, node: *mut SlabHeader) {
        unsafe {
            (*node).next = *list;
            (*node).back = core::ptr::null_mut();

            if !(*list).is_null() {
                (**list).back = node;
            }
            
            *list = node;
        }
    }

    unsafe fn remove_from_list(list: &mut *mut SlabHeader, node: *mut SlabHeader) {
        unsafe {
            if !(*node).back.is_null() {
                (*(*node).back).next = (*node).next;
            } else {
                *list = (*node).next;
            }
            if !(*node).next.is_null() {
                (*(*node).next).back = (*node).back;
            }
            (*node).next = core::ptr::null_mut();
            (*node).back = core::ptr::null_mut();
        }
    }

    fn alloc(&mut self) -> *mut u8{

        if self.partial_list.is_null() {
            self.refill();
        }

        let partial = unsafe {
            &mut *self.partial_list
        };

        if let Some(ptr) = partial.pop_slot() {
            if partial.used_slots == self.total_slot {
                unsafe {
                    let node = self.partial_list;
                    Self::remove_from_list(&mut self.partial_list, node);
                    Self::push_to_list(&mut self.full_list, node);
                };
            }

            return ptr;
        }

        panic!("the above logic garanties that we never reach here!")
    }

    fn free(&mut self, ptr: *mut u8, header: &mut SlabHeader) {
        let was_full = header.used_slots == self.total_slot;
        header.push_slot(ptr);

        let header_ptr = header as *mut SlabHeader;

        if was_full {

            unsafe {
                Self::remove_from_list(&mut self.full_list, header_ptr);
                Self::push_to_list(&mut self.partial_list, header_ptr);
            }
        }

        if header.used_slots == 0 {
            unsafe {
                Self::remove_from_list(&mut self.partial_list, header_ptr);
            };

            let hhdm = HHDM_OFFSET.load(Relaxed);
            let phys_addr = (header_ptr as u64) - hhdm;
            PHYSICAL_MEMORY_ALLOCATOR.lock().free_page(PhyAddr(phys_addr));
        }
    }
}

static SLAB: [SpinLock<SlabClass>; 9] = [
    SpinLock::new(SlabClass::new(0)),
    SpinLock::new(SlabClass::new(1)),
    SpinLock::new(SlabClass::new(2)),
    SpinLock::new(SlabClass::new(3)),
    SpinLock::new(SlabClass::new(4)),
    SpinLock::new(SlabClass::new(5)),
    SpinLock::new(SlabClass::new(6)),
    SpinLock::new(SlabClass::new(7)),
    SpinLock::new(SlabClass::new(8)),
];

fn size_to_class_index(size: usize) -> Option<usize> {
    if size > 2048 {
        return None;
    }
    let size = size.max(8);
    let power = size.next_power_of_two();
    Some((power.trailing_zeros() as usize).saturating_sub(3))
}

struct KernelAllocator;

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let max_size = layout.size().max(layout.align());
        
        if let Some(class_index) = size_to_class_index(max_size) {
            SLAB[class_index].lock().alloc()
        } else {
            let num_pages = (max_size + 4095) / 4096;
            if num_pages > 1 {todo!("time to implement multiple page allocator")}

            let PhyAddr(addr) = PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page().expect("out of memory");
            let hhdm = HHDM_OFFSET.load(Relaxed);

            (addr + hhdm) as *mut u8
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        let page_addr = (ptr as u64) & !0xFFF;
        let required_size = layout.size().max(layout.align());

        if let Some(class_index) = size_to_class_index(required_size) {
            let header = unsafe {
                &mut *(page_addr as *mut SlabHeader)
            };

            assert_eq!(header.header_magic, HEADER_MAGIC, "Corrupted Slab Header!");

            SLAB[class_index].lock().free(ptr, header);
        } else {
            let num_pages = (required_size + 4095) / 4096;
            if num_pages > 1 {todo!("time to implement multiple page allocator")}
            
            let hhdm = HHDM_OFFSET.load(Relaxed);
            let phys_addr = page_addr - hhdm;

            PHYSICAL_MEMORY_ALLOCATOR.lock().free_page(PhyAddr(phys_addr));
        }
    }
}