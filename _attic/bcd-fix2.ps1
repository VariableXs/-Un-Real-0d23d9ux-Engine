# VARIX 引导失败诊断+加固：
# 1) Secure Boot 状态（签名拒绝是「文件异常」高发原因）
# 2) 把 VARIX 项 device 从盘符形式改为 \Device\HarddiskVolumeN 显式卷号（免盘符依赖）
# 3) 全程回读验证
$ErrorActionPreference = 'Continue'

try {
  Write-Output ("SecureBoot: " + (Confirm-SecureBootUEFI))
} catch {
  Write-Output ("SecureBoot: check failed - " + $_.Exception.Message)
}

Write-Output "=== VARIX entry BEFORE ==="
$guid = '{6ebc0f52-5567-11f0-aa5f-c035321b0fae}'
Write-Output ((bcdedit /enum $guid 2>&1 | Out-String).Trim())

Write-Output "=== Volume list (find VARIX-ESP) ==="
$dp = "list volume`r`n"
$dpFile = Join-Path $env:TEMP 'varix-listvol.txt'
[IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
$volOut = diskpart /s $dpFile 2>&1 | Out-String
Write-Output $volOut.Trim()

# 解析 VARIX-ESP 行的卷号（diskpart 卷号 = \Device\HarddiskVolumeN 的 N）
$volN = $null
foreach ($line in ($volOut -split "`r?`n")) {
  if ($line -match 'VARIX-ESP') {
    if ($line -match '卷\s+(\d+)')      { $volN = $Matches[1] }
    elseif ($line -match 'Volume\s+(\d+)') { $volN = $Matches[1] }
  }
}
Write-Output ("VARIX-ESP volume number: " + $volN)
if ($volN) {
  Write-Output "=== Set device to explicit HarddiskVolume ==="
  bcdedit /set $guid device "partition=\Device\HarddiskVolume$volN" 2>&1 | Out-String | Write-Output
} else {
  Write-Output "!! 未解析到 VARIX-ESP 卷号（盘可能没插/卷未挂），跳过 device 修改"
}

Write-Output "=== bootmgr AFTER ==="
Write-Output ((bcdedit /enum '{bootmgr}' 2>&1 | Out-String).Trim())
Write-Output "=== VARIX entry AFTER ==="
Write-Output ((bcdedit /enum $guid 2>&1 | Out-String).Trim())
Write-Output "BCD2 DONE"
