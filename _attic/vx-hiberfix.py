import os, sys, datetime, traceback, winreg, ctypes, ctypes.wintypes as wt

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-hiberfix.rpt"
def log(msg=""):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(str(msg) + "\n")

try:
    log("== hiberfast fix " + datetime.datetime.now().isoformat())

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
    for name in ("SeBackupPrivilege", "SeRestorePrivilege"):
        luid = LUID()
        if advapi.LookupPrivilegeValueW(None, name, ctypes.byref(luid)):
            advapi.AdjustTokenPrivileges(tok, 0, ctypes.byref(TP1(1, luid, 2)), 0, None, None)

    def unload(keyname):
        rc = advapi.RegUnLoadKeyW(0x80000002, keyname)
        log("unload " + keyname + " rc=" + str(rc))

    # 0. find X:
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.GetVolumeInformationW.restype = wt.BOOL
    X = None
    for L in "CDEFGHIJKLMNOPQRSTUVWXYZ":
        p = L + ":\\"
        name = ctypes.create_unicode_buffer(300)
        if k32.GetVolumeInformationW(p, name, 300, None, None, None, None, 0) and name.value == "WIN_ENGINE":
            X = L + ":"
            break
    log("X=" + str(X))
    if not X:
        log("FATAL no WIN_ENGINE")
        log("done")
        sys.exit(1)

    # 1. hiberfil.sys present?
    hf = os.path.join(X, "hiberfil.sys")
    try:
        st = os.stat(hf)
        log("hiberfil.sys size=" + str(st.st_size) + " mtime=" + datetime.datetime.fromtimestamp(st.st_mtime).isoformat())
        os.remove(hf)
        log("hiberfil.sys REMOVED: " + str(not os.path.exists(hf)))
    except FileNotFoundError:
        log("hiberfil.sys not present")
    except Exception as ex:
        log("hiberfil ERR " + repr(ex))

    # 2. disable hibernation + fast startup offline
    winreg.LoadKey(winreg.HKEY_LOCAL_MACHINE, "VXHB", os.path.join(X, "Windows", "System32", "config", "SYSTEM"))
    cs = "VXHB\\ControlSet001"
    try:
        k = winreg.CreateKey(winreg.HKEY_LOCAL_MACHINE, cs + r"\Control\Power")
        try:
            old = winreg.QueryValueEx(k, "HibernateEnabled")[0]
        except OSError:
            old = "(absent)"
        winreg.SetValueEx(k, "HibernateEnabled", 0, winreg.REG_DWORD, 0)
        winreg.SetValueEx(k, "HibernateEnabledDefault", 0, winreg.REG_DWORD, 0)
        winreg.SetValueEx(k, "HiberbootEnabled", 0, winreg.REG_DWORD, 0)
        log("Power: old HibernateEnabled=" + str(old) + " -> HibernateEnabled=0, HiberbootEnabled=0")
        # verify
        k2 = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, cs + r"\Control\Power")
        for v in ("HibernateEnabled", "HiberbootEnabled"):
            log("  verify " + v + "=" + str(winreg.QueryValueEx(k2, v)[0]))
    except Exception as ex:
        log("Power ERR " + repr(ex))
    unload("VXHB")

    # 3. dirty state check (read-only chkdsk)
    import subprocess
    r = subprocess.run(["chkdsk", X], capture_output=True, text=True, errors="replace")
    tail = [l for l in r.stdout.splitlines() if l.strip()][-8:]
    for l in tail:
        log("chkdsk: " + l.strip())

    # 4. crash artifacts (dump enabled earlier?)
    md = os.path.join(X, "Windows", "Minidump")
    if os.path.isdir(md):
        for f in sorted(os.listdir(md), key=lambda f: os.path.getmtime(os.path.join(md, f)), reverse=True)[:5]:
            p = os.path.join(md, f)
            log("MINIDUMP: " + f + " " + str(os.path.getsize(p)) + " " + datetime.datetime.fromtimestamp(os.path.getmtime(p)).isoformat())
    else:
        log("no Minidump dir")
    md = os.path.join(X, "Windows", "MEMORY.DMP")
    if os.path.exists(md):
        log("MEMORY.DMP " + str(os.path.getsize(md)) + " " + datetime.datetime.fromtimestamp(os.path.getmtime(md)).isoformat())

    log("fix done")
except Exception:
    log("EXC " + traceback.format_exc()[:1200])
    log("done")
