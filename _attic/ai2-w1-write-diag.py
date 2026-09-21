# -*- coding: utf-8 -*-
"""X: 写能力诊断：卷级只读？文件级 ACL？"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference='Continue'
Write-Output ('VOL_READONLY_FLAG:')
$v = Get-Volume -DriveLetter X
Write-Output ($v | Format-List DriveLetter,FileSystemLabel,OperationalStatus,HealthStatus,Size,SizeRemaining | Out-String)
# 卷级写测试
try { [IO.File]::WriteAllText('X:\__wtest.tmp','t'); Write-Output 'VOL_WRITE=OK'; Remove-Item 'X:\__wtest.tmp' -Force }
catch { Write-Output ('VOL_WRITE=DENIED: ' + $_.Exception.Message) }
# 文件属性
$f = 'X:\varix-w1-ready.txt'
Write-Output ('ATTRIB=' + (Get-ItemProperty -LiteralPath $f -Name IsReadOnly -ErrorAction SilentlyContinue).IsReadOnly)
& attrib.exe "$f"
# 文件级写测试
try { [IO.File]::WriteAllText($f,'x'); Write-Output 'FILE_WRITE=OK' }
catch { Write-Output ('FILE_WRITE=DENIED: ' + $_.Exception.Message) }
# ACL
(Get-Acl -LiteralPath $f).AccessToString
"""
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-200:])
