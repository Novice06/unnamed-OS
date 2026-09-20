#include <stdbool.h>

#include <display/serial.h>

#include <utils/utils.h>

#include <mm/rust_interface.h>

#include "fadt.h"

#define ACPI_PM_TIMER_FREQ 3579545ULL

typedef struct
{
    bool is_available;
    bool is_32bit;
    bool is_mmio;
    uintptr_t addr_or_io;
    uintptr_t virt_addr;
} AcpiTimer;

static AcpiTimer timer;

void init_timer(struct FADT *fadt)
{
    if (fadt->PMTimerLength != 4)
    {
        timer.is_available =  false;
        SERIAL_printf("ACPI timer not available!\n");
        return;
    }
    timer.is_available = true;

    timer.is_mmio = false;
    timer.addr_or_io = fadt->PMTimerBlock;

    if(fadt->header.revision >= 2 && fadt->X_PMTimerBlock.Address != 0)
    {
        timer.is_mmio = true;
        timer.addr_or_io = fadt->X_PMTimerBlock.Address;

        timer.virt_addr = MEM_map_MMIO(timer.addr_or_io, 4096);
    }

    if (fadt->Flags & (1<<8)) timer.is_32bit = true;

    SERIAL_printf(
        "timer is %s at 0x%lx and is a %s value\n\n",
        timer.is_mmio ? "mmio" : "a port", 
        timer.addr_or_io,
        timer.is_32bit ? "32bit" : "24bit"
    );
}

uint32_t get_current_count()
{
    // timer is  3,579545 MHz.

    uint32_t current_count;
    if(timer.is_mmio) current_count = *(uint32_t* volatile)timer.virt_addr;
    else current_count = indw(timer.addr_or_io);

    if(!timer.is_32bit) current_count = current_count & 0xFFFFFF;

    return current_count;
}

uint64_t ms_to_ticks(uint32_t ms)
{
    return (ACPI_PM_TIMER_FREQ * ms) / 1000;
}

bool ACPI_timer_wait(uint32_t delay_ms)
{
    if (!timer.is_available) return false;

    uint32_t ticks = ms_to_ticks(delay_ms);
    uint32_t start = get_current_count();

    while ((get_current_count() - start) < ticks)
    {
        __builtin_ia32_pause();
    }
    
    return true;
}