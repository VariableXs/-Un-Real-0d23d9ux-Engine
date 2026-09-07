param(
  [ValidateSet("List", "Run", "Report", "Fill-ExeHint")]
  [string]$Action = "List",
  [string]$MatrixFile = "$PSScriptRoot\Data\compat-matrix.json",
  [string]$Category = "",                 # 只跑某一类: office/design/dev/tools/game
  [string]$Filter = "",                   # 只跑名字匹配的应用
  [int]$Limit = 0,                        # 只跑前 N 个（0=不限）
  [string]$DataDrive = "D:",
  [string]$AppsRoot = "D:\Data\Apps",
  [string]$OutDir = "D:\Data\Tests",
  [string]$ReportPath = "",               # 留空则写 $OutDir\compat-report.md
  [switch]$Force                          # 覆盖已有结果继续跑
)
# AI-5 交付核 / 第11.1章 兼容矩阵 Top200（5 类 × 40，A/B 双模式）
# 用法:
#   .\Compat-Matrix.ps1 -Action List                       # 只读：看矩阵与覆盖率
#   .\Compat-Matrix.ps1 -Action Fill-ExeHint               # 扫描 Data\Apps 回填 exeHint（只写矩阵文件）
#   .\Compat-Matrix.ps1 -Action Run -Category office       # 真机实测：逐项启动计时（只测已安装项）
#   .\Compat-Matrix.ps1 -Action Run -Filter Blender -Limit 3
#   .\Compat-Matrix.ps1 -Action Report                     # 由结果生成 Markdown 报告
# 说明:
#   * Run 只测「在 Data\Apps 下找得到主程序」的条目，找不到的记 skip，不臆造结果；
#   * 未安装条目保持 todo，等真机装好后重跑即可增量回填。
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "AI5-Lib.ps1")

function Get-Matrix {
  $m = Get-Ai5Json -Path $MatrixFile
  $apps = @($m.apps)
  if ($Category) { $apps = @($apps | Where-Object { $_.category -eq $Category }) }
  if ($Filter)   { $apps = @($apps | Where-Object { $_.name -like "*$Filter*" }) }
  if ($Limit -gt 0 -and $apps.Count -gt $Limit) { $apps = @($apps[0..($Limit - 1)]) }
  return @{ doc = $m; apps = $apps }
}

# 在 Data\Apps 下找主程序：优先矩阵里的 exeHint，其次常见入口名
function Find-AppExe {
  param([Parameter(Mandatory = $true)]$App)
  $roots = @()
  if ($App.exeHint) { $roots += $App.exeHint }
  $guessDir = Join-Path $AppsRoot $App.name
  if (Test-Path -LiteralPath $guessDir) { $roots += $guessDir }
  $roots += @($AppsRoot)
  $candidates = @()
  foreach ($r in $roots) {
    if (-not $r) { continue }
    if ((Test-Path -LiteralPath $r) -and $r.ToLower().EndsWith(".exe") -and (Test-Path -LiteralPath $r -PathType Leaf)) {
      $candidates += $r; continue
    }
    if (-not (Test-Path -LiteralPath $r)) { continue }
    $base = ($App.name -replace '[^\w\s\.\-]', '' -replace '\s+', '*')
    $found = @(Get-ChildItem -LiteralPath $r -Recurse -Depth 3 -Filter "*.exe" -File -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -like "$base*" } | Sort-Object Length -Descending)
    if ($found.Count -eq 0) {
      $found = @(Get-ChildItem -LiteralPath $r -Recurse -Depth 2 -Filter "*.exe" -File -ErrorAction SilentlyContinue |
        Where-Object { $_.DirectoryName -like "*$($App.name)*" } | Sort-Object Length -Descending)
    }
    foreach ($f in $found) { $candidates += $f.FullName }
  }
  foreach ($c in ($candidates | Select-Object -Unique)) {
    if (Test-Path -LiteralPath $c -PathType Leaf) { return $c }
  }
  return ""
}

