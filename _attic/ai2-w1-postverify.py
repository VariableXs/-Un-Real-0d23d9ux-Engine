# -*- coding: utf-8 -*-
"""AI-2 · W1 完工核验（只读）：X: 关键文件 + 卷状态 + 修正 ready 标记的 edition 名。"""
import json
import subprocess
import sys

PS = r"""
$ErrorActionPreference='SilentlyContinue'
$L='X:'
Write-Output ('LABEL=' + (Get-Volume -DriveLetter X).FileSystemLabel)
Write-Output ('SYSTEM32=' + (Test-Path -LiteralPath "$($L)\Windows\System32"))
Write-Output ('NTOSKRNL=' + (Test-Path -LiteralPath "$($L)\Windows\System32\ntoskrnl.exe"))
Write-Output ('EXPLORER=' + (Test-Path -LiteralPath "$($L)\Windows\explorer.exe"))
Write-Output ('REAGENTC=' + (Test-Path -LiteralPath "$($L)\Windows\System32\RecoveryAgent.xml"))
$ld = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='X:'"
Write-Output ('USED_GB=' + [math]::Round(($ld.Size-$ld.FreeSpace)/1GB,2))
Write-Output ('FREE_GB=' + [math]::Round($ld.FreeSpace/1GB,1))
Write-Output ('READY_MARKER=' + (Test-Path -LiteralPath "$($L)\varix-w1-ready.txt"))
# 修正 ready 标记的 edition 名（index=4 实际=专业版；原标记记录了残留变量名）
$m = "$($L)\varix-w1-ready.txt"
if (Test-Path -LiteralPath $m) {
  $j = Get-Content -LiteralPath $m -Raw | ConvertFrom-Json
  if ($j.edition -ne 'Windows 11 专业版') {
    $j.edition = 'Windows 11 专业版'
    [IO.File]::WriteAllText($m, ($j | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
    Write-Output ('MARKER_FIXED=True')
  } else { Write-Output 'MARKER_FIXED=ALREADY-OK' }
  Write-Output ('MARKER_CONTENT=' + (Get-Content -LiteralPath $m -Raw -ErrorAction SilentlyContinue))
}
"""
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-300:])
