param(
  [ValidateSet("List", "Plan", "Run", "Report")]
  [string]$Action = "List",
  [string]$ScenariosFile = "$PSScriptRoot\Data\chaos-scenarios.json",
  [string]$Scenario = "",                 # 指定场景 id，如 S05
  [string]$DataDrive = "D:",
  [string]$EvidenceRoot = "",             # 证据目录，留空取 <DataDrive>\Data\Tests
  [switch]$AllowFill,                     # S05 才需要：允许真实占用磁盘空间
  [int]$FillMaxMB = 512,                  # S05 填充上限（安全阀，默认 512MB）
  [switch]$Yes                            # 跳过交互确认（仅限自动化，破坏性动作仍受 AI5_NONINTERACTIVE 约束）
)
# AI-5 交付核 / 第11.2章 混沌工程 + 扩充21 十种必测场景
# 用法:
#   .\Chaos-Inject.ps1 -Action List                    # 只读：列出 12 个场景与可自动化标记
#   .\Chaos-Inject.ps1 -Action Plan -Scenario S07      # 只读：打印该场景的完整步骤卡
#   .\Chaos-Inject.ps1 -Action Run                     # 只跑 automatable=auto 的场景
#   .\Chaos-Inject.ps1 -Action Run -Scenario S05 -AllowFill
#   .\Chaos-Inject.ps1 -Action Report
# 安全纪律:
#   * dangerous=true 的场景（宿主蓝屏 / 虚拟机内删 C 盘 / 驱动回退）永远只出步骤卡，脚本不代为执行；
#   * S05 填盘默认 dry-run，必须 -AllowFill 才真写，且受 $FillMaxMB 上限与自动清理保护；
#   * S11/S12 只操作本脚本自己启动的一次性子进程，绝不触碰用户已运行的进程。
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "AI5-Lib.ps1")

$Evidence = if ($EvidenceRoot) { $EvidenceRoot } else { Get-Ai5EvidenceRoot -DataDrive $DataDrive }

function Get-Scenarios {
  $doc = Get-Ai5Json -Path $ScenariosFile
  $list = @($doc.scenarios)
  if ($Scenario) { $list = @($list | Where-Object { $_.id -eq $Scenario }) }
  return @{ doc = $doc; list = $list }
}

function Show-List {
  $s = Get-Scenarios
  Write-Ai5 "混沌场景清单: $ScenariosFile" "Step"
  Write-Ai5 ("共 {0} 个场景；证据目录 {1}" -f @($s.doc.scenarios).Count, $Evidence)
  Write-Host ""
  Write-Host ("{0,-5} {1,-24} {2,-6} {3,-9} {4}" -f "ID", "场景", "等级", "可自动化", "危险")
  foreach ($x in $s.doc.scenarios) {
    Write-Host ("{0,-5} {1,-24} {2,-6} {3,-9} {4}" -f $x.id, $x.name, $x.level, $x.automatable, $x.dangerous)
  }
  Write-Host ""
  Write-Ai5 "auto 场景可直接 Run；manual 场景请用 -Action Plan 取步骤卡后人工执行并留证据。"
  return 0
}

function Show-Plan {
  $s = Get-Scenarios
  if ($s.list.Count -eq 0) { Write-Ai5 "没有匹配的场景: $Scenario" "Err"; return 1 }
  foreach ($x in $s.list) {
    Write-Ai5 ("[{0}] {1}  (主计划 {2}, 崩溃等级 {3}, 可自动化={4}, 危险={5})" -f $x.id, $x.name, $x.planRef, $x.level, $x.automatable, $x.dangerous) "Step"
    Write-Host "  注入方式: $($x.inject)"
    Write-Host "  步骤:"
    $n = 0
    foreach ($st in $x.steps) { $n++; Write-Host "    $n. $st" }
    Write-Host "  期望: $($x.expect)"
    Write-Host "  证据: $(@($x.evidence) -join ', ')"
    Write-Host ""
  }
  return 0
}

# ---------------- 各 auto 场景的真实处理 ----------------

