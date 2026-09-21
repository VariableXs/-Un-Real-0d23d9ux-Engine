<#
.SYNOPSIS
  VARIX 三体系统 · AI-2 任务 S3.1 —— W2 三级差分链（Base->Apps->User）
  创建 / 体检 / 回滚 / 回滚演练 x10（幂等；容量配置化；三后端 Diskpart|HyperV|Mock）。

.DESCRIPTION
  布局契约（与 src-tauri/src/shell/engine.rs 的 EngineConfig::discover 一致）：
    <WIN_ENGINE 卷>\Engine\Base.vhdx（只读封存）<- Apps.vhdx <- User.vhdx
  - 卷定位：按标签 WIN_ENGINE 找卷（绝不按盘符猜——部署纪律同源）。
  - 幂等：New 对已存在层跳过；重复执行全链状态一致。
  - 回滚：Reset 重置差分层（Base 只读永不改写；Reset apps 连带重建 user）。
  - 演练：Drill 每轮对 Base/Apps 做前后哈希对比，任何一轮不一致即 FAIL。
  - 后端：Diskpart（默认，无 Hyper-V 依赖）/ HyperV（New-VHD）/ Mock
    （普通文件模拟层与父引用，零管理员，逻辑与真后端共用同一套代码路径）。

.EXAMPLE
  .\Engine-Chain.ps1 -Action Plan
  .\Engine-Chain.ps1 -Action New
  .\Engine-Chain.ps1 -Action Health
  .\Engine-Chain.ps1 -Action Reset -Layer user
  .\Engine-Chain.ps1 -Action Drill -Rounds 10
  .\Engine-Chain.ps1 -Action Drill -Rounds 10 -Backend Mock -OutDir D:\tmp\engine-chain

.NOTES
  编码纪律：UTF-8 BOM；PowerShell 5.1 兼容（不用 &&/?:/??）；盘上只写
  WIN_ENGINE 卷的 Engine 目录，绝不触碰内置盘与其余四分区。
#>
[CmdletBinding()]
param(
  [ValidateSet('Plan', 'New', 'Health', 'Reset', 'Drill')]
  [string]$Action = 'Plan',
  [ValidateSet('user', 'apps')]
  [string]$Layer = 'user',
  [int]$Rounds = 10,
  [ValidateSet('Diskpart', 'HyperV', 'Mock')]
  [string]$Backend = 'Diskpart',
  [string]$ConfigPath = '',
  [string]$OutDir = ''
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- 配置与落点

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $ConfigPath) { $ConfigPath = Join-Path $PSScriptRoot 'engine-chain.json' }
if (-not (Test-Path -LiteralPath $ConfigPath)) { throw "配置缺失：$ConfigPath" }
$Cfg = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json
if ($Cfg.version -ne 1) { throw "engine-chain.json version 必须=1（当前 $($Cfg.version)）" }

function Find-EngineVolume {
  param([string]$Label)
  $v = Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($v -and $v.DriveLetter) { return ("$($v.DriveLetter):") }
  return $null
}

function Resolve-EngineRoot {
  $vol = Find-EngineVolume -Label $Cfg.volumeLabel
  if (-not $vol) { throw "未找到 $($Cfg.volumeLabel) 卷：U 盘未插入或分区缺失（三重确认纪律：绝不猜盘符）" }
  return (Join-Path $vol $Cfg.engineDir)
}

function Get-LayerPaths {
  param([string]$Root)
  return [pscustomobject]@{
    Base = Join-Path $Root $Cfg.base.file
    Apps = Join-Path $Root $Cfg.apps.file
    User = Join-Path $Root $Cfg.user.file
  }
}

