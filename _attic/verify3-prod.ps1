# VARIX_SYS (P2) 终验：装入点方案（NTFS 卷接受 mount，免疫盘符残留绑定）。
# 前情：按标签 Get-Volume 能找到 VARIX_SYS 卷，但盘符分配两轮落空
#（T: <no volume>；Y: 残留绑定解析到 ESP）。装入点不占盘符命名空间。
$ErrorActionPreference = 'Continue'

'=== VARIX_SYS volume by label ==='
$vol = Get-Volume -FileSystemLabel 'VARIX_SYS' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $vol) { 'FATAL: no volume labeled VARIX_SYS'; '=== VERIFY3 DONE ==='; exit 0 }
"volume found: ObjectId=$($vol.ObjectId) Size=$([math]::Round($vol.Size/1GB,1))GB FS=$($vol.FileSystem)"

$mnt = 'C:\varix-mnt-sys'
if (-not (Test-Path -LiteralPath $mnt)) { New-Item -ItemType Directory -Force -Path $mnt | Out-Null }

'=== Mount via diskpart assign mount ==='
$dp = "select disk 1`r`nselect partition 2`r`nremove all dismount`r`nassign mount=$mnt`r`n"
$dpFile = Join-Path $env:TEMP 'varix-verify3-mount.txt'
[IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
$dpOut = diskpart /s $dpFile 2>&1 | Out-String
$dpOut.Trim()

Start-Sleep -Seconds 3

$ok = Test-Path -LiteralPath "$mnt\variable.exe"
if (-not $ok) {
  'mount path not reachable, fallback to letter S:'
  $dp2 = "select disk 1`r`nselect partition 2`r`nassign letter=S`r`n"
  $dpFile2 = Join-Path $env:TEMP 'varix-verify3-letter.txt'
  [IO.File]::WriteAllText($dpFile2, $dp2, [Text.Encoding]::ASCII)
  (diskpart /s $dpFile2 2>&1 | Out-String).Trim()
  Start-Sleep -Seconds 5
  $mnt = 'S:'
  $ok = Test-Path -LiteralPath "$mnt\variable.exe"
}

"variable.exe reachable: $ok"
if ($ok) {
  '--- VARIX_SYS root ---'
  Get-ChildItem -LiteralPath "$mnt\" -Force -ErrorAction SilentlyContinue |
    ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.Name }
  '--- variable.exe size ---'
  $f = Get-Item -LiteralPath "$mnt\variable.exe"
  "$($f.Length) bytes  mtime=$($f.LastWriteTime.ToString('o'))"
  '--- variable.exe sha256 (disk) ---'
  $h = (Get-FileHash -LiteralPath "$mnt\variable.exe" -Algorithm SHA256).Hash
  $h
  $local = 'F5B8DEE9853A1700907B92FE7BC06C02AB066427B81A0363555498DECB611360'
  if ($h -eq $local) { 'HASH MATCH: disk variable.exe == repo _attic/syssource/variable.exe' }
  else { "HASH MISMATCH: expected $local" }
}

'=== VERIFY3 DONE ==='
