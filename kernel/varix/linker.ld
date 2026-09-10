/* Varix kernel linker script — Limine protocol, higher-half image.
 * Bootloader scans the whole executable for request magics, so no special
 * ordering is required; KEEP() guards against linker section GC. */
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

SECTIONS
{
    . = 0xffffffff80000000;

    .limine_requests : ALIGN(8)
    {
        KEEP(*(.limine_requests))
    }

    .text : ALIGN(16)
    {
        *(.text .text.*)
    }

    .rodata : ALIGN(16)
    {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }

    .data : ALIGN(16)
    {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }

    .bss : ALIGN(16)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    __kernel_end = .;

    /DISCARD/ :
    {
        *(.eh_frame*)
        *(.note .note.*)
        *(.comment)
    }
}
