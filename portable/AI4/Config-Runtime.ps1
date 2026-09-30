param(
  [ValidateSet("Apply", "Mount-Registry", "Unmount-Registry", "Apply-Env", "Status")]
  [string]$Action = "Status",
  [string]$DataDrive = "D:",
  [string]$EnvFile = "D:\Data\Env\path.env",
  [string]$UserHive = "D:\Data\Registry\User.dat",
  [string]$KeyName = "VariableUser",
  [string]$ShortcutsFile = "D:\Data\Config\shortcuts.json",
  [switch]$Persist
)
# AI-4 拓展核 / 第8.4章 配置随盘走
#   RegLoadKey + SetEnvironmentVariable + shortcuts.json 热重载。
# 用法:
#   .\Config-Runtime.ps1 -Action Apply        # 挂注册表 + 写入环境变量 + 重载快捷键
#   .\Config-Runtime.ps1 -Action Mount-Registry
#   .\Config-Runtime.ps1 -Action Unmount-Registry
#   .\Config-Runtime.ps1 -Action Apply-Env
#   .\Config-Runtime.ps1 -Action Status
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-EnvMap {
  if (-not (Test-Path $EnvFile)) { return @{} }
  $map = @{}
  foreach ($line in Get-Content $EnvFile) {
    $t = $line.Trim()
    if (-not $t) { continue }
    if ($t -like '#*') { continue }
    if ($t -notmatch '^[A-Za-z_][A-Za-z0-9_]*=.*$') { continue }
    $k, $v = $t -split '=', 2
    if (-not $k -or -not $v) { continue }
    $map[$k.Trim()] = $v.Trim()
  }
  return $map
}

function Test-HiveMounted {
  & reg.exe query "HKU\$KeyName" 2>$null | Out-Null
  return ($LASTEXITCODE -eq 0)
}

function Mount-RegistryHive {
  if (-not (Test-Path $UserHive)) { Write-Warning "用户配置 Hive 不存在: $UserHive ; 先用 Data-Init.ps1 生成合法 Hive"; return }
  if (Test-HiveMounted) { Write-Host ">>> HKEY_USERS\$KeyName 已挂载, 跳过" -ForegroundColor Yellow; return }
  Write-Host ">>> RegLoadKey HKEY_USERS\$KeyName <- $UserHive" -ForegroundColor Cyan
  # PowerShell 无直接 RegLoadKey cmdlet, 用 REG.EXE LOAD(仅 Win 可用, 需要时可换成 P/Invoke)
  & reg.exe load "HKU\$KeyName" $UserHive 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) {
    Write-Warning "reg.exe load 失败(需要管理员/SeBackupPrivilege, 或 Hive 非法; 空文件不行, 用 Data-Init.ps1 重生成)"
  } else {
    Write-Host "    已挂载: HKEY_USERS\$KeyName" -ForegroundColor Green
  }
}

function Unmount-RegistryHive {
  if (-not (Test-HiveMounted)) { Write-Host ">>> HKU\$KeyName 未挂载, 跳过" -ForegroundColor Yellow; return }
  Write-Host ">>> 卸载 HKU\$KeyName" -ForegroundColor Cyan
  & reg.exe unload "HKU\$KeyName" 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) { Write-Warning "卸载失败(可能有句柄占用; reg unload 会自动落盘到 .dat)" } else { Write-Host "    已卸载(配置已写回随盘 Hive)" -ForegroundColor Green }
}

function Apply-EnvVars {
  $map = Get-EnvMap
  if (-not $map.Count) { Write-Host "无 path.env 配置"; return }
  $dataRoot = "$DataDrive\Data"
  Write-Host ">>> 写入环境变量(当前进程, \${DATA_ROOT} -> $dataRoot)" -ForegroundColor Cyan
  foreach ($k in $map.Keys) {
    # 支持随盘相对写法: ${DATA_ROOT}\Tools -> D:\Data\Tools (换宿主盘符只改 -DataDrive)
    $v = $map[$k] -replace '\$\{DATA_ROOT\}', $dataRoot
    try {
      Set-Item -Path "Env:\$k" -Value $v -ErrorAction Stop
      Write-Host "    $k = $v" -ForegroundColor Green
    } catch {
      Write-Warning "    写入失败 $k : $_"
    }
  }
  if ($Persist) {
    Write-Warning "不持久化写宿主注册表(便携系统要求)。如需持久化请用 Core 在启动时注入。"
  }
}

function Reload-Shortcuts {
  if (-not (Test-Path $ShortcutsFile)) { Write-Warning "快捷键配置不存在: $ShortcutsFile"; return }
  $cfg = Get-Content $ShortcutsFile -Raw | ConvertFrom-Json
  Write-Host ">>> 快捷键配置已热重载: $($cfg.hotkeys.Count) 项" -ForegroundColor Green
  foreach ($h in $cfg.hotkeys) {
    Write-Host "    $($h.keys) -> $($h.action)" -ForegroundColor DarkGray
  }
}

function Apply-All {
  Mount-RegistryHive
  Apply-EnvVars
  Reload-Shortcuts
}

function Show-Status {
  Write-Host "===== 配置随盘状态 =====" -ForegroundColor Cyan
  Write-Host "EnvFile:  $EnvFile  $(if(Test-Path $EnvFile){'存在'}else{'缺失'})"
  Write-Host "UserHive: $UserHive  $(if(Test-Path $UserHive){'存在'}else{'缺失'})"
  if (Test-Path $UserHive) {
    $fs = [IO.File]::OpenRead($UserHive)
    try {
      $magic = New-Object byte[] 4
      [void]$fs.Read($magic, 0, 4)
    } finally { $fs.Close() }
    Write-Host "Hive 魔数: $([Text.Encoding]::ASCII.GetString($magic))  $(if([Text.Encoding]::ASCII.GetString($magic) -eq 'regf'){'合法(可挂载)'}else{'非法(用 Data-Init.ps1 重生成)'})"
    Write-Host "挂载状态: $(if (Test-HiveMounted) {"HKEY_USERS\$KeyName 已挂载"} else {'未挂载'})"
  }
  Write-Host "Shortcuts: $ShortcutsFile  $(if(Test-Path $ShortcutsFile){'存在'}else{'缺失'})"
  $envMap = Get-EnvMap
  Write-Host "已载入环境变量: $($envMap.Keys -join ', ')"
  $shortcutCount = 0
  if (Test-Path $ShortcutsFile) { $shortcutCount = ((Get-Content $ShortcutsFile -Raw | ConvertFrom-Json).hotkeys | Measure-Object).Count }
  Write-Host "快捷键配置: $shortcutCount 项"
}

switch ($Action) {
  "Apply"           { Apply-All }
  "Mount-Registry"  { Mount-RegistryHive }
  "Unmount-Registry"{ Unmount-RegistryHive }
  "Apply-Env"       { Apply-EnvVars }
  "Status"          { Show-Status }
}
