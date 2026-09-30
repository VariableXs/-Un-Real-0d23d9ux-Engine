<#
.SYNOPSIS
  D-1（22.1）：VM 档「登录即 Variable」——离线配置 VM 镜像的 Shell 与自动登录。
  挂载 VHDX 后对镜像注册表离线写入；-Restore 一键改回 explorer（安全模式兜底/回滚）。
  注意：仅作用于 VM 镜像（-Vhdx 指定的盘），宿主注册表零改动。

.EXAMPLE
  .\Set-VmShell.ps1 -Vhdx D:\USB\Variable-OS.vhdx -EngineDir Variable
  .\Set-VmShell.ps1 -Vhdx D:\USB\Variable-OS.vhdx -Restore
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Vhdx,
  # 引擎在镜像内的相对目录（不含盘符）
  [string]$EngineDir = "Variable",
  # 自动登录账户（留空 = 不配置自动登录，用户密码进 VM 内 DPAPI 由 Autologon 工具另行处理）
  [string]$AutoLogonUser = "",
  [string]$AutoLogonPassword = "",
  [switch]$Restore
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  Write-Error '请以管理员运行 PowerShell'; exit 1
}
if (-not (Get-Module -ListAvailable -Name Hyper-V)) { throw '未检测到 Hyper-V 模块' }
Import-Module Hyper-V

Mount-VHD -Path $Vhdx -PassThru | Out-Null
try {
  $disk = Get-Disk | Where-Object { $_.Location -like "*$([IO.Path]::GetFileName($Vhdx))*" } | Select-Object -First 1
  $drive = (Get-Partition -DiskNumber $disk.Number | Where-Object { $_.DriveLetter }).DriveLetter + ':'
  Write-Host ">>> 镜像盘 $drive" -ForegroundColor Cyan
  reg load HKLM\VarSOFT "$drive\Windows\System32\config\SOFTWARE" | Out-Null
  try {
    $winlogon = 'HKLM:\VarSOFT\Microsoft\Windows NT\CurrentVersion\Winlogon'
    if ($Restore) {
      Set-ItemProperty -Path $winlogon -Name Shell -Value 'explorer.exe'
      Write-Host '>>> 已还原 Shell=explorer.exe（Variable 退回普通应用模式）' -ForegroundColor Green
    } else {
      $shellExe = "$drive\$EngineDir\Variable.exe"
      if (-not (Test-Path $shellExe)) { throw "镜像内引擎不存在：$shellExe（先跑 AI3\Install-VMAgent.ps1）" }
      Set-ItemProperty -Path $winlogon -Name Shell -Value "C:\$EngineDir\Variable.exe"
      Write-Host ">>> Shell = C:\$EngineDir\Variable.exe（登录即 Variable）" -ForegroundColor Green
      if ($AutoLogonUser) {
        Set-ItemProperty -Path $winlogon -Name AutoAdminLogon -Value '1'
        Set-ItemProperty -Path $winlogon -Name DefaultUserName -Value $AutoLogonUser
        Set-ItemProperty -Path $winlogon -Name DefaultPassword -Value $AutoLogonPassword
        Write-Host ">>> 自动登录已配置（$AutoLogonUser）。提示：生产环境建议用 Autologon for Windows（DPAPI 加密）替换明文 DefaultPassword" -ForegroundColor Yellow
      }
    }
  } finally { [gc]::Collect(); reg unload HKLM\VarSOFT | Out-Null }
} finally {
  Dismount-VHD -Path $Vhdx -ErrorAction SilentlyContinue | Out-Null
}
Write-Host '>>> 完成（引导器侧修复模式：把本脚本 -Restore 交由 Runbook 收录）' -ForegroundColor Cyan
