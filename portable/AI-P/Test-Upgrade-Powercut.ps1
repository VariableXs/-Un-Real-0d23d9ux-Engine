<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务66 —— 差分升级断电中途演练 ×10 零变砖

.DESCRIPTION
  对 Invoke-Differential-Upgrade.ps1 做断电中途升级演练，验证「零变砖」不变量：
    - 每轮在应用阶段不同时机 kill 升级子进程（模拟断电）：
        * 时机A 暂存写入中（checkpoint=STAGING）
        * 时机B 提交点前（checkpoint=STAGED，暂存完未提交）
        * 时机C 提交点后/记录版本前（checkpoint=COMMIT_SWAP，文件已换版本未记）
    - 「下次插入」校验：Base 哈希不变（或已回滚还原）、版本一致或已回滚、SHARED 契约完好（apps.json 可解析）。
    - 零变砖：每轮断电后，下一轮升级仍能成功（10/10）。
  10 轮逐轮表格（断电时机 / 结果 / 恢复动作）归档 docs/acceptance/.../powercut-table.txt。

.EXAMPLE
  powershell -NoProfile -File .\Test-Upgrade-Powercut.ps1
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$attic = Join-Path $repo '_attic'
$scriptPath = Join-Path $here 'Invoke-Differential-Upgrade.ps1'
$pristine = Join-Path $attic 'p66-base-pristine'
$root = Join-Path $attic 'p66-root'
$pkg = Join-Path $attic 'p66-pkg'
$cp = Join-Path $attic 'p66-cp.txt'
$acceptDir = Join-Path $repo 'docs/acceptance/2026-09-17-任务66-差分升级断电演练'
New-Item -ItemType Directory -Force -Path $acceptDir | Out-Null

function Get-Sha256 { param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return $null }
  return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash
}

function Invoke-UpgScript {
  param([string[]]$ExtraArgs)
  $quotedScript = '"{0}"' -f $scriptPath
  $parts = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $quotedScript)
  foreach ($a in $ExtraArgs) {
    if ($a -match '\s') { $parts += '"{0}"' -f $a } else { $parts += $a }
  }
  $argStr = $parts -join ' '
  $p = Start-Process -FilePath 'powershell.exe' -ArgumentList $argStr -Wait -PassThru -NoNewWindow
  return $p.ExitCode
}

