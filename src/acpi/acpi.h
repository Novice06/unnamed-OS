#pragma once

#include <stdbool.h>

typedef struct
{
    char signature[4];
    uint32_t length;
    uint8_t revision;
    uint8_t checksum;
    uint8_t oem[6];
    uint8_t oemTableId[8];
    uint32_t oemRevision;
    uint32_t creatorId;
    uint32_t creatorRevision;
} __attribute__ ((packed)) AcpiHeader_t;

bool ACPI_parse(void* sdp);