# -*- coding: utf-8 -*-
"""决定性实验（全可控、可回滚）：
1. 只读 dump Boot2001 当前形态（验证 02:20 手写形态是否仍在）
2. 用 Windows 标准通道设置 BootNext=0x2001 并回读
之后由用户正常重启验证固件能否兑现 Boot2001。
安全：BootNext 仅生效一次；失败回落 BootOrder 首项（内置 Windows）。
"""
import ctypes, ctypes.wintypes as wt, struct, os, sys

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-exp-boot2001.rpt"
out = []
EFI_GLOBAL = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}"

def log(s):
    out.append(str(s))

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
adv = ctypes.WinDLL("advapi32", use_last_error=True)
adv.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
adv.OpenProcessToken.restype = wt.BOOL
adv.LookupPrivilegeValueW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p]
adv.LookupPrivilegeValueW.restype = wt.BOOL
adv.AdjustTokenPrivileges.argtypes = [wt.HANDLE, wt.BOOL, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, ctypes.POINTER(wt.DWORD)]
adv.AdjustTokenPrivileges.restype = wt.BOOL

class LUID(ctypes.Structure):
    _fields_ = [("LowPart", ctypes.c_ulong), ("HighPart", ctypes.c_long)]
class TP(ctypes.Structure):
    _fields_ = [("Luid", LUID), ("Attr", wt.DWORD)]
class TPS(ctypes.Structure):
    _fields_ = [("Count", wt.DWORD), ("Priv", TP * 1)]

tok = wt.HANDLE()
adv.OpenProcessToken(k32.GetCurrentProcess(), 0x28, ctypes.byref(tok))
luid = LUID()
ok2 = adv.LookupPrivilegeValueW(None, "SeSystemEnvironmentPrivilege", ctypes.byref(luid))
tps = TPS()
tps.Count = 1
tps.Priv[0].Luid = luid
tps.Priv[0].Attr = 2
ok1 = adv.AdjustTokenPrivileges(tok, False, ctypes.byref(tps), 0, None, None)
log(f"[priv] adjust rc={ok1} gle={k32.GetLastError()} lookup={ok2}")
k32.CloseHandle(tok)

GetVar = k32.GetFirmwareEnvironmentVariableExW
GetVar.restype = wt.DWORD
GetVar.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p, wt.DWORD, ctypes.POINTER(wt.DWORD)]
SetVar = k32.SetFirmwareEnvironmentVariableExW
SetVar.restype = wt.BOOL
SetVar.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p, wt.DWORD, wt.DWORD]

def get_var(name):
    buf = ctypes.create_string_buffer(4096)
    attr = wt.DWORD(0)
    n = GetVar(name, EFI_GLOBAL, buf, 4096, ctypes.byref(attr))
    if n == 0:
        return None, k32.GetLastError(), 0
    return bytes(buf.raw[:n]), 0, attr.value

# ---- 1. dump Boot2001 ----
raw, err, attr = get_var("Boot2001")
if raw is None:
    log(f"Boot2001 ABSENT err={err}")
else:
    fplen = struct.unpack_from("<H", raw, 4)[0]
    desc_end = 6
    while desc_end + 1 < len(raw) and raw[desc_end:desc_end+2] != b"\x00\x00":
        desc_end += 2
    desc = raw[6:desc_end].decode("utf-16-le", "replace")
    path = raw[len(raw)-fplen:]
    nodes = []
    i = 0
    while i + 4 <= len(path):
        t, st_, l = struct.unpack_from("<BBH", path, i)
        nodes.append(f"t{t:02x}/s{st_:02x}/l{l}")
        if l < 4: break
        i += l
    log(f"Boot2001: total={len(raw)} attr={attr:#x} fplen={fplen} desc='{desc}'")
    log(f"  nodes={nodes}")
    # 分区 GUID（HD 节点内偏移 16）
    i = 0
    while i + 4 <= len(path):
        t, st_, l = struct.unpack_from("<BBH", path, i)
        if t == 0x04 and st_ == 0x01 and l >= 36:
            part = raw if False else path[i+16:i+32]
            log(f"  HD partition_guid_raw={part.hex()}")
        if l < 4: break
        i += l

# ---- 2. dump BootNext（应无或已被消费） ----
raw, err, _ = get_var("BootNext")
log(f"BootNext: {'ABSENT err=' + str(err) if raw is None else hex(struct.unpack_from('<H', raw, 0)[0])}")

# ---- 3. 设置 BootNext=0x2001 ----
data = struct.pack("<H", 0x2001)
ok = SetVar("BootNext", EFI_GLOBAL, data, len(data), 0x7)
log(f"Set BootNext=0x2001 rc={ok} gle={k32.GetLastError()}")
# 回读
raw, err, attr = get_var("BootNext")
if raw is None:
    log(f"  回读 FAIL err={err}")
else:
    log(f"  回读 OK: {hex(struct.unpack_from('<H', raw, 0)[0])} attr={attr:#x}")

with open(OUT, "w", encoding="utf-8") as f:
    f.write("\n".join(out) + "\n")
print("\n".join(out))
