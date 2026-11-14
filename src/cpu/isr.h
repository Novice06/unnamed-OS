#pragma once

#include <stdint.h>

typedef struct 
{
    // in the reverse order they are pushed:
    uint64_t ds;                                            // data segment pushed by us
    uint64_t rax, rbx, rcx, rdx, rsi, rdi, rbp;
    uint64_t r8, r9, r10, r11, r12, r13, r14, r15;
    uint64_t interrupt, error;                              // we push interrupt, error is pushed automatically (or our dummy)
    uint64_t rip, cs, rflags, rsp, ss;                      // pushed automatically by CPU
} __attribute__((packed)) TrapFrame;

typedef void (*ISRHandler) (TrapFrame* regs);

void ISR_init();
void ISR_registerNewHandler(int interrupt, ISRHandler handler);