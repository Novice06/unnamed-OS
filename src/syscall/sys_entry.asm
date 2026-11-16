extern SYSCALL_dispatcher

KERNEL_RSP equ 8
USER_RSP_TMP equ 16

global SYSCALL_entry
SYSCALL_entry :
    swapgs
    mov [gs:USER_RSP_TMP], rsp
    mov rsp, [gs:KERNEL_RSP]    ; the tss would've done this for us              

    ; building the stack frame excepted by the handler
    push 0x1B   ; ss
    push qword [gs:USER_RSP_TMP]  ; rsp
    push r11    ; rflags
    push 0x23    ; code segment
    push rcx    ; rip
    sub rsp, 16 ; error code and interrupt number
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rbp
    push rdi
    push rsi
    push rdx
    push rcx
    push rbx
    push rax

    xor rax, rax        ; push ds
    mov ax, ds
    push rax

    mov ax, 0x10        ; use kernel data segment
    mov ds, ax
    mov es, ax

    mov rdi, rsp        ; pass pointer to stack to C, so we can access all the pushed information
    call SYSCALL_dispatcher

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

    add rsp, 40
    pop rsp     ; restoring user stack

    swapgs
    o64 sysret