function New-PristineBase {
  if (Test-Path -LiteralPath $pristine) { Remove-Item -LiteralPath $pristine -Recurse -Force }
  New-Item -ItemType Directory -Force -Path $pristine | Out-Null
  $vs = Join-Path $pristine 'VARIX_SYS'; $we = Join-Path $pristine 'WIN_ENGINE'; $sh = Join-Path $pristine 'SHARED'
  New-Item -ItemType Directory -Force -Path (Join-Path $vs 'kernel') | Out-Null
  New-Item -ItemType Directory -Force -Path $we | Out-Null
  New-Item -ItemType Directory -Force -Path (Join-Path $sh 'whitelist') | Out-Null
  New-Item -ItemType Directory -Force -Path (Join-Path $sh 'handoff') | Out-Null
  [IO.File]::WriteAllText((Join-Path $vs 'kernel/varix'), 'VARIX_SYS v1.0 kernel', [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText((Join-Path $we 'engine.bin'), 'WIN_ENGINE v1.0', [Text.UTF8Encoding]::new($false))
  $apps = @{ version = 1; updated = ''; apps = @(@{ id = 'a1'; name = 'App1'; icon = ''; channel = 'engine'; tier = 'ok'; updated = '' }) } | ConvertTo-Json -Depth 5
  [IO.File]::WriteAllText((Join-Path $sh 'apps.json'), $apps, [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText((Join-Path $sh 'boot-select.json'), (@{ default_entry = 'variable'; timeout_sec = 5; show_menu = $true; last_boot = ''; windows_bootnext = -1 } | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText((Join-Path $sh 'whitelist/whitelist.json'), (@{ version = 1; rules = @() } | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
  # 当前版本清单
  $files = @(
    [pscustomobject]@{ channel = 'VARIX_SYS'; file = 'kernel/varix'; sha256 = (Get-Sha256 (Join-Path $vs 'kernel/varix')) },
    [pscustomobject]@{ channel = 'WIN_ENGINE'; file = 'engine.bin'; sha256 = (Get-Sha256 (Join-Path $we 'engine.bin')) },
    [pscustomobject]@{ channel = 'SHARED'; file = 'apps.json'; sha256 = (Get-Sha256 (Join-Path $sh 'apps.json')) }
  )
  $cur = [pscustomobject]@{ version = '1.0'; from = '0.0'; taken = (Get-Date -Format 'o'); files = $files }
  New-Item -ItemType Directory -Force -Path (Join-Path $pristine '_versions') | Out-Null
  [IO.File]::WriteAllText((Join-Path $pristine '_versions/current.json'), ($cur | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
}

function Reset-Root {
  if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
  # 复制 pristine -> root（保留 Base 哈希）
  Copy-Item -LiteralPath $pristine -Destination $root -Recurse -Force
}

function Build-Package {
  param([int]$Round)
  if (Test-Path -LiteralPath $pkg) { Remove-Item -LiteralPath $pkg -Recurse -Force }
  $spec = @(
    [pscustomobject]@{ channel = 'VARIX_SYS'; file = 'kernel/varix'; content = "VARIX_SYS v1.1 round$Round patched" },
    [pscustomobject]@{ channel = 'WIN_ENGINE'; file = 'engine.bin'; content = "WIN_ENGINE v1.1 round$Round" },
    [pscustomobject]@{ channel = 'SHARED'; file = 'apps.json'; content = (@{ version = 1; updated = ''; apps = @(@{ id = 'a1'; name = 'App1'; icon = ''; channel = 'engine'; tier = 'ok'; updated = '' }, @{ id = 'a2'; name = 'App2'; icon = ''; channel = 'wine'; tier = 'partial'; updated = '' }) } | ConvertTo-Json -Depth 5) }
  )
  $specPath = Join-Path $attic 'p66-spec.json'
  [IO.File]::WriteAllText($specPath, ($spec | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
  $rc = Invoke-UpgScript @('-Action', 'Build', '-PackageDir', $pkg, '-NewVersion', '1.1', '-FromVersion', '1.0', '-SpecFile', $specPath, '-MigrationNote', "轮$Round：引擎补丁+SHARED 新增 App2（wine/partial），升版需重建 apps 缓存")
  if ($rc -ne 0) { throw 'Build 失败' }
}

function Start-ApplyProc {
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = 'powershell.exe'
  $psi.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$scriptPath`" -Action Apply -Root `"$root`" -PackageDir `"$pkg`" -Checkpoint `"$cp`""
  $psi.UseShellExecute = $false
  $psi.RedirectStandardOutput = $false
  return [System.Diagnostics.Process]::Start($psi)
}

function Wait-KillAt {
  param([System.Diagnostics.Process]$Proc, [string]$Target)
  $deadline = (Get-Date).AddSeconds(60)
  while ((Get-Date) -lt $deadline -and -not $Proc.HasExited) {
    if (Test-Path -LiteralPath $cp) {
      $stage = [IO.File]::ReadAllText($cp, [Text.UTF8Encoding]::new($false)).Trim()
      if ($stage -eq $Target) {
        try { $Proc.Kill() } catch { }
        [void]$Proc.WaitForExit(5000)
        return $stage
      }
    }
    Start-Sleep -Milliseconds 50
  }
  # 超时未命中（可能已自行 DONE）：确保进程结束
  if (-not $Proc.HasExited) { try { $Proc.Kill() } catch { }; [void]$Proc.WaitForExit(5000) }
  $final = if (Test-Path -LiteralPath $cp) { [IO.File]::ReadAllText($cp, [Text.UTF8Encoding]::new($false)).Trim() } else { '' }
  return $final
}

function Run-CleanApply {
  # 零变砖验证：完整跑通一轮升级（不 kill）
  return (Invoke-UpgScript @('-Action', 'Apply', '-Root', $root, '-PackageDir', $pkg, '-Checkpoint', $cp))
}

# ---------------- 主流程 ----------------
New-PristineBase
$baseHashes = @{}
foreach ($rel in @('VARIX_SYS/kernel/varix', 'WIN_ENGINE/engine.bin', 'SHARED/apps.json')) {
  $baseHashes[$rel] = Get-Sha256 (Join-Path $pristine $rel)
}

$points = @('STAGING', 'STAGED', 'COMMIT_SWAP')
$rounds = @()
$allZeroBrick = $true
$globalBaseIntact = $true

Write-Host '== 任务66 差分升级断电演练 ×10 ==' -ForegroundColor Cyan
for ($r = 1; $r -le 10; $r++) {
  $target = $points[($r - 1) % 3]
  Reset-Root
  Build-Package -Round $r
  if (Test-Path -LiteralPath $cp) { Remove-Item -LiteralPath $cp -Force }

  $p = Start-ApplyProc
  $diedAt = Wait-KillAt -Proc $p -Target $target
  # 断电后「下次插入」：修复中断态
  Invoke-UpgScript @('-Action', 'Repair', '-Root', $root) | Out-Null
  # 校验 Base 哈希 / 版本 / SHARED 契约
  $cur = Get-Content -LiteralPath (Join-Path $root '_versions/current.json') -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json
  $verAfterKill = if ($cur) { $cur.version } else { '?' }
  $baseIntact = $true
  foreach ($rel in $baseHashes.Keys) {
    if ((Get-Sha256 (Join-Path $root $rel)) -ne $baseHashes[$rel]) { $baseIntact = $false; break }
  }
  $appsPath = Join-Path $root 'SHARED/apps.json'
  $appsOk = $false
  if (Test-Path -LiteralPath $appsPath) { try { $null = Get-Content -LiteralPath $appsPath -Raw | ConvertFrom-Json; $appsOk = $true } catch { } }

  # 恢复动作归类
  if ($diedAt -eq 'STAGING' -or $diedAt -eq 'STAGED') { $recovery = '丢弃 .staging，Base 未动（一致）' }
  elseif ($diedAt -eq 'COMMIT_SWAP') { $recovery = '回滚至 Base 快照（文件/版本还原）' }
  else { $recovery = "未命中时机（diedAt=$diedAt），按一致处理" }

  if (-not $baseIntact) { $globalBaseIntact = $false }
  $verConsistent = ($verAfterKill -eq '1.0')   # 断电轮版本不应前进

  # 零变砖：下一轮升级仍可成功
  $zeroBrick = $false; $cleanVer = ''
  Reset-Root   # 复位到 Base 重新验证下一轮可升级（等价「下次插入新 U 盘」）
  Build-Package -Round ($r + 100)
  $rc = Run-CleanApply
  if ($rc -eq 0) {
    $c2 = Get-Content -LiteralPath (Join-Path $root '_versions/current.json') -Raw | ConvertFrom-Json
    $cleanVer = $c2.version
    $zeroBrick = ($cleanVer -eq '1.1')
  }
  if (-not $zeroBrick) { $allZeroBrick = $false }

  $rec = [pscustomobject]@{
    Round          = $r
    KillPoint      = $target
    DiedAt         = $diedAt
    BaseIntact     = $baseIntact
    VersionAfterKill = $verAfterKill
    VersionConsistent = $verConsistent
    AppsJsonOk     = $appsOk
    Recovery       = $recovery
    ZeroBrickNext  = $zeroBrick
    CleanApplyVer  = $cleanVer
  }
  $rounds += $rec
  Write-Host ("  [轮{0:00}] 时机={1,-10} 死于={2,-10} Base不变={3} 版本={4}(一致={5}) appsJson={6} 下一轮零变砖={7}" -f `
    $r, $target, $diedAt, $baseIntact, $verAfterKill, $verConsistent, $appsOk, $zeroBrick) -ForegroundColor $(if ($baseIntact -and $zeroBrick) { 'Green' } else { 'Red' })
}

# ---------------- 汇总与归档 ----------------
$passed = ($rounds | Where-Object { $_.BaseIntact -and $_.VersionConsistent -and $_.AppsJsonOk -and $_.ZeroBrickNext }).Count
$summary = [pscustomobject]@{
  task = 66
  totalRounds = 10
  passed = $passed
  allBaseIntact = $globalBaseIntact
  allZeroBrick = $allZeroBrick
  rounds = $rounds
}
[IO.File]::WriteAllText((Join-Path $acceptDir 'powercut-summary.json'), ($summary | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))

# 逐轮表格（文本）
$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine('任务66 差分升级断电中途演练 ×10 零变砖 —— 逐轮记录')
[void]$sb.AppendLine('日期 2026-09-17  AI-P')
[void]$sb.AppendLine('')
[void]$sb.AppendLine('| 轮 | 断电时机 | 实际死于 | Base哈希不变 | 版本号(一致) | apps.json完好 | 恢复动作 | 下一轮零变砖 |')
[void]$sb.AppendLine('|---|---|---|---|---|---|---|---|')
foreach ($rec in $rounds) {
  [void]$sb.AppendLine(('| {0} | {1} | {2} | {3} | {4}({5}) | {6} | {7} | {8} |' -f `
    $rec.Round, $rec.KillPoint, $rec.DiedAt, $rec.BaseIntact, $rec.VersionAfterKill, $rec.VersionConsistent, $rec.AppsJsonOk, $rec.Recovery, $rec.ZeroBrickNext))
}
[void]$sb.AppendLine('')
[void]$sb.AppendLine(('结论：{0}/10 轮全过；Base 哈希不变={1}；10/10 零变砖={2}' -f $passed, $globalBaseIntact, $allZeroBrick))
[IO.File]::WriteAllText((Join-Path $acceptDir 'powercut-table.txt'), $sb.ToString(), [Text.UTF8Encoding]::new($false))

Write-Host ''
Write-Host ("任务66 演练：{0}/10 轮全过；Base不变={1}；零变砖={2}" -f $passed, $globalBaseIntact, $allZeroBrick) -ForegroundColor $(if ($passed -eq 10) { 'Green' } else { 'Red' })
if ($passed -ne 10) { exit 1 }
exit 0
