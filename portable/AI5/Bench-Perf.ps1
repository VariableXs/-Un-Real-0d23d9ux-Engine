param(
  [ValidateSet("Run", "Gate", "Report", "Manifest")]
  [string]$Action = "Manifest",
  [string]$TestDrive = "",                # 被测卷，默认取 $DataDrive 所在卷；例: E:\
  [string]$DataDrive = "D:",
  [int]$SeqMB = 512,                      # 顺序读写测试量
  [int]$RandMB = 64,                      # 4K 随机读测试量
  [string]$ProcessPath = "",              # 启动计时对象，默认 powershell.exe -NoProfile -Command exit
  [int]$StartRounds = 3,
  [string]$OutDir = "D:\Data\Tests",
  [string]$RepoBenchPath = "",            # -WriteRepo 时写入 docs/bench/<date>.md
  [switch]$WriteRepo,
  [switch]$Keep                           # 保留测试文件（默认写完即删）
)
# AI-5 交付核 / 第11.3章 性能基线 + 扩充17 压测口径
# 用法:
#   .\Bench-Perf.ps1 -Action Manifest                # 只读：打印指标与门禁阈值
#   .\Bench-Perf.ps1 -Action Run -TestDrive E:\      # 实测：顺序/4K/启动/内存
#   .\Bench-Perf.ps1 -Action Run -WriteRepo          # 同时写入仓库 docs/bench/<日期>.md
#   .\Bench-Perf.ps1 -Action Gate                    # 用上次结果做门禁（超预算退出码 1）
#   .\Bench-Perf.ps1 -Action Report
# 口径说明（务必如实标注）:
#   * 顺序/4K 用 .NET FileStream 无缓存写 + 重读，量级可对比，但不等于 CrystalDiskMark 的裸盘值；
#   * 启动时间测的是"进程拉起并退出"的墙钟时间，GUI 应用的"可交互"时间需 Compat-Matrix.ps1 实测；
#   * 内存为 variable* 进程 WorkingSet 之和，不含 WebView2 渲染子进程（与 tools/bench.cjs 口径一致）。
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "AI5-Lib.ps1")

$Budget = [ordered]@{
  seqWriteMBps   = 400    # 主计划 9.1 持续读写 ≥400MB/s
  seqReadMBps    = 900    # 主计划 1.3 SEQ ≥900
  rand4kReadMBps = 20     # 主计划 9.1 4K 随机 ≥20MB/s
  hotStartSec    = 6      # 大软件 6 秒
  coldStartSec   = 18
  systemBootSec  = 12
  memoryMB       = 600    # 与 tools/bench.cjs 的 ≤600MB 预算一致
}