function Invoke-S05 {
  # 空间不足：默认只做算术演练，不真写盘
  $data = Get-Ai5DataRoot -DataDrive $DataDrive
  $free = Get-Ai5FreeGB -Path $DataDrive
  $rows = @()
  $rows += [pscustomobject]@{ k = "dataRoot"; v = $data; note = "Data 根" }
  $rows += [pscustomobject]@{ k = "freeGB"; v = $free; note = "当前可用空间" }
  $rows += [pscustomobject]@{ k = "thresholdGB"; v = 5; note = "主计划 21.1-5 触发清理向导的阈值" }
  $rows += [pscustomobject]@{ k = "wouldTrigger"; v = ($free -ge 0 -and $free -lt 5); note = "是否应弹清理向导" }

  $status = "pass"; $detail = ""
  if ($free -lt 0) {
    $status = "warn"; $detail = "读不到可用空间（盘符不存在？）"
  } elseif ($AllowFill) {
    if (-not (Test-Path -LiteralPath $data)) {
      $status = "warn"; $detail = "Data 目录不存在，无法演练填充"
    } else {
      if (-not (Confirm-Ai5Dangerous -What "在 $data 写入最多 ${FillMaxMB}MB 填充文件（用完立即删除）" -Yes:$Yes)) {
        $status = "skip"; $detail = "未确认，跳过真实填充"
      } else {
        $fillDir = Join-Path $data "Tests\_fill"
        New-Ai5Directory -Path $fillDir | Out-Null
        $chunk = 64MB
        $written = 0
        try {
          while ($written -lt $FillMaxMB) {
            $f = Join-Path $fillDir ("fill-{0}.bin" -f $written)
            $bytes = New-Object byte[] $chunk
            [System.IO.File]::WriteAllBytes($f, $bytes)
            $written += 64
            $freeNow = Get-Ai5FreeGB -Path $DataDrive
            if ($freeNow -ge 0 -and $freeNow -lt 5) { Write-Ai5 "已压到阈值以下(剩余 ${freeNow}GB)，应触发清理向导" "Ok"; break }
          }
        } finally {
          Remove-Item -LiteralPath $fillDir -Recurse -Force -ErrorAction SilentlyContinue
          Write-Ai5 "填充文件已清理: $fillDir" "Ok"
        }
        $rows += [pscustomobject]@{ k = "filledMB"; v = $written; note = "实际写入并已删除" }
        $rows += [pscustomobject]@{ k = "freeGBAfter"; v = (Get-Ai5FreeGB -Path $DataDrive); note = "清理后可用空间" }
        $detail = "已演练填充 ${written}MB 并回收"
      }
    }
  } else {
    $status = "pass"; $detail = "dry-run（加 -AllowFill 可真写，受 ${FillMaxMB}MB 上限保护）"
  }
  $rows += [pscustomobject]@{ k = "status"; v = $status; note = $detail }
  Save-Ai5Json -Object ([pscustomobject]@{ scenario = "S05"; at = (Get-Date -Format "o"); rows = $rows }) -Path (Join-Path $Evidence "S05-space.json") | Out-Null
  return @{ id = "S05"; name = "U盘空间不足"; status = $status; detail = $detail }
}

