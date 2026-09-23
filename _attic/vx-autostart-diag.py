import os, sys, datetime, traceback

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-autostart-diag.rpt"
out = []
def log(msg=""):
    out.append(str(msg))
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(str(msg) + "\n")

def bail(msg):
    log("FATAL " + msg)
    log("done")
    sys.exit(1)

try:
    log("== autostart diag " + datetime.datetime.now().isoformat())

    # map labels to letters
    import ctypes, ctypes.wintypes as wt
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.GetVolumeInformationW.restype = wt.BOOL
    letter_by_label = {}
    for L in "CDEFGHIJKLMNOPQRSTUVWXYZ":
        p = L + ":\\"
        name = ctypes.create_unicode_buffer(300)
        if k32.GetVolumeInformationW(p, name, 300, None, None, None, None, 0):
            letter_by_label.setdefault(name.value, p)
    log("letters: " + repr(letter_by_label))
    X = letter_by_label.get("WIN_ENGINE")
    if not X:
        bail("WIN_ENGINE not present")
    X = X.rstrip("\\")

    now = datetime.datetime.now()
    # 1. real user dirs under X:\Users
    users_dir = os.path.join(X, "Users")
    log("-- X:\\Users --")
    for u in os.listdir(users_dir):
        p = os.path.join(users_dir, u)
        try:
            mt = datetime.datetime.fromtimestamp(os.path.getmtime(p))
            log(u + "/  mtime=" + mt.isoformat())
        except Exception:
            log(u + "/")

    # enable SeRestore/SeBackup privileges for LoadKey
    import winreg
    advapi = ctypes.WinDLL("advapi32", use_last_error=True)
    advapi.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
    advapi.OpenProcessToken.restype = wt.BOOL
    advapi.LookupPrivilegeValueW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p]
    advapi.LookupPrivilegeValueW.restype = wt.BOOL
    advapi.AdjustTokenPrivileges.argtypes = [wt.HANDLE, wt.BOOL, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, ctypes.POINTER(wt.DWORD)]
    advapi.AdjustTokenPrivileges.restype = wt.BOOL
    class LUID(ctypes.Structure):
        _fields_ = [("LowPart", wt.DWORD), ("HighPart", wt.LONG)]
    class TP1(ctypes.Structure):
        _fields_ = [("Count", wt.DWORD), ("Luid", LUID), ("Attr", wt.DWORD)]
    tok = wt.HANDLE()
    advapi.OpenProcessToken(k32.GetCurrentProcess(), 0x0028, ctypes.byref(tok))
    for priv, name in ((2, "SeBackupPrivilege"), (4, "SeRestorePrivilege")):
        luid = LUID()
        if advapi.LookupPrivilegeValueW(None, name, ctypes.byref(luid)):
            tp = TP1(1, luid, 2)
            rc = advapi.AdjustTokenPrivileges(tok, 0, ctypes.byref(tp), 0, None, None)
            log("priv " + name + " rc=" + str(rc) + " gle=" + str(ctypes.get_last_error()))
    advapi.RegUnLoadKeyW.argtypes = [wt.HKEY, wt.LPCWSTR]
    advapi.RegUnLoadKeyW.restype = wt.LONG
    def unload(keyname):
        rc = advapi.RegUnLoadKeyW(0x80000002, keyname)
        if rc != 0:
            log("unload " + keyname + " rc=" + str(rc))
    # 2. ProfileList (who actually logs in)
    PL = r"X:\Windows\System32\config\SOFTWARE"
    r = winreg.LoadKey(winreg.HKEY_LOCAL_MACHINE, "VXAS", PL)
    log("load SOFTWARE rc=" + str(r))
    try:
        base = r"VXAS\Microsoft\Windows NT\CurrentVersion\ProfileList"
        k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, base)
        i = 0
        while True:
            try:
                sid = winreg.EnumKey(k, i); i += 1
            except OSError:
                break
            try:
                sk = winreg.OpenKey(k, sid)
                path, _ = winreg.QueryValueEx(sk, "ProfileImagePath")
                log("Profile: " + sid + " -> " + path)
            except OSError:
                log("Profile: " + sid + " (no path)")
    except Exception as ex:
        log("ProfileList ERR " + repr(ex))
    unload("VXAS")

    # 3. each user hive Run key
    log("-- user hive Run keys --")
    for u in os.listdir(users_dir):
        hive = os.path.join(users_dir, u, "NTUSER.DAT")
        if not os.path.exists(hive):
            log(u + ": no hive")
            continue
        keyname = "VXU_" + u.replace(" ", "_")[:8]
        try:
            r = winreg.LoadKey(winreg.HKEY_LOCAL_MACHINE, keyname, hive)
            try:
                k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, keyname + r"\Software\Microsoft\Windows\CurrentVersion\Run")
                j = 0
                while True:
                    try:
                        v, data, _ = winreg.EnumValue(k, j); j += 1
                        log(u + " Run: " + v + " = " + str(data))
                    except OSError:
                        break
                if j == 0:
                    log(u + " Run: (empty)")
            except OSError as ex:
                log(u + " Run key open ERR " + repr(ex))
            unload(keyname)
        except OSError as ex:
            log(u + " hive load ERR " + repr(ex))

    # 4. Variable.exe on system volume
    ve = os.path.join(X, "Variable", "Variable.exe")
    if os.path.exists(ve):
        st = os.stat(ve)
        log("Variable.exe OK " + str(st.st_size) + " mtime=" + datetime.datetime.fromtimestamp(st.st_mtime).isoformat())
    else:
        log("Variable.exe MISSING at " + ve)

    # 5. WER after 23:35 today
    log("-- WER (recent) --")
    werroot = os.path.join(X, "ProgramData", "Microsoft", "Windows", "WER")
    for sub in ("ReportArchive", "ReportQueue"):
        d = os.path.join(werroot, sub)
        if not os.path.isdir(d):
            continue
        for e in sorted(os.listdir(d), key=lambda f: os.path.getmtime(os.path.join(d, f)), reverse=True)[:8]:
            p = os.path.join(d, e)
            mt = datetime.datetime.fromtimestamp(os.path.getmtime(p))
            flag = " <<<" if "variable" in e.lower() else ""
            log(sub + ": " + e[:90] + "  " + mt.isoformat() + flag)
            if "variable" in e.lower() and os.path.isdir(p):
                wer = os.path.join(p, "Report.wer")
                if os.path.exists(wer):
                    try:
                        txt = open(wer, encoding="utf-16", errors="replace").read()
                    except Exception:
                        txt = open(wer, encoding="utf-8", errors="replace").read()
                    for line in txt.splitlines():
                        if line.startswith(("Event[0].", "Sig[0.", "Sig[1.", "Sig[2.", "Sig[3.", "Sig[4.", "Sig[5.", "Sig[6.", "Sig[7.", "Sig[8.", "Sig[9.")):
                            log("   " + line[:180])

    log("done")
except Exception:
    log("EXC " + traceback.format_exc()[:1500])
    log("done")
