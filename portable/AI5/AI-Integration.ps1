param(
  [ValidateSet("Preflight", "Report", "Run-All")]
  [string]$Action = "Preflight",
  [string]$DataDrive = "D:",
  [string]$OutDir = "D:\Data\Tests",
  [switch]$Yes
)
# AI-5 交付核 / AI1-AI5 联调（对应 PORTABLE_AI_SPLIT_PLAN.md「等 AI1-4 完成后再联调」）
# 用法:
#   .\AI-Integration.ps1 -Action Preflight     # 只读：核验 5 个 AI 的交付物是否齐全
#   .\AI-Integration.ps1 -Action Run-All       # 串跑各核的只读自检 + AI5 的测试链
#   .\AI-Integration.ps1 -Action Report        # 只读：输出联调就绪度报告
# 说明: 本脚本只调用各核「只读/自检」类动作，不做任何造盘、加密、删除操作。
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "AI5-Lib.ps1")

$PortableDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$VhdxDirForRun = "D:\Variable-USB"

# 每个 AI 的交付物清单（按 PORTABLE_AI_SPLIT_PLAN.md 分工表）
# files/dirs = 相对 portable/；docs = 相对 portable/；repoDocs = 相对仓库根
$Cores = @(
  [pscustomobject]@{
    core = "AI-1 存储核"; chapter = "第3+9章"; status = "✅ 已完成（已合入 main）"
    files = @("AI1\Create-VHDX.ps1", "AI1\Tune-Guest.ps1", "AI1\Link-DataApps.ps1",
      "AI1\Bench-Storage.ps1", "AI1\Maintain-VHDX.ps1")
    dirs = @(); docs = @("AI1\Bench.md", "AI1\README.md"); repoDocs = @("docs\AI1-存储.md")
  }
  [pscustomobject]@{
    core = "AI-2 隔离核"; chapter = "第4+5章"; status = "⬜ 待实施"
    files = @("Test-VM.ps1"); dirs = @(); docs = @(); repoDocs = @("docs\AI2-隔离防崩.md", "src-tauri\src\shell\isolation.rs")
  }
  [pscustomobject]@{
    core = "AI-3 兼容核"; chapter = "第6+7章"; status = "✅ 已完成"
    files = @(); dirs = @(); docs = @(); repoDocs = @("docs\AI3-兼容体验.md", "src-tauri\src\shell\compat.rs")
  }
  [pscustomobject]@{
    core = "AI-4 拓展核"; chapter = "第8+10章"; status = "✅ 已完成"
    files = @("AI4\Merge-Apps.ps1", "AI4\MSIX-Attach.ps1", "AI4\Plugin-Host.ps1", "AI4\Plugin-Manager.ps1",
      "AI4\Config-Runtime.ps1", "AI4\Security-Manager.ps1", "AI4\Cloud-Sync.ps1", "AI4\Data-Init.ps1", "AI4\Benchmark.ps1")
    dirs = @("AI4\Data", "AI4\Config"); docs = @("AI4\AI4-拓展安全.md", "AI4\README.md"); repoDocs = @()
  }
  [pscustomobject]@{
    core = "AI-5 交付核"; chapter = "第11+12章"; status = "✅ 已实现（真机待验）"
    files = @("AI5\Compat-Matrix.ps1", "AI5\Chaos-Inject.ps1", "AI5\Bench-Perf.ps1", "AI5\Accept-Gate.ps1",
      "AI5\Deploy-To-USB.ps1", "AI5\Maintenance.ps1", "AI5\AI-Integration.ps1", "AI5\AI5-Lib.ps1",
      "tests\Run-PortableTests.ps1")
    dirs = @("AI5\Data"); data = @("AI5\Data\compat-matrix.json", "AI5\Data\chaos-scenarios.json")
    docs = @("AI5\USER-MANUAL.md", "AI5\FAQ.md", "AI5\README.md")
    repoDocs = @("docs\AI5-测试交付.md", "docs\bench\2026-09-07-portable.md")
  }
)

function Test-CoreArtifacts {
  param([Parameter(Mandatory = $true)]$Core)
  $rows = @(); $missing = 0
  $props = $Core.PSObject.Properties.Name
  foreach ($f in $(if ($props -contains "files") { $Core.files } else { @() })) {
    $ok = Test-Path -LiteralPath (Join-Path $PortableDir $f)
    $rows += [pscustomobject]@{ kind = "脚本"; path = $f; ok = $ok }
    if (-not $ok) { $missing++ }
  }
  foreach ($d in $(if ($props -contains "dirs") { $Core.dirs } else { @() })) {
    $ok = Test-Path -LiteralPath (Join-Path $PortableDir $d)
    $rows += [pscustomobject]@{ kind = "目录"; path = $d; ok = $ok }
    if (-not $ok) { $missing++ }
  }
  foreach ($doc in $(if ($props -contains "docs") { $Core.docs } else { @() })) {
    $ok = Test-Path -LiteralPath (Join-Path $PortableDir $doc)
    $rows += [pscustomobject]@{ kind = "文档"; path = $doc; ok = $ok }
    if (-not $ok) { $missing++ }
  }
  foreach ($d in $(if ($props -contains "data") { $Core.data } else { @() })) {
    $ok = Test-Path -LiteralPath (Join-Path $PortableDir $d)
    $rows += [pscustomobject]@{ kind = "数据"; path = $d; ok = $ok }
    if (-not $ok) { $missing++ }
  }
  foreach ($doc in $(if ($props -contains "repoDocs") { $Core.repoDocs } else { @() })) {
    $ok = Test-Path -LiteralPath (Join-Path $RepoRoot $doc)
    $rows += [pscustomobject]@{ kind = "仓库文档"; path = $doc; ok = $ok }
    if (-not $ok) { $missing++ }
  }
  return @{ rows = $rows; missing = $missing }
}

