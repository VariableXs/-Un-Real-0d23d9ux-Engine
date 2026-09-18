#!/usr/bin/env python3
"""任务41（AI-B）· 记事本级闭环 PE64 样例生成器 → kernel/varix/src/proc/notepad.pe。

与 make-pe-imp.py 同一手工构造路线（零外部依赖，逐字节可解释），规模升级：
- 4 个导入 DLL：KERNEL32.DLL(4) / USER32.DLL(7) / GDI32.DLL(3) / COMDLG32.DLL(2)；
- 完整 Win32 消息循环（用户态内联 WndProc 语义——单窗口演示等价，内核
  不回调 ring3，见 winsrv 模块头声明）：
    RegisterClassExW → CreateWindowExW(WM_CREATE) → ShowWindow →
    UpdateWindow(WM_PAINT 入队) → 循环 GetMessageW → switch：
      WM_PAINT  : BeginPaint → TextOutW(编辑缓冲) → EndPaint
      WM_CHAR   : 追加编辑缓冲 → InvalidateRect(重绘入队)
      WM_COMMAND: IDM_OPEN=GetOpenFileNameW+ReadFile(h3)+装载
                  IDM_SAVE=GetSaveFileNameW+窄化+WriteFile(h2)+CloseHandle
                  IDM_EXIT=PostQuitMessage → GetMessageW 返回 0 退出
    → ExitProcess(0)
- 闭环运行时证据：探针注入 菜单打开/字符/菜单保存 序列后，
  保存内容 == 打开内容 + 注入字符（串口 VERDICT 行）。

运行期 ABI（kernel/varix/src/proc/winapi.rs + winsrv::win32_dispatch）：
- 多参 API 走 NT 风格参数块（rcx = 用户态结构指针）；
- ReadFile = 参数块 rfparam{handle@0,buf@8,len@16,got@24}（真 Win32 4 参
  经 3 参 syscall 装不下，参数块对齐 winsrv 多参约定——根因⑤实证）；
- WriteFile = 3 参直传（handle=2, buf, len；bytes-written 出参缺省）；
  句柄分类：2 = 当前保存目标，3 = 当前读取源；
- GetMessageW 空队列写 WM_QUIT 返回 0（探针环境等价收尾）。
"""

import struct

BASE = 0x140000000
# 三节 W^X 分离：.text(RO+X) / .data(RW 可写全局) / .idata(RW 导入表)。
# 根因④实证：可写全局曾混入 RO+X 的 .text（TEXT_SIZE=0x1000 罩住数据区）→
# ring3 首个全局写（CreateWindowExW 返回存 HWND）即权限缺页 err=0x7——
# loader flags 并集+W^X 口径正确拒写，PE 必须自带独立 .data 节。
TEXT_RVA, TEXT_RAW = 0x1000, 0x400
TEXT_SIZE = 0x400
DATA_RVA, DATA_RAW = 0x2000, 0x800
DATA_SIZE = 0xC00
IDATA_RVA, IDATA_RAW = 0x3000, 0x1400
IDATA_SIZE = 0x800
FILE_SIZE = 0x2000

DLLS = [
    ("KERNEL32.DLL", ["ExitProcess", "WriteFile", "ReadFile", "CloseHandle"]),
    ("USER32.DLL", ["RegisterClassExW", "CreateWindowExW", "ShowWindow",
                    "UpdateWindow", "InvalidateRect", "GetMessageW", "PostQuitMessage"]),
    ("GDI32.DLL", ["TextOutW", "BeginPaint", "EndPaint"]),
    ("COMDLG32.DLL", ["GetOpenFileNameW", "GetSaveFileNameW"]),
]

WM_PAINT, WM_CHAR, WM_COMMAND = 0x0F, 0x102, 0x111
IDM_OPEN, IDM_SAVE, IDM_EXIT = 1, 2, 3
EDIT_UNITS = 400

slot = 0
SLOT = {}
for _dll, fns in DLLS:
    for f in fns:
        SLOT[f] = slot
        slot += 1
    slot += 1  # 终止槽：与 .idata 布局 cur_slots += len(fns)+1 对齐（漏加会导致
               # user32 起全部 call 错引前一 DLL 终止槽 → ring3 读 0 → 取指 fault）


