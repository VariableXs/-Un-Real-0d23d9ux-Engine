#!/usr/bin/env python3
"""部署后独立核验：五分区结构 / ESP 双链 / VARIX_SYS 内容 / SHARED 契约 / 容量表。"""
import subprocess

PS = r"""
'=== Partitions on Disk 1 ==='
Get-Partition -DiskNumber 1 | Sort-Object PartitionNumber |
  Select-Object PartitionNumber,DriveLetter,@{n='GB';e={[math]::Round($_.Size/1GB,1)}},GptType,@{n='OffMiB';e={[int]($_.Offset/1MB)}} |
  Format-Table -AutoSize | Out-String -Width 200

'=== Volumes by label ==='
foreach ($lbl in @('VARIX-ESP','VARIX_SYS','WIN_ENGINE','SHARED','SNAPSHOT')) {
  $v = Get-Volume -FileSystemLabel $lbl -ErrorAction SilentlyContinue
  if ($v) { "{0,-12} {1}:  {2,8:N1} GB total  {3,8:N1} GB free  {4}" -f $lbl, $v.DriveLetter, ($v.Size/1GB), ($v.SizeRemaining/1GB), $v.FileSystem }
  else { "$lbl  MISSING" }
}

$esp = (Get-Volume -FileSystemLabel 'VARIX-ESP').DriveLetter
'=== ESP tree (recursive) ==='
Get-ChildItem -LiteralPath "${esp}:\" -Recurse -File -ErrorAction SilentlyContinue |
  ForEach-Object { "{0,12}  {1}" -f $_.Length, $_.FullName.Substring(3) }

$sys = (Get-Volume -FileSystemLabel 'VARIX_SYS').DriveLetter
'=== VARIX_SYS root ==='
Get-ChildItem -LiteralPath "${sys}:\" -Force -ErrorAction SilentlyContinue |
  ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.Name }

$sh = (Get-Volume -FileSystemLabel 'SHARED').DriveLetter
'=== SHARED tree (2 levels) ==='
Get-ChildItem -LiteralPath "${sh}:\" -Recurse -Depth 1 -Force -ErrorAction SilentlyContinue |
  ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.FullName.Substring(3) }
"""

r = subprocess.run(["powershell.exe", "-NoProfile", "-Command", PS], capture_output=True)
print(r.stdout.decode("gbk", "replace"))
err = r.stderr.decode("gbk", "replace")
if err.strip():
    print("[stderr]", err)