function Test-Admin {
  $p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
  return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# ---------------------------------------------------------------- 层操作（三后端统一入口）

function New-LayerFile {
  param([string]$Path, [string]$Parent, [int]$SizeGB, [string]$Type)
  if ($script:BackendKind -eq 'Mock') {
    # Mock：普通文本文件模拟层；内容=父层标记快照（模拟差分视图的父依赖）。
    $marker = if ($Parent) { "layer|parent=" + [IO.Path]::GetFileName($Parent) } else { "layer|base" }
    [IO.File]::WriteAllText($Path, $marker, [Text.UTF8Encoding]::new($false))
    return
  }
  if ($script:BackendKind -eq 'HyperV') {
    if (-not (Get-Module -ListAvailable -Name Hyper-V)) { throw 'Hyper-V 模块不可用（-Backend HyperV 需启用 Hyper-V）' }
    Import-Module Hyper-V
    if ($Parent) { New-VHD -Path $Path -ParentPath $Parent -Differencing | Out-Null }
    elseif ($Type -eq 'fixed') { New-VHD -Path $Path -SizeBytes (($SizeGB * 1GB)) -Fixed | Out-Null }
    else { New-VHD -Path $Path -SizeBytes (($SizeGB * 1GB)) -Dynamic | Out-Null }
    return
  }
  # Diskpart：脚本文件必须 ASCII+单行，写临时脚本执行（diskpart 需管理员）。
  $dp = Join-Path $env:TEMP ("ec-dp-" + [Guid]::NewGuid().ToString('N') + ".txt")
  try {
    $lines = @('create vdisk file="' + $Path + '"')
    if ($Parent) { $lines += 'parent="' + $Parent + '"' }
    else {
      $lines += 'maximum=' + ($SizeGB * 1024)
      if ($Type -ne 'fixed') { $lines += 'type=expandable' }
    }
    [IO.File]::WriteAllLines($dp, $lines)
    $out = & diskpart /s $dp 2>&1
    if ($LASTEXITCODE -ne 0) { throw "diskpart 创建失败（exit=$LASTEXITCODE）：$out" }
  }
  finally { Remove-Item -LiteralPath $dp -Force -ErrorAction SilentlyContinue }
}

function Remove-LayerFile {
  param([string]$Path)
  if (Test-Path -LiteralPath $Path) { Remove-Item -LiteralPath $Path -Force }
}

function Set-BaseSealed {
  param([string]$Path, [bool]$Sealed)
  if ($script:BackendKind -eq 'Mock') {
    if (Test-Path -LiteralPath $Path) { (Get-Item -LiteralPath $Path -Force).IsReadOnly = $Sealed }
    return
  }
  if (Test-Path -LiteralPath $Path) { Set-ItemProperty -LiteralPath $Path -Name IsReadOnly -Value $Sealed }
}

function Get-FileHashSafe {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return $null }
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
}

# ---------------------------------------------------------------- 动作

$script:BackendKind = $Backend
if ($Backend -eq 'Mock' -and $OutDir) {
  # Mock 演练落点覆盖：模拟文件绝不写上真卷（WIN_ENGINE 在场也不许）。
  $root = $OutDir
  if (-not (Test-Path -LiteralPath $root)) { New-Item -ItemType Directory -Path $root -Force | Out-Null }
} else {
  try { $root = Resolve-EngineRoot } catch { if ($Action -ne 'Plan') { throw }; $root = $null }
}
$paths = $null
if ($root) { $paths = Get-LayerPaths -Root $root }

if ($Action -eq 'Plan') {
  Write-Host ">>> S3.1 三级差分链 Plan（后端=$Backend）" -ForegroundColor Cyan
  $vol = Find-EngineVolume -Label $Cfg.volumeLabel
  Write-Host ("  卷: {0}（label={1}）" -f $(if ($vol) { $vol } else { '<不在场>' }), $Cfg.volumeLabel)
  if ($paths) {
    Write-Host ("  Base: {0}  sizeGB={1} type={2}" -f $paths.Base, $Cfg.base.sizeGB, $Cfg.base.type)
    Write-Host ("  Apps: {0}（差分于 Base）" -f $paths.Apps)
    Write-Host ("  User: {0}（差分于 Apps）" -f $paths.User)
    foreach ($p in @($paths.Base, $paths.Apps, $paths.User)) {
      $ex = Test-Path -LiteralPath $p
      Write-Host ("    存在={0}  {1}" -f $ex, $p)
    }
  } else {
    Write-Host "  卷不在场：仅预演配置（零写入）" -ForegroundColor Yellow
  }
  if ($Backend -ne 'Mock' -and -not (Test-Admin)) {
    Write-Host "  当前非管理员：New/Reset/Drill（真后端）需提权后执行" -ForegroundColor Yellow
  }
  exit 0
}

if (-not $paths) { throw "未找到 $($Cfg.volumeLabel) 卷，无法执行 $Action" }
if ($Backend -ne 'Mock' -and -not (Test-Admin)) { throw "动作 $Action（后端 $Backend）需要管理员权限" }
if (-not (Test-Path -LiteralPath $root)) { New-Item -ItemType Directory -Path $root -Force | Out-Null }

