# -*- coding: utf-8 -*-
"""disk1 分区表核查：SHARED 分区是否在、有无盘符。"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference='SilentlyContinue'
Get-Partition -DiskNumber 1 | ForEach-Object {
  Write-Output ('PART|' + $_.PartitionNumber + '|L=' + $_.DriveLetter + '|size=' + [math]::Round($_.Size/1GB,1) + 'GB|type=' + $_.GptType)
}
Get-Volume | Where-Object { -not $_.DriveLetter } | ForEach-Object {
  Write-Output ('NOLETTER_VOL|' + $_.FileSystemLabel + '|' + $_.FileSystem + '|' + [math]::Round($_.Size/1GB,1) + 'GB')
}
"""
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-200:])
