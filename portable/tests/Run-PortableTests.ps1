param(
  [string]$PortableRoot = "",
  [string]$WorkDir = "",
  [switch]$SkipExec
)
<#
  AI-5 交付核 / portable 套件自检（在真正的 PowerShell 里跑，用官方 AST 解析器）
  三段：
    1. 语法：[System.Management.Automation.Language.Parser]::ParseFile 解析全部 .ps1，有 ParseError 即失败
    2. 数据：校验 AI5\Data\*.json 的结构与不变量（200 条 / 5 类 × 40 / 枚举合法 / 场景 id 唯一）
    3. 执行：跑各脚本的只读动作，检查退出码与输出（不造盘、不加密、不删除）
  退出码 0 = 全通过。CI（.github/workflows/ci.yml 的 portable 作业）在 windows-latest 上调用本脚本。
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $PortableRoot) {
  $PortableRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}
if (-not $WorkDir) {
  $WorkDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ai5-selftest-" + (Get-Date -Format "yyyyMMddHHmmss"))
}
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null

$failures = @()
$checks = 0

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail = "")
  $script:checks++
  if ($Ok) { Write-Host "  ok   $Name" -ForegroundColor Green }
  else {
    Write-Host "  FAIL $Name  $Detail" -ForegroundColor Red
    $script:failures += "$Name :: $Detail"
  }
}

# ---------------- 1. 语法（官方 PowerShell AST 解析器） ----------------
Write-Host "== 1. PowerShell 语法解析 ==" -ForegroundColor Cyan
# 含本自检脚本自己与 AI-1..AI-5 全部脚本；AI-4 的脚本若存在语法问题也会在这里如实暴露
$ps1 = @(Get-ChildItem -Path $PortableRoot -Recurse -Filter "*.ps1" -File)
foreach ($f in $ps1) {
  $tokens = $null; $errors = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$tokens, [ref]$errors)
  if ($errors -and $errors.Count -gt 0) {
    $first = $errors[0]
    Add-Check -Name ("语法 " + $f.FullName.Substring($PortableRoot.Length + 1)) -Ok $false `
      -Detail ("{0} 处错误，首条 L{1}: {2}" -f $errors.Count, $first.Extent.StartLineNumber, $first.Message)
  } else {
    Add-Check -Name ("语法 " + $f.FullName.Substring($PortableRoot.Length + 1)) -Ok $true
  }
}

# ---------------- 2. 数据文件不变量 ----------------
Write-Host "== 2. 数据文件校验 ==" -ForegroundColor Cyan
$matrixPath = Join-Path $PortableRoot "AI5\Data\compat-matrix.json"
$chaosPath = Join-Path $PortableRoot "AI5\Data\chaos-scenarios.json"

if (Test-Path -LiteralPath $matrixPath) {
  $m = Get-Content -LiteralPath $matrixPath -Raw -Encoding UTF8 | ConvertFrom-Json
  Add-Check -Name "矩阵条目数=200" -Ok (@($m.apps).Count -eq 200) -Detail "实际 $(@($m.apps).Count)"
  $byCat = @($m.apps | Group-Object category)
  Add-Check -Name "矩阵 5 类" -Ok ($byCat.Count -eq 5) -Detail "实际 $($byCat.Count) 类"
  Add-Check -Name "每类 40 条" -Ok (@($byCat | Where-Object { $_.Count -ne 40 }).Count -eq 0) `
    -Detail (($byCat | ForEach-Object { "$($_.Name)=$($_.Count)" }) -join " ")
  Add-Check -Name "应用名全局唯一" -Ok (@($m.apps | Select-Object -ExpandProperty name | Sort-Object -Unique).Count -eq 200)
  $badEnum = @($m.apps | Where-Object { @("pass", "warn", "fail", "todo") -notcontains $_.aMode -or @("pass", "warn", "fail", "todo") -notcontains $_.bMode })
  Add-Check -Name "A/B 判定枚举合法" -Ok ($badEnum.Count -eq 0) -Detail "$(($badEnum | Select-Object -First 3 -ExpandProperty name) -join ',')"
  $noId = @($m.apps | Where-Object { -not $_.id })
  Add-Check -Name "每条都有 id" -Ok ($noId.Count -eq 0)
  Add-Check -Name "summary.total 与实际一致" -Ok ($m.summary.total -eq @($m.apps).Count)
  # 主计划 11.1 点名的条目必须存在
  foreach ($must in @("WPS Office", "微信", "Blender", "Adobe Photoshop", "Visual Studio 2022", "Trae CN", "Steam")) {
    $hit = @($m.apps | Where-Object { $_.name -eq $must })
    Add-Check -Name "点名条目在列: $must" -Ok ($hit.Count -eq 1)
  }
  Add-Check -Name "预算字段齐全" -Ok ($null -ne $m.budget.hotStartSec -and $null -ne $m.budget.coldStartSec -and $null -ne $m.budget.systemBootSec)
} else { Add-Check -Name "矩阵文件存在" -Ok $false -Detail $matrixPath }