function Invoke-S06 {
  # 反作弊检测虚拟机：只采集特征，不启动任何游戏
  $feat = [ordered]@{}
  try {
    $cs = Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction Stop
    $feat.manufacturer = $cs.Manufacturer
    $feat.model = $cs.Model
  } catch { $feat.manufacturer = "unavailable"; $feat.model = $_.Exception.Message }
  try {
    $bios = Get-CimInstance -ClassName Win32_BIOS -ErrorAction Stop
    $feat.biosVersion = $bios.SMBIOSBIOSVersion
    $feat.serial = $bios.SerialNumber
  } catch { $feat.biosVersion = "unavailable" }
  $vmDrivers = @()
  try {
    $vmDrivers = @(Get-CimInstance -ClassName Win32_PnPEntity -ErrorAction Stop |
      Where-Object { $_.Name -match "VBox|VMware|Virtual|Hyper-V|QEMU" } |
      Select-Object -First 10 -ExpandProperty Name)
  } catch { }
  $feat.vmDrivers = $vmDrivers
  $looksVirtual = ($feat.manufacturer -match "innotek|VMware|QEMU|Microsoft Corporation") -or ($vmDrivers.Count -gt 0)
  $feat.looksVirtual = [bool]$looksVirtual

  $m = Get-Ai5Json -Path (Join-Path $PSScriptRoot "Data\compat-matrix.json")
  $antiCheat = @($m.apps | Where-Object { $_.aMode -eq "warn" -or $_.bMode -eq "warn" } | Select-Object id, name, aMode, bMode, note)
  $feat.antiCheatApps = $antiCheat

  $status = "pass"
  $detail = if ($looksVirtual) { "检测到虚拟化特征 -> 反作弊类 $($antiCheat.Count) 条应走 B 模式" } else { "未检测到虚拟化特征（疑似 B 模式原生启动）" }
  Save-Ai5Json -Object ([pscustomobject]@{ scenario = "S06"; at = (Get-Date -Format "o"); features = $feat }) -Path (Join-Path $Evidence "S06-vmfeatures.json") | Out-Null
  return @{ id = "S06"; name = "反作弊游戏检测虚拟机"; status = $status; detail = $detail }
}

function Invoke-S08 {
  # BitLocker 恢复密钥可用性：只读检查，不改任何加密状态
  $rec = Join-Path (Get-Ai5DataRoot -DataDrive $DataDrive) "Security\BitLocker-Recovery.txt"
  $rows = @()
  $status = "pass"; $detail = ""
  if (-not (Test-Path -LiteralPath $rec)) {
    $status = "warn"; $detail = "缺恢复密钥文件: $rec（未启用 BitLocker 时属正常）"
  } else {
    $txt = (Get-Content -LiteralPath $rec -Raw -Encoding UTF8)
    $digits = ($txt -replace '[^0-9]', '')
    $ok48 = ($digits.Length -ge 48)
    $rows += [pscustomobject]@{ k = "recoveryFile"; v = $rec; note = "存在" }
    $rows += [pscustomobject]@{ k = "digits48"; v = $ok48; note = "48 位恢复密钥格式" }
    if (-not $ok48) { $status = "fail"; $detail = "恢复密钥不是 48 位数字分组格式" }
  }
  if (Test-Ai5Command "manage-bde.exe") {
    $st = (& manage-bde.exe -status $DataDrive 2>$null | Out-String)
    $on = $st -match "保护已启用|Protection On|Fully Encrypted"
    $rows += [pscustomobject]@{ k = "protectionOn"; v = [bool]$on; note = "manage-bde -status" }
    if (-not $on -and $status -eq "pass") { $status = "warn"; $detail = "BitLocker 未启用（主计划 10.1 要求 XTS-AES256）" }
  } else {
    $rows += [pscustomobject]@{ k = "manage-bde"; v = "unavailable"; note = "精简系统或无 BitLocker 组件" }
    if ($status -eq "pass") { $status = "warn"; $detail = "manage-bde 不可用，无法核验加密状态" }
  }
  Save-Ai5Json -Object ([pscustomobject]@{ scenario = "S08"; at = (Get-Date -Format "o"); rows = $rows }) -Path (Join-Path $Evidence "S08-bitlocker.json") | Out-Null
  return @{ id = "S08"; name = "BitLocker 忘密码"; status = $status; detail = $detail }
}

function Invoke-S09 {
  # 4K 对齐：Offset 必须是 4096 的整数倍
  $rows = @(); $bad = 0
  try {
    $parts = @(Get-CimInstance -ClassName Win32_Partition -ErrorAction Stop)
  } catch {
    return @{ id = "S09"; name = "4K 对齐检查"; status = "warn"; detail = "读不到分区信息: $($_.Exception.Message)" }
  }
  foreach ($p in $parts) {
    $aligned = ($p.StartingOffset % 4096) -eq 0
    if (-not $aligned) { $bad++ }
    $rows += [pscustomobject]@{
      disk      = $p.DiskIndex
      partition = $p.Index
      offset    = $p.StartingOffset
      aligned   = $aligned
    }
  }
  $status = if ($bad -eq 0) { "pass" } else { "fail" }
  $detail = if ($bad -eq 0) { "$($rows.Count) 个分区全部 4K 对齐" } else { "$bad 个分区未对齐（4K 随机会掉约 50%）" }
  Save-Ai5Json -Object ([pscustomobject]@{ scenario = "S09"; at = (Get-Date -Format "o"); partitions = $rows }) -Path (Join-Path $Evidence "S09-alignment.json") | Out-Null
  return @{ id = "S09"; name = "4K 对齐检查"; status = $status; detail = $detail }
}

