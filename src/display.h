#pragma once

typedef void (*PutC)(char c);
void printf(PutC putc, const char* fmt, va_list args);