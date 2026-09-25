#pragma once

#include <stdint.h>
#include <stddef.h>
#include <limine.h>

#define PAGE_PRESENT                1 << 0
#define PAGE_WRITABLE               1 << 1
#define PAGE_USER                   1 << 2
#define PAGE_WRITE_THROUGH_CACHE    1 << 3
#define PAGE_DISABLE_CACHE          1 << 4
#define PAGE_GLOBAL                 1 << 5
#define PAGE_NO_EXECUTE             1 << 6

struct memory_map
{
    uint64_t rev;
    uint64_t count;
    struct limine_memmap_entry *contiguous_entries;
};


extern void MEM_init(
    struct memory_map mem_map,
    uint64_t limine_hhdm_offset,
    struct limine_executable_address_response
);
extern uint64_t MEM_reclaim_region(struct memory_map mem_map, uint64_t region_type);
extern void MEM_alloc_pages(uint64_t virt_addr, uint64_t num_pages, uint8_t flags);
extern uint64_t MEM_map_MMIO(uint64_t base, uint64_t size);
extern uint64_t MEM_get_kernel_addresspace();

extern void* kmalloc(size_t size);
extern void* krealloc(void* ptr, size_t new_size);
extern void  kfree(void* ptr);