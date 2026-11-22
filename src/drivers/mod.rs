use alloc::{collections::{BTreeMap, btree_map::Entry}, string::String, sync::Arc};

use crate::{drivers::console::Console, lock::rwlock::RwLock, vfs::{self, Vnode}};

mod console;

enum DeviceKind {
    Char,
    // Block
}

impl From<DeviceKind> for vfs::VnodeType {
    fn from(value: DeviceKind) -> Self {
        match value {
            DeviceKind::Char => Self::VCHAR,
            // DeviceKind::Block => Self::VBLCK,
        }
    }
}

trait Driver {
    fn dtype(&self) -> DeviceKind;
    fn read(&self, buf: &mut [u8]) -> Result<usize, vfs::VfsError>;
    fn write(&self, buf: &[u8]) -> Result<usize, vfs::VfsError>;
}

impl<T: Driver> Vnode for T {
    fn read(&self, _offset: u64, buffer: &mut [u8]) -> Result<usize, vfs::VfsError> {
        self.read(buffer)
    }

    fn write(&self, _offset: u64, buffer: &[u8]) -> Result<usize, vfs::VfsError> {
        self.write(buffer)
    }

    fn append(&self, buffer: &[u8]) -> Result<u64, vfs::VfsError> {
        self.write(buffer).map(|ok| ok as u64)
    }

    fn create(&self, _name: &str, _vtype: vfs::VnodeType, _mode: u32) -> Result<Arc<dyn Vnode>, vfs::VfsError> {
        Err(vfs::VfsError::NotSupported)
    }

    fn lookup(&self, _name: &str) -> Result<Arc<dyn Vnode>, vfs::VfsError> {
        Err(vfs::VfsError::NotSupported)
    }

    fn trunc(&self) -> Result<(), vfs::VfsError> {
        Err(vfs::VfsError::NotSupported)
    }

    fn vtype(&self) -> vfs::VnodeType {
        vfs::VnodeType::from(self.dtype())
    }
}

struct DevfsRoot;
impl Vnode for DevfsRoot {
    fn read(&self, _offset: u64, _buffer: &mut [u8]) -> Result<usize, vfs::VfsError> {
        Err(vfs::VfsError::IsDirectory)
    }

    fn write(&self, _offset: u64, _buffer: &[u8]) -> Result<usize, vfs::VfsError> {
        Err(vfs::VfsError::IsDirectory)
    }

    fn append(&self, _buffer: &[u8]) -> Result<u64, vfs::VfsError> {
        Err(vfs::VfsError::IsDirectory)
    }

    fn create(&self, _name: &str, _vtype: vfs::VnodeType, _mode: u32) -> Result<Arc<dyn Vnode>, vfs::VfsError> {
        Err(vfs::VfsError::NotSupported)
    }

    fn lookup(&self, name: &str) -> Result<Arc<dyn Vnode>, vfs::VfsError> {
        let node = DEVICES.read().get(name).ok_or(vfs::VfsError::NotFound)?.clone();
        Ok(node)
    }

    fn trunc(&self) -> Result<(), vfs::VfsError> {
        Err(vfs::VfsError::NotSupported)
    }

    fn vtype(&self) -> vfs::VnodeType {
        vfs::VnodeType::VDIR
    }
}

struct Devfs {
    rootfs: Arc<DevfsRoot>
}

impl Devfs {
    fn new() -> Arc<Self> {
        let fs = Self { rootfs: Arc::new(DevfsRoot) };
        Arc::new(fs)
    }
}

impl vfs::Filesystem for Devfs {
    fn root(&self) -> Arc<dyn vfs::Vnode> {
        self.rootfs.clone()
    }
}
static DEVICES: RwLock<BTreeMap<String, Arc<dyn Vnode>>> = RwLock::new(BTreeMap::new());

fn register<T: Driver + 'static>(device: T, name: String) -> Result<(), ()> {
    let mut device_list = DEVICES.write();
    match device_list.entry(name) {
        Entry::Occupied(_) => Err(()),
        Entry::Vacant(slot) => {
            slot.insert(Arc::new(device));
            Ok(())
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn DRIVER_init() {
    let dev = match vfs::resolve("/dev/", None) {
        Ok(d) => d,
        Err(vfs::VfsError::NotFound) => {
            let root = vfs::resolve("/", None).expect("failed to get root");
            root.create("dev", vfs::VnodeType::VDIR, 777).expect("cannot create the dev directory")
        }

        Err(_) => panic!("unexpected error while creating devfs")
    };

    vfs::mount(dev, Devfs::new()).expect("cannot mount devfs");

    let console = Console;
    register(console, "console".into()).unwrap();
}