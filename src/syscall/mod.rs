pub mod fs;
pub mod proc;

use alloc::{string::String, vec::Vec};

use crate::{TrapFrame, disable_interrupts, enable_interrupts, lock::crit_sec::CriticalGuard, mm::{ VirtAddr, paging, virtual_area}, smp::CpuInfo, syscall::{fs::{sys_close, sys_openat, sys_pipe, sys_read, sys_write, sys_writev}, proc::{sys_arch_prctl, sys_execve, sys_exit_group, sys_fork, sys_set_tid_address, sys_wait4}}, task::TaskError, vfs::VfsError};

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Errno {
    EPERM = 1,           // Operation not permitted
    ENOENT = 2,          // No such file or directory
    ESRCH = 3,           // No such process
    EINTR = 4,           // Interrupted system call
    EIO = 5,             // I/O error
    ENXIO = 6,           // No such device or address
    E2BIG = 7,           // Argument list too long
    ENOEXEC = 8,         // Exec format error
    EBADF = 9,           // Bad file descriptor
    ECHILD = 10,         // No child processes
    EAGAIN = 11,         // Try again (also EWOULDBLOCK)
    ENOMEM = 12,         // Out of memory
    EACCES = 13,         // Permission denied
    EFAULT = 14,         // Bad address
    ENOTBLK = 15,        // Block device required
    EBUSY = 16,          // Device or resource busy
    EEXIST = 17,         // File exists
    EXDEV = 18,          // Cross-device link
    ENODEV = 19,         // No such device
    ENOTDIR = 20,        // Not a directory
    EISDIR = 21,         // Is a directory
    EINVAL = 22,         // Invalid argument
    ENFILE = 23,         // File table overflow (system wide)
    EMFILE = 24,         // Too many open files (per process)
    ENOTTY = 25,         // Not a typewriter
    ETXTBSY = 26,        // Text file busy
    EFBIG = 27,          // File too large
    ENOSPC = 28,         // No space left on device
    ESPIPE = 29,         // Illegal seek
    EROFS = 30,          // Read-only file system
    EMLINK = 31,         // Too many links
    EPIPE = 32,          // Broken pipe
    EDOM = 33,           // Math argument out of domain
    ERANGE = 34,         // Result too large
    EDEADLK = 35,        // Resource deadlock would occur
    ENAMETOOLONG = 36,   // File name too long
    ENOLCK = 37,         // No record locks available
    ENOSYS = 38,         // Function not implemented
    ENOTEMPTY = 39,      // Directory not empty
    ELOOP = 40,          // Too many symbolic links encountered
    ENODATA = 61,        // No data available
    ETIME = 62,          // Timer expired
    EOVERFLOW = 75,      // Value too large for defined data type
    EILSEQ = 84,         // Illegal byte sequence
    ENOTSOCK = 88,       // Socket operation on non-socket
    EMSGSIZE = 90,       // Message too long
    EPROTONOSUPPORT = 93, // Protocol not supported
    EOPNOTSUPP = 95,    // Operation not supported (also ENOTSUP)
    EAFNOSUPPORT = 97,   // Address family not supported
    EADDRINUSE = 98,     // Address already in use
    EADDRNOTAVAIL = 99,  // Cannot assign requested address
    ENETUNREACH = 101,   // Network is unreachable
    ECONNABORTED = 103,  // Software caused connection abort
    ECONNRESET = 104,    // Connection reset by peer
    ENOBUFS = 105,       // No buffer space available
    EISCONN = 106,       // Transport endpoint is already connected
    ENOTCONN = 107,      // Transport endpoint is not connected
    ETIMEDOUT = 110,     // Connection timed out
    ECONNREFUSED = 111,  // Connection refused
    EHOSTUNREACH = 113,  // No route to host
    EALREADY = 114,      // Operation already in progress
    EINPROGRESS = 115,   // Operation now in progress
    ESTALE = 116,        // Stale file handle
    EDQUOT = 122,        // Quota exceeded
    ECANCELED = 125,     // Operation Canceled
}

impl Errno {
    // aliases: two names, one value (an enum cannot hold duplicate discriminants)
    pub const EWOULDBLOCK: Errno = Errno::EAGAIN;
    pub const EDEADLOCK: Errno = Errno::EDEADLK;
    pub const ENOTSUP: Errno = Errno::EOPNOTSUPP;

