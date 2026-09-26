#!/usr/bin/env python3
r"""U 盘（Disk1）分区布局只读探查 —— 定位 ESP 与 efibootmgr 目标。

只读：Get-Disk/Get-Partition/Get-Volume/Test-Path。不写入、不 assign 盘符。
"""
import subprocess


def ps(cmd: str, timeout: int = 120) -> str:
    r = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", cmd],
        capture_output=True, timeout=timeout,
    )
    out = r.stdout.decode("gbk", "replace") + r.stderr.decode("gbk", "replace")
    return out.strip() or "(空输出)"


print("=== Disk1 分区 ===")
print(ps(
    "Get-Disk 1 | Get-Partition | Select-Object PartitionNumber,DriveLetter,"
    "@{n='SizeMB';e={[math]::Round($_.Size/1MB,1)}},Type,GptType,IsBoot,IsSystem | "
    "Format-Table -AutoSize | Out-String"
))

print("=== 各分区详情 ===")
print(ps(
    "Get-Disk 1 | Get-Partition | ForEach-Object { "
    "  $p = $_; "
    "  if ($p.DriveLetter) { "
    "    $v = Get-Volume -DriveLetter $p.DriveLetter -ErrorAction SilentlyContinue; "
    "    Write-Output ('P' + $p.PartitionNumber + '  ' + $p.DriveLetter + ':  label=' + $v.FileSystemLabel + '  fs=' + $v.FileSystem) "
    "  } else { "
    "    Write-Output ('P' + $p.PartitionNumber + '  (no letter)  GptType=' + $p.GptType + '  SizeMB=' + [math]::Round($p.Size/1MB,1)) "
    "  } }"
))

print("=== 可能的 ESP（按 GptType 找 EFI 系统分区）===")
print(ps(
    "Get-Partition | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | "
    "ForEach-Object { Write-Output ('Disk' + $_.DiskNumber + ' P' + $_.PartitionNumber + "
    "'  letter=' + $_.DriveLetter + '  SizeMB=' + [math]::Round($_.Size/1MB,1)) }"
))

print("=== Disk0 各分区标签（内置盘，判断哪块是 Windows）===")
print(ps(
    "Get-Disk 0 | Get-Partition | ForEach-Object { "
    "  $p = $_; "
    "  if ($p.DriveLetter) { "
    "    $v = Get-Volume -DriveLetter $p.DriveLetter -ErrorAction SilentlyContinue; "
    "    Write-Output ('P' + $p.PartitionNumber + '  ' + $p.DriveLetter + ':  label=' + $v.FileSystemLabel) "
    "  } else { "
    "    Write-Output ('P' + $p.PartitionNumber + '  (no letter)  GptType=' + $p.GptType + '  SizeMB=' + [math]::Round($p.Size/1MB,1)) "
    "  } }"
))
