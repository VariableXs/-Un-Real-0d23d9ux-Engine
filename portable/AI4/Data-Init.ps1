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

# 注册表随盘: Data/Registry 下空 User.dat 占位(实际 RegLoadKey 由 Core 在启动时执行)
if (-not (Test-Path "$DataRoot\Registry\User.dat")) {
  New-Item -ItemType File -Force -Path "$DataRoot\Registry\User.dat" | Out-Null
  Write-Host "  OK $DataRoot\Registry\User.dat (占位, Core 启动 RegLoadKey)" -ForegroundColor Green
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
