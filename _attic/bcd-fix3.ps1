# VARIX 引导项修复 v3：
# fix2 教训：diskpart 卷号 != \Device\HarddiskVolumeN（fix2 设 HarddiskVolume8 结果解析成 X: WIN_ENGINE）
# 正确定位：ESP 挂临时字母 M: -> 内容校验(标签+BOOTX64.EFI) -> bcdedit 按盘符 set -> 摘除字母
# 顺带：BitLocker 若已恢复保护则再暂停 3 个重启周期（覆盖 BIOS 关 Secure Boot 的重启链）
$ErrorActionPreference = 'Continue'

try { Write-Output ("SecureBoot: " + (Confirm-SecureBootUEFI)) } catch { Write-Output ("SecureBoot: check failed - " + $_.Exception.Message) }

# BitLocker 状态：若恢复保护，再暂停（rc=3：进BIOS/保存重启/选VARIX 三个周期）
try {
  $bl = Get-BitLockerVolume -MountPoint 'C:' -ErrorAction Stop
  Write-Output ("BitLocker ProtectionStatus: " + $bl.ProtectionStatus)
  if ($bl.ProtectionStatus -eq 'On') {
    Write-Output ((manage-bde -protectors -disable C: -rc 3 2>&1 | Out-String).Trim())
    Write-Output "BitLocker suspended rc=3"
  }
} catch {
  Write-Output ("BitLocker check failed: " + $_.Exception.Message)
}

$guid = '{6ebc0f52-5567-11f0-aa5f-c035321b0fae}'
Write-Output "=== VARIX entry BEFORE ==="
Write-Output ((bcdedit /enum $guid 2>&1 | Out-String).Trim())

# 定位 VARIX 所在 USB 盘
$usbDisks = @(Get-Disk | Where-Object { $_.BusType -eq 'USB' })
$diskNo = if ($usbDisks.Count -ge 1) { $usbDisks[0].Number } else { 1 }
Write-Output ("USB disk number: " + $diskNo + " (count=" + $usbDisks.Count + ")")

# 在该盘上按 GPT 类型找 ESP（c12a7328 = EFI System Partition）
$esp = Get-Partition -DiskNumber $diskNo | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
if (-not $esp) { Write-Output "!! USB 盘上未找到 ESP 分区，中止"; Write-Output "BCD3 DONE"; exit }
Write-Output ("ESP partition number: " + $esp.PartitionNumber + "  size(MB): " + [math]::Round($esp.Size/1MB))

# 临时字母 M:（历史黑名单 {Z,Y,X,W,V,U,Q,P} 之外）
if (Get-Volume -DriveLetter M -ErrorAction SilentlyContinue) { Write-Output "!! M: 已被占用，中止"; Write-Output "BCD3 DONE"; exit }
$dp = "select disk $diskNo`r`nselect partition $($esp.PartitionNumber)`r`nassign letter=M`r`n"
$dpFile = Join-Path $env:TEMP 'varix-fix3-mnt.txt'
[IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
Write-Output ((diskpart /s $dpFile 2>&1 | Out-String).Trim())

# 内容+标签双校验落点（diskpart 报成功也可能落错卷的教训）
$ok = $false
for ($i = 0; $i -lt 5; $i++) {
  $v = Get-Volume -DriveLetter M -ErrorAction SilentlyContinue
  if ($v -and $v.FileSystemLabel -eq 'VARIX-ESP' -and (Test-Path 'M:\EFI\BOOT\BOOTX64.EFI')) { $ok = $true; break }
  Start-Sleep 1
}
if (-not $ok) {
  Write-Output "!! M: 落点校验失败（标签或 BOOTX64.EFI 不符），不动 BCD"
  Write-Output "BCD3 DONE"; exit
}
Write-Output "M: 落点校验 OK（VARIX-ESP + BOOTX64.EFI）"

# 取 ESP 真实 \Device\HarddiskVolumeN（留档比对）
Add-Type -Namespace W32 -Name QDD -MemberDefinition '[DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern uint QueryDosDevice(string lpDeviceName, System.Text.StringBuilder lpTargetPath, int ucchMax);'
$sb = New-Object System.Text.StringBuilder 1024
[void][W32.QDD]::QueryDosDevice('M:', $sb, $sb.Capacity)
$devPath = $sb.ToString()
Write-Output ("ESP true device path: " + $devPath)

Write-Output "=== Set device by verified letter M: ==="
bcdedit /set $guid device "partition=M:" 2>&1 | Out-String | Write-Output

Write-Output "=== VARIX entry (letter on) ==="
Write-Output ((bcdedit /enum $guid 2>&1 | Out-String).Trim())

# 摘除临时字母（BCD 存的是分区签名+偏移，不依赖盘符）
$dp2 = "select disk $diskNo`r`nselect partition $($esp.PartitionNumber)`r`nremove letter=M`r`n"
$dp2File = Join-Path $env:TEMP 'varix-fix3-umnt.txt'
[IO.File]::WriteAllText($dp2File, $dp2, [Text.Encoding]::ASCII)
Write-Output ((diskpart /s $dp2File 2>&1 | Out-String).Trim())

Write-Output "=== VARIX entry AFTER (letter removed) ==="
$after = ((bcdedit /enum $guid 2>&1 | Out-String).Trim())
Write-Output $after
if ($after -match 'HarddiskVolume(\d+)') {
  Write-Output ("displayed HarddiskVolume " + $Matches[1] + "  / true: " + $devPath)
}
Write-Output "BCD3 DONE"
