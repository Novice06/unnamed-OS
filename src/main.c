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

void test_lapic_timer(Registers* regs)
{
    SERIAL_printf("tick every 50ms\n");
    lapic_send_eoi();
}

void kmain_continue()
{
    // reclaime bootloader region
    // MEM_reclaim_region(contiguous, LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE);

    if(!ACPI_parse(rsdp_request.response->address))
        SERIAL_printf("cannot parse acpi tables\n");

    // reclaime acpi
    // MEM_reclaim_region(contiguous, LIMINE_MEMMAP_ACPI_RECLAIMABLE);

    LAPIC_init();
    IOAPIC_init();

    // test lapic timer
    ISR_registerNewHandler(0x20, test_lapic_timer);
    LAPIC_init_periodic_timer(0x20, 50);
    enable_interrupts();

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

    if((void*)rsdp_request.response->address == NULL)
        hcf();

    if((void*)hhdm_request.response->offset == NULL)
        hcf();

    if((void*)memmap_request.response->entries == NULL)
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

   uint64_t stack_top = MEM_init(contiguous, hhdm_request.response->offset, *executable_request.response);

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