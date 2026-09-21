# -*- coding: utf-8 -*-
"""ready 标记 edition 名修正重试（显式错误上报）。"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference='Stop'
$m = 'X:\varix-w1-ready.txt'
try {
  $j = Get-Content -LiteralPath $m -Raw | ConvertFrom-Json
  Write-Output ('BEFORE=' + $j.edition)
  $j.edition = 'Windows 11 专业版'
  $json = $j | ConvertTo-Json
  [IO.File]::WriteAllText($m, $json, [Text.UTF8Encoding]::new($false))
  $j2 = Get-Content -LiteralPath $m -Raw | ConvertFrom-Json
  Write-Output ('AFTER=' + $j2.edition)
  if ($j2.edition -eq 'Windows 11 专业版') { Write-Output 'FIX=OK' } else { Write-Output 'FIX=READBACK-MISMATCH' }
} catch {
  Write-Output ('FIX-ERROR=' + $_.Exception.Message)
}
"""
out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-200:])
