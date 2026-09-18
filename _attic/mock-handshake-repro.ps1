# Minimal repro: runspace mock + main client handshake
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
    }
  } catch { $Sync.Error = "$_" }
}

$sync = [hashtable]::Synchronized(@{ Ready = $false; Stage = '-'; Error = $null; Frames = 0 })
$rs = [runspacefactory]::CreateRunspace()
$rs.Open()
$ps = [powershell]::Create().AddScript($script:MockScript).AddArgument(14778).AddArgument($sync)
$null = $ps.BeginInvoke()
for ($i = 0; $i -lt 30 -and -not $sync.Ready; $i++) { Start-Sleep -Milliseconds 100 }
"mock ready: $($sync.Ready)"

$client = [System.Net.Sockets.TcpClient]::new()
$client.Connect('127.0.0.1', 14778)
"connected"
$stream = $client.GetStream()
$stream.ReadTimeout = 5000
$writer = [System.IO.StreamWriter]::new($stream)
$writer.NewLine = "`r`n"; $writer.AutoFlush = $true
$reader = [System.IO.StreamReader]::new($stream)
$writer.WriteLine('PING')
"ping sent; mock stage: $($sync.Stage)"
$reply = $reader.ReadLine()
"reply: [$reply]"
$client.Close()
$ps.Stop(); $rs.Close()
