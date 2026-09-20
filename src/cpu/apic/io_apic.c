#include <stdint.h>
#include <stdbool.h>

#include <utils/limine_requests.h>

#include <display/serial.h>

#include <mm/rust_interface.h>

#include <acpi/madt.h>

#include "io_apic.h"

#define VECTOR_OFFSET 0x20

void IOAPIC_init()
{
    MADT* madt = MADT_get();

    for (uint32_t i = 0; i < madt->io_apic_count; i++)
    {
        IoApic *io_apic = madt->io_apics[i];

        io_apic->io_apic_virt_addr = MEM_map_MMIO(io_apic->io_apic_addr, 4096);  // remapp all

        uint32_t id  = IOAPIC_read((void*)io_apic->io_apic_virt_addr, 0x00);
        uint32_t ver = IOAPIC_read((void*)io_apic->io_apic_virt_addr, 0x01);

        uint8_t ioapic_id = (id >> 24) & 0xF;
        uint8_t version = ver & 0xFF;
        uint8_t max_redir = (ver >> 16) & 0xFF;

        SERIAL_printf(
            "iopic at: 0x%lx, IOAPIC id=%u version=0x%x redirections=%u\n",
            io_apic->io_apic_virt_addr,
            ioapic_id,
            version,
            max_redir + 1
        );

        // mask all interrupts
        for (int index = 0; index <= max_redir; index++)
        {
            uint64_t IOREDTBL_entry = IOAPIC_read((void*)io_apic->io_apic_virt_addr, 0x10 + index * 2) | ((uint64_t)IOAPIC_read((void*)io_apic->io_apic_virt_addr, 0x10 + 1 + index * 2) << 32);
            IOREDTBL_entry = IOREDTBL_entry | (1 << 16);

            IOAPIC_write((void*)io_apic->io_apic_virt_addr, 0x10 + index * 2, IOREDTBL_entry & 0xFFFFFFFF);
            IOAPIC_write((void*)io_apic->io_apic_virt_addr, 0x10 + 1 + index * 2, IOREDTBL_entry >> 32);
        }
    }
}

IoApic* find_ioApic(uint32_t gsi) {
    MADT* madt = MADT_get();

    for (uint32_t i = 0; i < madt->io_apic_count; i++)
    {
        IoApic *io_apic = madt->io_apics[i];

        uint32_t ver = IOAPIC_read((void*)io_apic->io_apic_virt_addr, 0x01);
        uint8_t max_redir = (ver >> 16) & 0xFF;
        
        if(gsi >= io_apic->gsi_base && gsi <= io_apic->gsi_base + max_redir) return io_apic;
    }

    return NULL;
} 

void IOAPIC_setGSI(uint32_t gsi, uint8_t vector, uint32_t cpu_lapic_id, bool is_edge_triggered)
{
    IoApic* io_apic = find_ioApic(gsi);

    if(io_apic)
    {
        uint32_t index = gsi - io_apic->gsi_base;

        uint8_t delivery_mode = 0; // fixed
        uint8_t destination_mode = 0; // physical
        uint8_t interrupt_polarity = 0;    // high active
        uint8_t trigger_mode = is_edge_triggered ? 0 : 1;
        uint8_t mask = 0;  // interrupt not masked
        uint8_t cpu_handler = cpu_lapic_id;

        uint64_t IOREDTBL_entry = 
            (uint64_t)vector | 
            (uint64_t)delivery_mode << 8 |
            (uint64_t)destination_mode << 11 |
            (uint64_t)interrupt_polarity << 13 |
            (uint64_t)trigger_mode << 15 |
            (uint64_t)mask << 16 |
            (uint64_t)cpu_handler << 56
        ;

        IOAPIC_write((void*)io_apic->io_apic_virt_addr, 0x10 + index * 2, IOREDTBL_entry & 0xFFFFFFFF);
        IOAPIC_write((void*)io_apic->io_apic_virt_addr, 0x10 + 1 + index * 2, IOREDTBL_entry >> 32);
    }
}


uint32_t IOAPIC_read(uint32_t* volatile io_apic_addr, uint32_t reg)
{
    io_apic_addr[0] = (reg & 0xff);
    return io_apic_addr[4];
}

void IOAPIC_write(uint32_t* volatile io_apic_addr, uint32_t reg, uint32_t value)
{
    io_apic_addr[0] = (reg & 0xff);
    io_apic_addr[4] = value;
}