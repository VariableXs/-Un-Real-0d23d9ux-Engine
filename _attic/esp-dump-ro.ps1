# U 盘 ESP 只读 dump：limine.conf 全文 + 文件清单。不写不删。
$ErrorActionPreference = 'Continue'
$dpFile = Join-Path $env:TEMP 'varix-dump-assign.txt'
[IO.File]::WriteAllText($dpFile, "select disk 1`r`nselect partition 1`r`nassign letter=L`r`n", [Text.Encoding]::ASCII)
diskpart /s $dpFile | Out-String | Write-Output

if (Test-Path 'L:\EFI\BOOT\BOOTX64.EFI') {
  Write-Output 'GATE-OK: L: is VARIX ESP'
  Write-Output '===== limine.conf candidates ====='
  foreach ($p in @('L:\EFI\BOOT\limine.conf','L:\limine.conf','L:\EFI\BOOT\limine.cfg')) {
    if (Test-Path $p) {
      Write-Output ('--- FOUND: ' + $p + ' ---')
      Get-Content $p -Raw | Write-Output
    }
  }
  Write-Output '===== ESP root + kernel dir listing ====='
  Get-ChildItem 'L:\' -Force | Select-Object Name,Length | Format-Table -AutoSize | Out-String | Write-Output
  Get-ChildItem 'L:\kernel' -Force -ErrorAction SilentlyContinue | Select-Object Name,Length | Format-Table -AutoSize | Out-String | Write-Output
  Write-Output '===== limine dirs ====='
  Get-ChildItem 'L:\EFI\BOOT' -Force -ErrorAction SilentlyContinue | Select-Object Name,Length | Format-Table -AutoSize | Out-String | Write-Output
} else {
  Write-Output 'GATE-FAIL: L: not VARIX ESP (dump only, nothing written)'
}

$rmFile = Join-Path $env:TEMP 'varix-dump-remove.txt'
[IO.File]::WriteAllText($rmFile, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
diskpart /s $rmFile | Out-String | Write-Output
Write-Output 'ESP-DUMP-DONE'
