param(
  [ValidateSet("Setup", "Sync", "Restore", "Schedule", "Diff", "Status")]
  [string]$Action = "Status",
  [string]$Local = "D:\Data",
  [string]$Remote = "remote:VariableBackup",
  [string]$ConfigDir = "D:\Data\Sync",
  [string]$CloudExcludes = "*.tmp,*.log,.git,node_modules,Cache,Dumps",
  [int]$Transfers = 4,
  [int]$BwLimit = 10,              # MB/s
  [string]$ScheduleTime = "30",    # 分钟
  [string]$RemoteFile = "D:\Data\Backup\VariableData.zip",
  [switch]$DryRun
)
# AI-4 拓展核 / 第8.5章 云拓展 + 第14.3章 云同步与版本
#   要求已配置 rclone remote(如 OneDrive/坚果云/S3)。
# 用法:
#   .\Cloud-Sync.ps1 -Action Setup -Remote "onedrive:VariableBackup"
#   .\Cloud-Sync.ps1 -Action Sync                    # 增量上行
#   .\Cloud-Sync.ps1 -Action Diff -DryRun            # 只看差异
#   .\Cloud-Sync.ps1 -Action Restore -RemoteFile ... # 恢复
#   .\Cloud-Sync.ps1 -Action Schedule -ScheduleTime 30
#   .\Cloud-Sync.ps1 -Action Status
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Require-Rclone {
  if (-not (Get-Command rclone.exe -ErrorAction SilentlyContinue)) {
    throw "未找到 rclone.exe。请安装并配置 remote:  https://rclone.org/ "
  }
}

function Parse-Excludes {
  param([string]$Csv)
  return ($Csv -split "," | ForEach-Object { $_.Trim() } | Where-Object { $_ } | ForEach-Object { "--exclude=" + $_ })
}

function New-SyncLog {
  param([string]$Path)
  New-Item -ItemType Directory -Force -Path (Split-Path $Path -Parent) | Out-Null
}

function Setup-Cloud {
  Require-Rclone
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  if (-not $Remote) { throw "-Remote 必填, 例如 onedrive:VariableBackup" }
  $conf = @"
# Variable OS 云同步配置
local = $Local
remote = $Remote
transfers = $Transfers
bwlimit = ${BwLimit}M
excludes = $CloudExcludes
interval_min = $ScheduleTime
"@
  Set-Content -Path "$ConfigDir\rclone.conf" -Value $conf -Encoding UTF8
  Write-Host ">>> 已写入 $ConfigDir\rclone.conf" -ForegroundColor Green
  Write-Host "    先验证: rclone lsd $Remote" -ForegroundColor Yellow
}

function Sync-Cloud {
  Require-Rclone
  $args = @("sync", $Local, $Remote, "--transfers", "$Transfers", "--bwlimit", "${BwLimit}M")
  foreach ($x in (Parse-Excludes $CloudExcludes)) { $args += $x }
  if ($DryRun) {
    $args += "--dry-run"
    Write-Host ">>> 预演同步 (dry-run)" -ForegroundColor Cyan
  } else {
    Write-Host ">>> 增量同步 $Local -> $Remote" -ForegroundColor Cyan
  }
  & rclone.exe @args
  if ($LASTEXITCODE -ne 0) { throw "rclone sync 失败 (exit $LASTEXITCODE)" }
  Write-Host ">>> 同步完成" -ForegroundColor Green
}

function Diff-Cloud {
  Require-Rclone
  Write-Host ">>> 差异检查 $Local vs $Remote" -ForegroundColor Cyan
  & rclone.exe check $Local $Remote --compare size,modtime,checksum --one-way --exclude "*.tmp" --exclude "*.log" --exclude ".git"
  Write-Host ">>> 差异检查结束" -ForegroundColor Green
}

function Restore-Cloud {
  param([string]$File)
  Require-Rclone
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  if ($File) {
    if (-not (Test-Path $File)) { throw "恢复源不存在: $File" }
    Write-Host ">>> 恢复本地压缩包 $File -> $Local" -ForegroundColor Cyan
    Write-Host "    请先确认目标盘有足够空间。实际恢复命令示例:" -ForegroundColor Yellow
    Write-Host "    Expand-Archive $File -DestinationPath $Local -Force" -ForegroundColor Yellow
    return
  }
  Write-Host ">>> 从 $Remote 恢复到 $Local" -ForegroundColor Cyan
  $args = @("copy", $Remote, $Local, "--transfers", "$Transfers", "--bwlimit", "${BwLimit}M")
  foreach ($x in (Parse-Excludes $CloudExcludes)) { $args += $x }
  & rclone.exe @args
  if ($LASTEXITCODE -ne 0) { throw "rclone copy 失败" }
  Write-Host ">>> 恢复完成, 请重挂 VHDX/重启系统验证。" -ForegroundColor Green
}

function Setup-Schedule {
  if (-not (Get-Command schtasks.exe -ErrorAction SilentlyContinue)) { throw "schtasks 不可用" }
  $taskName = "VariableOS-CloudSync"
  $script = Join-Path $ConfigDir "Cloud-Sync.ps1"
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  if (-not (Test-Path $script)) {
    Copy-Item $PSCommandPath $script -Force
  }
  $actionCmd = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$script`" -Action Sync -Local `"$Local`" -Remote `"$Remote`""
  Write-Host ">>> 注册计划任务 $taskName (每 $ScheduleTime 分钟)" -ForegroundColor Cyan
  & schtasks.exe /Create /TN $taskName /TR $actionCmd /SC MINUTE /MO $ScheduleTime /F
  if ($LASTEXITCODE -ne 0) { Write-Warning "计划任务注册失败(可能需要管理员)" }
  else { Write-Host "    已注册: $taskName, 每 $ScheduleTime 分钟云同步。" -ForegroundColor Green }
}

function Show-Status {
  Require-Rclone
  Write-Host "===== 云同步状态 =====" -ForegroundColor Cyan
  & rclone.exe lsd $Remote --max-depth 1 2>&1 | Select-Object -First 20
  $task = schtasks.exe /Query /TN VariableOS-CloudSync 2>$null
  if ($task) { Write-Host "计划任务: $task" -ForegroundColor Green } else { Write-Host "计划任务未注册(可运行 -Action Schedule)" -ForegroundColor Yellow }
  Write-Host "本地目录: $Local"
  if (Test-Path $Local) { (Get-ChildItem $Local -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum / 1GB | ForEach-Object { Write-Host "本地大小: $([math]::Round($_,2)) GB" } }
}

switch ($Action) {
  "Setup"    { Setup-Cloud }
  "Sync"     { Sync-Cloud }
  "Restore"  { Restore-Cloud $RemoteFile }
  "Schedule" { Setup-Schedule }
  "Diff"     { Diff-Cloud }
  "Status"   { Show-Status }
}
