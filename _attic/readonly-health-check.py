#!/usr/bin/env python3
r"""只读体检：Windows 引导完整性 + U 盘现状。不删、不改、不写任何东西。
全部命令均为只读查询（Test-Path/Get-* /列目录）。"""
import subprocess


def ps(cmd, timeout=90):
    r = subprocess.run(
        ["powershell", "-NoProfile", "-Command", cmd],
        capture_output=True, timeout=timeout,
    )
    return r.stdout.decode("gbk", "replace") + r.stderr.decode("gbk", "replace")


print("=== 1) Windows 自身引导文件（硬盘上的备份副本，非提权可读） ===")
print(ps(
    "$p1='C:\\Windows\\Boot\\EFI\\bootmgfw.efi';"
    "$p2='C:\\Windows\\Boot\\EFI\\bootmgr.efi';"
    "$p3='C:\\Windows\\System32\\winload.exe';"
    "$p4='C:\\Windows\\System32\\winresume.exe';"
    "foreach ($p in @($p1,$p2,$p3,$p4)) { "
    "  if (Test-Path $p) { $i=Get-Item $p; Write-Output ('OK  ' + $i.Length + '  ' + $p) } "
    "  else { Write-Output ('MISSING  ' + $p) } }"
))

print("=== 2) 磁盘与卷 ===")
print(ps(
    "Get-Disk | Select-Object Number,FriendlyName,BusType,PartitionStyle,"
    "@{n='SizeGB';e={[math]::Round($_.Size/1GB,1)}} | Format-Table -AutoSize | Out-String"
))

print("=== 3) C 盘 BitLocker 与系统状态 ===")
print(ps(
    "Get-BitLockerVolume -MountPoint $env:SystemDrive 2>$null | "
    "Select-Object MountPoint,VolumeStatus,ProtectionStatus,EncryptionPercentage | Format-List | Out-String"
))

print("=== 4) U 盘分区布局（Disk 1，只读） ===")
print(ps(
    "Get-Disk -Number 1 2>$null | Get-Partition | "
    "Select-Object PartitionNumber,DriveLetter,GptType,"
    "@{n='SizeGB';e={[math]::Round($_.Size/1GB,2)}} | Format-Table -AutoSize | Out-String"
))
