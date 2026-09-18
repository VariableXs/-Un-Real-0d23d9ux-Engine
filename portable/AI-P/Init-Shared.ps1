<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务 9 —— SHARED 分区目录契约初始化 + apps.json schema 定版

.DESCRIPTION
  对应 docs/VARIX双域系统施工总案.md 阶段 1 步骤 4：
    SHARED 契约骨架：apps.json / boot-select.json / whitelist\whitelist.json / handoff\
    - apps.json 带 version 字段与迁移函数占位（开放性：升版演练走通）
    - 幂等：已有契约文件不覆盖，仅校验
    - 损坏 JSON 走默认：损坏文件改名 *.corrupt-<时间戳> 留证后重建（总案：损坏走默认）
    - -ValidateOnly：只校验不修改（测试与部署 Verify 段复用）
  契约细节与三方表（谁写/谁读）见 portable\AI-P\README.md。

.EXAMPLE
  .\Init-Shared.ps1 -SharedRoot S:\
  .\Init-Shared.ps1 -SharedRoot D:\tmp\shared-test -ValidateOnly

.NOTES
  编码纪律：UTF-8 BOM；不动 SHARED 卷上契约目录之外的任何文件。
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$SharedRoot,
  [switch]$ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step { param([string]$Msg) Write-Host ">>> $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "    + $Msg" -ForegroundColor Green }
function Write-Note { param([string]$Msg) Write-Host "    ! $Msg" -ForegroundColor Yellow }

# 内置默认契约（总案阶段 1 定版内容）
$script:DefaultApps = [pscustomobject]@{
  version = 1
  updated = ""
  apps    = @()
}
$script:DefaultBootSelect = [pscustomobject]@{
  default_entry    = "variable"
  timeout_sec      = 5
  show_menu        = $true
  last_boot        = ""
  windows_bootnext = -1
}
$script:DefaultWhitelist = [pscustomobject]@{
  version = 1
  rules   = @()
}

# 读取 JSON；损坏/缺失返回 $null（调用方决定走默认）
function Read-JsonSafe {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return $null }
  try {
    return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
  }
  catch {
    return $null
  }
}

# apps.json schema 校验（定版规则；-ValidateOnly 与初始化后自检共用）
# 返回错误清单，空表 = 通过
function Test-AppsJsonSchema {
  param([Parameter(Mandatory = $true)]$Obj)
  $errs = @()
  if ($null -eq $Obj) { return @('apps.json 整体损坏（无法解析）') }
  if (-not $Obj.PSObject.Properties['version']) { $errs += '缺 version 字段' }
  elseif ([int]$Obj.version -lt 1) { $errs += "version 非法：$($Obj.version)" }
  if (-not $Obj.PSObject.Properties['apps']) { $errs += '缺 apps 数组' }
  else {
    $allowedChannel = @('wine', 'engine', 'native-only')
    $allowedTier = @('ok', 'partial', 'blocked')
    $ids = @()
    foreach ($a in @($Obj.apps)) {
      if (-not $a.PSObject.Properties['id'] -or -not $a.id) { $errs += '存在缺 id 的条目'; continue }
      if ($ids -contains $a.id) { $errs += "id 重复：$($a.id)" }
      $ids += $a.id
      if (-not $a.PSObject.Properties['name'] -or -not $a.name) { $errs += "$($a.id) 缺 name" }
      if (-not $a.PSObject.Properties['channel'] -or $allowedChannel -notcontains $a.channel) {
        $errs += "$($a.id) channel 非法（允许 wine/engine/native-only）"
      }
      if (-not $a.PSObject.Properties['tier'] -or $allowedTier -notcontains $a.tier) {
        $errs += "$($a.id) tier 非法（允许 ok/partial/blocked）"
      }
    }
  }
  return @($errs)
}

# 版本迁移占位（开放性契约：schema 升版时在此追加分支，不改主逻辑）
function Move-AppsJsonVersion {
  param([Parameter(Mandatory = $true)]$Obj)
  switch ([int]$Obj.version) {
    1 { return $Obj }          # 当前定版
    default {
      throw "apps.json version=$($Obj.version) 高于本工具支持上限 1，请升级 Init-Shared.ps1"
    }
  }
}

$appsPath = Join-Path $SharedRoot 'apps.json'
$bootPath = Join-Path $SharedRoot 'boot-select.json'
$wlPath = Join-Path $SharedRoot 'whitelist\whitelist.json'
$handoffDir = Join-Path $SharedRoot 'handoff'

if ($ValidateOnly) {
  Write-Step "校验 SHARED 契约：$SharedRoot"
  $fail = @()
  $apps = Read-JsonSafe $appsPath
  if ($null -eq $apps) { $fail += "apps.json 缺失或损坏：$appsPath" }
  else {
    $errs = @(Test-AppsJsonSchema $apps)
    if ($errs.Count -gt 0) { $fail += ($errs -join '; ') } else { Write-Ok "apps.json schema 通过（version=$($apps.version)，$(@($apps.apps)).Count 个条目）" }
  }
  $boot = Read-JsonSafe $bootPath
  if ($null -eq $boot -or -not $boot.PSObject.Properties['default_entry']) { $fail += "boot-select.json 缺失/损坏/缺 default_entry：$bootPath" }
  else { Write-Ok "boot-select.json 通过（default_entry=$($boot.default_entry), timeout=$($boot.timeout_sec)s）" }
  $wl = Read-JsonSafe $wlPath
  if ($null -eq $wl -or -not $wl.PSObject.Properties['rules']) { $fail += "whitelist.json 缺失/损坏/缺 rules：$wlPath" }
  else { Write-Ok "whitelist.json 通过（$(@($wl.rules)).Count 条规则）" }
  if (-not (Test-Path -LiteralPath $handoffDir -PathType Container)) { $fail += "handoff 目录缺失：$handoffDir" }
  else { Write-Ok "handoff 目录就绪" }
  if ($fail.Count -gt 0) {
    $fail | ForEach-Object { Write-Note "  - $_" }
    exit 1
  }
  Write-Ok '契约校验全部通过'
  exit 0
}

# ------------------------------------------------------------- 初始化
Write-Step "初始化 SHARED 契约：$SharedRoot"
if (-not (Test-Path -LiteralPath $SharedRoot -PathType Container)) { throw "SHARED 根不存在：$SharedRoot" }

New-Item -ItemType Directory -Force -Path (Join-Path $SharedRoot 'whitelist') | Out-Null
New-Item -ItemType Directory -Force -Path $handoffDir | Out-Null

# 契约文件统一处理：缺失→写默认；损坏→留证改名后重建；完好→不动（幂等）
$contracts = @(
  @{ Path = $appsPath;    Default = $script:DefaultApps;       Name = 'apps.json' },
  @{ Path = $bootPath;    Default = $script:DefaultBootSelect; Name = 'boot-select.json' },
  @{ Path = $wlPath;      Default = $script:DefaultWhitelist;  Name = 'whitelist.json' }
)
foreach ($c in $contracts) {
  $existing = Read-JsonSafe $c.Path
  if ($null -ne $existing) {
    Write-Ok "$($c.Name) 已存在且可解析，保持不动（幂等）"
    continue
  }
  if (Test-Path -LiteralPath $c.Path) {
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $bad = "$($c.Path).corrupt-$stamp"
    Move-Item -LiteralPath $c.Path -Destination $bad -Force
    Write-Note "$($c.Name) 损坏，已留证改名 $(Split-Path -Leaf $bad) 后重建默认"
  }
  $json = $c.Default | ConvertTo-Json -Depth 8
  # UTF-8 无 BOM 写出（JSON 由内核/Windows 双侧读，统一无 BOM 便于解析器子集实现）
  [IO.File]::WriteAllText($c.Path, $json, [Text.UTF8Encoding]::new($false))
  Write-Ok "已写入默认 $($c.Name)"
}

# 初始化后自检：apps.json 必须过 schema
$apps2 = Read-JsonSafe $appsPath
if ($null -eq $apps2) { throw '初始化后 apps.json 仍不可解析，初始化流程缺陷' }
$errs2 = @(Test-AppsJsonSchema $apps2)
if ($errs2.Count -gt 0) { throw "初始化后 schema 自检失败：$($errs2 -join '; ')" }
Write-Ok 'apps.json schema 自检通过（version=1）'
Write-Host ''
Write-Host '>>> 完成。契约三方表（写者/读者）见 portable\AI-P\README.md' -ForegroundColor Green
# 显式 exit 0：被 Deploy 编排以 & 调用时，PS 脚本正常结束不会更新
# $LASTEXITCODE（残留 $null），编排侧 `$LASTEXITCODE -ne 0` 会误判失败。
exit 0
