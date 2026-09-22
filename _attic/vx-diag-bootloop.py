# -*- coding: utf-8 -*-
"""引导循环诊断（全只读，绝不写入）：
1) dump Boot2001/Boot0002 固件项原始字节——解析 FilePathListLength，验证
   「bcdedit set device 把 Load Option 设备路径改空」假说（52 字节 vs 182 字节）。
2) 挂 U 盘 ESP（USB 总线盘，Add-PartitionAccessPath 只读挂载）→
   bcdedit /store <ESP>\EFI\Microsoft\Boot\BCD /enum all —— 看 default 指向。
3) 摘除盘符。全程零写入。"""
import os
import sys
import subprocess

EFI_GLOBAL = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}"
HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.join(HERE, "vx-diag-bootloop.rpt")
out = []

INTERESTING = [0x2001, 0x0002, 0x0003]  # 坏项嫌疑 / copy 源 / 内置 Windows（对照）


def utf16_ascii(b):
    s = b.decode("utf-16-le", "replace")
    return "".join(c for c in s if 32 <= ord(c) < 127)


def dump_load_options():
    import ctypes
    from ctypes import wintypes as wt
    k32 = ctypes.windll.kernel32
    adv = ctypes.windll.advapi32
    adv.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
    adv.OpenProcessToken.restype = wt.BOOL
    adv.LookupPrivilegeValueW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p]
    adv.LookupPrivilegeValueW.restype = wt.BOOL
    adv.AdjustTokenPrivileges.argtypes = [wt.HANDLE, wt.BOOL, ctypes.c_void_p, wt.DWORD,
                                          ctypes.c_void_p, ctypes.POINTER(wt.DWORD)]
    adv.AdjustTokenPrivileges.restype = wt.BOOL
    tok = wt.HANDLE()
    adv.OpenProcessToken(k32.GetCurrentProcess(), 0x20 | 0x8, ctypes.byref(tok))

    class LUID(ctypes.Structure):
        _fields_ = [("LowPart", ctypes.c_ulong), ("HighPart", ctypes.c_long)]

    class TP(ctypes.Structure):
        _fields_ = [("Luid", LUID), ("Attr", wt.DWORD)]

    class TPP(ctypes.Structure):
        _fields_ = [("Count", wt.DWORD), ("Priv", TP * 1)]

    luid = LUID()
    adv.LookupPrivilegeValueW(None, "SeSystemEnvironmentPrivilege", ctypes.byref(luid))
    tp = TPP()
    tp.Count = 1
    tp.Priv[0].Luid = luid
    tp.Priv[0].Attr = 2
    adv.AdjustTokenPrivileges(tok, False, ctypes.byref(tp), 0, None, None)
    out.append(f"privilege: tok={tok.value:#x}")

    buf = ctypes.create_string_buffer(4096)
    attr_out = wt.DWORD(0)

    def get_var(name):
        n = k32.GetFirmwareEnvironmentVariableExW(
            name, EFI_GLOBAL, buf, 4096, ctypes.byref(attr_out))
        if n == 0:
            n = k32.GetFirmwareEnvironmentVariableW(name, EFI_GLOBAL, buf, 4096)
        return buf.raw[:n] if n else None

    for num in INTERESTING:
        name = "Boot%04x" % num
        raw = get_var(name)
        if raw is None:
            out.append(f"{name}: <absent>")
            continue
        total = len(raw)
        attr = int.from_bytes(raw[0:4], "little")
        fplen = int.from_bytes(raw[4:6], "little")
        # desc: UTF-16 NUL 结尾，从偏移 6 起
        desc_end = raw.find(b"\x00\x00", 6)
        desc_end = desc_end + (desc_end % 2) if desc_end >= 0 else 6  # 对齐 2
        desc = utf16_ascii(raw[6:desc_end])
        path_off = desc_end + 2
        path_raw = raw[path_off:path_off + fplen]
        out.append(f"{name}: total={total} attr={attr:#x} FilePathListLength={fplen} "
                   f"desc='{desc}'")
        out.append(f"  path[{path_off}:{path_off+fplen}] hex={path_raw.hex()}")
        # 判定：4 字节 = 纯 END 节点（7F FF 04 00）= 无效路径
        if fplen <= 6:
            out.append(f"  VERDICT: FilePathListLength={fplen} -> 路径实质为空/仅 END 节点，"
                       f"固件无法兑现此项（循环根因嫌疑成立）")
        elif fplen <= 12:
            out.append(f"  VERDICT: 路径过短，疑似残缺")
        else:
            out.append(f"  VERDICT: 路径长度正常")
    out.append("")


