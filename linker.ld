ENTRY(start32)

KERNEL_PHYS = 0x100000;
KERNEL_VIRT = 0xFFFF800000000000;

SECTIONS {
    /* Boot секция: VMA = LMA = физический адрес
       Содержит: multiboot2 header, start32, start64,
                 GDT, page tables, стек */
    . = KERNEL_PHYS;

    .boot ALIGN(4096) : {
        KEEP(*(.text.multiboot2_header))
        KEEP(*(.text.boot))
        KEEP(*(.rodata.boot))
        *(.bss.boot)
    }

    boot_end = .;

    /* Остальное ядро: VMA = higher half, LMA = физический */
    . = KERNEL_VIRT + boot_end;

    .text ALIGN(4096) : AT(boot_end) {
        *(.text .text.* .ltext .ltext.*)
    }

    .rodata ALIGN(4096) : AT(ADDR(.rodata) - KERNEL_VIRT) {
        *(.rodata .rodata.*)
    }

    .data ALIGN(4096) : AT(ADDR(.data) - KERNEL_VIRT) {
        *(.data .data.*)
    }

    .bss ALIGN(4096) : AT(ADDR(.bss) - KERNEL_VIRT) {
        *(.bss .bss.*)
        *(COMMON)
    }

    /DISCARD/ : {
        *(.eh_frame)
        *(.note.*)
    }
}
