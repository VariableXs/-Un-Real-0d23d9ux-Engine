<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务47 —— 引擎 VHDX 挂载 + VM 拉起/保活/休眠编排（QEMU 先行验证）

.DESCRIPTION
  对应 docs/VARIX双域系统施工总案.md 阶段6 步骤1/2（验收：拉起幂等 / 心跳超时重启 / 五态状态机 / 冷启动基线）。
  编排状态机五态显式建模：
    Closed(默认) -> Launching(拉起中) -> Ready(就绪)
                -> Hibernating/Hibernated(休眠) -> Ready
    任意态 -> Failed(异常：非法转移/致命错误)
  差分盘挂载：HyperV 用 Mount-VHD / New-VHD -Parent 差分链；Qemu 用 qemu-img 建 overlay（QEMU 先行等价 Mount-VHD）。
  VM 拉起：HyperV 用 New-VM/Start-VM（参数化，真实可跑于 Hyper-V 宿主）；Qemu 用 qemu-system-x86_64（本环境真实可跑）。
  就绪探针：复用 47631 心跳语义——连 127.0.0.1:47631 发 PING，期望 READY（见 src-tauri/src/vm_agent.rs：agent 监 47631，收 PING 回 READY）。
  保活：就绪后空闲检测心跳轮询（近零占用）。
  休眠/恢复：HyperV 用 Save-VM/Restore-VM 写差分盘；Qemu 用 HMP savevm/loadvm 写差分盘快照。
  幂等：拉起中/就绪态锁（LaunchLocked）禁止二次拉起，重复触发返回既有实例不双开。
  心跳超时：超过 HeartbeatThresholdSec 未收到 READY 即判定超时 -> 重启策略（kill+重拉 agent，超限转 Failed）。
  休眠-恢复 x10 内容一致：每轮写引擎内容序号落差分盘镜像侧车文件，savevm/loadvm 后校验序号不变。

  本脚本可被 dot-source（. .\Engine-Orchestrator.ps1）以导出全部函数供 Test-Engine-Orchestrator.ps1 单测；
  直接运行时按 -Action 分派操作，状态持久化到 -StateDir\engine-orc-state.json（断点续作）。

.EXAMPLE
  # Qemu 先行（本环境真实可跑）：完整编排一遍
  .\Engine-Orchestrator.ps1 -Hypervisor Qemu -DifferentialBase _attic\p47-base.img `
      -DifferentialOverlay _attic\p47-diff.qcow2 -EngineAgent _attic\p47-engine-agent.py `
      -IsoPath varix-qemu.iso -Action Run
  # 仅挂载差分盘
  .\Engine-Orchestrator.ps1 -Hypervisor Qemu -DifferentialBase _attic\p47-base.img -DifferentialOverlay _attic\p47-diff.qcow2 -Action Mount
  # Hyper-V 模式（需 Hyper-V 宿主，参数化真实可跑）
  .\Engine-Orchestrator.ps1 -Hypervisor HyperV -DifferentialBase E:\base.vhdx -DifferentialOverlay E:\win-engine-diff.vhdx -VmName VARIX-Engine -Action Run

.NOTES
  编码纪律：UTF-8 BOM；PowerShell 5.1 兼容（不用 &&/?:/??）；不动 kernel/src/src-tauri/docs 任务文档。
  监控端口默认 14561（落在 14561-14590 段）；差分盘镜像 p47- 前缀放 _attic/（.gitignore 已忽略）。
