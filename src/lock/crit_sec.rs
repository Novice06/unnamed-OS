use crate::{disable_interrupts, enable_interrupts, get_rflags};

pub struct CriticalGuard {
    was_enabled: bool,  // interrupt state
}

impl CriticalGuard {
    pub fn enter_critical_section() -> Self {
        let was_enabled = unsafe {
            let rflags = get_rflags();

            disable_interrupts();

            (rflags & (1 << 9)) != 0
        };

        Self { was_enabled }
    }

    pub fn with<F, T>(critical: F) -> T 
    where
        F: FnOnce() -> T
    {
        let _guard = Self::enter_critical_section();
        critical()
    }
}

impl Drop for CriticalGuard {
    fn drop(&mut self) {
        if self.was_enabled {
            unsafe { enable_interrupts() };
        }
    }
}