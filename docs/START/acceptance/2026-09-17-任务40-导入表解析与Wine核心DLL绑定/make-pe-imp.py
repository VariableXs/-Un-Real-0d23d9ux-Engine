#!/usr/bin/env python3
"""任务40（AI-B）· 带导入表的 PE64 样例生成器 → kernel/varix/src/proc/hello-imp.pe。

与 make-pe.py 同一手工构造路线（零外部依赖，逐字节可解释），区别：
带 1 个导入 DLL（KERNEL32.DLL）× 3 个 API（WriteFile/GetProcAddress/
ExitProcess），IAT 独占可写节 .idata（绑定补钉前提），INT/描述符/名字
同在 .idata。代码全经 IAT 间接调用——运行期证据：
  1) WriteFile(1, "hello from winapi\\n", 18)   —— 经 thunk 陷内核 SYS_WRITE 路
  2) GetProcAddress("KERNEL32.DLL","WriteFile") 返回值与 IAT 槽内容比较
     —— 相等则打 "winapi: getproc == iat\\n" 并 ExitProcess(0)
     —— 不等则打 "winapi: getproc != iat\\n" 并 ExitProcess(7)
绑定语义（kernel/varix/src/proc/winapi.rs）：IAT 槽与 GetProcAddress 都指向
同一 thunk 槽地址（静态/动态两路一致的结构性保证）。
"""

import struct

BASE = 0x140000000
TEXT_RVA, TEXT_RAW, TEXT_SIZE = 0x1000, 0x400, 0x200
IDATA_RVA, IDATA_RAW, IDATA_SIZE = 0x2000, 0x600, 0x200
FILE_SIZE = 0x800

FUNC_ORDER = ["WriteFile", "GetProcAddress", "ExitProcess"]
DLL_NAME = b"KERNEL32.DLL\x00"

MSG = b"hello from winapi\n"
OK_MSG = b"winapi: getproc == iat\n"
BAD_MSG = b"winapi: getproc != iat\n"
SZ_DLL = b"KERNEL32.DLL\x00"
SZ_FN = b"WriteFile\x00"


def rva_of(fileoff: int) -> int:
    if TEXT_RAW <= fileoff < TEXT_RAW + TEXT_SIZE:
        return TEXT_RVA + (fileoff - TEXT_RAW)
    if IDATA_RAW <= fileoff < IDATA_RAW + IDATA_SIZE:
        return IDATA_RVA + (fileoff - IDATA_RAW)
    raise AssertionError(f"fileoff {fileoff:#x} 不在任何节内")


