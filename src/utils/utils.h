#pragma once

#include <stdint.h>
#include <stddef.h>

extern uint8_t inb(uint16_t port);
extern uint8_t outb(uint16_t port, uint8_t data);
extern uint64_t read_msr(uint32_t ecx);
extern void switch_pdbr(uint64_t pdbr);
extern uint64_t get_pdbr();
extern void switch_stack(uint64_t new_stack, void* func);
extern void halt();

void *memcpy(void *restrict dest, const void *restrict src, size_t n);
void *memset(void *s, int c, size_t n);
void *memmove(void *dest, const void *src, size_t n);
int memcmp(const void *s1, const void *s2, size_t n);
void hcf(void);