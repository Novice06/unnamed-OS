use alloc::{collections::{BTreeMap, btree_map::Entry}, string::String, sync::Arc, vec::Vec};

use crate::{lock::rwlock::RwLock, vfs::{Filesystem, VfsError, Vnode, VnodeType}};

const MAX_FILE_SIZE: usize = 16 * 1024 * 1024;
 
enum NodeKind {
    File(RwLock<Vec<u8>>),
    Dir(RwLock<BTreeMap<String, Arc<TempFsNode>>>),
}
 
pub struct TempFsNode {
    kind: NodeKind,
}
 
impl TempFsNode {
    fn new(vtype: VnodeType) -> Self {
        let kind = match vtype {
            VnodeType::VDIR => NodeKind::Dir(RwLock::new(BTreeMap::new())),
            VnodeType::VREG => NodeKind::File(RwLock::new(Vec::new())),
        };
 
        Self { kind }
    }
}
 
fn validate_name(name: &str) -> Result<(), VfsError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(VfsError::InvalidName);
    }
 
    Ok(())
}
 
impl Vnode for TempFsNode {
    fn vtype(&self) -> VnodeType {
        match self.kind {
            NodeKind::File(_) => VnodeType::VREG,
            NodeKind::Dir(_) => VnodeType::VDIR,
        }
    }
 
    fn lookup(&self, name: &str) -> Result<Arc<dyn Vnode>, VfsError> {
        let NodeKind::Dir(entries) = &self.kind else {
            return Err(VfsError::NotDirectory);
        };
 
        let entries = entries.read();
        let child = entries.get(name).ok_or(VfsError::NotFound)?;
 
        Ok(Arc::clone(child) as Arc<dyn Vnode>)
    }
 
    fn read(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, VfsError> {
        let NodeKind::File(data) = &self.kind else {
            return Err(VfsError::IsDirectory);
        };
 
        let data = data.read();
 
        // an offset that does not even fit in usize is necessarily past the end
        let Ok(offset) = usize::try_from(offset) else {
            return Ok(0);
        };
        if offset >= data.len() {
            return Ok(0);
        }
 
        let len = (data.len() - offset).min(buffer.len());
        buffer[..len].copy_from_slice(&data[offset..offset + len]);
 
        Ok(len)
    }
 
    fn write(&self, offset: u64, buffer: &[u8]) -> Result<usize, VfsError> {
        let NodeKind::File(data) = &self.kind else {
            return Err(VfsError::IsDirectory);
        };
 
        let offset = usize::try_from(offset).map_err(|_| VfsError::NoSpace)?;
        let end = offset.checked_add(buffer.len()).ok_or(VfsError::NoSpace)?;
        if end > MAX_FILE_SIZE {
            return Err(VfsError::NoSpace);
        }
 
        let mut data = data.write();
        if data.len() < end {
            // try_reserve: report out-of-memory instead of aborting the kernel
            let additional = end - data.len();
            data.try_reserve(additional).map_err(|_| VfsError::NoSpace)?;
            data.resize(end, 0); // the gap between the old end and `offset` is zeroed
        }
 
        data[offset..end].copy_from_slice(buffer);
 
        Ok(buffer.len())
    }
 
    fn create(&self, name: &str, vtype: VnodeType) -> Result<Arc<dyn Vnode>, VfsError> {
        let NodeKind::Dir(entries) = &self.kind else {
            return Err(VfsError::NotDirectory);
        };
 
        validate_name(name)?;
 
        // "does it exist?" and "insert it" op
        let mut entries = entries.write();
        match entries.entry(String::from(name)) {
            Entry::Occupied(_) => Err(VfsError::EntryExist),
            Entry::Vacant(slot) => {
                let node = Arc::new(TempFsNode::new(vtype));
                slot.insert(Arc::clone(&node));
 
                Ok(node as Arc<dyn Vnode>)
            }
        }
    }
}
 
pub struct TempFs {
    root: Arc<TempFsNode>,
}

impl TempFs {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            root: Arc::new(TempFsNode::new(VnodeType::VDIR)),
        })
    }
}
 
impl Filesystem for TempFs {
    fn root(&self) -> Arc<dyn Vnode> {
        Arc::clone(&self.root) as Arc<dyn Vnode>
    }
}