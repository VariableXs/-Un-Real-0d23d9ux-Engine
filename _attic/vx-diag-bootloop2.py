# -*- coding: utf-8 -*-
"""循环复盘诊断（全只读，绝不写入）：
1) NVRAM 现状：BootOrder / BootNext 残留 / Boot2001 是否仍是修复后形态
   （固件重启后会不会回滚/改写手工 Load Option——关键疑点）
2) U 盘分区布局（分区号/GPT 类型/大小/GPT GUID）
3) U 盘 ESP BCD {default} 的设备定位（bcdedit /v）+ U 盘 Windows 分区
   winload.efi / SYSTEM 配置单元存在性（引导失败的硬件级证据）
4) 各数据分区找 boot-select.json，读 last_boot / handoff 字段
   （内核每次启动会写 last_boot——时间戳/内容在变 = 内核在循环中真实运行）
"""
import os
import sys
import subprocess

EFI_GLOBAL = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}"
HERE = os.path.dirname(os.path.abspath(__file__))
LOG = os.path.join(HERE, "vx-diag-bootloop2.rpt")
out = []
ESP_TYPE = "{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}"
BASIC_TYPE = "{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}"


def utf16_ascii(b):
    s = b.decode("utf-16-le", "replace")
    return "".join(c for c in s if 32 <= ord(c) < 127)


def setup(k32_out):
    import ctypes
    from ctypes import wintypes as wt
    k32, adv = ctypes.windll.kernel32, ctypes.windll.advapi32
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


def nvram_dump():
    import ctypes
    from ctypes import wintypes as wt
    k32 = ctypes.windll.kernel32
    buf = ctypes.create_string_buffer(4096)
    attr_out = wt.DWORD(0)

    def get_var(name):
        n = k32.GetFirmwareEnvironmentVariableExW(
            name, EFI_GLOBAL, buf, 4096, ctypes.byref(attr_out))
        if n == 0:
            n = k32.GetFirmwareEnvironmentVariableW(name, EFI_GLOBAL, buf, 4096)
        return buf.raw[:n] if n else None

    out.append("=== NVRAM ===")
    order = get_var("BootOrder")
    if order:
        nums = [int.from_bytes(order[i:i + 2], "little") for i in range(0, len(order), 2)]
        out.append(f"BootOrder={[hex(n) for n in nums]}")
    bn = get_var("BootNext")
    out.append(f"BootNext={'0x' + bn.hex() if bn else '<absent>'} len={bn and len(bn)}")
    interest = nums if order else []
    for n in interest + [0x2001, 0x0002, 0x0003, 0x0000, 0x0001]:
        raw = get_var("Boot%04x" % n)
        if raw is None:
            continue
        fplen = int.from_bytes(raw[4:6], "little") if len(raw) >= 6 else -1
        end = raw.find(b"\x00\x00", 6)
        end = end + (end % 2) if end >= 0 else 6
        desc = utf16_ascii(raw[6:end])
        path_raw = raw[end + 2:end + 2 + max(fplen, 0)]
        out.append(f"Boot{n:04x}: total={len(raw)} fplen={fplen} desc='{desc}' "
                   f"path_head={path_raw[:24].hex()}")
    out.append("")


def ps_json(script):
    r = subprocess.run(["powershell", "-NoProfile", "-Command", script],
                       capture_output=True, text=True, timeout=180)
    return r.stdout, r.stderr, r.returncode


def partitions():
    script = ("$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
              "if (-not $d) { 'NO-USB-DISK'; exit 0 };"
              "'DISK=' + $d.Number + ' MODEL=' + $d.FriendlyName + ' SIZE=' + $d.Size;"
              "Get-Partition -DiskNumber $d.Number | Sort-Object PartitionNumber |"
              " ForEach-Object { 'PART#' + $_.PartitionNumber"
              + " + ' type=' + $_.GptType"
              + " + ' size=' + [math]::Round($_.Size/1GB,1) + 'GB'"
              + " + ' guid=' + $_.Guid"
              + " + ' letter=' + ($__.DriveLetter -replace '^$','(none)') }")
    o, e, _ = ps_json(script)
    out.append("=== U 盘分区布局 ===")
    out.append(o.strip())
    if e.strip():
        out.append("STDERR: " + e.strip()[:300])
    out.append("")
    return o


