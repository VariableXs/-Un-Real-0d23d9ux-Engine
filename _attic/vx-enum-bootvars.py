# -*- coding: utf-8 -*-
"""只读枚举 UEFI 固件引导变量：BootOrder + Boot####。
标注：U 盘 ESP GUID 两种端序是否在内容里、内核 looks_like_windows 是否会认。
绝不写入。"""
import os
import sys

EFI_GLOBAL = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}"
EFI_ORDER = bytes.fromhex("cb86786367e9f649b0df7608909d1f11")   # EFI 字节序
RAW_ORDER = bytes.fromhex("636786cbe96749f6b0df7608909d1f11")   # 文本端序
HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.join(HERE, "vx-enum-bootvars.rpt")
out = []


def utf16_ascii(b):
    s = b.decode("utf-16-le", "replace")
    return "".join(c for c in s if 32 <= ord(c) < 127)


def main():
    import ctypes
    from ctypes import wintypes as wt
    k32 = ctypes.windll.kernel32
    adv = ctypes.windll.advapi32
    adv.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
    adv.OpenProcessToken.restype = wt.BOOL
    adv.LookupPrivilegeValueW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p]
    adv.LookupPrivilegeValueW.restype = wt.BOOL
    adv.AdjustTokenPrivileges.argtypes = [wt.HANDLE, wt.BOOL, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, ctypes.POINTER(wt.DWORD)]
    adv.AdjustTokenPrivileges.restype = wt.BOOL
    TOKEN_ADJUST_PRIVILEGES = 0x20
    TOKEN_QUERY = 0x8
    tok = wt.HANDLE()
    ok0 = adv.OpenProcessToken(k32.GetCurrentProcess(),
                         TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, ctypes.byref(tok))
    out.append(f"OpenProcessToken rc={ok0} tok={tok.value:#x} gle={k32.GetLastError()}")
    class LUID(ctypes.Structure):
        _fields_ = [("LowPart", ctypes.c_ulong), ("HighPart", ctypes.c_long)]

    luid = LUID()
    ok2 = adv.LookupPrivilegeValueW(None, "SeSystemEnvironmentPrivilege", ctypes.byref(luid))

    class TP(ctypes.Structure):
        _fields_ = [("Luid", LUID), ("Attr", wt.DWORD)]

    class TPP(ctypes.Structure):
        _fields_ = [("Count", wt.DWORD), ("Priv", TP * 1)]

    tp = TPP()
    tp.Count = 1
    tp.Priv[0].Luid = luid
    tp.Priv[0].Attr = 2
    prev = TPP()
    rlen = wt.DWORD()
    ok1 = adv.AdjustTokenPrivileges(tok, False, ctypes.byref(tp),
                                    ctypes.sizeof(prev), ctypes.byref(prev), ctypes.byref(rlen))
    out.append(f"AdjustTokenPrivileges rc={ok1} gle={k32.GetLastError()} lookup_rc={ok2}")

    buf = ctypes.create_string_buffer(4096)
    attr_out = wt.DWORD(0)

    def get_var(name, guid_str):
        n = k32.GetFirmwareEnvironmentVariableExW(
            name, guid_str, buf, 4096, ctypes.byref(attr_out))
        if n == 0:
            e1 = k32.GetLastError()
            n2 = k32.GetFirmwareEnvironmentVariableW(name, guid_str, buf, 4096)
            if n2 == 0:
                return None, e1
            return buf.raw[:n2], 0
        return buf.raw[:n], 0

    order_raw, e = get_var("BootOrder", EFI_GLOBAL)
    out.append(f"BootOrder bytes={order_raw and len(order_raw)} err={e}")
    targets = []
    if order_raw:
        out.append(f"BootOrder hex={order_raw.hex()}")
        targets = [int.from_bytes(order_raw[i:i + 2], "little")
                   for i in range(0, len(order_raw), 2)]
    extra = [i for i in range(0x20) if i not in targets]
    for num in targets + extra:
        name = "Boot%04x" % num
        raw, e = get_var(name, EFI_GLOBAL)
        if raw is None:
            continue
        body = raw[4:]
        desc = utf16_ascii(body.split(b"\x00\x00", 1)[0]) if body else ""
        ascii_txt = utf16_ascii(body)
        looks_win = ("windows" in ascii_txt.lower()) or (
            "microsoft" in ascii_txt.lower() and "bootmgfw" in ascii_txt.lower())
        out.append(
            f"{name}: attr={int.from_bytes(raw[:4], 'little'):#x} desc='{desc}' "
            f"guid_efi={EFI_ORDER in body} guid_raw={RAW_ORDER in body} "
            f"looks_win={looks_win} len={len(raw)}"
        )
    out.append("ENUM-BOOTVARS-DONE")


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
                print(open(LOG, encoding="utf-8").read()[-4500:])
                break
        else:
            print("timeout waiting report")
