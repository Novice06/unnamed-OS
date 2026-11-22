use alloc::{sync::Arc, vec::Vec};

use crate::{lock::{rwlock::RwLock, spinlock::SpinLockIrqSave}, vfs::{tempfs::TempFs}};

mod tempfs;
pub mod cpio;
mod ffi;
pub mod elf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VfsError {
    NoRoot,
    NotFound,
    NotDirectory,
    IsDirectory,
    NoSpace,
    EntryExist,
    InvalidName,
    Busy,
    NotSupported,
}
 
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VnodeType {
    VREG,
    VDIR,
    VCHAR,
    VBLCK
}

pub trait Vnode {
    fn vtype(&self) -> VnodeType;

    fn lookup(&self, name: &str) -> Result<Arc<dyn Vnode>, VfsError>;
    fn read(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&self, offset: u64, buffer: &[u8]) -> Result<usize, VfsError>;
    fn create(&self, name: &str, vtype: VnodeType, mode: u32) -> Result<Arc<dyn Vnode>, VfsError>;
    fn trunc(&self) -> Result<(), VfsError>;
    fn append(&self, buffer: &[u8]) -> Result<u64, VfsError>;
}

pub trait Filesystem {
    fn root(&self) -> Arc<dyn Vnode>;
}

struct Mount {
    covered: Arc<dyn Vnode>,
    filesystem: Arc<dyn Filesystem>,
}

pub static ROOT_FILESYSTEM: SpinLockIrqSave<Option<Arc<dyn Filesystem>>> = SpinLockIrqSave::new(None);
static MOUNT_POINTS: RwLock<Vec<Mount>> = RwLock::new(Vec::new());

fn is_same_vnode(a: &Arc<dyn Vnode>, b: &Arc<dyn Vnode>) -> bool {
    core::ptr::eq(Arc::as_ptr(a) as *const (), Arc::as_ptr(b) as *const ())
}

fn follow_mounts(mut node: Arc<dyn Vnode>) -> Arc<dyn Vnode> {
    loop {
        let next = {
            let mounts = MOUNT_POINTS.read();
            let found = mounts
                .iter()
                .find(|m| is_same_vnode(&m.covered, &node))
                .map(|m| m.filesystem.root().clone());
            found
        };
 
        match next {
            Some(root) => node = root,
            None => return node,
        }
    }
}

pub fn mount(node: Arc<dyn Vnode>, filesystem: Arc<dyn Filesystem>) -> Result<(), VfsError> {
    if node.vtype() != VnodeType::VDIR {
        return Err(VfsError::IsDirectory)
    }

    let new = Mount {
        covered: node,
        filesystem
    };

    MOUNT_POINTS.write().push(new);

    Ok(())
}

pub fn resolve(path: &str, in_this_dir: Option<Arc<dyn Vnode>>) -> Result<Arc<dyn Vnode>, VfsError> {

    let initial_dir = match in_this_dir {
        // today I learned a new pattern matching, crazy
        Some(dir) if !path.starts_with('/') => dir,

        _ => ROOT_FILESYSTEM
            .lock()
            .as_ref()
            .ok_or(VfsError::NoRoot)?
            .root(),
    };
 
    let mut current = follow_mounts(initial_dir);
    for component in path.split('/').filter(|c| !c.is_empty() && *c != ".") {
        current = follow_mounts(current.lookup(component)?);
    }
 
    Ok(current)
}

pub fn walk_to_parent(path: &str, in_this_dir: Option<Arc<dyn Vnode>>) -> Result<(Arc<dyn Vnode>, Option<&str>), VfsError> {

    let initial_dir = match in_this_dir {
        Some(dir) if !path.starts_with('/') => follow_mounts(dir),
        _ => follow_mounts(ROOT_FILESYSTEM.lock().as_ref().unwrap().root()),
    };

    let mut components = path.split('/').filter(|c| !c.is_empty() && *c != ".");
    let Some(mut last) = components.next() else {
        return Ok((initial_dir, None));
    };

    let mut current = initial_dir;
    for component in components {
        current = follow_mounts(current.lookup(last)?);
        last = component;
    }

    let child = if last.is_empty() {
        None
    } else {
        Some(last)
    };

    Ok((current, child))
}

pub fn init() {
    *ROOT_FILESYSTEM.lock() = Some(TempFs::new());
}