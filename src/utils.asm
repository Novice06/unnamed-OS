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

global halt
halt:
    push rbp
    mov rbp, rsp

    hlt

    mov rsp, rbp
    pop rbp
    ret