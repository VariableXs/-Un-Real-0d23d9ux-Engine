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

function Mount-RegistryHive {
  if (-not (Test-Path $UserHive)) { Write-Warning "用户配置 Hive 不存在: $UserHive ; 先用 Data-Init 生成"; return }
  Write-Host ">>> RegLoadKey HKEY_USERS\$KeyName <- $UserHive" -ForegroundColor Cyan
  # PowerShell 无直接 RegLoadKey cmdlet, 用 REG.EXE LOAD(仅 Win 可用, 需要时可换成 P/Invoke)
  & reg.exe load "HKU\$KeyName" $UserHive 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) {
    Write-Warning "reg.exe load 失败(该 key 可能已挂载或系统限制); 继续但保留警告"
  } else {
    Write-Host "    已挂载: HKEY_USERS\$KeyName" -ForegroundColor Green
  }
}

function Unmount-RegistryHive {
  Write-Host ">>> 卸载 HKU\$KeyName" -ForegroundColor Cyan
  & reg.exe unload "HKU\$KeyName" 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) { Write-Warning "卸载失败或未挂载" } else { Write-Host "    已卸载" -ForegroundColor Green }
}

function Apply-EnvVars {
  $map = Get-EnvMap
  if (-not $map.Count) { Write-Host "无 path.env 配置"; return }
  Write-Host ">>> 写入环境变量(当前进程)" -ForegroundColor Cyan
  foreach ($k in $map.Keys) {
    $v = $map[$k]
    if ($v -like 'D:\*') {
      # 保留绝对路径; 其他相对值留给 Core 按 DataRoot 展开
    }
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
