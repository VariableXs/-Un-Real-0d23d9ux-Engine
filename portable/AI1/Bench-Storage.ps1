<#
.SYNOPSIS
  Variable OS · AI-1 存储核 —— 9.1 选盘验证 + SEQ/4K/膨胀率 压测（零依赖，纯 PowerShell）

.DESCRIPTION
  对应 docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md：
    9.1  U 盘选型      -> 判定 SEQ >= 900MB/s、4K >= 20MB/s（1TB 1000MB/s 双接口盘目标）
    9.2  文件系统      -> 校验簇大小是否 64KB、分区是否 4K 对齐（扩充 29.4）
    9.3  VHDX 优化     -> 校验 TRIM(DisableDeleteNotify) 是否启用
    3.1  验收          -> VHDX 碎片率 < 5%、膨胀率（实占 vs 有效数据）
    11.3 性能基线      -> 产出可入库的基线数据（供 AI-5 bench/ 汇总）

  口径说明（写进报告，避免误读）：
    * 顺序写：FileStream + WriteThrough，绕过写缓存，接近真实设备写速。
    * 顺序读：普通带缓存读，偏乐观；权威值以 CrystalDiskMark 为准，本脚本用于快速判定与回归对比。
    * 4K 随机写：容器文件内随机 4KB 偏移直写（WriteThrough），输出 IOPS 与 MB/s。

.EXAMPLE
  .\Bench-Storage.ps1 -Path E:\
  .\Bench-Storage.ps1 -Path E:\ -Vhdx E:\Variable-OS.vhdx -OutFile .\Bench.md
  .\Bench-Storage.ps1 -Path D:\ -SeqMB 2048 -RandOps 8192
