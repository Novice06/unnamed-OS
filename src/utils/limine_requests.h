#pragma once

#include <stdint.h>

#include <limine.h>

extern volatile uint64_t limine_base_revision[];
extern volatile struct limine_framebuffer_request framebuffer_request;
extern volatile struct limine_rsdp_request rsdp_request;
extern volatile struct limine_hhdm_request hhdm_request;