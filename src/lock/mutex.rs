use core::{cell::UnsafeCell, ops::{Deref, DerefMut}, sync::atomic::{AtomicBool, Ordering}};

use alloc::{collections::VecDeque, sync::Arc};

use crate::{CpuInfo, lock::{crit_sec::CriticalGuard, spinlock::SpinLockIrqSave}, task::{Thread, ThreadState, scheduler}};

pub struct Mutex<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
    waiting_queue: SpinLockIrqSave<VecDeque<Arc<Thread>>>,
}

unsafe impl<T> Send for Mutex<T> {}
unsafe impl<T> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
            waiting_queue: SpinLockIrqSave::new(VecDeque::new()),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        if !self.lock.swap(true, Ordering::Acquire) {
            return MutexGuard { lock: &self };
        }

        self.lock_slow()
    }

    fn lock_slow(&self) -> MutexGuard<'_, T> {
        // this lock also serve as a means of synchronisation to avoid races situation like
        // a thread is put on the queue while simultaneously on another core 
        // the lock is being released (the thread would be stuck on the queue forever)
        let mut wait_queue = self.waiting_queue.lock();
        
        if self.lock.swap(true, Ordering::Acquire) {
            wait_queue.push_back(CriticalGuard::with(|| {
                let current_thread = CpuInfo::get_current().current_thread.as_ref().cloned().unwrap();
                current_thread.set_state(ThreadState::SLEEPING);

                current_thread
            }));

            drop(wait_queue);   // VERY IMPORTANT !!

            scheduler::yield_task();
        }

        MutexGuard { lock: &self }
    }
}

pub struct MutexGuard<'a, T> {
    lock: &'a Mutex<T>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe {
            &*self.lock.data.get()
        }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *self.lock.data.get()
        }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        let mut q = self.lock.waiting_queue.lock();

        if let Some(next_owner) = q.pop_front() {
            scheduler::wake_thread(next_owner);
        } else {
            self.lock.lock.store(false, Ordering::Release);
        }
    }
}