    /// the value a failing syscall puts in rax: -errno
    pub const fn to_ret(self) -> i64 {
        -(self as i32 as i64)
    }
}

// exhaustive on purpose: adding a VfsError variant is a compile error here,
// so a new error can never silently become EINVAL
impl From<VfsError> for Errno {
    fn from(e: VfsError) -> Self {
        match e {
            VfsError::NoRoot | VfsError::NotFound => Errno::ENOENT,
            VfsError::NotDirectory => Errno::ENOTDIR,
            VfsError::IsDirectory => Errno::EISDIR,
            VfsError::NoSpace => Errno::ENOSPC,
            VfsError::EntryExist => Errno::EEXIST,
            VfsError::InvalidName => Errno::EINVAL,
            VfsError::Busy => Errno::EBUSY,
            VfsError::NotSupported => Errno::ENOSYS,
            VfsError::BrokenPipe => Errno::EPIPE,
        }
    }
}

impl From<TaskError> for Errno {
    fn from(value: TaskError) -> Self {
        match value {
            TaskError::OutOfMemory => Self::ENOMEM
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn SYSCALL_dispatcher(frame: *mut TrapFrame) {
    unsafe {enable_interrupts();}

    let frame = unsafe {
        &mut *frame
    };

    if let Some(Some(sys)) = SYSCALL_TABLE.get(frame.rax as usize) {
        sys(frame);
    } else {
        // crate::warn!("unimplemented syscall {}", frame.rax);
        frame.rax = Errno::ENOSYS.to_ret() as u64;
    };

    unsafe {disable_interrupts();}
}

const NUM_SYSCALLS: usize = 512;
const MAX_STRING_LEN: usize = 0x1000;
const MAX_RW_COUNT: usize = 1024 * 1024; // 1 MiB

macro_rules! return_errno {
    ($frame:expr, $errno:expr) => {{
        $frame.rax = $errno.to_ret() as u64;
        return;
    }};
}

type SyscallFn = fn(frame: &mut TrapFrame);

static SYSCALL_TABLE: [Option<SyscallFn>; NUM_SYSCALLS] = {
    let mut table: [Option<SyscallFn>; NUM_SYSCALLS] = [None; NUM_SYSCALLS];

    table[257] = Some(openat);
    table[2] = Some(open);
    table[3] = Some(close);
    table[0] = Some(read);
    table[1] = Some(write);
    table[20] = Some(writev);
    table[61] = Some(wait4);
    table[57] = Some(fork);
    table[59] = Some(execve);
    table[16] = Some(ioctl);
    table[158] = Some(arch_prctl);
    table[218] = Some(set_tid_address);
    table[231] = Some(exit_group);
    table[39] = Some(getpid);
    table[22] = Some(pipe);
    table[293] = Some(pipe2);

    table
};

fn access_ok(ptr: u64, len: u64) -> bool {

    if ptr < virtual_area::USER_SPACE_START {
        return false;
    }

    if ptr >= virtual_area::USER_SPACE_END {
        return false;
    }
 
    if ptr.checked_add(len).is_none() {
        return false;
    }

    if ptr + len > virtual_area::USER_SPACE_END {
        return false;
    }

    return true;
}

fn copy_from_user(dst: *mut u8, src_user: u64, len: usize) -> Result<(), Errno> {

    let current_process = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());
    let addr_space = current_process.addr_space.lock();

    if !access_ok(src_user, len as u64) {
        return Err(Errno::EFAULT);
    }

    let mut validated_len = 0u64;
    while (validated_len as usize) <= len {
        let ptr = src_user + validated_len;
        let page = ptr & !(0xFFF);
        let offset = ptr & 0xFFF;

        addr_space
        .find(VirtAddr(page))
        .ok_or(Errno::EFAULT)?;

        let Some(_) = paging::get_physical(addr_space.pml4, VirtAddr(page)) else {
            panic!("something is wrong with address space but with demand paging we should map this page");
            // return Err(Errno::EFAULT);
        };

        validated_len += 0x1000 - offset;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(src_user as *mut u8, dst, len);
    }

    return Ok(());
}

fn copy_to_user(dest_user: u64, src: *const u8, len: usize) -> Result<(), Errno> {
    let current_process = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());
    let addr_space = current_process.addr_space.lock();

    if !access_ok(dest_user, len as u64) {
        return Err(Errno::EFAULT);
    }

    let mut validated_len = 0u64;
    while (validated_len as usize) <= len {
        let ptr = dest_user + validated_len;
        let page = ptr & !(0xFFF);
        let offset = ptr & 0xFFF;

        addr_space
        .find(VirtAddr(page))
        .ok_or(Errno::EFAULT)?
        .can_write()
        .ok_or(Errno::EFAULT)?;

        let Some(_) = paging::get_physical(addr_space.pml4, VirtAddr(page)) else {
            panic!("something is wrong with address space but with demand paging we should map this page");
            // return Err(Errno::EFAULT);
        };

        validated_len += 0x1000 - offset;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(src, dest_user as *mut u8, len);
    }

    return Ok(());
}

fn copy_string_from_user(src_user: u64, max_len: u64) -> Result<String, Errno> {
    let current_process = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());
    let addr_space = current_process.addr_space.lock();