function Measure-AppStart {
  param([Parameter(Mandatory = $true)][string]$ExePath)
  # 冷/热启动：第一次进程视为冷启动，紧接着第二次视为热启动（文件系统缓存已热）
  $times = @()
  for ($i = 0; $i -lt 2; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = $null
    try {
      $p = Start-Process -FilePath $ExePath -PassThru -ErrorAction Stop
      # 等到主窗口出现或超时（GUI 应用的"可用"口径）；无窗口的 CLI 会立即退出。
      # 超时只看 $sw.Elapsed —— 不要拿 TimeSpan 和 DateTime 比。
      while ($sw.Elapsed.TotalSeconds -lt 60) {
        $p.Refresh()
        if ($p.HasExited) { break }
        if ($p.MainWindowHandle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
      }
      $sw.Stop()
      $times += [math]::Round($sw.Elapsed.TotalSeconds, 2)
      if ($p -and -not $p.HasExited) {
        try { $p.CloseMainWindow() | Out-Null; Start-Sleep -Milliseconds 500 } catch { }
        if (-not $p.HasExited) { try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch { } }
      }
    } catch {
      $sw.Stop()
      return @{ ok = $false; error = $_.Exception.Message; cold = $null; hot = $null; memMB = $null }
    }
    Start-Sleep -Milliseconds 800
  }
  $cold = $null; $hot = $null; $mem = $null
  if ($times.Count -ge 1) { $cold = $times[0] }
  if ($times.Count -ge 2) { $hot = $times[1] }
  try {
    $procs = @(Get-Process -Name ([System.IO.Path]::GetFileNameWithoutExtension($ExePath)) -ErrorAction SilentlyContinue)
    if ($procs.Count -gt 0) { $mem = [math]::Round((($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB), 0) }
  } catch { }
  return @{ ok = $true; error = ""; cold = $cold; hot = $hot; memMB = $mem }
}

function Show-List {
  $m = Get-Matrix
  Write-Ai5 "兼容矩阵: $MatrixFile" "Step"
  Write-Ai5 ("条目 {0} | 已判定 {1} | 待实测 {2}" -f $m.doc.summary.total, $m.doc.summary.withMeasuredVerdict, $m.doc.summary.pendingMeasured)
  Write-Ai5 ("预算: 冷≤{0}s 热≤{1}s 系统≤{2}s 崩溃恢复≤{3}s PnP≤{4}s" -f `
    $m.doc.budget.coldStartSec, $m.doc.budget.hotStartSec, $m.doc.budget.systemBootSec, `
    $m.doc.budget.crashRecoverSec, $m.doc.budget.pnpSwitchSec)
  Write-Host ""
  foreach ($g in ($m.doc.apps | Group-Object categoryName)) {
    $pass = @($g.Group | Where-Object { $_.aMode -eq "pass" }).Count
    $warn = @($g.Group | Where-Object { $_.aMode -eq "warn" }).Count
    $todo = @($g.Group | Where-Object { $_.aMode -eq "todo" }).Count
    Write-Host ("{0,-6} 共{1,3}  A模式 pass={2,2} warn={3,2} todo={4,3}" -f $g.Name, $g.Count, $pass, $warn, $todo)
  }
  Write-Host ""
  $sel = $m.apps
  Write-Ai5 "当前选择 $($sel.Count) 条（-Category / -Filter / -Limit 可缩小范围）"
  $sel | Select-Object -First 20 id, name, version, aMode, bMode | Format-Table -AutoSize | Out-Host
  if ($sel.Count -gt 20) { Write-Ai5 "... 其余 $($sel.Count - 20) 条见 JSON" }
}

function Invoke-FillExeHint {
  if (-not (Test-Path -LiteralPath $AppsRoot)) {
    Write-Ai5 "未找到 $AppsRoot，先把绿色软件放进 Data\Apps 再回填" "Warn"; return 1
  }
  $m = Get-Ai5Json -Path $MatrixFile
  $filled = 0
  foreach ($a in $m.apps) {
    $exe = Find-AppExe -App $a
    if ($exe -and $a.exeHint -ne $exe) { $a.exeHint = $exe; $filled++ }
  }
  Save-Ai5Json -Object $m -Path $MatrixFile | Out-Null
  Write-Ai5 "已回填 exeHint: $filled 条 -> $MatrixFile" "Ok"
  return 0
}

function Invoke-Run {
  $m = Get-Matrix
  New-Ai5Directory -Path $OutDir | Out-Null
  $rows = @()
  $budget = $m.doc.budget
  Write-Ai5 "开始实测 $($m.apps.Count) 条（只测 Data\Apps 下能找到的主程序）" "Step"
  $i = 0
  foreach ($a in $m.apps) {
    $i++
    $exe = Find-AppExe -App $a
    if (-not $exe) {
      Write-Ai5 ("[{0}/{1}] {2} 未在 {3} 找到主程序 -> skip" -f $i, $m.apps.Count, $a.name, $AppsRoot) "Warn"
      $rows += [pscustomobject]@{ id = $a.id; name = $a.name; category = $a.categoryName; exe = ""; status = "skip"; coldSec = $null; hotSec = $null; memMB = $null; error = "主程序未找到（未安装或未放入 Data\Apps）" }
      continue
    }
    Write-Ai5 ("[{0}/{1}] {2} -> {3}" -f $i, $m.apps.Count, $a.name, $exe) "Step"
    $r = Measure-AppStart -ExePath $exe
    if (-not $r.ok) {
      Write-Ai5 ("    启动失败: {0}" -f $r.error) "Err"
      $rows += [pscustomobject]@{ id = $a.id; name = $a.name; category = $a.categoryName; exe = $exe; status = "fail"; coldSec = $null; hotSec = $null; memMB = $null; error = $r.error }
      continue
    }
    $status = "pass"
    $notes = @()
    if ($null -ne $r.hot -and $r.hot -gt $budget.hotStartSec) { $status = "warn"; $notes += "热启动 $($r.hot)s 超预算 $($budget.hotStartSec)s" }
    if ($null -ne $r.cold -and $r.cold -gt $budget.coldStartSec) { $status = "warn"; $notes += "冷启动 $($r.cold)s 超预算 $($budget.coldStartSec)s" }
    if ($null -ne $r.memMB -and $r.memMB -gt $budget.memPerAppMB) { $status = "warn"; $notes += "内存 $($r.memMB)MB 超预算" }
    Write-Ai5 ("    冷={0}s 热={1}s 内存={2}MB -> {3} {4}" -f $r.cold, $r.hot, $r.memMB, $status, ($notes -join "; ")) $(if ($status -eq "pass") { "Ok" } else { "Warn" })
    $rows += [pscustomobject]@{ id = $a.id; name = $a.name; category = $a.categoryName; exe = $exe; status = $status; coldSec = $r.cold; hotSec = $r.hot; memMB = $r.memMB; error = ($notes -join "; ") }
  }

  $result = [pscustomobject]@{
    tool      = "Compat-Matrix.ps1"
    planRef   = "11.1"
    at        = (Get-Date -Format "o")
    host      = $env:COMPUTERNAME
    dataDrive = $DataDrive
    appsRoot  = $AppsRoot
    budget    = $budget
    counts    = @{
      total = $rows.Count
      pass  = @($rows | Where-Object { $_.status -eq "pass" }).Count
      warn  = @($rows | Where-Object { $_.status -eq "warn" }).Count
      fail  = @($rows | Where-Object { $_.status -eq "fail" }).Count
      skip  = @($rows | Where-Object { $_.status -eq "skip" }).Count
    }
    rows      = $rows
  }
  $out = Join-Path $OutDir "compat-results.json"
  Save-Ai5Json -Object $result -Path $out | Out-Null
  Write-Ai5 "结果已写入 $out" "Ok"
  Write-Ai5 ("通过 {0} / 有条件 {1} / 失败 {2} / 跳过 {3}" -f $result.counts.pass, $result.counts.warn, $result.counts.fail, $result.counts.skip)
  if ($result.counts.fail -gt 0) { return 1 }
  return 0
}

function Invoke-Report {
  $in = Join-Path $OutDir "compat-results.json"
  if (-not (Test-Path -LiteralPath $in)) {
    Write-Ai5 "没有结果文件 $in，先跑 -Action Run" "Warn"; return 1
  }
  $r = Get-Ai5Json -Path $in
  $m = (Get-Ai5Json -Path $MatrixFile)
  $rp = $ReportPath
  if (-not $rp) { $rp = Join-Path $OutDir "compat-report.md" }

  $L = @()
  $L += "# 兼容矩阵实测报告 (主计划 11.1)"
  $L += ""
  $L += "- 生成时间: $($r.at)"
  $L += "- 主机: $($r.host)  Data: $($r.dataDrive)  应用根: $($r.appsRoot)"
  $L += "- 矩阵规模: $($m.summary.total) 条 (5 类 × 40)，本轮实测 $($r.counts.total) 条"
  $L += "- 预算: 冷 ≤$($r.budget.coldStartSec)s / 热 ≤$($r.budget.hotStartSec)s / 内存 ≤$($r.budget.memPerAppMB)MB"
  $L += ""
  $L += "| 结果 | 数量 |"
  $L += "|---|---|"
  $L += "| ✅ pass | $($r.counts.pass) |"
  $L += "| ⚠️ warn | $($r.counts.warn) |"
  $L += "| ❌ fail | $($r.counts.fail) |"
  $L += "| ⏭ skip (未安装) | $($r.counts.skip) |"
  $L += ""
  $L += "## 明细"
  $L += ""
  $L += "| ID | 应用 | 类别 | 冷启动 | 热启动 | 内存 | 结果 | 备注 |"
  $L += "|---|---|---|---|---|---|---|---|"
  foreach ($row in $r.rows) {
    $cold = $(if ($null -ne $row.coldSec) { "$($row.coldSec)s" } else { "—" })
    $hot = $(if ($null -ne $row.hotSec) { "$($row.hotSec)s" } else { "—" })
    $mem = $(if ($null -ne $row.memMB) { "$($row.memMB)MB" } else { "—" })
    $icon = Get-Ai5VerdictIcon -Status $row.status
    $L += "| $($row.id) | $($row.name) | $($row.category) | $cold | $hot | $mem | $icon $($row.status) | $($row.error) |"
  }
  $L += ""
  $L += "> 口径: 冷启动=首次拉起进程到主窗口句柄出现; 热启动=紧接着第二次(文件系统缓存已热)。"
  $L += "> 未安装条目记 skip 而非 pass —— 矩阵的 ✅ 必须来自实测，不允许推断。"
  Save-Ai5Text -Lines $L -Path $rp | Out-Null
  Write-Ai5 "报告已写入 $rp" "Ok"
  return 0
}

$exit = 0
switch ($Action) {
  "List"         { Show-List }
  "Fill-ExeHint" { $exit = Invoke-FillExeHint }
  "Run"          { $exit = Invoke-Run }
  "Report"       { $exit = Invoke-Report }
}
exit $exit
