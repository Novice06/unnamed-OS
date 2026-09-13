#pragma once

#include <stdint.h>
#include <limine.h>

struct memory_map
{
    uint64_t rev;
    uint64_t count;
    struct limine_memmap_entry *contiguous_entries;
};


extern uint64_t MEM_init(
    struct memory_map mem_map,
    uint64_t limine_hhdm_offset,
    struct limine_executable_address_response
);
extern uint64_t MEM_reclaim_region(struct memory_map mem_map, uint64_t region_type);
extern void MEM_map_MMIO(uint64_t base, uint64_t size);

extern void* kmalloc(size_t size);
extern void* krealloc(void* ptr, size_t new_size);
extern void  kfree(void* ptr);