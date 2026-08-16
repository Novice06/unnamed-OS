#include <stdint.h>

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

    // user code and data segment
    GDT_ENTRY(0, 0xFFFFF, 0xF2, 0xC),
    GDT_ENTRY(0, 0xFFFFF, 0xFA, 0xA),
};

Gdt_descriptor GDT_desc = {
    .offset = GDT,
    .size = sizeof(GDT) - 1,
};

extern void gdt_flush(Gdt_descriptor* descriptor);
void GDT_init()
{
    gdt_flush(&GDT_desc);
}
