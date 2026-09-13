global inb
inb:
    push rbp
    mov rbp, rsp

    ; push rbx
    ; push r12
    ; push r13
    ; push r14
    ; push r15

    mov dx, di
    in al, dx
    movzx eax, al

    ; pop r15
    ; pop r14
    ; pop r13
    ; pop r12
    ; pop rbx

    mov rsp, rbp
    pop rbp
    ret

global outb
outb:
    push rbp
    mov rbp, rsp

    ; push rbx
    ; push r12
    ; push r13
    ; push r14
    ; push r15

    mov dx, di
    mov al, sil
    out dx, al

    ; pop r15
    ; pop r14
    ; pop r13
    ; pop r12
    ; pop rbx

    mov rsp, rbp
    pop rbp
    ret

global read_msr
read_msr:
    push rbp
    mov rbp, rsp

    mov ecx, edi
    rdmsr

    shl rdx, 32
    or rax, rdx

    mov rsp, rbp
    pop rbp
    ret

global switch_pdbr
switch_pdbr:
    push rbp
    mov rbp, rsp

    mov cr3, rdi

    mov rsp, rbp
    pop rbp
    ret

global get_pdbr
get_pdbr:
    push rbp
    mov rbp, rsp

    mov rax, cr3

    mov rsp, rbp
    pop rbp
    ret

global switch_stack
switch_stack:
    push rbp
    mov rbp, rsp

    mov rsp, rdi
    jmp rsi

    mov rsp, rbp
    pop rbp
    ret

global halt
halt:
    push rbp
    mov rbp, rsp

    hlt

    mov rsp, rbp
    pop rbp
    ret