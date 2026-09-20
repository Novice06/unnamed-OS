#include <stdint.h>

#include <utils/utils.h>

#include "acpi.h"
#include "fadt.h"
#include "timer.h"

void fadt_parse(void* table_base) 
{
    struct FADT *FADT_info = (struct FADT *) table_base;
    init_timer(FADT_info);
}