#pragma once

#include <stdint.h>

typedef struct
{
    uint8_t processor_id;
    uint8_t apic_id;
    uint32_t flags;
} ProcessorLapic;

typedef struct
{
    uint8_t io_apic_id;
    uintptr_t io_apic_addr;
    uint32_t gsi_base;
} IoApic;

typedef struct
{
    uint8_t bus_source;
    uint8_t irq_source;
    uint32_t gsi;
    uint16_t flags;
} InterruptSourceOverride;

typedef struct
{
    uintptr_t localApicAddr;
    uint32_t flags;

    uint32_t processor_lapic_count;
    ProcessorLapic **lapics;
    
    uint32_t io_apic_count;
    IoApic **io_apics;

    uint32_t interrupt_source_override_count;
    InterruptSourceOverride **interrupts;
}MADT;

void madt_parse(void* table_base);
MADT* MADT_get();