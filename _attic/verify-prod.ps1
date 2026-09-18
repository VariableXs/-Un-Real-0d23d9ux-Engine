# 部署产物独立核验：五分区重新挂盘符 -> 读取 ESP / VARIX_SYS / SHARED 内容
$ErrorActionPreference = 'Continue'

'=== Partitions on Disk 1 ==='
Get-Partition -DiskNumber 1 | Sort-Object PartitionNumber |
  ForEach-Object { "P{0}  {1,7:N1}GB  {2}" -f $_.PartitionNumber, ($_.Size/1GB), $_.GptType }

'=== Assign session letters ==='
$letters = @('Z', 'Y', 'X', 'W', 'V')
$pn = 1
foreach ($L in $letters) {
  $rm = if ($pn -eq 1) { "remove all dismount`r`n" } else { "" }
  $dp = "select disk 1`r`nselect partition $pn`r`n$rm" + "assign letter=$L`r`n"
  $dpFile = Join-Path $env:TEMP 'varix-verify-assign.txt'
  [IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
  diskpart /s $dpFile | Out-Null
  Start-Sleep -Milliseconds 800
  "P$pn -> $L`:"
  $pn++
}

'=== ESP tree (P1 -> Z:) ==='
Get-ChildItem -LiteralPath 'Z:\' -Recurse -File -Force -ErrorAction SilentlyContinue |
  ForEach-Object { "{0,12}  {1}" -f $_.Length, $_.FullName.Substring(3) }

'=== ESP-MANIFEST.json ==='
Get-Content -LiteralPath 'Z:\ESP-MANIFEST.json' -Raw -ErrorAction SilentlyContinue

'=== limine.conf ==='
Get-Content -LiteralPath 'Z:\limine.conf' -Raw -ErrorAction SilentlyContinue

'=== VARIX_SYS root (P2 -> Y:) ==='
Get-ChildItem -LiteralPath 'Y:\' -Force -ErrorAction SilentlyContinue |
  ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.Name }

'=== SHARED tree (P3 -> X:) ==='
Get-ChildItem -LiteralPath 'X:\' -Recurse -Depth 1 -Force -ErrorAction SilentlyContinue |
  ForEach-Object { "{0,12}  {1}" -f $(if ($_.PSIsContainer) { '<DIR>' } else { $_.Length }), $_.FullName.Substring(3) }

'=== SHARED boot-select.json ==='
Get-Content -LiteralPath 'X:\contracts\boot-select.json' -Raw -ErrorAction SilentlyContinue

'=== VERIFY DONE ==='
