OUTPUT_ARCH(riscv)
ENTRY(_start)
BASE_ADDRESS = 0x80200000;

SECTIONS
{
    . = BASE_ADDRESS;
    skernel = .;

    stext = .;
    .text : {
        _stext = .;
        *(.text.entry)
        . = ALIGN(4K);
        strampoline = .;
        *(.text.trampoline);
        . = ALIGN(4K);
        *(.text .text.*)
                . = ALIGN(4K);
        _swap_frame = .;
        *(.swap)
        ASSERT(. - _swap_frame <= 4K, "swap frame larger than one page");
        _etext = .;
    }

    . = ALIGN(4K);
    etext = .;
    srodata = .;
    .rodata : {
        _srodata = .;
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
        _erodata = .;
    }

    . = ALIGN(4K);
    erodata = .;
    sdata = .;
    .data : ALIGN(4k){
        _sidata = LOADADDR(.data);
        _sdata = .;
        *(.data .data.*)
        *(.sdata .sdata.*)
        _edata = .;
    }

    . = ALIGN(4K);
    edata = .;
    sbss_with_stack = .;
    .bss : {
        _sbss = .;
        *(.bss.stack)
        sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        _ebss = .;
    }
    .kernel_modules : {
        __kernel_modules_start = .;
        KEEP(*(.kernel_modules.init))
        __kernel_modules_end = .;
    }

    . = ALIGN(4K);
    ebss = .;
    __end = .;
    free_memory_start = .;
    ekernel = .;

    /DISCARD/ : {
        *(.eh_frame)
    }
}
