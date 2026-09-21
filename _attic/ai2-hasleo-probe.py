# -*- coding: utf-8 -*-
"""AI-2 · Hasleo WinToUSB 就绪度只读探针（拍板①后 S1.2 前置核查）。

只读：查注册表卸载键 + 常见安装路径，判断 Hasleo WinToUSB 是否已安装；
顺带核对官方 Win11 ISO 是否在常见位置（不扫全盘）。零写入。
"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference = 'SilentlyContinue'
# 1) 注册表卸载键查 Hasleo
$found = $false
foreach ($root in @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
                    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
                    'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall')) {
  Get-ChildItem $root | ForEach-Object {
    $dn = (Get-ItemProperty $_.PSPath).DisplayName
    if ($dn -match 'Hasleo|WinToUSB') {
      $found = $true
      Write-Output ("HASLEO|registry|" + $dn + "|" + (Get-ItemProperty $_.PSPath).DisplayVersion)
    }
  }
}
# 2) 常见安装路径
foreach ($p in @('C:\Program Files\Hasleo', 'C:\Program Files (x86)\Hasleo', 'D:\Hasleo', 'D:\Program Files\Hasleo')) {
  if (Test-Path $p) { Write-Output ("HASLEO|path|" + $p) }
}
if (-not $found) { Write-Output 'HASLEO|NOT-INSTALLED' }
# 3) 常见位置找 Win11 ISO（不扫全盘，如实列出命中）
foreach ($d in @('C:\Users\varia\Downloads', 'D:\', 'D:\iso', 'D:\ISO', 'D:\Downloads')) {
  if (Test-Path $d) {
    Get-ChildItem -LiteralPath $d -Filter '*.iso' -ErrorAction SilentlyContinue | ForEach-Object {
      Write-Output ("ISO|" + $_.FullName + "|" + [math]::Round($_.Length/1GB,2) + "GB")
    }
  }
}
Write-Output 'PROBE-DONE'
"""

out = subprocess.run(
    ["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120,
)
sys.stdout.write(out.stdout)
if out.returncode != 0:
    sys.stderr.write(out.stderr)
    sys.exit(out.returncode)
