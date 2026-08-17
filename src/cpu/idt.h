#pragma once

#include <stdint.h>

typedef enum {
    INTERRUPT_GATE  = 0x0E,
    TRAP_GATE       = 0x0F,

    DPL_KERNEL      = 0x00,
    DPL_USER        = 0x30,

    GATE_PRESENT    = 0x80,
}GATE_ATTRIBUTE;

void IDT_init();
void IDT_setGate(int interrupt, void* handler, uint8_t attribute);