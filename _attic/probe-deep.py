#!/usr/bin/env python3
"""深度快照：分区明细 + 活跃 powershell/python 进程命令行 + 日志指纹。"""
import subprocess

PS = r"""
'=== Partitions full detail ==='
Get-Partition -DiskNumber 1 -ErrorAction SilentlyContinue | Sort-Object PartitionNumber |
  ForEach-Object { "P{0} Off={1} Size={2:N2}GB Type={3} Letter={4} Sys={5}" -f $_.PartitionNumber, $_.Offset, ($_.Size/1GB), $_.GptType, $_.DriveLetter, $_.IsSystem }
'=== Disk ==='
Get-Disk -Number 1 | ForEach-Object { "Style={0} Offline={1} Parts={2}" -f $_.PartitionStyle, $_.IsOffline, $_.NumberOfPartitions }
'=== Active PS processes cmdline ==='
Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" |
  ForEach-Object { "PID={0} {1}" -f $_.ProcessId, $_.CommandLine.Substring(0, [Math]::Min(160, $_.CommandLine.Length)) }
'=== python processes ==='
Get-CimInstance Win32_Process -Filter "Name='python.exe'" |
  ForEach-Object { "PID={0} {1}" -f $_.ProcessId, $_.CommandLine.Substring(0, [Math]::Min(120, $_.CommandLine.Length)) }
"""

r = subprocess.run(["powershell.exe", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
err = r.stderr.decode("gbk", "replace")
if err.strip():
    print("[stderr]", err)
