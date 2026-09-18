#include <stdint.h>

#include <display/serial.h>

#include <mm/rust_interface.h>

#include <utils/utils.h>
#include <utils/limine_requests.h>

#include "lapic.h"

#define IA32_APIC_BASE 0x1B

#define PIC1_COMMAND_PORT           0x20
#define PIC1_DATA_PORT              0x21
#define PIC2_COMMAND_PORT           0xA0
#define PIC2_DATA_PORT              0xA1

typedef enum {
    PIC_ICW1 = 0x11, // initialization sequence

    // emapping the IRQs starting at 32 in the IDT
    PIC1_ICW2 = 0x20,
    PIC2_ICW2 = 0x28,

    PIC1_ICW3 = 0x4,  // tell PIC1 that it has a slave at IRQ2 (0000 0100)
    PIC2_ICW3 = 0x2,  // tell PIC2 its cascade identity (0000 0010)

    PIC_ICW4 = 0x1, // use the 8086 mode.
}PIC_controlWord;

static uint32_t volatile *lapic_base_addr;

void disable_pic() {
    outb(PIC1_COMMAND_PORT, PIC_ICW1);
    outb(PIC2_COMMAND_PORT, PIC_ICW1);
    outb(PIC1_DATA_PORT, PIC1_ICW2);
    outb(PIC2_DATA_PORT, PIC2_ICW2);
    outb(PIC1_DATA_PORT, PIC1_ICW3);
    outb(PIC2_DATA_PORT, PIC2_ICW3);
    outb(PIC1_DATA_PORT, PIC_ICW4);
    outb(PIC2_DATA_PORT, PIC_ICW4);

    // disable pic
    outb(PIC1_DATA_PORT, 0xFF);
    outb(PIC2_DATA_PORT, 0xFF);
}

void LAPIC_init()
{
    disable_pic();

    // TODO: check apic with cpuid

    uint32_t lapic_physical = read_msr(IA32_APIC_BASE) & 0x000FFFFFFFFFF000; 
    lapic_base_addr = (uint32_t*) MEM_map_MMIO(lapic_physical, 4096);
    SERIAL_printf("lapic at 0x%lx\n", lapic_base_addr);

    // enable APIC by setting bit 8 (Software Enable) of SVR (0xF0)
    // assign vector 0xFF for spurious interrupts
    uint32_t spurious_vector_reg = LAPIC_read(0xF0);
    LAPIC_write(0xF0, spurious_vector_reg | 0x100 | 0xFF);
}

uint32_t LAPIC_read(unsigned int reg)
{
    return lapic_base_addr[reg / sizeof(uint32_t)];
}

void LAPIC_write(unsigned int reg, uint32_t value)
{
    lapic_base_addr[reg / sizeof(uint32_t)] = value;
}