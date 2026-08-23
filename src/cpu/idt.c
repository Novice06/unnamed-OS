#include "idt.h"

typedef struct
{
    uint16_t address_low;
    uint16_t selector;
    uint8_t ist;
    uint8_t flags;
    uint16_t address_mid;
    uint32_t address_high;
    uint32_t reserved;
} __attribute__((packed)) Idt_gate;

typedef struct
{
    uint16_t limit;
    uint64_t base;
} __attribute__((packed)) Idt_descriptor;

Idt_gate IDT[256];
Idt_descriptor IDT_desc = {
    .limit = sizeof(IDT) -1,
    .base = (uint64_t)IDT,
};

void IDT_setGate(int interrupt, void* handler, uint8_t attribute)
{
    IDT[interrupt].address_low = (uint64_t)handler & 0xFFFF;
    IDT[interrupt].selector = 0x8;
    IDT[interrupt].ist = 0x0;
    IDT[interrupt].flags = attribute;
    IDT[interrupt].address_mid = ((uint64_t)handler >> 16) & 0xFFFF;
    IDT[interrupt].address_high = ((uint64_t)handler >> 32);
    IDT[interrupt].reserved = 0x0;
}

extern void idt_flush(Idt_descriptor* desc);
void IDT_init()
{
    idt_flush(&IDT_desc);
}