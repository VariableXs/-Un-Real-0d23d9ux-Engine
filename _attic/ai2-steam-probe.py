# -*- coding: utf-8 -*-
"""AI-2 · Steam/WE 位置探针（只读）：SteamPath、库列表、WE app 与 workshop 内容定位及体量。"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference='SilentlyContinue'
$sp = (Get-ItemProperty 'HKCU:\Software\Valve\Steam').SteamPath
Write-Output ('STEAMPATH=' + $sp)
$lf = Join-Path $sp 'config\libraryfolders.vdf'
if (Test-Path $lf) {
  $raw = Get-Content -LiteralPath $lf -Raw
  $paths = [regex]::Matches($raw, '"path"\s+"([^"]+)"') | ForEach-Object { $_.Groups[1].Value }
  foreach ($p in $paths) {
    $p2 = $p -replace '\\\\', '\'
    Write-Output ('LIB=' + $p2)
    $we = Join-Path $p2 'steamapps\common\wallpaper_engine'
    $ws = Join-Path $p2 'steamapps\workshop\content\431960'
    $acf = Join-Path $p2 'steamapps\appmanifest_431960.acf'
    if (Test-Path $we) { $s = (Get-ChildItem $we -Recurse -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum; Write-Output ('  WE_APP=' + $we + ' ' + [math]::Round($s/1GB,2) + 'GB') }
    if (Test-Path $ws) { $s2 = (Get-ChildItem $ws -Recurse -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum; Write-Output ('  WE_WORKSHOP=' + $ws + ' ' + [math]::Round($s2/1GB,2) + 'GB') }
    if (Test-Path $acf) { Write-Output ('  ACF=' + $acf) }
  }
}
$steam = Get-Process steam -ErrorAction SilentlyContinue
Write-Output ('STEAM_RUNNING=' + ($null -ne $steam))
Write-Output 'PROBE-DONE'
"""
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-300:])
