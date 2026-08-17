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
    ; pusha               ; pushes in order: rax, rcx, rdx, rbx, rsp, rbp, rsi, rdi
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi

    xor rax, rax        ; push ds
    mov ax, ds
    push rax

    mov ax, 0x10        ; use kernel data segment
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    
    mov rdi, rsp        ; pass pointer to stack to C, so we can access all the pushed information
    call ISR_handler

    pop rax             ; restore old segment
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    ; popa                ; pop what we pushed with pusha
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    pop rdx
    pop rcx
    pop rax

    add esp, 16         ; remove error code and interrupt number
    iretq                ; will pop: cs, rip, rflags, ss, rsp