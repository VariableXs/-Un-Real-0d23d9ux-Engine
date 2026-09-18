#!/usr/bin/env python3
"""非提权探查：M: 卷现状 + 磁盘1分区表（修复 fix3 校验失败定性）。"""
import subprocess

PS = r"""
Write-Output "=== Get-Volume M ==="
$v = Get-Volume -DriveLetter M -ErrorAction SilentlyContinue
if ($v) { $v | Format-List DriveLetter, FileSystemLabel, FileSystem, Size, SizeRemaining | Out-String | Write-Output }
else { Write-Output "<M: no volume>" }

Write-Output "=== M: content ==="
Write-Output ("root exists: " + (Test-Path 'M:\'))
Write-Output ("BOOTX64 exists: " + (Test-Path 'M:\EFI\BOOT\BOOTX64.EFI'))
Write-Output ("limine.sys exists: " + (Test-Path 'M:\limine-bios.sys'))
Get-ChildItem 'M:\' -Force -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_.Name) }

Write-Output "=== QueryDosDevice M ==="
Add-Type -Namespace W32 -Name QDD -MemberDefinition '[DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern uint QueryDosDevice(string lpDeviceName, System.Text.StringBuilder lpTargetPath, int ucchMax);'
$sb = New-Object System.Text.StringBuilder 1024
$null = [W32.QDD]::QueryDosDevice('M:', $sb, $sb.Capacity)
Write-Output ("M: -> " + $sb.ToString())

Write-Output "=== Partitions on Disk 1 ==="
Get-Partition -DiskNumber 1 -ErrorAction SilentlyContinue | ForEach-Object {
  Write-Output ("  P" + $_.PartitionNumber + " letter=[" + $_.DriveLetter + "] sizeMB=" + [math]::Round($_.Size/1MB) + " gpt=" + $_.GptType)
}
"""

r = subprocess.run(["powershell", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
err = r.stderr.decode("gbk", "replace")
if err.strip():
    print("STDERR:", err)
