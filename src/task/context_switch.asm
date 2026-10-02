
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
    mov cr3, rdi

    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret