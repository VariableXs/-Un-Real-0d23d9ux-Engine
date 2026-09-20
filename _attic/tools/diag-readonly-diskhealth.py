#!/usr/bin/env python3
"""只读磁盘健康体检（需求12：不给数据/硬件安全构成威胁 → 只读诊断先行）。

检查项：
  1. 所有物理磁盘 HealthStatus / OperationalStatus / BusType / 介质类型
  2. 所有卷的 HealthStatus + 文件系统 + 剩余空间（重点关注 V: SNAPSHOT）
  3. 上一次报告的 Warning 是否仍在

绝不写入、绝不修复、绝不删除。用法：python _attic/tools/diag-readonly-diskhealth.py
"""
import subprocess
import sys

PS = r'''
$ErrorActionPreference = 'SilentlyContinue'
Write-Output "===== PHYSICAL DISKS ====="
Get-PhysicalDisk | ForEach-Object {
  "{0,-28} | Health={1,-8} | Op={2,-20} | Bus={3,-6} | Media={4,-6} | Size={5}GB" -f `
    $_.FriendlyName, $_.HealthStatus, ($_.OperationalStatus -join ','), $_.BusType, $_.MediaType, `
    [math]::Round($_.Size/1GB,1)
}
Write-Output ""
Write-Output "===== VOLUMES ====="
Get-Volume | Where-Object { $_.DriveLetter } | Sort-Object DriveLetter | ForEach-Object {
  "{0}: | {1,-16} | FS={2,-6} | Health={3,-8} | Size={4}GB | Free={5}GB" -f `
    $_.DriveLetter, $_.FileSystemLabel, $_.FileSystem, $_.HealthStatus, `
    [math]::Round($_.Size/1GB,1), [math]::Round($_.SizeRemaining/1GB,1)
}
Write-Output ""
Write-Output "===== VIRTUAL DISKS (Storage Spaces / 其他) ====="
Get-VirtualDisk | ForEach-Object {
  "{0,-24} | Health={1,-8} | Op={2}" -f $_.FriendlyName, $_.HealthStatus, ($_.OperationalStatus -join ',')
}
Write-Output ""
Write-Output "===== 上次自检相关的 SMART/可靠性计数（仅读） ====="
Get-PhysicalDisk | ForEach-Object {
  $r = $_ | Get-StorageReliabilityCounter
  if ($r) {
    "{0,-28} | Temp={1}C | ReadErr={2} | WriteErr={3} | Wear={4} | PowerOnHrs={5}" -f `
      $_.FriendlyName, $r.Temperature, $r.ReadErrorsTotal, $r.WriteErrorsTotal, $r.Wear, $r.PowerOnHours
  }
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
        print(err[:2000])
    print("\n===== 结论（脚本不写盘、不修复） =====")
    if "Warning" in out or "Unhealthy" in out or "Degraded" in out:
        print("发现非 Healthy 状态项（见上）——仅报告，未做任何修复动作。")
    else:
        print("全部 Healthy，未发现告警。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