function Resolve-TestRoot {
  if ($TestDrive) { return $TestDrive.TrimEnd('\') }
  return (Get-Ai5DataRoot -DataDrive $DataDrive)
}

function Measure-Sequential {
  param([Parameter(Mandatory = $true)][string]$Root, [int]$Megabytes)
  $dir = Join-Path $Root "Tests\_bench"
  New-Ai5Directory -Path $dir | Out-Null
  $file = Join-Path $dir "seq.bin"
  $chunk = 4MB
  $count = [int](($Megabytes * 1MB) / $chunk)
  $buf = New-Object byte[] $chunk
  (New-Object Random).NextBytes($buf)
  $res = @{ ok = $false; writeMBps = $null; readMBps = $null; error = "" }
  try {
    $fs = [System.IO.File]::Create($file)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    for ($i = 0; $i -lt $count; $i++) { $fs.Write($buf, 0, $chunk) }
    $fs.Flush($true)
    $sw.Stop(); $fs.Close()
    $res.writeMBps = [math]::Round($Megabytes / $sw.Elapsed.TotalSeconds, 1)

    $fs2 = New-Object System.IO.FileStream($file, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read, $chunk, [System.IO.FileOptions]::SequentialScan)
    $rb = New-Object byte[] $chunk
    $sw2 = [System.Diagnostics.Stopwatch]::StartNew()
    $read = 0
    while (($n = $fs2.Read($rb, 0, $chunk)) -gt 0) { $read += $n }
    $sw2.Stop(); $fs2.Close()
    $res.readMBps = [math]::Round(($read / 1MB) / $sw2.Elapsed.TotalSeconds, 1)
    $res.ok = $true
  } catch {
    $res.error = $_.Exception.Message
  } finally {
    if (-not $Keep) { Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue }
  }
  return $res
}

function Measure-Random4k {
  param([Parameter(Mandatory = $true)][string]$Root, [int]$Megabytes)
  $dir = Join-Path $Root "Tests\_bench"
  New-Ai5Directory -Path $dir | Out-Null
  $file = Join-Path $dir "rand.bin"
  $res = @{ ok = $false; readMBps = $null; iops = $null; error = "" }
  try {
    $total = $Megabytes * 1MB
    $fs = [System.IO.File]::Create($file)
    $fill = New-Object byte[] (4MB)
    (New-Object Random).NextBytes($fill)
    $written = 0
    while ($written -lt $total) { $fs.Write($fill, 0, [Math]::Min(4MB, $total - $written)); $written += 4MB }
    $fs.Flush($true); $fs.Close()

    $fs2 = New-Object System.IO.FileStream($file, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read, 4096, [System.IO.FileOptions]::RandomAccess)
    $rnd = New-Object Random
    $buf = New-Object byte[] 4096
    $ops = 0
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt 5) {
      $pos = [int64]($rnd.NextDouble() * ($total - 4096))
      $fs2.Seek($pos, [System.IO.SeekOrigin]::Begin) | Out-Null
      if ($fs2.Read($buf, 0, 4096) -eq 4096) { $ops++ }
    }
    $sw.Stop(); $fs2.Close()
    $res.readMBps = [math]::Round((($ops * 4096) / 1MB) / $sw.Elapsed.TotalSeconds, 2)
    $res.iops = [math]::Round($ops / $sw.Elapsed.TotalSeconds, 0)
    $res.ok = $true
  } catch {
    $res.error = $_.Exception.Message
  } finally {
    if (-not $Keep) { Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue }
  }
  return $res
}

function Measure-ProcessStart {
  param([string]$Path, [int]$Rounds)
  # 不覆盖自动变量 $args，用独立的 $procArgs
  $exe = "powershell.exe"
  $procArgs = @("-NoProfile", "-NonInteractive", "-Command", "exit")
  if ($Path) {
    $exe = $Path
    $procArgs = @()
  }
  $times = @()
  for ($i = 0; $i -lt $Rounds; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
      $p = Start-Process -FilePath $exe -ArgumentList $procArgs -PassThru -WindowStyle Hidden
      if (-not $p.WaitForExit(30000)) { try { Stop-Process -Id $p.Id -Force } catch { } }
    } catch {
      return @{ ok = $false; cold = $null; hot = $null; error = $_.Exception.Message }
    }
    $sw.Stop()
    $times += [math]::Round($sw.Elapsed.TotalSeconds, 3)
  }
  $cold = $times[0]
  $hot = $null
  if ($times.Count -gt 1) {
    $rest = $times | Select-Object -Skip 1
    $hot = [math]::Round((($rest | Measure-Object -Average).Average), 3)
  }
  return @{ ok = $true; cold = $cold; hot = $hot; rounds = $times; error = "" }
}

