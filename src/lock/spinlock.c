#include <stdint.h>

#include "spinlock.h"

void* spinlock_acquire(Spinlock* lock)
{
    while (__atomic_test_and_set(&lock->state, __ATOMIC_ACQUIRE)) __builtin_ia32_pause();
    
    return lock->data;
}

void spinlock_release(Spinlock *lock)
{
    __atomic_clear(&lock->state, __ATOMIC_RELEASE);
}