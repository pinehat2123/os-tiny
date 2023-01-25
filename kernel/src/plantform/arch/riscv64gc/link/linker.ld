OUTPUT_ARCH(riscv)
SECTIONS {
    .text 0x80200000 : {
        __start = .;
        *(.text.entry)
        *(.text .text.*)
    }
    .rodata : ALIGN(4K) {
        __rodata = .;
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }
    .data : ALIGN(4K) {
        __data = .;
        *(.data .data.*)
        *(.sdata .sdata.*)
    }
    .bss : ALIGN(8) {
        __sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        __ebss = .;
    }
    .boot : ALIGN(4K) {
        __boot = .;
        KEEP(*(.boot.stack))
    }
    __end = .;
}