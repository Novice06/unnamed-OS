#include <stdint.h>
#include <stddef.h>

#include <cpu/gdt.h>
#include <cpu/idt.h>
#include <cpu/isr.h>
#include <cpu/apic/lapic.h>
#include <cpu/apic/io_apic.h>

#include <display/serial.h>

#include <utils/utils.h>
#include <utils/limine_requests.h>

#include <acpi/acpi.h>
#include <acpi/timer.h>

#include <mm/rust_interface.h>

#define KERNEL_STACK_PAGES   8  // 32kb

extern uint8_t kernel_end;

static struct limine_memmap_entry contiguous_entries[256];
struct memory_map contiguous = {0};

static inline uintptr_t get_cr2(void) {
    uintptr_t value;
    __asm__ volatile ("mov %%cr2, %0" : "=r"(value));
    return value;
}

void early_page_fault_handler(Registers* regs)
{
    SERIAL_printf("PAGE FAULT !!\n");
    SERIAL_printf("  rax=0x%lx  rbx=0x%lx  rcx=0x%lx  rdx=0x%lx  rsi=0x%lx  rdi=0x%lx\n",
            regs->rax, regs->rbx, regs->rcx, regs->rdx, regs->rsi, regs->rdi);

    SERIAL_printf("  rsp=0x%lx  rbp=0x%lx  rip=0x%lx  rflags=0x%lx  cs=0x%lx  ds=0x%lx  ss=0x%lx\n",
            regs->rsp, regs->rbp, regs->rip, regs->rflags, regs->cs, regs->ds, regs->ss);

    SERIAL_printf("  interrupt=%lx  errorcode=%lx\n", regs->interrupt, regs->error);
    SERIAL_printf("cr2: 0x%lx\n", get_cr2());

    SERIAL_puts("KERNEL PANIC!\n");
    hcf();
}

volatile uint8_t CORE_BOOTED = 1;    // the boostrap processesor

void continue_boot_ap_core()
{
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

    switch_stack(stack_top, (void*)continue_boot_ap_core);
}

void kmain_continue()
{
    if(!ACPI_parse(rsdp_request.response->address))
        SERIAL_printf("cannot parse acpi tables\n");

    // reclaime acpi
    // MEM_reclaim_region(contiguous, LIMINE_MEMMAP_ACPI_RECLAIMABLE);

    LAPIC_init_bootstrap();
    IOAPIC_init();

    SERIAL_printf(
        "bootstrap lapic id: %d, cpu count: %ld\n\n",
        mp_request.response->bsp_lapic_id,
        mp_request.response->cpu_count
    );

    for(uint32_t i = 0; i < mp_request.response->cpu_count; i++)
    {
        struct limine_mp_info *cpu = mp_request.response->cpus[i];
        if(cpu->lapic_id == mp_request.response->bsp_lapic_id) continue;

        __atomic_store_n(&cpu->goto_address, boot_ap_core, __ATOMIC_RELAXED);
    }

    while (CORE_BOOTED != mp_request.response->cpu_count)
    {
        __builtin_ia32_pause();
    }

    // reclaime bootloader region
    // MEM_reclaim_region(contiguous, LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE);

    // Fetch the first framebuffer.
    struct limine_framebuffer *framebuffer = framebuffer_request.response->framebuffers[0];

    SERIAL_printf("framebuffer at 0x%lx\n", framebuffer->address);

    extern void draw_framebuffer(uint8_t* addr, int width, int heigh, int pitch);
    draw_framebuffer(
        framebuffer->address,
        framebuffer->width,
        framebuffer->height,
        framebuffer->pitch
    );

    // We're done, just hang...
    hcf();
}

void kmain()
{
    if(!LIMINE_BASE_REVISION_SUPPORTED(limine_base_revision))
        hcf();

    if(framebuffer_request.response == NULL || framebuffer_request.response->framebuffer_count < 1)
        hcf();

    if(rsdp_request.response == NULL || (void*)rsdp_request.response->address == NULL)
        hcf();

    if(hhdm_request.response == NULL || (void*)hhdm_request.response->offset == NULL)
        hcf();

    if(memmap_request.response == NULL || (void*)memmap_request.response->entries == NULL)
        hcf();

    if(mp_request.response == NULL || mp_request.response->cpu_count <= 0) // we shoud at least recieve the bootstrap processor
        hcf();

    SERIAL_init();

    SERIAL_printf("hhdm offset 0x%lx\n", hhdm_request.response->offset);

    GDT_init();
    IDT_init();
    ISR_init();

    ISR_registerNewHandler(14, early_page_fault_handler);

    contiguous.rev = memmap_request.response->revision;
    contiguous.count = memmap_request.response->entry_count;
    contiguous.contiguous_entries = contiguous_entries;

    for(uint64_t i = 0; i < memmap_request.response->entry_count; i++)
        memcpy(&contiguous_entries[i], memmap_request.response->entries[i], sizeof(struct limine_memmap_entry));

    MEM_init(contiguous, hhdm_request.response->offset, *executable_request.response);

    uint64_t kernel_virtual_end_page = ((uint64_t)&kernel_end + 4095) & ~(0xFFF);
    MEM_alloc_pages(
        kernel_virtual_end_page,
        KERNEL_STACK_PAGES,
        PAGE_PRESENT | PAGE_WRITABLE | PAGE_GLOBAL
    );
        
    uint64_t stack_top = kernel_virtual_end_page + KERNEL_STACK_PAGES * 0x1000;

    SERIAL_printf("new stack at: 0x%lx\n", stack_top);

//    // switch to our own stack...
//    // hopefully we dont have anything else we want to access from the stack ...
//    // which is the case because every variable used until now is a global variable !
//    __asm__ volatile (
//         "mov %0, %%rsp"
//         :
//         : "r"(stack_top)
//         : "memory"
//     );

    switch_stack(stack_top, (void*)kmain_continue);

    // uint32_t* not_mmaped = (uint32_t*)stack_top;

    // uint32_t really = *not_mmaped;

    // never reach here
    hcf();
}