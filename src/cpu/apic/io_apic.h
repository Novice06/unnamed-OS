#pragma once

void IOAPIC_init();
uint32_t IOAPIC_read(uint32_t* volatile io_apic_addr, uint32_t reg);
void IOAPIC_write(uint32_t* volatile io_apic_addr, uint32_t reg, uint32_t value);