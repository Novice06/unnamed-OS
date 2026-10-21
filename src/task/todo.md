- [ ] Stack alignment is incorrect at thread entry: rsp is 16-byte aligned instead of 8 mod 16 as required by the SysV ABI, potentially causing a #GP fault with aligned SSE instructions. Reserve 8 bytes for a fake return address pointing to thread_exit.

- [ ] Kernel stack overflow risk: The 4 KiB stack has no guard page, so deep calls or interrupt frames can silently corrupt adjacent physical memory. Allocate multiple pages with an unmapped guard page below the stack, and use a dedicated double-fault IST stack to prevent triple faults.

- [ ] Unnecessary TLB flushes: Reloading CR3 on every context switch flushes the TLB unnecessarily, even between threads of the same process. Compare the new and current CR3 values and skip the reload if they are identical.

- [ ] Preemption race condition: CpuInfo::get_current() can return a stale CPU pointer if the thread is preempted and rescheduled on another CPU, causing state corruption. Disable kernel-mode preemption and only preempt threads interrupted in user mode to eliminate this class of bugs. but that would cause a significant performance issue so I'm just gonna maniputlate the cpu struct in a critical section  state