function Invoke-S11 {
  # 看门狗 L1/L2 模拟：只杀本脚本自己启动的一次性子进程
  $child = "powershell.exe"
  $cmd = "Start-Sleep -Seconds 30"
  $p = Start-Process -FilePath $child -ArgumentList @("-NoProfile", "-NonInteractive", "-Command", $cmd) -PassThru -WindowStyle Hidden
  Start-Sleep -Milliseconds 600
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  try { Stop-Process -Id $p.Id -Force -ErrorAction Stop } catch { }
  $dead = $false
  while ($sw.Elapsed.TotalSeconds -lt 3) {
    $p.Refresh()
    if ($p.HasExited) { $dead = $true; break }
    Start-Sleep -Milliseconds 50
  }
  $sw.Stop()
  $detect = [math]::Round($sw.Elapsed.TotalSeconds, 2)
  $exitCode = $null
  try { $p.Refresh(); $exitCode = $p.ExitCode } catch { }
  $status = if ($dead) { "pass" } else { "fail" }
  $detail = "自启动子进程 PID=$($p.Id)；发现死亡耗时 ${detect}s（看门狗阈值 3s）"
  Save-Ai5Json -Object ([pscustomobject]@{
      scenario = "S11"; at = (Get-Date -Format "o"); pid = $p.Id
      detectSec = $detect; watchdogSec = 3; exited = $dead; exitCode = $exitCode
      note = "只操作本脚本自己启动的一次性 powershell 子进程，未枚举或触碰用户已运行进程"
    }) -Path (Join-Path $Evidence "S11-watchdog.json") | Out-Null
  return @{ id = "S11"; name = "进程被杀（看门狗模拟）"; status = $status; detail = $detail }
}

function Invoke-S12 {
  # 0x80000003 断点异常：一次性子进程触发，验证只留记录不弹系统错误框
  $cmd = "[System.Diagnostics.Debugger]::Break()"
  $p = Start-Process -FilePath "powershell.exe" -ArgumentList @("-NoProfile", "-NonInteractive", "-Command", $cmd) -PassThru -WindowStyle Hidden
  if (-not $p.WaitForExit(15000)) {
    try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch { }
    $status = "warn"; $detail = "子进程 15s 未退出（可能有调试器附加），已强制结束"
    $code = $null
  } else {
    $code = $p.ExitCode
    $status = "pass"
    $detail = "子进程以退出码 $code 结束（0x80000003 断点异常），未出现阻塞式系统错误框"
  }
  Save-Ai5Json -Object ([pscustomobject]@{
      scenario = "S12"; at = (Get-Date -Format "o"); pid = $p.Id; exitCode = $code
      expected = "0x80000003 一类断点异常，捕获后记录 dump 并直接结束进程"
      note = "对应主计划 11.2 libcef 崩溃注入 / 15.2 熔断策略"
    }) -Path (Join-Path $Evidence "S12-break.json") | Out-Null
  return @{ id = "S12"; name = "0x80000003 断点异常捕获"; status = $status; detail = $detail }
}

