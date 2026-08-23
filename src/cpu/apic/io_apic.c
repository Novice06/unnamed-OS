#include <stdint.h>

#include <utils/limine_requests.h>

#include <display/serial.h>

#include <acpi/madt.h>

#include "io_apic.h"

static IO_pic_info_t volatile *info;
uint32_t* volatile io_apic_addr;

void IOAPIC_init()
{
    info = MADT_getIOApicInfo();
    io_apic_addr = (uint32_t*)(info->io_apic->io_pic_addr + hhdm_request.response->offset);

    uint32_t id  = IOAPIC_read(0x00);
    uint32_t ver = IOAPIC_read(0x01);

    uint8_t ioapic_id = (id >> 24) & 0xF;
    uint8_t version = ver & 0xFF;
    uint8_t max_redir = (ver >> 16) & 0xFF;

    SERIAL_printf(
        "IOAPIC id=%u version=0x%x redirections=%u\n",
        ioapic_id,
        version,
        max_redir + 1
    );
}

uint32_t IOAPIC_read(uint32_t reg)
{
    io_apic_addr[0] = (reg & 0xff);
    return io_apic_addr[4];
}

void IOAPIC_write(uint32_t reg, uint32_t value)
{
    io_apic_addr[0] = (reg & 0xff);
    io_apic_addr[4] = value;
}