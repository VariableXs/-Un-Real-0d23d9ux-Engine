# -*- coding: utf-8 -*-
"""0xED v4：三方对账 — BCD osdevice 原始凭据 vs U盘 GPT 身份 vs SYSTEM MountedDevices。
纯只读。输出 vx-diag-0xed-v4.rpt
"""
import ctypes
import os
import subprocess
import struct
import traceback

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-diag-0xed-v4.rpt"
logf = None

def log(m):
    print(m, flush=True); logf.write(m + "\n"); logf.flush()

def run(cmd, timeout=120):
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

def read_gpt_header_and_entries(physical_drive):
    """读 GPT disk GUID 和分区表"""
    CREATE_ALWAYS, OPEN_EXISTING = 2, 3
    GENERIC_READ = 0x80000000
    SHARE = 3
    k32 = ctypes.WinDLL("kernel32")
    k32.CreateFileW.restype = ctypes.c_void_p
    k32.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32,
                                ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
    h = k32.CreateFileW(physical_drive, GENERIC_READ, SHARE, None, OPEN_EXISTING, 0, None)
    if not h or h == 0xFFFFFFFFFFFFFFFF:
        log(f"open {physical_drive} failed gle={ctypes.get_last_error()}")
        return None, []
    try:
        # GPT header at LBA1
        buf = ctypes.create_string_buffer(512)
        read = ctypes.c_uint32()
        k32.ReadFile.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32), ctypes.c_void_p]
        ok = k32.ReadFile(h, buf, 512, ctypes.byref(read), None)
        if not ok:
            log("ReadFile LBA1 failed")
            return None, []
        hdr = buf.raw
        if hdr[:8] != b"EFI PART":
            log("not GPT (MBR?) sig=" + hdr[:8].hex())
            return None, []
        disk_guid_raw = hdr[72:88]
        # EFI 混合端序转标准文本
        def guid_text(b):
            d1 = struct.unpack("<I", b[0:4])[0]
            d2 = struct.unpack("<H", b[4:6])[0]
            d3 = struct.unpack("<H", b[6:8])[0]
            rest = b[8:16].hex()
            return f"{d1:08x}-{d2:04x}-{d3:04x}-{rest[:4]}-{rest[4:]}"
        disk_guid = guid_text(disk_guid_raw)
        # 分区数
        num_parts = struct.unpack("<I", hdr[80:84])[0]
        part_lba = struct.unpack("<Q", hdr[72+8:88+8] if False else hdr[80+8:88+8])[0]
        # 简化：hdr[72]=disk guid;  hdr  fields: 0 sig,72 disk_guid,80 first_usable...实际:
        # header layout: 0:sig(8) 8:rev(4) 12:hsize(4) 16:crc(4) 20:res(4) 24:cur_lba(8) 32:bak(8)
        # 40:first_usable(8) 48:last_usable(8) 56:disk_guid(16) 72:part_entry_lba(8)
        part_lba = struct.unpack("<Q", hdr[72:80])[0]
        num_parts = struct.unpack("<I", hdr[80:84])[0]
        entry_size = struct.unpack("<I", hdr[84:88])[0]
        log(f"GPT disk_guid={disk_guid} num_parts={num_parts} entry_lba={part_lba} entry_size={entry_size}")
        # 读分区表
        ents = []
        SET = 0x80000000
        k32.SetFilePointer.argtypes = [ctypes.c_void_p, ctypes.c_long, ctypes.POINTER(ctypes.c_long), ctypes.c_uint32]
        lo = part_lba * 512
        k32.SetFilePointer(h, lo & 0xFFFFFFFF, ctypes.byref(ctypes.c_long(lo >> 32)), 0)
        blob = ctypes.create_string_buffer(entry_size * num_parts)
        k32.ReadFile(h, blob, entry_size * num_parts, ctypes.byref(read), None)
        for i in range(num_parts):
            e = blob.raw[i*entry_size:(i+1)*entry_size]
            if e[:16] == b"\x00"*16:
                continue
            pguid = guid_text(e[0:16])
            first = struct.unpack("<Q", e[32:40])[0]
            last = struct.unpack("<Q", e[40:48])[0]
            name16 = e[56:128].decode("utf-16-le", errors="replace").rstrip("\x00")
            ents.append((pguid, first, last, name16))
            log(f"  part[{i}] guid={pguid} first={first} name={name16!r}")
        return disk_guid, ents
    finally:
        k32.CloseHandle.argtypes = [ctypes.c_void_p]
        k32.CloseHandle(h)

def main():
    global logf
    logf = open(LOG, "w", encoding="utf-8")
    try:
        log("=== 0xED v4 三方对账（只读） ===")
        # 1) BCD 原始凭据
        esp = find_vol("VARIX-ESP")
        eng = find_vol("WIN_ENGINE")
        log(f"esp={esp} eng={eng}")
        if esp:
            mount(esp, "Y:")
            run(["bcdedit", "/store", r"Y:\EFI\Microsoft\Boot\BCD", "/enum", "{default}", "/v"])
        # 2) U 盘物理盘 GPT 身份：从卷 GUID 反查物理盘号用 PowerShell
        run(["powershell", "-NoProfile", "-Command",
             "Get-Disk | Select-Object Number, DiskId, FriendlyName, Size | Format-List | Out-String"])
        # 3) MountedDevices（离线 SYSTEM hive 是 SYSTEM\MountedDevices 同级）
        if eng:
            mount(eng, "X:")
        if esp and os.path.exists(r"Y:\EFI\Microsoft\Boot\BCD"):
            pass
        # MountedDevices 在 SYSTEM hive 根下
        if eng:
            run(["reg", "load", "HKLM\\VXSYS2", r"X:\Windows\System32\config\SYSTEM"])
            run(["reg", "query", r"HKLM\VXSYS2\MountedDevices"])
            run(["reg", "unload", "HKLM\\VXSYS2"])
        log("=== done v4 ===")
    except Exception:
        log("EXC-PATH:\n" + traceback.format_exc())
    finally:
        try: logf.close()
        except Exception: pass

if __name__ == "__main__":
    if not ctypes.windll.shell32.IsUserAnAdmin():
        print("need admin"); raise SystemExit(1)
    main()
