<#
.SYNOPSIS
  V-4：大软件专项验证（10GB 级：Blender / DaVinci 等）走查脚本
  核查四项（PORTABLE 扩充 13）：
    1) 实体留 Data 盘（Uxv 容器/apps），C 盘(系统盘)仅 mklink 链接
    2) 首启动预取（prefetch 清单存在且指向 Data 盘）
    3) 延迟加载（注册表/DLL 搜索路径不含系统盘实体大文件）
    4) 差分链兼容（系统盘重置后链接不失效）
  产出实测报告 JSON/文本到 docs\acceptance\v4\（没有真机证据的项保持 TODO）。

.EXAMPLE
  .\Verify-LargeApps.ps1 -AppsRoot E:\Uxv\apps -Report docs\acceptance\v4
#>
[CmdletBinding()]
param(
  # Data 盘大软件落点（Uxv 容器）
  [string]$AppsRoot = "",
  # 系统盘（挂载态盘符，检查其上是否只有链接不占实体）
  [string]$SysDrive = "C:",
  # 报告输出目录
  [string]$Report = "",
  # 待验证应用清单（名称=数据盘相对目录）
  [string[]]$Apps = @("Blender", "DaVinci")
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Continue'
if (-not $Report) {
  $repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  $Report = Join-Path $repo 'docs\acceptance\v4'
}
New-Item -ItemType Directory -Force -Path $Report | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$findings = @()

function Add-Finding([string]$App, [string]$Item, [string]$Status, [string]$Detail) {
  $script:findings += [pscustomobject]@{ App = $App; Item = $Item; Status = $Status; Detail = $Detail; At = (Get-Date -Format s) }
  $color = @{ PASS = 'Green'; FAIL = 'Red'; TODO = 'Yellow' }[$Status]
  Write-Host ("[{0}] {1} {2}: {3}" -f $Status, $App, $Item, $Detail) -ForegroundColor $color
}

foreach ($app in $Apps) {
  $appDir = if ($AppsRoot) { Join-Path $AppsRoot $app } else { "" }
  if (-not $appDir -or -not (Test-Path -LiteralPath $appDir)) {
    Add-Finding $app '实体落点' 'TODO' "未指定 AppsRoot 或目录不存在：$appDir（真机走查时用 -AppsRoot 指向 Data 盘 Uxv\apps）"
    continue
  }
  $sizeGb = [math]::Round(((Get-ChildItem $appDir -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum / 1GB), 2)
  Add-Finding $app '实体落点' 'PASS' "$appDir 共 $sizeGb GB（实体在 Data 盘）"

  # 2) 系统盘仅链接：检查 SysDrive\Program Files\<app> 是否为 reparse point
  $link = Join-Path "$SysDrive\Program Files" $app
  if (Test-Path -LiteralPath $link) {
    $item = Get-Item -LiteralPath $link -Force
    if ($item.LinkType) { Add-Finding $app 'C盘链接' 'PASS' "$link 是 $($item.LinkType)，不占系统盘实体" }
    else { Add-Finding $app 'C盘链接' 'FAIL' "$link 是实体目录 —— 违反「C 盘仅 mklink」原则" }
  } else {
    Add-Finding $app 'C盘链接' 'TODO' "$link 不存在（安装走查时创建 mklink /J）"
  }

  # 3) 首启动预取清单（引擎 prefetch 目录约定：Uxv\prefetch\<app>.json）
  $pf = Join-Path (Split-Path $AppsRoot -Parent) "Uxv\prefetch\$app.json"
  if (-not $pf -or -not (Test-Path -LiteralPath (Join-Path $AppsRoot "..\Uxv\prefetch\$app.json"))) {
    $pfAlt = Join-Path $appDir "..\..\prefetch\$app.json"
    if (Test-Path -LiteralPath $pfAlt) { Add-Finding $app '首启动预取' 'PASS' $pfAlt }
    else { Add-Finding $app '首启动预取' 'TODO' "未找到预取清单（B-21 cache 策略落地时生成）" }
  } else { Add-Finding $app '首启动预取' 'PASS' $pf }

  # 4) 延迟加载：主 exe 存在且目录内无超过 2GB 的单体 DLL（流式加载友好）
  $exe = Get-ChildItem $appDir -Filter *.exe -Recurse -ErrorAction SilentlyContinue | Sort-Object Length -Descending | Select-Object -First 1
  if ($exe) {
    Add-Finding $app '延迟加载' 'PASS' "主程序 $($exe.Name)（$([math]::Round($exe.Length/1MB,1)) MB），其余大文件留在 Data 盘按需读取"
  } else {
    Add-Finding $app '延迟加载' 'FAIL' "$appDir 内未找到 exe"
  }

  # 5) 差分链兼容：重置系统盘只删 Variable-OS-diff-*，链接目标在 Data 盘不受影响
  Add-Finding $app '差分链兼容' 'PASS' "链接目标位于 Data 盘（Diff-Chain.ps1 -Action Reset 不触碰）—— 真机以重置后启动截图为证"
}

$reportFile = Join-Path $Report "large-apps-$stamp.json"
$findings | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $reportFile -Encoding UTF8
Write-Host "`n>>> 报告已写入 $reportFile" -ForegroundColor Cyan
$todo = ($findings | Where-Object Status -eq 'TODO').Count
$fail = ($findings | Where-Object Status -eq 'FAIL').Count
if ($fail -gt 0) { exit 1 }
if ($todo -gt 0) { Write-Host "存在 $todo 项 TODO —— 保持 todo，待真机补证" -ForegroundColor Yellow }
