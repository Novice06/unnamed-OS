use alloc::vec::Vec;

use crate::mm::{self, VirtAddr, paging};

const PAGE_SIZE: u64 = 0x1000;
const USER_SPACE_START: u64 = 0x0000000000400000;
const USER_SPACE_END: u64 = 0x0000800000000000;

pub type VmFlags = u8;

pub const VM_READ: VmFlags  = 1 << 0;
pub const VM_WRITE: VmFlags = 1 << 1;
pub const VM_EXEC: VmFlags  = 1 << 2;


#[derive(Clone, Copy, Debug)]
pub struct VirtualArea {
    pub start: VirtAddr,
    pub pages: u64,
    pub flags: VmFlags,
}

impl VirtualArea {
    pub fn new(start: VirtAddr, pages: u64, flags: VmFlags) -> Option<Self> {
        let size = pages.checked_mul(PAGE_SIZE)?;
        let end = start.0.checked_add(size)?;

        if start.0 < USER_SPACE_START {
            return None;
        }

        if start.0 % PAGE_SIZE != 0 {
            return None;
        }

        if pages == 0 {
            return None;
        }

        if end > USER_SPACE_END {
            return None;
        }

        Some(Self {
            start,
            pages,
            flags,
        })
    }

    pub fn size(&self) -> u64 {
        self.pages * PAGE_SIZE
    }

    pub fn to_paging_flags(flags: VmFlags) -> u8 {
        let mut pflags = if (flags & VM_WRITE) == 0 { paging::PAGE_DEFAULT  } else {
            paging::PAGE_PRESENT
        };

        if (flags & VM_EXEC) != 0 {
            pflags = pflags | paging::PAGE_NO_EXECUTE;
        }

        pflags | paging::PAGE_USER
    }

    pub fn end(&self) -> VirtAddr {
        VirtAddr(self.start.0 + self.size())
    }

    pub fn contains(&self, addr: VirtAddr) -> bool {
        (self.start.0..self.end().0).contains(&addr.0)
    }

    pub fn overlaps(&self, other: &VirtualArea) -> bool {
        self.start.0 < other.end().0
            && other.start.0 < self.end().0
    }
}


pub struct AddressSpace {
    pub areas: Vec<VirtualArea>,
    pub pml4: VirtAddr,
}


impl AddressSpace {
    pub const fn new(pml4: VirtAddr) -> Self {
        Self {
            areas: Vec::new(),
            pml4,
        }
    }

    pub fn find(&self, addr: VirtAddr) -> Option<&VirtualArea> {
        self.areas.iter().find(|area| area.contains(addr))
    }

    pub fn find_mut(&mut self, addr: VirtAddr) -> Option<&mut VirtualArea> {
        self.areas.iter_mut().find(|area| area.contains(addr))
    }

    pub fn insert(&mut self, area: VirtualArea) -> bool {
        let size = match area.pages.checked_mul(PAGE_SIZE) {
            Some(size) => size,
            None => return false,
        };

        let end = match area.start.0.checked_add(size) {
            Some(end) => end,
            None => return false,
        };

        if area.pages == 0
            || area.start.0 < USER_SPACE_START
            || area.start.0 % PAGE_SIZE != 0
            || end > USER_SPACE_END
        {
            return false;
        }

        let index = self.areas.partition_point(
            |current| current.start.0 < area.start.0
        );

        if index > 0 {
            let previous = &self.areas[index - 1];

            if previous.end().0 > area.start.0 {
                return false;
            }
        }

        if index < self.areas.len() {
            let next = &self.areas[index];

            if end > next.start.0 {
                return false;
            }
        }

        self.areas.insert(index, area);

        paging::alloc_pages_low(self.pml4, area.start, area.pages, VirtualArea::to_paging_flags(area.flags)).is_ok()
    }

    pub fn remove(&mut self, addr: VirtAddr) -> Option<VirtualArea> {
        let index = self.areas.iter().position(
            |area| area.contains(addr)
        )?;

        let removed = self.areas.remove(index);

        paging::dealloc_pages_low(self.pml4, removed.start, removed.pages).ok()?;

        Some(removed)
    }

    pub fn clean(&mut self) {
        while let Some(removed) = self.areas.pop() {
            let _ = paging::dealloc_pages_low(self.pml4, removed.start, removed.pages);
        }
    }

    pub fn find_free(&self, pages: u64) -> Option<VirtAddr> {
        if pages == 0 {
            return None;
        }

        let size = pages.checked_mul(PAGE_SIZE)?;

        let mut candidate = USER_SPACE_START;

        for area in &self.areas {
            let candidate_end = candidate.checked_add(size)?;

            if candidate_end > USER_SPACE_END {
                return None;
            }

            if candidate_end <= area.start.0 {
                return Some(VirtAddr(candidate));
            }

            if candidate < area.end().0 {
                candidate = area.end().0;
            }
        }

        let end = candidate.checked_add(size)?;

        if end <= USER_SPACE_END {
            Some(VirtAddr(candidate))
        } else {
            None
        }
    }

    pub fn clone(src: &Self, dest: &mut Self) {
        dest.clean();
        let hhdm = mm::get_hhdm();

        for area in &src.areas {
            dest.insert(*area);

            for addr in (area.start.0..area.end().0).step_by(PAGE_SIZE as usize) {
                let Some(src_addr) = paging::get_physical(src.pml4, VirtAddr(addr)) else {
                    continue;
                };
                let dest_addr = paging::get_physical(dest.pml4, VirtAddr(addr)).unwrap();

                unsafe {
                    let src_addr = (src_addr.0 + hhdm) as *mut u8;
                    let dest_addr = (dest_addr.0 + hhdm) as *mut u8;

                    core::ptr::copy_nonoverlapping(src_addr, dest_addr, PAGE_SIZE as usize);
                }
            }
        }
    }

    pub fn set_flags(&mut self, addr: VirtAddr, flags: VmFlags) -> bool {
        if let Some(area) = self.find_mut(addr) {
            area.flags = flags;
            true
        } else {
            false
        }
    }
}