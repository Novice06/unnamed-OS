#include <stdint.h>

#include <mm/rust_interface.h>

#include <utils/utils.h>

#include "gdt.h"

typedef struct{
    uint16_t limit_low;                 // limit (bits 0-15)
    uint16_t base_low;                  // base (bits 0-15)
    uint8_t base_middle;                // base (bits 16-23)
    uint8_t access_byte;                // access
    uint8_t highLimit_flags;            // limit (bits 16-19) | flags
    uint8_t base_high;                  // base (bits 24-31)
} __attribute__((packed)) Gdt_entry;

typedef struct{
    uint16_t size;
    Gdt_entry* offset;
}__attribute__((packed)) Gdt_descriptor;

#define GDT_ENTRY(base, limit, access, flags) {             \
    limit & 0xffff,                                         \
    base & 0xffff,                                          \
    ((base >> 16) & 0xff),                                  \
    access,                                                 \
    ((limit >> 16) & 0xf) | (flags << 4),                   \
    (base >> 24) & 0xff                                     \
}

// flat memory model (mandatory on 64bit)
Gdt_entry GDT[] = {
    GDT_ENTRY(0, 0, 0, 0),

    // kernel code and data segment
    GDT_ENTRY(0, 0xFFFFF, 0x9A, 0xA),
    GDT_ENTRY(0, 0xFFFFF, 0x92, 0xC),

    // user data segment
    GDT_ENTRY(0, 0xFFFFF, 0xF2, 0xC),
    // user code segement
    GDT_ENTRY(0, 0xFFFFF, 0xFA, 0xA),
};

Gdt_descriptor GDT_desc = {
    .offset = GDT,
    .size = sizeof(GDT) - 1,
};

extern void gdt_flush(Gdt_descriptor* descriptor);

void GDT_init_early()
{
    gdt_flush(&GDT_desc);
}

extern void TSS_flush(uint8_t gdt_index);

void write_tss(Gdt_entry *g, struct Tss *tss)
{
    uint64_t base = (uint64_t)tss;
    uint32_t limit = sizeof(struct Tss) - 1;

    g[0].limit_low = limit & 0xFFFF;
    g[0].base_low = base & 0xFFFF;
    g[0].base_middle = (base >> 16) & 0xFF;

    g[0].access_byte = 0x89;

    g[0].highLimit_flags = (limit >> 16) & 0x0F;
    g[0].base_high = (base >> 24) & 0xFF;

    // this second enty is only there to write the remaining 32bits address of the tss
    g[1].limit_low = (base >> 32) & 0xFFFF;
    g[1].base_low = (base >> 48) & 0xFFFF;

    // other bits must be 0
    g[1].base_middle = 0;
    g[1].access_byte = 0;
    g[1].highLimit_flags = 0;
    g[1].base_high = 0;

    memset(tss, 0, sizeof(struct Tss));

    tss->iopb = sizeof(struct Tss);
}

Gdt_entry GDT_model[] = {
        GDT_ENTRY(0, 0, 0, 0),

        // kernel code and data segment
        GDT_ENTRY(0, 0xFFFFF, 0x9A, 0xA),
        GDT_ENTRY(0, 0xFFFFF, 0x92, 0xC),

        // user data segment
        GDT_ENTRY(0, 0xFFFFF, 0xF2, 0xC),
        // user code segement
        GDT_ENTRY(0, 0xFFFFF, 0xFA, 0xA),

        // TSS (2 entries)
        GDT_ENTRY(0, 0, 0, 0),
        GDT_ENTRY(0, 0, 0, 0)
    };

struct Tss* GDT_init_core() {
    
    Gdt_entry* local_GDT = kmalloc(sizeof(GDT_model));
    memcpy(local_GDT, GDT_model, sizeof(GDT_model));

    Gdt_descriptor descriptor = {
        .offset = local_GDT,
        .size = sizeof(GDT_model) - 1,
    };

    struct Tss* local_TSS = kmalloc(sizeof(struct Tss));

    write_tss(&local_GDT[5], local_TSS);
    gdt_flush(&descriptor);
    TSS_flush(5);

    return local_TSS;
}