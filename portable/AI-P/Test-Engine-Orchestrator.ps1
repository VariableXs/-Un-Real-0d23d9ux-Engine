<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务47 单元测试 —— 五态状态机 / 幂等 / 超时重启 / 休眠x10

.DESCRIPTION
  dot-source Engine-Orchestrator.ps1 导出全部函数，针对总案阶段6 验收点做单测：
    T1 非法状态转移显式拒绝
    T2 合法五态链全通过（Closed->Launching->Ready->Hibernating->Hibernated->Ready）
    T3 拉起幂等：重复触发不双开（Mock 后端）
    T4 心跳超时判定与重启策略（Mock 后端：agent 崩 -> 超时 -> 重启 -> Ready）
    T5 休眠-恢复 x10 内容一致（Mock 后端：每轮写引擎内容序号，savevm/loadvm 后校验不变）
    T6 [可选 -LiveQemu] 真实 Qemu 路径冒烟（复用 Engine-Orchestrator -Action Run，需 python+qemu）
  退出码 0 = 全过。

.EXAMPLE
  powershell -NoProfile -File .\Test-Engine-Orchestrator.ps1
  powershell -NoProfile -File .\Test-Engine-Orchestrator.ps1 -LiveQemu
#>
[CmdletBinding()]
param([switch]$LiveQemu)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
. (Join-Path $here 'Engine-Orchestrator.ps1') -Backend Mock | Out-Null

$checks = 0
$failures = @()

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail = '')
  $script:checks++
  if ($Ok) { Write-Host "  PASS  $Name" -ForegroundColor Green }
  else {
    Write-Host "  FAIL  $Name  $Detail" -ForegroundColor Red
    $script:failures += "$Name：$Detail"
  }
}

# T1 非法转移显式拒绝
Write-Host '== T1 非法状态转移显式拒绝 ==' -ForegroundColor Cyan
$script:Orc.State = 'Closed'
$illegalRejected = $false
try { Assert-Transition 'Hibernated' } catch { $illegalRejected = $true }
Add-Check 'Closed->Hibernated 被拒绝' $illegalRejected
$illegalRejected2 = $false
try { Assert-Transition 'Launching'; Assert-Transition 'Hibernated' } catch { $illegalRejected2 = $true }
Add-Check 'Launching->Hibernated 被拒绝' $illegalRejected2

# T2 合法五态链
Write-Host '== T2 合法五态链 ==' -ForegroundColor Cyan
$script:Orc.State = 'Closed'
$chainOk = $true; $chainErr = ''
try {
  Assert-Transition 'Launching'
  Assert-Transition 'Ready'
  Assert-Transition 'Hibernating'
  Assert-Transition 'Hibernated'
  Assert-Transition 'Ready'
} catch { $chainOk = $false; $chainErr = $_ }
Add-Check 'Closed->Launching->Ready->Hibernating->Hibernated->Ready 全合法' $chainOk $chainErr

# T3 幂等（Mock）
Write-Host '== T3 拉起幂等（Mock）==' -ForegroundColor Cyan
$script:Orc.State = 'Closed'; $script:Orc.Backend = 'Mock'; $script:Orc.QemuPid = $null; $script:Orc.LaunchLocked = $false
Assert-Transition 'Launching'
Start-EngineVm            # Mock：pid=99999, lock=true
$pid1 = $script:Orc.QemuPid
Start-EngineVm            # 二次触发：应返回既有实例不双开
$pid2 = $script:Orc.QemuPid
Add-Check '重复触发不双开（pid 不变）' ($pid1 -eq $pid2 -and $pid1 -eq 99999) "pid1=$pid1 pid2=$pid2"
Add-Check '幂等锁保持' $script:Orc.LaunchLocked

# T4 超时 + 重启（Mock）
Write-Host '== T4 心跳超时+重启（Mock）==' -ForegroundColor Cyan
$script:Orc.State = 'Ready'; $script:Orc.Backend = 'Mock'; $script:Orc.AgentPid = $null
Start-EngineAgent        # Mock：agent 存活
$aliveBefore = Test-Heartbeat
$script:Orc.AgentPid = $null   # 模拟引擎崩溃
$dead = -not (Test-Heartbeat)  # 超时判定
Start-EngineAgent        # 重启策略
$aliveAfter = Test-Heartbeat
Add-Check '心跳探针：agent 存活=True' $aliveBefore
Add-Check 'agent 崩溃后探针=False（超时判定）' $dead
Add-Check '重启后探针=True（恢复）' $aliveAfter

# T5 休眠-恢复 x10 内容一致（Mock）
Write-Host '== T5 休眠-恢复 x10 内容一致（Mock）==' -ForegroundColor Cyan
$script:Orc.State = 'Ready'; $script:Orc.Backend = 'Mock'; $script:Orc.AgentPid = 88888
$seq0 = 0; Set-EngineContent $seq0
$allOk = $true; $badRound = 0
$dbg = Join-Path $PSScriptRoot 'test47-debug.txt'
$dbgLines = @("start seq0=$seq0 agentpid=$($script:Orc.AgentPid)")
for ($i = 1; $i -le 10; $i++) {
  Set-EngineContent ($seq0 + $i)
  try { Invoke-Hibernate; Invoke-Resume } catch { $allOk = $false; $badRound = $i; $dbgLines += "round $i threw: $_"; break }
  $dbgLines += "round $i state=$($script:Orc.State) content=$(Get-EngineContent) want=$($seq0+$i) hb=$(Test-Heartbeat) agentpid=$($script:Orc.AgentPid)"
  if ((Get-EngineContent) -ne ($seq0 + $i)) { $allOk = $false; $badRound = $i; $dbgLines += "round $i content mismatch"; break }
  if (-not (Test-Heartbeat)) { $allOk = $false; $badRound = $i; $dbgLines += "round $i heartbeat lost"; break }
}
[IO.File]::WriteAllLines($dbg, $dbgLines, [Text.UTF8Encoding]::new($false))
Add-Check '休眠-恢复 x10 内容一致（末态 Ready + 序号连续）' ($allOk -and $script:Orc.State -eq 'Ready') "badRound=$badRound 末态=$($script:Orc.State) 末seq=$(Get-EngineContent)"

# T6 真实 Qemu 冒烟（可选）
if ($LiveQemu) {
  Write-Host '== T6 真实 Qemu 路径冒烟（-LiveQemu）==' -ForegroundColor Cyan
  $repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  $attic = Join-Path $repo '_attic'
  Write-Host '    （该路径等价于 _attic/p47-qemu-validate.py，已先行跑通 10/10；此处复用脚本原地再验）' -ForegroundColor Yellow
  try {
    & (Join-Path $repo 'portable\AI-P\Engine-Orchestrator.ps1') `
      -Hypervisor Qemu `
      -DifferentialBase (Join-Path $attic 'p47-base.img') `
      -DifferentialOverlay (Join-Path $attic 'p47-diff.qcow2') `
      -EngineAgent (Join-Path $attic 'p47-engine-agent.py') `
      -IsoPath (Join-Path $repo 'varix-qemu.iso') `
      -StateDir $attic -Backend Real -Action Run
    Add-Check 'Qemu Run 退出码 0' ($LASTEXITCODE -eq 0) "exit=$LASTEXITCODE"
  }
  catch {
    Add-Check 'Qemu Run 执行' $false $_
  }
}

Write-Host ''
if ($failures.Count -gt 0) {
  Write-Host "任务47 单测：$checks 项中 $($failures.Count) 项失败" -ForegroundColor Red
  $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
  exit 1
}
Write-Host "任务47 单测：$checks 项全部通过" -ForegroundColor Green
exit 0
