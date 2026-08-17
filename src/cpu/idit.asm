global idt_flush
idt_flush:
    push rbp
    mov rbp, rsp

    lidt [rdi]

    mov rsp, rbp
    pop rbp
    ret