    let mut s = Vec::new();
    let mut bytes_read = 0u64;

    while bytes_read < max_len {
        let Some(ptr) = src_user.checked_add(bytes_read) else {
            return Err(Errno::EFAULT);
        };
        let page = ptr & !(0xFFF);
        let offset = ptr & 0xFFF;
        let to_read = (0x1000 - offset).min(max_len - bytes_read);

        if !access_ok(ptr, to_read) {
            return Err(Errno::EFAULT); // -EFAULT
        }

        addr_space
        .find(VirtAddr(page))
        .ok_or(Errno::EFAULT)?;

        let Some(_) = paging::get_physical(addr_space.pml4, VirtAddr(page)) else {
            panic!("something is wrong with address space but with demand paging we should map this page");
            // return Err(Errno::EFAULT);
        };

        for i in 0..to_read {
            let byte = unsafe { *((ptr + i) as *const u8) };

            if byte == 0 {  // end of string !

                let ret = String::from_utf8(s).map_err(|_| Errno::EILSEQ)?;
                return Ok(ret);
            }

            s.push(byte);
        }

        bytes_read += to_read;
    }


    Err(Errno::ENAMETOOLONG)
}

fn openat(frame: &mut TrapFrame) {

    let dirfd = frame.rdi as usize;
    let path = match copy_string_from_user(frame.rsi, MAX_STRING_LEN as u64) {
        Ok(path) => path,
        Err(e) => return_errno!(frame, e),
    };
    let flags = frame.rdx as u32;
    let mode = frame.r10 as u32;

    match sys_openat(dirfd, &path, flags, mode) {
        Ok(ret) => frame.rax = ret as u64,
        Err(e) => frame.rax = e.to_ret() as u64,
    }
}

fn open(frame: &mut TrapFrame) {
    let path = match copy_string_from_user(frame.rdi, MAX_STRING_LEN as u64) {
        Ok(path) => path,
        Err(e) => return_errno!(frame, e),
    };
    let flags = frame.rsi as u32;
    let mode = frame.rdx as u32;

    match sys_openat(fs::AT_FDCWD as usize, &path, flags, mode) {
        Ok(ret) => frame.rax = ret as u64,
        Err(e) => frame.rax = e.to_ret() as u64,
    }
}

fn close(frame: &mut TrapFrame) {
    if let Err(e) = sys_close(frame.rdi as usize) {
        frame.rax = e.to_ret() as u64;
    } else {
        frame.rax = 0;
    }
}

fn read(frame: &mut TrapFrame) {

    let fd = frame.rdi as usize;
    let user_buffer = frame.rsi;
    let count = (frame.rdx as usize).min(MAX_RW_COUNT);

    match sys_read(fd, user_buffer, count) {
        Ok(ret) => frame.rax = ret as u64,
        Err(e) => frame.rax = e.to_ret() as u64,
    }
}