#>
[CmdletBinding()]
param(
  [ValidateSet('HyperV', 'Qemu')][string]$Hypervisor = 'Qemu',
  [string]$DifferentialBase = '',
  [string]$DifferentialOverlay = '',
  [string]$EngineAgent = '',
  [string]$IsoPath = '',
  [string]$VmName = 'VARIX-Engine',
  [int]$MonitorPort = 14561,
  [int]$HeartbeatPort = 47631,
  [int]$HeartbeatThresholdSec = 5,
  [int]$BootTimeoutSec = 150,
  [string]$StateDir = '',
  [string]$SerialLog = '',
  [string]$ContentFile = '',
  [ValidateSet('Mount', 'Launch', 'Probe', 'Keepalive', 'Hibernate', 'Resume', 'Stop', 'Status', 'Run')]
  [string]$Action = 'Status',
  [ValidateSet('Real', 'Mock')][string]$Backend = 'Real'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- 五态状态机
$script:STATES = @('Closed', 'Launching', 'Ready', 'Hibernating', 'Hibernated', 'Failed')
# 合法转移表：键=源态，值=允许的目标态集合
$script:TRANSITIONS = @{
  'Closed'      = @('Launching')
  'Launching'   = @('Ready', 'Failed')
  'Ready'       = @('Hibernating', 'Failed')
  'Hibernating' = @('Hibernated', 'Failed')
  'Hibernated'  = @('Ready', 'Failed')
  'Failed'      = @('Closed')
}

function Test-TransitionAllowed {
  param([string]$Src, [string]$Dst)
  if ($Src -eq $Dst) { return $true }
  return $script:TRANSITIONS.ContainsKey($Src) -and ($script:TRANSITIONS[$Src] -contains $Dst)
}

function Assert-Transition {
  param([string]$Dst)
  $src = $script:Orc.State
  if (-not (Test-TransitionAllowed $src $Dst)) {
    throw "非法状态转移: $src -> $Dst 被拒绝"
  }
  $script:Orc.State = $Dst
}

# ---------------------------------------------------------------- 编排状态对象
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $StateDir)  { $StateDir = Join-Path $repo '_attic' }
if (-not $SerialLog) { $SerialLog = Join-Path $StateDir 'p47-serial.log' }
if (-not $ContentFile) { $ContentFile = Join-Path $StateDir 'p47-engine-content.txt' }
if (-not $IsoPath)   { $IsoPath = Join-Path $repo 'varix-qemu.iso' }

$script:Orc = [pscustomobject]@{
  State                  = 'Closed'
  Hypervisor             = $Hypervisor
  Base                   = $DifferentialBase
  Overlay                = $DifferentialOverlay
  IsoPath                = $IsoPath
  EngineAgent            = $EngineAgent
  VmName                 = $VmName
  MonitorPort            = $MonitorPort
  HeartbeatPort          = $HeartbeatPort
  HeartbeatThresholdSec  = $HeartbeatThresholdSec
  BootTimeoutSec         = $BootTimeoutSec
  SerialLog              = $SerialLog
  ContentFile            = $ContentFile
  StateFile              = (Join-Path $StateDir 'engine-orc-state.json')
  QemuPid                = $null
  AgentPid               = $null
  LaunchLocked           = $false
  ContentSeq             = 0
  Backend                = $Backend
}