#>
[CmdletBinding()]
param(
  [string]$Path = 'D:\',
  [int]$SeqMB = 1024,
  [int]$SeqBlockKB = 1024,
  [int]$RandMB = 64,
  [int]$RandOps = 4096,
  [int]$RandBlockKB = 4,
  [string]$Vhdx,
  [string]$OutFile,
  [switch]$KeepFiles
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ------------------------------------------------------------ 工作目录
$work = $Path
if ($Path -match '^[A-Za-z]:\\?$') { $work = Join-Path $Path 'variable-bench' }
New-Item -ItemType Directory -Force -Path $work | Out-Null
$work = (Resolve-Path -LiteralPath $work).Path
Write-Host ">>> 压测目标：$work（SEQ ${SeqMB}MB / 4K x $RandOps）" -ForegroundColor Cyan

# ------------------------------------------------------------ 卷与盘信息
$root = [IO.Path]::GetPathRoot($work)
$letter = $root.TrimEnd('\', ':')
$vol = Get-Volume -DriveLetter $letter -ErrorAction SilentlyContinue
$part = Get-Partition -DriveLetter $letter -ErrorAction SilentlyContinue
$disk = $null
if ($part) { $disk = Get-Disk -Number $part.DiskNumber -ErrorAction SilentlyContinue }

$clusterKB = 0
if ($vol -and $vol.BlockSize) { $clusterKB = [int]($vol.BlockSize / 1KB) }
$alignOK = 'n/a'
if ($part) { $alignOK = if (($part.Offset % 4096) -eq 0) { 'OK' } else { "BAD(Offset=$($part.Offset))" } }
$trim = (fsutil.exe behavior query DisableDeleteNotify 2>&1) -join ' '
$trimOK = if ($trim -match '=\s*0') { 'OK(0)' } else { 'CHECK' }

Write-Host ("    卷 {0} 文件系统={1} 簇={2}KB 可用={3:N1}GB 4K对齐={4} TRIM={5}" -f `
    $root, $(if ($vol) { $vol.FileSystem } else { 'n/a' }), $clusterKB, `
    $(if ($vol) { $vol.SizeRemaining / 1GB } else { 0 }), $alignOK, $trimOK) -ForegroundColor Gray

# ------------------------------------------------------------ 顺序写
$seqFile = Join-Path $work 'variable-seq.bin'
$seqBytes = [int64]$SeqMB * 1MB
$blockBytes = [int64]$SeqBlockKB * 1KB
$buf = New-Object byte[] $blockBytes
(New-Object Random).NextBytes($buf)

Write-Host ">>> 顺序写 ${SeqMB}MB（WriteThrough）" -ForegroundColor Cyan
$fs = New-Object IO.FileStream($seqFile, [IO.FileMode]::Create, [IO.FileAccess]::Write,
  [IO.FileShare]::None, [int]$blockBytes, [IO.FileOptions]::WriteThrough)
$sw = [Diagnostics.Stopwatch]::StartNew()
$written = [int64]0
while ($written -lt $seqBytes) {
  $fs.Write($buf, 0, [int]$blockBytes)
  $written += $blockBytes
}
$fs.Flush($true)
$sw.Stop()
$fs.Dispose()
$seqWriteMBs = [math]::Round(($seqBytes / 1MB) / $sw.Elapsed.TotalSeconds, 1)
Write-Host "    顺序写 = $seqWriteMBs MB/s（$([math]::Round($sw.Elapsed.TotalSeconds,2))s）" -ForegroundColor Green

# ------------------------------------------------------------ 顺序读
Write-Host ">>> 顺序读 ${SeqMB}MB（含缓存，偏乐观）" -ForegroundColor Cyan
$rbuf = New-Object byte[] $blockBytes
$fsr = New-Object IO.FileStream($seqFile, [IO.FileMode]::Open, [IO.FileAccess]::Read,
  [IO.FileShare]::Read, [int]$blockBytes, [IO.FileOptions]::SequentialScan)
$sw2 = [Diagnostics.Stopwatch]::StartNew()
$total = [int64]0
while (($n = $fsr.Read($rbuf, 0, [int]$blockBytes)) -gt 0) { $total += $n }
$sw2.Stop()
$fsr.Dispose()
$seqReadMBs = [math]::Round(($total / 1MB) / $sw2.Elapsed.TotalSeconds, 1)
Write-Host "    顺序读 = $seqReadMBs MB/s" -ForegroundColor Green

# ------------------------------------------------------------ 4K 随机写
Write-Host ">>> 4K 随机写 x $RandOps（WriteThrough）" -ForegroundColor Cyan
$randFile = Join-Path $work 'variable-4k.bin'
$randBytes = [int64]$RandMB * 1MB
$rblk = [int64]$RandBlockKB * 1KB
$rnd = New-Object Random
$rndBuf = New-Object byte[] $rblk
$rnd.NextBytes($rndBuf)

$fs2 = New-Object IO.FileStream($randFile, [IO.FileMode]::Create, [IO.FileAccess]::ReadWrite,
  [IO.FileShare]::None, [int]$rblk, [IO.FileOptions]::WriteThrough)
$fs2.SetLength($randBytes)
$maxOffset = $randBytes - $rblk
$slots = [int64][math]::Floor($maxOffset / $rblk)
$sw3 = [Diagnostics.Stopwatch]::StartNew()
for ($i = 0; $i -lt $RandOps; $i++) {
  $off = ([int64]$rnd.Next(0, [int]$slots)) * $rblk
  $fs2.Seek($off, [IO.SeekOrigin]::Begin) | Out-Null
  $fs2.Write($rndBuf, 0, [int]$rblk)
}
$fs2.Flush($true)
$sw3.Stop()
$fs2.Dispose()
$sec3 = $sw3.Elapsed.TotalSeconds
$randIOPS = [math]::Round($RandOps / $sec3, 0)
$randMBs = [math]::Round(($RandOps * $rblk / 1MB) / $sec3, 1)
Write-Host "    4K 随机写 = $randMBs MB/s / $randIOPS IOPS" -ForegroundColor Green

if (-not $KeepFiles) {
  Remove-Item -LiteralPath $seqFile, $randFile -Force -ErrorAction SilentlyContinue
}

# ------------------------------------------------------------ VHDX 膨胀率/碎片
$vhdRows = $null
if ($Vhdx) {
  Write-Host ">>> VHDX 实占与碎片：$Vhdx" -ForegroundColor Cyan
  if (-not (Get-Module -ListAvailable -Name Hyper-V)) {
    Write-Host '    ! 无 Hyper-V 模块，跳过 VHDX 检查' -ForegroundColor Yellow
  }
  else {
    Import-Module Hyper-V
    $v = Get-VHD -Path $Vhdx
    $fileGB = [math]::Round($v.FileSize / 1GB, 2)
    $virtGB = [math]::Round($v.Size / 1GB, 1)
    $frag = $v.FragmentationPercentage
    $usedGB = $null
    $attached = $v.Attached
    try {
      if (-not $attached) { Mount-VHD -Path $Vhdx -ReadOnly }
      Start-Sleep -Seconds 2
      $leaf = Split-Path -Leaf $Vhdx
      $bd = Get-Disk | Where-Object { $_.Location -like "*$leaf*" } | Select-Object -First 1
      if ($bd) {
        $sum = (Get-Partition -DiskNumber $bd.Number -ErrorAction SilentlyContinue |
            ForEach-Object {
              $pv = $_ | Get-Volume -ErrorAction SilentlyContinue
              if ($pv -and $pv.Size) { ($pv.Size - $pv.SizeRemaining) } else { 0 }
            } | Measure-Object -Sum).Sum
        if ($null -ne $sum) { $usedGB = [math]::Round($sum / 1GB, 2) }
      }
    }
    finally {
      if (-not $attached) { Dismount-VHD -Path $Vhdx -ErrorAction SilentlyContinue }
    }
    $bloat = 'n/a'
    if ($null -ne $usedGB -and $fileGB -gt 0) {
      $bloat = [math]::Round((($fileGB - $usedGB) / $fileGB) * 100, 1)
    }
    $vhdRows = [pscustomobject]@{
      VhdType   = $v.VhdType
      虚拟GB    = $virtGB
      实占GB    = $fileGB
      有效数据GB = $(if ($null -ne $usedGB) { $usedGB } else { 'n/a' })
      膨胀率pct = $bloat
      碎片率pct = $frag
    }
    $vhdRows | Format-List | Out-Host
    if ($v.VhdType -eq 'Fixed') {
      Write-Host '    ! 固定盘实占≈虚拟大小；「回收后 <20GB 实占」验收需用 -Sparse 或改动态盘' -ForegroundColor Yellow
    }
  }
}

# ------------------------------------------------------------ 判定
$seqPass = if ($seqWriteMBs -ge 900) { 'PASS' } else { 'FAIL' }
$randPass = if ($randMBs -ge 20) { 'PASS' } else { 'FAIL' }
$clPass = if ($clusterKB -eq 64) { 'PASS' } else { "CHECK($clusterKB KB)" }
$fragPass = if ($null -ne $vhdRows -and $vhdRows.碎片率pct -gt 5) { "FAIL($($vhdRows.碎片率pct)%)" } else { 'PASS' }

$verdict = @(
  [pscustomobject]@{ 指标 = 'SEQ 顺序写'; 实测 = "$seqWriteMBs MB/s"; 目标 = '>= 900 MB/s'; 判定 = $seqPass }
  [pscustomobject]@{ 指标 = 'SEQ 顺序读（含缓存）'; 实测 = "$seqReadMBs MB/s"; 目标 = '>= 900 MB/s'; 判定 = $(if ($seqReadMBs -ge 900) { 'PASS' } else { 'INFO' }) }
  [pscustomobject]@{ 指标 = "4K 随机写（$RandBlockKB KB x $RandOps）"; 实测 = "$randMBs MB/s / $randIOPS IOPS"; 目标 = '>= 20 MB/s'; 判定 = $randPass }
  [pscustomobject]@{ 指标 = 'NTFS 簇大小（9.2）'; 实测 = "$clusterKB KB"; 目标 = '64 KB'; 判定 = $clPass }
  [pscustomobject]@{ 指标 = '4K 对齐（29.4）'; 实测 = $alignOK; 目标 = 'OK'; 判定 = $(if ($alignOK -eq 'OK') { 'PASS' } else { 'CHECK' }) }
  [pscustomobject]@{ 指标 = 'TRIM（9.3）'; 实测 = $trimOK; 目标 = '0'; 判定 = $(if ($trimOK -eq 'OK(0)') { 'PASS' } else { 'CHECK' }) }
  [pscustomobject]@{ 指标 = 'VHDX 碎片（3.1）'; 实测 = $(if ($null -ne $vhdRows) { "$($vhdRows.碎片率pct)%" } else { 'n/a' }); 目标 = '< 5%'; 判定 = $fragPass }
)
$verdict | Format-Table -AutoSize | Out-Host

# ------------------------------------------------------------ 报告
if (-not $OutFile) {
  $OutFile = Join-Path $PSScriptRoot ("Bench-{0:yyyyMMdd-HHmm}.md" -f (Get-Date))
}
$md = New-Object System.Text.StringBuilder
[void]$md.AppendLine("# 存储压测 - $((Get-Date).ToString('yyyy-MM-dd HH:mm'))")
[void]$md.AppendLine('')
[void]$md.AppendLine("- 目标：``$work``  文件系统：$(if ($vol) { $vol.FileSystem } else { 'n/a' })  簇：${clusterKB}KB")
[void]$md.AppendLine("- 参数：SEQ ${SeqMB}MB / 块 ${SeqBlockKB}KB，4K x ${RandOps}（容器 ${RandMB}MB）")
[void]$md.AppendLine('')
[void]$md.AppendLine('| 指标 | 实测 | 目标 | 判定 |')
[void]$md.AppendLine('| --- | --- | --- | --- |')
foreach ($r in $verdict) {
  [void]$md.AppendLine('| ' + $r.指标 + ' | ' + $r.实测 + ' | ' + $r.目标 + ' | ' + $r.判定 + ' |')
}
if ($null -ne $vhdRows) {
  [void]$md.AppendLine('')
  [void]$md.AppendLine("VHDX：``$Vhdx``  类型 $($vhdRows.VhdType)  虚拟 $($vhdRows.虚拟GB)GB  实占 $($vhdRows.实占GB)GB  有效数据 $($vhdRows.有效数据GB)GB  膨胀率 $($vhdRows.膨胀率pct)%  碎片 $($vhdRows.碎片率pct)%")
}
[void]$md.AppendLine('')
[void]$md.AppendLine('> 口径：顺序写为 WriteThrough 直写（接近真实设备）；顺序读含系统缓存，偏乐观，权威值以 CrystalDiskMark 为准。')
$md.ToString() | Set-Content -LiteralPath $OutFile -Encoding UTF8
Write-Host ">>> 报告已写入 $OutFile" -ForegroundColor Green