if ($Action -eq 'New') {
  Write-Host ">>> S3.1 三级差分链 New（后端=$Backend root=$root）" -ForegroundColor Cyan
  foreach ($pair in @(@('Base', $null, $paths.Base), @('Apps', $paths.Base, $paths.Apps), @('User', $paths.Apps, $paths.User))) {
    $name = $pair[0]; $parent = $pair[1]; $file = $pair[2]
    if (Test-Path -LiteralPath $file) { Write-Host "    $name 已存在：$file（幂等跳过）" -ForegroundColor Yellow; continue }
    if ($parent -and -not (Test-Path -LiteralPath $parent)) { throw "父层缺失：$parent（先建 $($name) 的父层）" }
    $sizeGB = 0; $type = 'dynamic'
    if ($name -eq 'Base') { $sizeGB = $Cfg.base.sizeGB; $type = $Cfg.base.type }
    New-LayerFile -Path $file -Parent $parent -SizeGB $sizeGB -Type $type
    Write-Host "    $name 已建立：$file$(if ($parent) { '（父：' + [IO.Path]::GetFileName($parent) + '）' })" -ForegroundColor Green
  }
  Set-BaseSealed -Path $paths.Base -Sealed $true
  Write-Host ">>> 链就绪：Base(只读封存) <- Apps <- User；VM 只挂 User 层" -ForegroundColor Green
}
elseif ($Action -eq 'Health') {
  Write-Host ">>> S3.1 三级差分链 Health" -ForegroundColor Cyan
  $fail = 0
  foreach ($p in @($paths.Base, $paths.Apps, $paths.User)) {
    $ok = Test-Path -LiteralPath $p
    if (-not $ok) { $fail += 1 }
    Write-Host ("  {0} {1}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $p) -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
  }
  $sealed = (Get-Item -LiteralPath $paths.Base -Force).IsReadOnly
  Write-Host ("  {0} Base 只读封存: {1}" -f $(if ($sealed) { 'PASS' } else { 'WARN' }), $sealed) -ForegroundColor $(if ($sealed) { 'Green' } else { 'Yellow' })
  if ($fail -gt 0) { exit 1 }
}
elseif ($Action -eq 'Reset') {
  Write-Host ">>> S3.1 回滚：重置 $Layer 层（后端=$Backend）" -ForegroundColor Cyan
  $target = if ($Layer -eq 'apps') { $paths.Apps } else { $paths.User }
  Remove-LayerFile -Path $target
  $newParent = if ($Layer -eq 'apps') { $paths.Base } else { $paths.Apps }
  New-LayerFile -Path $target -Parent $newParent -SizeGB 0 -Type 'dynamic'
  if ($Layer -eq 'apps') {
    # 重置 apps 层会使 user 层父引用失效 —— user 层连带重建（Layer-Chain 既有语义）。
    Remove-LayerFile -Path $paths.User
    New-LayerFile -Path $paths.User -Parent $paths.Apps -SizeGB 0 -Type 'dynamic'
    Write-Host "    user 层已连带重建" -ForegroundColor Green
  }
  Set-BaseSealed -Path $paths.Base -Sealed $true
  Write-Host ">>> $Layer 层已重置（Base 与数据不受影响）" -ForegroundColor Green
}
elseif ($Action -eq 'Drill') {
  if ($Rounds -lt 1) { throw "Rounds 必须 >=1（验收口径 x10）" }
  Write-Host ">>> S3.1 回滚演练 x$Rounds（后端=$Backend；每轮 Base/Apps 哈希前后必须一致）" -ForegroundColor Cyan
  if (-not (Test-Path -LiteralPath $paths.Apps)) { throw "Apps 层缺失：先执行 -Action New" }
  $pass = 0
  for ($i = 1; $i -le $Rounds; $i++) {
    $hBase0 = Get-FileHashSafe -Path $paths.Base
    $hApps0 = Get-FileHashSafe -Path $paths.Apps
    Remove-LayerFile -Path $paths.User
    New-LayerFile -Path $paths.User -Parent $paths.Apps -SizeGB 0 -Type 'dynamic'
    Set-BaseSealed -Path $paths.Base -Sealed $true
    $hBase1 = Get-FileHashSafe -Path $paths.Base
    $hApps1 = Get-FileHashSafe -Path $paths.Apps
    $ok = ($hBase0 -eq $hBase1) -and ($hApps0 -eq $hApps1) -and (Test-Path -LiteralPath $paths.User)
    if ($ok) { $pass += 1 }
    Write-Host ("  轮 {0}/{1}: Base不变={2} Apps不变={3} User重建={4} -> {5}" -f `
      $i, $Rounds, ($hBase0 -eq $hBase1), ($hApps0 -eq $hApps1), (Test-Path -LiteralPath $paths.User), $(if ($ok) { 'PASS' } else { 'FAIL' })) `
      -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
    if (-not $ok) { throw "第 $i 轮回滚破坏了 Base/Apps（零变砖红线被触发，立即停止）" }
  }
  Write-Host ">>> 演练 $pass/$Rounds PASS（Base/Apps 每轮哈希不变）" -ForegroundColor Green
}
