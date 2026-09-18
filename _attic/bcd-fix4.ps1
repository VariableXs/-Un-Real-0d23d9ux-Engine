# VARIX 引导项修复 v4：恢复指向 ESP（\Device\HarddiskVolume6，已内容实证）
# 实证链：QueryDosDevice(M:) = \Device\HarddiskVolume6，且 M:\ 上有 BOOTX64.EFI + limine-bios.sys
# fix2 教训：diskpart 卷号 != HarddiskVolumeN（卷8=VARIX-ESP 但 HarddiskVolume8=X: WIN_ENGINE）
# 顺带清掉 fix3 的 M: 半挂载残留（mount manager 未登记的 DOS device = 地雷）
$ErrorActionPreference = 'Continue'

try { Write-Output ("SecureBoot: " + (Confirm-SecureBootUEFI)) } catch { Write-Output ("SecureBoot: check failed - " + $_.Exception.Message) }

$guid = '{6ebc0f52-5567-11f0-aa5f-c035321b0fae}'
Write-Output "=== VARIX entry BEFORE ==="
Write-Output ((bcdedit /enum $guid 2>&1 | Out-String).Trim())

# 第一步：借 M: 的解析做内容实证（M: -> HarddiskVolume6）
Add-Type -Namespace W32 -Name QDD -MemberDefinition '[DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern uint QueryDosDevice(string lpDeviceName, System.Text.StringBuilder lpTargetPath, int ucchMax);'
$sb = New-Object System.Text.StringBuilder 1024
[void][W32.QDD]::QueryDosDevice('M:', $sb, $sb.Capacity)
$devPath = $sb.ToString()
Write-Output ("M: resolves to: " + $devPath)

$bootOk = Test-Path 'M:\EFI\BOOT\BOOTX64.EFI'
$limOk  = Test-Path 'M:\limine-bios.sys'
Write-Output ("content check: BOOTX64=" + $bootOk + " limine-bios.sys=" + $limOk)
if (-not ($bootOk -and $limOk)) {
  Write-Output "!! 内容实证失败，不恢复不清理，人工介入"
  Write-Output "BCD4 DONE"; exit
}
if ($devPath -ne '\Device\HarddiskVolume6') {
  Write-Output "!! M: 解析路径非 HarddiskVolume6，中止"
  Write-Output "BCD4 DONE"; exit
}
Write-Output "实证通过：HarddiskVolume6 = VARIX ESP"

# 第二步：清 M: 残留（先 mountvol /D，失败再 diskpart remove）
Write-Output "=== Cleanup M: ==="
$mv = (mountvol M: /D 2>&1 | Out-String).Trim()
Write-Output ("mountvol /D: " + $mv)
$sb2 = New-Object System.Text.StringBuilder 1024
$r = [W32.QDD]::QueryDosDevice('M:', $sb2, $sb2.Capacity)
if ($r -gt 0) {
  Write-Output "mountvol 未清掉，尝试 diskpart remove"
  $dp = "select disk 1`r`nselect partition 1`r`nremove letter=M`r`n"
  $dpFile = Join-Path $env:TEMP 'varix-fix4-rm.txt'
  [IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
  Write-Output ((diskpart /s $dpFile 2>&1 | Out-String).Trim())
}
$sb3 = New-Object System.Text.StringBuilder 1024
$r3 = [W32.QDD]::QueryDosDevice('M:', $sb3, $sb3.Capacity)
Write-Output ("M: after cleanup QueryDosDevice rc=" + $r3 + " -> [" + $sb3.ToString() + "]")
Write-Output ("M: Test-Path after cleanup: " + (Test-Path 'M:\'))

# 第三步：恢复 BCD 指向
Write-Output "=== Restore device to HarddiskVolume6 ==="
bcdedit /set $guid device "partition=\Device\HarddiskVolume6" 2>&1 | Out-String | Write-Output

Write-Output "=== VARIX entry AFTER ==="
Write-Output ((bcdedit /enum $guid 2>&1 | Out-String).Trim())
Write-Output "BCD4 DONE"
