use crate::{syscall::Errno, task::{TaskError, scheduler::yield_task, wait_anychildproc, wait_process}};

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
pub const WSTOPPED: i32 = 2;
pub const WEXITED: i32 = 4;
pub const WCONTINUED: i32 = 8;
pub const WNOWAIT: i32 = 0x01000000;
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
            Err(TaskError::ProcessStillRunning) => {
                if (options & WNOHANG) != 0 {
                    return Ok(0);
                }

                yield_task();
            }
            Err(TaskError::ProcessNotMySon) | Err(TaskError::ProcessIsChildless) => {
                return Err(Errno::ECHILD);
            }
            Err(_) => unreachable!("unexpected wait error"),
        }
    }
}
