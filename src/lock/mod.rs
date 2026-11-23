use core::{mem::ManuallyDrop, ops::{Deref, DerefMut}};

pub mod spinlock;
pub mod rwlock;
pub mod crit_sec;
pub mod mutex;

pub struct Condom<T, F>
where F: FnOnce(T)
{
    data: ManuallyDrop<T>,
    destroyer: ManuallyDrop<F>
}

impl<T, F> Condom<T, F> 
where F: FnOnce(T)
{
    pub fn put(data: T, destroyer: F) -> Self{
        Self {
            data: ManuallyDrop::new(data),
            destroyer: ManuallyDrop::new(destroyer)
        }
    }

    pub fn remove(mut self) -> T {
        let data = unsafe {
            ManuallyDrop::drop(&mut self.destroyer);
            ManuallyDrop::take(&mut self.data)
        };

        core::mem::forget(self);
        data
    }
}

impl<T, F> Drop for Condom<T, F> 
where F: FnOnce(T)
{
    fn drop(&mut self) {

        let (data, destroyer) = unsafe {
            (
                ManuallyDrop::take(&mut self.data),
                ManuallyDrop::take(&mut self.destroyer)
            )
        };

        destroyer(data);
    }
}

impl<T, F> Deref for Condom<T, F> 
where F: FnOnce(T)
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T, F> DerefMut for Condom<T, F> 
where F: FnOnce(T)
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}