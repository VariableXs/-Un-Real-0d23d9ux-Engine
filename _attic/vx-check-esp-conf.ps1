# vx-check-esp-conf.ps1 — 只读校验 U 盘 ESP limine.conf 的真实字节（严格零写入）。
# 标记：ESP-CONF-CHECK-DONE
$ErrorActionPreference = 'Stop'
$log = '{log}'
$out = New-Object System.Collections.Generic.List[string]
function Say($m) { $script:out.Add($m) }

$dp = Join-Path $env:TEMP 'vx-esp-assign.txt'
[IO.File]::WriteAllText($dp, "select disk 1`r`nselect partition 1`r`nassign letter=L`r`n", [Text.Encoding]::ASCII)
diskpart /s $dp | Out-Null
try {
  if (-not (Test-Path 'L:\limine.conf')) { throw 'L:\limine.conf missing' }
  $bytes = [IO.File]::ReadAllBytes('L:\limine.conf')
  Say ('size=' + $bytes.Length)
  $sha = [System.Security.Cryptography.SHA256]::Create()
  Say ('sha256=' + ([BitConverter]::ToString($sha.ComputeHash($bytes)).Replace('-','')))
  # 首 4 字节 + 是否含 BOM
  Say ('first16hex=' + [BitConverter]::ToString($bytes[0..15]).Replace('-',''))
  # 用 UTF-8 严格解码（无效序列会变 U+FFFD）
  $text = [Text.Encoding]::UTF8.GetString($bytes)
  if ($text.Contains('�')) { Say 'utf8=INVALID (mojibake on disk)' } else { Say 'utf8=VALID' }
  Say ($text.Split("`n")[0])
} finally {
  [IO.File]::WriteAllText($dp, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
  diskpart /s $dp | Out-Null
}
$out | Out-File -FilePath ($log + '.rpt') -Encoding UTF8
Write-Output 'ESP-CONF-CHECK-DONE'
