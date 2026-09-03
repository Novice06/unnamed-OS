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