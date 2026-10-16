#pragma once
#include <stdbool.h>

typedef struct 
{
    void* data;
    bool state;
}Spinlock;

void* spinlock_acquire(Spinlock* lock);
void spinlock_release(Spinlock *lock);