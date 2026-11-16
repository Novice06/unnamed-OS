#include <stdint.h>

#include <cpu/idt.h>
#include <cpu/isr.h>

#include <utils/utils.h>

#define IA32_EFER 0xC0000080
#define IA32_STAR 0xC0000081
#define IA32_LSTAR 0xC0000082
#define IA32_FMASK 0xC0000084

#define EFER_SCE 1 << 0 // System Call Extensions

#define KERNEL_CS 0x08
#define USER_BASE 0x10 // STAR high, sysret fera +8 and +16

extern void SYSCALL_entry();
extern void SYSCALL_dispatcher(TrapFrame* frame);

void SYSCALL_init_core()
{
    // authorize syscall opcode
    uint64_t efer = read_msr(IA32_EFER);
    write_msr(IA32_EFER, efer |= EFER_SCE);

    // selectors
    uint64_t star = ((uint64_t)USER_BASE << 48) | ((uint64_t)KERNEL_CS << 32);
    write_msr(IA32_STAR, star);

    write_msr(IA32_LSTAR, (uint64_t)SYSCALL_entry); // entry point

    uint64_t fmask = (1 << 9)   // IF (disable interrupt)
    | (1 << 10)  // DF (normal forward direction for string operations (e.g. memcpy))
    | (1 << 8)   // TF (Disable single-step debugging)
    | (1 << 18)  // AC (Disable alignment checking)
    | (1 << 14)  // NT (Disable nested task handling)
    | (1 << 17)  // VM (Disable virtual-8086 mode)
    | (1 << 16); // RF (Clear the resume flag)

    write_msr(IA32_FMASK, fmask);
}

void SYSCALL_init_bootstrap()
{
    SYSCALL_init_core();

    // enable interrupt syscalls
    extern void ISR128();
    IDT_setGate(0x80, ISR128, INTERRUPT_GATE | DPL_USER | GATE_PRESENT); // grant this interrupt authorisation to be called from usermode
    ISR_registerNewHandler(0x80, SYSCALL_dispatcher);
}