#include <stdint.h>
#include <stddef.h>

#include <cpu/gdt.h>
#include <cpu/idt.h>
#include <cpu/isr.h>

#include "limine.h"
#include "serial.h"
#include "utils.h"

__attribute__((used, section(".limine_requests")))
static volatile uint64_t limine_base_revision[] = LIMINE_BASE_REVISION(6);

__attribute__((used, section(".limine_requests")))
static volatile struct limine_framebuffer_request framebuffer_request = {
    .id = LIMINE_FRAMEBUFFER_REQUEST_ID,
    .revision = 0,
};

__attribute__((used, section(".limine_requests_start")))
static volatile uint64_t limine_start_marker = LIMINE_REQUESTS_START_MARKER;

__attribute__((used, section(".limine_requests_end")))
static volatile uint64_t limine_end_marker = LIMINE_REQUESTS_END_MARKER;

void kmain()
{
    if(!LIMINE_BASE_REVISION_SUPPORTED(limine_base_revision))
        hcf();

    if(framebuffer_request.response == NULL || framebuffer_request.response->framebuffer_count < 1)
        hcf();

    SERIAL_init();

    GDT_init();
    IDT_init();
    ISR_init();

    int test = 1/0;

    // Fetch the first framebuffer.
    struct limine_framebuffer *framebuffer = framebuffer_request.response->framebuffers[0];

    SERIAL_printf("framebuffer at 0x%x", framebuffer);

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