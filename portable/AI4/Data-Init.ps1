param(
  [string]$DataDrive = "D:",
  [switch]$FixLinks
)
# AI-4 拓展核 / 第3.2章 读写分离 Data 结构 + 第8.4章 配置随盘走
#   初始化 Data 目录树, 并可创建 C 盘符号链接(安装版大软件实体留 Data)。
# 用法:
#   .\Data-Init.ps1 -DataDrive D:
#   .\Data-Init.ps1 -DataDrive D: -FixLinks
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$DataRoot = Join-Path $DataDrive "Data"

$Dirs = @(
  "$DataRoot\Apps", "$DataRoot\MSIX", "$DataRoot\Plugins", "$DataRoot\User",
  "$DataRoot\Exchange", "$DataRoot\Cache", "$DataRoot\Dumps", "$DataRoot\Config",
  "$DataRoot\Env", "$DataRoot\Registry", "$DataRoot\Backup", "$DataRoot\Security",
  "$DataRoot\Sync", "$DataRoot\Benchmark", "$DataRoot\Compat"
)

Write-Host "===== 初始化 Data 目录树 $DataRoot =====" -ForegroundColor Cyan
foreach ($d in $Dirs) {
  New-Item -ItemType Directory -Force -Path $d | Out-Null
  Write-Host "  OK $d" -ForegroundColor Green
}

# 配置随盘走: path.env / permissions / shortcuts
$envSrc = "$PSScriptRoot\Config\path.env"
if (Test-Path $envSrc) {
  $envDst = "$DataRoot\Env\path.env"
  Copy-Item $envSrc $envDst -Force
  Write-Host "  OK $envDst (环境变量模板)" -ForegroundColor Green
}
foreach ($json in @("permissions.json","shortcuts.json","plugin-market.json")) {
  $src = Join-Path $PSScriptRoot "Config\$json"
  if (Test-Path $src) {
    $dst = "$DataRoot\Config\$json"
    Copy-Item $src $dst -Force
    Write-Host "  OK $dst" -ForegroundColor Green
  }
}

# 注册表随盘: 生成"可挂载"的最小合法 Hive(空文件无法被 RegLoadKey/reg load 识别)。
# 做法: 在 HKCU 下种一个临时键, reg save 导出为独立 Hive 文件, 再删临时键。
$seedKey = "HKCU:\VariableHiveSeed"
if (-not (Test-Path "$DataRoot\Registry\User.dat")) {
  try {
    New-Item -Path $seedKey -Force -ErrorAction Stop | Out-Null
    & reg.exe save "HKCU\VariableHiveSeed" "$DataRoot\Registry\User.dat" /y 2>$null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path "$DataRoot\Registry\User.dat")) { throw "reg save 失败 (exit $LASTEXITCODE)" }
    Write-Host "  OK $DataRoot\Registry\User.dat (合法最小 Hive, 可 reg load/RegLoadKey)" -ForegroundColor Green
  } catch {
    New-Item -ItemType File -Force -Path "$DataRoot\Registry\User.dat" | Out-Null
    Write-Warning "  Hive 种子失败($($_.Exception.Message)), 已放占位文件; 挂载前请重跑本脚本或用 reg save 手工生成"
  } finally {
    Remove-Item $seedKey -Recurse -Force -ErrorAction SilentlyContinue
  }
} else {
  # 已存在: 校验是否合法 Hive(文件头 bRegHive 魔数 'regf'), 空占位文件则尝试补种
  $fs = [IO.File]::OpenRead("$DataRoot\Registry\User.dat")
  try {
    $magic = New-Object byte[] 4
    [void]$fs.Read($magic, 0, 4)
  } finally { $fs.Close() }
  if ([Text.Encoding]::ASCII.GetString($magic) -ne "regf") {
    Write-Warning "  $DataRoot\Registry\User.dat 不是合法 Hive(缺少 regf 头), 尝试重新种种子..."
    Remove-Item "$DataRoot\Registry\User.dat" -Force -ErrorAction SilentlyContinue
    try {
      New-Item -Path $seedKey -Force -ErrorAction Stop | Out-Null
      & reg.exe save "HKCU\VariableHiveSeed" "$DataRoot\Registry\User.dat" /y 2>$null
      if ($LASTEXITCODE -eq 0) { Write-Host "  OK 已补种合法 Hive" -ForegroundColor Green }
    } catch { Write-Warning "  补种失败, 保留原文件" } finally {
      Remove-Item $seedKey -Recurse -Force -ErrorAction SilentlyContinue
    }
  } else {
    Write-Host "  OK $DataRoot\Registry\User.dat (已有合法 Hive)" -ForegroundColor Green
  }
}

if ($FixLinks) {
  Write-Host "===== 创建 C 盘符号链接(实体在 Data\\Apps) =====" -ForegroundColor Cyan
  $apps = Get-ChildItem "$DataRoot\Apps" -Directory -ErrorAction SilentlyContinue
  foreach ($app in $apps) {
    $target = $app.FullName
    $link = "C:\Program Files\$($app.Name)"
    New-Item -ItemType Directory -Force -Path "C:\Program Files" | Out-Null
    if (Test-Path $link) { Remove-Item $link -Recurse -Force -ErrorAction SilentlyContinue }
    New-Item -ItemType Junction -Path $link -Target $target | Out-Null
    Write-Host "  OK $link -> $target" -ForegroundColor Green
  }
}

Write-Host ">>> Data 初始化完成。下一步: Security-Manager.ps1 -Action Apply 启用加密防御。" -ForegroundColor Green
