use alloc::{sync::Arc, vec::Vec};

use crate::{lock::{rwlock::RwLock, spinlock::SpinLock}, println, vfs::{cpio::CpioError, tempfs::TempFs}};

mod tempfs;
mod cpio;
mod ffi;

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
}
 
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VnodeType {
    VREG,
    VDIR,
}

trait Vnode {
    fn vtype(&self) -> VnodeType;

    fn lookup(&self, name: &str) -> Result<Arc<dyn Vnode>, VfsError>;
    fn read(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&self, offset: u64, buffer: &[u8]) -> Result<usize, VfsError>;
    fn create(&self, name: &str, vtype: VnodeType) -> Result<Arc<dyn Vnode>, VfsError>;
}

trait Filesystem {
    fn root(&self) -> Arc<dyn Vnode>;
}

struct Mount {
    covered: Arc<dyn Vnode>,
    root: Arc<dyn Vnode>,
}

static ROOT_FILESYSTEM: SpinLock<Option<Arc<dyn Filesystem>>> = SpinLock::new(None);
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
                .map(|m| m.root.clone());
            found
        };
 
        match next {
            Some(root) => node = root,
            None => return node,
        }
    }
}

fn resolve(path: &str) -> Result<Arc<dyn Vnode>, VfsError> {
    let root = ROOT_FILESYSTEM.lock().as_ref().ok_or(VfsError::NoRoot)?.root();
 
    let mut current = follow_mounts(root);  // because maybe the root was used to another fs ?? idk
    for component in path.split('/').filter(|c| !c.is_empty() && *c != ".") {
        current = follow_mounts(current.lookup(component)?);
    }
 
    Ok(current)
}

pub fn init(cpio_archive: Option<&[u8]>) {
    *ROOT_FILESYSTEM.lock() = Some(TempFs::new());

    let root = ROOT_FILESYSTEM.lock().as_ref().unwrap().root();

    if let Some(archive) = cpio_archive {
        match cpio::unpack_cpio_in(&root, archive) {
            Ok(_) => (),
            Err(err) => {
                match err {
                    CpioError::Vfs(_) => println!("there was a VFS error while unpacking the cpio archive"),
                    _ => println!("Invalid cpio Archive"),
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn VFS_test() {
    let vnode = resolve("/home/welcome.txt").unwrap_or_else(|_| {
        panic!("failed to open /home/welcome.txt");
    });

    let mut buff = [0u8; 60];

    vnode.read(0, &mut buff).unwrap_or_else(|_| {
        panic!("failed to read /home/welcome.txt")
    });

    let message = core::str::from_utf8(&buff);
    match message {
        Ok(m) => println!("data read: {m}"),
        Err(_) => println!("failed to convert the content into string but here is the raw data {:?}", buff),
    }
}