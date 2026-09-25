use core::sync::atomic::{AtomicU64, Ordering::{self, Relaxed}};

use crate::{mm::physical::PHYSICAL_MEMORY_ALLOCATOR, println};

pub mod physical;
pub mod paging;
pub mod slab;

const LIMINE_MEMMAP_USABLE: u64 = 0;
const _LIMINE_MEMMAP_RESERVED: u64 = 1;
const _LIMINE_MEMMAP_ACPI_RECLAIMABLE: u64 = 2;
const _LIMINE_MEMMAP_ACPI_NVS: u64 = 3;
const _LIMINE_MEMMAP_BAD_MEMORY: u64 = 4;
const LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE: u64 = 5;
const LIMINE_MEMMAP_EXECUTABLE_AND_MODULES: u64 = 6;
const LIMINE_MEMMAP_FRAMEBUFFER: u64 = 7;
const _LIMINE_MEMMAP_RESERVED_MAPPED: u64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhyAddr(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtAddr(pub u64);

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

#[repr(C)]
pub struct LimineExecutableAddr {
    revision: u64,
    physical_base: u64,
    virtual_base: u64,
}

static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);
static KERNEL_ADDR_SPACE: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn MEM_init(
    mem_map: LimineMemMap,
    limine_hhdm_offset: u64,
    executable_addr: LimineExecutableAddr,
) {

    HHDM_OFFSET.store(limine_hhdm_offset, Ordering::Relaxed); // use the same hhdm as limine

    let mem_map_entries= unsafe {
        core::slice::from_raw_parts(mem_map.entries, mem_map.count as usize)
    };

    let memory_size : u64 = mem_map_entries
    .iter()
    .filter(|entry| [LIMINE_MEMMAP_USABLE, LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE, LIMINE_MEMMAP_EXECUTABLE_AND_MODULES].contains(&entry.etype) )
    .map(|entry| entry.base + entry.length)
    .max()
    .unwrap();

    println!("mem size: {}MB", memory_size / (1024 * 1024));

    for entry in mem_map_entries.iter() {
        println!("base 0x{:x}, length 0x{:x}, type {}", entry.base, entry.length, entry.etype);
    }

    physical::init(memory_size as u32, mem_map_entries, limine_hhdm_offset);
    paging::init(mem_map_entries, executable_addr);
}

#[unsafe(no_mangle)]
pub extern "C" fn MEM_alloc_pages(virt_addr: u64, num_pages: u64, flags: u8) {
    let hhdm = HHDM_OFFSET.load(Relaxed);
    let pml4_addr = unsafe {
        crate::get_pdbr()
    };

    paging::alloc_pages(VirtAddr(pml4_addr + hhdm), VirtAddr(virt_addr), num_pages, flags);
}

#[unsafe(no_mangle)]
pub extern "C" fn MEM_reclaim_region(mem_map: LimineMemMap, region: u64) {
    let mem_map_entries= unsafe {
        core::slice::from_raw_parts(mem_map.entries, mem_map.count as usize)
    };

    PHYSICAL_MEMORY_ALLOCATOR.lock().reclaim(mem_map_entries, region);
}

#[unsafe(no_mangle)]
pub extern "C" fn MEM_map_MMIO(base: u64, size: u64) -> u64 {
    let num_pages = size / 0x1000;
    
    let VirtAddr(virt) = paging::map_mmio(PhyAddr(base), num_pages);

    virt
}

#[unsafe(no_mangle)]
pub extern "C" fn MEM_get_kernel_addresspace() -> u64 {
    return KERNEL_ADDR_SPACE.load(Relaxed);
}