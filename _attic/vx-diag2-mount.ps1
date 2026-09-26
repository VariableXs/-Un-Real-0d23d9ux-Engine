$ErrorActionPreference = 'Continue'
$d = Get-Disk | Where-Object BusType -eq 'USB' | Select-Object -First 1
if (-not $d) { 'NO-USB-DISK'; exit 0 }
'DISK=' + $d.Number + ' ' + $d.FriendlyName
$esp = Get-Partition -DiskNumber $d.Number | Where-Object GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' | Select-Object -First 1
if ($esp) {
  if ($esp.AccessPaths -contains 'Y:\') { 'ESP already Y:' }
  else { Add-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $esp.PartitionNumber -AccessPath 'Y:\' | Out-Null; 'ESP mounted Y:' }
  if (Test-Path 'Y:\boot-select.json') {
    Get-Item 'Y:\boot-select.json' | ForEach-Object { 'ESP_CFG mtime=' + $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
    '--- ESP boot-select.json ---'
    Get-Content 'Y:\boot-select.json' -Raw
  } else { 'ESP: no boot-select.json' }
  '=== BCD {default} raw ==='
  bcdedit /store 'Y:\EFI\Microsoft\Boot\BCD' /enum '{default}' /v
}
$parts = Get-Partition -DiskNumber $d.Number | Where-Object GptType -eq '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}' | Sort-Object PartitionNumber
foreach ($p in $parts) {
  $lp = 'S:'
  $existing = @($p.AccessPaths | Where-Object { $_ -match '^[A-Z]:\\$' })
  if ($existing.Count -gt 0) {
    $lp = $existing[0].TrimEnd('\')
    'PART already ' + $lp + ' #' + $p.PartitionNumber
  } else {
    Add-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath ($lp + '\') | Out-Null
    'PART mounted S: #' + $p.PartitionNumber
  }
  $fs = Get-Volume -DriveLetter $lp.Substring(0,1) -ErrorAction SilentlyContinue
  '  guid=' + $p.Guid + ' fs=' + $fs.FileSystem + ' label=' + $fs.FileSystemLabel + ' size=' + [math]::Round($p.Size/1GB,1) + 'GB'
  '  winload.efi=' + (Test-Path ($lp + '\Windows\System32\winload.efi'))
  '  SYSTEM hive=' + (Test-Path ($lp + '\Windows\System32\config\SYSTEM'))
  '  boot-select.json=' + (Test-Path ($lp + '\boot-select.json'))
  '  Variable.exe=' + (Test-Path ($lp + '\Variable\Variable.exe'))
  '  Default NTUSER.DAT=' + (Test-Path ($lp + '\Users\Default\NTUSER.DAT'))
  $cfg = Get-Item ($lp + '\boot-select.json') -ErrorAction SilentlyContinue
  if ($cfg) {
    '  CFG mtime=' + $cfg.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
    '  CFG content: ' + (Get-Content $cfg.FullName -Raw)
  }
  if ($p.AccessPaths -contains 'S:\') {
    Remove-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $p.PartitionNumber -AccessPath 'S:\' | Out-Null
  }
}
if ($esp -and ($esp.AccessPaths -contains 'Y:\')) {
  Remove-PartitionAccessPath -DiskNumber $d.Number -PartitionNumber $esp.PartitionNumber -AccessPath 'Y:\' | Out-Null
  'ESP unmounted'
}
'PS-DIAG-DONE'
