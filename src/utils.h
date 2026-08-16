#pragma once

#include <stdint.h>

extern uint8_t inb(uint16_t port);
extern uint8_t outb(uint16_t port, uint8_t data);