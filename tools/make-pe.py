#!/usr/bin/env python3
"""任务39（AI-B）· 生成可真实运行的静态 PE64 样例 hello.pe。

不加任何第三方依赖，纯手工构造 PE32+（x86-64）映像：
- 单个 .text 节（R|X），代码 = write(1,"hello from PE\\n",14) 后 exit(0)，
  与 hello.elf 走同一 syscall ABI（rax=nr, rdi/rsi/rdx=a1/a2/a3）；
- 无导入表（NumberOfRvaAndSizes=16 但数据目录全零）——任务39 边界：
  只装载运行，不做导入解析（任务40 的活）；
- ImageBase=0x140000000，SectionAlignment=FileAlignment 页对齐。

用法：python tools/make-pe.py   （产物写入 kernel/varix/src/proc/hello.pe）
"""

import struct
import sys

BASE = 0x140000000
TEXT_RVA = 0x1000
FILE_ALIGN = 0x200
SEC_ALIGN = 0x1000
MSG_RVA = TEXT_RVA + 0x40  # msg 与代码隔开，位于同一节内
MSG = b"hello from PE\n"

# ---- 代码节 ----
# mov rax,2 (SYS_WRITE); mov rdi,1; lea rsi,[rip+disp32]; mov rdx,len;
# syscall; xor eax,eax (SYS_EXIT=0); xor edi,edi; syscall
code = b"".join(
    [
        b"\x48\xC7\xC0\x02\x00\x00\x00",  # mov rax,2
        b"\x48\xC7\xC7\x01\x00\x00\x00",  # mov rdi,1
        b"\x48\x8D\x35", b"",  # lea rsi,[rip+disp]（disp 回填）
        b"\x48\xC7\xC2" + struct.pack("<I", len(MSG)),  # mov rdx,len
        b"\x0F\x05",  # syscall
        b"\x31\xC0",  # xor eax,eax → SYS_EXIT=0
        b"\x31\xFF",  # xor edi,edi → exit(0)
        b"\x0F\x05",  # syscall
    ]
)
LEA_LEN = 7
lea_end_rva = TEXT_RVA + 7 + 7 + LEA_LEN
disp = MSG_RVA - lea_end_rva
code = code.replace(b"\x48\x8D\x35" + b"", b"\x48\x8D\x35" + struct.pack("<i", disp), 1)
assert len(code) <= MSG_RVA - TEXT_RVA, "代码不得越过 msg 槽"

# ---- 头部 ----
sec_raw = FILE_ALIGN  # .text 原始尺寸（文件对齐）
headers_raw = FILE_ALIGN
size_of_image = SEC_ALIGN * 2  # 头页 + 一节页

dos = bytearray(0x80)
dos[0:2] = b"MZ"
struct.pack_into("<I", dos, 0x3C, 0x80)

pe = bytearray()
pe += b"PE\x00\x00"
# COFF（20B）
pe += struct.pack(
    "<HHIIIHH",
    0x8664,  # Machine = AMD64
    1,  # NumberOfSections
    0, 0, 0,  # TimeStamp/SymbolTable/NumSymbols
    0xF0,  # SizeOfOptionalHeader（PE32+ = 240）
    0x0022,  # Characteristics: EXECUTABLE_IMAGE | LARGE_ADDRESS_AWARE
)
# Optional header PE32+（240B）
opt = bytearray()
opt += struct.pack("<HBB", 0x20B, 14, 0)  # magic, linkerver
opt += struct.pack("<III", len(code), 0, 0)  # SizeOfCode/Init/Uninit
opt += struct.pack("<II", TEXT_RVA, TEXT_RVA)  # AddressOfEntryPoint, BaseOfCode
opt += struct.pack("<Q", BASE)  # ImageBase
opt += struct.pack("<II", SEC_ALIGN, FILE_ALIGN)
opt += struct.pack("<HHHHHH", 6, 0, 0, 0, 3, 0)  # OS/Img/Sub 版本
opt += struct.pack("<IIII", 0, size_of_image, headers_raw, 0)  # Win32Ver/SizeOfImage/SizeOfHeaders/CheckSum
opt += struct.pack("<HH", 3, 0)  # Subsystem=console, DllCharacteristics
opt += struct.pack("<QQQQ", 0x100000, 0x1000, 0x100000, 0x1000)  # 栈/堆 保留/提交
opt += struct.pack("<II", 0, 16)  # LoaderFlags, NumberOfRvaAndSizes
opt += b"\x00" * (16 * 8)  # 16 个数据目录全零 = 无导入/无重定位/无异常表
assert len(opt) == 0xF0
pe += opt
# 节表（40B ×1）
sec = bytearray()
sec += b".text\x00\x00\x00"
sec += struct.pack("<IIII", len(code) + (MSG_RVA - TEXT_RVA) + len(MSG), TEXT_RVA, sec_raw, headers_raw)
sec += struct.pack("<IIHHI", 0, 0, 0, 0, 0x60000020)  # CODE|EXECUTE|READ
assert len(sec) == 40
pe += sec
assert len(pe) <= headers_raw

img = bytearray(size_of_image)
img[0 : len(dos)] = dos
img[0x80 : 0x80 + len(pe)] = pe
img[TEXT_RVA : TEXT_RVA + len(code)] = code
img[MSG_RVA : MSG_RVA + len(MSG)] = MSG
blob = bytes(img[:headers_raw]) + bytes(img[TEXT_RVA:TEXT_RVA + sec_raw])

out = sys.argv[1] if len(sys.argv) > 1 else "kernel/varix/src/proc/hello.pe"
with open(out, "wb") as f:
    f.write(blob)
print(f"wrote {out}: {len(blob)} bytes, entry VA={BASE + TEXT_RVA:#x}, msg VA={BASE + MSG_RVA:#x}")
