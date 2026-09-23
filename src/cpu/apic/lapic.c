#include <stdint.h>

#include <display/serial.h>

#include <mm/rust_interface.h>

#include <utils/utils.h>
#include <utils/limine_requests.h>

#include <acpi/timer.h>

#include "lapic.h"

#define IA32_APIC_BASE 0x1B

#define PIC1_COMMAND_PORT           0x20
#define PIC1_DATA_PORT              0x21
#define PIC2_COMMAND_PORT           0xA0
#define PIC2_DATA_PORT              0xA1

#define DIVISOR_CONFIGURATION 3 // 16 

typedef enum {
    PIC_ICW1 = 0x11, // initialization sequence

    // emapping the IRQs starting at 32 in the IDT
    PIC1_ICW2 = 0x20,
    PIC2_ICW2 = 0x28,

    PIC1_ICW3 = 0x4,  // tell PIC1 that it has a slave at IRQ2 (0000 0100)
    PIC2_ICW3 = 0x2,  // tell PIC2 its cascade identity (0000 0010)

    PIC_ICW4 = 0x1, // use the 8086 mode.
}PIC_controlWord;

static uint32_t volatile *LAPIC_BASE_ADDR;
static uint32_t LAPIC_TICKS_IN_1MS;

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

void LAPIC_init_bootstrap()
{
    disable_pic();

    // TODO: check apic with cpuid

    uint32_t lapic_physical = read_msr(IA32_APIC_BASE) & 0x000FFFFFFFFFF000; 
    LAPIC_BASE_ADDR = (uint32_t*) MEM_map_MMIO(lapic_physical, 4096);
    SERIAL_printf("lapic at 0x%lx\n", LAPIC_BASE_ADDR);

    // enable APIC by setting bit 8 (Software Enable) of SVR (0xF0)
    // assign vector 0xFF for spurious interrupts
    uint32_t spurious_vector_reg = LAPIC_read(0xF0);
    LAPIC_write(0xF0, spurious_vector_reg | 0x100 | 0xFF);

    // try to figure out how many ticks in 1ms this apic generate
    LAPIC_write(0x3E0, DIVISOR_CONFIGURATION);  // divide the frequency by 16
    LAPIC_write(0x380, 0xFFFFFFFF);     // count down from this

    ACPI_timer_wait(1); // wait 1ms

    LAPIC_TICKS_IN_1MS = 0xFFFFFFFF - LAPIC_read(0x390);  // read current count register and compute elapsed ticks
    LAPIC_write(0x380, 0);  // stop the count down

    SERIAL_printf("lapic ticks in 1ms 0x%x\n", LAPIC_TICKS_IN_1MS);
}

void LAPIC_init()
{
    // enable APIC by setting bit 8 (Software Enable) of SVR (0xF0)
    // assign vector 0xFF for spurious interrupts
    uint32_t spurious_vector_reg = LAPIC_read(0xF0);
    LAPIC_write(0xF0, spurious_vector_reg | 0x100 | 0xFF);
}

uint32_t LAPIC_read(unsigned int reg)
{
    return LAPIC_BASE_ADDR[reg / sizeof(uint32_t)];
}

void LAPIC_write(unsigned int reg, uint32_t value)
{
    LAPIC_BASE_ADDR[reg / sizeof(uint32_t)] = value;
}

void lapic_send_eoi()
{
    LAPIC_write(0x0B0, 0);
}

/**
 * 
 * Bit Details:
 * -------------
 * [07:00] Vector          - Interrupt vector number (0-255) injected into the CPU IDT.
 * [11:08] Reserved        - Read as 0.
 * [12]    Delivery Status - 0: Idle (no pending interrupt).
 *                           1: Send Pending (interrupt injected but not yet accepted). [Read-Only]
 * [15:13] Reserved        - Read as 0.
 * [16]    Mask            - Interrupt Mask. 0: Unmasked (enabled); 1: Masked (disabled).
 * [18:17] Timer Mode      - Operational mode:
 *                           00b = One-shot (counts down once, fires, stops).
 *                           01b = Periodic (automatically reloads initial count when 0).
 *                           10b = TSC-Deadline (uses IA32_TSC_DEADLINE MSR).
 *                           11b = Reserved.
 * [31:19] Reserved        - Read as 0.
 */

void LAPIC_init_periodic_timer(uint8_t vector, uint32_t ms)
{
    uint32_t lvt = (1 << 17) | vector;
    LAPIC_write(0x320, lvt);

    uint32_t ticks_count = LAPIC_TICKS_IN_1MS * ms;
    LAPIC_write(0x380, ticks_count);
}

void LAPIC_init_1shot_timer(uint8_t vector, uint32_t ms)
{
    uint32_t lvt = vector;
    LAPIC_write(0x320, lvt);

    uint32_t ticks_count = LAPIC_TICKS_IN_1MS * ms;
    LAPIC_write(0x380, ticks_count);
}