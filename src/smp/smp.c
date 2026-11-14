#include <stdint.h>
#include <stdbool.h>

#include <display/serial.h>

#include <cpu/apic/lapic.h>
#include <cpu/gdt.h>
#include <cpu/idt.h>

#include <mm/rust_interface.h>

#include <task/rust_interface.h>

#include <utils/limine_requests.h>
#include <utils/utils.h>

#include "smp.h"

#define IA32_GS_Base 0xC0000101

#define MAX_CPU 256

struct LapicIds
{
    uint8_t* pointer;
    uint32_t size;
};

extern void SMP_init_IPI(); // smp/mod.rs
extern CpuInfo* CPU_init(uint64_t lapic_id, uint64_t processor_id, uint64_t stack_top, struct Tss* tss_addr);

extern bool kernel_ready;
extern const uint32_t KERNEL_STACK_PAGES;
extern uint8_t kernel_end;

CpuInfo* CPUS[MAX_CPU];
struct LapicIds LAPIC_IDS;

volatile uint8_t CORE_BOOTED = 1;    // the boostrap processesor

void continue_boot_ap_core()
{
    TASK_init_idle_thread();

    LAPIC_calibrate_timer();
    LAPIC_init_default_timer();

    LAPIC_lock_timer();
    enable_interrupts();

    while (!kernel_ready) __builtin_ia32_pause();
    LAPIC_unlock_timer();
    
    for(;;)
    {
        halt();
    }
    
}

void boot_ap_core(struct limine_mp_info *core)
{
    GDT_init_early();
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

    init_cpu_struct(core->lapic_id, core->processor_id, stack_top);

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

CpuInfo* get_cpu(uint8_t lapic_id) 
{
    return CPUS[lapic_id];
}

void init_cpu_struct(uint32_t lapic_id, uint32_t processor_id, uint64_t stack_top)
{
    struct Tss* tss = GDT_init_core();

    CpuInfo *this_cpu = CPU_init(lapic_id, processor_id, stack_top, tss);

    CPUS[this_cpu->lapic_id] = this_cpu;

    // save this struct to GS.base
    write_msr(IA32_GS_Base, this_cpu);
}

void init_smp()
{
    SMP_init_IPI(); // init inter-processor interrupt subsystem

    LAPIC_IDS.pointer = kmalloc(mp_request.response->cpu_count);
    LAPIC_IDS.size = mp_request.response->cpu_count;

    for(uint32_t i = 0; i < mp_request.response->cpu_count; i++)
    {
        struct limine_mp_info *cpu = mp_request.response->cpus[i];

        ((uint8_t*)LAPIC_IDS.pointer)[i] = (uint8_t)cpu->lapic_id;

        if(cpu->lapic_id == mp_request.response->bsp_lapic_id)
        {
            uint64_t kernel_virtual_end_page = ((uint64_t)&kernel_end + 4095) & ~(0xFFF);
            uint64_t stack_top = kernel_virtual_end_page + KERNEL_STACK_PAGES * 0x1000;
            init_cpu_struct(cpu->lapic_id, cpu->processor_id, stack_top);

            continue;
        }

        __atomic_store_n(&cpu->goto_address, boot_ap_core, __ATOMIC_RELAXED);
    }

    while (CORE_BOOTED != mp_request.response->cpu_count)
    {
        __builtin_ia32_pause();
    }
}