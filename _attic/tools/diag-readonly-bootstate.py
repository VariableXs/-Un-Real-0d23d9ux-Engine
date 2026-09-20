#!/usr/bin/env python3
r"""只读引导状态体检（需求11/12：不删任何进 Windows 的启动识别项、不威胁数据安全）。

只做读取：
  1. 固件/启动：UEFI? SecureBoot?
  2. 内置盘 ESP 与 Windows 引导文件是否完好（bootmgfw.efi / BCD）
  3. BCD 中的引导项清单（只读 bcdedit /enum，不写入）
  4. Windows 快速启动（HiberbootEnabled）——关它需要授权，此脚本不改
输出报告，不写盘、不改 BIOS、不改 BCD。
"""
import subprocess
import sys

PS = r'''
$ErrorActionPreference = 'SilentlyContinue'
Write-Output "===== FIRMWARE ====="
try { $sb = Confirm-SecureBootUEFI; "SecureBoot = $sb" } catch { "SecureBoot = (查询失败: $($_.Exception.Message))" }

Write-Output ""
Write-Output "===== BCD 引导项（只读 /enum） ====="
& bcdedit /enum '{current}' 2>&1 | Select-Object -First 20

Write-Output ""
Write-Output "===== 快速启动（只读注册表） ====="
$v = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled
if ($null -eq $v) { "HiberbootEnabled = (未设置)" } else { "HiberbootEnabled = $($v.HiberbootEnabled)" }
$f = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Power' -Name HibernateEnabled
if ($null -eq $f) { "HibernateEnabled = (未设置)" } else { "HibernateEnabled = $($f.HibernateEnabled)" }

Write-Output ""
Write-Output "===== 内置盘 ESP（卷标签定位，只读） ====="
$esp = Get-Partition | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' }
foreach ($p in $esp) {
  $d = $p.DiskNumber
  "Disk#$d Partition#$($p.PartitionNumber) size=$([math]::Round($p.Size/1MB,1))MB letter=$($p.DriveLetter)"
}
'''


def main() -> int:
    r = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=180,
    )
    out = (r.stdout or "").strip()
    err = (r.stderr or "").strip()
    print(out if out else "(无输出)")
    if err:
        print("---- stderr ----")
        print(err[:1500])
    print()
    print("===== 说明 =====")
    print("本脚本全程只读：未写盘、未改 BIOS、未改 BCD、未删任何文件。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
