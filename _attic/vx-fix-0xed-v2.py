# -*- coding: utf-8 -*-
"""0xED 修复 v2：chkdsk X: /f（无尾斜杠）+ ESP 防自愈闸。逐行落盘。"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import traceback

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-fix-0xed-v2.rpt"
logf = None

def log(msg):
    print(msg, flush=True)
    logf.write(msg + "\n")
    logf.flush()

def run(cmd, timeout=3600):
    log("$ " + " ".join(cmd))
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, errors="replace")
        for line in ((r.stdout or "") + (r.stderr or "")).splitlines():
            if line.strip():
                log("  | " + line.strip())
        return r.returncode
    except subprocess.TimeoutExpired:
        log("  | TIMEOUT"); return -1
    except Exception as ex:
        log(f"  | EXC {ex!r}"); return -2

def find_volumes():
    k32 = ctypes.WinDLL("kernel32")
    k32.FindFirstVolumeW.restype = wt.HANDLE
    k32.FindFirstVolumeW.argtypes = [ctypes.c_wchar_p, wt.DWORD]
    k32.FindNextVolumeW.restype = wt.BOOL
    k32.FindNextVolumeW.argtypes = [wt.HANDLE, ctypes.c_wchar_p, wt.DWORD]
    k32.FindVolumeClose.restype = wt.BOOL
    k32.FindVolumeClose.argtypes = [wt.HANDLE]
    k32.GetVolumeInformationW.restype = wt.BOOL
    k32.GetVolumeInformationW.argtypes = [wt.LPCWSTR, wt.LPWSTR, wt.DWORD, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, wt.LPWSTR, wt.DWORD]
    vols, buf = [], ctypes.create_unicode_buffer(300)
    h = k32.FindFirstVolumeW(buf, 300)
    if h == wt.HANDLE(-1).value:
        return vols
    while True:
        g = buf.value
        root = g.rstrip("\\") + "\\"
        name = ctypes.create_unicode_buffer(261)
        ok = k32.GetVolumeInformationW(root, name, 261, None, None, None, None, 0)
        vols.append((g, name.value if ok else "?"))
        if not k32.FindNextVolumeW(h, buf, 300):
            break
    k32.FindVolumeClose(h)
    return vols

def mount(guid_vol, letter):
    k32 = ctypes.WinDLL("kernel32")
    k32.SetVolumeMountPointW.restype = wt.BOOL
    k32.SetVolumeMountPointW.argtypes = [wt.LPCWSTR, wt.LPCWSTR]
    k32.DeleteVolumeMountPointW.restype = wt.BOOL
    k32.DeleteVolumeMountPointW.argtypes = [wt.LPCWSTR]
    k32.DeleteVolumeMountPointW(letter + "\\")
    return bool(k32.SetVolumeMountPointW(letter + "\\", guid_vol))

def main():
    global logf
    logf = open(LOG, "w", encoding="utf-8")
    try:
        log("=== vx-fix-0xed v2 ===")
        vols = find_volumes()
        for g, l in vols:
            log(f"vol label={l!r}")
        eng = next((g for g, l in vols if l == "WIN_ENGINE"), None)
        esp = next((g for g, l in vols if l == "VARIX-ESP"), None)
        log(f"eng={bool(eng)} esp={bool(esp)}")
        if eng:
            mount(eng, "X:")
            log("X: mounted")
            rc = run(["chkdsk", "X:", "/f"], timeout=3600)
            log(f"chkdsk rc={rc}")
        if esp:
            mount(esp, "Y:")
            log("Y: mounted (ESP)")
            run(["bcdedit", "/store", "Y:\\EFI\\Microsoft\\Boot\\BCD", "/set", "{default}", "recoveryenabled", "No"])
            run(["bcdedit", "/store", "Y:\\EFI\\Microsoft\\Boot\\BCD", "/set", "{default}", "bootstatuspolicy", "IgnoreAllFailures"])
            run(["bcdedit", "/store", "Y:\\EFI\\Microsoft\\Boot\\BCD", "/enum", "{default}"])
        log("=== fix done ===")
    except Exception:
        log("EXC-PATH:\n" + traceback.format_exc())
    finally:
        try: logf.close()
        except Exception: pass

if __name__ == "__main__":
    if not ctypes.windll.shell32.IsUserAnAdmin():
        print("need admin"); raise SystemExit(1)
    main()
