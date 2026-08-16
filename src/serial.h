#pragma once

void SERIAL_init();
void SERIAL_putc(char c);
void SERIAL_puts(char* str);
void SERIAL_printf(char* fmt, ...);