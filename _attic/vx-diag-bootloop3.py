# -*- coding: utf-8 -*-
"""循环第三轮只读诊断：时间戳证据 + NVRAM 现场复查。全只读。"""
import ctypes, ctypes.wintypes as wt, os, struct, sys, datetime

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-diag-bootloop3.rpt"
out = []

def log(s):
    out.append(str(s))

# ---------- 1. 时间戳证据（内核运行痕迹） ----------
log("=== 1. 内核运行痕迹（文件 mtime） ===")
targets = [
    (r"S:\boot-select.json", "SHARED 真相源"),
    (r"V:\boot-select.json", "ESP 副本"),
    (r"Y:\boot-select.json", "Y盘副本(若挂)"),
]
now = datetime.datetime.now()
for path, label in targets:
    try:
        st = os.stat(path)
        mt = datetime.datetime.fromtimestamp(st.st_mtime)
        age = (now - mt).total_seconds() / 60
        log(f"  {label}: {path} mtime={mt} ({age:.1f} 分钟前)")
    except Exception as ex:
        log(f"  {label}: {path} 不可读 ({ex!r})")

# ---------- 2. NVRAM 复查 ----------
log("")
log("=== 2. NVRAM 引导变量现状 ===")
advapi = ctypes.WinDLL("advapi32", use_last_error=True)
advapi.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
advapi.OpenProcessToken.restype = wt.BOOL
advapi.LookupPrivilegeValueW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p]
advapi.LookupPrivilegeValueW.restype = wt.BOOL
advapi.AdjustTokenPrivileges.argtypes = [wt.HANDLE, wt.BOOL, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, ctypes.POINTER(wt.DWORD)]
advapi.AdjustTokenPrivileges.restype = wt.BOOL

class LUID(ctypes.Structure):
    _fields_ = [("LowPart", wt.DWORD), ("HighPart", wt.LONG)]

class TOKEN_PRIVS(ctypes.Structure):
    _fields_ = [("PrivilegeCount", wt.DWORD), ("Luid", LUID), ("Attributes", wt.DWORD)]

TP = SE_PRIVILEGE_ENABLED = 2
tok = wt.HANDLE()
advapi.OpenProcessToken(ctypes.windll.kernel32.GetCurrentProcess(), 0x28, ctypes.byref(tok))
tp = TOKEN_PRIVS()
tp.PrivilegeCount = 1
name = ctypes.create_unicode_buffer("SeSystemEnvironmentPrivilege")
luid = LUID()
advapi.LookupPrivilegeValueW(None, name, ctypes.byref(luid))
tp.Luid = luid
tp.Attributes = TP
rc_adj = advapi.AdjustTokenPrivileges(tok, 0, ctypes.byref(tp), 0, None, None)
log(f"  [priv] AdjustTokenPrivileges rc={rc_adj} gle={ctypes.get_last_error()}")
ctypes.windll.kernel32.CloseHandle(tok)

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
GetVar = k32.GetFirmwareEnvironmentVariableExW
GetVar.restype = wt.DWORD
GetVar.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p, wt.DWORD, ctypes.POINTER(wt.DWORD)]

def get_var(name, guid="{8be4df61-93ca-11d2-aa0d-00e098032b8c}"):
    buf = ctypes.create_string_buffer(4096)
    sz = wt.DWORD(0)
    n = GetVar(name, guid, buf, 4096, ctypes.byref(sz))
    return bytes(buf.raw[:sz.value]) if n else None

for vn in ("BootNext", "BootOrder", "Boot2001", "Boot0002", "Boot0003"):
    data = get_var(vn)
    if data is None:
        log(f"  {vn}: ABSENT (err={ctypes.get_last_error()})")
    elif vn == "BootOrder":
        order = [struct.unpack_from("<H", data, i)[0] for i in range(0, len(data), 2)]
        log(f"  BootOrder: {[hex(x) for x in order]}")
    elif vn == "BootNext":
        log(f"  BootNext: {hex(struct.unpack_from('<H', data, 0)[0])}")
    else:
        desc_len = 0
        try:
            fplen = struct.unpack_from("<H", data, 4)[0]
            # 描述 = UTF-16
            desc_end = 6
            while desc_end + 1 < len(data) and data[desc_end:desc_end+2] != b"\x00\x00":
                desc_end += 2
            desc = data[6:desc_end].decode("utf-16-le", "replace")
            path = data[len(data)-fplen:] if fplen <= len(data) else b""
            node_kinds = []
            i = 0
            while i + 4 <= len(path):
                t, st_, l = struct.unpack_from("<BBH", path, i)
                node_kinds.append(f"type{t:02x}/sub{st_:02x}/len{l}")
                if l < 4: break
                i += l
            log(f"  {vn}: total={len(data)} fplen={fplen} desc='{desc}' nodes={node_kinds}")
        except Exception as ex:
            log(f"  {vn}: total={len(data)} 解析失败 {ex!r}")
            log(f"    hex={data[:80].hex()}")

with open(OUT, "w", encoding="utf-8") as f:
    f.write("\n".join(out) + "\n")
print("\n".join(out))
