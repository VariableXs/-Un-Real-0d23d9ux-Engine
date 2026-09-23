# -*- coding: utf-8 -*-
"""0xED 破解后的收尾：给 U 盘 Win11 所有真实用户的 NTUSER.DAT 离线写 Run 键
（Variable.exe 登录自启）。以 Default 模板现有值为基准，无则用 C:\\Variable\\Variable.exe。
逐行落盘+回读验证。输出 vx-runkeys.rpt
"""
import ctypes
import os
import subprocess
import traceback
import winreg

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-runkeys.rpt"
logf = None
EXE = r"C:\Variable\Variable.exe"

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

def mount(g, letter):
    k32 = ctypes.WinDLL("kernel32")
    k32.SetVolumeMountPointW.restype = ctypes.c_int
    k32.SetVolumeMountPointW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p]
    k32.DeleteVolumeMountPointW.argtypes = [ctypes.c_wchar_p]
    k32.DeleteVolumeMountPointW(letter + "\\")
    return bool(k32.SetVolumeMountPointW(letter + "\\", g))

def read_default_run(users_dir):
    hive = os.path.join(users_dir, "Default", "NTUSER.DAT")
    if not os.path.exists(hive):
        return None
    r = subprocess.run(["reg", "load", r"HKLM\VXD", hive], capture_output=True, text=True)
    val = None
    try:
        k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXD\Software\Microsoft\Windows\CurrentVersion\Run", 0, winreg.KEY_READ)
        i = 0
        while True:
            try:
                n, v, t = winreg.EnumValue(k, i); i += 1
                if "variable" in (n + v).lower():
                    val = (n, v)
                    break
            except OSError:
                break
        winreg.CloseKey(k)
    except OSError:
        pass
    subprocess.run(["reg", "unload", r"HKLM\VXD"], capture_output=True)
    return val

def main():
    global logf
    logf = open(LOG, "w", encoding="utf-8")
    try:
        log("=== runkeys ===")
        eng = find_vol("WIN_ENGINE")
        if not eng:
            log("FATAL no WIN_ENGINE"); return
        mount(eng, "X:"); log("X: mounted")
        users = r"X:\Users"
        base = read_default_run(users)
        log(f"default template run = {base}")
        name, cmd = base if base else ("Variable", EXE)
        log(f"using run entry: {name} = {cmd}")
        for u in os.listdir(users):
            hive = os.path.join(users, u, "NTUSER.DAT")
            if not os.path.isfile(hive):
                continue
            if u.lower() in ("public", "default user", "default", "all users", "desktop.ini"):
                continue
            if u.lower().startswith("wsi"):
                log(f"skip {u}")
                continue
            r = subprocess.run(["reg", "load", r"HKLM\VXU2", hive], capture_output=True, text=True, errors="replace")
            if r.returncode != 0:
                log(f"{u}: load failed {(r.stderr or '').strip()[:120]}")
                continue
            try:
                key = r"VXU2\Software\Microsoft\Windows\CurrentVersion\Run"
                k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key, 0, winreg.KEY_SET_VALUE | winreg.KEY_READ)
                winreg.SetValueEx(k, name, 0, winreg.REG_SZ, cmd)
                got = winreg.QueryValueEx(k, name)
                log(f"{u}: Run[{name}] -> {got[0]} (verify ok={got[0] == cmd})")
                winreg.CloseKey(k)
            except OSError as ex:
                log(f"{u}: write err {ex!r}")
            finally:
                ru = subprocess.run(["reg", "unload", r"HKLM\VXU2"], capture_output=True, text=True)
                if ru.returncode != 0:
                    log(f"{u}: UNLOAD FAILED {(ru.stderr or '').strip()[:120]}")
        log("=== runkeys done ===")
    except Exception:
        log("EXC-PATH:\n" + traceback.format_exc())
    finally:
        try: logf.close()
        except Exception: pass

if __name__ == "__main__":
    if not ctypes.windll.shell32.IsUserAnAdmin():
        print("need admin"); raise SystemExit(1)
    main()
