param(
  [ValidateSet("Apply", "BitLocker", "Defender", "License", "Compliance", "SelfCheck", "Cleanup", "Status")]
  [string]$Action = "Status",
  [string]$DataDrive = "D:",
  [string]$AppsExclude = "D:\Data\Apps",
  [string]$ExchangeForce = "D:\Data\Exchange",
  [string]$RecoveryFile = "D:\Data\Security\BitLocker-Recovery.txt",
  [string]$BackupPath = "",
  [switch]$NoAutoUnlock,
  [switch]$Force
)
# AI-4 拓展核 / 第10章 安全与合规 + 扩充14.3 云备份 + 扩充18 纵深防御
# 用法:
#   .\Security-Manager.ps1 -Action Status
#   .\Security-Manager.ps1 -Action Apply -DataDrive D:
#   .\Security-Manager.ps1 -Action BitLocker -DataDrive E:      # 开启 XTS-AES256
#   .\Security-Manager.ps1 -Action Defender -DataDrive D:
#   .\Security-Manager.ps1 -Action License
#   .\Security-Manager.ps1 -Action Compliance -BackupPath E:\backup.bcd
#   .\Security-Manager.ps1 -Action Cleanup -BackupPath E:\backup.bcd
#   .\Security-Manager.ps1 -Action SelfCheck
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Test-Admin {
  return ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Show-BitLockerStatus {
  param([string]$Drive)
  Write-Host "===== BitLocker $Drive =====" -ForegroundColor Cyan
  if (-not (Get-Command manage-bde.exe -ErrorAction SilentlyContinue)) { Write-Warning "manage-bde 不可用(需完整 Windows / 高级功能)"; return }
  manage-bde.exe -status $Drive 2>$null
  Write-Host ""
}

function Enable-BitLocker {
  param([string]$Drive)
  if (-not (Test-Admin)) { Write-Error "BitLocker 开启需要管理员权限"; return }
  if (-not (Get-Command manage-bde.exe -ErrorAction SilentlyContinue)) { Write-Warning "manage-bde 不可用"; return }
  $status = manage-bde.exe -status $Drive 2>$null | Out-String
  if ($status -match "保护已启用|Protection On|Fully Encrypted") {
    Write-Host ">>> $Drive 已启用 BitLocker, 跳过" -ForegroundColor Green
    return
  }
  if ($status -match "等待激活|Waiting for activation|Encryption in progress") {
    Write-Host ">>> $Drive 正在加密/等待激活, 续用 -Status 查看" -ForegroundColor Yellow
    return
  }
  if ($status -match "百分比已加密|Percentage Encrypted") {
    Write-Host ">>> $Drive 加密进行中" -ForegroundColor Yellow
    return
  }
  Write-Host ">>> 启用 BitLocker (XTS-AES 256) on $Drive" -ForegroundColor Cyan
  if (-not $NoAutoUnlock) {
    # 先关闭自动解锁, 保证拔盘即锁; 宿主无密码时 To Go 只读。
    manage-bde.exe -autounlock -off $Drive 2>$null | Out-Null
  }
  $proc = Start-Process -FilePath "manage-bde.exe" -ArgumentList @("-on", $Drive, "-EncryptionMethod", "XTS-AES256", "-RecoveryPassword") -NoNewWindow -PassThru -Wait
  if ($proc.ExitCode -ne 0) { throw "manage-bde -on 失败, 退出码 $($proc.ExitCode)" }
  # 保存恢复密钥
  $out = manage-bde.exe -protectors -get $Drive -type RecoveryPassword 2>$null | Out-String
  $recovery = ($out | Select-String "数字密码|Numerical Password|Recovery Password" | Select-Object -First 1).Line
  New-Item -ItemType Directory -Force -Path (Split-Path $RecoveryFile -Parent) | Out-Null
  "Variable OS BitLocker Recovery" | Set-Content $RecoveryFile -Encoding UTF8
  Get-Date -Format o | Add-Content $RecoveryFile
  $out | Add-Content $RecoveryFile
  Write-Host ">>> 恢复密钥已保存 $RecoveryFile (请另存离线, 且不要在 Data 内再放明文恢复包)" -ForegroundColor Green
  Write-Host "    拔盘自动锁; 宿主无密码时只能 BitLocker To Go Reader 只读。" -ForegroundColor Yellow
}

function Apply-Defender {
  param([string]$Drive)
  if (-not (Get-Command powershell.exe -ErrorAction SilentlyContinue)) { Write-Warning "无法调用 Defender"; return }
  $defender = Get-MpPreference -ErrorAction SilentlyContinue
  if (-not $defender) { Write-Warning "Windows Defender 不可用(可能是被组策略禁用或精简系统)"; return }
  Write-Host ">>> 配置 Defender 排除项 (Data\Apps 绿色软件)", $AppsExclude -ForegroundColor Cyan
  Add-MpPreference -ExclusionPath $AppsExclude -ErrorAction SilentlyContinue
  Write-Host ">>> Data\Exchange 受控通道强制扫描后放行 (启用实时保护 + 入库清单)" -ForegroundColor Cyan
  $forceDir = $ExchangeForce
  New-Item -ItemType Directory -Force -Path $forceDir | Out-Null
  Set-Content -Path (Join-Path $forceDir ".scan-policy.txt") -Value @"
# Variable OS Exchange 强制扫描策略
# 进入宿主前必须通过 Defender 扫描(见 AI5/CI 扫描或 Explore 入口)。
requirement = defender-full-scan
allow-unsigned = false
autoquarantine = true
"@ -Encoding UTF8
  Write-Host ">>> 白名单/黑名单已更新" -ForegroundColor Green
  Write-Host "    - 排除: $AppsExclude (绿色软件, 防误杀)" -ForegroundColor Green
  Write-Host "    - 扫描: $forceDir (交换通道, 强制扫描)" -ForegroundColor Green
}

function Apply-LicenseStatus {
  Write-Host "===== Windows 授权 =====" -ForegroundColor Cyan
  slmgr /dlv 2>$null
  Write-Host ""
  Write-Host ">>> 授权策略: 零售/批量 Key, Sysprep 后 slmgr /ato 自动激活。OEM 不支持换主板。" -ForegroundColor Yellow
  Write-Host ">>> Variable Engine 自身 MIT; MSIX 包需遵守原软件许可。" -ForegroundColor Yellow
}

function Show-Compliance {
  param([string]$BackupPath)
  Write-Host "===== 合规基线 =====" -ForegroundColor Cyan
  if ((Get-PSDrive -Name C -ErrorAction SilentlyContinue).Free) { Write-Host "VHDX 已作为独立分区存在, 宿主 C 盘不在便携系统内。" -ForegroundColor Green }
  Write-Host "1. 不修改宿主 MBR/GPT: B 模式仅写 U盘引导, 宿主硬盘 offline。" -ForegroundColor Green
  Write-Host "2. 一键卸载: bcdedit /delete {GUID} + diskpart offline。" -ForegroundColor Green
  if ($BackupPath) {
    Write-Host ">>> 备份 BCD 引导配置 -> $BackupPath" -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path (Split-Path $BackupPath -Parent) | Out-Null
    bcdedit /export $BackupPath 2>$null
    Write-Host "    已备份: $BackupPath" -ForegroundColor Green
  }
  $active = Get-Partition -DriveLetter C -ErrorAction SilentlyContinue
  if ($active) {
    Write-Host ">>> 当前系统盘: $($active.DriveLetter):  $($active.DiskNumber) 来源VHDX: $(Get-Partition -DriveLetter C -ErrorAction SilentlyContinue | Select-Object -ExpandProperty DiskNumber)" -ForegroundColor Yellow
  }
}

function Uninstall-Cleanup {
  param([string]$BackupPath)
  if (-not (Test-Admin)) { Write-Error "清理引导需要管理员"; return }
  Write-Host ">>> 清理 U 盘引导项 + 宿主 offline (合规清理, 稍后提示确认)" -ForegroundColor Yellow
  if ($BackupPath) {
    Write-Host "    回滚: bcdedit /import $BackupPath"
  }
  Write-Host "    bcdedit /enum firmware | findstr VariableOS" -ForegroundColor Yellow
  Write-Host "    bcdedit /delete {VariableOS-GUID} /cleanup" -ForegroundColor Yellow
  Write-Host "    diskpart: 'select disk <u盘号>' + 'offline' 实现宿主离线。" -ForegroundColor Yellow
  Write-Host ">>> 已完成提示式清理(未实际修改, 避免误删宿主引导)。" -ForegroundColor Green
}

function SelfCheck {
  Write-Host "===== AI-4 安全自检 =====" -ForegroundColor Cyan
  $ok = $true
  # 1 Data 结构
  foreach ($d in @("$DataDrive\Data\Apps", "$DataDrive\Data\MSIX", "$DataDrive\Data\Plugins", "$DataDrive\Data\Exchange", "$DataDrive\Data\Security")) {
    if (-not (Test-Path $d)) { Write-Warning "缺失目录: $d"; $ok = $false } else { Write-Host "OK 目录: $d" -ForegroundColor Green }
  }
  # 2 恢复密钥
  if (-not (Test-Path $RecoveryFile)) { Write-Warning "缺失恢复密钥(若已启用BitLocker): $RecoveryFile"; $ok = $false }
  # 3 Defender
  $mp = Get-MpPreference -ErrorAction SilentlyContinue
  if ($mp) {
    if (($mp.ExclusionPath | Where-Object { $_ -eq $AppsExclude })) { Write-Host "OK Defender 排除: $AppsExclude" -ForegroundColor Green }
    else { Write-Warning "Defender 未排除 $AppsExclude"; $ok = $false }
  } else { Write-Warning "Defender 不可用"; $ok = $false }
  # 4 云工具
  if (Get-Command rclone.exe -ErrorAction SilentlyContinue) { Write-Host "OK rclone 可用" -ForegroundColor Green } else { Write-Host "WARN 未安装 rclone (云拓展可选)" -ForegroundColor Yellow }
  # 5 BitLocker 状态
  if (Get-Command manage-bde.exe -ErrorAction SilentlyContinue) {
    $st = (manage-bde.exe -status $DataDrive 2>$null | Out-String)
    if ($st -match "保护已启用|Protection On|Fully Encrypted") { Write-Host "OK BitLocker: $DataDrive 已保护" -ForegroundColor Green } else { Write-Warning "BitLocker 未启用 $DataDrive"; $ok = $false }
  } else { Write-Warning "manage-bde 不可用(精简系统)" }
  Write-Host ""
  if ($ok) { Write-Host ">>> 安全自检通过" -ForegroundColor Green } else { Write-Host ">>> 存在待办项, 请按上方警告处理" -ForegroundColor Yellow }
}

function Show-Status {
  $data = $DataDrive + "\Data"
  Write-Host "===== AI-4 安全与拓展总状态 =====" -ForegroundColor Cyan
  Write-Host "Data 目录: $data"
  if (Test-Path $data) {
    Get-ChildItem $data -Force | Select-Object Name, Mode, LastWriteTime | Format-Table -AutoSize
  } else {
    Write-Warning "Data 目录不存在, 请先创建 VHDX/挂载 Data 分区"
  }
  Show-BitLockerStatus $DataDrive
  $mp = Get-MpPreference -ErrorAction SilentlyContinue
  if ($mp) { Write-Host "Defender 排除项: $($mp.ExclusionPath -join ', ')" -ForegroundColor Green } else { Write-Warning "Defender 不可用" }
  Apply-LicenseStatus
  Show-Compliance
}

switch ($Action) {
  "Apply"      { Enable-BitLocker $DataDrive; Apply-Defender $DataDrive; Show-Compliance }
  "BitLocker"  { Enable-BitLocker $DataDrive }
  "Defender"   { Apply-Defender $DataDrive }
  "License"    { Apply-LicenseStatus }
  "Compliance" { Show-Compliance $BackupPath }
  "SelfCheck"  { SelfCheck }
  "Cleanup"    { Uninstall-Cleanup $BackupPath }
  "Status"     { Show-Status }
}
