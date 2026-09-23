
import os, sys, ctypes, winreg, ctypes.wintypes as wt
LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-mountcheck.rpt"
def log(m=""):
    with open(LOG, "a", encoding="utf-8") as f: f.write(str(m) + "\n")
try:
    advapi = ctypes.WinDLL("advapi32", use_last_error=True)
    advapi.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
    advapi.LookupPrivilegeValueW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p]
    advapi.AdjustTokenPrivileges.argtypes = [wt.HANDLE, wt.BOOL, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, ctypes.POINTER(wt.DWORD)]
    advapi.RegUnLoadKeyW.argtypes = [wt.HKEY, wt.LPCWSTR]
    advapi.RegUnLoadKeyW.restype = wt.LONG
    class LUID(ctypes.Structure):
        _fields_ = [("LowPart", wt.DWORD), ("HighPart", wt.LONG)]
    class TP1(ctypes.Structure):
        _fields_ = [("Count", wt.DWORD), ("Luid", LUID), ("Attr", wt.DWORD)]
    tok = wt.HANDLE()
    advapi.OpenProcessToken(ctypes.WinDLL("kernel32").GetCurrentProcess(), 0x0028, ctypes.byref(tok))
    for name in ("SeBackupPrivilege", "SeRestorePrivilege", "SeTakeOwnershipPrivilege"):
        luid = LUID()
        if advapi.LookupPrivilegeValueW(None, name, ctypes.byref(luid)):
            advapi.AdjustTokenPrivileges(tok, 0, ctypes.byref(TP1(1, luid, 2)), 0, None, None)
    # clean stale keys
    for key in ("VXHB", "VXAS", "VXMC", "VXPOST", "VXSAC_S", "VXSAC_F", "VXSOFT", "VXU2", "VXU_Default", "VXU_defaultu", "VXU_Varia"):
        rc = advapi.RegUnLoadKeyW(0x80000002, key)
        log("clean " + key + " rc=" + str(rc) + ("" if rc == 0 else " (not loaded?)"))
    log("-- now mount SYSTEM --")
    k32 = ctypes.WinDLL("kernel32")
    X = None
    for L in "CDEFGHIJKLMNOPQRSTUVWXYZ":
        p = L + ":\\"
        nm = ctypes.create_unicode_buffer(300)
        if k32.GetVolumeInformationW(p, nm, 300, None, None, None, None, 0) and nm.value == "WIN_ENGINE":
            X = L + ":"
    log("X=" + str(X))
    BS = chr(92)
    hive = X + BS + "Windows" + BS + "System32" + BS + "config" + BS + "SYSTEM"
    winreg.LoadKey(winreg.HKEY_LOCAL_MACHINE, "VXMC", hive)
    k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXMC\MountedDevices")
    i = 0
    while True:
        try:
            v, data, t = winreg.EnumValue(k, i); i += 1
            hexs = data.hex() if isinstance(data, bytes) else str(data)
            log(f"[{i}] {v}  len={len(data)}  {hexs[:130]}")
        except OSError:
            break
    try:
        k2 = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXMC\ControlSet001\Control\CrashControl")
        for v in ("AutoReboot", "CrashDumpEnabled"):
            try: log("CrashControl " + v + "=" + str(winreg.QueryValueEx(k2, v)[0]))
            except OSError: log("CrashControl " + v + " N/A")
    except OSError as ex:
        log("CrashControl ERR " + repr(ex))
    rc = advapi.RegUnLoadKeyW(0x80000002, "VXMC"); log("unload VXMC rc=" + str(rc))
    log("done")
except Exception:
    import traceback
    log("EXC " + traceback.format_exc()[:1200])
    log("done")