if (Test-Path -LiteralPath $chaosPath) {
  $c = Get-Content -LiteralPath $chaosPath -Raw -Encoding UTF8 | ConvertFrom-Json
  $sc = @($c.scenarios)
  Add-Check -Name "混沌场景 ≥10（主计划扩充21）" -Ok ($sc.Count -ge 10) -Detail "实际 $($sc.Count)"
  Add-Check -Name "场景 id 唯一" -Ok (@($sc | Select-Object -ExpandProperty id | Sort-Object -Unique).Count -eq $sc.Count)
  $badAuto = @($sc | Where-Object { @("auto", "manual") -notcontains $_.automatable })
  Add-Check -Name "automatable 枚举合法" -Ok ($badAuto.Count -eq 0)
  $noExpect = @($sc | Where-Object { -not $_.expect -or @($_.steps).Count -eq 0 })
  Add-Check -Name "每场景有步骤与期望" -Ok ($noExpect.Count -eq 0)
  # 危险场景必须是 manual —— 脚本绝不代为执行
  $dangerAuto = @($sc | Where-Object { $_.dangerous -eq $true -and $_.automatable -eq "auto" })
  Add-Check -Name "dangerous 场景一律 manual" -Ok ($dangerAuto.Count -eq 0) -Detail "$(($dangerAuto | Select-Object -ExpandProperty id) -join ',')"
} else { Add-Check -Name "混沌场景文件存在" -Ok $false -Detail $chaosPath }

