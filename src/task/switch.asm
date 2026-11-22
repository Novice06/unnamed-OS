
global context_switch   ; void context_switch(uint64_t pml4, uint64_t* old_rsp, uint64_t new_rsp)
context_switch:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov [rsi], rsp      ; save old stack
    mov rsp, rdx      ; load new stack
    
    mov rax, cr3
    cmp rax, rdi
    je .sameVAS

    mov cr3, rdi

.sameVAS:
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

global switch_to_usermode   ; void switch_to_usermode(TrapFrame* frame);
switch_to_usermode:
    cli
    mov rsp, rdi        ;   load the trap frame on the stack

    pop rax             ; restore old segment
    mov ds, ax
    mov es, ax

    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rsi
    pop rdi
    pop rbp
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15

    add rsp, 16         ; remove error code and interrupt number

    swapgs
    iretq                ; will pop: cs, rip, rflags, ss, rsp