function Invoke-Preflight {
  Write-Ai5 "AI1-AI5 交付物联调预检" "Step"
  $summary = @()
  foreach ($c in $Cores) {
    $r = Test-CoreArtifacts -Core $c
    $icon = if ($r.missing -eq 0) { "✅" } else { "❌" }
    Write-Ai5 ("{0} {1,-14} {2,-10} 计划状态 {3}  交付物缺 {4} 项" -f $icon, $c.core, $c.chapter, $c.status, $r.missing) `
      $(if ($r.missing -eq 0) { "Ok" } else { "Err" })
    foreach ($x in ($r.rows | Where-Object { -not $_.ok })) { Write-Host "      缺: [$($x.kind)] $($x.path)" }
    $summary += [pscustomobject]@{ core = $c.core; chapter = $c.chapter; planStatus = $c.status; missing = $r.missing; rows = $r.rows }
  }

  # AI-4 只读自检（不改任何状态）
  Write-Host ""
  Write-Ai5 "调用 AI-4 只读自检（Security-Manager -Action Status 需要真实盘，这里只做脚本可用性检查）"
  $sec = Join-Path $PortableDir "AI4\Security-Manager.ps1"
  if (Test-Path -LiteralPath $sec) { Write-Ai5 "AI4\Security-Manager.ps1 可被调用" "Ok" }

  $out = Join-Path $OutDir "integration-preflight.json"
  Save-Ai5Json -Object ([pscustomobject]@{ tool = "AI-Integration.ps1"; at = (Get-Date -Format "o"); cores = $summary }) -Path $out | Out-Null
  Write-Ai5 "预检结果已写入 $out" "Ok"

  $blocked = @($summary | Where-Object { $_.missing -gt 0 })
  if ($blocked.Count -gt 0) {
    Write-Ai5 "以下核的交付物不齐，联调会受阻: $(($blocked | ForEach-Object { $_.core }) -join ', ')" "Warn"
    return 1
  }
  Write-Ai5 "5 个核交付物齐全，可进入联调" "Ok"
  return 0
}

function Invoke-RunAll {
  Write-Ai5 "联调串跑（只读动作，不造盘/不加密/不删除）" "Step"
  $code = Invoke-Preflight
  if ($code -ne 0) { Write-Ai5 "预检未过，仍继续跑只读测试项以便暴露问题" "Warn" }

  $steps = @(
    @{ name = "兼容矩阵清单"; file = "AI5\Compat-Matrix.ps1"; args = @("-Action", "List") }
    @{ name = "混沌场景清单"; file = "AI5\Chaos-Inject.ps1"; args = @("-Action", "List") }
    @{ name = "性能基线清单"; file = "AI5\Bench-Perf.ps1"; args = @("-Action", "Manifest") }
    @{ name = "运维调优清单"; file = "AI5\Maintenance.ps1"; args = @("-Action", "Tune") }
    @{ name = "运维现状"; file = "AI5\Maintenance.ps1"; args = @("-Action", "Status", "-VhdxDir", $VhdxDirForRun, "-DataDrive", $DataDrive) }
    @{ name = "验收汇总"; file = "AI5\Accept-Gate.ps1"; args = @("-Action", "Report", "-DataDrive", $DataDrive, "-OutDir", $OutDir) }
  )
  $failed = 0
  foreach ($s in $steps) {
    $p = Join-Path $PortableDir $s.file
    if (-not (Test-Path -LiteralPath $p)) { Write-Ai5 "缺脚本 $($s.file)" "Err"; $failed++; continue }
    Write-Ai5 "→ $($s.name)" "Step"
    try {
      $callArgs = $s.args
      & $p @callArgs | Out-Host
      if ($LASTEXITCODE -ne 0) { Write-Ai5 "  退出码 $LASTEXITCODE" "Warn" }
    } catch {
      Write-Ai5 "  执行异常: $($_.Exception.Message)" "Err"; $failed++
    }
  }
  if ($failed -gt 0) { return 1 }
  Write-Ai5 "联调串跑完成" "Ok"
  return 0
}

function Invoke-Report {
  $in = Join-Path $OutDir "integration-preflight.json"
  if (-not (Test-Path -LiteralPath $in)) { Write-Ai5 "先跑 -Action Preflight" "Warn"; return 1 }
  $r = Get-Ai5Json -Path $in
  $rp = Join-Path $OutDir "integration-report.md"
  $L = @()
  $L += "# AI1-AI5 联调就绪度报告"
  $L += ""
  $L += "- 生成: $($r.at)"
  $L += ""
  $L += "| 核 | 章节 | 计划状态 | 缺失交付物 |"
  $L += "|---|---|---|---|"
  foreach ($c in $r.cores) {
    $L += "| $($c.core) | $($c.chapter) | $($c.planStatus) | $($c.missing) |"
  }
  $L += ""
  $L += "## 明细"
  foreach ($c in $r.cores) {
    $L += ""
    $L += "### $($c.core)"
    foreach ($x in $c.rows) {
      $L += "- $(if ($x.ok) { '✅' } else { '❌' }) [$($x.kind)] ``$($x.path)``"
    }
  }
  Save-Ai5Text -Lines $L -Path $rp | Out-Null
  Write-Ai5 "报告已写入 $rp" "Ok"
  return 0
}

$exit = 0
switch ($Action) {
  "Preflight" { $exit = Invoke-Preflight }
  "Run-All"   { $exit = Invoke-RunAll }
  "Report"    { $exit = Invoke-Report }
}
exit $exit
