#include <stddef.h>

#include <display/serial.h>
#include <utils/utils.h>

#include "isr.h"

static const char* const g_Exceptions[] = {
    "Divide by zero error",
    "Debug",
    "Non-maskable Interrupt",
    "Breakpoint",
    "Overflow",
    "Bound Range Exceeded",
    "Invalid Opcode",
    "Device Not Available",
    "Double Fault",
    "Coprocessor Segment Overrun",
    "Invalid TSS",
    "Segment Not Present",
    "Stack-Segment Fault",
    "General Protection Fault",
    "Page Fault",
    "",
    "x87 Floating-Point Exception",
    "Alignment Check",
    "Machine Check",
    "SIMD Floating-Point Exception",
    "Virtualization Exception",
    "Control Protection Exception ",
    "",
    "",
    "",
    "",
    "",
    "",
    "Hypervisor Injection Exception",
    "VMM Communication Exception",
    "Security Exception",
    ""
};

ISRHandler g_ISR_handlers[256];

void ISR_initializeGates();
void ISR_init()
{
    ISR_initializeGates();
}

void ISR_handler(Registers* regs)
{
    if(g_ISR_handlers[regs->interrupt] != NULL)
        g_ISR_handlers[regs->interrupt](regs);

    else if (regs->interrupt >= 32)
        SERIAL_printf("Unhandled interrupt %d!\n", regs->interrupt);
    
    else {
        SERIAL_printf("Unhandled exception %d %s\n", regs->interrupt, g_Exceptions[regs->interrupt]);
        
        SERIAL_printf("  rax=0x%lx  rbx=0x%lx  rcx=0x%lx  rdx=0x%lx  rsi=0x%lx  rdi=0x%lx\n",
               regs->rax, regs->rbx, regs->rcx, regs->rdx, regs->rsi, regs->rdi);

        SERIAL_printf("  rsp=0x%lx  rbp=0x%lx  rip=0x%lx  rflags=0x%lx  cs=0x%lx  ds=0x%lx  ss=0x%lx\n",
               regs->rsp, regs->rbp, regs->rip, regs->rflags, regs->cs, regs->ds, regs->ss);

        SERIAL_printf("  interrupt=%lx  errorcode=%lx\n", regs->interrupt, regs->error);

        SERIAL_puts("KERNEL PANIC!\n");
        hcf();
    }
}

void ISR_registerNewHandler(int interrupt, ISRHandler handler)
{
    g_ISR_handlers[interrupt] = handler;
}
