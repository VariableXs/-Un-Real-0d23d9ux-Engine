import os, sys, datetime, traceback

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-postlogin.rpt"
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
    log("== post-login forensics " + datetime.datetime.now().isoformat())

    # ---- mount volumes by label ----
    import ctypes, ctypes.wintypes as wt
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.FindFirstVolumeW.restype = wt.HANDLE
    k32.FindFirstVolumeW.argtypes = [ctypes.c_wchar_p, wt.DWORD]
    k32.FindNextVolumeW.restype = wt.BOOL
    k32.FindNextVolumeW.argtypes = [wt.HANDLE, ctypes.c_wchar_p, wt.DWORD]
    k32.FindVolumeClose.argtypes = [wt.HANDLE]
    k32.GetVolumeInformationW = ctypes.WinDLL("kernel32").GetVolumeInformationW
    k32.DeleteVolumeMountPointW.argtypes = [wt.LPCWSTR]
    k32.SetVolumeMountPointW.argtypes = [wt.LPCWSTR, wt.LPCWSTR]
    k32.SetVolumeMountPointW.restype = wt.BOOL
    mpr = k32

    vols = []
    buf = ctypes.create_unicode_buffer(300)
    h = k32.FindFirstVolumeW(buf, 300)
    if h == wt.HANDLE(-1).value or h == 0xFFFFFFFFFFFFFFFF:
        bail("FindFirstVolumeW failed")
    while True:
        name, fs = ctypes.create_unicode_buffer(300), ctypes.create_unicode_buffer(100)
        ok = k32.GetVolumeInformationW(buf.value, name, 300, None, None, None, fs, 100)
        if ok:
            vols.append((buf.value, name.value, fs.value))
        if not k32.FindNextVolumeW(h, buf, 300):
            break
    k32.FindVolumeClose(h)
    for g, n, fs in vols:
        log("VOL " + g + " label=" + n + " fs=" + fs)

    letters = {}
    for L in "XYZWVUST":
        letters[L] = None
    # scan all drive letters, map label -> letter
    letter_by_label = {}
    for L in "CDEFGHIJKLMNOPQRSTUVWXYZ":
        p = L + ":\\"
        name = ctypes.create_unicode_buffer(300)
        ok = k32.GetVolumeInformationW(p, name, 300, None, None, None, None, 0)
        if ok:
            letter_by_label.setdefault(name.value, p)
    log("letters: " + repr(letter_by_label))
    X = letter_by_label.get("WIN_ENGINE")
    Y = letter_by_label.get("SHARED")
    if not X:
        # fallback: mount by guid
        for g, n, fs in vols:
            if n == "WIN_ENGINE":
                mp = "X:\\"
                k32.DeleteVolumeMountPointW(mp)
                if k32.SetVolumeMountPointW(mp, g):
                    X = "X:"
                else:
                    log("mount X gle=" + str(ctypes.get_last_error()))
    log("X=" + str(X) + " Y=" + str(Y))
    if not X:
        bail("WIN_ENGINE not mounted")

    # ---- 1. did the OS reboot recently? winevt mtimes ----
    log("-- winevt mtimes --")
    evtdir = os.path.join(X, "Windows", "System32", "winevt", "Logs")
    if os.path.isdir(evtdir):
        now = datetime.datetime.now()
        for f in ("System.evtx", "Application.evtx", "Microsoft-Windows-Kernel-Boot%4Operational.evtx"):
            p = os.path.join(evtdir, f)
            try:
                st = os.stat(p)
                age = (now - datetime.datetime.fromtimestamp(st.st_mtime)).total_seconds() / 60
                log(f + " mtime=" + datetime.datetime.fromtimestamp(st.st_mtime).isoformat() + " (" + str(int(age)) + " min ago)")
            except Exception as ex:
                log(f + " ERR " + repr(ex))
    else:
        log("evtx dir missing")

    # ---- 2. WER reports (app crashes incl. Variable.exe) ----
    log("-- WER reports --")
    for werdir in (os.path.join(X, "ProgramData", "Microsoft", "Windows", "WER"),
                   os.path.join(X, "Windows", "System32", "config", "systemprofile", "AppData", "Local", "Microsoft", "Windows", "WER")):
        if not os.path.isdir(werdir):
            continue
        for sub in ("ReportArchive", "ReportQueue"):
            d = os.path.join(werdir, sub)
            if not os.path.isdir(d):
                continue
            try:
                entries = sorted(os.listdir(d), key=lambda f: os.path.getmtime(os.path.join(d, f)), reverse=True)[:10]
                for e in entries:
                    p = os.path.join(d, e)
                    mt = datetime.datetime.fromtimestamp(os.path.getmtime(p))
                    log(sub + ": " + e + "  " + mt.isoformat())
                    # if Variable-related, pull Report.wer key lines
                    if "variable" in e.lower():
                        wer = os.path.join(p, "Report.wer")
                        if os.path.exists(wer):
                            try:
                                txt = open(wer, encoding="utf-16", errors="replace").read()
                            except Exception:
                                txt = open(wer, encoding="utf-8", errors="replace").read()
                            for line in txt.splitlines():
                                if line.startswith(("Event[0].", "Sig[", "DynamicSig[1.", "UI[2]", "AppName", "AppPath")) or "Exception" in line:
                                    log("   " + line[:200])
            except Exception as ex:
                log(sub + " ERR " + repr(ex))

    # ---- 3. LiveKernelReports / minidump (kernel-side) ----
    log("-- dumps --")
    for d in (os.path.join(X, "Windows", "Minidump"), os.path.join(X, "Windows", "LiveKernelReports")):
        if os.path.isdir(d):
            for f in sorted(os.listdir(d), key=lambda f: os.path.getmtime(os.path.join(d, f)), reverse=True)[:6]:
                p = os.path.join(d, f)
                log(d + ": " + f + " " + str(os.path.getsize(p)) + " " + datetime.datetime.fromtimestamp(os.path.getmtime(p)).isoformat())
        else:
            log(d + " N/A")

    # ---- 4. user hive Run key still intact? + last logon hints ----
    log("-- defaultuser0 hive Run --")
    hive = os.path.join(X, "Users", "defaultuser0", "NTUSER.DAT")
    log("hive exists: " + str(os.path.exists(hive)))
    if os.path.exists(hive):
        import winreg
        r = winreg.LoadKey(winreg.HKEY_LOCAL_MACHINE, "VXPOST", hive)
        try:
            k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXPOST\Software\Microsoft\Windows\CurrentVersion\Run")
            i = 0
            while True:
                try:
                    v, data, _ = winreg.EnumValue(k, i); i += 1
                    log("Run: " + v + " = " + str(data))
                except OSError:
                    break
        except OSError as ex:
            log("Run key ERR " + repr(ex))
        winreg.UnloadKey(winreg.HKEY_LOCAL_MACHINE, "VXPOST")

    # ---- 5. Variable.exe presence + mtime ----
    ve = os.path.join(X, "Variable", "Variable.exe")
    if os.path.exists(ve):
        log("Variable.exe mtime=" + datetime.datetime.fromtimestamp(os.path.getmtime(ve)).isoformat())
    else:
        log("Variable.exe MISSING at X:\\Variable")

    log("done")
except Exception:
    log("EXC " + traceback.format_exc()[:1500])
    log("done")
