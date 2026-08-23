#include <stdint.h>

#include <display/serial.h>
#include <utils/utils.h>

#include "acpi.h"
#include "madt.h"

typedef struct 
{
    AcpiHeader_t header;
    uint32_t localApicAddr;
    uint32_t flags;
} __attribute__ ((packed)) MADT_t;

static MADT_t MADT;
static IO_pic_info_t io_apic_info;

void madt_parse(void* table_base)
{
    memcpy(&MADT, table_base, sizeof(MADT_t));


    RecordHeader_t* header = (RecordHeader_t*)((uint64_t)table_base + sizeof(MADT_t));

    while((uint64_t)header < (MADT.header.length + (uint64_t)table_base))
    {
        switch (header->type)
        {
        case 1:
            IO_pic_t* io_pic = (IO_pic_t*)header;
            memcpy(&io_apic_info.io_apic[io_apic_info.io_apic_count++], io_pic, sizeof(IO_pic_t));
            break;

        case 2:
            IO_pic_IS_override_t* io_pic_override = (IO_pic_IS_override_t*)header;
            memcpy(&io_apic_info.is_override[io_apic_info.is_override_count++], io_pic_override, sizeof(IO_pic_IS_override_t));
            break;
        
        default:
            SERIAL_printf("this header is of type: %x\n", header->type);
            break;
        }

        header = (RecordHeader_t*)((uint64_t)header + header->length);
    }
}

IO_pic_info_t* MADT_getIOApicInfo()
{
    return &io_apic_info;
}

uint32_t MADT_getLocalApicAddr()
{
    return MADT.localApicAddr;
}