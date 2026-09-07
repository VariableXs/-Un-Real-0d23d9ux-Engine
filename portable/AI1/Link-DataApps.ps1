<#
.SYNOPSIS
  Variable OS · AI-1 存储核 —— 3.2 读写分离 / 扩充 13.4 Data 分区符号链接

.DESCRIPTION
  把大软件实体留在 Data 分区，系统盘只留链接，VHDX 永不膨胀。
  对应 docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md 3.2、扩充 13.4、9.2（Data 文件系统）。

  关键实现点（详见 docs/AI1-存储.md 第 3.2 节）：
    * Data 为 exFAT 时只能用「目录符号链接」（SymbolicLink），不能用 Junction：
      Junction / 硬链接要求目标卷也是 NTFS。
    * 链接对象建在系统盘（NTFS）一侧，目标可以指向 exFAT 路径。
    * 幂等：重复运行只补缺失/修正指向错误的链接，不重复搬迁数据。
    * -Move 会把系统盘上已安装的真实目录整体搬到 Data 后再建链接。

.PARAMETER DataRoot
  Data 分区根，默认 D:\Data（B 模式即 U 盘的 Data 分区）。

.PARAMETER MapFile
  链接映射表 JSON，默认 <DataRoot>\linkmap.json；不存在时自动生成含 Blender 样例的模板。
  格式：[{ "Link": "C:\\Program Files\\Blender Foundation", "Target": "{Data}\\Apps\\Blender-5.2" }]
  占位符 {Data} 会替换成 -DataRoot，便于换盘符后不改表。

.EXAMPLE
  .\Link-DataApps.ps1 -DataRoot D:\Data
  .\Link-DataApps.ps1 -DataRoot D:\Data -Move        # 已装在 C 盘的软件先搬到 Data
  .\Link-DataApps.ps1 -DataRoot D:\Data -Verify      # 只体检，不改动
  .\Link-DataApps.ps1 -DataRoot D:\Data -Remove      # 卸载：删链接（Data 实体保留）
#>
#Requires -RunAsAdministrator
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
  [string]$DataRoot = 'D:\Data',

  [string]$MapFile,

  [ValidateSet('SymbolicLink', 'Junction', 'Auto')]
  [string]$LinkType = 'Auto',

  [switch]$Move,
  [switch]$Verify,
  [switch]$Remove
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $MapFile) { $MapFile = Join-Path $DataRoot 'linkmap.json' }

# 3.2 的 Data 七目录（与 Create-VHDX.ps1 一致）
$dataDirs = @('Apps', 'MSIX', 'Plugins', 'User', 'Exchange', 'Cache', 'Dumps')

function Write-Ok   { param([string]$Msg) Write-Host "    + $Msg" -ForegroundColor Green }
function Write-Note { param([string]$Msg) Write-Host "    ! $Msg" -ForegroundColor Yellow }
function Write-Bad  { param([string]$Msg) Write-Host "    x $Msg" -ForegroundColor Red }

function Get-LinkKind {
  param($Item)
  if ($null -eq $Item) { return 'Missing' }
  if ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    if ($Item.LinkType) { return $Item.LinkType }
    return 'ReparsePoint'
  }
  if ($Item.PSIsContainer) { return 'RealDir' }
  return 'RealFile'
}

function Test-NtfsVolume {
  param([string]$Path)
  $root = [IO.Path]::GetPathRoot($Path)
  if (-not $root) { return $false }
  $letter = $root.TrimEnd('\', ':')
  if ($letter.Length -ne 1) { return $false }
  $v = Get-Volume -DriveLetter $letter -ErrorAction SilentlyContinue
  if (-not $v) { return $false }
  return ($v.FileSystem -eq 'NTFS')
}

# --------------------------------------------------------------- 0. 前置
Write-Host ">>> 读写分离（主计划 3.2 / 扩充 13.4）DataRoot=$DataRoot" -ForegroundColor Cyan

if (-not (Test-Path -LiteralPath $DataRoot)) {
  if ($Verify) { Write-Bad "Data 根不存在：$DataRoot"; return }
  throw "Data 根不存在：$DataRoot（先跑 Create-VHDX.ps1 或插入 U 盘）"
}
foreach ($d in $dataDirs) {
  New-Item -ItemType Directory -Force -Path (Join-Path $DataRoot $d) | Out-Null
}
$dataIsNtfs = Test-NtfsVolume -Path $DataRoot
Write-Ok ("Data 卷文件系统：{0}" -f $(if ($dataIsNtfs) { 'NTFS（可用 Junction）' } else { 'exFAT/FAT（仅可用 SymbolicLink）' }))

# 建符号链接需要 SeCreateSymbolicLinkPrivilege（管理员或开发者模式）
$canSymlink = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
  [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $canSymlink) { Write-Note '当前非管理员：无 SeCreateSymbolicLinkPrivilege 时将建链失败（或需开启开发者模式）' }

# ---------------------------------------------------------- 1. 映射表
$defaultMap = @(
  @{ Link = 'C:\Program Files\Blender Foundation'; Target = '{Data}\Apps\Blender-5.2' }
  @{ Link = 'C:\ProgramData\Blender Foundation';  Target = '{Data}\Apps\Blender-5.2\config' }
  @{ Link = 'C:\Users\Public\Documents\Variable'; Target = '{Data}\User\Documents' }
  @{ Link = 'C:\ProgramData\Variable\Dumps';     Target = '{Data}\Dumps' }
  @{ Link = 'C:\ProgramData\Variable\Cache';     Target = '{Data}\Cache' }
)

if (-not (Test-Path -LiteralPath $MapFile)) {
  $tpl = $defaultMap | ForEach-Object { [pscustomobject]@{ Link = $_.Link; Target = $_.Target } }
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $MapFile) | Out-Null
  $tpl | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $MapFile -Encoding UTF8
  Write-Ok "已生成模板映射表：$MapFile（Blender 样例，按需增删）"
}

