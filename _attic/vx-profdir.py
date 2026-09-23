# -*- coding: utf-8 -*-
"""查 U 盘 Win11 ProfileList 重定向（提权，只读）。输出 vx-profdir.rpt"""
import ctypes
import os
import subprocess
import traceback

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-profdir.rpt"
logf = None

def log(m):
    print(m, flush=True); logf.write(m + "\n"); logf.flush()

def find_vol(label):
    k32 = ctypes.WinDLL("kernel32")
    k32.FindFirstVolumeW.restype = ctypes.c_void_p
    k32.FindFirstVolumeW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32]
    k32.FindNextVolumeW.restype = ctypes.c_int
    k32.FindNextVolumeW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_uint32]
    k32.FindVolumeClose.argtypes = [ctypes.c_void_p]
    k32.GetVolumeInformationW.restype = ctypes.c_int
    k32.GetVolumeInformationW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32,
                                          ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                                          ctypes.c_wchar_p, ctypes.c_uint32]
    buf = ctypes.create_unicode_buffer(300)
    h = k32.FindFirstVolumeW(buf, 300)
    if not h:
        return None
    found = None
    while True:
        root = buf.value.rstrip("\\") + "\\"
        name = ctypes.create_unicode_buffer(261)
        if k32.GetVolumeInformationW(root, name, 261, None, None, None, None, 0) and name.value == label:
            found = buf.value
            break
        if not k32.FindNextVolumeW(h, buf, 300):
            break
    k32.FindVolumeClose(h)
    return found

def main():
    global logf
    logf = open(LOG, "w", encoding="utf-8")
    try:
        eng = find_vol("WIN_ENGINE")
        if not eng:
            log("FATAL no eng"); return
        k32 = ctypes.WinDLL("kernel32")
        k32.SetVolumeMountPointW.restype = ctypes.c_int
        k32.SetVolumeMountPointW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p]
        k32.DeleteVolumeMountPointW.argtypes = [ctypes.c_wchar_p]
        k32.DeleteVolumeMountPointW("X:\\")
        k32.SetVolumeMountPointW("X:\\", eng)
        log("X: mounted")
        # 全 U 盘卷找 NTUSER.DAT（真实用户 profile 可能重定向到其他分区）
        for label in ("VARIX_SYS", "SNAPSHOT", "SHARED"):
            g = find_vol(label)
            if not g:
                continue
            letter = {"VARIX_SYS": "V:", "SNAPSHOT": "N:", "SHARED": "S:"}[label]
            k32.DeleteVolumeMountPointW(letter)
            k32.SetVolumeMountPointW(letter + "\\", g)
            log(f"{label} -> {letter}")
            for root, dirs, files in os.walk(letter + "\\"):
                if root.count("\\") > 4:
                    dirs.clear(); continue
                if "NTUSER.DAT" in files:
                    log(f"  NTUSER.DAT: {root}")
        # SOFTWARE ProfileList
        import winreg
        r = subprocess.run(["reg", "load", r"HKLM\VXSOFT", r"X:\Windows\System32\config\SOFTWARE"],
                           capture_output=True, text=True, errors="replace")
        log(f"load SOFTWARE rc={r.returncode} {(r.stderr or '').strip()[:80]}")
        if r.returncode == 0:
            r2 = subprocess.run(["reg", "query", r"HKLM\VXSOFT\Microsoft\Windows NT\CurrentVersion\ProfileList", "/s"],
                                capture_output=True, text=True, errors="replace")
            for line in (r2.stdout or "").splitlines():
                if line.strip():
                    log("  | " + line.strip())
            subprocess.run(["reg", "unload", r"HKLM\VXSOFT"], capture_output=True)
        log("=== profdir done ===")
    except Exception:
        log("EXC:\n" + traceback.format_exc())
    finally:
        try: logf.close()
        except Exception: pass

if __name__ == "__main__":
    if not ctypes.windll.shell32.IsUserAnAdmin():
        print("need admin"); raise SystemExit(1)
    main()
