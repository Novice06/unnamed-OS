use alloc::{collections::VecDeque, sync::Arc, vec::Vec};

use crate::{ipc::IpcError, lock::{crit_sec::CriticalGuard, mutex::Mutex}, smp::CpuInfo, task::{Thread, ThreadState, scheduler}, vfs::{self, VfsError, Vnode}};

struct Pipe {
    buffer: VecDeque<u8>,

    reader_is_open: bool,
    writer_is_open: bool,

    waiting_readers: Vec<Arc<Thread>>,
    waiting_writers: Vec<Arc<Thread>>,
}

struct PipeConnector {
    pipe: Mutex<Pipe>
}

impl PipeConnector {
    const MAX_PIPE_SIZE: usize = 8 * 0x1000;
    const PIPE_BUF: usize = 0x1000;
    fn new() -> Self{
        Self { 
            pipe: Mutex::new(
                Pipe {
                    buffer: VecDeque::with_capacity(Self::MAX_PIPE_SIZE),
                    reader_is_open: true,
                    writer_is_open: true,
                    waiting_readers: Vec::new(),
                    waiting_writers: Vec::new()
                }
            )
        }
    }

    fn read(&self, buffer: &mut [u8]) -> Result<usize, IpcError> {
        if buffer.is_empty() { return Ok(0); }

        loop {
            let mut pipe = self.pipe.lock();
            let len = buffer.len().min(pipe.buffer.len());

            if len > 0 {
                let (first, second) = pipe.buffer.as_slices();

                if first.len() >= len {
                    buffer[..len].copy_from_slice(&first[..len]);
                } else {
                    let first_len = first.len();

                    buffer[..first_len].copy_from_slice(first);
                    buffer[first_len..len].copy_from_slice(&second[..len - first_len]);
                }

                pipe.buffer.drain(..len);
            }
            else if pipe.writer_is_open {
                let current_thread = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().clone());
                pipe.waiting_readers.push(current_thread.clone());

                CriticalGuard::with(move || {
                    current_thread.set_state(ThreadState::SLEEPING);
                    drop(pipe);
                });

                scheduler::yield_task();
                continue;
            }
            
            while let Some(writer) = pipe.waiting_writers.pop() {
                scheduler::wake_thread(writer);
            }

            return Ok(len);
        }
    }

    fn write(&self, buffer: &[u8]) -> Result<usize, IpcError> {
        if buffer.is_empty() { return Ok(0); }

        loop {
            let mut pipe = self.pipe.lock();

            if !pipe.reader_is_open {
                return Err(IpcError::BrokenPipe);
            }

            let len = buffer.len().min(Self::MAX_PIPE_SIZE - pipe.buffer.len());
            let minimum = buffer.len().min(Self::PIPE_BUF);

            if len >= minimum {
                pipe.buffer.extend(buffer[..len].iter());

                while let Some(reader) = pipe.waiting_readers.pop() {
                    scheduler::wake_thread(reader);
                }

                return Ok(len);
            }
            
            let current_thread = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().clone());
            pipe.waiting_writers.push(current_thread.clone());

            CriticalGuard::with(move || {
                current_thread.set_state(ThreadState::SLEEPING);
                drop(pipe);
            });

            scheduler::yield_task();
        }
    }
}

pub struct PipeReader {
    connector: Arc<PipeConnector>,
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        let mut pipe = self.connector.pipe.lock();
        pipe.reader_is_open = false;

        while let Some(writer) = pipe.waiting_writers.pop() {
            scheduler::wake_thread(writer);
        }
    }
}

impl Vnode for PipeReader {
    fn read(&self, _offset: u64, buffer: &mut [u8]) -> Result<usize, crate::vfs::VfsError> {
        self.connector.read(buffer).map_err(|e| {
            match e {
                IpcError::BrokenPipe => VfsError::BrokenPipe
            }
        })
    }

    fn write(&self, _offset: u64, _buffer: &[u8]) -> Result<usize, crate::vfs::VfsError> {
        Err(VfsError::NotSupported)
    }

    fn append(&self, _buffer: &[u8]) -> Result<u64, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn vtype(&self) -> crate::vfs::VnodeType {
        vfs::VnodeType::VPIPE
    }

    fn create(&self, _name: &str, _vtype: vfs::VnodeType, _mode: u32) -> Result<Arc<dyn Vnode>, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn lookup(&self, _name: &str) -> Result<Arc<dyn Vnode>, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn trunc(&self) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }
}

pub struct PipeWriter {
    connector: Arc<PipeConnector>
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        let mut pipe = self.connector.pipe.lock();
        pipe.writer_is_open = false;

        while let Some(reader) = pipe.waiting_readers.pop() {
            scheduler::wake_thread(reader);
        }
    }
}

impl Vnode for PipeWriter {
    fn read(&self, _offset: u64, _buffer: &mut [u8]) -> Result<usize, crate::vfs::VfsError> {
        Err(VfsError::NotSupported)
    }

    fn write(&self, _offset: u64, buffer: &[u8]) -> Result<usize, crate::vfs::VfsError> {
        self.connector.write(buffer).map_err(|e| {
            match e {
                IpcError::BrokenPipe => VfsError::BrokenPipe
            }
        })
    }

    fn append(&self, _buffer: &[u8]) -> Result<u64, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn vtype(&self) -> crate::vfs::VnodeType {
        vfs::VnodeType::VPIPE
    }

    fn create(&self, _name: &str, _vtype: vfs::VnodeType, _mode: u32) -> Result<Arc<dyn Vnode>, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn lookup(&self, _name: &str) -> Result<Arc<dyn Vnode>, VfsError> {
        Err(VfsError::NotSupported)
    }

    fn trunc(&self) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }
}

pub fn create() -> (PipeReader, PipeWriter) {
    let connector = Arc::new(PipeConnector::new());

    let reader = PipeReader {
        connector: connector.clone()
    };

    let writer = PipeWriter {
        connector
    };

    (reader, writer)
}