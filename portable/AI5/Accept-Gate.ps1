param(
  [ValidateSet("Report", "Check", "Init")]
  [string]$Action = "Report",
  [string]$DataDrive = "D:",
  [string]$OutDir = "D:\Data\Tests",
  [string]$EvidenceRoot = "",             # 证据目录，留空取 <DataDrive>\Data\Tests
  [string]$RepoRoot = "",                 # 留空自动推导仓库根
  [string]$UsbDrive = "",                 # 成品盘盘符，用于阶段4交付物核验
  [string]$ManualResults = "",            # 人工实测结果 JSON（覆盖 manual 项）
  [switch]$Strict                         # 任一未实测项即视为不通过
)
# AI-5 交付核 / 主计划 1.3 成功标准 + 扩充19 交付清单 + 扩充28 验收单
# 用法:
#   .\Accept-Gate.ps1 -Action Init                    # 只读：生成空白人工实测模板
#   .\Accept-Gate.ps1 -Action Report                  # 只读：汇总所有证据 -> 验收报告
#   .\Accept-Gate.ps1 -Action Report -UsbDrive E:\    # 额外核验成品盘交付物
#   .\Accept-Gate.ps1 -Action Check                   # 门禁：有 fail 则退出码 1
#   .\Accept-Gate.ps1 -Action Check -Strict           # 更严：未实测也算不通过
# 原则: 验收状态只有三种来源 —— 自动脚本实测 / 人工实测录入 / 明确标注"待真机"。
#       没有证据的项一律记 todo，绝不自动打 ✅。
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "AI5-Lib.ps1")

$Evidence = if ($EvidenceRoot) { $EvidenceRoot } else { Get-Ai5EvidenceRoot -DataDrive $DataDrive }

# 主计划 1.3 + 扩充28 的验收项定义
$Gate = @(
  [pscustomobject]@{ id = "A01"; planRef = "1.3 / 28"; name = "5 台机 A/B 双模式各启动一次"; auto = "manual" }
  [pscustomobject]@{ id = "A02"; planRef = "1.3 / 28"; name = "系统启动 ≤12s"; auto = "manual" }
  [pscustomobject]@{ id = "A03"; planRef = "1.3 / 28"; name = "大软件启动 ≤6s（热）"; auto = "compat" }
  [pscustomobject]@{ id = "A04"; planRef = "11.1"; name = "兼容矩阵 Top200 A/B 双模式跑通"; auto = "compat" }
  [pscustomobject]@{ id = "A05"; planRef = "11.2 / 21"; name = "混沌 10 场景全部有结论"; auto = "chaos" }
  [pscustomobject]@{ id = "A06"; planRef = "11.3"; name = "顺序读 ≥900MB/s"; auto = "bench" }
  [pscustomobject]@{ id = "A07"; planRef = "11.3"; name = "4K 随机读 ≥20MB/s"; auto = "bench" }
  [pscustomobject]@{ id = "A08"; planRef = "11.3"; name = "待机内存 ≤600MB"; auto = "bench" }
  [pscustomobject]@{ id = "A09"; planRef = "21.1-3 / 4.1"; name = "虚拟机内删 C 盘，宿主无影响"; auto = "manual" }
  [pscustomobject]@{ id = "A10"; planRef = "21.1-1 / 4.7"; name = "拔盘宿主无痕迹"; auto = "manual" }
  [pscustomobject]@{ id = "A11"; planRef = "10.1"; name = "BitLocker XTS-AES256 + 拔盘即锁"; auto = "bitlocker" }
  [pscustomobject]@{ id = "A12"; planRef = "12.5 / 22.2"; name = "一键还原（User.vhdx 回滚）可用"; auto = "restore" }
  [pscustomobject]@{ id = "A13"; planRef = "12.4 / 19.1"; name = "成品盘交付物齐全"; auto = "deliver" }
  [pscustomobject]@{ id = "A14"; planRef = "21.1-9 / 29.4"; name = "全部分区 4K 对齐"; auto = "align" }
)

function Read-IfExists { param([string]$Path) if (Test-Path -LiteralPath $Path) { return (Get-Ai5Json -Path $Path) } return $null }

