#pragma once

#include <stdint.h>

typedef struct Thread Thread;

typedef struct CpuInfo
{
    struct CpuInfo* self;
    uint32_t lapic_id;
    uint32_t processor_id;
    void* own_stack;
    Thread* current_thread;
    Thread* idle_thread;
}CpuInfo;

void init_smp();
CpuInfo* current_cpu();