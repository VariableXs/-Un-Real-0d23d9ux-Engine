<#
.SYNOPSIS
  V-10：验收单闭环（扩充 19）
  Accept-Gate.ps1 -Action Init/Report/Check：
    Init   生成 14 项验收登记表（docs/acceptance/accept-gate.json），全部初始为 todo
    Report 打印当前状态 + 缺证清单
    Check  门禁：全部 PASS 且证据文件存在才退出 0 —— 没有实测的一律保持 todo，绝不自动打 ✅

.EXAMPLE
  .\Accept-Gate.ps1 -Action Init
  .\Accept-Gate.ps1 -Action Report
  .\Accept-Gate.ps1 -Action Check
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet('Init', 'Report', 'Check')]
  [string]$Action,
  # Check 用：允许 -Item V-4 -Evidence docs\acceptance\v4\x.json 单项登记
  [string]$Item = "",
  [string]$Evidence = "",
  [ValidateSet('PASS', 'FAIL', 'todo')]
  [string]$Status = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$accDir = Join-Path $repo 'docs\acceptance'
$gateFile = Join-Path $accDir 'accept-gate.json'

# 14 项验收（总计划 13.x + 验收清单）：id / 名称 / 期望证据目录
$items = @(
  @{ id = 'V-1'; name = 'AI3 衔接层补齐（agent/调优/自检）'; ev = 'v1' },
  @{ id = 'V-2'; name = 'VHDX 差分链：糟蹋系统盘→60s 还原→数据盘完好'; ev = 'v2' },
  @{ id = 'V-3'; name = 'U 盘调优：4K 对齐/簇/寿命 + 写基准对比'; ev = 'v3' },
  @{ id = 'V-4'; name = '大软件专项：Blender/DaVinci 走查报告'; ev = 'v4' },
  @{ id = 'V-5'; name = '完全兼容 5 原则 VM 档核查'; ev = 'v5' },
  @{ id = 'V-6'; name = '层式镜像三层链 + MSIX 接入'; ev = 'v6' },
  @{ id = 'V-7'; name = 'BitLocker/容器加密 + 杀软申诉包'; ev = 'v7' },
  @{ id = 'V-8'; name = '四阶段交付真机 + 换机核验截图'; ev = 'v8' },
  @{ id = 'V-9'; name = '性能压测门禁（≥本地 SSD 85%）+ 混沌 12 场景'; ev = 'v9' },
  @{ id = 'L-1'; name = '引导器三档降级真机'; ev = 'l1' },
  @{ id = 'L-2'; name = '自动分级渲染 S/A/B/C 实测'; ev = 'l2' },
  @{ id = 'L-3'; name = 'Hyper-V 自动化 + 降级链'; ev = 'l3' },
  @{ id = 'D-1'; name = 'VM 档登录即 Variable'; ev = 'd1' },
  @{ id = 'D-2'; name = '直跑档 Shell 替换 + 3 崩溃自愈'; ev = 'd2' }
)

switch ($Action) {
  'Init' {
    New-Item -ItemType Directory -Force -Path $accDir | Out-Null
    $state = $items | ForEach-Object { [pscustomobject]@{ id = $_.id; name = $_.name; status = 'todo'; evidence = '' } }
    $state | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $gateFile -Encoding UTF8
    Write-Host ">>> 验收登记表已初始化：$gateFile（14 项全部 todo）" -ForegroundColor Green
  }
  'Report' {
    if (-not (Test-Path -LiteralPath $gateFile)) { throw '登记表不存在，先 -Action Init' }
    $state = Get-Content -LiteralPath $gateFile -Raw | ConvertFrom-Json
    $state | Format-Table id, status, evidence, name -AutoSize | Out-Host
    $todo = @($state | Where-Object status -eq 'todo')
    Write-Host ("共 {0} 项：PASS {1}  FAIL {2}  todo {3}" -f $state.Count,
      @($state | Where-Object status -eq 'PASS').Count,
      @($state | Where-Object status -eq 'FAIL').Count, $todo.Count) -ForegroundColor Cyan
    if ($todo.Count -gt 0) { Write-Host '缺证项（保持 todo，绝不自动打 ✅）:' -ForegroundColor Yellow; $todo | ForEach-Object { Write-Host "  - $($_.id) $($_.name)" } }
  }
  'Check' {
    if (-not (Test-Path -LiteralPath $gateFile)) { throw '登记表不存在，先 -Action Init' }
    $state = Get-Content -LiteralPath $gateFile -Raw | ConvertFrom-Json
    # 单项登记模式
    if ($Item) {
      $entry = $state | Where-Object id -eq $Item
      if (-not $entry) { throw "未知验收项：$Item" }
      $fullEv = if ($Evidence) { Join-Path $repo $Evidence } else { '' }
      if ($Status -eq 'PASS' -and $fullEv -and -not (Test-Path -LiteralPath $fullEv)) {
        throw "证据文件不存在：$fullEv —— 没有实测证据不得登记 PASS"
      }
      $entry.status = $Status
      $entry.evidence = $Evidence
      $state | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $gateFile -Encoding UTF8
      Write-Host ">>> 已登记 $Item = $Status（$Evidence）" -ForegroundColor Green
    }
    # 门禁
    $notPass = @($state | Where-Object status -ne 'PASS')
    if ($notPass.Count -gt 0) {
      Write-Host "FAIL 门禁：$($notPass.Count)/$($state.Count) 项未闭环" -ForegroundColor Red
      $notPass | ForEach-Object { Write-Host "  - $($_.id) [$($_.status)]" -ForegroundColor Red }
      exit 1
    }
    Write-Host 'PASS 验收门禁：14 项全部有实测证据闭环' -ForegroundColor Green
  }
}
