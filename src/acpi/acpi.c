#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#include <display/serial.h>
#include <utils/utils.h>
#include <utils/limine_requests.h>

#include "acpi.h"
#include "madt.h"

// structure for revision 2 (version 2.0+)

typedef struct
{
    char Signature[8];
    uint8_t Checksum;
    char OEMID[6];
    uint8_t Revision;
    uint32_t RsdtAddress;      // deprecated since version 2.0
}__attribute__ ((packed)) RSDPDescriptor_t;

typedef struct {
    RSDPDescriptor_t rsdp;

    uint32_t Length;
    uint64_t XsdtAddress;
    uint8_t ExtendedChecksum;
    uint8_t reserved[3];
} __attribute__ ((packed)) XSDPDescriptor_t;

typedef struct
{
    AcpiHeader_t header;
    uint64_t XSDT_addresses[];
}__attribute__ ((packed)) XSDT_t;

typedef struct
{
    AcpiHeader_t header;
    uint32_t RSDT_addresses[];
}__attribute__ ((packed)) RSDT_t;


bool validate_SDP(char *byte_array, size_t size) {
    uint32_t sum = 0;
    for(uint32_t i = 0; i < size; i++) {
        sum += byte_array[i];
    }
    return (sum & 0xFF) == 0;
}

bool parse_rsdp(RSDPDescriptor_t* rsdp)
{
    SERIAL_printf("it's rsdp, 0x%lx\n", rsdp);
    return validate_SDP((char*) rsdp, sizeof(RSDPDescriptor_t));
}

bool parse_xsdp(XSDPDescriptor_t* xsdp)
{
    if (!validate_SDP((char*) xsdp, sizeof(XSDPDescriptor_t))) return false;

    XSDT_t* tables = (XSDT_t*)(xsdp->XsdtAddress + hhdm_request.response->offset);

    int entry_count = (tables->header.length - sizeof(AcpiHeader_t)) / sizeof(uint64_t);
    for(int i = 0; i < entry_count; i++)
    {
        AcpiHeader_t* table = (AcpiHeader_t*)(tables->XSDT_addresses[i] + hhdm_request.response->offset);

        if (memcmp(table->signature, "APIC", 4) == 0) madt_parse(table);
    }

    return true;
}

bool ACPI_parse(void* sdp)
{
    RSDPDescriptor_t* rsdp = (RSDPDescriptor_t*)sdp;
    if(rsdp->Revision == 0) return parse_rsdp(sdp);
    else return parse_xsdp(sdp);
}