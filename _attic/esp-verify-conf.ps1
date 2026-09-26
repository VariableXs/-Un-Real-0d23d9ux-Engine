# 只读核验 U 盘 ESP limine.conf：与 repo 根逐字节比对 + FNV-1a 正文 hash。
# 绝不写 ESP；结束时移除盘符。
$ErrorActionPreference = 'Stop'
try {
  $dp = Join-Path $env:TEMP 'varix-verify-assign.txt'
  [IO.File]::WriteAllText($dp, "select disk 1`r`nselect partition 1`r`nassign letter=Y`r`n", [Text.Encoding]::ASCII)
  diskpart /s $dp | Out-Null

  if (-not (Test-Path 'Y:\limine.conf')) { throw 'Y: 无 limine.conf' }
  $repo = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\limine.conf'

  $espBytes = [IO.File]::ReadAllBytes('Y:\limine.conf')
  $repoBytes = [IO.File]::ReadAllBytes($repo)
  $espSha = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($espBytes)).Replace('-','')
  $repoSha = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($repoBytes)).Replace('-','')
  Write-Output ("esp  sha256=" + $espSha)
  Write-Output ("repo sha256=" + $repoSha)
  if ($espSha -ne $repoSha) { throw 'limine.conf ESP 与 repo 不一致' }
  Write-Output 'conf byte-identical: OK'

  # FNV-1a 32 正文 hash（口径：首个非注释非空行起至 EOF；与 bootchain::hash_bytes 同源）。
  # 注意：PS 算术会提升为 int32/int64——用 int64 承载、每步 -band 0xFFFFFFFF 收敛，
  # 最后再转 uint32 输出，避免“值对于 UInt32 太大”的溢出异常。
  function Fnvl([byte[]]$b) {
    [long]$h = 0x811c9dc5
    [long]$prime = 0x01000193
    [long]$mask = [Convert]::ToInt64('FFFFFFFF', 16)   # 显式 int64 掩码（PS5.1 里 0xFFFFFFFF 字面量是 int32 -1，-band 会变空操作）
    for ($i = 0; $i -lt $b.Length; $i++) {
      $h = ($h -bxor [long]$b[$i]) -band $mask
      $h = ($h * $prime) -band $mask
    }
    return [uint32]($h -band $mask)
  }
  # 找正文起点
  $off = 0
  $lines = New-Object System.Collections.Generic.List[object]
  $start = 0
  for ($i = 0; $i -lt $espBytes.Length; $i++) {
    if ($espBytes[$i] -eq 0x0A) {
      $lines.Add(@{ s = $start; e = $i + 1 }); $start = $i + 1
    }
  }
  if ($start -lt $espBytes.Length) { $lines.Add(@{ s = $start; e = $espBytes.Length }) }
  foreach ($l in $lines) {
    $len = $l.e - $l.s
    $seg = New-Object byte[] $len
    [Array]::Copy($espBytes, $l.s, $seg, 0, $len)
    $trimmed = ($seg | ForEach-Object { $_ }) -as [byte[]]
    # strip ASCII whitespace both ends
    $a = 0; $b2 = $len - 1
    while ($a -le $b2 -and ($seg[$a] -eq 0x20 -or $seg[$a] -eq 0x09 -or $seg[$a] -eq 0x0D -or $seg[$a] -eq 0x0A)) { $a++ }
    while ($b2 -ge $a -and ($seg[$b2] -eq 0x20 -or $seg[$b2] -eq 0x09 -or $seg[$b2] -eq 0x0D -or $seg[$b2] -eq 0x0A)) { $b2-- }
    if ($a -gt $b2) { $off = $l.e; continue }
    if ($seg[$a] -eq 0x23) { $off = $l.e; continue }  # '#'
    $off = $l.s; break
  }
  $body = New-Object byte[] ($espBytes.Length - $off)
  [Array]::Copy($espBytes, $off, $body, 0, $body.Length)
  $h = Fnvl $body
  Write-Output ("body fnv1a32 = " + $h.ToString('x8'))
  $declared = (Get-Content 'Y:\limine.conf' -Encoding UTF8 | Where-Object { $_ -like '# hash: *' } | Select-Object -First 1) -replace '^# hash: ',''
  Write-Output ("declared    = " + $declared)
  if ($declared -ne $h.ToString('x8')) { throw 'hash 注释与正文不一致' }
  Write-Output 'conf body hash consistent: OK'
  Write-Output 'VERIFY-CONF-DONE'
} catch {
  Write-Output ("VERIFY-CONF-FAIL: " + $_)
} finally {
  $rm = Join-Path $env:TEMP 'varix-verify-remove.txt'
  [IO.File]::WriteAllText($rm, "select disk 1`r`nselect partition 1`r`nremove letter=Y`r`n", [Text.Encoding]::ASCII)
  diskpart /s $rm | Out-Null
}
