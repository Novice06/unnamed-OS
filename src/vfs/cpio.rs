use alloc::sync::Arc;

use crate::vfs::{VfsError, Vnode, VnodeType};

const MAGIC: &[u8] = b"070701";
const HEADER_SIZE: usize = 6 + 13 * 8;
const TRAILER: &str = "TRAILER!!!";
 
const MODE_TYPE_MASK: usize = 0o170000;
const MODE_DIR: usize = 0o040000;
 
#[derive(Debug, PartialEq, Eq)]
pub enum CpioError {
    BadMagic,
    Truncated,
    BadField,
    BadName,
    Vfs(VfsError),
}
 
impl From<VfsError> for CpioError {
    fn from(e: VfsError) -> Self {
        Self::Vfs(e)
    }
}

/// cpio fields are in hexa decimal c-styel string
fn hex_field(field: &[u8]) -> Result<usize, CpioError> {
    let s = core::str::from_utf8(field).map_err(|_| CpioError::BadField)?;
    usize::from_str_radix(s, 16).map_err(|_| CpioError::BadField)
}

/// because cpio align its component by 4bytes
fn align4(n: usize) -> Result<usize, CpioError> {
    // basic computer science knowledge
    // align⁡(n,a)=(n+a−1) & ∼(a−1)
    n.checked_add(3).map(|v| v & !3).ok_or(CpioError::Truncated)
}
 
/// unpack a cpio archive in the provided directory
pub fn unpack_cpio_in(root: &Arc<dyn Vnode>, archive: &[u8]) -> Result<(), CpioError> {
    let mut offset = 0usize;
 
    loop {
        let header_end = offset.checked_add(HEADER_SIZE).ok_or(CpioError::Truncated)?;
        let header = archive.get(offset..header_end).ok_or(CpioError::Truncated)?;
 
        if &header[..6] != MAGIC {
            return Err(CpioError::BadMagic);
        }
 
        // layout: magic | ino | mode | uid | gid | nlink | mtime | filesize | ...
        let mode = hex_field(&header[14..22])?;
        let filesize = hex_field(&header[54..62])?;
        let namesize = hex_field(&header[94..102])?;
 
        let name_end = header_end.checked_add(namesize).ok_or(CpioError::Truncated)?;
        let raw_name = archive.get(header_end..name_end).ok_or(CpioError::Truncated)?;
 
        // namesize includes the trailing NULL (and is never 0 in a valid archive)
        let name_bytes = match raw_name.split_last() {
            Some((0, rest)) => rest,
            _ => return Err(CpioError::BadName),
        };
        let name = core::str::from_utf8(name_bytes).map_err(|_| CpioError::BadName)?;
        if name == TRAILER {
            return Ok(());
        }
 
        // data starts after header + name, padded to 4 bytes
        let data_start = align4(name_end)?;
        let data_end = data_start.checked_add(filesize).ok_or(CpioError::Truncated)?;
        let data = archive.get(data_start..data_end).ok_or(CpioError::Truncated)?;
 
        insert_entry(root, name, mode, data)?;
 
        offset = align4(data_end)?;
    }
}

fn insert_entry(root: &Arc<dyn Vnode>, path: &str, mode: usize, data: &[u8]) -> Result<(), CpioError> {
    let mut components = path.split('/').filter(|c| !c.is_empty() && *c != ".");
 
    // this is the root, nothing to create
    let Some(mut name) = components.next() else {
        return Ok(());
    };
 
    // every component except the last one must be a directory
    let mut dir = Arc::clone(root);
    for next in components {
        dir = ensure_dir(&dir, name)?;
        name = next;
    }
 
    if mode & MODE_TYPE_MASK == MODE_DIR {
        ensure_dir(&dir, name)?;
    } else {
        // symlinks, devices... are stored as regular files for now and probably forever idk
        let file = dir.create(name, VnodeType::VREG)?;
        file.write(0, data)?;
    }
 
    Ok(())
}
 
/// returns the existing directory or creates it.
fn ensure_dir(parent: &Arc<dyn Vnode>, name: &str) -> Result<Arc<dyn Vnode>, VfsError> {
    match parent.lookup(name) {
        Ok(node) if node.vtype() == VnodeType::VDIR => Ok(node),
        Ok(_) => Err(VfsError::NotDirectory),
        Err(VfsError::NotFound) => parent.create(name, VnodeType::VDIR),
        Err(e) => Err(e),
    }
}