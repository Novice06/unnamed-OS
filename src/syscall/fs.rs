use alloc::sync::Arc;

use crate::{CpuInfo, lock::crit_sec::CriticalGuard, syscall::Errno, task::{FileDescriptor, OpenFile}, vfs::{self, VfsError, VnodeType}};

pub const AT_FDCWD: i32 = -100; // "relative to the current directory"

// access mode: the low 2 bits of the flags
pub const O_ACCMODE: u32 = 0o3;
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;

pub const O_CREAT: u32 = 0o100;      // create the file if it does not exist
pub const O_EXCL: u32 = 0o200;       // with O_CREAT: fail if it exists (EEXIST)
pub const O_TRUNC: u32 = 0o1000;     // empty the file when opening it
pub const O_APPEND: u32 = 0o2000;    // every write goes to the end
pub const O_DIRECTORY: u32 = 0o200000; // fail if it is not a directory (ENOTDIR)
pub const O_CLOEXEC: u32 = 0o2000000;  // close this fd on execve

pub fn sys_openat(dirfd: usize, path: &str, flags: u32, mode: u32) -> Result<usize, Errno> {

    if flags & O_ACCMODE == O_ACCMODE {
        return Err(Errno::EINVAL);
    }

    let current_process = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());

    let (parent, node_name) = {
        let dir = if dirfd as i32 == AT_FDCWD {
            current_process.current_dir.clone()
        } else {
            let resources = current_process.resources.lock();
            let Some(Some(dir)) = resources.get(dirfd) else {
                return Err(Errno::EBADF);
            };

            dir.file.node.clone()
        };

        vfs::walk_to_parent(path, Some(dir.clone()))?
    };

    let node = if let Some(name) = node_name {
        match vfs::resolve(name, Some(parent.clone())){
            Ok(node) => {
                if (flags & O_CREAT) != 0 && (flags & O_EXCL) != 0 {
                    return Err(Errno::EEXIST);
                }

                node
            }

            Err(VfsError::NotFound) if flags & O_CREAT != 0 => {
                match parent.create(name, VnodeType::VREG, mode & 0o7777) {
                    Ok(node) => node,
                    Err(VfsError::EntryExist) if flags & O_EXCL != 0 => parent.lookup(name)?, // another thread already created this file
                    Err(e) => return Err(Errno::from(e)),
                }
            }

            Err(e) => return Err(Errno::from(e)),
        }
    } else {
        parent
    };

    if (flags & O_DIRECTORY) != 0 && node.vtype() != VnodeType::VDIR {
        return Err(Errno::ENOTDIR);
    }

    let access_mode = flags & O_ACCMODE;
    if flags & O_TRUNC != 0
        && (access_mode == O_WRONLY || access_mode == O_RDWR)
    {
        node.trunc()?;
    }

    let opened = OpenFile::new(node, flags & (O_ACCMODE | O_APPEND)); // if append never write anywhere except the end probably by using a kind of built in vnode fucntion append

    current_process.alloc_resource(FileDescriptor {
        file: Arc::new(opened),
        is_cloexec: (flags & O_CLOEXEC) != 0,
    }).ok_or(Errno::EMFILE)
}

pub fn sys_close(fd: usize) -> Result<(), Errno> {
    let current_process = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());
    
    let file = {
        let mut resources = current_process.resources.lock();

        let Some(resource) = resources.get_mut(fd) else {
            return Err(Errno::EBADF);
        };

        resource.take()
    };

    if file.is_none() {
        return Err(Errno::EBADF);
    }

    Ok(())
}

pub fn sys_read(fd: usize, buffer: &mut[u8]) -> Result<usize, Errno> {
    let current_process = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());

    let file = {
        let resources = current_process.resources.lock();

        let Some(Some(resource)) = resources.get(fd) else {
            return Err(Errno::EBADF);
        };

        resource.file.clone()
    };

    let access_mode = file.flags & O_ACCMODE;
    if !(access_mode == O_RDONLY || access_mode == O_RDWR) {
        return Err(Errno::EBADF);
    }

    let mut offset = file.offset.lock();
    let read = file.node.read(*offset, buffer)?;
    *offset += read as u64;

    Ok(read)
}

pub fn sys_write(fd: usize, buffer: &[u8]) -> Result<usize, Errno> {
    let current_process = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());

    let file = {
        let resources = current_process.resources.lock();

        let Some(Some(resource)) = resources.get(fd) else {
            return Err(Errno::EBADF);
        };

        resource.file.clone()
    };

    let access_mode = file.flags & O_ACCMODE;
    if !(access_mode == O_WRONLY || access_mode == O_RDWR) {
        return Err(Errno::EBADF);
    }

    if file.flags & O_APPEND != 0 {
        let mut offset = file.offset.lock();
        let file_size = file.node.append(buffer)?;
        *offset = file_size;

        Ok(buffer.len())
    } else {
        let mut offset = file.offset.lock();
        let write = file.node.write(*offset, buffer)?;
        *offset += write as u64;

        Ok(write)
    }
}