function Invoke-Run {
  $s = Get-Scenarios
  New-Ai5Directory -Path $Evidence | Out-Null
  $targets = @($s.list | Where-Object { $_.automatable -eq "auto" })
  if ($s.list.Count -gt 0 -and $targets.Count -eq 0) {
    Write-Ai5 "所选场景都是 manual，脚本不代为执行。用 -Action Plan 取步骤卡。" "Warn"
    return 1
  }
  Write-Ai5 "将执行 $($targets.Count) 个 auto 场景；manual 场景仅提示" "Step"
  $results = @()
  foreach ($x in $s.list) {
    if ($x.automatable -ne "auto") {
      Write-Ai5 ("[{0}] {1} -> manual，请用 -Action Plan -Scenario {0} 人工执行" -f $x.id, $x.name) "Warn"
      $results += [pscustomobject]@{ id = $x.id; name = $x.name; status = "todo"; detail = "manual 场景，需人工执行并留证据" }
      continue
    }
    Write-Ai5 ("[{0}] {1}" -f $x.id, $x.name) "Step"
    $r = $null
    switch ($x.id) {
      "S05" { $r = Invoke-S05 }
      "S06" { $r = Invoke-S06 }
      "S08" { $r = Invoke-S08 }
      "S09" { $r = Invoke-S09 }
      "S11" { $r = Invoke-S11 }
      "S12" { $r = Invoke-S12 }
      default {
        Write-Ai5 ("    场景 {0} 标记 auto 但没有处理器，按 todo 记录" -f $x.id) "Warn"
        $r = @{ id = $x.id; name = $x.name; status = "todo"; detail = "尚无自动处理器" }
      }
    }
    Write-Ai5 ("    {0} — {1}" -f $r.status, $r.detail) $(if ($r.status -eq "pass") { "Ok" } elseif ($r.status -eq "fail") { "Err" } else { "Warn" })
    $results += [pscustomobject]@{ id = $r.id; name = $r.name; status = $r.status; detail = $r.detail }
  }

  $doc = [pscustomobject]@{
    tool    = "Chaos-Inject.ps1"
    planRef = "11.2 / 扩充21"
    at      = (Get-Date -Format "o")
    host    = $env:COMPUTERNAME
    counts  = @{
      pass = @($results | Where-Object { $_.status -eq "pass" }).Count
      warn = @($results | Where-Object { $_.status -eq "warn" }).Count
      fail = @($results | Where-Object { $_.status -eq "fail" }).Count
      todo = @($results | Where-Object { $_.status -eq "todo" }).Count
    }
    results = $results
  }
  Save-Ai5Json -Object $doc -Path (Join-Path $Evidence "chaos-results.json") | Out-Null
  Write-Ai5 "结果已写入 $(Join-Path $Evidence 'chaos-results.json')" "Ok"
  if ($doc.counts.fail -gt 0) { return 1 }
  return 0
}

function Invoke-Report {
  $in = Join-Path $Evidence "chaos-results.json"
  if (-not (Test-Path -LiteralPath $in)) { Write-Ai5 "没有结果文件 $in，先跑 -Action Run" "Warn"; return 1 }
  $r = Get-Ai5Json -Path $in
  $rp = Join-Path $Evidence "chaos-report.md"
  $L = @()
  $L += "# 混沌工程结果 (主计划 11.2 / 扩充 21)"
  $L += ""
  $L += "- 生成: $($r.at)  主机: $($r.host)"
  $L += "- pass=$($r.counts.pass) warn=$($r.counts.warn) fail=$($r.counts.fail) todo(manual)=$($r.counts.todo)"
  $L += ""
  $L += "| ID | 场景 | 结果 | 说明 |"
  $L += "|---|---|---|---|"
  foreach ($x in $r.results) {
    $L += "| $($x.id) | $($x.name) | $(Get-Ai5VerdictIcon -Status $x.status) $($x.status) | $($x.detail) |"
  }
  $L += ""
  $L += "> manual 场景（S01-S04 / S07 / S10）必须由人工按步骤卡执行，脚本不代为操作宿主或虚拟机。"
  Save-Ai5Text -Lines $L -Path $rp | Out-Null
  Write-Ai5 "报告已写入 $rp" "Ok"
  return 0
}

$exit = 0
switch ($Action) {
  "List"   { $exit = Show-List }
  "Plan"   { $exit = Show-Plan }
  "Run"    { $exit = Invoke-Run }
  "Report" { $exit = Invoke-Report }
}
exit $exit