function Get-VariableMemoryMB {
  try {
    $procs = @(Get-Process -ErrorAction Stop | Where-Object { $_.ProcessName -like "variable*" })
    if ($procs.Count -eq 0) { return $null }
    return [math]::Round((($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB), 0)
  } catch { return $null }
}

function Show-Manifest {
  Write-Ai5 "AI-5 性能基线清单 (主计划 11.3 / 扩充17)" "Step"
  Write-Host ("{0,-16} {1,-14} {2}" -f "指标", "预算", "口径")
  Write-Host ("{0,-16} {1,-14} {2}" -f "seqWriteMBps", "≥$($Budget.seqWriteMBps)", "FileStream 无缓存顺序写")
  Write-Host ("{0,-16} {1,-14} {2}" -f "seqReadMBps", "≥$($Budget.seqReadMBps)", "FileStream 顺序重读")
  Write-Host ("{0,-16} {1,-14} {2}" -f "rand4kReadMBps", "≥$($Budget.rand4kReadMBps)", "4KB 随机读 5 秒取样")
  Write-Host ("{0,-16} {1,-14} {2}" -f "hotStartSec", "≤$($Budget.hotStartSec)", "进程第二次拉起（缓存已热）")
  Write-Host ("{0,-16} {1,-14} {2}" -f "coldStartSec", "≤$($Budget.coldStartSec)", "进程首次拉起")
  Write-Host ("{0,-16} {1,-14} {2}" -f "systemBootSec", "≤$($Budget.systemBootSec)", "虚拟机上电到桌面（需 Test-VM 真机测）")
  Write-Host ("{0,-16} {1,-14} {2}" -f "memoryMB", "≤$($Budget.memoryMB)", "variable* 进程 WorkingSet 之和")
  Write-Host ""
  Write-Ai5 "本脚本能自动测前 5 项 + 内存；systemBootSec 需在真机上由 Test-VM.ps1 配合秒表/日志采集。" "Warn"
  return 0
}

function Invoke-Run {
  $root = Resolve-TestRoot
  if (-not (Test-Path -LiteralPath $root)) {
    Write-Ai5 "被测目录不存在: $root（先建 Data 或指定 -TestDrive）" "Err"; return 1
  }
  New-Ai5Directory -Path $OutDir | Out-Null
  Write-Ai5 "被测卷: $root  顺序 ${SeqMB}MB / 4K ${RandMB}MB" "Step"

  $seq = Measure-Sequential -Root $root -Megabytes $SeqMB
  $rnd = Measure-Random4k -Root $root -Megabytes $RandMB
  $st = Measure-ProcessStart -Path $ProcessPath -Rounds $StartRounds
  $mem = Get-VariableMemoryMB

  $rows = @()
  $rows += [pscustomobject]@{ metric = "seqWriteMBps";   value = $seq.writeMBps;  budget = "≥$($Budget.seqWriteMBps)";  pass = $(if ($null -ne $seq.writeMBps) { $seq.writeMBps -ge $Budget.seqWriteMBps } else { $null }) }
  $rows += [pscustomobject]@{ metric = "seqReadMBps";    value = $seq.readMBps;   budget = "≥$($Budget.seqReadMBps)";   pass = $(if ($null -ne $seq.readMBps) { $seq.readMBps -ge $Budget.seqReadMBps } else { $null }) }
  $rows += [pscustomobject]@{ metric = "rand4kReadMBps"; value = $rnd.readMBps;   budget = "≥$($Budget.rand4kReadMBps)"; pass = $(if ($null -ne $rnd.readMBps) { $rnd.readMBps -ge $Budget.rand4kReadMBps } else { $null }); iops = $rnd.iops }
  $rows += [pscustomobject]@{ metric = "coldStartSec";   value = $st.cold;        budget = "≤$($Budget.coldStartSec)";  pass = $(if ($null -ne $st.cold) { $st.cold -le $Budget.coldStartSec } else { $null }) }
  $rows += [pscustomobject]@{ metric = "hotStartSec";    value = $st.hot;         budget = "≤$($Budget.hotStartSec)";   pass = $(if ($null -ne $st.hot) { $st.hot -le $Budget.hotStartSec } else { $null }) }
  $rows += [pscustomobject]@{ metric = "memoryMB";       value = $mem;            budget = "≤$($Budget.memoryMB)";      pass = $(if ($null -ne $mem) { $mem -le $Budget.memoryMB } else { $null }) }
  $rows += [pscustomobject]@{ metric = "systemBootSec";  value = $null;           budget = "≤$($Budget.systemBootSec)"; pass = $null; note = "需真机 Test-VM 实测" }

  foreach ($r in $rows) {
    $icon = if ($null -eq $r.pass) { "—" } elseif ($r.pass) { "✅" } else { "❌" }
    Write-Ai5 ("{0} {1,-16} = {2}  (预算 {3})" -f $icon, $r.metric, $r.value, $r.budget) $(if ($r.pass -eq $false) { "Err" } else { "Info" })
  }
  if ($seq.error) { Write-Ai5 "顺序测试错误: $($seq.error)" "Warn" }
  if ($rnd.error) { Write-Ai5 "4K 测试错误: $($rnd.error)" "Warn" }
  if ($st.error)  { Write-Ai5 "启动测试错误: $($st.error)" "Warn" }

  $doc = [pscustomobject]@{
    tool      = "Bench-Perf.ps1"
    planRef   = "11.3 / 扩充17"
    at        = (Get-Date -Format "o")
    host      = $env:COMPUTERNAME
    testRoot  = $root
    osVersion = $(try { [System.Environment]::OSVersion.Version.ToString() } catch { "unknown" })
    psVersion = $PSVersionTable.PSVersion.ToString()
    budget    = $Budget
    rows      = $rows
  }
  $out = Join-Path $OutDir "bench-results.json"
  Save-Ai5Json -Object $doc -Path $out | Out-Null
  Write-Ai5 "结果已写入 $out" "Ok"

  if ($WriteRepo) {
    $repoPath = $RepoBenchPath
    if (-not $repoPath) {
      $repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
      $repoPath = Join-Path $repoRoot ("docs\bench\" + (Get-Date -Format "yyyy-MM-dd") + "-portable.md")
    }
    $L = @()
    $L += "# 便携系统性能基线 — $(Get-Date -Format yyyy-MM-dd)"
    $L += ""
    $L += "- 来源：``portable/AI5/Bench-Perf.ps1``（主计划 11.3 / 扩充 17）"
    $L += "- 环境：$($doc.host) · Windows $($doc.osVersion) · PowerShell $($doc.psVersion) · 被测卷 ``$root``"
    $L += "- 参数：顺序 ${SeqMB}MB / 4K ${RandMB}MB / 启动 $StartRounds 轮"
    $L += ""
    $L += "| 指标 | 实测 | 预算 | 判定 |"
    $L += "| --- | --- | --- | --- |"
    foreach ($r in $rows) {
      $v = $(if ($null -eq $r.value) { "—" } else { "$($r.value)" })
      $icon = if ($null -eq $r.pass) { "⏭ 待真机" } elseif ($r.pass) { "✅" } else { "❌" }
      $L += "| $($r.metric) | $v | $($r.budget) | $icon |"
    }
    $L += ""
    $L += "> 口径：顺序/4K 为 FileStream 用户态读写，量级可横向对比，不等同于 CrystalDiskMark 裸盘值；"
    $L += "> 启动时间为进程墙钟时间；``systemBootSec`` 需配合 ``portable/Test-VM.ps1`` 在真机采集，未测即标注 ⏭，不推断。"
    Save-Ai5Text -Lines $L -Path $repoPath | Out-Null
    Write-Ai5 "仓库基线已写入 $repoPath" "Ok"
  }

  $failed = @($rows | Where-Object { $_.pass -eq $false })
  if ($failed.Count -gt 0) { Write-Ai5 "$($failed.Count) 项未达预算" "Err"; return 1 }
  return 0
}

function Invoke-Gate {
  $in = Join-Path $OutDir "bench-results.json"
  if (-not (Test-Path -LiteralPath $in)) { Write-Ai5 "没有结果文件 $in，先跑 -Action Run" "Warn"; return 1 }
  $r = Get-Ai5Json -Path $in
  $failed = @($r.rows | Where-Object { $_.pass -eq $false })
  $pending = @($r.rows | Where-Object { $null -eq $_.pass })
  $okc = @($r.rows | Where-Object { $_.pass -eq $true })
  Write-Ai5 ("门禁: 通过 {0} / 未达 {1} / 待真机 {2}" -f $okc.Count, $failed.Count, $pending.Count) $(if ($failed.Count -gt 0) { "Err" } else { "Ok" })
  foreach ($f in $failed) { Write-Ai5 ("  未达: {0} = {1} (预算 {2})" -f $f.metric, $f.value, $f.budget) "Err" }
  foreach ($p in $pending) { Write-Ai5 ("  待真机: {0}" -f $p.metric) "Warn" }
  if ($failed.Count -gt 0) { return 1 }
  return 0
}

function Invoke-Report {
  $in = Join-Path $OutDir "bench-results.json"
  if (-not (Test-Path -LiteralPath $in)) { Write-Ai5 "没有结果文件 $in" "Warn"; return 1 }
  $r = Get-Ai5Json -Path $in
  $rp = Join-Path $OutDir "bench-report.md"
  $L = @()
  $L += "# 性能基线报告 (主计划 11.3)"
  $L += ""
  $L += "- 生成: $($r.at)  主机: $($r.host)  被测卷: $($r.testRoot)"
  $L += ""
  $L += "| 指标 | 实测 | 预算 | 判定 |"
  $L += "|---|---|---|---|"
  foreach ($x in $r.rows) {
    $v = $(if ($null -eq $x.value) { "—" } else { "$($x.value)" })
    $icon = if ($null -eq $x.pass) { "⏭" } elseif ($x.pass) { "✅" } else { "❌" }
    $L += "| $($x.metric) | $v | $($x.budget) | $icon |"
  }
  Save-Ai5Text -Lines $L -Path $rp | Out-Null
  Write-Ai5 "报告已写入 $rp" "Ok"
  return 0
}

$exit = 0
switch ($Action) {
  "Manifest" { $exit = Show-Manifest }
  "Run"      { $exit = Invoke-Run }
  "Gate"     { $exit = Invoke-Gate }
  "Report"   { $exit = Invoke-Report }
}
exit $exit
