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

#include <mm/rust_interface.h>

static struct limine_memmap_entry contiguous_entries[256];

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

    struct memory_map contiguous = {
        .rev = memmap_request.response->revision,
        .count = memmap_request.response->entry_count,
        .contiguous_entries = contiguous_entries,
    };

    for(uint64_t i = 0; i < memmap_request.response->entry_count; i++)
        memcpy(&contiguous_entries[i], memmap_request.response->entries[i], sizeof(struct limine_memmap_entry));

    PHYSMEM_init(contiguous, hhdm_request.response->offset);


    if(!ACPI_parse(rsdp_request.response->address))
        SERIAL_printf("cannot parse acpi tables\n");

    // LAPIC_init();
    // IOAPIC_init();

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