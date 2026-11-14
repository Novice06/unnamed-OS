global gdt_flush
gdt_flush:
    push rbp
    mov rbp, rsp

    lgdt [rdi]

    push 0x8
    lea rax, [rel .reload_CS]
    push rax
    retfq

.reload_CS:
    mov   ax, 0x10
    mov   ds, ax
    mov   es, ax
    mov   fs, ax
    mov   gs, ax
    mov   ss, ax

    mov rsp, rbp
    pop rbp
    ret

global TSS_flush

TSS_flush:
    push rbp
    mov rbp, rsp

    movzx ax, dil
    shl ax, 3

    ltr ax

    mov rsp, rbp
    pop rbp
    ret
