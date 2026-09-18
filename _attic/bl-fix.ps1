# BitLocker 保命 + 状态全查 + SB 闸门下清 BCD（一次 UAC 做完三件事）
# 背景：用户密钥解锁进 Windows；SB 仍 On（UEFISecureBootEnabled=1）；
#       BitLocker 恢复屏曾报「TPM 不存在或无法识别」。
$ErrorActionPreference = 'Continue'

# ── 1) BitLocker 现状 ──
Write-Output "=== BitLocker status BEFORE ==="
try {
  $bl = Get-BitLockerVolume -MountPoint $env:SystemDrive
  Write-Output ("ProtectionStatus=" + $bl.ProtectionStatus + " VolumeStatus=" + $bl.VolumeStatus + " Encrypted=" + $bl.EncryptionPercentage + "%")
} catch { Write-Output ("BL check failed: " + $_.Exception.Message) }

# ── 2) 无限期暂停 BitLocker（RebootCount=0 直到手动恢复，杜绝再锁机）──
Write-Output "=== Suspend BitLocker (indefinite) ==="
try {
  Suspend-BitLocker -MountPoint $env:SystemDrive -RebootCount 0 -ErrorAction Stop
  Write-Output "SUSPEND-OK (RebootCount=0, 手动恢复前不再要求密钥)"
} catch { Write-Output ("Suspend failed: " + $_.Exception.Message) }

# ── 3) TPM 真实状态（恢复屏说「不存在或无法识别」，查实）──
Write-Output "=== TPM status ==="
try {
  $t = Get-Tpm -ErrorAction Stop
  Write-Output ("TpmPresent=" + $t.TpmPresent + " TpmReady=" + $t.TpmReady + " TpmEnabled=" + $t.TpmEnabled + " TpmActivated=" + $t.TpmActivated + " TpmOwned=" + $t.TpmOwned)
} catch { Write-Output ("TPM check failed: " + $_.Exception.Message) }

# ── 4) Secure Boot 权威判定 ──
$sb = $null
try { $sb = Confirm-SecureBootUEFI -ErrorAction Stop } catch { Write-Output ("SB check failed: " + $_.Exception.Message) }
Write-Output ("SecureBoot: " + $sb)

# ── 5) SB 闸门：关了才删坏引导项 ──
$guid = '{6ebc0f52-5567-11f0-aa5f-c035321b0fae}'
if ($sb -eq $false) {
  Write-Output "=== Delete VARIX chainload entry ==="
  bcdedit /delete $guid 2>&1 | Out-String | Write-Output
  bcdedit /timeout 30 2>&1 | Out-String | Write-Output
  $bm = (bcdedit /enum '{bootmgr}' 2>&1 | Out-String)
  if ($bm.Contains($guid)) {
    Write-Output "displayorder 残留，执行 /remove"
    bcdedit /displayorder $guid /remove 2>&1 | Out-String | Write-Output
  }
  Write-Output ("guid still in BCD: " + ((bcdedit /enum 2>&1 | Out-String)).Contains($guid))
  Write-Output "BCD-CLEANED"
} else {
  Write-Output "SB-STILL-ON: BCD untouched（引导项留着，等真正关 SB 后再清）"
}

# ── 6) 终态回读 ──
Write-Output "=== BitLocker status AFTER ==="
try {
  $bl2 = Get-BitLockerVolume -MountPoint $env:SystemDrive
  Write-Output ("ProtectionStatus=" + $bl2.ProtectionStatus + " VolumeStatus=" + $bl2.VolumeStatus)
} catch { Write-Output ("BL check failed: " + $_.Exception.Message) }
Write-Output "ALLDONE"
