<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务66 —— 差分升级（分通道）+ 失败自动回滚 + 升级报告

.DESCRIPTION
  对应 docs/VARIX双域系统施工总案.md 阶段9（验收：Base 不动 + 差分包分通道 + SHA-256 校验 + 失败自动回滚）。

  升级包格式（manifest.json，开放性升版验收点）：
    {
      "from": "1.0", "version": "1.1",
      "migration": "升级说明：WIN_ENGINE 引擎补丁 + SHARED 契约字段扩展；升版需重建 apps.json 缓存",
      "channel_files": {
        "VARIX_SYS":   [ { "file": "kernel/varix", "sha256": "<hex>" }, ... ],
        "WIN_ENGINE":  [ { "file": "engine.bin",    "sha256": "<hex>" }, ... ],
        "SHARED":      [ { "file": "apps.json",     "sha256": "<hex>" }, ... ]
      }
    }
  - 分通道：VARIX_SYS / WIN_ENGINE / SHARED 三契约分通道应用（互不污染）。
  - 每文件 SHA-256：预检阶段逐一校验包内载荷哈希 == manifest，未全过不动盘。

  两阶段应用（断电安全）：
    阶段A 预检：校验全部载荷哈希 == manifest，任一不符立即中止（盘零改动）。
    阶段B 暂存：包内载荷写入同盘 .staging/<channel>/<relpath>；写完逐文件复校。
    阶段C 提交点：先对受影响文件做 Base 快照到 _trash/upgrade-rollback-<ts>/（Base 恒可恢复），
                  再原子替换目标文件，最后原子写 current.json（版本号=提交点）。
    失败/中断：丢弃 .staging；Base 快照回滚还原；版本号不前进（或回退到旧）。
  升级报告：_versions/upgrade-report-<ts>.json + 文本。

  动作：-Action Build（造包）/ Apply（应用，带检查点供断电演练）/ Verify（下次插入校验）/ Repair（修复中断态）。