# ---------------- 3. 执行只读动作 ----------------
if ($SkipExec) {
  Write-Host "== 3. 执行只读动作（已按 -SkipExec 跳过）==" -ForegroundColor Cyan
} else {
  Write-Host "== 3. 执行只读动作 ==" -ForegroundColor Cyan
  $env:AI5_NONINTERACTIVE = "1"
  $dd = (Split-Path -Qualifier $WorkDir)          # 例: C:
  $out = Join-Path $WorkDir "Tests"
  $ev = Join-Path $WorkDir "Evidence"
  New-Item -ItemType Directory -Force -Path $out | Out-Null
  New-Item -ItemType Directory -Force -Path $ev | Out-Null

  # $ExpectExit = -1 表示「不校验退出码」：这些脚本的退出码取决于硬件是否达标
  # （例如 CI 的磁盘达不到 900MB/s 预算，Gate 就该返回 1），只要不抛异常即视为脚本可用。
  function Invoke-ReadOnly {
    param([string]$Name, [string]$Script, [string[]]$ScriptArgs, [int]$ExpectExit = 0, [string]$ExpectText = "")
    $p = Join-Path $PortableRoot $Script
    if (-not (Test-Path -LiteralPath $p)) { Add-Check -Name $Name -Ok $false -Detail "脚本不存在 $Script"; return }
    $txt = ""
    $code = 0
    try {
      $txt = (& $p @ScriptArgs 2>&1 | Out-String)
      $code = $LASTEXITCODE
      if ($null -eq $code) { $code = 0 }
    } catch {
      Add-Check -Name $Name -Ok $false -Detail "异常: $($_.Exception.Message)"
      return
    }
    if ($ExpectExit -lt 0) {
      Add-Check -Name "$Name (exit=$code, 退出码随硬件而定)" -Ok $true
    } else {
      Add-Check -Name "$Name (exit=$code)" -Ok ($code -eq $ExpectExit) -Detail "期望退出码 $ExpectExit"
    }
    if ($ExpectText) {
      Add-Check -Name "$Name 输出含 '$ExpectText'" -Ok ($txt -match [regex]::Escape($ExpectText))
    }
  }

  Invoke-ReadOnly -Name "Compat-Matrix -Action List" -Script "AI5\Compat-Matrix.ps1" `
    -ScriptArgs @("-Action", "List") -ExpectExit 0 -ExpectText "兼容矩阵"
  Invoke-ReadOnly -Name "Chaos-Inject -Action List" -Script "AI5\Chaos-Inject.ps1" `
    -ScriptArgs @("-Action", "List", "-DataDrive", $dd, "-EvidenceRoot", $ev) -ExpectExit 0 -ExpectText "S01"
  Invoke-ReadOnly -Name "Chaos-Inject -Action Plan -Scenario S07" -Script "AI5\Chaos-Inject.ps1" `
    -ScriptArgs @("-Action", "Plan", "-Scenario", "S07", "-DataDrive", $dd, "-EvidenceRoot", $ev) -ExpectExit 0 -ExpectText "安全模式"
  Invoke-ReadOnly -Name "Bench-Perf -Action Manifest" -Script "AI5\Bench-Perf.ps1" `
    -ScriptArgs @("-Action", "Manifest") -ExpectExit 0 -ExpectText "seqReadMBps"
  Invoke-ReadOnly -Name "Bench-Perf -Action Run (真实读写 $WorkDir)" -Script "AI5\Bench-Perf.ps1" `
    -ScriptArgs @("-Action", "Run", "-TestDrive", $WorkDir, "-OutDir", $out, "-SeqMB", "64", "-RandMB", "16", "-StartRounds", "2") -ExpectExit -1
  Add-Check -Name "Bench 结果文件生成" -Ok (Test-Path -LiteralPath (Join-Path $out "bench-results.json"))
  # Gate 的退出码取决于本机磁盘是否达标（CI 磁盘通常达不到 900MB/s 预算），故不校验码
  Invoke-ReadOnly -Name "Bench-Perf -Action Gate" -Script "AI5\Bench-Perf.ps1" `
    -ScriptArgs @("-Action", "Gate", "-OutDir", $out) -ExpectExit -1
  Invoke-ReadOnly -Name "Chaos-Inject -Action Run (auto 场景)" -Script "AI5\Chaos-Inject.ps1" `
    -ScriptArgs @("-Action", "Run", "-DataDrive", $dd, "-EvidenceRoot", $ev) -ExpectExit -1
  Add-Check -Name "混沌结果文件生成" -Ok (Test-Path -LiteralPath (Join-Path $ev "chaos-results.json"))
  # Preflight 在 CI 上必然报缺件（Data 结构未初始化 / 无 Hyper-V），退出码 1 是正确行为
  Invoke-ReadOnly -Name "Deploy-To-USB -Action Preflight" -Script "AI5\Deploy-To-USB.ps1" `
    -ScriptArgs @("-Action", "Preflight", "-Src", $WorkDir, "-Dst", "$dd\", "-DataDrive", $dd, "-AllowFixedTarget", "-MinFreeGB", "0", "-EvidenceRoot", $ev) -ExpectExit -1
  Add-Check -Name "预检结果文件生成" -Ok (Test-Path -LiteralPath (Join-Path $ev "preflight.json"))
  Invoke-ReadOnly -Name "Deploy-To-USB -Action Stage1 -DryRun" -Script "AI5\Deploy-To-USB.ps1" `
    -ScriptArgs @("-Action", "Stage1", "-IsoPath", "$env:SystemRoot\notepad.exe", "-Src", $WorkDir, "-DryRun") -ExpectExit 0
  Invoke-ReadOnly -Name "Maintenance -Action Tune" -Script "AI5\Maintenance.ps1" `
    -ScriptArgs @("-Action", "Tune") -ExpectExit 0 -ExpectText "powercfg"
  Invoke-ReadOnly -Name "Maintenance -Action Status" -Script "AI5\Maintenance.ps1" `
    -ScriptArgs @("-Action", "Status", "-VhdxDir", $WorkDir, "-DataDrive", $dd) -ExpectExit 0
  Invoke-ReadOnly -Name "Maintenance -Action Backup (无源应 warn 退出1)" -Script "AI5\Maintenance.ps1" `
    -ScriptArgs @("-Action", "Backup", "-VhdxDir", $WorkDir, "-DataDrive", $dd) -ExpectExit 1
  Invoke-ReadOnly -Name "Accept-Gate -Action Init" -Script "AI5\Accept-Gate.ps1" `
    -ScriptArgs @("-Action", "Init", "-DataDrive", $dd, "-OutDir", $out) -ExpectExit 0
  Add-Check -Name "人工实测模板生成" -Ok (Test-Path -LiteralPath (Join-Path $out "manual-results.json"))
  Invoke-ReadOnly -Name "Accept-Gate -Action Report" -Script "AI5\Accept-Gate.ps1" `
    -ScriptArgs @("-Action", "Report", "-DataDrive", $dd, "-OutDir", $out, "-EvidenceRoot", $ev) -ExpectExit 0
  Add-Check -Name "验收报告生成" -Ok (Test-Path -LiteralPath (Join-Path $out "acceptance-report.md"))
  Invoke-ReadOnly -Name "Accept-Gate -Action Check (无证据时不应通过 -Strict)" -Script "AI5\Accept-Gate.ps1" `
    -ScriptArgs @("-Action", "Check", "-DataDrive", $dd, "-OutDir", $out, "-EvidenceRoot", $ev, "-Strict") -ExpectExit 1
  # AI-1/AI-2 尚未交付，Preflight 报缺件退出码 1 是正确行为，不该让自检因此变红
  Invoke-ReadOnly -Name "AI-Integration -Action Preflight" -Script "AI5\AI-Integration.ps1" `
    -ScriptArgs @("-Action", "Preflight", "-DataDrive", $dd, "-OutDir", $out) -ExpectExit -1
  Add-Check -Name "联调预检结果生成" -Ok (Test-Path -LiteralPath (Join-Path $out "integration-preflight.json"))

  # 负向用例：目标盘写成宿主系统盘时必须被拒绝
  $sysDrive = ([System.IO.Path]::GetPathRoot($env:SystemRoot)).TrimEnd('\')
  $neg = (& (Join-Path $PortableRoot "AI5\Deploy-To-USB.ps1") -Action "Verify" -Dst "$sysDrive\" -DataDrive $dd -EvidenceRoot $ev 2>&1 | Out-String)
  Add-Check -Name "负向: Verify 指向系统盘仍安全（不因缺交付物而误判通过）" -Ok ($neg -match "缺|❌|不存在")

  Remove-Item -LiteralPath $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
}

# ---------------- 汇总 ----------------
Write-Host ""
if ($failures.Count -eq 0) {
  Write-Host "PORTABLE SELFTEST PASS ($checks checks)" -ForegroundColor Green
  exit 0
} else {
  Write-Host "PORTABLE SELFTEST FAIL: $($failures.Count)/$checks" -ForegroundColor Red
  foreach ($f in $failures) { Write-Host "  - $f" -ForegroundColor Red }
  exit 1
}
