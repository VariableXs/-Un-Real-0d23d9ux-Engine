<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务 11 —— 多 U 盘版本管理：差分版本命名与回收站区约定

.DESCRIPTION
  对应 docs/VARIX双域系统施工总案.md 阶段 1 步骤 8（为阶段 9 升级协议打地基）：
    - 版本命名：vMAJOR.MINOR（MANIFEST.json 的 version + label），单调递增校验
    - MANIFEST.json：盘根版本清单（version/label/files 文件哈希），Snapshot 动作生成
    - New-Diff：相邻版本文件差集 -> 差分升级包清单（version diff JSON，阶段 9 打包的输入）
    - Retire：退役旧版本文件移动到盘根 _trash\<旧版本>-<时间戳>\（回收站区约定，不删除）
    - Compare：两盘 MANIFEST 的 files 哈希逐一比对 -> Base 相同性结论
  -Root 可指向 U 盘根或普通目录（测试通用同一代码路径）。

.EXAMPLE
  .\Manage-Versions.ps1 -Action Snapshot -Root E:\ -Label 'first-burn'
  .\Manage-Versions.ps1 -Action New-Diff  -Root E:\
  .\Manage-Versions.ps1 -Action Compare   -Root E:\ -OtherRoot F:\
  .\Manage-Versions.ps1 -Action Retire    -Root E:\ -Files kernel\varix
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet('Snapshot', 'New-Diff', 'Compare', 'Retire', 'Show')]
  [string]$Action,
  [Parameter(Mandatory = $true)][string]$Root,
  [string]$OtherRoot = "",
  [string]$Label = "",
  [string[]]$Files = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Ok { param([string]$Msg) Write-Host "    + $Msg" -ForegroundColor Green }
function Write-Note { param([string]$Msg) Write-Host "    ! $Msg" -ForegroundColor Yellow }

