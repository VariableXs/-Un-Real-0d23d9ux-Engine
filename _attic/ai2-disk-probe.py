# -*- coding: utf-8 -*-
"""AI-2 只读探针：磁盘清单 / Hyper-V 模块 / 提权状态。
仅只读查询（Win32_DiskDrive / Get-Module / WindowsPrincipal），不写任何盘。"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference='SilentlyContinue'
$c = Get-CimInstance Win32_DiskDrive
Write-Output ("DiskCount=" + @($c).Count)
foreach ($d in $c) { Write-Output ("DISK|" + $d.Index + "|" + $d.Model + "|" + $d.InterfaceType + "|" + [math]::Round($d.Size/1GB,1) + "GB") }
$p = Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3 OR DriveType=2'
foreach ($v in $p) { Write-Output ("VOL|" + $v.DeviceID + "|" + $v.VolumeName + "|" + $v.FileSystem + "|" + [math]::Round($v.Size/1GB,1) + "GB") }
$m = Get-Module -ListAvailable -Name Hyper-V
Write-Output ("HyperVModule=" + ($null -ne $m))
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$pr = New-Object Security.Principal.WindowsPrincipal($id)
Write-Output ("Elevated=" + $pr.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))
Write-Output ("Host=" + $env:COMPUTERNAME)
"""

out = subprocess.run(
    ["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120,
)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write("[stderr] " + out.stderr)
sys.exit(out.returncode or 0)
