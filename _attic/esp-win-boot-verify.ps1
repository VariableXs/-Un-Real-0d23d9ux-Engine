# S1.3 只读核验探针（AI-1, 2026-09-22）：不写任何东西，只取证。
$ErrorActionPreference = 'Continue'
$log = '{log}'
$out = New-Object System.Collections.Generic.List[string]

# 1) 内置盘 ESP 四件套 hash
$sysDisk = (Get-Partition -DriveLetter $env:SystemDrive.Substring(0,1)).DiskNumber
$intEsp = Get-Partition -DiskNumber $sysDisk | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
$acc = '\\?\Volume{' + $intEsp.Guid + '}\EFI\Microsoft\Boot\'
foreach ($f in @('bootmgfw.efi','BCD','bootmgr')) {
  $p = $acc + $f
  if (Test-Path $p) { $out.Add(('int-esp ' + $f + ' = ' + (Get-FileHash $p -Algorithm SHA256).Hash.Substring(0,16))) }
  else { $out.Add(('int-esp ' + $f + ' = MISSING')) }
}
$out.Add(('int-esp guid=' + $intEsp.Guid))

# 2) SHARED boot-select.json 现内容
$cfg = 'W:\boot-select.json'
if (Test-Path $cfg) {
  $out.Add('shARED cfg content:')
  $out.Add((Get-Content $cfg -Raw))
  $j = Get-Content $cfg -Raw | ConvertFrom-Json
  $out.Add(('usb_windows_bootnext=' + $j.usb_windows_bootnext))
} else { $out.Add('shared cfg MISSING') }

# 3) U 盘 ESP 是否已有 Microsoft Boot 文件（挂 L: 只读检查，用后摘）
$usbDisk = $null
foreach ($p in (Get-Partition)) {
  $v = $p | Get-Volume -ErrorAction SilentlyContinue
  if ($null -ne $v -and $v.FileSystemLabel -eq 'WIN_ENGINE') { $usbDisk = $p.DiskNumber }
}
$out.Add(('usb disk=' + $usbDisk))
if ($null -ne $usbDisk) {
  $usbEsp = Get-Partition -DiskNumber $usbDisk | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
  $dp = Join-Path $env:TEMP 'vx-s13chk-assign.txt'
  [IO.File]::WriteAllText($dp, "select disk $usbDisk`r`nselect partition $($usbEsp.PartitionNumber)`r`nassign letter=L`r`n", [Text.Encoding]::ASCII)
  diskpart /s $dp | Out-Null
  foreach ($f in @('L:\EFI\Microsoft\Boot\bootmgfw.efi','L:\EFI\Microsoft\Boot\BCD','L:\EFI\BOOT\BOOTX64.EFI','L:\limine.conf','L:\boot-select.json')) {
    if (Test-Path $f) { $out.Add(('usb-esp ' + $f + ' EXISTS ' + (Get-FileHash $f -Algorithm SHA256).Hash.Substring(0,16))) }
    else { $out.Add(('usb-esp ' + $f + ' MISSING')) }
  }
  $rm = Join-Path $env:TEMP 'vx-s13chk-remove.txt'
  [IO.File]::WriteAllText($rm, "select disk $usbDisk`r`nselect partition $($usbEsp.PartitionNumber)`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
  diskpart /s $rm | Out-Null
}

# 4) 固件项数（bcdedit firmware 尾部）
$fw = & bcdedit /enum firmware 2>&1 | Out-String
$fwLines = ($fw -split "`n") | Where-Object { $_ -match '标识符|identifier|Windows Boot Manager' }
$out.Add('firmware entries (identifier/Windows lines):')
foreach ($l in $fwLines) { $out.Add(('  ' + $l.Trim())) }

Set-Content -Path $log -Value $out -Encoding Unicode
