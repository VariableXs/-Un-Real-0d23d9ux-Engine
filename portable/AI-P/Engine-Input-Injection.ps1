#!/usr/bin/env pwsh
# =====================================================================
# VARIX 双域系统 · AI-P 任务 49 —— 输入注入通道（Variable → 引擎反向注入）
# =====================================================================
# 总案步骤 5：键鼠事件 Variable → 引擎反向注入；IME（中文输入法）路径专项。
# 〔验收：逻辑=快速打字 30s 无丢键（字符计数比对）；IME 组合键全流程；
#   代码=输入事件与阶段 3 输入总线同源；完善性=注入延迟实测入基线〕
#
# 同源约束：注入帧与内核 shim://input 16B 定长布局逐字段同源
# （kernel/varix/src/inputsvc.rs 任务19/26 契约）：
#   [0..8)   u64 seq      事件序号（自 1 单调，LE）
#   [8]      u8  kind     0=Key 1=Mouse
#   [9]      u8  key      kind=Key：0=Up 1=Down 2=Enter（归一化键表）
#   [10..12) i16 dx       kind=Mouse：X 位移（LE）
#   [12..14) i16 dy       kind=Mouse：Y 位移（LE）
#   [14]     u8  buttons  bit0 左 bit1 右 bit2 中
#   [15]     u8  pad      恒 0
#
# 传输：与任务 47 心跳同通道语义（127.0.0.1:47631）——连接后发送
#   "INPUT <hex16>" 行，引擎 agent 逐帧回 ACK <seq>；无 ACK 的帧计入
#   未达账本（诚实计数，绝不静默）。
#
# 用法：
#   .\Engine-Input-Injection.ps1                          # 自测（内置 mock 引擎）
#   .\Engine-Input-Injection.ps1 -EnginePort 47631        # 对真机 agent 注入
#   .\Engine-Input-Injection.ps1 -SelfTest -TypingSeconds 30
#
# 编码纪律：UTF-8 BOM；ASCII only（.ps1 戒律）。
# =====================================================================
[CmdletBinding()]
param(
  # 引擎 agent 端口（任务 47 心跳语义同端口；0 = 仅自测模式）
  [int]$EnginePort = 0,
  # 自测开关：内置 mock 引擎 + 打字速率演练 + IME 组合键序列
  [switch]$SelfTest,
  # 打字演练时长（秒；总案 30s，自测默认压缩 3s，实机验收传 30）
  [int]$TypingSeconds = 3,
  # 单帧注入超时（ms）
  [int]$AckTimeoutMs = 2000
)

$ErrorActionPreference = 'Stop'
$script:Seq = [UInt64]0
$script:Sent = [UInt64]0
$script:Acked = [UInt64]0
$script:Unacked = [UInt64]0
$script:Latencies = New-Object System.Collections.Generic.List[double]

# ---- 16B 帧编码（同源 shim://input）--------------------------------
function New-InputFrame {
  param([byte]$Kind, [byte]$Key, [int16]$Dx, [int16]$Dy, [byte]$Buttons)
  $script:Seq++
  $b = New-Object byte[] 16
  [Array]::Copy([BitConverter]::GetBytes([UInt64]$script:Seq), 0, $b, 0, 8)
  $b[8] = $Kind
  $b[9] = $Key
  if ($Kind -eq 1) {
    [Array]::Copy([BitConverter]::GetBytes($Dx), 0, $b, 10, 2)
    [Array]::Copy([BitConverter]::GetBytes($Dy), 0, $b, 12, 2)
    $b[14] = $Buttons
  }
  return ,$b
}

function ConvertTo-Hex16 { param([byte[]]$Frame)
  return (($Frame | ForEach-Object { $_.ToString('x2') }) -join '')
}

