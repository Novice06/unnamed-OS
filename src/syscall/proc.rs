use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{lock::crit_sec::CriticalGuard, mm::{self, VirtAddr, virtual_area::{self, AddressSpace, VirtualArea}}, smp::CpuInfo, syscall::Errno, task::{WaitError, exit_process, scheduler::yield_task, wait_anychildproc, wait_process}, vfs};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Timeval {
    pub tv_sec: isize,  
    pub tv_usec: isize, 
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Rusage {
    pub ru_utime: Timeval,  
    pub ru_stime: Timeval,  
    pub ru_maxrss: isize,     
    pub ru_ixrss: isize,      
    pub ru_idrss: isize,      
    pub ru_isrss: isize,      
    pub ru_minflt: isize,     
    pub ru_majflt: isize,     
    pub ru_nswap: isize,      
    pub ru_inblock: isize,    
    pub ru_oublock: isize,    
    pub ru_msgsnd: isize,     
    pub ru_msgrcv: isize,     
    pub ru_nsignals: isize,   
    pub ru_nvcsw: isize,      
    pub ru_nivcsw: isize,     
}

pub const WNOHANG: i32 = 1;
pub const WUNTRACED: i32 = 2;
// pub const WSTOPPED: i32 = 2;
// pub const WEXITED: i32 = 4;
pub const WCONTINUED: i32 = 8;
// pub const WNOWAIT: i32 = 0x01000000;
pub const __WNOTHREAD: i32 = 0x20000000;
pub const __WALL: i32 = 0x40000000;
pub const __WCLONE: i32 = 0x80000000u32 as i32;

pub fn sys_wait4(pid: isize, status: &mut u32, options: i32, _usage: &mut Rusage) -> Result<usize, Errno> {
    if pid < -1 {
        return Err(Errno::EINVAL);
    }

    if !((options & WUNTRACED) != 0 || (options & WCONTINUED) != 0 || (options & WNOHANG) != 0) {
        return Err(Errno::EINVAL);
    }

    loop {
        let result = if pid == -1 || pid == 0 {
            wait_anychildproc()
        } else {
            wait_process(pid as usize)
        };

        match result {
            Ok((id, s)) => {
                *status = s;
                return Ok(id);
            }
            Err(WaitError::ProcessStillRunning) => {
                if (options & WNOHANG) != 0 {
                    return Ok(0);
                }

                yield_task();
            }
            Err(WaitError::ProcessNotMySon) | Err(WaitError::ProcessIsChildless) => {
                return Err(Errno::ECHILD);
            }
        }
    }
}

pub fn sys_arch_prctl(code: i32, base: VirtAddr) -> Result<Option<u64>, Errno> {
    const ARCH_SET_FS: i32 = 0x1002;
    const ARCH_GET_FS: i32 = 0x1003;

    let current_thread = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().clone());

    match code {
        ARCH_SET_FS => {
            current_thread.set_fs(base.0);
            CriticalGuard::with(|| CpuInfo::get_current().set_fs_base(base.0));
            Ok(None)
        },

        ARCH_GET_FS => {
            Ok(Some(current_thread.get_fs()))
        }

        _ => Err(Errno::EINVAL)
    }
}

pub fn sys_set_tid_address(_addr: VirtAddr) -> Result<usize, Errno>{
    Ok(
        CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().tid)
    )
}

pub fn sys_exit_group(code: i32) -> ! {
    exit_process(code);
}

pub fn sys_fork(frame: &crate::TrapFrame) -> Result<usize, Errno> {
    let current_thread = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().clone());
    let child_pid = crate::task::alloc_pid();

    let child_proc = crate::task::TASK_MANAGER.add_process(
        current_thread.process.give_birth(child_pid)?, 
        child_pid
    );

    current_thread.process.add_child(child_proc.clone());

    let new_frame = crate::TrapFrame {
        rax: 0,
        ..*frame
    };
    
    let thread_of_child = {
        let tid = crate::task::alloc_tid();

        crate::task::TASK_MANAGER.add_thread(
            crate::task::Thread::new(VirtAddr(0), Some(new_frame), child_proc.clone(), tid)?, 
            tid
        )
    };
    child_proc.add_thread(Arc::downgrade(&thread_of_child));

    match crate::task::scheduler::schedule_thread(thread_of_child) {
        Ok(_) => (),
        Err(_) => panic!("failed to schedule a new thread"),
    };

    Ok(child_pid)
}

