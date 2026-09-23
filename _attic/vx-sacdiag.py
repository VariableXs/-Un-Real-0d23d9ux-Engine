import os, sys, datetime, traceback, winreg

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-sacdiag.rpt"
def log(msg=""):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(str(msg) + "\n")

try:
    log("== SAC/SmartScreen diag " + datetime.datetime.now().isoformat())

    # enable load-key privileges
    import ctypes, ctypes.wintypes as wt
    advapi = ctypes.WinDLL("advapi32", use_last_error=True)
    advapi.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
    advapi.LookupPrivilegeValueW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p]
    advapi.AdjustTokenPrivileges.argtypes = [wt.HANDLE, wt.BOOL, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, ctypes.POINTER(wt.DWORD)]
    class LUID(ctypes.Structure):
        _fields_ = [("LowPart", wt.DWORD), ("HighPart", wt.LONG)]
    class TP1(ctypes.Structure):
        _fields_ = [("Count", wt.DWORD), ("Luid", LUID), ("Attr", wt.DWORD)]
    tok = wt.HANDLE()
    advapi.OpenProcessToken(ctypes.WinDLL("kernel32").GetCurrentProcess(), 0x0028, ctypes.byref(tok))
    for name in ("SeBackupPrivilege", "SeRestorePrivilege"):
        luid = LUID()
        if advapi.LookupPrivilegeValueW(None, name, ctypes.byref(luid)):
            tp = TP1(1, luid, 2)
            advapi.AdjustTokenPrivileges(tok, 0, ctypes.byref(tp), 0, None, None)

    SYSHIVE = r"X:\Windows\System32\config\SYSTEM"
    SOFHIVE = r"X:\Windows\System32\config\SOFTWARE"
    advapi.RegUnLoadKeyW.argtypes = [wt.HKEY, wt.LPCWSTR]
    advapi.RegUnLoadKeyW.restype = wt.LONG
    def unload(keyname):
        rc = advapi.RegUnLoadKeyW(0x80000002, keyname)
        if rc != 0:
            log("unload " + keyname + " rc=" + str(rc))

    winreg.LoadKey(winreg.HKEY_LOCAL_MACHINE, "VXSAC_S", SYSHIVE)
    # CCSet is a symlink of ControlSet001 offline; enumerate to find real set
    cs_base = r"VXSAC_S\Select"
    try:
        k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, cs_base)
        cur = winreg.QueryValueEx(k, "Current")[0]
        log("Current set: " + str(cur))
    except Exception as ex:
        cur = 1
        log("select ERR " + repr(ex))
    cs = r"VXSAC_S\ControlSet%03d" % cur

    # SAC service
    try:
        k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, cs + r"\Services\SACSvc")
        start = winreg.QueryValueEx(k, "Start")[0]
        log("SACSvc Start = " + str(start) + "  (2=auto/ON, 3=manual, 4=disabled)")
    except OSError as ex:
        log("SACSvc ERR " + repr(ex))

    # AppControl policy (WDAC / Smart App Control state via CodeIntegrity)
    try:
        k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, cs + r"\Services\OneSyncSvc")  # probe only
    except OSError:
        pass
    for sub, valname in ((r"\Services\SecurityHealthService", "Start"),
                         (r"\Services\wscsvc", "Start"),
                         (r"\Services\AppIDSvc", "Start"),
                         (r"\Services\Appinfo", "Start")):
        try:
            k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, cs + sub)
            log(sub.strip("\\").split("\\")[-1] + " Start = " + str(winreg.QueryValueEx(k, valname)[0]))
        except OSError as ex:
            log(sub + " ERR " + repr(ex))
    unload("VXSAC_S")

    winreg.LoadKey(winreg.HKEY_LOCAL_MACHINE, "VXSAC_F", SOFHIVE)
    for path, val in ((r"VXSAC_F\Microsoft\Windows\CurrentVersion\Explorer", "SmartScreenEnabled"),
                      (r"VXSAC_F\Policies\Microsoft\Windows\System", "EnableSmartScreen"),
                      (r"VXSAC_F\Microsoft\Windows\Safer\CodeIdentifiers", "TransparentEnabled")):
        try:
            k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, path)
            v = winreg.QueryValueEx(k, val)[0]
            log(path + " \ " + val + " = " + repr(v))
        except OSError as ex:
            log(path + " \ " + val + " N/A")
    # SAC state under Software\Microsoft\Windows\CurrentVersion\SIPolicy
    try:
        k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXSAC_F\Microsoft\Windows\CurrentVersion\SIPolicy")
        i = 0
        while True:
            try:
                v, data, _ = winreg.EnumValue(k, i); i += 1
                log("SIPolicy: " + v + " = " + repr(data))
            except OSError:
                break
    except OSError:
        log("SIPolicy key N/A")
    unload("VXSAC_F")
    log("done")
except Exception:
    log("EXC " + traceback.format_exc()[:1200])
    log("done")
