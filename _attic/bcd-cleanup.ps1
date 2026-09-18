# VARIX 引导项清理：删除失败的 bootmgr 链载项，恢复 timeout=30
# 依据（0xc000007b 定性）：QEMU+EDK2 隔离测试铁证 —— 同一份五件套固件直接引导全链跑通；
#       Windows bootmgr 链式加载 BOOTX64.EFI 报镜像非法（0xc000007b），该中转路径不可用。
# 引导改走 UEFI 固件直接引导（Boot Menu 选 U 盘 / BIOS 设第一启动项）。
# 闸门：SecureBoot 仍开启则不动 BCD（签名拦截未解除，删项无意义）。
$ErrorActionPreference = 'Continue'

$guid = '{6ebc0f52-5567-11f0-aa5f-c035321b0fae}'

try { Write-Output ("SecureBoot: " + (Confirm-SecureBootUEFI)) } catch { Write-Output ("SecureBoot: check failed - " + $_.Exception.Message) }
$sbState = $false
try { $sbState = Confirm-SecureBootUEFI } catch { }
if ($sbState) {
  Write-Output "SB-STILL-ON: ABORT - 不动 BCD，请先在 BIOS 关闭 Secure Boot"
  Write-Output "BCD-CLEANUP DONE"
  exit
}

Write-Output "=== BEFORE: bootmgr settings ==="
Write-Output ((bcdedit /enum '{bootmgr}' 2>&1 | Out-String).Trim())

Write-Output "=== Delete VARIX bootmgr chainload entry ==="
bcdedit /delete $guid 2>&1 | Out-String | Write-Output
Write-Output ("delete rc=" + $LASTEXITCODE)

Write-Output "=== Restore timeout to 30 ==="
bcdedit /timeout 30 2>&1 | Out-String | Write-Output
Write-Output ("timeout rc=" + $LASTEXITCODE)

Write-Output "=== AFTER: verify ==="
$bm = (bcdedit /enum '{bootmgr}' 2>&1 | Out-String)
Write-Output $bm.Trim()
if ($bm.Contains($guid)) {
  Write-Output "displayorder 残留，执行 /displayorder /remove"
  bcdedit /displayorder $guid /remove 2>&1 | Out-String | Write-Output
  Write-Output ((bcdedit /enum '{bootmgr}' 2>&1 | Out-String).Trim())
}
$all = (bcdedit /enum 2>&1 | Out-String)
Write-Output ("guid still in BCD: " + $all.Contains($guid))
Write-Output "BCD-CLEANUP DONE"