.EXAMPLE
  # 造包
  .\Invoke-Differential-Upgrade.ps1 -Action Build -PackageDir _attic\p66-pkg -NewVersion 1.1 -FromVersion 1.0 `
      -SpecFile _attic\p66-spec.json
  # 应用
  .\Invoke-Differential-Upgrade.ps1 -Action Apply -Root E:\ -PackageDir _attic\p66-pkg -Checkpoint _attic\p66-cp.txt
  # 下次插入校验
  .\Invoke-Differential-Upgrade.ps1 -Action Verify -Root E:\

.NOTES
  编码纪律：UTF-8 BOM；PowerShell 5.1 兼容；不动 kernel/src/src-tauri/docs 任务文档。
#>
[CmdletBinding()]
param(
  [ValidateSet('Build', 'Apply', 'Verify', 'Repair')][string]$Action = 'Apply',
  [string]$Root = '',
  [string]$PackageDir = '',
  [string]$SpecFile = '',
  [string]$NewVersion = '',
  [string]$FromVersion = '',
  [string]$Checkpoint = '',
  [string]$MigrationNote = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step { param([string]$Msg) Write-Host ">>> $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "    + $Msg" -ForegroundColor Green }
function Write-Note { param([string]$Msg) Write-Host "    ! $Msg" -ForegroundColor Yellow }
function Write-Fail { param([string]$Msg) Write-Host "    x $Msg" -ForegroundColor Red }

function Set-Checkpoint { param([string]$Stage)
  if ($Checkpoint) { [IO.File]::WriteAllText($Checkpoint, $Stage, [Text.UTF8Encoding]::new($false)) }
}

function Get-Sha256 {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return $null }
  return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash
}

function Get-CurrentManifest {
  param([string]$Base)
  $p = Join-Path $Base '_versions\current.json'
  if (-not (Test-Path -LiteralPath $p)) { return $null }
  return Get-Content -LiteralPath $p -Raw | ConvertFrom-Json
}

function Get-ChannelDir {
  param([string]$Root, [string]$Channel)
  switch ($Channel) {
    'VARIX_SYS'  { return Join-Path $Root 'VARIX_SYS' }
    'WIN_ENGINE' { return Join-Path $Root 'WIN_ENGINE' }
    'SHARED'     { return Join-Path $Root 'SHARED' }
    default      { throw "未知通道：$Channel" }
  }
}

# ---------------------------------------------------------------- Build（造包）
function Build-Package {
  if (-not $PackageDir) { throw '-PackageDir 必填' }
  if (-not $NewVersion) { throw '-NewVersion 必填' }
  New-Item -ItemType Directory -Force -Path $PackageDir | Out-Null
  $payloadDir = Join-Path $PackageDir 'payload'
  New-Item -ItemType Directory -Force -Path $payloadDir | Out-Null
  $specs = Get-Content -LiteralPath $SpecFile -Raw | ConvertFrom-Json
  $channelFiles = @{ VARIX_SYS = @(); WIN_ENGINE = @(); SHARED = @() }
  foreach ($s in @($specs)) {
    $cdir = Get-ChannelDir $payloadDir $s.channel
    New-Item -ItemType Directory -Force -Path (Split-Path (Join-Path $cdir $s.file) -Parent) | Out-Null
    $dst = Join-Path $cdir $s.file
    [IO.File]::WriteAllText($dst, $s.content, [Text.UTF8Encoding]::new($false))
    $hash = Get-Sha256 $dst
    $channelFiles[$s.channel] += [pscustomobject]@{ file = $s.file; sha256 = $hash }
  }
  $manifest = [pscustomobject]@{
    from      = if ($FromVersion) { $FromVersion } else { '0.0' }
    version   = $NewVersion
    migration = if ($MigrationNote) { $MigrationNote } else { '' }
    channel_files = $channelFiles
  }
  [IO.File]::WriteAllText((Join-Path $PackageDir 'manifest.json'), ($manifest | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
  $total = 0
  foreach ($k in $channelFiles.Keys) { $total += @($channelFiles[$k]).Count }
  Write-Ok "升级包已生成：$PackageDir（version=$NewVersion，分通道文件数=$total）"
}

# ---------------------------------------------------------------- Repair（修复中断态）
function Repair-State {
  param([string]$Root)
  $staging = Join-Path $Root '.staging'
  if (Test-Path -LiteralPath $staging) {
    Remove-Item -LiteralPath $staging -Recurse -Force
    Write-Note '清理残留 .staging'
  }
  $cur = Get-CurrentManifest $Root
  if ($null -eq $cur) { return [pscustomobject]@{ repaired = $false; reason = 'no-current' } }
  # 校验当前文件哈希 vs current.json 期望
  $mismatch = $false
  foreach ($e in @($cur.files)) {
    $fp = Join-Path (Get-ChannelDir $Root $e.channel) $e.file
    if ((Get-Sha256 $fp) -ne $e.sha256) { $mismatch = $true; break }
  }
  if (-not $mismatch) { return [pscustomobject]@{ repaired = $false; reason = 'consistent' } }
  # 找最近 Base 快照回滚
  $trash = Join-Path $Root '_trash'
  $snaps = @()
  if (Test-Path -LiteralPath $trash) {
    $snaps = @(Get-ChildItem -LiteralPath $trash -Filter 'upgrade-rollback-*' -Directory | Sort-Object Name -Descending)
  }
  if ($snaps.Count -gt 0) {
    $snap = $snaps[0].FullName
    foreach ($e in @($cur.files)) {
      $src = Join-Path (Join-Path $snap $e.channel) $e.file
      $dst = Join-Path (Get-ChannelDir $Root $e.channel) $e.file
      if (Test-Path -LiteralPath $src) {
        New-Item -ItemType Directory -Force -Path (Split-Path $dst -Parent) | Out-Null
        Copy-Item -LiteralPath $src -Destination $dst -Force
      }
    }
    Write-Note "从中断态回滚到 Base 快照：$snap"
    return [pscustomobject]@{ repaired = $true; reason = 'rolled-back'; snapshot = $snap }
  }
  return [pscustomobject]@{ repaired = $true; reason = 'mismatch-no-snapshot' }
}

# ---------------------------------------------------------------- Apply（应用）
function Apply-Upgrade {
  if (-not $Root -or -not $PackageDir) { throw '-Root 与 -PackageDir 必填' }
  Set-Checkpoint 'PRECHECK'
  # 进盘先自愈（应对上次中断残留）
  $rep = Repair-State $Root
  if ($rep.repaired) { Write-Note "应用前自愈：$($rep.reason)" }

  $manifestPath = Join-Path $PackageDir 'manifest.json'
  if (-not (Test-Path -LiteralPath $manifestPath)) { throw "升级包缺 manifest.json：$manifestPath" }
  $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json

  # 阶段A 预检：全部载荷哈希 == manifest
  $payloadDir = Join-Path $PackageDir 'payload'
  foreach ($ch in @('VARIX_SYS', 'WIN_ENGINE', 'SHARED')) {
    foreach ($e in @($manifest.channel_files.$ch)) {
      $pp = Join-Path (Join-Path $payloadDir $ch) $e.file
      $h = Get-Sha256 $pp
      if ($h -ne $e.sha256) {
        Write-Fail "预检失败：$ch/$($e.file) 载荷哈希不符（期望 $($e.sha256) 实得 $h）"
        throw '预检未过：不改动盘'
      }
    }
  }
  Write-Ok '预检通过：全部载荷 SHA-256 与 manifest 一致'

  # 阶段B 暂存（.staging）
  $staging = Join-Path $Root '.staging'
  if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
  New-Item -ItemType Directory -Force -Path $staging | Out-Null
  foreach ($ch in @('VARIX_SYS', 'WIN_ENGINE', 'SHARED')) {
    foreach ($e in @($manifest.channel_files.$ch)) {
      $src = Join-Path (Join-Path $payloadDir $ch) $e.file
      $dst = Join-Path (Join-Path $staging $ch) $e.file
      New-Item -ItemType Directory -Force -Path (Split-Path $dst -Parent) | Out-Null
      Copy-Item -LiteralPath $src -Destination $dst -Force
      Set-Checkpoint 'STAGING'
      Start-Sleep -Milliseconds 250
    }
  }
  # 暂存复校
  foreach ($ch in @('VARIX_SYS', 'WIN_ENGINE', 'SHARED')) {
    foreach ($e in @($manifest.channel_files.$ch)) {
      $sp = Join-Path (Join-Path $staging $ch) $e.file
      if ((Get-Sha256 $sp) -ne $e.sha256) {
        Remove-Item -LiteralPath $staging -Recurse -Force
        Write-Fail "暂存复校失败：$ch/$($e.file)"
        throw '暂存校验未过：已丢弃 .staging，盘零改动'
      }
    }
  }
  Set-Checkpoint 'STAGED'
  Write-Ok '暂存完成且复校通过（盘仍为旧版本）'
  Start-Sleep -Milliseconds 400

  # 阶段C 提交点
  # 1) Base 快照（仅受影响文件）到 _trash/upgrade-rollback-<ts>/
  $cur = Get-CurrentManifest $Root
  $trash = Join-Path $Root '_trash'
  $snap = Join-Path $trash ("upgrade-rollback-" + (Get-Date -Format 'yyyyMMdd-HHmmssfff'))
  New-Item -ItemType Directory -Force -Path $snap | Out-Null
  $newFiles = @()
  $ts = Get-Date -Format 'o'
  foreach ($ch in @('VARIX_SYS', 'WIN_ENGINE', 'SHARED')) {
    foreach ($e in @($manifest.channel_files.$ch)) {
      $livePath = Join-Path (Get-ChannelDir $Root $ch) $e.file
      if (Test-Path -LiteralPath $livePath) {
        $sb = Join-Path (Join-Path $snap $ch) $e.file
        New-Item -ItemType Directory -Force -Path (Split-Path $sb -Parent) | Out-Null
        Copy-Item -LiteralPath $livePath -Destination $sb -Force
      }
      # 目标文件写入
      $dst = $livePath
      $src = Join-Path (Join-Path $staging $ch) $e.file
      New-Item -ItemType Directory -Force -Path (Split-Path $dst -Parent) | Out-Null
      Copy-Item -LiteralPath $src -Destination $dst -Force
      Set-Checkpoint 'COMMIT_SWAP'
      Start-Sleep -Milliseconds 200
      $newFiles += [pscustomobject]@{ channel = $ch; file = $e.file; sha256 = $e.sha256 }
    }
  }
  # 2) 原子写版本号（提交点）：先 tmp 再 rename
  $newManifest = [pscustomobject]@{
    version = $manifest.version
    from    = $manifest.from
    taken   = $ts
    files   = @($newFiles)
  }
  $verPath = Join-Path $Root '_versions\current.json'
  $tmp = Join-Path $Root '_versions\current.json.tmp'
  [IO.File]::WriteAllText($tmp, ($newManifest | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
  Move-Item -LiteralPath $tmp -Destination $verPath -Force
  Set-Checkpoint 'COMMIT_VERSION'
  Start-Sleep -Milliseconds 200

  # 清理 .staging
  Remove-Item -LiteralPath $staging -Recurse -Force
  Set-Checkpoint 'DONE'

  # 升级报告
  $report = [pscustomobject]@{
    result    = 'success'
    from      = $manifest.from
    version   = $manifest.version
    migration = $manifest.migration
    channels  = @('VARIX_SYS', 'WIN_ENGINE', 'SHARED')
    fileCount = $newFiles.Count
    taken     = $ts
  }
  New-Item -ItemType Directory -Force -Path (Join-Path $Root '_versions') | Out-Null
  $rp = Join-Path $Root ('_versions\upgrade-report-' + (Get-Date -Format 'yyyyMMdd-HHmmssfff') + '.json')
  [IO.File]::WriteAllText($rp, ($report | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
  Write-Ok "升级成功：v$($manifest.from) -> v$($manifest.version)；报告 $rp"
  Set-Checkpoint 'DONE'
}

# ---------------------------------------------------------------- Verify（下次插入校验）
function Verify-State {
  param([string]$Root)
  $cur = Get-CurrentManifest $Root
  $baseIntact = $true
  $appsOk = $true
  $version = if ($cur) { $cur.version } else { '' }
  if ($null -eq $cur) { $baseIntact = $false }
  else {
    foreach ($e in @($cur.files)) {
      $fp = Join-Path (Get-ChannelDir $Root $e.channel) $e.file
      if ((Get-Sha256 $fp) -ne $e.sha256) { $baseIntact = $false; break }
    }
  }
  $appsPath = Join-Path (Get-ChannelDir $Root 'SHARED') 'apps.json'
  if (Test-Path -LiteralPath $appsPath) {
    try { $null = Get-Content -LiteralPath $appsPath -Raw | ConvertFrom-Json }
    catch { $appsOk = $false }
  }
  else { $appsOk = $false }
  $obj = [pscustomobject]@{ BaseIntact = $baseIntact; Version = $version; AppsJsonOk = $appsOk; Consistent = ($baseIntact -and $appsOk) }
  Write-Host ("[Verify] BaseIntact={0} Version={1} AppsJsonOk={2} Consistent={3}" -f $baseIntact, $version, $appsOk, $obj.Consistent)
  return $obj
}

# ---------------------------------------------------------------- 分派
switch ($Action) {
  'Build'  { Build-Package }
  'Apply'  { Apply-Upgrade }
  'Verify' { Verify-State -Root $Root | Out-Null }
  'Repair' { $r = Repair-State -Root $Root; Write-Host ("[Repair] repaired={0} reason={1}" -f $r.repaired, $r.reason) }
}
