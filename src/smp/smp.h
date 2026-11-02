#pragma once

#include <stdint.h>

typedef struct Thread Thread;

typedef struct CpuInfo
{
    struct CpuInfo* self;
    uint32_t lapic_id;
    uint32_t processor_id;
    uint32_t lapic_ticks_in_1ms;
    void* own_stack;

    // rust specific member (there might be more since I'm not updating this struct)
    // check `smp/mod.rs` for more !
    Thread* current_thread;
    Thread* idle_thread;
}CpuInfo;

void init_smp();
CpuInfo* current_cpu();