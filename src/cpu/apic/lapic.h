#pragma once

void LAPIC_init();
uint32_t LAPIC_read(unsigned int reg);
void LAPIC_write(unsigned int reg, uint32_t value);