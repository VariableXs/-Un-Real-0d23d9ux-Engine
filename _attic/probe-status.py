#!/usr/bin/env python3
"""只读探针：查看 Disk 1 当前进度（分区数/卷/盘符），判断 Partition 阶段卡点。"""
import subprocess

PS = r"""
$d = Get-Disk -Number 1 -ErrorAction SilentlyContinue
if ($d) { "Disk1: Offline={0} Style={1} Parts={2}" -f $d.IsOffline, $d.PartitionStyle, @($d | Get-Partition -ErrorAction SilentlyContinue).Count }
Get-Partition -DiskNumber 1 -ErrorAction SilentlyContinue | Sort-Object PartitionNumber |
  ForEach-Object { "  P{0} {1} {2:N1}GB letter={3}" -f $_.PartitionNumber, $_.GptType, ($_.Size/1GB), $_.DriveLetter }
Get-Volume | Where-Object { $_.DriveLetter -and $_.DriveLetter -ge 'F' } |
  ForEach-Object { "  Vol {0}: {1} {2} {3:N1}GB" -f $_.DriveLetter, $_.FileSystemLabel, $_.FileSystem, ($_.Size/1GB) }
'--- deploy log size ---'
(Get-Item 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\deploy-full.log' -ErrorAction SilentlyContinue).Length
'--- powershell processes ---'
Get-Process powershell -ErrorAction SilentlyContinue | Select-Object Id,CPU,@{n='MB';e={[int]($_.WorkingSet64/1MB)}} | Format-Table -AutoSize | Out-String
"""

r = subprocess.run(["powershell.exe", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
err = r.stderr.decode("gbk", "replace")
if err.strip():
    print("[stderr]", err)
