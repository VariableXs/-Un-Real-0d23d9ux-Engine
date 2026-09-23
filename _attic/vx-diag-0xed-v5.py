# -*- coding: utf-8 -*-
"""0xED v5：BCD hive 原始 osdevice 凭据 vs U盘 GPT disk GUID 终极对账。只读。"""
import ctypes
import os
import struct
import subprocess
import traceback
import winreg

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-diag-0xed-v5.rpt"
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

def mount(g, letter):
    k32 = ctypes.WinDLL("kernel32")
    k32.SetVolumeMountPointW.restype = ctypes.c_int
    k32.SetVolumeMountPointW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p]
    k32.DeleteVolumeMountPointW.argtypes = [ctypes.c_wchar_p]
    k32.DeleteVolumeMountPointW(letter + "\\")
    return bool(k32.SetVolumeMountPointW(letter + "\\", g))

def guid_text(b):
    d1 = struct.unpack("<I", b[0:4])[0]
    d2 = struct.unpack("<H", b[4:6])[0]
    d3 = struct.unpack("<H", b[6:8])[0]
    rest = b[8:16].hex()
    return f"{d1:08x}-{d2:04x}-{d3:04x}-{rest[:4]}-{rest[4:]}"

def read_gpt(device_path):
    k32 = ctypes.WinDLL("kernel32")
    k32.CreateFileW.restype = ctypes.c_void_p
    k32.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32,
                                ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
    GENERIC_READ, SHARE, OPEN_EXISTING = 0x80000000, 3, 3
    h = k32.CreateFileW(device_path, GENERIC_READ, SHARE, None, OPEN_EXISTING, 0, None)
    if not h or h == 0xFFFFFFFFFFFFFFFF:
        log(f"open {device_path} failed gle={ctypes.get_last_error()}")
        return
    try:
        k32.ReadFile.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32,
                                 ctypes.POINTER(ctypes.c_uint32), ctypes.c_void_p]
        k32.SetFilePointer.argtypes = [ctypes.c_void_p, ctypes.c_long, ctypes.POINTER(ctypes.c_long), ctypes.c_uint32]
        buf = ctypes.create_string_buffer(512)
        rd = ctypes.c_uint32()
        if not k32.ReadFile(h, buf, 512, ctypes.byref(rd), None):
            log("read LBA1 fail"); return
        hdr = buf.raw
        if hdr[:8] != b"EFI PART":
            log("LBA1 not GPT: " + hdr[:16].hex()); return
        disk_guid = guid_text(hdr[56:72])
        part_lba = struct.unpack("<Q", hdr[72:80])[0]
        num = struct.unpack("<I", hdr[80:84])[0]
        esz = struct.unpack("<I", hdr[84:88])[0]
        log(f"USB GPT disk_guid={disk_guid}")
        lo = part_lba * 512
        k32.SetFilePointer(h, lo & 0xFFFFFFFF, ctypes.byref(ctypes.c_long(lo >> 32)), 0)
        blob = ctypes.create_string_buffer(esz * num)
        k32.ReadFile(h, blob, esz * num, ctypes.byref(rd), None)
        for i in range(num):
            e = blob.raw[i*esz:(i+1)*esz]
            if e[:16] == b"\x00" * 16:
                continue
            log(f"  part guid={guid_text(e[0:16])} first={struct.unpack('<Q', e[32:40])[0]} name={e[56:128].decode('utf-16-le', errors='replace').rstrip(chr(0))!r}")
    finally:
        k32.CloseHandle.argtypes = [ctypes.c_void_p]
        k32.CloseHandle(h)

def main():
    global logf
    logf = open(LOG, "w", encoding="utf-8")
    try:
        log("=== 0xED v5 ===")
        # 1) 枚举物理盘，读 U 盘 GPT（Lenovo thinkplus）
        for i in range(4):
            read_gpt(f"\\\\.\\PhysicalDrive{i}")
        # 2) BCD hive 原始元素
        esp = find_vol("VARIX-ESP")
        if esp:
            mount(esp, "Y:")
            bcd = r"Y:\EFI\Microsoft\Boot\BCD"
            subprocess.run(["reg", "load", r"HKLM\VXBCD", bcd], capture_output=True)
            base = r"HKLM\VXBCD\Objects"
            try:
                k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, base)
                i = 0
                while True:
                    try:
                        obj = winreg.EnumKey(k, i); i += 1
                    except OSError:
                        break
                    if not obj.lower().startswith("a8897e9f") and "03e8a6a4" not in obj.lower():
                        continue
                    log(f"--- object {obj}")
                    try:
                        ek = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, base + "\\" + obj + "\\Elements")
                        j = 0
                        while True:
                            try:
                                el = winreg.EnumKey(ek, j); j += 1
                            except OSError:
                                break
                            try:
                                v, t = winreg.QueryValueEx(ek, el)
                                if t == winreg.REG_BINARY and len(v) <= 64:
                                    log(f"  el {el} = {v.hex()}")
                                else:
                                    log(f"  el {el} type={t} len={len(v) if isinstance(v, bytes) else '-'} val={v if not isinstance(v, bytes) else ''}")
                            except OSError as ex2:
                                log(f"  el {el} qerr {ex2!r}")
                    except OSError as ex3:
                        log(f"  elements err {ex3!r}")
            finally:
                subprocess.run(["reg", "unload", r"HKLM\VXBCD"], capture_output=True)
        log("=== done v5 ===")
    except Exception:
        log("EXC-PATH:\n" + traceback.format_exc())
    finally:
        try: logf.close()
        except Exception: pass

if __name__ == "__main__":
    if not ctypes.windll.shell32.IsUserAnAdmin():
        print("need admin"); raise SystemExit(1)
    main()
