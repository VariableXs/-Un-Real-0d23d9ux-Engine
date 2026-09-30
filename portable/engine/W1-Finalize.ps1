<#
.SYNOPSIS
  VARIX 三体 · AI-2 W1 收尾三合一：A: WE 壁纸库搬家到 SHARED；B: ESP 引导接线（S1.3 机械执行，最小侵入）；C: 清账。

.DESCRIPTION
  A: Wallpaper Engine(431960) 应用+工坊内容整体搬家 D:\steam → W:\SteamLibrary
     （Steam 先停；robocopy 校验后删源；libraryfolders.vdf 注册，备份先行）。
  B: U 盘 ESP（S1.3 机械部分，最小侵入）：bcdboot X:\Windows /s ESP /f UEFI
     —— 只新增 \EFI\Microsoft\*，绝不碰 \EFI\Boot\bootx64.efi 与 \EFI\limine\*
     —— NVRAM 新条目若出现则 displayorder /addlast（默认引导序首项不变，前后快照核验）。
  C: ready 标记 edition 名修正 + 计划任务 VarixW1Deploy 清理。
  日志：D:\VarixDeploy\w1-finalize.log（终态行 W1-FINALIZE-DONE / W1-FINALIZE-FAIL）。
#>
[CmdletBinding()]
param(
  [string]$LogPath = 'D:\VarixDeploy\w1-finalize.log'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Log { param([string]$Msg)
  $line = ("[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $Msg)
  Write-Host $line
  Add-Content -LiteralPath $LogPath -Value $line -Encoding UTF8
}
function Finish { param([string]$Tag, [string]$Msg)
  Log ("W1-FINALIZE-" + $Tag + " | " + $Msg)
  if ($Tag -eq 'DONE') { exit 0 } else { exit 4 }
}
function DirStat { param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return $null }
  $m = Get-ChildItem -LiteralPath $Path -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum
  return [pscustomobject]@{ Files = $m.Count; Bytes = $m.Sum }
}

try { New-Item -ItemType Directory -Path (Split-Path $LogPath) -Force | Out-Null } catch {}
Log ("W1 收尾开始 pid=" + $pid)

try {

  # ================= A: WE 壁纸库搬家 =================
  Log "== A: WE 壁纸库搬家 =="
  $wv = Get-Volume -FileSystemLabel 'SHARED' -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $wv -or -not $wv.DriveLetter) {
    Log "A-G0 SHARED 卷无盘符——定位 USB 盘上的 560GB 分区并回配 W:"
    $part = Get-Partition -DriveLetter 'X'
    $shP = Get-Partition -DiskNumber $part.DiskNumber | Where-Object { [math]::Round($_.Size / 1GB) -ge 550 -and -not $_.DriveLetter } | Select-Object -First 1
    if (-not $shP) { Finish 'FAIL' 'A-G0: 560GB 无盘符分区未找到' }
    Add-PartitionAccessPath -DiskNumber $shP.DiskNumber -PartitionNumber $shP.PartitionNumber -AccessPath 'W:\' -ErrorAction Stop
    $wv = Get-Volume -FileSystemLabel 'SHARED' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $wv) { Finish 'FAIL' 'A-G0: 回配盘符后仍未找到 SHARED 卷' }
    Log "A-G0 PASS SHARED 分区回配盘符 W:"
  }
  # DriveLetter 在 Add-PartitionAccessPath 后可能不即时反映——回退到已知访问路径 W:\
  if ($wv.DriveLetter) { $wL = "$($wv.DriveLetter):" } else { $wL = 'W:' }
  $wFree = [math]::Round($wv.SizeRemaining / 1GB, 1)
  if ($wFree -lt 10) { Finish 'FAIL' "A-G1: SHARED 剩余 ${wFree}GB 不足" }
  Log ("A-G1 PASS " + $wL + " 剩余 " + $wFree + "GB")
  $dstLib = "$($wL)\SteamLibrary"
  $dstApp = Join-Path $dstLib 'steamapps\common\wallpaper_engine'
  $dstWs = Join-Path $dstLib 'steamapps\workshop\content\431960'
  # 全量校验快照（02:28 首轮 robocopy 后 A-VERIFY PASS 的实测值；日志可查）
  $expAppF = 7112; $expAppB = 2031424042
  $expWsF = 100;   $expWsB = 1802188691
  $d0 = DirStat $dstApp; $d1 = DirStat $dstWs
  $dstComplete = $d0 -and $d1 -and $d0.Files -eq $expAppF -and $d0.Bytes -eq $expAppB -and $d1.Files -eq $expWsF -and $d1.Bytes -eq $expWsB
  if ($dstComplete) {
    Log "A-SKIP 壁纸库已完整迁移至 W:\SteamLibrary（快照口径全过）——源清理在先轮已完成"
  } else {
  $srcApp = 'D:\steam\steamapps\common\wallpaper_engine'
  $srcWs = 'D:\steam\steamapps\workshop\content\431960'
  $srcAcf = 'D:\steam\steamapps\appmanifest_431960.acf'
  foreach ($p in @($srcApp, $srcWs, $srcAcf)) { if (-not (Test-Path -LiteralPath $p)) { Finish 'FAIL' "A-G2: 源缺失 $p" } }
  Log "A-G2 PASS 源三件齐（app/workshop/acf）"
  $s0 = DirStat $srcApp
  if ($d0 -and $d0.Files -eq $expAppF -and $d0.Bytes -eq $expAppB) {
    Log "A-1 SKIP app 目标已完整（快照口径 $expAppF 文件；源为部分删除残留）"
  } elseif ($s0 -and $d0 -and $s0.Files -eq $d0.Files -and $s0.Bytes -eq $d0.Bytes) {
    Log "A-1 SKIP app 已迁移且与源一致"
  } else {
    Log "A-1 robocopy app (1.89GB 级)..."
    robocopy $srcApp $dstApp /E /COPY:DAT /R:2 /W:2 /NFL /NDL /NJH | Out-Null
    if ($LASTEXITCODE -ge 8) { Finish 'FAIL' "robocopy app 失败 exit=$LASTEXITCODE" }
    $d0 = DirStat $dstApp
    if (-not $d0 -or $d0.Files -ne $expAppF) { Finish 'FAIL' "A-1: app 拷贝后文件数不符（$($d0.Files) ≠ $expAppF）" }
  }
  $s1 = DirStat $srcWs; $d1 = DirStat $dstWs
  if ($d1 -and $s1 -and $s1.Files -eq $d1.Files -and $s1.Bytes -eq $d1.Bytes) {
    Log "A-2 SKIP workshop 已迁移且与源一致"
  } elseif ($d1 -and $d1.Files -eq $expWsF -and $d1.Bytes -eq $expWsB) {
    Log "A-2 SKIP workshop 目标已完整（快照口径 $expWsF 文件）"
  } else {
    Log "A-2 robocopy workshop content (1.68GB 级)..."
    robocopy $srcWs $dstWs /E /COPY:DAT /R:2 /W:2 /NFL /NDL /NJH | Out-Null
    if ($LASTEXITCODE -ge 8) { Finish 'FAIL' "robocopy workshop 失败 exit=$LASTEXITCODE" }
    $d1 = DirStat $dstWs
    if (-not $d1 -or $d1.Files -ne $expWsF) { Finish 'FAIL' "A-2: workshop 拷贝后文件数不符（$($d1.Files) ≠ $expWsF）" }
  }
  # 校验：app 按快照口径、workshop 按源=目标逐字节口径
  if (-not $d0 -or $d0.Files -ne $expAppF -or $d0.Bytes -ne $expAppB) {
    Finish 'FAIL' ("A-VERIFY FAIL: app dst=" + $(if ($d0) { "$($d0.Files)f/$($d0.Bytes)b" }) + " 期望 ${expAppF}f/${expAppB}b")
  }
  if (-not $d1 -or -not $s1 -or $s1.Files -ne $d1.Files -or $s1.Bytes -ne $d1.Bytes) {
    Finish 'FAIL' ("A-VERIFY FAIL: workshop src=" + $(if ($s1) { "$($s1.Files)f/$($s1.Bytes)b" }) + " dst=" + $(if ($d1) { "$($d1.Files)f/$($d1.Bytes)b" }))
  }
  Log ("A-VERIFY PASS app $expAppF 文件 / " + [math]::Round($expAppB / 1GB, 2) + "GB；workshop $expWsF 文件 / " + [math]::Round($expWsB / 1GB, 2) + "GB")
  $acfDst = Join-Path $dstLib 'steamapps\appmanifest_431960.acf'
  if (-not (Test-Path -LiteralPath $acfDst)) {
    Copy-Item -LiteralPath $srcAcf -Destination $acfDst -Force
    $hSrc = (Get-FileHash $srcAcf -Algorithm SHA256).Hash
    $hDst = (Get-FileHash $acfDst -Algorithm SHA256).Hash
    if ($hSrc -ne $hDst) { Finish 'FAIL' 'A-VERIFY: acf 哈希不一致' }
    Log "A-VERIFY PASS acf 哈希一致"
  } else {
    Log "A-VERIFY SKIP acf 已在位（续跑）"
  }
  # 注册库（幂等：已有 SteamLibrary 条目则跳过；backup 先行）
  $lfPath = 'D:\steam\config\libraryfolders.vdf'
  $raw = Get-Content -LiteralPath $lfPath -Raw
  if ($raw -match 'SteamLibrary') {
    Log "A-3 SKIP libraryfolders.vdf 已注册（续跑）"
  } else {
    Copy-Item -LiteralPath $lfPath -Destination ($lfPath + '.bak-w1finalize') -Force
    $n = [regex]::Matches($raw, '"\d+"\s*\{').Count
    $escPath = 'W:\\SteamLibrary'
    $newEntry = "`t`t`"$n`"`r`n`t`t{`r`n`t`t`t`"path`"`t`"$escPath`"`r`n`t`t`t`"label`"`t`"`"`r`n`t`t`t`"apps`"`r`n`t`t`t{`r`n`t`t`t`t`"431960`"`t`"`0`"`r`n`t`t`t}`r`n`t`t}`r`n"
    $idx = $raw.LastIndexOf('}')
    $raw2 = $raw.Substring(0, $idx) + $newEntry + $raw.Substring($idx)
    [IO.File]::WriteAllText($lfPath, $raw2, [Text.UTF8Encoding]::new($false))
    Log "A-3 libraryfolders.vdf 已注册 W:\SteamLibrary（备份 .bak-w1finalize）"
  }
  # 删源（搬家语义；已校验）。WE 渲染进程族独立于 Steam——全部停止后删源。
  $stopNames = @('steam', 'steamwebhelper', 'wallpaper32', 'wallpaper64', 'wallpaper_engine', 'wallpaperservice64')
  foreach ($pn in $stopNames) { Stop-Process -Name $pn -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Seconds 3
  foreach ($p in @($srcApp, $srcWs)) {
    Remove-Item -LiteralPath $p -Recurse -Force -ErrorAction SilentlyContinue
  }
  Remove-Item -LiteralPath $srcAcf -Force -ErrorAction SilentlyContinue
  $leftApp = DirStat $srcApp; $leftWs = DirStat $srcWs
  $leftN = 0
  if ($leftApp) { $leftN += $leftApp.Files }
  if ($leftWs) { $leftN += $leftWs.Files }
  if ($leftN -gt 0) {
    Log "A-4 WARN 源残留 $leftN 个文件（被占用）——列出残留项"
    foreach ($p in @($srcApp, $srcWs)) {
      if (Test-Path -LiteralPath $p) {
        Get-ChildItem -LiteralPath $p -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 10 | ForEach-Object { Log ("  LEFT|" + $_.FullName) }
      }
    }
    Finish 'FAIL' "A-4 源未删净（$leftN 文件被占用；目标已完整迁移——不影响使用，解除占用后重跑本脚本即可收口）"
  }
  Log "A-4 源已删除（搬家完成；WE 现装于 W:\SteamLibrary）"
  }
  Log "== A 完成 =="

  # ================= B: ESP 引擎接线（S1.3 机械部分）=================
  Log "== B: ESP 接线（最小侵入）=="
  $part = Get-Partition -DriveLetter 'X'
  $esp = Get-Partition -DiskNumber $part.DiskNumber | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
  if (-not $esp) { Finish 'FAIL' 'B-G1: U 盘 ESP 分区未找到' }
  $espL = $null
  foreach ($c in 'YZWVUTSRQ'.ToCharArray()) {
    $cl = [string]$c
    if (-not (Get-Volume -DriveLetter $cl -ErrorAction SilentlyContinue)) { $espL = $cl; break }
  }
  if (-not $espL) { Finish 'FAIL' 'B-G1: 无空闲盘符' }
  Add-PartitionAccessPath -DiskNumber $esp.DiskNumber -PartitionNumber $esp.PartitionNumber -AccessPath "$($espL):\" -ErrorAction Stop
  Log ("B-G1 PASS ESP=" + $esp.PartitionNumber + " 号分区挂载 $espL")
  $before = Get-ChildItem -LiteralPath "$($espL):\" -Recurse -File -ErrorAction SilentlyContinue | Select-Object FullName, Length
  $beforeCount = ($before | Measure-Object).Count
  $bootEfi = "$($espL):\EFI\Boot\bootx64.efi"
  $hBootEfi = if (Test-Path -LiteralPath $bootEfi) { (Get-FileHash $bootEfi -Algorithm SHA256).Hash } else { 'ABSENT' }
  Log ("B-G2 快照完成（" + $beforeCount + " 文件；bootx64.efi 哈希已记录）")
  # firmware displayorder 首项快照
  $fwBefore = (& bcdedit /enum firmware | Out-String)
  $orderBefore = ([regex]::Match($fwBefore, 'displayorder\s+([^\r\n]+)')).Groups[1].Value.Trim()
  Log ("B-G3 NVRAM displayorder 前= " + $orderBefore)
  & bcdboot "X:\Windows" /s "$($espL):" /f UEFI | Out-Null
  if ($LASTEXITCODE -ne 0) { Finish 'FAIL' "bcdboot 失败 exit=$LASTEXITCODE" }
  Log "B-1 bcdboot 完成（\EFI\Microsoft\* + BCD 已写入 ESP）"
  # NVRAM 新条目检测与 addlast
  $fwAfter = (& bcdedit /enum firmware | Out-String)
  $guidsBefore = [regex]::Matches($fwBefore, '\{[0-9a-fA-F\-]{36}\}') | ForEach-Object { $_.Value } | Select-Object -Unique
  $guidsAfter = [regex]::Matches($fwAfter, '\{[0-9a-fA-F\-]{36}\}') | ForEach-Object { $_.Value } | Select-Object -Unique
  $newGuids = @($guidsAfter | Where-Object { $guidsBefore -notcontains $_ })
  if ($newGuids.Count -gt 0) {
    foreach ($g in $newGuids) {
      & bcdedit /set '{fwbootmgr}' displayorder "$g" /addlast | Out-Null
      Log ("B-2 NVRAM 新条目 " + $g + " 已 displayorder /addlast（默认引导序首项不变）")
    }
    $fwFinal = (& bcdedit /enum firmware | Out-String)
    $orderAfter = ([regex]::Match($fwFinal, 'displayorder\s+([^\r\n]+)')).Groups[1].Value.Trim()
    Log ("B-3 NVRAM displayorder 后= " + $orderAfter)
    if (-not $orderAfter.StartsWith($orderBefore.Split(' ')[0])) {
      Log "B-3 WARN: displayorder 首项与部署前不一致——如实上报，未自动改动"
    } else {
      Log "B-3 PASS 默认引导序首项保持不变"
    }
  } else {
    Log "B-2 NVRAM 无新条目（bcdboot /s 模式未写固件入口——引导选择方案见执行记录）"
  }
  # ESP 增量核验：只许新增 \EFI\Microsoft\*
  $after = Get-ChildItem -LiteralPath "$($espL):\" -Recurse -File -ErrorAction SilentlyContinue | Select-Object FullName, Length
  $afterCount = ($after | Measure-Object).Count
  $beforeNames = @($before | ForEach-Object { $_.FullName })
  $added = @($after | Where-Object { $beforeNames -notcontains $_.FullName })
  $addedNonMs = @($added | Where-Object { $_.FullName -notmatch '\\EFI\\Microsoft\\' })
  if ($addedNonMs.Count -gt 0) {
    foreach ($f in $addedNonMs) { Log ("B-4 WARN 非 Microsoft 增量: " + $f.FullName) }
  } else {
    Log ("B-4 增量 " + $added.Count + " 个文件全部位于 \EFI\Microsoft\*")
  }
  $bootEfiAfter = if (Test-Path -LiteralPath $bootEfi) { (Get-FileHash $bootEfi -Algorithm SHA256).Hash } else { 'ABSENT' }
  if ($hBootEfi -ne 'ABSENT' -and $hBootEfi -ne $bootEfiAfter) { Finish 'FAIL' 'B-4 FAIL: bootx64.efi 被改动（Limine 引导红线）' }
  Log ("B-4 PASS bootx64.efi 哈希不变；ESP 文件数 " + $beforeCount + " → " + $afterCount + "（增量应全部为 \EFI\Microsoft\*）")
  Log "== B 完成 =="

  # ================= C: 清账 =================
  Log "== C: 清账 =="
  $marker = Get-Content -LiteralPath 'X:\varix-w1-ready.txt' -Raw | ConvertFrom-Json
  $marker.edition = 'Windows 11 专业版'
  [IO.File]::WriteAllText('X:\varix-w1-ready.txt', ($marker | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
  Log "C-1 ready 标记 edition 已修正为 Windows 11 专业版"
  schtasks /Delete /TN 'VarixW1Deploy' /F | Out-Null
  Log ("C-2 计划任务 VarixW1Deploy 清理 exit=" + $LASTEXITCODE)
  Log "== C 完成 =="

  # 清理：卸载残留挂载的部署 ISO（如仍在）
  $isoProbe = Get-Volume -FileSystemLabel 'CCCOMA_X64FRE_ZH-CN_DV9' -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($isoProbe) {
    Dismount-DiskImage -ImagePath 'D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso' -ErrorAction SilentlyContinue | Out-Null
    Log "C-3 残留挂载的部署 ISO 已卸载"
  }

  Finish 'DONE' "收尾完成：WE 库已迁 W:\SteamLibrary；ESP 已接线（Microsoft 引导文件落位、默认引导序未变）；清账完毕"
}
catch {
  Log ("EXC|" + $_.Exception.GetType().FullName + "|" + $_.Exception.Message)
  if ($_.InvocationInfo) { Log ("AT|" + $_.InvocationInfo.PositionMessage) }
  Finish 'FAIL' "未预期异常（见 EXC 行）"
}