$rootPath = $Root.TrimEnd('\') + '\'
if (-not (Test-Path -LiteralPath $rootPath -PathType Container)) { throw "根不存在：$rootPath" }
$manifestPath = Join-Path $rootPath 'MANIFEST.json'

function Get-Manifest {
  param([string]$Base)
  $p = Join-Path $Base 'MANIFEST.json'
  if (-not (Test-Path -LiteralPath $p)) { return $null }
  return Get-Content -LiteralPath $p -Raw | ConvertFrom-Json
}

function Get-FileMap {
  # 注意：循环变量不能用与参数仅大小写不同的名字（PS 变量不区分大小写，会自写自读）
  param([string]$Base, [string[]]$RelPaths)
  $map = @{}
  foreach ($relPath in $RelPaths) {
    $p = Join-Path $Base $relPath
    if (-not (Test-Path -LiteralPath $p)) { throw "清单内文件缺失：$relPath" }
    $map[$relPath] = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash
  }
  return $map
}

switch ($Action) {
  'Snapshot' {
    # 默认快照范围：部署关键产物（存在才收）
    $targets = @('limine.conf', 'limine-bios.sys', 'EFI\BOOT\BOOTX64.EFI', 'kernel\varix',
      'ESP-MANIFEST.json', 'apps.json', 'boot-select.json', 'whitelist\whitelist.json')
    if ($Files.Count -gt 0) { $targets = $Files }
    $existing = @($targets | Where-Object { Test-Path -LiteralPath (Join-Path $rootPath $_) })
    if ($existing.Count -eq 0) { throw '快照范围内没有任何文件（盘未部署？）' }
    $prev = Get-Manifest $rootPath
    if ($prev) {
      # 单调递增：默认 MINOR+1；-Label 提供 'vX.Y' 可跳版
      $v = [version]$prev.version
      $newV = if ($Label -match '^v?(\d+)\.(\d+)$') { "$($Matches[1]).$($Matches[2])" } else { "$($v.Major).$($v.Minor + 1)" }
      if ([version]$newV -le $v) { throw "版本必须单调递增：$newV <= $($prev.version)" }
    }
    else { $newV = if ($Label -match '^v?(\d+)\.(\d+)$') { "$($Matches[1]).$($Matches[2])" } else { '1.0' } }
    $map = Get-FileMap $rootPath $existing
    $mf = [pscustomobject]@{
      version = $newV
      label   = $Label
      taken   = (Get-Date -Format 'o')
      files   = @($existing | ForEach-Object { [pscustomobject]@{ file = $_; sha256 = $map[$_] } })
    }
    [IO.File]::WriteAllText($manifestPath, ($mf | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    # 历史同步落账（New-Diff 的 from 基线来源；同版本重盖）
    $histPath = Join-Path $rootPath '_versions\history.json'
    New-Item -ItemType Directory -Force -Path (Split-Path $histPath -Parent) | Out-Null
    $hist = @()
    if (Test-Path -LiteralPath $histPath) {
      # PS5.1 管道中 ConvertFrom-Json 对顶层 JSON 数组输出单个 Object[]，先落变量再 @() 展开
      $parsed = Get-Content -LiteralPath $histPath -Raw | ConvertFrom-Json
      $hist = @($parsed)
    }
    $hist = @(@($hist) | Where-Object { $_.version -ne $mf.version }) + [pscustomobject]@{ version = $mf.version; label = $mf.label; taken = $mf.taken; files = $mf.files }
    [IO.File]::WriteAllText($histPath, ($hist | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    Write-Ok "MANIFEST.json 版本 v$newV（$($existing.Count) 个文件入清单）"
  }
  'New-Diff' {
    $mf = Get-Manifest $rootPath
    if (-not $mf) { throw '当前盘无 MANIFEST.json，先 Snapshot' }
    $histPath = Join-Path $rootPath '_versions\history.json'
    $hist = @()
    if (Test-Path -LiteralPath $histPath) {
      # PS5.1 管道中 ConvertFrom-Json 对顶层 JSON 数组输出单个 Object[]，先落变量再 @() 展开
      $parsed = Get-Content -LiteralPath $histPath -Raw | ConvertFrom-Json
      $hist = @($parsed)
    }
    # from 基线 = 当前版本之前最近的历史版本（历史含当前版本，需排除）
    $prevEntry = @(@($hist) | Where-Object { $_.version -ne $mf.version }) | Select-Object -Last 1
    $cur = @{}
    foreach ($e in @($mf.files)) { $cur[$e.file] = $e.sha256 }
    $diff = [pscustomobject]@{
      from    = if ($prevEntry) { $prevEntry.version } else { $null }
      to      = $mf.version
      added   = @()
      changed = @()
      removed = @()
    }
    if ($prevEntry) {
      $prevMap = @{}
      foreach ($e in @($prevEntry.files)) { $prevMap[$e.file] = $e.sha256 }
      foreach ($k in $cur.Keys) {
        if (-not $prevMap.ContainsKey($k)) { $diff.added = @($diff.added) + $k }
        elseif ($prevMap[$k] -ne $cur[$k]) { $diff.changed = @($diff.changed) + $k }
      }
      foreach ($k in $prevMap.Keys) { if (-not $cur.ContainsKey($k)) { $diff.removed = @($diff.removed) + $k } }
    }
    New-Item -ItemType Directory -Force -Path (Split-Path $histPath -Parent) | Out-Null
    $hist += [pscustomobject]@{ version = $mf.version; label = $mf.label; taken = $mf.taken; files = $mf.files }
    $diffPath = Join-Path $rootPath ('_versions\diff-' + $diff.from + '-' + $diff.to + '.json')
    [IO.File]::WriteAllText($diffPath, ($diff | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    Write-Ok ("差分包清单 {0}：新增 {1} / 变更 {2} / 删除 {3}" -f `
        (Split-Path -Leaf $diffPath), @($diff.added).Count, @($diff.changed).Count, @($diff.removed).Count)
    Write-Note '升级包实体打包（压缩/签名）由阶段 9 任务 66 接手，本步产出的是其输入清单'
  }
  'Compare' {
    if (-not $OtherRoot) { throw 'Compare 需要 -OtherRoot' }
    $a = Get-Manifest $rootPath
    $b = Get-Manifest ($OtherRoot.TrimEnd('\') + '\')
    if (-not $a -or -not $b) { throw '两侧均需 MANIFEST.json（先 Snapshot）' }
    $ma = @{}; foreach ($e in @($a.files)) { $ma[$e.file] = $e.sha256 }
    $mb = @{}; foreach ($e in @($b.files)) { $mb[$e.file] = $e.sha256 }
    $diffs = 0
    foreach ($k in $ma.Keys) {
      if (-not $mb.ContainsKey($k)) { Write-Note "仅 $Root 有：$k"; $diffs++ }
      elseif ($ma[$k] -ne $mb[$k]) { Write-Note "哈希不同：$k"; $diffs++ }
    }
    foreach ($k in $mb.Keys) { if (-not $ma.ContainsKey($k)) { Write-Note "仅 $OtherRoot 有：$k"; $diffs++ } }
    if ($diffs -eq 0) { Write-Ok "Base 相同：v$($a.version) 与 v$($b.version) 全部 $($ma.Count) 文件哈希一致" }
    else { Write-Note "Base 不同：$diffs 处差异（以上清单）"; exit 1 }
  }
  'Retire' {
    if ($Files.Count -eq 0) { throw 'Retire 需要 -Files（相对根路径列表）' }
    $trash = Join-Path $rootPath ("_trash\retired-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
    foreach ($rel in $Files) {
      $p = Join-Path $rootPath $rel
      if (-not (Test-Path -LiteralPath $p)) { Write-Note "不存在，跳过：$rel"; continue }
      $dst = Join-Path $trash $rel
      New-Item -ItemType Directory -Force -Path (Split-Path $dst -Parent) | Out-Null
      Move-Item -LiteralPath $p -Destination $dst -Force
      Write-Ok "$rel -> _trash\$([IO.Path]::GetFileName($trash))\"
    }
    Write-Note '回收站区约定：_trash\ 只进不自动删；清空需人工确认（零残留全局不变量）'
  }
  'Show' {
    $mf = Get-Manifest $rootPath
    if (-not $mf) { Write-Note '无 MANIFEST.json'; exit 1 }
    Write-Ok "版本 v$($mf.version)  label=$($mf.label)  taken=$($mf.taken)  files=@($($mf.files)).Count"
  }
}
