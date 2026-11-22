[BITS 64]
global enable_sse

enable_sse:
    mov eax, 0x1
    cpuid
    test edx, 1<<25
    jz .noSSE

    ;SSE is available
    ; enable it
    mov rax, cr0
    and ax, 0xFFFB		;clear coprocessor emulation CR0.EM
    or ax, 0x2			;set coprocessor monitoring  CR0.MP
    mov cr0, rax
    mov rax, cr4
    or ax, 3 << 9		;set CR4.OSFXSR and CR4.OSXMMEXCPT at the same time
    mov cr4, rax

    ; shamelessly stole this code from TACOS after seeing !
    ; set sane values (banan's words, not mine)
    finit
    ldmxcsr [mxcsr_val]

.noSSE:
    ret

mxcsr_val: dd 0x1f80