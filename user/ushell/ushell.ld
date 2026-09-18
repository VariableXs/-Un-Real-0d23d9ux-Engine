OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

/* 用户半区低段固定入口（< USER_TOP=0x0000_8000_0000_0000）。同 hello.ld。 */
SECTIONS
{
    . = 0x400000;

    .text : ALIGN(0x1000)
    {
        *(.text._start)
        *(.text .text.*)
    }

    .rodata : ALIGN(0x1000)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(0x1000)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(0x1000)
    {
        *(.bss .bss.*)
        *(COMMON)
    }

    /DISCARD/ :
    {
        *(.eh_frame .eh_frame_hdr .eh_frame_entry)
        *(.note .note.*)
        *(.comment)
        *(.debug_*)
    }
}
