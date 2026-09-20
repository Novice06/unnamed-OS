#pragma once

#include <stdint.h>
#include <stdbool.h>

bool ACPI_timer_wait(uint32_t delay_ms);
void init_timer(struct FADT *fadt);