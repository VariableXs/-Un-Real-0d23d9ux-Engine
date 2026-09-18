#!/usr/bin/env python3
"""诊断 1：主板/BIOS/固件类型 + 磁盘列表（无提权）。"""
import subprocess

PS = r"""
$cs = Get-WmiObject Win32_ComputerSystem
$bb = Get-WmiObject Win32_BaseBoard
$bios = Get-WmiObject Win32_BIOS
Write-Output ("PC: " + $cs.Manufacturer + ' ' + $cs.Model)
Write-Output ("Board: " + $bb.Manufacturer + ' ' + $bb.Product)
Write-Output ("BIOS: " + $bios.Manufacturer + ' ' + $bios.SMBIOSBIOSVersion + ' ' + $bios.ReleaseDate)
Write-Output ("FirmwareType: " + $env:firmware_type)
Get-Disk | Select-Object Number,FriendlyName,BusType,Size,IsBoot,IsSystem,OperationalStatus |
  Format-Table -AutoSize | Out-String -Width 220
"""

r = subprocess.run(["powershell", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
print(r.stderr.decode("gbk", "replace"))