def build() -> bytes:
    img = bytearray(FILE_SIZE)

    # ---- DOS 头 ----
    img[0:2] = b"MZ"
    struct.pack_into("<I", img, 0x3C, 0x40)

    # ---- PE 签名 + COFF ----
    img[0x40:0x44] = b"PE\x00\x00"
    struct.pack_into("<H", img, 0x44, 0x8664)  # AMD64
    struct.pack_into("<H", img, 0x46, 2)       # 2 节
    struct.pack_into("<I", img, 0x4C, 0)       # 时间戳 0（可复现）
    struct.pack_into("<H", img, 0x54, 0xF0)    # 可选头大小
    struct.pack_into("<H", img, 0x56, 0x22)    # 特征

    # ---- PE32+ 可选头 @0x58 ----
    opt = 0x58
    struct.pack_into("<H", img, opt + 0, 0x20B)          # PE32+
    struct.pack_into("<B", img, opt + 2, 8)              #/linker major
    struct.pack_into("<I", img, opt + 16, TEXT_RVA)      # AddressOfEntryPoint
    struct.pack_into("<I", img, opt + 20, TEXT_RVA)      # BaseOfCode
    struct.pack_into("<Q", img, opt + 24, BASE)          # ImageBase
    struct.pack_into("<I", img, opt + 32, 0x1000)        # SectionAlignment
    struct.pack_into("<I", img, opt + 36, 0x200)         # FileAlignment
    struct.pack_into("<H", img, opt + 40, 6)             # major OS
    struct.pack_into("<H", img, opt + 44, 6)             # subsystem version
    struct.pack_into("<I", img, opt + 56, 0x4000)        # SizeOfImage
    struct.pack_into("<I", img, opt + 60, 0x400)         # SizeOfHeaders
    struct.pack_into("<H", img, opt + 68, 3)             # SubSystem = console
    struct.pack_into("<I", img, opt + 108, 16)           # NumberOfRvaAndSizes

    # ---- 数据目录[1] = 导入表 ----
    # .idata 布局（节内偏移）：
    #   0x00 描述符(20B) + 终止(20B) | 0x28 INT(32B) | 0x48 IAT(32B)
    #   0x68 DLL 名 | 0x78 hint+name 条目
    imp_rva = IDATA_RVA
    imp_size = 40
    struct.pack_into("<I", img, opt + 112 + 8, imp_rva)
    struct.pack_into("<I", img, opt + 112 + 12, imp_size)

    # ---- 节表 @ opt+0xF0 ----
    sh = opt + 0xF0
    img[sh:sh + 8] = b".text\x00\x00\x00"
    struct.pack_into("<I", img, sh + 8, TEXT_SIZE)       # VirtualSize
    struct.pack_into("<I", img, sh + 12, TEXT_RVA)
    struct.pack_into("<I", img, sh + 16, TEXT_SIZE)      # SizeOfRawData
    struct.pack_into("<I", img, sh + 20, TEXT_RAW)
    struct.pack_into("<I", img, sh + 36, 0x60000020)     # CODE|EXEC|READ
    sh2 = sh + 40
    img[sh2:sh2 + 8] = b".idata\x00\x00"
    struct.pack_into("<I", img, sh2 + 8, IDATA_SIZE)
    struct.pack_into("<I", img, sh2 + 12, IDATA_RVA)
    struct.pack_into("<I", img, sh2 + 16, IDATA_SIZE)
    struct.pack_into("<I", img, sh2 + 20, IDATA_RAW)
    struct.pack_into("<I", img, sh2 + 36, 0xC0000040)    # DATA|READ|WRITE

    # ---- .idata：描述符 + INT + IAT + 名字 ----
    int_rva = IDATA_RVA + 0x28
    iat_rva = IDATA_RVA + 0x48
    name_rva = IDATA_RVA + 0x68
    img[IDATA_RAW + 0:IDATA_RAW + 4] = struct.pack("<I", int_rva)      # OriginalFirstThunk
    struct.pack_into("<I", img, IDATA_RAW + 4, 0)                       # TimeDateStamp=0（未绑定）
    struct.pack_into("<I", img, IDATA_RAW + 8, 0xFFFFFFFF)              # ForwarderChain
    struct.pack_into("<I", img, IDATA_RAW + 12, name_rva)               # Name
    struct.pack_into("<I", img, IDATA_RAW + 16, iat_rva)                # FirstThunk
    # （终止描述符 = 20B 全零，bytearray 天然为零）

    img[IDATA_RAW + 0x68:IDATA_RAW + 0x68 + len(DLL_NAME)] = DLL_NAME

    # hint+name 条目链。
    hn_rva = IDATA_RVA + 0x78
    hn_off = IDATA_RAW + 0x78
    thunk_vals = []
    for fn in FUNC_ORDER:
        thunk_vals.append(hn_rva)
        struct.pack_into("<H", img, hn_off, 0)          # hint
        nb = fn.encode() + b"\x00"
        img[hn_off + 2:hn_off + 2 + len(nb)] = nb
        hn_rva += 2 + len(nb)
        hn_off += 2 + len(nb)
    for i, tv in enumerate(thunk_vals):
        struct.pack_into("<Q", img, IDATA_RAW + 0x28 + 8 * i, tv)  # INT
        struct.pack_into("<Q", img, IDATA_RAW + 0x48 + 8 * i, tv)  # IAT（未绑定 RVA）

    # ---- .text：代码 + 数据（两遍装配算 RIP 相对位移）----
    iat_w = iat_rva                       # 槽 0 WriteFile
    iat_gp = iat_rva + 8                  # 槽 1 GetProcAddress
    iat_ex = iat_rva + 16                 # 槽 2 ExitProcess
    data_off = TEXT_RAW + 0x90            # 代码区后：msg/ok/bad/sz_dll/sz_fn
    va_of = lambda fo: BASE + rva_of(fo)
    t_msg, t_ok, t_bad = data_off, data_off + len(MSG), data_off + len(MSG) + len(OK_MSG)
    t_dll, t_fn = (
        t_bad + len(BAD_MSG),
        t_bad + len(BAD_MSG) + len(SZ_DLL),
    )

    code = bytearray()
    relocs = []  # (insn 末文件偏移, 目标文件偏移, is_rel8)
    iat_file = lambda rva: IDATA_RAW + (rva - IDATA_RVA)

    def emit(*parts):
        code.extend(parts)

    # WriteFile(1, msg, len(MSG))
    emit(0xB9, 1, 0, 0, 0)                                   # mov ecx,1
    emit(0x48, 0x8D, 0x15); relocs.append((TEXT_RAW + len(code) + 4, t_msg, False)); emit(0, 0, 0, 0)
    emit(0x41, 0xB8, len(MSG), 0, 0, 0)                      # mov r8d,len
    emit(0xFF, 0x15); relocs.append((TEXT_RAW + len(code) + 4, iat_file(iat_w), False)); emit(0, 0, 0, 0)
    # GetProcAddress(SZ_DLL, SZ_FN)
    emit(0x48, 0x8D, 0x0D); relocs.append((TEXT_RAW + len(code) + 4, t_dll, False)); emit(0, 0, 0, 0)
    emit(0x48, 0x8D, 0x15); relocs.append((TEXT_RAW + len(code) + 4, t_fn, False)); emit(0, 0, 0, 0)
    emit(0xFF, 0x15); relocs.append((TEXT_RAW + len(code) + 4, iat_file(iat_gp), False)); emit(0, 0, 0, 0)
    # mov r10,[iat_w]; cmp rax,r10; jne fail
    emit(0x4C, 0x8B, 0x15); relocs.append((TEXT_RAW + len(code) + 4, iat_file(iat_w), False)); emit(0, 0, 0, 0)
    emit(0x4C, 0x39, 0xD0)                                   # cmp rax,r10
    emit(0x75); jne_at = len(code); emit(0)                  # jne rel8（fail 位置后面回填）
    # 成功路：WriteFile(1, ok, len) ; ExitProcess(0)
    emit(0xB9, 1, 0, 0, 0)
    emit(0x48, 0x8D, 0x15); relocs.append((TEXT_RAW + len(code) + 4, t_ok, False)); emit(0, 0, 0, 0)
    emit(0x41, 0xB8, len(OK_MSG), 0, 0, 0)
    emit(0xFF, 0x15); relocs.append((TEXT_RAW + len(code) + 4, IDATA_RAW + (iat_w - IDATA_RVA), False)); emit(0, 0, 0, 0)
    emit(0x31, 0xC9)                                         # xor ecx,ecx
    emit(0xFF, 0x15); relocs.append((TEXT_RAW + len(code) + 4, IDATA_RAW + (iat_ex - IDATA_RVA), False)); emit(0, 0, 0, 0)
    fail_off = len(code)
    # 失败路：WriteFile(1, bad, len) ; ExitProcess(7)
    emit(0xB9, 1, 0, 0, 0)
    emit(0x48, 0x8D, 0x15); relocs.append((TEXT_RAW + len(code) + 4, t_bad, False)); emit(0, 0, 0, 0)
    emit(0x41, 0xB8, len(BAD_MSG), 0, 0, 0)
    emit(0xFF, 0x15); relocs.append((TEXT_RAW + len(code) + 4, IDATA_RAW + (iat_w - IDATA_RVA), False)); emit(0, 0, 0, 0)
    emit(0xB9, 7, 0, 0, 0)                                   # mov ecx,7
    emit(0xFF, 0x15); relocs.append((TEXT_RAW + len(code) + 4, IDATA_RAW + (iat_ex - IDATA_RVA), False)); emit(0, 0, 0, 0)

    assert len(code) <= 0x90, f"代码区 {len(code):#x} 超出预留 0x90"
    img[TEXT_RAW:TEXT_RAW + len(code)] = code
    # jne rel8 回填（fail 与 jne 下一指令同节内，差值 < 128）。
    rel = fail_off - (jne_at + 1)
    assert 0 < rel < 128
    img[TEXT_RAW + jne_at] = rel
    # RIP 相对位移回填：disp = 目标 VA - (RIP=下一条指令 VA)。
    for end_off, target_off, _ in relocs:
        disp = va_of(target_off) - va_of(end_off)
        struct.pack_into("<i", img, end_off - 4, disp)

    # 字符串落位。
    img[t_msg:t_msg + len(MSG)] = MSG
    img[t_ok:t_ok + len(OK_MSG)] = OK_MSG
    img[t_bad:t_bad + len(BAD_MSG)] = BAD_MSG
    img[t_dll:t_dll + len(SZ_DLL)] = SZ_DLL
    img[t_fn:t_fn + len(SZ_FN)] = SZ_FN
    return bytes(img)


if __name__ == "__main__":
    import pathlib
    out = pathlib.Path(__file__).resolve().parents[1] / "kernel/varix/src/proc/hello-imp.pe"
    blob = build()
    out.write_bytes(blob)
    print(f"wrote {out} ({len(blob)} bytes)")
