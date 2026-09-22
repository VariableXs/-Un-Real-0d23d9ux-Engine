# -*- coding: utf-8 -*-
"""修复 Boot2001（VARIX Windows (USB)）的空设备路径（2026-09-23 引导循环根因）。

实锤：Boot2001 FilePathListLength=4（纯 END 节点 7fff0400）——固件兑现失败
回落本次引导设备（U 盘）→ Limine→varix→bootselect→交接→BootNext=Boot2001
→ 无限复位循环。元凶：v2 的 `bcdedit /set device partition=Y:` 把 /copy
继承的 96 字节 USB 设备路径清成了空。

修复策略（双层，以回读实证为准，绝不假装成功）：
  attempt1  bcdedit /set {1300a859} path \\EFI\\Microsoft\\Boot\\bootmgfw.efi
            （BCD store 层修正；bcdedit 可能自行同步 NVRAM）
  attempt2  手写 NVRAM Load Option（SetFirmwareEnvironmentVariableExW）：
            USB+HD 节点取自 Boot0002（EFI USB Device），File 节点取自
            Boot0003（Windows Boot Manager 的 bootmgfw.efi），END 收尾。
            只写 Boot2001 这一个我们自己造的 U 盘侧项——内置项零触碰。
验证闸门：回读 total/fplen/desc/含 bootmgfw UTF-16/END 收尾，全过才算
  PASS；任一不过 → 如实 FAIL。附 BootOrder 自检（内置 Windows 必须永居首项）。
"""
import os
import sys
import subprocess

EFI_GLOBAL = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}"
TARGET_NAME = "Boot2001"
TARGET_BCD_ID = "{1300a859-b6a1-11f1-ab93-c4c6e62c8a4c}"
BOOTMGFW_UTF16 = r"\EFI\Microsoft\Boot\bootmgfw.efi".encode("utf-16-le")
DESC_TEXT = "VARIX Windows (USB)"
HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.join(HERE, "vx-fix-boot2001.rpt")
out = []


def utf16_ascii(b):
    s = b.decode("utf-16-le", "replace")
    return "".join(c for c in s if 32 <= ord(c) < 127)


def parse_desc(raw):
    """返回 (desc_text, path_offset)。"""
    end = raw.find(b"\x00\x00", 6)
    if end < 0:
        return "", 6
    end += end % 2  # UTF-16 对齐
    return utf16_ascii(raw[6:end]), end + 2


def parse_nodes(path_bytes):
    """解析设备路径节点 [(type, subtype, node_bytes)]，容错截断。"""
    nodes = []
    i = 0
    while i + 4 <= len(path_bytes):
        t, st = path_bytes[i], path_bytes[i + 1]
        ln = int.from_bytes(path_bytes[i + 2:i + 4], "little")
        if ln < 4 or i + ln > len(path_bytes):
            break
        nodes.append((t, st, path_bytes[i:i + ln]))
        i += ln
        if t == 0x7F:  # END
            break
    return nodes


def grab(raw, want_types, drop_end=True):
    """从 Load Option 原始字节提取设备路径，拼接指定类型节点（丢 END）。"""
    _, off = parse_desc(raw)
    fplen = int.from_bytes(raw[4:6], "little")
    nodes = parse_nodes(raw[off:off + fplen])
    picked = b"".join(n for t, _, n in nodes if t in want_types and t != 0x7F)
    return picked, nodes