def check_windows_partition(disk_out):
    """找 U 盘 Windows 分区（basic data、非 ESP、非第一个），只读检查 winload.efi。"""
    parts = [l for l in disk_out.splitlines() if l.startswith("PART#")]
    cand = []
    for l in parts:
        if BASIC_TYPE in l:
            cand.append(l)
    out.append(f"basic-data 分区 {len(cand)} 个")
    mount_letter = "S:"
    win_part = None
    for l in cand:
        m = [x.split('=')[1].strip() for x in l.split() if x.startswith('part#=')]
    # 用 PowerShell 逐个挂 basic 分区找 \Windows
    script = ("$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
              "if (-not $d) { exit 0 };"
              "$ps = Get-Partition -DiskNumber $d.Number |"
              " Where-Object GptType -eq '{%s}' | Sort-Object PartitionNumber;"
              "foreach ($p in $ps) {"
              "  $lp = 'S:';"
              "  $existing = ($p.AccessPaths | Where-Object { $_ -like '?:\\' });"
              "  if ($existing) { $lp = $existing.TrimEnd('\\'); 'ALREADY=' + $lp + ' part=' + $p.PartitionNumber }"
              "  else { Add-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath ($lp + '\\') | Out-Null; 'MOUNTED=' + $lp + ' part=' + $p.PartitionNumber };"
              "  $wl = Join-Path $lp 'Windows\\System32\\winload.efi';"
              "  $sys = Join-Path $lp 'Windows\\System32\\config\\SYSTEM';"
              "  $bs = Join-Path $lp 'boot-select.json';"
              "  $vi = Join-Path $lp 'Variable\\Variable.exe';"
              "  'CHECK part=' + $p.PartitionNumber"
              "    + ' winload=' + (Test-Path $wl)"
              "    + ' SYSTEM=' + (Test-Path $sys)"
              + "    + ' bootselect_json=' + (Test-Path $bs)"
              + "    + ' variable_exe=' + (Test-Path $vi);"
              "  Get-Item $wl,$sys,$bs -ErrorAction SilentlyContinue |"
              "    ForEach-Object { 'FILE ' + $_.FullName + ' mtime=' + $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') + ' size=' + $_.Length };"
              "}" % BASIC_TYPE.strip('{}'))
    o2, e2, _ = ps_json(script)
    out.append("=== U 盘各 basic 分区检查 ===")
    out.append(o2.strip())
    if e2.strip():
        out.append("STDERR: " + e2.strip()[:300])
    # 只摘我们挂的 S:
    un = ("$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
          "if ($d) { $ps = Get-Partition -DiskNumber $d.Number |"
          " Where-Object GptType -eq '{%s}';"
          " foreach ($p in $ps) { if ($p.AccessPaths -contains 'S:\\') {"
          " Remove-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath 'S:\\' | Out-Null; 'UNMOUNTED=S: part=' + $p.PartitionNumber } } }"
          % BASIC_TYPE.strip('{}'))
    o3, _, _ = ps_json(un)
    out.append(o3.strip())
    out.append("")


def read_shared_cfg_via_esp():
    """ESP 挂 Y: 读 boot-select.json 副本（ESP 双副本之一）。"""
    script = ("$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
              "if (-not $d) { 'NO-DISK'; exit 0 };"
              "$p = Get-Partition -DiskNumber $d.Number | Where-Object GptType -eq "
              "'{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' | Select-Object -First 1;"
              "$lp = 'Y:';"
              "if ($p.AccessPaths -contains 'Y:\\') { 'ALREADY' }"
              "else { Add-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath 'Y:\\' | Out-Null; 'MOUNTED' };"
              "$cfg = 'Y:\\boot-select.json';"
              "if (Test-Path $cfg) { Get-Item $cfg | ForEach-Object { 'CFG mtime=' + $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') + ' size=' + $_.Length };"
              "'---'; Get-Content $cfg -Raw; '---' }"
              "else { 'NO boot-select.json on ESP' }")
    o, e, _ = ps_json(script)
    out.append("=== ESP boot-select.json ===")
    out.append(o.strip())
    if e.strip():
        out.append("STDERR: " + e.strip()[:200])
    un = ("$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1;"
          "if ($d) { $p = Get-Partition -DiskNumber $d.Number | Where-Object GptType -eq "
          "'{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' | Select-Object -First 1;"
          "if ($p -and ($p.AccessPaths -contains 'Y:\\')) { Remove-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath 'Y:\\' | Out-Null; 'UNMOUNTED=Y:' } }")
    o2, _, _ = ps_json(un)
    out.append(o2.strip())


def main():
    setup(None)
    nvram_dump()
    do = partitions()
    check_windows_partition(do)
    read_shared_cfg_via_esp()
    out.append("VX-DIAG-BOOTLOOP2-DONE (read-only)")


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
        for _ in range(120):
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8").read()[-7000:])
                break
        else:
            print("timeout waiting report")
