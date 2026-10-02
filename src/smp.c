#include <stdint.h>

#include <display/serial.h>

#include <cpu/apic/lapic.h>
#include <cpu/gdt.h>
#include <cpu/idt.h>

#include <mm/rust_interface.h>

#include <task/rust_interface.h>

#include <utils/limine_requests.h>
#include <utils/utils.h>

#include "smp.h"
#include "rust_interface.h"

#define IA32_GS_Base 0xC0000101

extern const uint32_t KERNEL_STACK_PAGES;
extern uint8_t kernel_end;

volatile uint8_t CORE_BOOTED = 1;    // the boostrap processesor

void continue_boot_ap_core()
{
    TASK_init();
    SERIAL_printf("waiting for anything to do!\n");
    hcf();
}

void boot_ap_core(struct limine_mp_info *core)
{
    GDT_init();
    IDT_init();

    uint64_t pdbr = MEM_get_kernel_addresspace();
    switch_pdbr(pdbr);

    LAPIC_init();

    SERIAL_printf("core %d, booted successfully\n", core->processor_id);

    uint8_t old_count = __atomic_fetch_add(&CORE_BOOTED, 1, __ATOMIC_ACQ_REL);

    uint64_t kernel_virtual_end_page = ((uint64_t)&kernel_end + 4095 + old_count * KERNEL_STACK_PAGES * 0x1000) & ~(0xFFF);    // this way every cpu will have its own 32kb stack mapped
    MEM_alloc_pages(
        kernel_virtual_end_page,
        KERNEL_STACK_PAGES,
        PAGE_PRESENT | PAGE_WRITABLE | PAGE_GLOBAL
    );

    uint64_t stack_top = kernel_virtual_end_page + KERNEL_STACK_PAGES * 0x1000;
    SERIAL_printf("new stack at: 0x%lx\n", stack_top);

    CpuInfo *this_cpu = CPU_init(core->lapic_id, core->processor_id, stack_top);

    // save this struct to GS.base
    write_msr(IA32_GS_Base, this_cpu);

    switch_stack(stack_top, (void*)continue_boot_ap_core);
}

CpuInfo* current_cpu()
{
    CpuInfo* cpu;

    __asm__ volatile (
        "mov %%gs:0, %0"
        : "=r"(cpu)
    );

    return cpu;
}

void init_smp()
{
    for(uint32_t i = 0; i < mp_request.response->cpu_count; i++)
    {
        struct limine_mp_info *cpu = mp_request.response->cpus[i];

        if(cpu->lapic_id == mp_request.response->bsp_lapic_id)
        {
            uint64_t kernel_virtual_end_page = ((uint64_t)&kernel_end + 4095) & ~(0xFFF);
            uint64_t stack_top = kernel_virtual_end_page + KERNEL_STACK_PAGES * 0x1000;
            CpuInfo *this_cpu = CPU_init(cpu->lapic_id, cpu->processor_id, stack_top);

            // save this struct to GS.base
            write_msr(IA32_GS_Base, this_cpu);

            continue;
        }

        __atomic_store_n(&cpu->goto_address, boot_ap_core, __ATOMIC_RELAXED);
    }

    while (CORE_BOOTED != mp_request.response->cpu_count)
    {
        __builtin_ia32_pause();
    }
}