def setup_privilege():
    import ctypes
    from ctypes import wintypes as wt
    k32 = ctypes.windll.kernel32
    adv = ctypes.windll.advapi32
    adv.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
    adv.OpenProcessToken.restype = wt.BOOL
    adv.LookupPrivilegeValueW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p]
    adv.LookupPrivilegeValueW.restype = wt.BOOL
    adv.AdjustTokenPrivileges.argtypes = [wt.HANDLE, wt.BOOL, ctypes.c_void_p, wt.DWORD,
                                          ctypes.c_void_p, ctypes.POINTER(wt.DWORD)]
    adv.AdjustTokenPrivileges.restype = wt.BOOL
    tok = wt.HANDLE()
    adv.OpenProcessToken(k32.GetCurrentProcess(), 0x20 | 0x8, ctypes.byref(tok))

    class LUID(ctypes.Structure):
        _fields_ = [("LowPart", ctypes.c_ulong), ("HighPart", ctypes.c_long)]

    class TP(ctypes.Structure):
        _fields_ = [("Luid", LUID), ("Attr", wt.DWORD)]

    class TPP(ctypes.Structure):
        _fields_ = [("Count", wt.DWORD), ("Priv", TP * 1)]

    luid = LUID()
    ok2 = adv.LookupPrivilegeValueW(None, "SeSystemEnvironmentPrivilege", ctypes.byref(luid))
    tp = TPP()
    tp.Count = 1
    tp.Priv[0].Luid = luid
    tp.Priv[0].Attr = 2
    adv.AdjustTokenPrivileges(tok, False, ctypes.byref(tp), 0, None, None)
    out.append(f"privilege: tok={tok.value:#x} lookup={ok2}")
    return k32


def make_api(k32):
    buf = ctypes_create_buffer()
    attr_out = ctypes_attr_out()

    import ctypes
    k32.SetFirmwareEnvironmentVariableExW.argtypes = [
        ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_ulong,
        ctypes.c_ulong]
    k32.SetFirmwareEnvironmentVariableExW.restype = ctypes.c_long  # BOOL

    def get_var(name):
        n = k32.GetFirmwareEnvironmentVariableExW(
            name, EFI_GLOBAL, buf, 4096, ctypes.byref(attr_out))
        if n == 0:
            n = k32.GetFirmwareEnvironmentVariableW(name, EFI_GLOBAL, buf, 4096)
        return buf.raw[:n] if n else None

    def set_var(name, data, attributes=1):
        ok = k32.SetFirmwareEnvironmentVariableExW(
            name, EFI_GLOBAL, data, len(data), attributes)
        return bool(ok), k32.GetLastError()

    return get_var, set_var


def ctypes_create_buffer():
    import ctypes
    return ctypes.create_string_buffer(4096)


def ctypes_attr_out():
    import ctypes
    from ctypes import wintypes as wt
    return wt.DWORD(0)


def bcd_set_path():
    p = subprocess.run(["bcdedit", "/set", TARGET_BCD_ID, "path", r"\EFI\Microsoft\Boot\bootmgfw.efi"],
                       capture_output=True)
    txt = (p.stdout + p.stderr).decode("gbk", "replace").strip()
    out.append(f"=== [bcdedit set path] rc={p.returncode}\n{txt}")


def verify(get_var, tag):
    raw = get_var(TARGET_NAME)
    if raw is None:
        out.append(f"[{tag}] 回读失败：{TARGET_NAME} 不存在")
        return False
    total = len(raw)
    fplen = int.from_bytes(raw[4:6], "little")
    desc, off = parse_desc(raw)
    path_raw = raw[off:off + fplen]
    nodes = parse_nodes(path_raw)
    has_file = any(t == 0x04 and s == 0x04 for t, s, _ in nodes)
    has_usb = any(t == 0x03 for t, _, _ in nodes)
    has_hd = any(t == 0x04 and s == 0x01 for t, s, _ in nodes)
    ends_ok = bool(nodes) and nodes[-1][0] == 0x7F and nodes[-1][1] == 0xFF
    bootmgfw = BOOTMGFW_UTF16 in path_raw
    out.append(f"[{tag}] total={total} fplen={fplen} desc='{desc}' nodes="
               f"{[(hex(t), hex(s)) for t, s, _ in nodes]}")
    ok = (fplen > 100 and has_file and has_usb and has_hd and ends_ok
          and bootmgfw and desc == DESC_TEXT)
    out.append(f"[{tag}] fplen>100={fplen > 100} usb={has_usb} hd={has_hd} "
               f"file={has_file} end_ok={ends_ok} bootmgfw_in_path={bootmgfw} "
               f"desc_ok={desc == DESC_TEXT} -> {'PASS' if ok else 'NOT-YET'}")
    return ok