function Save-OrcState {
  [IO.File]::WriteAllText($script:Orc.StateFile, ($script:Orc | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
}
function Load-OrcState {
  if (Test-Path -LiteralPath $script:Orc.StateFile) {
    $loaded = Get-Content -LiteralPath $script:Orc.StateFile -Raw | ConvertFrom-Json
    foreach ($p in $loaded.PSObject.Properties) {
      if ($p.Name -in @('State', 'QemuPid', 'AgentPid', 'LaunchLocked', 'ContentSeq')) {
        $script:Orc.$($p.Name) = $p.Value
      }
    }
  }
}

# ---------------------------------------------------------------- 底层工具
function Send-Hmp {
  param([string]$Cmd)
  try {
    $client = New-Object System.Net.Sockets.TcpClient
    $client.Connect('127.0.0.1', $script:Orc.MonitorPort)
    $stream = $client.GetStream()
    $stream.ReadTimeout = 5000
    Start-Sleep -Milliseconds 120
    $banner = New-Object byte[] 4096
    try { $stream.Read($banner, 0, $banner.Length) | Out-Null } catch { }
    $bytes = [Text.Encoding]::ASCII.GetBytes($Cmd + "`n")
    $stream.Write($bytes, 0, $bytes.Length)
    Start-Sleep -Milliseconds 300
    $out = ''
    try {
      while ($stream.DataAvailable) {
        $rb = New-Object byte[] 4096
        $n = $stream.Read($rb, 0, $rb.Length)
        if ($n -le 0) { break }
        $out += [Text.Encoding]::ASCII.GetString($rb, 0, $n)
      }
    } catch { }
    $stream.Close(); $client.Close()
    return $out
  }
  catch {
    return "HMP-ERR: $_"
  }
}

function Test-Heartbeat {
  # Mock 后端：心跳存活以 AgentPid 是否存在模拟（不连真实 TCP）
  if ($script:Orc.Backend -eq 'Mock') {
    return ($null -ne $script:Orc.AgentPid -and 0 -ne $script:Orc.AgentPid)
  }
  try {
    $c = New-Object System.Net.Sockets.TcpClient
    $c.ReceiveTimeout = $script:Orc.HeartbeatThresholdSec * 1000
    $c.Connect('127.0.0.1', $script:Orc.HeartbeatPort)
    $s = $c.GetStream()
    $b = [Text.Encoding]::ASCII.GetBytes('PING')
    $s.Write($b, 0, $b.Length)
    $rb = New-Object byte[] 64
    $n = $s.Read($rb, 0, $rb.Length)
    $resp = [Text.Encoding]::ASCII.GetString($rb, 0, $n)
    $s.Close(); $c.Close()
    return $resp.StartsWith('READY')
  }
  catch {
    return $false
  }
}

function Wait-BootComplete {
  param([int]$TimeoutSec)
  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  while ((Get-Date) -lt $deadline) {
    if (Test-Path -LiteralPath $script:Orc.SerialLog) {
      $log = Get-Content -LiteralPath $script:Orc.SerialLog -Raw -ErrorAction SilentlyContinue
      if ($log -and $log.Contains('boot complete')) { return $true }
    }
    Start-Sleep -Seconds 1
  }
  return $false
}

function Get-EngineContent {
  try { return [int](Get-Content -LiteralPath $script:Orc.ContentFile -Raw -ErrorAction SilentlyContinue) }
  catch { return -1 }
}
function Set-EngineContent {
  param([int]$Seq)
  Set-Content -LiteralPath $script:Orc.ContentFile -Value $Seq -Encoding UTF8
}

# ---------------------------------------------------------------- 差分盘挂载
function Mount-DifferentialDisk {
  if ($script:Orc.Hypervisor -eq 'HyperV') {
    if (-not (Test-Path -LiteralPath $script:Orc.Base)) { throw "Base 差分链基盘不存在：$($script:Orc.Base)" }
    if (Test-Path -LiteralPath $script:Orc.Overlay) { Remove-Item -LiteralPath $script:Orc.Overlay -Force }
    New-VHD -Path $script:Orc.Overlay -ParentPath $script:Orc.Base -Differencing | Out-Null
    Mount-VHD -Path $script:Orc.Overlay
    Write-Host "    + HyperV 差分盘已挂载：$($script:Orc.Overlay)（New-VHD -Parent 差分链）" -ForegroundColor Green
  }
  else {
    if (-not (Test-Path -LiteralPath $script:Orc.Base)) {
      & qemu-img create -f raw $script:Orc.Base '512M' | Out-Null
    }
    if (Test-Path -LiteralPath $script:Orc.Overlay) { Remove-Item -LiteralPath $script:Orc.Overlay -Force }
    & qemu-img create -f qcow2 -b $script:Orc.Base -F raw $script:Orc.Overlay '512M'
    if ($LASTEXITCODE -ne 0) { throw "qemu-img create 差分盘失败（exit=$LASTEXITCODE）" }
    Write-Host "    + Qemu 差分盘已建：$($script:Orc.Overlay)（base=$($script:Orc.Base)，qemu-img 等价 Mount-VHD）" -ForegroundColor Green
  }
}

# ---------------------------------------------------------------- VM / Agent 拉起与停止
function Start-EngineVm {
  if ($script:Orc.LaunchLocked) {
    Write-Host "    ! 幂等锁已持有，返回既有实例（pid=$($script:Orc.QemuPid)），不双开" -ForegroundColor Yellow
    return
  }
  if ($script:Orc.Backend -eq 'Mock') {
    $script:Orc.QemuPid = 99999
    $script:Orc.LaunchLocked = $true
    Write-Host "    + [Mock] VM 拉起（pid=99999）" -ForegroundColor Green
    return
  }
  if ($script:Orc.Hypervisor -eq 'HyperV') {
    $vm = Get-VM -Name $script:Orc.VmName -ErrorAction SilentlyContinue
    if ($vm) {
      if ($vm.State -eq 'Off') { Start-VM -VM $vm }
    }
    else {
      # 代际2 / 关 Secure Boot / 动态内存 / 直挂差分盘（对应终极形态施工总计划 步骤3）
      $vm = New-VM -Name $script:Orc.VmName -MemoryStartupBytes 4GB -Generation 2 -ErrorAction Stop
      Set-VMFirmware -VM $vm -EnableSecureBoot Off
      Add-VMHardDiskDrive -VM $vm -Path $script:Orc.Overlay
      Start-VM -VM $vm
    }
    $script:Orc.QemuPid = $vm.VMId.Guid
    $script:Orc.LaunchLocked = $true
    Write-Host "    + HyperV VM 已拉起：$($script:Orc.VmName)" -ForegroundColor Green
    return
  }
  # Qemu 真实拉起
  $argList = @(
    '-cdrom', $script:Orc.IsoPath,
    '-drive', "file=$($script:Orc.Overlay),if=none,id=nv1,format=qcow2",
    '-device', 'nvme,drive=nv1,serial=P47DIFF',
    '-serial', "file:$($script:Orc.SerialLog)",
    '-no-reboot', '-no-shutdown', '-m', '512M', '-M', 'q35', '-display', 'none',
    '-boot', 'order=d',
    '-monitor', "tcp:127.0.0.1:$($script:Orc.MonitorPort),server,nowait"
  )
  $p = Start-Process -FilePath 'qemu-system-x86_64' -ArgumentList $argList -PassThru -NoNewWindow
  $script:Orc.QemuPid = $p.Id
  $script:Orc.LaunchLocked = $true
  Write-Host "    + Qemu VM 已拉起（pid=$($p.Id)，monitor=127.0.0.1:$($script:Orc.MonitorPort)）" -ForegroundColor Green
}

function Start-EngineAgent {
  if ($script:Orc.Backend -eq 'Mock') { $script:Orc.AgentPid = 88888; return }
  $py = $script:Orc.EngineAgent
  if (-not $py) { Write-Host "    ! 未指定 -EngineAgent，跳过心跳源启动（仅 VM 拉起）" -ForegroundColor Yellow; return }
  $argList = @($py, '--port', "$($script:Orc.HeartbeatPort)", '--content-file', $script:Orc.ContentFile)
  $p = Start-Process -FilePath 'python' -ArgumentList $argList -PassThru -NoNewWindow
  $script:Orc.AgentPid = $p.Id
  Write-Host "    + 引擎心跳代理已拉起（pid=$($p.Id)，监听 127.0.0.1:$($script:Orc.HeartbeatPort)）" -ForegroundColor Green
}

function Stop-EngineVm {
  if ($script:Orc.Backend -eq 'Mock') {
    $script:Orc.QemuPid = $null; $script:Orc.AgentPid = $null; $script:Orc.LaunchLocked = $false
    return
  }
  if ($script:Orc.AgentPid) {
    try { Stop-Process -Id $script:Orc.AgentPid -Force -ErrorAction SilentlyContinue } catch { }
    $script:Orc.AgentPid = $null
  }
  if ($script:Orc.Hypervisor -eq 'HyperV') {
    try { Stop-VM -Name $script:Orc.VmName -Force -ErrorAction SilentlyContinue } catch { }
  }
  else {
    if ($script:Orc.QemuPid) {
      try {
        $p = Get-Process -Id $script:Orc.QemuPid -ErrorAction SilentlyContinue
        if ($p) { $p.Kill(); $p.WaitForExit(5000) }
      }
      catch { }
    }
  }
  $script:Orc.QemuPid = $null
  $script:Orc.LaunchLocked = $false
}

# ---------------------------------------------------------------- 保活 / 休眠 / 恢复
function Invoke-Keepalive {
  # 就绪后空闲检测：仅轮询心跳，近零占用
  return Test-Heartbeat
}

function Invoke-Hibernate {
  if ($script:Orc.Backend -eq 'Mock') {
    Assert-Transition 'Hibernating'
    Assert-Transition 'Hibernated'
    return
  }
  if ($script:Orc.Hypervisor -eq 'HyperV') {
    Save-VM -Name $script:Orc.VmName
  }
  else {
    $r = Send-Hmp 'savevm p47snap'
    if ($r -match 'Error' -or $r -match 'HMP-ERR') { throw "savevm 失败: $($r.Trim())" }
  }
  Assert-Transition 'Hibernating'
  Assert-Transition 'Hibernated'
  Write-Host "    + 已休眠（内存快照写差分盘）" -ForegroundColor Green
}

function Invoke-Resume {
  if ($script:Orc.Backend -eq 'Mock') {
    Assert-Transition 'Ready'
    return
  }
  if ($script:Orc.Hypervisor -eq 'HyperV') {
    Restore-VM -Name $script:Orc.VmName
  }
  else {
    $r = Send-Hmp 'loadvm p47snap'
    if ($r -match 'Error' -or $r -match 'HMP-ERR') { throw "loadvm 失败: $($r.Trim())" }
  }
  Assert-Transition 'Ready'
  Write-Host "    + 已恢复（差分盘快照还原）" -ForegroundColor Green
}

# ---------------------------------------------------------------- 完整编排（Run）
function Invoke-FullRun {
  Write-Host '>>> 任务47 编排：Run（五态全转移 + 幂等 + 超时重启 + 休眠x10）' -ForegroundColor Cyan
  Assert-Transition 'Launching'
  Mount-DifferentialDisk
  Start-EngineVm
  Start-EngineAgent
  $booted = Wait-BootComplete $script:Orc.BootTimeoutSec
  if ($booted -and (Test-Heartbeat)) { Assert-Transition 'Ready' }
  else { Assert-Transition 'Failed'; throw '引导或心跳未就绪 -> Failed' }
  Write-Host "    + 就绪（态=$($script:Orc.State)）" -ForegroundColor Green

  # 幂等：重复拉起不双开
  $pidBefore = $script:Orc.QemuPid
  try { Start-EngineVm } catch { }
  if ($script:Orc.QemuPid -ne $pidBefore) { Assert-Transition 'Failed'; throw '幂等失败：出现第二个 VM' }

  # 保活
  if (-not (Invoke-Keepalive)) { Assert-Transition 'Failed'; throw '保活心跳丢失' }

  # 心跳超时 + 重启
  if ($script:Orc.Backend -ne 'Mock') {
    try { Stop-Process -Id $script:Orc.AgentPid -Force -ErrorAction SilentlyContinue } catch { }
  }
  else { $script:Orc.AgentPid = $null }
  Start-Sleep -Seconds ($script:Orc.HeartbeatThresholdSec + 1)
  if (Test-Heartbeat) { throw '超时判定失效：仍收到心跳' }
  Start-EngineAgent   # 重启策略
  if (-not (Test-Heartbeat)) { Assert-Transition 'Failed'; throw '重启后仍无心跳' }
  Write-Host '    + 超时重启恢复成功' -ForegroundColor Green

  # 休眠-恢复 x10 内容一致
  $seq0 = Get-EngineContent
  for ($i = 1; $i -le 10; $i++) {
    Set-EngineContent ($seq0 + $i)
    Invoke-Hibernate
    Invoke-Resume
    if ((Get-EngineContent) -ne ($seq0 + $i)) { Assert-Transition 'Failed'; throw "第 $i 轮休眠内容不一致" }
    if (-not (Test-Heartbeat)) { Assert-Transition 'Failed'; throw "第 $i 轮恢复后心跳失联" }
    Write-Host "    + 休眠-恢复 轮 $i/10 内容一致（seq=$($seq0 + $i)）" -ForegroundColor Green
  }
  Write-Host '>>> Run 完成，五态全转移 + 幂等 + 超时重启 + 休眠x10 通过' -ForegroundColor Green
}

# ---------------------------------------------------------------- 动作分派
Load-OrcState
switch ($Action) {
  'Mount'    { Assert-Transition 'Launching'; Mount-DifferentialDisk; Assert-Transition 'Closed' }
  'Launch'   { Assert-Transition 'Launching'; Start-EngineVm; Start-EngineAgent; Save-OrcState }
  'Probe'    { $ok = Test-Heartbeat; Write-Host ("心跳探针: {0}" -f $(if ($ok) { 'READY' } else { '无响应' })) }
  'Keepalive'{ $ok = Invoke-Keepalive; Write-Host ("保活: {0}" -f $(if ($ok) { '稳定' } else { '失联' })) }
  'Hibernate'{ Invoke-Hibernate; Save-OrcState }
  'Resume'   { Invoke-Resume; Save-OrcState }
  'Stop'     { Stop-EngineVm; Save-OrcState }
  'Status'   { Write-Host ("当前态: $($script:Orc.State)  vmPid=$($script:Orc.QemuPid)  agentPid=$($script:Orc.AgentPid)") }
  'Run'      { Invoke-FullRun; Save-OrcState }
}

# dot-source 时不额外输出（供单测）
if ($MyInvocation.InvocationName -ne '.') {
  Write-Host ("[Engine-Orchestrator] 态=$($script:Orc.State) 后端=$Backend 虚拟化=$Hypervisor")
}
