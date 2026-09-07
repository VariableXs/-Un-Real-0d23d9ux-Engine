# V-1：把引擎（vm-agent 构建）安装进 VM 镜像
# 用法（宿主）: .\Install-VMAgent.ps1 -Vhdx <vhdx> -EngineSrc <Variable.exe 所在目录>
# 前提：已以管理员挂载 VHDX（Mount-VHD），拿到盘符 -Drive
param(
  [Parameter(Mandatory=$true)][string]$Vhdx,
  [Parameter(Mandatory=$true)][string]$EngineSrc,
  [string]$Drive = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  Write-Error "请以管理员运行 PowerShell"; exit 1
}
if (-not $Drive) {
  Mount-VHD -Path $Vhdx -PassThru | Out-Null
  $disk = Get-Disk | Where-Object { $_.Location -like "*$([IO.Path]::GetFileName($Vhdx))*" } | Select-Object -First 1
  $Drive = (Get-Partition -DiskNumber $disk.Number | Where-Object { $_.DriveLetter }).DriveLetter + ":"
}
Write-Host ">>> 目标盘 $Drive" -ForegroundColor Cyan
$dest = Join-Path $Drive "Variable"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item -Path (Join-Path $EngineSrc "*") -Destination $dest -Recurse -Force
Write-Host ">>> 引擎已复制到 $dest\Variable.exe" -ForegroundColor Green
# 运行档环境（VM 内持久化在镜像注册表 —— VM 内允许，宿主不受影响）
reg load HKLM\VarVM "$Drive\Windows\System32\config\SYSTEM" | Out-Null
try {
  New-Item -Path "HKLM:\VarVM\ControlSet001\Control\Session Manager\Environment" -ErrorAction SilentlyContinue | Out-Null
  Set-ItemProperty -Path "HKLM:\VarVM\ControlSet001\Control\Session Manager\Environment" -Name "VAR_RUNTIME_MODE" -Value "vm"
  Set-ItemProperty -Path "HKLM:\VarVM\ControlSet001\Control\Session Manager\Environment" -Name "VAR_HOST_ADDR" -Value "" # Default Switch 动态，agent 兜底用 47631 被动探测
} finally { reg unload HKLM\VarVM | Out-Null }
Write-Host ">>> VAR_RUNTIME_MODE=vm 已写入镜像环境" -ForegroundColor Green
Write-Host ">>> 完成。D-1 批次将把 Shell 指向 $dest\Variable.exe（当前脚本不改 Winlogon）" -ForegroundColor Yellow
Dismount-VHD -Path $Vhdx -ErrorAction SilentlyContinue | Out-Null
