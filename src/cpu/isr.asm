extern ISR_handler

%macro ISR_NOERRORCODE 1
global ISR%1:
ISR%1:
    push 0              ; push dummy error code
    push %1             ; push interrupt number
    jmp isr_common

%endmacro

%macro ISR_ERRORCODE 1
global ISR%1:
ISR%1:
                        ; cpu pushes an error code to the stack
    push %1             ; push interrupt number
    jmp isr_common

%endmacro

%include "src/cpu/isrs_gen.inc"

; cpu pushes to the stack: ss, rsp, rflags, cs, rip
isr_common:
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
    call ISR_handler

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
    iretq                ; will pop: cs, rip, rflags, ss, rsp