def rva_of(fileoff: int) -> int:
    if TEXT_RAW <= fileoff < TEXT_RAW + TEXT_SIZE:
        return TEXT_RVA + (fileoff - TEXT_RAW)
    if DATA_RAW <= fileoff < DATA_RAW + DATA_SIZE:
        return DATA_RVA + (fileoff - DATA_RAW)
    if IDATA_RAW <= fileoff < IDATA_RAW + IDATA_SIZE:
        return IDATA_RVA + (fileoff - IDATA_RAW)
    raise AssertionError(f"fileoff {fileoff:#x} 不在任何节内")


def build() -> bytes:
    img = bytearray(FILE_SIZE)

    # ---- DOS/PE/COFF 头 ----
    img[0:2] = b"MZ"
    struct.pack_into("<I", img, 0x3C, 0x40)
    img[0x40:0x44] = b"PE\x00\x00"
    struct.pack_into("<H", img, 0x44, 0x8664)
    struct.pack_into("<H", img, 0x46, 3)  # COFF NumberOfSections：.text/.data/.idata
    struct.pack_into("<I", img, 0x4C, 0)
    struct.pack_into("<H", img, 0x54, 0xF0)
    struct.pack_into("<H", img, 0x56, 0x22)
    opt = 0x58
    struct.pack_into("<H", img, opt + 0, 0x20B)
    struct.pack_into("<B", img, opt + 2, 8)
    struct.pack_into("<I", img, opt + 4, TEXT_SIZE)             # SizeOfCode
    struct.pack_into("<I", img, opt + 8, DATA_SIZE + IDATA_SIZE)  # SizeOfInitializedData
    struct.pack_into("<I", img, opt + 16, TEXT_RVA)             # AddressOfEntryPoint
    struct.pack_into("<I", img, opt + 20, TEXT_RVA)             # BaseOfCode
    struct.pack_into("<Q", img, opt + 24, BASE)
    struct.pack_into("<I", img, opt + 32, 0x1000)
    struct.pack_into("<I", img, opt + 36, 0x200)
    struct.pack_into("<H", img, opt + 40, 6)
    struct.pack_into("<H", img, opt + 44, 6)
    struct.pack_into("<I", img, opt + 56, 0x4000)
    struct.pack_into("<I", img, opt + 60, 0x400)
    struct.pack_into("<H", img, opt + 68, 3)
    struct.pack_into("<I", img, opt + 108, 16)
    struct.pack_into("<I", img, opt + 112 + 8, IDATA_RVA)
    struct.pack_into("<I", img, opt + 112 + 12, (len(DLLS) + 1) * 20)

    # ---- 节表（3 节，W^X 分离；NumberOfSections=3 见 opt+68）----
    sh = opt + 0xF0
    for i, (nm, vsz, rva, raw, ch) in enumerate([
        (b".text\x00\x00\x00", TEXT_SIZE, TEXT_RVA, TEXT_RAW, 0x60000020),
        (b".data\x00\x00\x00", DATA_SIZE, DATA_RVA, DATA_RAW, 0xC0000040),
        (b".idata\x00\x00", IDATA_SIZE, IDATA_RVA, IDATA_RAW, 0xC0000040),
    ]):
        s = sh + i * 40
        img[s:s + 8] = nm
        struct.pack_into("<I", img, s + 8, vsz)
        struct.pack_into("<I", img, s + 12, rva)
        struct.pack_into("<I", img, s + 16, vsz)
        struct.pack_into("<I", img, s + 20, raw)
        struct.pack_into("<I", img, s + 36, ch)

    # ---- .idata 布局 ----
    n_slots = sum(len(fns) + 1 for _, fns in DLLS)
    desc_off = IDATA_RAW
    int_off = desc_off + (len(DLLS) + 1) * 20
    iat_off = int_off + n_slots * 8
    dllname_off = iat_off + n_slots * 8
    hn_off = dllname_off + sum(len(d) + 1 for d, _ in DLLS)

    int_rvas = []
    iat_rvas = []
    cur_slots = 0
    for _d, fns in DLLS:
        int_rvas.append(IDATA_RVA + (int_off + cur_slots * 8 - IDATA_RAW))
        iat_rvas.append(IDATA_RVA + (iat_off + cur_slots * 8 - IDATA_RAW))
        cur_slots += len(fns) + 1

    dn = dllname_off
    for di, (dll, _fns) in enumerate(DLLS):
        off = desc_off + di * 20
        struct.pack_into("<I", img, off + 0, int_rvas[di])
        struct.pack_into("<I", img, off + 8, 0xFFFFFFFF)
        struct.pack_into("<I", img, off + 12, IDATA_RVA + (dn - IDATA_RAW))
        struct.pack_into("<I", img, off + 16, iat_rvas[di])
        nb = dll.encode() + b"\x00"
        img[dn:dn + len(nb)] = nb
        dn += len(nb)

    hn = hn_off
    for di, (_dll, fns) in enumerate(DLLS):
        base_int = int_off + sum(len(f2) + 1 for _d2, f2 in DLLS[:di]) * 8
        base_iat = iat_off + sum(len(f2) + 1 for _d2, f2 in DLLS[:di]) * 8
        for fi, f in enumerate(fns):
            tv = IDATA_RVA + (hn - IDATA_RAW)
            struct.pack_into("<Q", img, base_int + fi * 8, tv)
            struct.pack_into("<Q", img, base_iat + fi * 8, tv)
            struct.pack_into("<H", img, hn, 0)
            nb = f.encode() + b"\x00"
            img[hn + 2:hn + 2 + len(nb)] = nb
            hn += 2 + len(nb)
        # 终止槽（全零）。
    idata_end = hn
    assert idata_end <= IDATA_RAW + IDATA_SIZE, f".idata 溢出 {idata_end:#x}"

    # ---- .data 数据区布局（独立 RW 节，见头部三节 W^X 注）----
    data_off = DATA_RAW
    va = lambda fo: BASE + rva_of(fo)

    sz_class = b"NP\x00"
    sz_title = b"Notepad\x00"
    sz_class_fo = data_off
    sz_title_fo = sz_class_fo + len(sz_class)
    wcex_fo = sz_title_fo + len(sz_title)
    cw_fo = wcex_fo + 72
    to_fo = cw_fo + 32
    ofn_fo = to_fo + 32
    msg_fo = ofn_fo + 96
    ps_fo = msg_fo + 24
    hwnd_fo = ps_fo + 64
    editlen_fo = hwnd_fo + 8
    got_fo = editlen_fo + 8
    editbuf_fo = got_fo + 8
    ansibuf_fo = editbuf_fo + EDIT_UNITS * 2
    fnbuf_fo = ansibuf_fo + EDIT_UNITS
    rfparam_fo = fnbuf_fo + 64
    data_end = rfparam_fo + 32
    assert data_end <= DATA_RAW + DATA_SIZE, f".data 溢出 {data_end:#x}"

    wcex_va, cw_va, to_va = va(wcex_fo), va(cw_fo), va(to_fo)
    ofn_va, msg_va, ps_va = va(ofn_fo), va(msg_fo), va(ps_fo)
    editbuf_va, ansibuf_va, fnbuf_va = va(editbuf_fo), va(ansibuf_fo), va(fnbuf_fo)
    got_va, rfparam_va = va(got_fo), va(rfparam_fo)
    sz_class_va, sz_title_va = va(sz_class_fo), va(sz_title_fo)
    iat_fo = lambda fname: iat_off + SLOT[fname] * 8

    # ---- 代码装配 ----
    code = bytearray()
    # (指令末偏移, 目标文件偏移)——RIP 相对 disp32。
    relocs = []
    labels = {}
    fixups = []  # (code 内偏移, 字节数, 目标 label, kind)

    def emit(*b):
        code.extend(b)

    def rip_rel(tail, target_fo):
        """tail 后跟 RIP 相对 disp32。"""
        code.extend(tail)
        relocs.append((TEXT_RAW + len(code) + 4, target_fo))
        emit(0, 0, 0, 0)

    def call_iat(fname):
        code.extend([0xFF, 0x15])
        relocs.append((TEXT_RAW + len(code) + 4, iat_fo(fname)))
        emit(0, 0, 0, 0)

    def lea_rcx(fo):
        rip_rel([0x48, 0x8D, 0x0D], fo)

    def lea_rdx(fo):
        rip_rel([0x48, 0x8D, 0x15], fo)

    def lea_r9(fo):
        rip_rel([0x4C, 0x8D, 0x0D], fo)

    def mov_rax_rip(fo):
        rip_rel([0x48, 0x8B, 0x05], fo)

    def mov_eax_rip(fo):
        rip_rel([0x8B, 0x05], fo)

    def mov_r8d_rip(fo):
        rip_rel([0x44, 0x8B, 0x05], fo)

    def store_rax(fo):
        """mov [rip+disp32], rax"""
        rip_rel([0x48, 0x89, 0x05], fo)

    def store_rcx(fo):
        rip_rel([0x48, 0x89, 0x0D], fo)

    def store_rax_m64(fo):
        """mov moffs64（绝对地址回填 VA）——不使用，保留说明。"""
        raise AssertionError("用 store_rax（RIP 相对）")

    def label(name):
        labels[name] = len(code)

    def jcc32(cc, target_label):
        """0F 8x rel32（长条件跳转）。"""
        emit(0x0F, 0x80 + cc)
        fixups.append((len(code), 4, target_label, "rel32"))
        emit(0, 0, 0, 0)

    def jmp32(target_label):
        emit(0xE9)
        fixups.append((len(code), 4, target_label, "rel32"))
        emit(0, 0, 0, 0)

    # _start：
    lea_rcx(wcex_fo)
    call_iat("RegisterClassExW")
    emit(0x66, 0x85, 0xC0)            # test ax, ax
    jcc32(4, "fail")                  # je fail

    lea_rcx(cw_fo)
    call_iat("CreateWindowExW")
    emit(0x85, 0xC0)                  # test eax, eax
    jcc32(4, "fail")
    store_rax(hwnd_fo)                # [hwnd] = hwnd

    # ShowWindow(hwnd, 1)
    rip_rel([0x48, 0x8B, 0x0D], hwnd_fo)  # mov rcx, [hwnd]
    emit(0xBA, 1, 0, 0, 0)            # mov edx, 1
    call_iat("ShowWindow")
    # UpdateWindow(hwnd) → WM_PAINT 入队
    rip_rel([0x48, 0x8B, 0x0D], hwnd_fo)
    call_iat("UpdateWindow")

    # ---- 消息循环 ----
    label("loop_top")
    lea_rcx(msg_fo)
    emit(0x31, 0xD2)                  # xor edx, edx
    emit(0x45, 0x31, 0xC0)            # xor r8d, r8d
    emit(0x45, 0x31, 0xC9)            # xor r9d, r9d
    call_iat("GetMessageW")
    emit(0x85, 0xC0)                  # test eax, eax
    jcc32(4, "done")                  # 0 = WM_QUIT → 退出

    mov_eax_rip(msg_fo + 4)           # mov eax, [msg.message]
    emit(0x3D, WM_PAINT, 0, 0, 0)
    jcc32(4, "paint")
    emit(0x3D, *struct.pack('<I', WM_CHAR))
    jcc32(4, "char")
    emit(0x3D, *struct.pack('<I', WM_COMMAND))
    jcc32(4, "cmd")
    jmp32("loop_top")

    # ---- paint ----
    label("paint")
    rip_rel([0x48, 0x8B, 0x0D], hwnd_fo)
    lea_rdx(ps_fo)
    call_iat("BeginPaint")            # rax = hdc（约定 1）
    # to_args.len = editlen
    mov_rax_rip(editlen_fo)
    rip_rel([0x48, 0xA3], to_fo + 24) if False else store_rax(to_fo + 24)
    lea_rcx(to_fo)
    call_iat("TextOutW")
    rip_rel([0x48, 0x8B, 0x0D], hwnd_fo)
    lea_rdx(ps_fo)
    call_iat("EndPaint")
    jmp32("loop_top")

    # ---- char：追加 editbuf[rcx], rcx=editlen ----
    label("char")
    mov_rax_rip(msg_fo + 8)           # wparam = 字符
    rip_rel([0x48, 0x8B, 0x0D], editlen_fo)  # mov rcx, [editlen]
    emit(0x48, 0x81, 0xF9, EDIT_UNITS & 0xFF, EDIT_UNITS >> 8, 0, 0)  # cmp rcx, EDIT_UNITS
    jcc32(3, "loop_top")              # jae loop（满则丢）
    emit(0x66, 0x89, 0x44, 0x4D, 0x00)  # mov [rbp + rcx*2], ax（editbuf 基址）
    emit(0x48, 0xFF, 0xC1)            # inc rcx
    store_rcx(editlen_fo)
    rip_rel([0x48, 0x8B, 0x0D], hwnd_fo)
    call_iat("InvalidateRect")        # 重绘入队
    jmp32("loop_top")

    # ---- cmd ----
    label("cmd")
    mov_rax_rip(msg_fo + 8)           # wparam = IDM
    emit(0x48, 0x83, 0xF8, IDM_OPEN)
    jcc32(4, "open")
    emit(0x48, 0x83, 0xF8, IDM_SAVE)
    jcc32(4, "save")
    emit(0x31, 0xC9)                  # IDM_EXIT → PostQuitMessage(0)
    call_iat("PostQuitMessage")
    jmp32("loop_top")

    # ---- open：GetOpenFileNameW → ReadFile(3) → widen 装载 ----
    label("open")
    lea_rcx(ofn_fo)
    call_iat("GetOpenFileNameW")
    emit(0x85, 0xC0)
    jcc32(4, "loop_top")              # 失败 → 回循环
    # ReadFile 走 NT 风格参数块 rfparam{handle,buf,len,got}——真 Win32 4 参
    # (handle,buf,len,&read) 经 3 参 syscall 装不下，曾把 a3=长度当出参
    # 指针裸写 → 内核态写缺页（根因⑤）。参数块与本 PE 其余多参 API 同构。
    lea_rcx(rfparam_fo)
    call_iat("ReadFile")
    # widen：editlen=0；rcx 计数。rbx/rbp = ansibuf/editbuf 基址——x64 的
    # 无基址 SIB（mod=00,base=101）是绝对 disp32 寻址，表达不了 >4GB 的
    # image VA，且回填机制写的是 RIP 相对 delta → 曾在 widen 首迭代读
    # 绝对 0x12f9 缺页（根因⑥）；基址寄存器走 SysV callee-saved（rbx/rbp）
    # 内核 dispatch 保存，thunk 亦不触碰。
    emit(0x48, 0x31, 0xC9)
    store_rcx(editlen_fo)
    rip_rel([0x48, 0x8D, 0x1D], ansibuf_fo)   # lea rbx, [rip+ansibuf]
    rip_rel([0x48, 0x8D, 0x2D], editbuf_fo)   # lea rbp, [rip+editbuf]
    label("widen_top")
    rip_rel([0x48, 0x3B, 0x0D], got_fo)  # cmp rcx, [got]
    jcc32(3, "loop_top")              # jae loop
    emit(0x0F, 0xB6, 0x04, 0x0B)      # movzx eax, byte [rbx + rcx]（SIB 0B=idx rcx,base rbx）
    emit(0x66, 0x89, 0x44, 0x4D, 0x00)  # mov [rbp + rcx*2], ax
    emit(0x48, 0xFF, 0xC1)            # inc rcx
    store_rcx(editlen_fo)
    jmp32("widen_top")

    # ---- save：GetSaveFileNameW → 窄化 → WriteFile(2) → CloseHandle(2) ----
    label("save")
    lea_rcx(ofn_fo)
    call_iat("GetSaveFileNameW")
    emit(0x85, 0xC0)
    jcc32(4, "loop_top")
    emit(0x4D, 0x31, 0xD2)            # xor r10, r10
    rip_rel([0x48, 0x8D, 0x1D], ansibuf_fo)   # lea rbx, [rip+ansibuf]
    rip_rel([0x48, 0x8D, 0x2D], editbuf_fo)   # lea rbp, [rip+editbuf]
    label("narrow_top")
    rip_rel([0x4C, 0x3B, 0x15], editlen_fo)  # cmp r10, [editlen]
    jcc32(3, "do_write")              # jae do_write
    emit(0x4C, 0x89, 0xD1)            # mov rcx, r10
    emit(0x0F, 0xB7, 0x44, 0x4D, 0x00)  # movzx eax, word [rbp + rcx*2]
    emit(0x88, 0x04, 0x0B)            # mov [rbx + rcx], al（SIB 0B=idx rcx,base rbx）
    emit(0x49, 0xFF, 0xC2)            # inc r10
    jmp32("narrow_top")
    label("do_write")
    emit(0xB9, 2, 0, 0, 0)            # mov ecx, 2
    lea_rdx(ansibuf_fo)
    mov_r8d_rip(editlen_fo)           # mov r8d, [editlen]
    call_iat("WriteFile")
    emit(0xB9, 2, 0, 0, 0)
    call_iat("CloseHandle")
    jmp32("loop_top")

    # ---- done / fail ----
    label("done")
    emit(0x31, 0xC9)
    call_iat("ExitProcess")
    label("fail")
    emit(0xB9, 7, 0, 0, 0)
    call_iat("ExitProcess")

    assert len(code) <= 0x400, f"代码 {len(code):#x} 超出 0x400"

    # ---- 回填 ----
    for at, size, target, kind in fixups:
        rel = labels[target] - (at + size)
        code[at:at + size] = struct.pack("<i", rel)
    # 先落 code 再回填：relocs 写 img，顺序颠倒会被本拷贝整体冲掉
    # （实机 #GP 取证：rel32 全 0 → call *(%rip) 读到 .text 字节非 canonical）。
    img[TEXT_RAW:TEXT_RAW + len(code)] = code
    for end_off, target_off in relocs:
        disp = va(target_off) - va(end_off)
        struct.pack_into("<i", img, end_off - 4, disp)

    # ---- 数据落位 ----
    img[sz_class_fo:sz_class_fo + len(sz_class)] = sz_class
    img[sz_title_fo:sz_title_fo + len(sz_title)] = sz_title
    struct.pack_into("<Q", img, wcex_fo + 0, 72)
    struct.pack_into("<Q", img, wcex_fo + 8, BASE + TEXT_RVA + 0x100)
    struct.pack_into("<Q", img, wcex_fo + 64, sz_class_va)
    struct.pack_into("<Q", img, cw_fo + 0, sz_class_va)
    struct.pack_into("<Q", img, cw_fo + 8, sz_title_va)
    struct.pack_into("<Q", img, cw_fo + 16, 320)
    struct.pack_into("<Q", img, cw_fo + 24, 200)
    struct.pack_into("<Q", img, to_fo + 0, 8)
    struct.pack_into("<Q", img, to_fo + 8, 8)
    struct.pack_into("<Q", img, to_fo + 16, editbuf_va)
    struct.pack_into("<Q", img, to_fo + 24, 0)
    struct.pack_into("<I", img, ofn_fo + 0, 96)
    struct.pack_into("<Q", img, ofn_fo + 48, fnbuf_va)
    struct.pack_into("<Q", img, ofn_fo + 56, 64)
    # ReadFile 参数块（NT 风格）：handle/buf/len/got（见 open 段注）。
    struct.pack_into("<Q", img, rfparam_fo + 0, 3)
    struct.pack_into("<Q", img, rfparam_fo + 8, ansibuf_va)
    struct.pack_into("<Q", img, rfparam_fo + 16, EDIT_UNITS)
    struct.pack_into("<Q", img, rfparam_fo + 24, got_va)
    return bytes(img)


if __name__ == "__main__":
    import pathlib
    out = pathlib.Path(__file__).resolve().parents[1] / "kernel/varix/src/proc/notepad.pe"
    blob = build()
    out.write_bytes(blob)
    print(f"wrote {out} ({len(blob)} bytes)")
