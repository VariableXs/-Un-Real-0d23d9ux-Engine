# -*- coding: utf-8 -*-
"""占用者排查：wallpaper/steam/icue 进程 + CUESDK 独占打开测试。"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference='SilentlyContinue'
Get-Process | Where-Object { $_.Name -match 'wallpaper|steam|icue|cue' } | ForEach-Object {
  Write-Output ('PROC|' + $_.Name + '|' + $_.Id)
}
try {
  $fs = [IO.File]::Open('D:\steam\steamapps\common\wallpaper_engine\x64\CUESDK.x64_2017.dll', 'Open', 'ReadWrite', 'None')
  $fs.Close()
  Write-Output 'LOCKTEST=UNLOCKED'
} catch { Write-Output ('LOCKTEST=LOCKED: ' + $_.Exception.Message) }
# 残留概览
$s = Get-ChildItem 'D:\steam\steamapps\common\wallpaper_engine' -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum
Write-Output ('SRC_LEFT=' + $s.Count + 'f/' + $s.Sum + 'b')
$ws = Get-ChildItem 'D:\steam\steamapps\workshop\content\431960' -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum
Write-Output ('WS_LEFT=' + $ws.Count + 'f/' + $ws.Sum + 'b')
$d = Get-ChildItem 'W:\SteamLibrary\steamapps\common\wallpaper_engine' -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum
Write-Output ('DST_APP=' + $d.Count + 'f/' + $d.Sum + 'b')
$dw = Get-ChildItem 'W:\SteamLibrary\steamapps\workshop\content\431960' -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum
Write-Output ('DST_WS=' + $dw.Count + 'f/' + $dw.Sum + 'b')
$acf = Test-Path 'D:\steam\steamapps\appmanifest_431960.acf'
Write-Output ('SRC_ACF=' + $acf)
"""
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-200:])
