#pragma once

#include <stdbool.h>

void IOAPIC_init();
uint32_t IOAPIC_read(uint32_t* volatile io_apic_addr, uint32_t reg);
void IOAPIC_write(uint32_t* volatile io_apic_addr, uint32_t reg, uint32_t value);
void IOAPIC_setGSI(uint32_t gsi, uint8_t vector, uint32_t cpu_lapic_id, bool is_edge_triggered);