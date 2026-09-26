$ErrorActionPreference = 'Continue'
$iso = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\varix-qemu.iso'
$repoCfg = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\boot-select.json'
$distExe = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\dist-portable\Variable.exe'

# ========== 1) ESP 全量对账 vs ISO ==========
$m = Mount-DiskImage -ImagePath $iso -PassThru
$iv = ($m | Get-Volume).DriveLetter
'ISO mounted ' + $iv + ':'
$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1
if (-not $d) { 'NO-USB-DISK'; Dismount-DiskImage -ImagePath $iso | Out-Null; exit 1 }
$p = Get-Partition -DiskNumber $d.Number | Where-Object GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' | Select-Object -First 1
if ($p.AccessPaths -notcontains 'Y:\') {
  Add-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath 'Y:\' | Out-Null
  'ESP mounted Y:'
}
$srcRoot = $iv + ':'
$dstRoot = 'Y:'
$files = Get-ChildItem -Path ($srcRoot + '\') -Recurse -File
$matchCount = 0; $diffCount = 0; $missCount = 0
foreach ($f in $files) {
  $rel = $f.FullName.Substring($f.FullName.IndexOf(':') + 1)
  $dst = $dstRoot + $rel
  if (-not (Test-Path $dst)) {
    'MISSING-ON-ESP  ' + $rel
    $missCount++
  } else {
    $h1 = (Get-FileHash -Algorithm SHA256 $f.FullName).Hash
    $h2 = (Get-FileHash -Algorithm SHA256 $dst).Hash
    if ($h1 -eq $h2) { $matchCount++ }
    else {
      $m1 = $f.LastWriteTime.ToString('MM-dd HH:mm:ss')
      $m2 = (Get-Item $dst).LastWriteTime.ToString('MM-dd HH:mm:ss')
      'DIFF  ' + $rel + '  iso=' + $m1 + ' esp=' + $m2
      $diffCount++
    }
  }
}
'SUMMARY match=' + $matchCount + ' diff=' + $diffCount + ' missing=' + $missCount + ' total=' + $files.Count
$espFiles = Get-ChildItem -Path ($dstRoot + '\') -Recurse -File
$onlyCount = 0
foreach ($e in $espFiles) {
  $rel = $e.FullName.Substring($e.FullName.IndexOf(':') + 1)
  if (-not (Test-Path ($srcRoot + $rel))) {
    'ESP-ONLY  ' + $rel + '  mtime=' + $e.LastWriteTime.ToString('MM-dd HH:mm:ss')
    $onlyCount++
  }
}
'ESP-ONLY count=' + $onlyCount
if ($p.AccessPaths -contains 'Y:\') {
  Remove-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath 'Y:\' | Out-Null
}
Dismount-DiskImage -ImagePath $iso | Out-Null
'ESP-AUDIT-CLEANED'

# ========== 2) U 盘 Win11 分区里的 Variable.exe ==========
$vxe = 'X:\Variable\Variable.exe'
if (Test-Path $vxe) {
  'X:\Variable\Variable.exe mtime=' + (Get-Item $vxe).LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
  'X exe sha256=' + (Get-FileHash $vxe -Algorithm SHA256).Hash
} else {
  'X:\Variable\Variable.exe MISSING'
}
if (Test-Path $distExe) {
  'dist    sha256=' + (Get-FileHash $distExe -Algorithm SHA256).Hash
  'dist    mtime=' + (Get-Item $distExe).LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
}

# ========== 3) SHARED boot-select.json vs repo 种子 ==========
$sh = Get-Partition -DiskNumber $d.Number | Where-Object { $_.GptType -eq '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}' } | ForEach-Object {
  $lp = 'S:'
  if ($_.AccessPaths -notcontains 'S:\') {
    Add-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $_.PartitionNumber -AccessPath 'S:\' | Out-Null
  }
  if (Test-Path 'S:\boot-select.json') { $_.PartitionNumber }
  if ($_.AccessPaths -contains 'S:\') {
    Remove-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $_.PartitionNumber -AccessPath 'S:\' | Out-Null
  }
} | Select-Object -First 1
if ($sh) {
  'SHARED part#' + $sh + ' has boot-select.json (re-mounting for content)'
  Add-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $sh -AccessPath 'S:\' | Out-Null
  $shCfg = Get-Item 'S:\boot-select.json'
  'SHARED cfg mtime=' + $shCfg.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
  'SHARED cfg content:'
  Get-Content 'S:\boot-select.json' -Raw
  Remove-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $sh -AccessPath 'S:\' | Out-Null
} else {
  'SHARED boot-select.json NOT FOUND on any basic partition'
}
if (Test-Path $repoCfg) {
  'REPO cfg content:'
  Get-Content $repoCfg -Raw
}
'FULL-AUDIT-DONE'
