#!/usr/bin/env python3
"""诊断 uefi-esp.vhd 挂载：为什么 Mount-DiskImage 后拿不到盘符。"""
import subprocess

DISK = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\uefi-esp.vhd"

cmd = (
    "$ErrorActionPreference='Continue';"
    "$img = Mount-DiskImage -ImagePath '" + DISK + "' -PassThru;"
    "Write-Output ('Attached: ' + $img.Attached);"
    "$d = $img | Get-Disk;"
    "Write-Output ('Disk: ' + $d.Number + ' opstatus=' + $d.OperationalStatus + ' ro=' + $d.IsReadOnly);"
    "$p = $d | Get-Partition;"
    "Write-Output ('Partitions: ' + $p.Count);"
    "$p | ForEach-Object { Write-Output ('  pn=' + $_.PartitionNumber + ' letter=[' + $_.DriveLetter + '] gpt=' + $_.GptType) };"
    "$v = $d | Get-Partition | Get-Volume;"
    "Write-Output ('Volumes: ' + $v.Count);"
    "$v | ForEach-Object { Write-Output ('  label=' + $_.FileSystemLabel + ' letter=[' + $_.DriveLetter + '] fs=' + $_.FileSystem) }"
)

r = subprocess.run(["powershell", "-NoProfile", "-Command", cmd], capture_output=True, timeout=120)
print("rc =", r.returncode)
print("--- stdout ---")
print(r.stdout.decode("gbk", "replace"))
print("--- stderr ---")
print(r.stderr.decode("gbk", "replace"))
