#pragma once

#include "smp.h"

extern CpuInfo* CPU_init(uint32_t lapic_id, uint32_t processor_id, uint64_t stack_top);
extern void draw_framebuffer(uint8_t* addr, int width, int heigh, int pitch);