# ---- 注入通道 ------------------------------------------------------
function Send-InputFrame {
  param([System.IO.StreamWriter]$Writer, [System.IO.StreamReader]$Reader, [byte[]]$Frame)
  $hex = ConvertTo-Hex16 $Frame
  $t0 = [DateTime]::UtcNow
  $Writer.WriteLine("INPUT $hex")
  $Writer.Flush()
  $script:Sent++
  $line = $null
  # ACK 读取（带超时退化：异步读 + 轮询）
  $task = $Reader.ReadLineAsync()
  if ($task.Wait($AckTimeoutMs)) { $line = $task.Result }
  if ($line -match "^ACK (\d+)$") {
    $script:Acked++
    $script:Latencies.Add(([DateTime]::UtcNow - $t0).TotalMilliseconds)
  } else {
    $script:Unacked++
    Write-Warning ("frame seq={0} unacked (line={1})" -f $script:Seq, $line)
  }
}

function Send-Key { param($Writer, $Reader, [byte]$Key)
  Send-InputFrame $Writer $Reader (New-InputFrame 0 $Key 0 0 0)
}
function Send-Mouse { param($Writer, $Reader, [int16]$Dx, [int16]$Dy, [byte]$Buttons)
  Send-InputFrame $Writer $Reader (New-InputFrame 1 0 $Dx $Dy $Buttons)
}

# ---- 快速打字演练（字符计数比对）------------------------------------
function Invoke-TypingDrill {
  param($Writer, $Reader, [int]$Seconds)
  Write-Host "== typing drill: ${Seconds}s fast typing (char-count comparison) =="
  $script:Sent = 0; $script:Acked = 0; $script:Unacked = 0; $script:Seq = 0
  $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
  $keys = @(0, 1, 2)   # 归一化键表全词汇（Up/Down/Enter）
  $i = 0
  while ([DateTime]::UtcNow -lt $deadline) {
    Send-Key $Writer $Reader $keys[$i % 3]
    $i++
  }
  Write-Host ("sent={0} acked={1} unacked={2}" -f $script:Sent, $script:Acked, $script:Unacked)
  if ($script:Sent -ne $script:Acked) { throw ("LOSS DETECTED: sent {0} != acked {1}" -f $script:Sent, $script:Acked) }
  Write-Host "char-count comparison: PASS (zero loss)"
}

# ---- IME 组合键全流程（序列原样透传：组合/候选/上屏由引擎侧 IME 完成）----
function Invoke-ImeDrill {
  param($Writer, $Reader)
  Write-Host "== IME drill: composition sequences (up/down/enter vocabulary) =="
  # ① 触发（Enter 序列）② 候选翻页（Down×3）③ 上屏（Enter）
  foreach ($k in @(2)) { Send-Key $Writer $Reader $k }
  foreach ($k in @(1, 1, 1)) { Send-Key $Writer $Reader $k }
  foreach ($k in @(2)) { Send-Key $Writer $Reader $k }
  # 鼠标候选选择：移动 + 左键按下/抬起
  Send-Mouse $Writer $Reader 12 -8 0
  Send-Mouse $Writer $Reader 0 0 1
  Send-Mouse $Writer $Reader 0 0 0
  if ($script:Unacked -gt 0) { throw ("IME drill lost frames: unacked={0}" -f $script:Unacked) }
  Write-Host "IME composition sequence: PASS (all frames acked)"
}

