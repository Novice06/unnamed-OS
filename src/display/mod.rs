use core::fmt::{self, Write};
use crate::spinlock::SpinLock;

pub struct Serial;

impl Serial {
    fn write_string(&self, string: &str) {
        for byte in string.bytes() {
            unsafe {crate::SERIAL_putc(byte)};
        }
    }
}

impl Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

pub static SERIAL: SpinLock<Serial> = SpinLock::new(Serial);

pub fn _print(args: fmt::Arguments) {
    let _ = SERIAL.lock().write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::display::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n");
    };

    ($($arg:tt)*) => {
        $crate::print!("{}\n", format_args!($($arg)*))
    };
}