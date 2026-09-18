# 部署产物独立核验 v2：按卷标签定位（不按分区序号臆测），assign 后回读标签确认落点。
# 背景：v1 按 P2=VARIX_SYS/P3=SHARED 臆测 + 撞上跨会话字母残留（Y: 残留绑定指向 ESP），
# 导致 VARIX_SYS 列成 ESP 内容、SHARED 列成 WIN_ENGINE。实际分区序：
#   P1=VARIX-ESP  P2=VARIX_SYS  P3=WIN_ENGINE  P4=SHARED  P5=SNAPSHOT
# 教训：diskpart assign 可能因 mount manager 残留绑定「成功」到别的卷上，
# 必须 assign 后用 Get-Volume -DriveLetter 回读 FileSystemLabel 确认（标签五卷唯一）。
$ErrorActionPreference = 'Continue'

'=== Partitions on Disk 1 ==='
Get-Partition -DiskNumber 1 | Sort-Object PartitionNumber |
  ForEach-Object { "P{0}  {1,7:N1}GB  {2}" -f $_.PartitionNumber, ($_.Size/1GB), $_.GptType }

'=== Current letters in session ==='
Get-Volume | Where-Object { $_.DriveLetter } |
  ForEach-Object { "{0}:  {1}" -f $_.DriveLetter, $_.FileSystemLabel }

# 历史用过的字母（跨会话残留绑定雷区，全部避开）：Z Y X W V U Q P
$candidates = @('T','S','R','O','N','M','L','K','J','I','H','G','F')

function Mount-ByLabel {
  param([string]$Label, [switch]$Esp)
  $vol = Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $vol) { return @{ Letter = $null; Error = "label not found: $Label" } }
  if ($vol.DriveLetter) { return @{ Letter = "$($vol.DriveLetter)"; Error = $null } }
  $used = @(Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object { $_.DriveLetter })
  $L = $null
  foreach ($c in $candidates) { if ($used -notcontains $c) { $L = $c; break } }
  if (-not $L) { return @{ Letter = $null; Error = 'no free letter' } }
  # 按卷 UniqueId 反查分区号（不臆测序号）
  $part = Get-Partition -DiskNumber 1 -ErrorAction SilentlyContinue |
    Where-Object { $_.UniqueId -and $vol.ObjectId -and ("{$($_.Guid)}" -eq $vol.ObjectId -or $_.AccessPaths -contains "\\?\Volume{$($_.Guid)}\") } |
    Select-Object -First 1
  # 上面的 GUID 匹配在 WMI 陈旧视图下可能落空，兜底直接按分区遍历 assign 到该卷所在分区：
  # 用 remove all dismount + assign，然后回读标签验证落点，落点不对立即报告。
  $pn = if ($part) { $part.PartitionNumber } else { 0 }
  if ($pn -eq 0) {
    # 兜底：按标签容量唯一匹配分区
    $all = @(Get-Partition -DiskNumber 1 | Sort-Object PartitionNumber)
    foreach ($p in $all) {
      $pv = Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue | Select-Object -First 1
      if ($pv -and [math]::Abs(($p.Size/1GB) - ($pv.Size/1GB)) -lt 0.2) { $pn = $p.PartitionNumber; break }
    }
  }
  if ($pn -eq 0) { return @{ Letter = $null; Error = "partition not resolved for $Label" } }
  $dp = "select disk 1`r`nselect partition $pn`r`nremove all dismount`r`nassign letter=$L`r`n"
  $dpFile = Join-Path $env:TEMP 'varix-verify2-assign.txt'
  [IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
  $out = diskpart /s $dpFile 2>&1 | Out-String
  "  [diskpart $Label -> $L`:] " + ($out.Trim() -replace "`r`n", ' | ')
  Start-Sleep -Seconds 2
  # 落点确认：回读标签，必须等于目标标签（防止残留绑定把字母解析到别的卷）
  $got = Get-Volume -DriveLetter $L -ErrorAction SilentlyContinue
  if ($got -and $got.FileSystemLabel -eq $Label) {
    return @{ Letter = "$L"; Error = $null }
  }
  $gotLbl = if ($got) { $got.FileSystemLabel } else { '<no volume>' }
  return @{ Letter = "$L"; Error = "MISMATCH: letter $L`: landed on label [$gotLbl], expected [$Label]" }
}

'=== Mount VARIX_SYS (P2) ==='
$sys = Mount-ByLabel 'VARIX_SYS'
"VARIX_SYS -> $($sys.Letter):  err=$($sys.Error)"
if ($sys.Letter) {
  '--- VARIX_SYS root ---'
  Get-ChildItem -LiteralPath "$($sys.Letter):\" -Force -ErrorAction SilentlyContinue |
    ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.Name }
}

'=== Mount SHARED (P4) ==='
$sh = Mount-ByLabel 'SHARED'
"SHARED -> $($sh.Letter):  err=$($sh.Error)"
if ($sh.Letter) {
  '--- SHARED tree (depth 2) ---'
  Get-ChildItem -LiteralPath "$($sh.Letter):\" -Recurse -Depth 2 -Force -ErrorAction SilentlyContinue |
    ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.FullName.Substring(3) }
  '--- boot-select.json ---'
  Get-Content -LiteralPath "$($sh.Letter):\boot-select.json" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
  '--- apps.json ---'
  Get-Content -LiteralPath "$($sh.Letter):\apps.json" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
  '--- whitelist.json ---'
  Get-Content -LiteralPath "$($sh.Letter):\whitelist\whitelist.json" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
  '--- handoff dir ---'
  if (Test-Path -LiteralPath "$($sh.Letter):\handoff") { 'handoff dir OK' } else { 'handoff dir MISSING' }
}

'=== Mount WIN_ENGINE (P3) ==='
$we = Mount-ByLabel 'WIN_ENGINE'
"WIN_ENGINE -> $($we.Letter):  err=$($we.Error)"
if ($we.Letter) {
  '--- WIN_ENGINE root ---'
  Get-ChildItem -LiteralPath "$($we.Letter):\" -Force -ErrorAction SilentlyContinue |
    ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.Name }
}

'=== Mount SNAPSHOT (P5) ==='
$sn = Mount-ByLabel 'SNAPSHOT'
"SNAPSHOT -> $($sn.Letter):  err=$($sn.Error)"
if ($sn.Letter) {
  '--- SNAPSHOT root ---'
  Get-ChildItem -LiteralPath "$($sn.Letter):\" -Force -ErrorAction SilentlyContinue |
    ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.Name }
}

'=== ESP limine.conf (UTF-8 recheck) ==='
$esp = Mount-ByLabel 'VARIX-ESP' -Esp
"VARIX-ESP -> $($esp.Letter):  err=$($esp.Error)"
if ($esp.Letter) {
  Get-Content -LiteralPath "$($esp.Letter):\limine.conf" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
}

'=== Summary ==='
"VARIX_SYS : letter=$($sys.Letter) err=$($sys.Error)"
"SHARED    : letter=$($sh.Letter) err=$($sh.Error)"
"WIN_ENGINE: letter=$($we.Letter) err=$($we.Error)"
"SNAPSHOT  : letter=$($sn.Letter) err=$($sn.Error)"
"VARIX-ESP : letter=$($esp.Letter) err=$($esp.Error)"

'=== VERIFY2 DONE ==='
