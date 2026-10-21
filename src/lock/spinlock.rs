use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

use crate::lock::crit_sec::CriticalGuard;

pub struct SpinLockIrqSave<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>
}

unsafe impl<T> Send for SpinLockIrqSave<T> {}
unsafe impl<T> Sync for SpinLockIrqSave<T> {}

impl<T> SpinLockIrqSave<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data)
        }
    }

    pub fn lock(&self) -> SpinLockIrqSaveGuard<'_, T> {
        let guard = CriticalGuard::enter_critical_section();
        while self.lock.swap(true, Ordering::Acquire) {
            core::hint::spin_loop();
        }

        SpinLockIrqSaveGuard { lock: &self, _critical_guard: guard }
    }
}

pub struct SpinLockIrqSaveGuard<'a, T> {
    lock: &'a SpinLockIrqSave<T>,
    _critical_guard: CriticalGuard,
}

impl<T> Deref for SpinLockIrqSaveGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe {
            &*self.lock.data.get()
        }
    }
}

impl<T> DerefMut for SpinLockIrqSaveGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *self.lock.data.get()
        }
    }
}

impl<T> Drop for SpinLockIrqSaveGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
    }
}