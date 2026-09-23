# -*- coding: utf-8 -*-
"""0xED 修复 v3：PortableOperatingSystem=1（便携系统标志）+ 蓝屏停屏（AutoReboot=0）+ 转储开关。
逐行落盘。输出 vx-fix-0xed-v3.rpt
"""
import ctypes
import os
import subprocess
import traceback
import winreg

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-fix-0xed-v3.rpt"
logf = None

def log(m):
    print(m, flush=True); logf.write(m + "\n"); logf.flush()

def run(cmd, timeout=300):
    log("$ " + " ".join(cmd))
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, errors="replace")
        for line in ((r.stdout or "") + (r.stderr or "")).splitlines():
            if line.strip():
                log("  | " + line.strip())
        return r.returncode
    except Exception as ex:
        log(f"  | EXC {ex!r}"); return -2

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

def mount(g, letter):
    k32 = ctypes.WinDLL("kernel32")
    k32.SetVolumeMountPointW.restype = ctypes.c_int
    k32.SetVolumeMountPointW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p]
    k32.DeleteVolumeMountPointW.argtypes = [ctypes.c_wchar_p]
    k32.DeleteVolumeMountPointW(letter + "\\")
    return bool(k32.SetVolumeMountPointW(letter + "\\", g))

def main():
    global logf
    logf = open(LOG, "w", encoding="utf-8")
    try:
        log("=== vx-fix-0xed v3 ===")
        eng = find_vol("WIN_ENGINE")
        esp = find_vol("VARIX-ESP")
        log(f"eng={bool(eng)} esp={bool(esp)}")
        if not eng:
            log("FATAL no WIN_ENGINE"); return
        mount(eng, "X:"); log("X: mounted")
        # ---- 离线注册表 ----
        run(["reg", "load", r"HKLM\VXSYS", r"X:\Windows\System32\config\SYSTEM"])
        # 便携系统标志（Windows To Go 同款）
        try:
            k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXSYS\ControlSet001\Control", 0, winreg.KEY_SET_VALUE | winreg.KEY_QUERY_VALUE)
            try:
                old = winreg.QueryValueEx(k, "PortableOperatingSystem")
                log(f"PortableOperatingSystem old={old}")
            except FileNotFoundError:
                old = None
                log("PortableOperatingSystem absent")
            winreg.SetValueEx(k, "PortableOperatingSystem", 0, winreg.REG_DWORD, 1)
            v = winreg.QueryValueEx(k, "PortableOperatingSystem")
            log(f"PortableOperatingSystem set -> {v[0]}")
            winreg.CloseKey(k)
        except Exception as ex:
            log(f"PortableOS EXC {ex!r}")
        # 蓝屏停屏 + 完整转储（下回蓝屏停住可拍全参数）
        try:
            k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXSYS\ControlSet001\Control\CrashControl", 0, winreg.KEY_SET_VALUE | winreg.KEY_QUERY_VALUE)
            winreg.SetValueEx(k, "AutoReboot", 0, winreg.REG_DWORD, 0)
            winreg.SetValueEx(k, "CrashDumpEnabled", 0, winreg.REG_DWORD, 7)  # 自动内存转储
            for n in ("AutoReboot", "CrashDumpEnabled"):
                log(f"CrashControl {n} -> {winreg.QueryValueEx(k, n)[0]}")
            winreg.CloseKey(k)
        except Exception as ex:
            log(f"CrashControl EXC {ex!r}")
        run(["reg", "unload", r"HKLM\VXSYS"])
        # ---- BCD 防自愈闸复核 ----
        if esp:
            mount(esp, "Y:"); log("Y: mounted (ESP)")
            run(["bcdedit", "/store", r"Y:\EFI\Microsoft\Boot\BCD", "/set", "{default}", "recoveryenabled", "No"])
            run(["bcdedit", "/store", r"Y:\EFI\Microsoft\Boot\BCD", "/set", "{default}", "bootstatuspolicy", "IgnoreAllFailures"])
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
