# -*- coding: utf-8 -*-
"""部署进程死亡取证：应用事件日志 + WER 报告 + Defender 检测记录（全只读）。"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference='SilentlyContinue'
Write-Output '--- APP LOG: powershell/dism errors (last 2h) ---'
Get-WinEvent -FilterHashtable @{LogName='Application'; Level=1,2,3; StartTime=(Get-Date).AddHours(-2)} |
  Where-Object { $_.ProviderName -match 'Application Error|Windows Error|.NET|Windows PowerShell' -or $_.Message -match 'powershell|dism' } |
  Select-Object -First 8 | ForEach-Object {
    Write-Output ('EVT|' + $_.TimeCreated.ToString('HH:mm:ss') + '|' + $_.ProviderName + '|' + ($_.Message -replace "`r`n", ' ' -replace '  +', ' ').Substring(0, [Math]::Min(220, $_.Message.Length)))
  }
Write-Output '--- SYS LOG: volmgr/ntfs/disk warnings (last 2h) ---'
Get-WinEvent -FilterHashtable @{LogName='System'; Level=1,2,3; StartTime=(Get-Date).AddHours(-2)} |
  Where-Object { $_.ProviderName -match 'volmgr|Ntfs|disk|iaStor|stornvme' } |
  Select-Object -First 6 | ForEach-Object {
    Write-Output ('SYS|' + $_.TimeCreated.ToString('HH:mm:ss') + '|' + $_.ProviderName + '|' + ($_.Message -replace "`r`n", ' ' -replace '  +', ' ').Substring(0, [Math]::Min(180, $_.Message.Length)))
  }
Write-Output '--- WER recent reports ---'
Get-ChildItem 'C:\ProgramData\Microsoft\Windows\WER\ReportArchive','C:\ProgramData\Microsoft\Windows\WER\ReportQueue' -ErrorAction SilentlyContinue |
  Where-Object { $_.LastWriteTime -gt (Get-Date).AddHours(-2) } |
  Select-Object -First 8 | ForEach-Object { Write-Output ('WER|' + $_.LastWriteTime.ToString('HH:mm:ss') + '|' + $_.Name) }
Write-Output '--- Defender detections (last 2h) ---'
Get-MpThreatDetection -ErrorAction SilentlyContinue | Where-Object { $_.InitialDetectionTime -gt (Get-Date).AddHours(-2) } |
  Select-Object -First 5 | ForEach-Object { Write-Output ('DEF|' + $_.InitialDetectionTime + '|' + $_.ProcessName + '|' + $_.Resources -replace ' +', ' ') }
Write-Output '--- probe done ---'
"""
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=180)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-400:])
