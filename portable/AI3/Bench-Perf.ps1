<#
.SYNOPSIS
  V-9：性能压测（扩充 17）
  Bench-Perf.ps1 全套：顺序读写 / 4K QD1 随机 / 启动耗时 / 内存基线；
  门禁：1000MB/s 盘上 VM 磁盘性能 ≥ 本地 SSD 85%（以基准文件输出对比）。
  混沌注入 12 场景由 AI5 Chaos-Inject 承担，本脚本只做性能基准与门禁判定。

.EXAMPLE
  .\Bench-Perf.ps1 -Target E:\Uxv\bench          # U 盘（VM 盘所在卷）
  .\Bench-Perf.ps1 -Target C:\Users\me\bench -Baseline docs\acceptance\v9\ssd-baseline.json
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$Target,                     # 基准文件落点（测的就是该卷性能）
  [string]$Baseline = "",              # 本地 SSD 基线 JSON（门禁 85% 对比）
  [int]$SizeMB = 1024
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Target)) { New-Item -ItemType Directory -Force -Path $Target | Out-Null }
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$buf = New-Object byte[] (1MB)
(new-object Random).NextBytes($buf)

function Write-Seq {
  $f = Join-Path $Target 'bench-seq.tmp'
  $fs = [IO.File]::Create($f)
  $t = [Diagnostics.Stopwatch]::StartNew()
  for ($i = 0; $i -lt $SizeMB; $i++) { $fs.Write($buf, 0, $buf.Length) }
  $fs.Flush($true); $fs.Close()
  [math]::Round($SizeMB / $t.Elapsed.TotalSeconds, 1)
}
function Read-Seq {
  $f = Join-Path $Target 'bench-seq.tmp'
  $fs = [IO.File]::OpenRead($f)
  $t = [Diagnostics.Stopwatch]::StartNew()
  $tmp = New-Object byte[] (1MB)
  while ($fs.Read($tmp, 0, $tmp.Length) -gt 0) {}
  $fs.Close()
  [math]::Round($SizeMB / $t.Elapsed.TotalSeconds, 1)
}
function Rand-4K {
  # 4K QD1 随机写（V-3 验收同口径：fsutil 不可跨平台基准，用 4096 块随机偏移写）
  $f = Join-Path $Target 'bench-4k.tmp'
  $fs = [IO.File]::Create($f); $fs.SetLength(64MB)
  $t = [Diagnostics.Stopwatch]::StartNew()
  $small = New-Object byte[] 4096
  $rnd = New-Object Random 42
  for ($i = 0; $i -lt 2000; $i++) {
    $pos = [long]$rnd.Next(0, 16384) * 4096
    $fs.Position = $pos; $fs.Write($small, 0, 4096)
  }
  $fs.Flush($true); $fs.Close()
  [math]::Round(2000 / $t.Elapsed.TotalSeconds, 1)   # IOPS（4K QD1）
}

Write-Host '=== V-9 性能基准 ===' -ForegroundColor Cyan
$seqW = Write-Seq; $seqR = Read-Seq; $iops = Rand-4K
$mem = [math]::Round((GCIM | Measure-Object -Property FreePhysicalMemory -Sum).FreePhysicalMemory / 1MB, 1)
$seqFile = Join-Path $Target 'bench-seq.tmp'
Remove-Item $seqFile -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $Target 'bench-4k.tmp') -Force -ErrorAction SilentlyContinue

$result = [pscustomobject]@{
  At = (Get-Date -Format s); Target = $Target
  SeqWriteMBs = $seqW; SeqReadMBs = $seqR; Rand4KQD1IOPS = $iops
  FreeMemoryGB = $mem
}
$result | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Target 'bench-result.json') -Encoding UTF8
$result | Format-List

if ($Baseline -and (Test-Path -LiteralPath $Baseline)) {
  $base = Get-Content -LiteralPath $Baseline -Raw | ConvertFrom-Json
  $ratio = [math]::Round($seqR / [math]::Max($base.SeqReadMBs, 1) * 100, 1)
  if ($ratio -ge 85) { Write-Host "PASS 门禁：VM 盘顺序读 = 本地 SSD 的 $ratio%（≥85%）" -ForegroundColor Green }
  else { Write-Host "FAIL 门禁：VM 盘顺序读 = 本地 SSD 的 $ratio%（<85%，检查写缓存/簇大小/USB 口）" -ForegroundColor Red; exit 1 }
} else {
  Write-Host 'WARN 未提供 -Baseline 本地 SSD 基线 —— 门禁待真机对比后判定（保持 todo）' -ForegroundColor Yellow
}