def bootorder_sanity(get_var):
    raw = get_var("BootOrder")
    if not raw:
        out.append("BootOrder 不可读（自检跳过）")
        return
    order = [int.from_bytes(raw[i:i + 2], "little") for i in range(0, len(raw), 2)]
    out.append(f"BootOrder={[hex(n) for n in order]}")
    if order and order[0] == 0x0003:
        out.append("SANITY-OK: 内置 Windows (Boot0003) 仍居首项")
    else:
        out.append("SANITY-WARN: 首项不是 Boot0003 —— 需人工核查（本次修复未改动 BootOrder）")


def main():
    k32 = setup_privilege()
    get_var, set_var = make_api(k32)

    # 素材提取（只读）
    raw_usb = get_var("Boot0002")
    raw_win = get_var("Boot0003")
    if not raw_usb or not raw_win:
        out.append("MISSING-SOURCE: Boot0002/Boot0003 不可读，无法提取设备路径骨架")
        return
    usb_hd, nodes_usb = grab(raw_usb, {0x03, 0x04})
    out.append(f"Boot0002 USB+HD 节点共 {len(usb_hd)} 字节（{len(nodes_usb)} 节点）")
    _, off_win = parse_desc(raw_win)
    fplen_win = int.from_bytes(raw_win[4:6], "little")
    file_node = None
    for t, st, n in parse_nodes(raw_win[off_win:off_win + fplen_win]):
        if t == 0x04 and st == 0x04:
            file_node = n
            break
    if not usb_hd or file_node is None:
        out.append("MISSING-MATERIAL: USB+HD 或 File 节点提取失败")
        return
    out.append(f"Boot0003 File(bootmgfw) 节点 {len(file_node)} 字节")

    # attempt1：bcdedit 标准通道
    bcd_set_path()
    if verify(get_var, "attempt1-bcdedit"):
        out.append("BOOT2001-REPAIR-DONE (bcdedit)")
        bootorder_sanity(get_var)
        return

    # attempt2：手写 NVRAM Load Option（USB+HD + File + END）
    desc_bytes = DESC_TEXT.encode("utf-16-le") + b"\x00\x00"
    path_new = usb_hd + file_node + b"\x7f\xff\x04\x00"
    data = (1).to_bytes(4, "little") + len(path_new).to_bytes(2, "little") + desc_bytes + path_new
    out.append(f"attempt2: 手写 Load Option 共 {len(data)} 字节（fplen={len(path_new)}）")
    ok, gle = set_var(TARGET_NAME, data)
    out.append(f"SetFirmwareEnvironmentVariableExW rc={ok} gle={gle}")
    if not ok:
        out.append("BOOT2001-REPAIR-FAIL: 固件拒绝写入（Lenovo 可能锁定该变量）")
        bootorder_sanity(get_var)
        return
    if verify(get_var, "attempt2-nvram-write"):
        out.append("BOOT2001-REPAIR-DONE (nvram-write)")
    else:
        out.append("BOOT2001-REPAIR-FAIL: 写入后回读不合格（如实报告，未做其他改动）")
    bootorder_sanity(get_var)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--elevated":
        try:
            main()
        except Exception:
            import traceback
            out.append("EXCEPTION:\n" + traceback.format_exc())
        with open(LOG, "w", encoding="utf-8") as f:
            f.write("\n".join(out) + "\n")
    else:
        import ctypes
        import time
        me = os.path.abspath(__file__)
        rc = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", sys.executable, f'"{me}" --elevated', None, 0)
        print("ShellExecute rc=", rc)
        for _ in range(90):
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8").read()[-4000:])
                break
        else:
            print("timeout waiting report")
