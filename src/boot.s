/* ============================================================
   Multiboot2 header
   ============================================================ */
.section .text.multiboot2_header
.align 8
multiboot2_header:
    .long 0xE85250D6                                            /* magic */
    .long 0                                                     /* arch: i386 */
    .long header_end - multiboot2_header                        /* length */
    .long -(0xE85250D6 + 0 + (header_end - multiboot2_header)) /* checksum */

    /* framebuffer tag */
    .align 8
    .short 5        /* type = framebuffer */
    .short 1        /* flags = не обязательный */
    .long  20       /* size */
    .long  0        /* width  = любой */
    .long  0        /* height = любой */
    .long  32       /* bpp = 32 */

    /* end tag */
    .align 8
    .short 0
    .short 0
    .long  8
header_end:

/* ============================================================
   32-bit entry point  (VMA = LMA = физический адрес)
   ============================================================ */
.section .text.boot
.code32
.global start32
start32:
    /* 1. проверить magic от GRUB */
    cmp eax, 0x36D76289
    jne .halt32

    /* 2. сохранить указатель на boot info */
    mov esi, ebx

    /* 3. настроить стек */
    mov esp, offset stack_top

    /* 4. обнулить все page tables (5 таблиц × 4KB) */
    mov edi, offset pml4
    mov ecx, 5 * 1024       /* 5 таблиц × 1024 dword */
    xor eax, eax
    rep stosd

    /* 5. заполнить page tables */

    /* PML4[0] → pdpt0 (identity map 0-512GB) */
    mov eax, offset pdpt0
    or  eax, 0x3
    mov dword ptr [pml4],     eax
    mov dword ptr [pml4 + 4], 0

    /* PML4[1] → pdpt1 (identity map 512GB-1TB) */
    mov eax, offset pdpt1
    or  eax, 0x3
    mov dword ptr [pml4 + 8],  eax
    mov dword ptr [pml4 + 12], 0

    /* PML4[256] → pdpt_high (higher half 0xFFFF800000000000) */
    mov eax, offset pdpt_high
    or  eax, 0x3
    mov dword ptr [pml4 + 256 * 8],     eax
    mov dword ptr [pml4 + 256 * 8 + 4], 0

    /* pdpt0: 512 записей × 1GB начиная с физ. 0x0 */
    mov edi, offset pdpt0
    mov eax, 0x83           /* Present + Writable + PageSize(1GB) */
    xor edx, edx
    mov ecx, 512
.fill_pdpt0:
    mov dword ptr [edi],     eax
    mov dword ptr [edi + 4], edx
    add eax, 0x40000000     /* +1GB */
    adc edx, 0
    add edi, 8
    loop .fill_pdpt0

    /* pdpt1: 512 записей × 1GB начиная с физ. 512GB
       512GB = 0x0000_0080_0000_0000 → edx=0x80, eax=0x83 */
    mov edi, offset pdpt1
    mov eax, 0x83
    mov edx, 0x80
    mov ecx, 512
.fill_pdpt1:
    mov dword ptr [edi],     eax
    mov dword ptr [edi + 4], edx
    add eax, 0x40000000
    adc edx, 0
    add edi, 8
    loop .fill_pdpt1

    /* pdpt_high[0] → pd_kernel */
    mov eax, offset pd_kernel
    or  eax, 0x3
    mov dword ptr [pdpt_high],     eax
    mov dword ptr [pdpt_high + 4], 0

    /* pd_kernel[0] → 2MB страница (физ. 0x0, покрывает ядро) */
    mov dword ptr [pd_kernel],     0x83
    mov dword ptr [pd_kernel + 4], 0

    /* 6. включить PAE */
    mov eax, cr4
    or  eax, (1 << 5)
    mov cr4, eax

    /* 7. загрузить CR3 */
    mov eax, offset pml4
    mov cr3, eax

    /* 8. включить Long Mode (EFER MSR) */
    mov ecx, 0xC0000080
    rdmsr
    or  eax, (1 << 8)       /* LME бит */
    wrmsr

    /* 9. включить Paging (CR0) */
    mov eax, cr0
    or  eax, (1 << 31)
    mov cr0, eax

    /* 10. загрузить GDT */
    lgdt [gdt64_ptr]

    /* 11. far jump → 64-bit сегмент (raw-байты: EA + offset32 + selector16) */
    .byte 0xEA
    .long start64
    .word 0x0008

.halt32:
    hlt
    jmp .halt32

/* ============================================================
   64-bit code  (VMA = LMA = физический адрес, до прыжка в higher half)
   ============================================================ */
.code64
start64:
    /* 12. настроить сегментные регистры */
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    /* 13. прыгнуть в higher half (64-bit absolute jump через raw bytes) */
    .byte 0x48, 0xB8    /* REX.W + MOV RAX, imm64 */
    .quad start64_high
    jmp rax

/* ============================================================
   64-bit higher half code  (VMA = higher half)
   ============================================================ */
.section .text
.code64
.global start64_high
start64_high:
    /* 14. обновить стек на higher half адрес */
    mov rsp, offset stack_top

    /* 15. вызвать kernel_main(boot_info: u64) */
    mov rdi, rsi
    call kernel_main

.halt64:
    hlt
    jmp .halt64

/* ============================================================
   GDT  (в boot секции — нужна до включения paging)
   ============================================================ */
.section .rodata.boot
.align 8
gdt64:
    .quad 0x0000000000000000    /* 0x00: null */
gdt64_code:
    .quad 0x00AF9A000000FFFF    /* 0x08: 64-bit code, DPL=0 */
gdt64_data:
    .quad 0x00AF92000000FFFF    /* 0x10: 64-bit data, DPL=0 */
gdt64_end:

/* указатель с физическим адресом (для lgdt в 32-bit) */
gdt64_ptr:
    .short gdt64_end - gdt64 - 1
    .long  gdt64

/* ============================================================
   BSS: page tables + стек  (в boot секции — физические адреса)
   ============================================================ */
.section .bss.boot
.align 4096
pml4:       .space 4096
pdpt0:      .space 4096     /* identity 0-512GB */
pdpt1:      .space 4096     /* identity 512GB-1TB */
pdpt_high:  .space 4096     /* higher half */
pd_kernel:  .space 4096     /* ядро (2MB страницы) */

.align 16
stack_bottom:
    .space 16384            /* 16KB */
stack_top:
