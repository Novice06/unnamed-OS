#pragma once

void LAPIC_init_bootstrap();
void LAPIC_init();
uint32_t LAPIC_read(unsigned int reg);
void LAPIC_write(unsigned int reg, uint32_t value);
void lapic_send_eoi();
void LAPIC_init_periodic_timer(uint8_t vector, uint32_t ms);
void LAPIC_init_1shot_timer(uint8_t vector, uint32_t ms);