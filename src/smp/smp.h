#pragma once

#include <stdint.h>

#include <cpu/gdt.h>

typedef struct Thread Thread;

typedef struct CpuInfo
{
    struct CpuInfo* self;
    uint64_t* kernel_stack_top;
    uint64_t* user_stack_temp;
    uint64_t lapic_id;
    uint64_t processor_id;
    uint64_t lapic_ticks_in_1ms;
    struct Tss* tss;

    // rust specific member (there might be more since I'm not updating this struct)
    // check `smp/mod.rs` for more !
    Thread* current_thread;
    Thread* idle_thread;
}CpuInfo;

void init_smp();
CpuInfo* current_cpu();