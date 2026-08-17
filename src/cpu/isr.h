#pragma once

#include <stdint.h>

typedef struct 
{
    // in the reverse order they are pushed:
    uint64_t ds;                                            // data segment pushed by us
    uint64_t rdi, rsi, rbp, rbx, rdx, rcx, rax;    // pusha
    uint64_t interrupt, error;                              // we push interrupt, error is pushed automatically (or our dummy)
    uint64_t rip, cs, rflags, rsp, ss;                      // pushed automatically by CPU
} __attribute__((packed)) Registers;

typedef void (*ISRHandler) (Registers* regs);

void ISR_init();