$map = @(Get-Content -LiteralPath $MapFile -Raw | ConvertFrom-Json)
if ($map.Count -eq 0) { Write-Note '映射表为空，无可执行项'; return }
Write-Ok "映射表 $MapFile：$($map.Count) 条"

# ---------------------------------------------------------- 2. 逐条处理
$rows = New-Object System.Collections.Generic.List[object]

foreach ($entry in $map) {
  $link = $entry.Link
  $target = ($entry.Target -replace '\{Data\}', $DataRoot.TrimEnd('\'))
  $kind = Get-LinkKind -Item (Get-Item -LiteralPath $link -Force -ErrorAction SilentlyContinue)
  $action = 'skip'
  $status = 'warn'   # ok / warn / bad，逐条显式赋值，不靠字符串匹配统计

  if ($Verify) {
    $ok = ($kind -in @('SymbolicLink', 'Junction', 'ReparsePoint')) -and (Test-Path -LiteralPath $link)
    if ($kind -eq 'Missing') { $action = 'missing'; $status = 'bad' }
    elseif (-not $ok) { $action = "broken($kind)"; $status = 'bad' }
    else { $action = 'ok'; $status = 'ok' }
  }
  elseif ($Remove) {
    if ($kind -in @('SymbolicLink', 'Junction', 'ReparsePoint')) {
      if ($PSCmdlet.ShouldProcess($link, '删除链接（Data 实体保留）')) {
        # 只删链接本身：非递归删除，避免穿透删掉 Data 里的实体
        [IO.Directory]::Delete($link, $false)
        $action = 'removed'; $status = 'ok'
      }
    }
    else { $action = 'notlink'; $status = 'warn' }
  }
  else {
    if ($kind -eq 'RealDir' -and $Move) {
      New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
      if ($PSCmdlet.ShouldProcess("$link -> $target", '搬迁实体到 Data 并建链')) {
        Move-Item -LiteralPath $link -Destination $target -Force
        $action = 'moved'; $status = 'ok'
        $kind = 'Missing'   # 实体已搬走，下面继续建链
      }
    }
    elseif ($kind -eq 'RealDir') {
      $action = 'realdir(用 -Move 搬迁)'; $status = 'warn'
    }
    elseif ($kind -in @('SymbolicLink', 'Junction', 'ReparsePoint')) {
      $cur = $null
      try { $cur = (Get-Item -LiteralPath $link -Force).Target } catch { $cur = $null }
      if ($cur -and ($cur -join '') -eq $target) { $action = 'exists'; $status = 'ok' }
      elseif ($PSCmdlet.ShouldProcess($link, "重建链接 -> $target")) {
        [IO.Directory]::Delete($link, $false)
        $kind = 'Missing'
        $action = 'relink'; $status = 'ok'
      }
    }

    if ($kind -eq 'Missing' -and $action -notlike 'realdir*') {
      if (-not (Test-Path -LiteralPath $target)) {
        New-Item -ItemType Directory -Force -Path $target | Out-Null
        $action = "$action+mkdir"
      }
      $want = $LinkType
      if ($want -eq 'Auto') { $want = $(if ($dataIsNtfs) { 'Junction' } else { 'SymbolicLink' }) }
      if ($want -eq 'Junction' -and -not $dataIsNtfs) {
        Write-Note "$link 目标在 exFAT 卷，Junction 不支持，回退 SymbolicLink"
        $want = 'SymbolicLink'
      }
      if ($PSCmdlet.ShouldProcess($link, "新建 $want -> $target")) {
        New-Item -ItemType $want -Path $link -Target $target -Force | Out-Null
        $action = "$action/$want"; $status = 'ok'
      }
    }
  }

  $rows.Add([pscustomobject]@{
      Link = $link; Target = $target; Kind = $kind; Action = $action; Status = $status
    })
}

$rows | Format-Table -AutoSize | Out-Host
$done = @($rows | Where-Object { $_.Status -eq 'ok' }).Count
$bad = @($rows | Where-Object { $_.Status -eq 'bad' }).Count
$warn = @($rows | Where-Object { $_.Status -eq 'warn' }).Count
Write-Host (">>> 完成 {0} 条 / 待处理 {1} 条 / 异常 {2} 条（共 {3}）；验收：装 Blender 到 Data 后 Variable-OS.vhdx 大小不变" -f `
    $done, $warn, $bad, $rows.Count) -ForegroundColor $(if ($bad -gt 0) { 'Yellow' } else { 'Green' })