fn build_new_stack(path: &str, VirtAddr(stack_top): VirtAddr, VirtAddr(entry): VirtAddr, argv: Vec<String>, envp: Vec<String>) -> u64 {
    let mut rsp = stack_top;

    let mut argv_ptrs = Vec::new();
    let mut envp_ptrs = Vec::new();

    // copying envp string in reverse (kinda useless) and saving their pointers in envp_ptrs
    for s in envp.iter().rev() {
        rsp -= s.len() as u64 + 1; // +1 pour le \0
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), rsp as *mut u8, s.len());
            *((rsp + s.len() as u64) as *mut u8) = 0; // \0
        }

        envp_ptrs.push(rsp);
    }
    envp_ptrs.reverse();

    // same here for argv
    for s in argv.iter().rev() {
        rsp -= s.len() as u64 + 1;
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), rsp as *mut u8, s.len());
            *((rsp + s.len() as u64) as *mut u8) = 0; // \0
        }
        argv_ptrs.push(rsp);
    }
    argv_ptrs.reverse();


    // now saving a bunch of other strings for auxv table
    let platform_ptr = {
        let s = b"x86_64\0";
        rsp -= s.len() as u64;

        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), rsp as *mut u8, s.len());
        }

        rsp
    };
    let random_ptr = {
        let s = b"\x7f\xa3\x2c\x91\xe5\x08\xb4\xd6\x3a\xf2\x6e\x81\xc7\x59\x0d\xab";
        rsp -= s.len() as u64;

        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), rsp as *mut u8, s.len());
        }

        rsp
    };
    let execfn_ptr = {
        rsp -= path.len() as u64 + 1;
        unsafe {
            core::ptr::copy_nonoverlapping(path.as_ptr(), rsp as *mut u8, path.len());
            *((rsp + path.len() as u64) as *mut u8) = 0; // \0
        }
        
        rsp
    };

    // algn at 16
    rsp &= !0xF;
    // this is make sure that after building envp and argv, argc will be aligned at 16
    // if the total number of pointers written after the auxv is odd
    // we need to add 8 bytes of padding to preserve 16-byte stack alignment
    // the +3 is just the 2 nulls pointer written after the last envp and argv plus argc
    if (3 + envp_ptrs.len() + argv_ptrs.len()) % 2 != 0 {
        rsp -= 8;
    }


    // now copying the auxv table
    let auxv = [
        (3, 0),              // AT_PHDR, 0
        (4, 0),              // AT_PHENT, 0
        (5, 0),              // AT_PHNUM, 0
        (6, 4096),           // AT_PAGESZ, 4096
        (7, 0),              // AT_BASE, 0
        (9, entry),          // AT_ENTRY
        (11, 0),             // AT_UID
        (12, 0),             // AT_EUID
        (13, 0),             // AT_GID
        (14, 0),             // AT_EGID
        (17, 100),           // AT_CLKTCK, 100
        (15, platform_ptr),  // AT_PLATFORM, ptr -> "x86_64\0"
        (16, 0),             // AT_HWCAP, 0
        (23, 0),             // AT_SECURE, 0
        (25, random_ptr),    // AT_RANDOM, ptr -> 16 random bytes
        (31, execfn_ptr),    // AT_EXECFN, ptr -> path of the executable
        (33, 0),             // AT_SYSINFO_EHDR, 0
        (0, 0),              // AT_NULL, fin
    ];
    for (typ, val) in auxv.iter().rev() {
        rsp -= 16;

        unsafe {
            *(rsp as *mut u64) = *typ;
            *((rsp + 8) as *mut u64) = *val;
        }
    }

    // building the envp pointer table
    rsp -= 8;
    unsafe {
        *(rsp as *mut u64) = 0; // NULL must be last
    }
    for ptr in envp_ptrs.iter().rev() {
        rsp -= 8;
        unsafe {
            *(rsp as *mut u64) = *ptr;
        }
    }

    // 5. same here
    rsp -= 8;
    unsafe {
        *(rsp as *mut u64) = 0;
    }
    for ptr in argv_ptrs.iter().rev() {
        rsp -= 8;
        unsafe {
            *(rsp as *mut u64) = *ptr;
        }
    }

    // now argc
    rsp -= 8;
    unsafe {
        *(rsp as *mut u64) = argv.len() as u64;
    }

    // if rsp != (rsp &!0xF) {
    //     crate::warn!("something went wrong here !!");
    // }

    return rsp; // new stack top
}


// TODO: there is a memory leakage for temp_cr3 and tmp_addr_space every time we do an early return
pub fn sys_execve(path: String, argv: Vec<String>, envp: Vec<String>) -> Result<(), Errno> {
    let current_process = CriticalGuard::with(|| CpuInfo::get_current().current_thread.as_ref().unwrap().process.clone());

    let (entry_point, stack_top) = {
        let file = vfs::resolve(&path, Some(current_process.current_dir.clone()))?;
        if file.vtype() != vfs::VnodeType::VREG { return Err(Errno::EACCES); }

        // temporary address space in case the elf loader fail
        let tmp_cr3 = mm::physical::PHYSICAL_MEMORY_ALLOCATOR.lock().alloc_page().ok_or(Errno::ENOMEM)?;
        let mut tmp_addr_space = AddressSpace::new(VirtAddr(tmp_cr3.0 + mm::get_hhdm()));

        let entry_point = match vfs::elf::load_in(&mut tmp_addr_space, file) {
            Ok(entry) => entry,
            Err(vfs::elf::ElfError::ReadError(v)) => return Err(Errno::from(v)),
            Err(_) => return Err(Errno::ENOEXEC),
        };

        let stack_area = {
            let start = VirtAddr(0x7FFFFFFF7000);
            VirtualArea::new(start, 8, virtual_area::VM_WRITE).ok_or(Errno::ENOEXEC)?
        };
        tmp_addr_space.insert(stack_area).ok_or(Errno::ENOEXEC)?;

        // TODO: kill all running threads

        current_process.close_cloexec_res();
        AddressSpace::clone(&tmp_addr_space, &mut *current_process.addr_space.lock());

        // nicely cleaning
        tmp_addr_space.clean();
        mm::physical::PHYSICAL_MEMORY_ALLOCATOR.lock().free_page(tmp_cr3);

        let stack_top = build_new_stack(&path, stack_area.end(), entry_point, argv, envp);
        (entry_point, VirtAddr(stack_top))
    };

    let frame = crate::TrapFrame::user_frame(stack_top, entry_point);
    crate::task::spawn_user_thread(frame, current_process);

    drop(path);
    crate::task::exit_thread(0);
}
