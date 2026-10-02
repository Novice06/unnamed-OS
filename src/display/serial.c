#include <stdarg.h>

#include <spinlock.h>

#include <utils/utils.h>
#include "formatter.h"

#define PORT 0x3f8          // COM1

static Spinlock LOCK;

int is_transmit_empty() {
   return inb(PORT + 5) & 0x20;
}

void SERIAL_putc(char c)
{
    while (is_transmit_empty() == 0);

    outb(PORT, c);
}

void SERIAL_puts(char* str)
{
    spinlock_acquire(&LOCK);

    for(; *str != '\0'; str++)
        SERIAL_putc(*str);

    spinlock_release(&LOCK);
}

void SERIAL_printf(char* fmt, ...)
{
    spinlock_acquire(&LOCK);

    va_list args;
    va_start(args, fmt);

    printf(SERIAL_putc, fmt, args);

    va_end(args);

    spinlock_release(&LOCK);
}

void SERIAL_init()
{
    outb(PORT + 1, 0x00);
    outb(PORT + 3, 0x80);
    outb(PORT + 0, 0x03);
    outb(PORT + 1, 0x00);
    outb(PORT + 3, 0x03);
    outb(PORT + 2, 0xC7);
    outb(PORT + 4, 0x0B);

    SERIAL_puts("\n\n\n");
}