fn write(frame: &mut TrapFrame) {
    let fd = frame.rdi as usize;
    let count = (frame.rdx as usize).min(MAX_RW_COUNT);

    let buffer = {
        let mut buffer:Vec<u8> = Vec::new();
        if let Some(_) = buffer.try_reserve(count).err() {
            return_errno!(frame, Errno::ENOMEM)
        };
        buffer.resize(count, 0);

        if let Some(e) = copy_from_user(buffer.as_mut_ptr(), frame.rsi, buffer.len()).err() {
            return_errno!(frame, e)
        }

        buffer
    };
    
    match sys_write(fd, &buffer) {
        Ok(ret) => frame.rax = ret as u64,
        Err(e) => frame.rax = e.to_ret() as u64,
    }
}

fn writev(frame: &mut TrapFrame) {

    #[repr(C)]
    #[derive(Default, Debug)]
    struct Iovec {
        iov_base: *mut u8,
        iov_len: usize,
    }

    let fd = frame.rdi as usize;
    let iovcnt = if frame.rdx > 1024 {
        return_errno!(frame, Errno::EINVAL)
    } else {
        frame.rdx
    };

    let iovs = {
        let mut iovs: Vec<Iovec> = Vec::with_capacity(iovcnt as usize);
        iovs.resize_with(iovcnt as usize, || unsafe {
            core::mem::zeroed()
        });

        let iov_slice = unsafe {
            core::slice::from_raw_parts_mut(
                iovs.as_mut_ptr() as *mut u8,
                iovs.len() * core::mem::size_of::<Iovec>(),
            )
        };

        if let Some(e) = copy_from_user(iov_slice.as_mut_ptr(), frame.rsi, iov_slice.len()).err() {
            return_errno!(frame, e)
        }

        iovs
    };

    let mut buffers: Vec<Vec<u8>> = Vec::new();
    let mut total_write = 0;
    for iov in iovs {

        if iov.iov_len <= 0 { continue; }
    
        let to_write = (MAX_RW_COUNT - total_write).min(iov.iov_len);

        let mut buffer: Vec<u8> = Vec::with_capacity(to_write);
        buffer.resize_with(to_write, || unsafe {
            core::mem::zeroed()
        });

        if let Some(e) = copy_from_user(buffer.as_mut_ptr(), iov.iov_base as u64, to_write).err() {
            return_errno!(frame, e)
        }

        buffers.push(buffer);

        total_write += to_write;
        if total_write >= MAX_RW_COUNT { break; }
    }

    match sys_writev(fd, buffers) {
        Ok(ret) => frame.rax = ret as u64,
        Err(e) => frame.rax = e.to_ret() as u64,
    }

}

fn pipe(frame: &mut TrapFrame) {
    match sys_pipe(0) {
        Ok((read, write)) => {
            let fds = [read as i32, write as i32];
            if let Some(e) = copy_to_user(frame.rdi, fds.as_ptr() as *const u8, fds.len() * core::mem::size_of::<i32>()).err() {
                let _ = sys_close(read);
                let _ = sys_close(write);
                return_errno!(frame, e)
            }
        },

        Err(e) => return_errno!(frame, e)
    }

    frame.rax = 0;
}

fn pipe2(frame: &mut TrapFrame) {
    match sys_pipe(frame.rsi as u32) {
        Ok((read, write)) => {
            let fds = [read as i32, write as i32];
            if let Some(e) = copy_to_user(frame.rdi, fds.as_ptr() as *const u8, fds.len() * core::mem::size_of::<i32>()).err() {
                let _ = sys_close(read);
                let _ = sys_close(write);
                return_errno!(frame, e)
            }
        },

        Err(e) => return_errno!(frame, e)
    }

    frame.rax = 0;
}

fn wait4(frame: &mut TrapFrame) {
    let pid = frame.rdi as isize;
    let mut status: u32 = 0;
    let options = frame.rdx as i32;
    let mut usage = proc::Rusage::default();

    match sys_wait4(pid, &mut status, options, &mut usage) {
        Ok(id) => {
            if frame.rsi != 0 {
                if let Some(e) = copy_to_user(frame.rsi, core::ptr::from_ref(&status) as *const u8, core::mem::size_of_val(&status)).err() {
                    return_errno!(frame, e)
                }
            }
            if frame.r10 != 0 {
                if let Some(e) = copy_to_user(frame.r10, core::ptr::from_ref(&usage) as *const u8, core::mem::size_of_val(&usage)).err() {
                    return_errno!(frame, e)
                }
            }
            
            frame.rax = id as u64;
        },

        Err(e) => frame.rax = e.to_ret() as u64
    }
}

