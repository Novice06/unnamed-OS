pub mod fs;
pub mod proc;

use crate::vfs::VfsError;

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
        }
    }
}