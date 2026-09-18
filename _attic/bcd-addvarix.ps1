# BCD 加 VARIX 引导项（用户已授权）。安全流程：
#   备份 BCD -> BitLocker 检查(On 则先暂停，防恢复密钥锁机) -> ESP 盘符 ->
#   创建 osloader 指向 \EFI\BOOT\BOOTX64.EFI -> 挂 {bootmgr} 菜单第二位(Windows 保持默认) ->
#   回读验证。回滚：bcdedit /delete {guid} + timeout 还原原值；或 bcdedit /import 备份文件。
$ErrorActionPreference = 'Stop'
$repo = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'

# 1. 备份 BCD（export 到仓库 _attic）
bcdedit /export "$repo\_attic\BCD-backup-$stamp"
Write-Output "BCD backed up: BCD-backup-$stamp"

# 2. BitLocker / 设备加密检查（家庭版 Get-BitLockerVolume 可能不存在，双路检测）
$protOn = $false
try {
  $bl = Get-BitLockerVolume -MountPoint $env:SystemDrive -ErrorAction Stop
  Write-Output ("BitLocker ProtectionStatus: " + $bl.ProtectionStatus)
  if ($bl.ProtectionStatus -eq 'On') { $protOn = $true }
} catch {
  Write-Output ("Get-BitLockerVolume unavailable: " + $_.Exception.Message)
}
if (-not $protOn) {
  # manage-bde 兜底（中文输出「保护已开启」/ 英文 "Protection On"）
  $mb = manage-bde -status C: 2>&1 | Out-String
  Write-Output $mb.Trim()
  if ($mb -match '保护已开启' -or $mb -match 'Protection On') { $protOn = $true }
}
if ($protOn) {
  try {
    manage-bde -protectors -disable C: -rc 2 2>&1 | Out-String | Write-Output
    Write-Output "BitLocker/DeviceEncryption suspended for 2 reboots (rc=2)"
  } catch {
    Write-Output ("SUSPEND FAILED - abort BCD change: " + $_.Exception.Message)
    Write-Output "BCD ABORT"
    exit 0
  }
}

# 3. ESP 盘符（remove all dismount 清残留 -> assign Z）
$dp = "select disk 1`r`nselect partition 1`r`nremove all dismount`r`nassign letter=Z`r`n"
$dpFile = Join-Path $env:TEMP 'varix-bcd-esp.txt'
[IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
$dpOut = diskpart /s $dpFile 2>&1 | Out-String
Write-Output "diskpart ESP assign output:"
Write-Output $dpOut.Trim()
Start-Sleep -Seconds 2
if (-not (Test-Path -LiteralPath 'Z:\')) { throw 'ESP 盘符分配失败（Z: 不可达）' }
Write-Output "ESP mounted Z:"

# 4. 记录 {bootmgr} 原始状态（timeout 原值供回滚）
$before = bcdedit /enum '{bootmgr}' 2>&1 | Out-String
Write-Output "=== bootmgr BEFORE ==="
Write-Output $before.Trim()
$origTimeout = $null
if ($before -match 'timeout\s+(\d+)') { $origTimeout = $Matches[1] }
Write-Output ("original timeout: " + $origTimeout)

# 5. 创建 osloader 条目
$createOut = bcdedit /create /d 'VARIX (USB)' /application osloader 2>&1 | Out-String
Write-Output $createOut.Trim()
if ($createOut -match '\{([0-9a-fA-F\-]{36})\}') {
  $guid = '{' + $Matches[1] + '}'
} else { throw ('bcdedit /create 未返回 GUID: ' + $createOut) }
Write-Output ("New entry: " + $guid)

# 6. 设 device（bcdedit 在 set 时把盘符解析并固化为 GPT 分区引用，跨重启稳定）与 path
bcdedit /set $guid device partition=Z:  2>&1 | Out-String | Write-Output
bcdedit /set $guid path '\EFI\BOOT\BOOTX64.EFI'  2>&1 | Out-String | Write-Output
bcdedit /set $guid locale zh-CN  2>&1 | Out-String | Write-Output

# 7. 挂 {bootmgr} 菜单：VARIX 加在最后（Windows 保持默认第一），timeout 5 秒
bcdedit /set '{bootmgr}' displayorder $guid /addlast  2>&1 | Out-String | Write-Output
bcdedit /set '{bootmgr}' timeout 5  2>&1 | Out-String | Write-Output

# 8. 回读验证
Write-Output "=== bootmgr AFTER ==="
Write-Output ((bcdedit /enum '{bootmgr}' 2>&1 | Out-String).Trim())
Write-Output "=== VARIX entry ==="
Write-Output ((bcdedit /enum $guid 2>&1 | Out-String).Trim())
Write-Output ("rollback: bcdedit /delete $guid ; bcdedit /set '{bootmgr}' timeout " + $origTimeout)
Write-Output "BCD DONE"
