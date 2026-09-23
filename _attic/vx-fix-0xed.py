# -*- coding: utf-8 -*-
"""0xED UNMOUNTABLE_BOOT_VOLUME 离线诊断+修复（U盘侧施工，内置盘只读体检）。
逐行落盘，崩溃留遗言。输出: _attic\\vx-fix-0xed.rpt
"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import traceback

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-fix-0xed.rpt"
logf = None

def log(msg):
    print(msg, flush=True)
    logf.write(msg + "\n")
    logf.flush()

def run(cmd, timeout=600):
    log(f"$ {' '.join(cmd)}")
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout,
                           errors="replace")
        out = (r.stdout or "") + (r.stderr or "")
        for line in out.splitlines():
            if line.strip():
                log("  | " + line.strip())
        return r.returncode
    except subprocess.TimeoutExpired:
        log("  | TIMEOUT")
        return -1
    except Exception as ex:
        log(f"  | EXC {ex!r}")
        return -2

def find_volumes():
    """返回 [(drive_without_slash, label), ...]"""
    k32 = ctypes.WinDLL("kernel32")
    k32.FindFirstVolumeW.restype = wt.HANDLE
    k32.FindFirstVolumeW.argtypes = [ctypes.c_wchar_p, wt.DWORD]
    k32.FindNextVolumeW.restype = wt.BOOL
    k32.FindNextVolumeW.argtypes = [wt.HANDLE, ctypes.c_wchar_p, wt.DWORD]
    k32.FindVolumeClose.restype = wt.BOOL
    k32.FindVolumeClose.argtypes = [wt.HANDLE]
    k32.GetVolumeInformationW.restype = wt.BOOL
    k32.QueryDosDeviceW.restype = wt.DWORD
    k32.QueryDosDeviceW.argtypes = [wt.LPCWSTR, ctypes.c_wchar_p, wt.DWORD]
    vols, buf = [], ctypes.create_unicode_buffer(300)
    h = k32.FindFirstVolumeW(buf, 300)
    if h == wt.HANDLE(-1).value:
        return vols
    while True:
        guid_vol = buf.value
        # 去掉尾部反斜杠得到挂载点格式 \\?\Volume{guid}
        root = guid_vol.rstrip("\\") + "\\"
        name = ctypes.create_unicode_buffer(261)
        ok = k32.GetVolumeInformationW(root, name, 261, None, None, None, None, 0)
        # 目标设备路径
        dd = ctypes.create_unicode_buffer(500)
        dos = guid_vol[4:]  # Volume{guid}\
        k32.QueryDosDeviceW(dos, dd, 500)
        label = name.value if ok else "?"
        vols.append((guid_vol, label, dd.value))
        if not k32.FindNextVolumeW(h, buf, 300):
            break
    k32.FindVolumeClose(h)
    return vols

def set_letter(guid_vol, letter):
    """SetVolumeMountPoint 分配盘符，letter 形如 'X:'"""
    k32 = ctypes.WinDLL("kernel32")
    k32.SetVolumeMountPointW.restype = wt.BOOL
    k32.SetVolumeMountPointW.argtypes = [wt.LPCWSTR, wt.LPCWSTR]
    mp = letter + "\\"
    k32.DeleteVolumeMountPointW.argtypes = [wt.LPCWSTR]
    k32.DeleteVolumeMountPointW.restype = wt.BOOL
    # 若盘符被占，先摘（仅临时盘符 X/Y/S）
    k32.DeleteVolumeMountPointW(mp)
    ok = k32.SetVolumeMountPointW(mp, guid_vol)
    return bool(ok)

def main():
    global logf
    logf = open(LOG, "w", encoding="utf-8")
    try:
        log("=== vx-fix-0xed start ===")
        vols = find_volumes()
        for g, label, dev in vols:
            log(f"vol label={label!r} dev={dev}")
        # 定位三块
        win_engine = next((g for g, l, d in vols if l == "WIN_ENGINE"), None)
        shared = next((g for g, l, d in vols if l == "SHARED"), None)
        # ESP: FAT32 小卷，label 通常空且 dev 是 U 盘
        esp = None
        if win_engine:
            usb_dev = next((d for g, l, d in vols if g == win_engine), "")
            for g, l, d in vols:
                if l in ("WIN_ENGINE", "SHARED", "Windows-SSD", "WINRE_DRV"):
                    continue
                if "USB" in d.upper() or "U盘" in d:
                    esp = g  # 取第一个 U 盘上的非数据卷
                    break
        log(f"win_engine={bool(win_engine)} shared={bool(shared)} esp={bool(esp)}")
        if not win_engine:
            log("FATAL: WIN_ENGINE not found (U盘未接?)")
            return
        X = "X:"
        if not set_letter(win_engine, X):
            log("FATAL: assign X failed")
            return
        log("X: assigned")
        # 1) 只读 chkdsk 看损伤
        rc = run(["chkdsk", X + "\\", "/f"], timeout=1800)  # 直接修：U盘侧NTFS
        log(f"chkdsk /f rc={rc}")
        # 2) 回读关键文件
        for p in (r"X:\Windows\System32\ntoskrnl.exe", r"X:\Windows\System32\config\SYSTEM",
                  r"X:\Variable\Variable.exe"):
            try:
                st = os.stat(p)
                log(f"OK {p} size={st.st_size}")
            except Exception as ex:
                log(f"MISS {p} {ex!r}")
        # 3) 重上 BCD 防自愈闸（ESP）
        if esp:
            Y = "Y:"
            if set_letter(esp, Y):
                log("Y: assigned (ESP)")
                run(["bcdedit", "/store", Y + r"\EFI\Microsoft\Boot\BCD",
                     "/set", "{default}", "recoveryenabled", "No"])
                run(["bcdedit", "/store", Y + r"\EFI\Microsoft\Boot\BCD",
                     "/set", "{default}", "bootstatuspolicy", "IgnoreAllFailures"])
                run(["bcdedit", "/store", Y + r"\EFI\Microsoft\Boot\BCD", "/enum", "{default}"])
            else:
                log("WARN: assign Y failed")
        # 4) 内置盘只读体检（脏位）
        run(["fsutil", "dirty", "query", "C:"], timeout=60)
        log("=== fix done ===")
    except Exception:
        log("EXC-PATH:\n" + traceback.format_exc())
    finally:
        try:
            logf.close()
        except Exception:
            pass

if __name__ == "__main__":
    if not ctypes.windll.shell32.IsUserAnAdmin():
        print("need admin")
        sys.exit(1)
    main()
