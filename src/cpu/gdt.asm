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
    mov   ax, 0x10 ; 0x10 is a stand-in for your data segment
    mov   ds, ax
    mov   es, ax
    mov   fs, ax
    mov   gs, ax
    mov   ss, ax

    mov rsp, rbp
    pop rbp
    ret