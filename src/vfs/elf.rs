use alloc::{sync::Arc, vec::Vec};

use crate::{mm::{VirtAddr, get_hhdm, paging, virtual_area::{self, VirtualArea}}, task::Process, vfs::{VfsError, Vnode}};

#[repr(C)]
#[derive(Default)]
struct Elf64_Ehdr {
    e_ident: [u8; 16],
    e_type: u16,      
    e_machine: u16,   
    e_version: u32,   
    e_entry: u64,     
    e_phoff: u64,     
    e_shoff: u64,     
    e_flags: u32,     
    e_ehsize: u16,    
    e_phentsize: u16, 
    e_phnum: u16,     
    e_shentsize: u16, 
    e_shnum: u16,     
    e_shstrndx: u16,  
}

#[repr(C)]
struct Elf64_Phdr {
    p_type: u32,  
    p_flags: u32, 
    p_offset: u64,
    p_vaddr: u64, 
    p_paddr: u64, 
    p_filesz: u64,
    p_memsz: u64, 
    p_align: u64, 
}

impl Elf64_Phdr {
    fn to_vm_flags(&self) -> virtual_area::VmFlags {
        let mut flags: virtual_area::VmFlags = 0;
        if (self.p_flags & 1) != 0 { flags |= virtual_area::VM_EXEC }
        if (self.p_flags & 2) != 0 { flags |= virtual_area::VM_WRITE }

        flags
    }
}

pub enum ElfError {
    ReadError(VfsError),
    InvalidMagic,
    InvalidClass,
    InvalidEndian,
    InvalidArch,
    InvalidHeader,
    InvalidProgramHeader,
    Unsupported,
    AddressSpaceError
}

impl From<VfsError> for ElfError {
    fn from(value: VfsError) -> Self {
        ElfError::ReadError(value)
    }
}

pub fn load_in(proc: Arc<Process>, file: Arc<dyn Vnode>) -> Result<Option<VirtAddr>, ElfError>{
    let header = {
        let mut header = Elf64_Ehdr::default();

        let header_slice = unsafe {
            core::slice::from_raw_parts_mut(
                &mut header as *mut Elf64_Ehdr as *mut u8, 
                core::mem::size_of::<Elf64_Ehdr>()
            )
        };

        match file.read(0, header_slice) {
            Ok(bytes_read) => {
                if bytes_read != core::mem::size_of::<Elf64_Ehdr>() {
                    return Err(ElfError::InvalidHeader);
                }

                if &header.e_ident[0..4] != b"\x7FELF" {
                    return Err(ElfError::InvalidMagic);
                }
            },

            Err(e) => return Err(ElfError::from(e)),
        }

        header
    };

    if header.e_ident[4] != 2 { return Err(ElfError::InvalidClass); }
    if header.e_ident[5] != 1 { return Err(ElfError::InvalidEndian); }
    if header.e_type != 0x2 { return Err(ElfError::Unsupported); }
    if header.e_machine != 0x3E { return Err(ElfError::InvalidArch); }

    if header.e_phentsize as usize != core::mem::size_of::<Elf64_Phdr>() { return Err(ElfError::InvalidProgramHeader); }


    let mut addr_space = proc.addr_space.lock();
    addr_space.clean();

    let program_headers = {
        let program_headers_size = (header.e_phentsize as usize) * (header.e_phnum as usize);
        let mut program_headers: Vec<Elf64_Phdr> = Vec::with_capacity(header.e_phnum as usize);
        program_headers.resize_with(header.e_phnum as usize, || unsafe {
            core::mem::zeroed()
        });

        let phdr_slice = unsafe {
            core::slice::from_raw_parts_mut(
                program_headers.as_mut_ptr() as *mut u8,
                program_headers_size,
            )
        };

        match file.read(header.e_phoff, phdr_slice) {
            Ok(bytes_read) => {
                if bytes_read != program_headers_size {
                    return Err(ElfError::InvalidProgramHeader);
                }
            },

            Err(e) => return Err(ElfError::from(e)),
        }

        program_headers
    };

    // PT_LOAD only
    for pheader in program_headers.iter().filter(|h| h.p_type == 1) {
        let start = VirtAddr(pheader.p_vaddr & !0xFFF);
        let pages = ((pheader.p_vaddr & 0xFFF) + pheader.p_memsz).div_ceil(0x1000);

        let area = VirtualArea::new(
            start, 
            pages,
            pheader.to_vm_flags()
        ).ok_or(ElfError::InvalidProgramHeader)?;

        addr_space.insert(area).ok_or(ElfError::InvalidHeader)?;

        let mut copied = 0u64;
        let mut buffer = [0u8; 0x1000];

        while copied < pheader.p_filesz {
            let current_vaddr = pheader.p_vaddr + copied;
            let page_vaddr = current_vaddr & !0xFFF;
            let page_offset = current_vaddr & 0xFFF;
            let remaining = pheader.p_filesz - copied;
            let chunk_size = remaining.min(0x1000 - page_offset);

            let phys = paging::get_physical(
                addr_space.pml4,
                VirtAddr(page_vaddr)
            ).ok_or(ElfError::AddressSpaceError)?;

            let dst = (phys.0 + get_hhdm()) as *mut u8;

            let buffer = &mut buffer[..chunk_size as usize];

            match file.read(pheader.p_offset + copied as u64, buffer) {
                Ok(bytes_read) if bytes_read == chunk_size as usize => {}
                Ok(_) => return Err(ElfError::InvalidProgramHeader),
                Err(e) => return Err(ElfError::from(e)),
            }

            unsafe {
                core::ptr::copy_nonoverlapping(
                    buffer.as_ptr(),
                    dst.add(page_offset as usize),
                    chunk_size as usize
                );
            }

            copied += chunk_size;
        }
    }

    let entry_point = if header.e_entry != 0 { Some(VirtAddr(header.e_entry)) } else {
        None
    };

    Ok(entry_point)
}