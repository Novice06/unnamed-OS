use core::{cell::UnsafeCell, ops::{Deref, DerefMut}, sync::atomic::{AtomicU64, Ordering::{AcqRel, Acquire, Relaxed, Release}}};

const WRITE_BIT: u64 = 1 << 59;

pub struct RwLock<T> {
    rw_count: AtomicU64,
    data: UnsafeCell<T>
}

unsafe impl<T> Send for RwLock<T> {}
unsafe impl<T> Sync for RwLock<T> {}

impl<T> RwLock<T> {
    pub const fn new(data: T) -> Self {
        Self { rw_count: AtomicU64::new(0), data: UnsafeCell::new(data) }
    }

    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        loop {
            let current = self.rw_count.load(Relaxed);
            if current & WRITE_BIT == 0 {
                if self.rw_count.compare_exchange(current, current + 1, AcqRel, Acquire).is_ok() {break;}
            }

            core::hint::spin_loop();
        }

        RwLockReadGuard {
            lock: self
        }
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        loop {
            if self.rw_count.load(Relaxed) == 0 {
                if self.rw_count.compare_exchange(0, WRITE_BIT, AcqRel, Acquire).is_ok() {break;}
            }

            core::hint::spin_loop();
        }

        RwLockWriteGuard {
            lock: self
        }
    }
}

pub struct RwLockReadGuard<'a, T> {
    lock: &'a RwLock<T>
}

impl<T> Deref for RwLockReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe {
            & *self.lock.data.get()
        }
    }
}

impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.rw_count.fetch_sub(1, Release);
    }
}

pub struct RwLockWriteGuard<'a, T> {
    lock: &'a RwLock<T>
}

impl<T> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe {
            & *self.lock.data.get()
        }
    }
}

impl<T> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *self.lock.data.get()
        }
    }
}

impl<T> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.rw_count.store(0, Release);
    }
}