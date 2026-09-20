#include <stdint.h>

#include <display/serial.h>
#include <utils/utils.h>

#include <mm/rust_interface.h>

#include "acpi.h"
#include "madt.h"

typedef struct
{
    uint8_t type;
    uint8_t length;
}__attribute__ ((packed)) RecordHeader_t;

typedef struct
{
    RecordHeader_t header;
    uint8_t processor_id;
    uint8_t apic_id;
    uint32_t flags;
} __attribute__((packed)) Processor_lapic_t;

typedef struct
{
    RecordHeader_t header;
    uint8_t io_pic_id;
    uint8_t reserved;
    uint32_t io_pic_addr;
    uint32_t global_SI_base;
}__attribute__ ((packed)) IO_apic_t;

typedef struct
{
    RecordHeader_t header;
    uint8_t bus_source;
    uint8_t irq_source;
    uint32_t global_SI;
    uint16_t flags;
}__attribute__ ((packed)) IO_apic_IS_override_t;

typedef struct 
{
    AcpiHeader_t header;
    uint32_t localApicAddr;
    uint32_t flags;
} __attribute__ ((packed)) MADT_t;

static MADT_t packed_MADT;
static MADT MADT_info = {0};

void MADT_log(const MADT *madt);
void madt_parse(void* table_base)
{
    memcpy(&packed_MADT, table_base, sizeof(MADT_t));
    MADT_info.localApicAddr = packed_MADT.localApicAddr;
    MADT_info.flags = packed_MADT.flags;

    RecordHeader_t* header = (RecordHeader_t*)((uint64_t)table_base + sizeof(MADT_t));

    while((uint64_t)header < (packed_MADT.header.length + (uint64_t)table_base))
    {
        switch (header->type)
        {
        case 0: {
            Processor_lapic_t *lapic = (Processor_lapic_t *)header;

            ProcessorLapic *new = kmalloc(sizeof(ProcessorLapic));
            new->processor_id = lapic->processor_id;
            new->apic_id      = lapic->apic_id;
            new->flags        = lapic->flags;

            MADT_info.processor_lapic_count++;
            MADT_info.lapics = krealloc(MADT_info.lapics, sizeof(ProcessorLapic*) * MADT_info.processor_lapic_count);

            MADT_info.lapics[MADT_info.processor_lapic_count - 1] = new;
            break;
        }

        case 1: {
            IO_apic_t* io_apic = (IO_apic_t*)header;
            
            IoApic *new = kmalloc(sizeof(IoApic));
            new->io_apic_id = io_apic->io_pic_id;
            new->io_apic_addr = io_apic->io_pic_addr;
            new->gsi_base = io_apic->global_SI_base;

            MADT_info.io_apic_count++;
            MADT_info.io_apics = krealloc(MADT_info.io_apics, sizeof(IoApic*) * MADT_info.io_apic_count);

            MADT_info.io_apics[MADT_info.io_apic_count - 1] = new;
            break;
        }

        case 2: {
            IO_apic_IS_override_t* io_apic_override = (IO_apic_IS_override_t*)header;

            InterruptSourceOverride* new = kmalloc(sizeof(InterruptSourceOverride));
            new->bus_source = io_apic_override->bus_source;
            new->flags = io_apic_override->flags;
            new->gsi = io_apic_override->global_SI;
            new->irq_source = io_apic_override->irq_source;

            MADT_info.interrupt_source_override_count++;
            MADT_info.interrupts = krealloc(MADT_info.interrupts, sizeof(InterruptSourceOverride*) * MADT_info.interrupt_source_override_count);
            
            MADT_info.interrupts[MADT_info.interrupt_source_override_count - 1] =  new;
            break;
        }
        
        default:
            SERIAL_printf("this header is of type: %x\n", header->type);
            break;
        }

        header = (RecordHeader_t*)((uint64_t)header + header->length);
    }

    MADT_log(&MADT_info);
}

void MADT_log(const MADT *madt)
{
    if (madt == NULL)
    {
        SERIAL_printf("[MADT] NULL\n");
        return;
    }

    SERIAL_printf("\n");
    SERIAL_printf("========== MADT ==========\n");

    SERIAL_printf("Local APIC Address : 0x%x\n", madt->localApicAddr);
    SERIAL_printf("Flags              : 0x%x\n", madt->flags);

    /*
     * Processor LAPICs
     */
    SERIAL_printf("\n");
    SERIAL_printf("--- Processor LAPICs (%u) ---\n",
                  madt->processor_lapic_count);

    for (uint32_t i = 0; i < madt->processor_lapic_count; i++)
    {
        ProcessorLapic *lapic = madt->lapics[i];

        if (lapic == NULL)
        {
            SERIAL_printf("LAPIC[%u] : NULL\n", i);
            continue;
        }

        SERIAL_printf("LAPIC[%u] @ %lp\n", i, lapic);
        SERIAL_printf("  Processor ID : %u\n", lapic->processor_id);
        SERIAL_printf("  APIC ID      : %u\n", lapic->apic_id);
        SERIAL_printf("  Flags        : 0x%x\n", lapic->flags);
    }

    /*
     * I/O APICs
     */
    SERIAL_printf("\n");
    SERIAL_printf("--- I/O APICs (%u) ---\n",
                  madt->io_apic_count);

    for (uint32_t i = 0; i < madt->io_apic_count; i++)
    {
        IoApic *io_apic = madt->io_apics[i];

        if (io_apic == NULL)
        {
            SERIAL_printf("IOAPIC[%u] : NULL\n", i);
            continue;
        }

        SERIAL_printf("IOAPIC[%u] @ %lp\n", i, io_apic);
        SERIAL_printf("  ID       : %u\n", io_apic->io_apic_id);
        SERIAL_printf("  Address  : 0x%x\n", io_apic->io_apic_addr);
        SERIAL_printf("  GSI Base : %u\n", io_apic->gsi_base);
    }

    /*
     * Interrupt Source Overrides
     */
    SERIAL_printf("\n");
    SERIAL_printf("--- Interrupt Source Overrides (%u) ---\n",
                  madt->interrupt_source_override_count);

    for (uint32_t i = 0;
         i < madt->interrupt_source_override_count;
         i++)
    {
        InterruptSourceOverride *override = madt->interrupts[i];

        if (override == NULL)
        {
            SERIAL_printf("OVERRIDE[%u] : NULL\n", i);
            continue;
        }

        SERIAL_printf("OVERRIDE[%u] @ %lp\n", i, override);
        SERIAL_printf("  Bus Source : %u\n", override->bus_source);
        SERIAL_printf("  IRQ Source : %u\n", override->irq_source);
        SERIAL_printf("  GSI        : %u\n", override->gsi);
        SERIAL_printf("  Flags      : 0x%x\n", override->flags);
    }

    SERIAL_printf("===========================\n\n");
}

MADT* MADT_get()
{
    return &MADT_info;
}