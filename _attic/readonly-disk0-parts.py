#!/usr/bin/env python3
"""只读:Disk0(EFI/MSR/Windows分区)与 Disk1 的分区 GUID——Limine 链载 Windows 用。"""
import subprocess


def ps(cmd, timeout=90):
    r = subprocess.run(
        ["powershell", "-NoProfile", "-Command", cmd],
        capture_output=True, timeout=timeout,
    )
    return r.stdout.decode("gbk", "replace") + r.stderr.decode("gbk", "replace")


print("=== Disk 0(内置 NVMe)分区 ===")
print(ps(
    "Get-Disk -Number 0 | Get-Partition | Select-Object PartitionNumber,"
    "GptType,Guid,@{n='SizeGB';e={[math]::Round($_.Size/1GB,1)}} | "
    "Format-Table -AutoSize | Out-String"
))
print("=== Disk 0 各分区卷与标签 ===")
print(ps(
    "Get-Partition -DiskNumber 0 | ForEach-Object { "
    "$g = if ($_.DriveLetter) { Get-Volume -DriveLetter $_.DriveLetter -ErrorAction SilentlyContinue } else { $null }; "
    "('{0} letter={1} label={2} fs={3}' -f $_.PartitionNumber, $_.DriveLetter, $g.FileSystemLabel, $g.FileSystem) } | Out-String"
))