function Get-RepoRoot {
  if ($RepoRoot) { return $RepoRoot }
  try { return (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path } catch { return "" }
}

function Invoke-Init {
  $tpl = [pscustomobject]@{
    note    = "人工实测结果模板：把每条的 status 改成 pass/warn/fail，并填 evidence 路径与 at 时间。Accept-Gate 会读取本文件覆盖 manual 项。"
    results = @($Gate | ForEach-Object {
        [pscustomobject]@{ id = $_.id; name = $_.name; status = "todo"; detail = ""; evidence = ""; at = "" }
      })
  }
  $out = Join-Path $OutDir "manual-results.json"
  Save-Ai5Json -Object $tpl -Path $out | Out-Null
  Write-Ai5 "人工实测模板已生成: $out" "Ok"
  Write-Ai5 "填好后用 -Action Report -ManualResults $out 汇总"
  return 0
}

function Get-Verdicts {
  $compat = Read-IfExists (Join-Path $OutDir "compat-results.json")
  $chaos = Read-IfExists (Join-Path $Evidence "chaos-results.json")
  $bench = Read-IfExists (Join-Path $OutDir "bench-results.json")
  $manual = $null
  if ($ManualResults) { $manual = Read-IfExists $ManualResults }
  else { $manual = Read-IfExists (Join-Path $OutDir "manual-results.json") }

  $rows = @()
  foreach ($g in $Gate) {
    $status = "todo"; $detail = "尚无证据"; $evidence = ""

    switch ($g.auto) {
      "manual" {
        if ($manual) {
          $hit = $manual.results | Where-Object { $_.id -eq $g.id } | Select-Object -First 1
          if ($hit -and $hit.status -and $hit.status -ne "todo") {
            $status = $hit.status; $detail = $hit.detail; $evidence = $hit.evidence
          } else { $detail = "人工实测未录入（-Action Init 生成模板）" }
        }
      }
      "compat" {
        if ($compat) {
          $c = $compat.counts
          if ($g.id -eq "A04") {
            $status = $(if ($c.fail -gt 0) { "fail" } elseif (($c.pass + $c.warn) -eq 0) { "todo" } else { "pass" })
            $detail = "pass=$($c.pass) warn=$($c.warn) fail=$($c.fail) skip=$($c.skip) / 共 $($c.total)"
          } else {
            $over = @($compat.rows | Where-Object { $null -ne $_.hotSec -and $_.hotSec -gt $compat.budget.hotStartSec })
            $status = $(if ($compat.rows.Count -eq 0) { "todo" } elseif ($over.Count -gt 0) { "warn" } else { "pass" })
            $detail = "热启动超预算条目 $($over.Count) 个"
          }
          $evidence = Join-Path $OutDir "compat-results.json"
        }
      }
      "chaos" {
        if ($chaos) {
          $c = $chaos.counts
          $status = $(if ($c.fail -gt 0) { "fail" } elseif (($c.pass + $c.warn) -eq 0) { "todo" } else { "warn" })
          $detail = "pass=$($c.pass) warn=$($c.warn) fail=$($c.fail) manual待做=$($c.todo)"
          $evidence = Join-Path $Evidence "chaos-results.json"
        }
      }
      "bench" {
        if ($bench) {
          $key = switch ($g.id) { "A06" { "seqReadMBps" } "A07" { "rand4kReadMBps" } default { "memoryMB" } }
          $hit = $bench.rows | Where-Object { $_.metric -eq $key } | Select-Object -First 1
          if ($hit) {
            $status = $(if ($null -eq $hit.pass) { "todo" } elseif ($hit.pass) { "pass" } else { "fail" })
            $detail = "$key = $($hit.value) (预算 $($hit.budget))"
          }
          $evidence = Join-Path $OutDir "bench-results.json"
        }
      }
      "bitlocker" {
        $s8 = Read-IfExists (Join-Path $Evidence "S08-bitlocker.json")
        if ($s8) {
          $on = $s8.rows | Where-Object { $_.k -eq "protectionOn" } | Select-Object -First 1
          $k48 = $s8.rows | Where-Object { $_.k -eq "digits48" } | Select-Object -First 1
          if ($on -and $on.v -eq $true -and (-not $k48 -or $k48.v -eq $true)) { $status = "pass"; $detail = "加密已启用且恢复密钥格式正确" }
          elseif ($k48 -and $k48.v -eq $false) { $status = "fail"; $detail = "恢复密钥格式不正确" }
          else { $status = "warn"; $detail = "未检测到 BitLocker 已启用" }
          $evidence = Join-Path $Evidence "S08-bitlocker.json"
        }
      }
      "align" {
        $s9 = Read-IfExists (Join-Path $Evidence "S09-alignment.json")
        if ($s9) {
          $bad = @($s9.partitions | Where-Object { $_.aligned -eq $false })
          $status = $(if ($bad.Count -eq 0) { "pass" } else { "fail" })
          $detail = "分区 $(@($s9.partitions).Count) 个，未对齐 $($bad.Count) 个"
          $evidence = Join-Path $Evidence "S09-alignment.json"
        }
      }
      "restore" {
        $bk = Join-Path (Get-Ai5DataRoot -DataDrive $DataDrive) "Backup"
        if (Test-Path -LiteralPath $bk) {
          $files = @(Get-ChildItem -LiteralPath $bk -Filter "*.vhdx" -File -ErrorAction SilentlyContinue)
          $status = $(if ($files.Count -gt 0) { "pass" } else { "warn" })
          $detail = "Backup 下 User.vhdx 备份 $($files.Count) 份"
          $evidence = $bk
        } else { $detail = "Backup 目录不存在（先跑 Maintenance.ps1 -Action Backup）" }
      }
      "deliver" {
        $target = $UsbDrive
        if (-not $target) { $detail = "未指定 -UsbDrive，跳过成品盘核验" }
        else {
          $need = @("Variable-OS.vhdx", "Data", "PortableVM")
          $miss = @()
          foreach ($n in $need) { if (-not (Test-Path -LiteralPath (Join-Path $target $n))) { $miss += $n } }
          $status = $(if ($miss.Count -eq 0) { "pass" } else { "fail" })
          $detail = $(if ($miss.Count -eq 0) { "$target 交付物齐全" } else { "$target 缺: $($miss -join ', ')" })
          $evidence = $target
        }
      }
    }

    $rows += [pscustomobject]@{
      id = $g.id; planRef = $g.planRef; name = $g.name
      status = $status; detail = $detail; evidence = $evidence
    }
  }
  return $rows
}

function Invoke-Report {
  $rows = Get-Verdicts
  New-Ai5Directory -Path $OutDir | Out-Null
  $pass = @($rows | Where-Object { $_.status -eq "pass" }).Count
  $warn = @($rows | Where-Object { $_.status -eq "warn" }).Count
  $fail = @($rows | Where-Object { $_.status -eq "fail" }).Count
  $todo = @($rows | Where-Object { $_.status -eq "todo" }).Count

  Write-Ai5 "验收汇总 (主计划 1.3 / 扩充28)" "Step"
  foreach ($r in $rows) {
    Write-Ai5 ("[{0}] {1} {2} — {3}" -f $r.id, (Get-Ai5VerdictIcon -Status $r.status), $r.name, $r.detail) `
      $(if ($r.status -eq "pass") { "Ok" } elseif ($r.status -eq "fail") { "Err" } else { "Warn" })
  }
  Write-Host ""
  Write-Ai5 ("通过 {0} / 有条件 {1} / 不通过 {2} / 待实测 {3}" -f $pass, $warn, $fail, $todo)

  $rp = Join-Path $OutDir "acceptance-report.md"
  $L = @()
  $L += "# 验收报告 (主计划 1.3 成功标准 / 扩充 19 交付清单 / 扩充 28 验收单)"
  $L += ""
  $L += "- 生成: $(Get-Date -Format o)  主机: $env:COMPUTERNAME  Data: $DataDrive"
  $L += "- 汇总: ✅$pass ⚠️$warn ❌$fail ⬜$todo"
  $L += ""
  $L += "| ID | 主计划 | 验收项 | 结果 | 依据 | 证据 |"
  $L += "|---|---|---|---|---|---|"
  foreach ($r in $rows) {
    $L += "| $($r.id) | $($r.planRef) | $($r.name) | $(Get-Ai5VerdictIcon -Status $r.status) $($r.status) | $($r.detail) | $($r.evidence) |"
  }
  $L += ""
  $L += "> 状态来源只有三种：自动脚本实测（compat/chaos/bench/bitlocker/align/restore/deliver）、"
  $L += "> 人工实测录入（``manual-results.json``）、以及明确标注 ``todo`` 待真机。无证据不打 ✅。"
  Save-Ai5Text -Lines $L -Path $rp | Out-Null
  Write-Ai5 "报告已写入 $rp" "Ok"

  $repo = Get-RepoRoot
  if ($repo) {
    $docsCheck = Join-Path $repo "docs\ACCEPTANCE_CHECKLIST.md"
    if (Test-Path -LiteralPath $docsCheck) {
      Write-Ai5 "仓库已有 docs\ACCEPTANCE_CHECKLIST.md，可把上面的结论同步过去（AI-5 不直接改其他 AI 的文件）"
    }
  }
  return 0
}

function Invoke-Check {
  $rows = Get-Verdicts
  $fail = @($rows | Where-Object { $_.status -eq "fail" })
  $todo = @($rows | Where-Object { $_.status -eq "todo" })
  foreach ($f in $fail) { Write-Ai5 ("FAIL [{0}] {1} — {2}" -f $f.id, $f.name, $f.detail) "Err" }
  foreach ($t in $todo) { Write-Ai5 ("TODO [{0}] {1} — {2}" -f $t.id, $t.name, $t.detail) "Warn" }
  if ($fail.Count -gt 0) { Write-Ai5 "验收门禁不通过：$($fail.Count) 项 fail" "Err"; return 1 }
  if ($Strict -and $todo.Count -gt 0) { Write-Ai5 "严格模式：$($todo.Count) 项仍待实测" "Err"; return 1 }
  Write-Ai5 "验收门禁通过（fail=0$(if ($todo.Count -gt 0) { ", 待实测 $($todo.Count) 项" })）" "Ok"
  return 0
}

$exit = 0
switch ($Action) {
  "Init"   { $exit = Invoke-Init }
  "Report" { $exit = Invoke-Report }
  "Check"  { $exit = Invoke-Check }
}
exit $exit