fn getpid(frame: &mut TrapFrame) {
    frame.rax = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap()).process.pid as u64;
}

fn fork(frame: &mut TrapFrame) {
    match sys_fork(frame) {
        Ok(id) => frame.rax = id as u64,
        Err(e) => frame.rax = e.to_ret() as u64,
    }
}

fn execve(frame: &mut TrapFrame) {
    let path=  match copy_string_from_user(frame.rdi, MAX_STRING_LEN as u64) {
        Ok(p) => p,
        Err(e) => return_errno!(frame, e)
    };

    let argv = {
        let mut ptrs: Vec<u64> = Vec::new();
        let mut early_break = false;
        for i in 0..1024 {
            let mut ptr = 0u64;
            let Some(src_ptr) = frame.rsi.checked_add(i * core::mem::size_of_val(&ptr) as u64) else {
                return_errno!(frame, Errno::EFAULT)
            };
            if let Some(e) = copy_from_user(core::ptr::from_mut(&mut ptr) as *mut u8, src_ptr, core::mem::size_of_val(&ptr)).err() {
                return_errno!(frame, e)
            }

            if (ptr as *const u8).is_null() {
                early_break = true;
                break;
            }

            ptrs.push(ptr);
        }

        if !early_break {
            return_errno!(frame, Errno::E2BIG)
        }

        let mut argv: Vec<String> = Vec::new();
        for s in ptrs {
            let arg = match copy_string_from_user(s, MAX_STRING_LEN as u64) {
                Ok(arg) => arg,
                Err(e) => return_errno!(frame, e),
            };

            argv.push(arg);
        }

        argv
    };

    let envp = {
        let mut ptrs: Vec<u64> = Vec::new();
        let mut early_break = false;
        for i in 0..1024 {
            let mut ptr = 0u64;
            let Some(src_ptr) = frame.rdx.checked_add(i * core::mem::size_of_val(&ptr) as u64) else {
                return_errno!(frame, Errno::EFAULT)
            };
            if let Some(e) = copy_from_user(core::ptr::from_mut(&mut ptr) as *mut u8, src_ptr, core::mem::size_of_val(&ptr)).err() {
                return_errno!(frame, e)
            }

            if (ptr as *const u8).is_null() {
                early_break = true;
                break;
            }

            ptrs.push(ptr);
        }

        if !early_break {
            return_errno!(frame, Errno::E2BIG)
        }

        let mut envp: Vec<String> = Vec::new();
        for s in ptrs {
            let arg = match copy_string_from_user(s, MAX_STRING_LEN as u64) {
                Ok(arg) => arg,
                Err(e) => return_errno!(frame, e),
            };

            envp.push(arg);
        }

        envp
    };

    match sys_execve(path, argv, envp) {
        Ok(_) => unreachable!("exceve returned on success"),
        Err(e) => return_errno!(frame, e),
    }
}

fn ioctl(frame: &mut TrapFrame) {
    return_errno!(frame, Errno::ENOTTY)
}

fn arch_prctl(frame: &mut TrapFrame) {
    let code = frame.rdi as i32;
    let addr = frame.rsi;

    match sys_arch_prctl(code, VirtAddr(addr)) {
        Ok(ret) if let Some(v) = ret => {
            if addr != 0 {
                if let Some(e) = copy_to_user(addr, core::ptr::from_ref(&v) as *const u8, core::mem::size_of_val(&v)).err() {
                    return_errno!(frame, e)
                }
            }
        }
        
        Ok(_) => (),

        Err(e) => return_errno!(frame, e)
    }

    frame.rax = 0;
}

fn set_tid_address(frame: &mut TrapFrame) {
    match sys_set_tid_address(VirtAddr(frame.rdi)) {
        Ok(tid) => frame.rax = tid as u64,
        Err(e) => return_errno!(frame, e)
    }
}

fn exit_group(frame: &mut TrapFrame) {
    sys_exit_group(frame.rdi as i32);
}