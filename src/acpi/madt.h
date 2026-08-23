#pragma once

#include <stdint.h>

typedef struct
{
    uint8_t type;
    uint8_t length;
}__attribute__ ((packed)) RecordHeader_t;

typedef struct
{
    RecordHeader_t header;
    uint8_t io_pic_id;
    uint8_t reserved;
    uint32_t io_pic_addr;
    uint32_t global_SI_base;
}__attribute__ ((packed)) IO_pic_t;

typedef struct
{
    RecordHeader_t header;
    uint8_t bus_source;
    uint8_t irq_source;
    uint32_t global_SI;
    uint16_t flags;
}__attribute__ ((packed)) IO_pic_IS_override_t;

typedef struct
{   
    int io_apic_count;
    IO_pic_t io_apic[32];

    int is_override_count;
    IO_pic_IS_override_t is_override[32];
}IO_pic_info_t;


void madt_parse(void* table_base);
uint32_t MADT_getLocalApicAddr();
IO_pic_info_t* MADT_getIOApicInfo();