# ---- Mock 引擎（自测模式：runspace accept 循环，逐帧 ACK）-----------
# 协议与真机 agent 一致：PING->READY；INPUT <hex16> -> ACK <seq>。
$script:MockScript = {
  param([int]$Port, [hashtable]$Sync)
  try {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    $listener.Start()
    $Sync.Ready = $true
    $client = $listener.AcceptTcpClient()
    $Sync.Stage = 'accepted'
    $stream = $client.GetStream()
    $writer = [System.IO.StreamWriter]::new($stream)
    $writer.NewLine = "`r`n"; $writer.AutoFlush = $true
    $reader = [System.IO.StreamReader]::new($stream)
    while ($true) {
      $line = $reader.ReadLine()
      $Sync.Stage = "read:[$line]"
      if ($null -eq $line) { break }
      if ($line -eq 'PING') { $writer.WriteLine('READY'); $Sync.Stage = 'replied-READY' }
      elseif ($line -match '^INPUT ([0-9a-f]{32})$') {
        $seqHex = $Matches[1].Substring(0, 16)
        $writer.WriteLine("ACK $([Convert]::ToUInt64($seqHex, 16))")
        $Sync.Frames++
      }
    }
  } catch { $Sync.Error = "$_" }
}
function Start-MockEngine {
  param([int]$Port)
  $sync = [hashtable]::Synchronized(@{ Ready = $false; Frames = 0; Error = $null })
  $rs = [runspacefactory]::CreateRunspace()
  $rs.Open()
  $ps = [powershell]::Create().AddScript($script:MockScript).AddArgument($Port).AddArgument($sync)
  $null = $ps.BeginInvoke()
  for ($i = 0; $i -lt 30 -and -not $sync.Ready; $i++) { Start-Sleep -Milliseconds 100 }
  if (-not $sync.Ready) { throw 'mock engine failed to start' }
  return @{ Runspace = $rs; PowerShell = $ps; Sync = $sync; Port = $Port }
}
function Stop-MockEngine { param($Mock)
  if ($Mock) {
    $Mock.PowerShell.Stop()
    $Mock.Runspace.Close()
    Write-Host ("mock engine served {0} frames" -f $Mock.Sync.Frames)
  }
}

# ---- 主流程 ----------------------------------------------------------
$mock = $null
$client = $null
try {
  $port = if ($SelfTest) { 14777 } elseif ($EnginePort -gt 0) { $EnginePort } else { 47631 }
  if ($SelfTest) {
    $mock = Start-MockEngine $port
    Write-Host "mock engine listening on 127.0.0.1:$port"
  }
  $client = [System.Net.Sockets.TcpClient]::new()
  $client.Connect('127.0.0.1', $port)
  $stream = $client.GetStream()
  $stream.ReadTimeout = 5000
  $writer = [System.IO.StreamWriter]::new($stream)
  $writer.NewLine = "`r`n"
  $writer.AutoFlush = $true
  $reader = [System.IO.StreamReader]::new($stream)

  # 心跳就绪握手（任务 47 同语义）
  $writer.WriteLine('PING')
  $hello = $reader.ReadLine()
  if ($hello -ne 'READY') { throw ("handshake failed: expected READY, got '{0}'" -f $hello) }
  Write-Host "handshake READY on port $port"

  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  Invoke-TypingDrill $writer $reader $TypingSeconds
  Invoke-ImeDrill $writer $reader
  $sw.Stop()

  $lat = if ($script:Latencies.Count -gt 0) { [math]::Round(($script:Latencies | Measure-Object -Average).Average, 3) } else { 0 }
  Write-Host ("injection latency avg = {0} ms (baseline entry)" -f $lat)
  Write-Host ("TOTAL: sent={0} acked={1} unacked={2} elapsed={3}ms" -f $script:Sent, $script:Acked, $script:Unacked, $sw.ElapsedMilliseconds)
  Write-Host 'RESULT: PASS'
  exit 0
}
catch {
  Write-Error $_
  Write-Host 'RESULT: FAIL'
  exit 1
}
finally {
  if ($client) { $client.Close() }
  if ($mock) {
    if ($mock.Sync.Error) { Write-Warning ("mock-side error: {0}" -f $mock.Sync.Error) }
    Write-Warning ("mock stage: {0}" -f $mock.Sync.Stage)
    # 双账本比对：注入侧 acked == mock 侧收帧数（字符计数比对的第二本账）
    if ($mock.Sync.Frames -ne $script:Acked) {
      Write-Warning ("ledger mismatch: acked={0} mock-frames={1}" -f $script:Acked, $mock.Sync.Frames)
    } else {
      Write-Host ("double-ledger comparison: PASS (acked == mock frames == {0})" -f $script:Acked)
    }
    Stop-MockEngine $mock
  }
}
