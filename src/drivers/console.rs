use crate::{drivers::Driver};

pub struct Console;

impl Driver for Console {
    fn dtype(&self) -> super::DeviceKind {
        super::DeviceKind::Char
    }

    fn read(&self, _buf: &mut [u8]) -> Result<usize, crate::vfs::VfsError> {
        Ok(0)
    }

    fn write(&self, buf: &[u8]) -> Result<usize, crate::vfs::VfsError> {
        
        for chunk in buf.chunks(256) {
            crate::display::SERIAL.lock().write(chunk);
        }

        Ok(buf.len())

    }
}