def usb_esp_mount():
    """找 USB 总线磁盘的 ESP 分区，挂 Y:（只读挂载，不写盘面）。"""
    ps = ("$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
          "if (-not $d) { 'NO-USB-DISK'; exit 0 };"
          "$n = $d.Number;"
          "'USB-DISK-NUMBER=' + $n;"
          "'USB-DISK-MODEL=' + $d.FriendlyName;"
          "$p = Get-Partition -DiskNumber $n | Where-Object GptType -eq "
          "'{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' | Select-Object -First 1;"
          "if (-not $p) { 'NO-ESP-PARTITION'; exit 0 };"
          "'ESP-PARTITION-NUMBER=' + $p.PartitionNumber;"
          "$existing = ($p.AccessPaths | Where-Object { $_ -like '*\\' -and $_.Length -le 4 });"
          "if ($existing) { 'ALREADY-MOUNTED=' + $existing; exit 0 };"
          "Add-PartitionAccessPath -DiskNumber $n -PartitionNumber $p.PartitionNumber "
          "-AccessPath 'Y:\\' | Out-Null;"
          "'MOUNTED=Y:'")
    r = subprocess.run(["powershell", "-NoProfile", "-Command", ps],
                       capture_output=True, text=True, timeout=120)
    out.append("=== usb esp mount ===")
    out.append(r.stdout.strip())
    if r.stderr.strip():
        out.append("STDERR: " + r.stderr.strip()[:500])
    return r.stdout


def bcd_read(mount_out):
    out.append("")
    out.append("=== usb BCD (READ-ONLY enum) ===")
    if "ALREADY-MOUNTED=" in mount_out:
        letter = mount_out.split("ALREADY-MOUNTED=")[1].strip().rstrip("\\")
    elif "MOUNTED=Y:" in mount_out:
        letter = "Y:"
    else:
        out.append("ESP 未挂载成功，跳过 BCD 读取")
        return None
    store = f"{letter}\\EFI\\Microsoft\\Boot\\BCD"
    if not os.path.exists(store):
        out.append(f"{store} 不存在！U 盘 ESP 上没有 Microsoft 引导存储")
        # 顺便列 ESP 根，看引导文件分布
        for sub in ("EFI", "EFI\\Microsoft", "EFI\\Microsoft\\Boot", "EFI\\BOOT", "EFI\\limine"):
            p = f"{letter}\\{sub}"
            try:
                names = os.listdir(p)
                out.append(f"  {sub}: {names[:20]}")
            except OSError as e2:
                out.append(f"  {sub}: {e2}")
        return None
    r = subprocess.run(["bcdedit", "/store", store, "/enum", "all"],
                       capture_output=True, text=True, timeout=60)
    out.append(r.stdout.strip() or "(empty)")
    if r.returncode != 0 or r.stderr.strip():
        out.append(f"bcdedit rc={r.returncode} stderr={r.stderr.strip()[:400]}")
    return letter


def unmount(letter):
    if letter != "Y:":
        return  # 本来就挂着的，不动
    ps = ("$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
          "if (-not $d) { exit 0 };"
          "$p = Get-Partition -DiskNumber $d.Number | Where-Object GptType -eq "
          "'{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' | Select-Object -First 1;"
          "if ($p) { Remove-PartitionAccessPath -DiskNumber $d.Number "
          "-PartitionNumber $p.PartitionNumber -AccessPath 'Y:\\' | Out-Null; 'UNMOUNTED=Y:' }")
    r = subprocess.run(["powershell", "-NoProfile", "-Command", ps],
                       capture_output=True, text=True, timeout=120)
    out.append("")
    out.append("=== unmount ===")
    out.append(r.stdout.strip() or "(no output)")


def main():
    dump_load_options()
    mo = usb_esp_mount()
    letter = bcd_read(mo)
    if letter:
        unmount(letter)
    out.append("")
    out.append("VX-DIAG-BOOTLOOP-DONE (read-only)")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--elevated":
        try:
            main()
        except Exception:
            import traceback
            out.append("EXCEPTION:\n" + traceback.format_exc())
        with open(LOG, "w", encoding="utf-8") as f:
            f.write("\n".join(out) + "\n")
    else:
        import ctypes
        import time
        me = os.path.abspath(__file__)
        rc = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", sys.executable, f'"{me}" --elevated', None, 0)
        print("ShellExecute rc=", rc)
        for _ in range(90):
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8").read()[-6000:])
                break
        else:
            print("timeout waiting report")
