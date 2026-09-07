param(
  [ValidateSet("Preflight", "Stage1", "Stage2", "Stage3", "Stage4", "Deploy", "Verify", "All")]
  [string]$Action = "Preflight",
  [string]$Src = "D:\Variable-USB",
  [string]$Dst = "E:\",
  [string]$DataDrive = "D:",
  [string]$IsoPath = "",
  [int]$SizeGB = 150,
  [switch]$Fixed,
  [string]$EngineDir = "",                # 换皮用：Variable Engine 编译产物目录
  [string]$VhdxPath = "",                 # 留空取 $Src\Variable-OS.vhdx
  [switch]$AllowFixedTarget,              # 允许把固定磁盘当目标（默认拒绝）
  [int]$MinFreeGB = 160,
  [switch]$DryRun,
  [switch]$Yes
)
# AI-5 交付核 / 第12章 交付与运维 四阶段落地（主计划 12.1-12.4）+ 扩充19 交付清单
# 用法:
#   .\Deploy-To-USB.ps1 -Action Preflight -Src D:\Variable-USB -Dst E:\   # 只读：预检
#   .\Deploy-To-USB.ps1 -Action Stage1 -IsoPath C:\Win11_22H2.iso         # 造盘（调 portable\Create-VHDX.ps1）
#   .\Deploy-To-USB.ps1 -Action Stage2                                     # 隔离验证（调 portable\Test-VM.ps1）
#   .\Deploy-To-USB.ps1 -Action Stage3 -EngineDir ..\..\src-tauri\target\release
#   .\Deploy-To-USB.ps1 -Action Stage4 -Dst E:\                            # 上盘（robocopy）
#   .\Deploy-To-USB.ps1 -Action Verify -Dst E:\                            # 只读：核验成品盘
#   .\Deploy-To-USB.ps1 -Action All -IsoPath C:\Win11_22H2.iso -Dst E:\    # 四阶段串起来
# 安全纪律:
#   * 目标盘必须是可移动盘且不是宿主系统盘；固定磁盘需显式 -AllowFixedTarget；
#   * robocopy 不使用 /MIR（避免误删目标盘上已有数据），只增量复制；
#   * Stage3 换皮默认 -DryRun 打印将要做的事，加 -Yes 才真写注册表/拷文件。
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "AI5-Lib.ps1")

$PortableDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path

function Get-ResolvedVhdx {
  if ($VhdxPath) { return $VhdxPath }
  return (Join-Path $Src "Variable-OS.vhdx")
}

function Invoke-Preflight {
  Write-Ai5 "阶段预检 (主计划 12.1-12.4)" "Step"
  $rows = @()

  # 1 造盘脚本
  $cv = Join-Path $PortableDir "Create-VHDX.ps1"
  $rows += [pscustomobject]@{ item = "Create-VHDX.ps1"; ok = (Test-Path -LiteralPath $cv); detail = $cv }
  $tv = Join-Path $PortableDir "Test-VM.ps1"
  $rows += [pscustomobject]@{ item = "Test-VM.ps1"; ok = (Test-Path -LiteralPath $tv); detail = $tv }

  # 2 Hyper-V 能力
  $hv = [bool](Get-Module -ListAvailable -Name Hyper-V -ErrorAction SilentlyContinue)
  $rows += [pscustomobject]@{ item = "Hyper-V 模块"; ok = $hv; detail = $(if ($hv) { "可用" } else { "未启用（造盘/测VM 需要，或改用 QEMU）" }) }

  # 3 管理员
  $adm = Test-Ai5Admin
  $rows += [pscustomobject]@{ item = "管理员权限"; ok = $adm; detail = $(if ($adm) { "是" } else { "否（Stage1/Stage3 需要）" }) }

  # 4 源目录
  $srcOk = Test-Path -LiteralPath $Src
  $rows += [pscustomobject]@{ item = "源目录 $Src"; ok = $srcOk; detail = $(if ($srcOk) { "体积 $(Get-Ai5FolderGB -Path $Src) GB" } else { "不存在" }) }

  # 5 目标盘
  $problems = @(Test-Ai5UsbTarget -Drive $Dst)
  if ($AllowFixedTarget) { $problems = @($problems | Where-Object { $_ -notmatch "固定磁盘" }) }
  $freeGB = Get-Ai5FreeGB -Path $Dst
  if ($freeGB -ge 0 -and $freeGB -lt $MinFreeGB) { $problems += "$Dst 可用 ${freeGB}GB 小于要求 ${MinFreeGB}GB" }
  $rows += [pscustomobject]@{ item = "目标盘 $Dst"; ok = ($problems.Count -eq 0); detail = $(if ($problems.Count -eq 0) { "可用 ${freeGB}GB" } else { $problems -join "; " }) }

  # 6 Data 结构
  $data = Get-Ai5DataRoot -DataDrive $DataDrive
  $need = @("Apps", "MSIX", "Plugins", "User", "Exchange", "Cache", "Dumps", "Backup", "Tests")
  $missData = @($need | Where-Object { -not (Test-Path -LiteralPath (Join-Path $data $_)) })
  $rows += [pscustomobject]@{ item = "Data 结构 $data"; ok = ($missData.Count -eq 0); detail = $(if ($missData.Count -eq 0) { "齐全" } else { "缺: $($missData -join ', ')（跑 AI4\Data-Init.ps1）" }) }

  # 7 1TB 盘可用容量校验（主计划 1.3：930GB 可用）
  try {
    $dinfo = New-Object System.IO.DriveInfo(($Dst -replace '[\\\/].*$', ''))
    $totalGB = [math]::Round($dinfo.TotalSize / 1GB, 0)
    $rows += [pscustomobject]@{ item = "盘总容量"; ok = ($totalGB -ge 900); detail = "${totalGB}GB（1TB 盘实测应 ≥900GB，标称 1TB≈931GB）" }
  } catch {
    $rows += [pscustomobject]@{ item = "盘总容量"; ok = $false; detail = "读不到容量: $($_.Exception.Message)" }
  }

  $bad = 0
  foreach ($r in $rows) {
    $icon = if ($r.ok) { "✅" } else { "❌" }
    Write-Ai5 ("{0} {1,-22} {2}" -f $icon, $r.item, $r.detail) $(if ($r.ok) { "Ok" } else { "Err" })
    if (-not $r.ok) { $bad++ }
  }
  Save-Ai5Json -Object ([pscustomobject]@{ tool = "Deploy-To-USB.ps1"; action = "Preflight"; at = (Get-Date -Format "o"); rows = $rows }) `
    -Path (Join-Path (Get-Ai5EvidenceRoot -DataDrive $DataDrive) "preflight.json") | Out-Null
  if ($bad -gt 0) { Write-Ai5 "预检有 $bad 项待处理" "Err"; return 1 }
  Write-Ai5 "预检通过，可以进入四阶段" "Ok"
  return 0
}

function Invoke-Stage1 {
  Write-Ai5 "阶段1 本地造盘（主计划 12.1）" "Step"
  if (-not $IsoPath) { Write-Ai5 "需要 -IsoPath 指向 Win11 ISO" "Err"; return 1 }
  if (-not (Test-Path -LiteralPath $IsoPath)) { Write-Ai5 "ISO 不存在: $IsoPath" "Err"; return 1 }
  $cv = Join-Path $PortableDir "Create-VHDX.ps1"
  if (-not (Test-Path -LiteralPath $cv)) { Write-Ai5 "缺 $cv" "Err"; return 1 }
  $a = @("-IsoPath", $IsoPath, "-OutDir", $Src, "-SizeGB", "$SizeGB")
  if ($Fixed) { $a += "-Fixed" }
  if ($DryRun) {
    Write-Ai5 "[dry-run] 将执行: $cv $($a -join ' ')" "Warn"; return 0
  }
  if (-not (Confirm-Ai5Dangerous -What "在 $Src 造 ${SizeGB}GB VHDX（需要管理员，约 10-20 分钟）" -Yes:$Yes)) { return 1 }
  & $cv @a
  $code = $LASTEXITCODE
  if ($code -ne 0 -and $null -ne $code) { Write-Ai5 "Create-VHDX 退出码 $code" "Err"; return 1 }
  Write-Ai5 "阶段1 完成: $(Get-ResolvedVhdx)" "Ok"
  return 0
}

function Invoke-Stage2 {
  Write-Ai5 "阶段2 隔离验证（主计划 12.2）" "Step"
  $tv = Join-Path $PortableDir "Test-VM.ps1"
  if (-not (Test-Path -LiteralPath $tv)) { Write-Ai5 "缺 $tv" "Err"; return 1 }
  $vhdx = Get-ResolvedVhdx
  if (-not (Test-Path -LiteralPath $vhdx)) { Write-Ai5 "VHDX 不存在: $vhdx（先跑 Stage1）" "Err"; return 1 }
  if ($DryRun) { Write-Ai5 "[dry-run] 将执行: $tv -Vhdx $vhdx" "Warn"; return 0 }
  Write-Ai5 "启动隔离虚拟机后，请按扩充21 逐项演练：del C:\ / 大软件加载 / 0x80000003 崩溃"
  Write-Ai5 "证据请存到 $(Get-Ai5EvidenceRoot -DataDrive $DataDrive)"
  & $tv -Vhdx $vhdx
  Write-Ai5 "阶段2 完成，接着跑 Chaos-Inject.ps1 -Action Run 与 Compat-Matrix.ps1 -Action Run" "Ok"
  return 0
}

function Invoke-Stage3 {
  Write-Ai5 "阶段3 换皮（主计划 12.3）" "Step"
  if (-not $EngineDir) {
    $guess = Join-Path $RepoRoot "src-tauri\target\release"
    if (Test-Path -LiteralPath $guess) { $EngineDir = $guess }
  }
  if (-not $EngineDir -or -not (Test-Path -LiteralPath $EngineDir)) {
    Write-Ai5 "找不到 Variable Engine 产物目录（先 npm run tauri build 或指定 -EngineDir）" "Err"; return 1
  }
  $vhdx = Get-ResolvedVhdx
  if (-not (Test-Path -LiteralPath $vhdx)) { Write-Ai5 "VHDX 不存在: $vhdx" "Err"; return 1 }

  Write-Ai5 "将执行: 挂载 $vhdx -> 拷贝 $EngineDir 到 <盘>:\Variable -> 改 Winlogon\Shell -> 卸载"
  if ($DryRun -or -not (Confirm-Ai5Dangerous -What "挂载 VHDX 并写入 Shell 替换（改注册表 Winlogon\Shell）" -Yes:$Yes)) {
    Write-Ai5 "[dry-run] 未做任何改动" "Warn"; return 0
  }
  if (-not (Test-Ai5Command "Mount-VHD")) { Write-Ai5 "Mount-VHD 不可用（无 Hyper-V 模块）" "Err"; return 1 }

  Mount-VHD -Path $vhdx -PassThru | Out-Null
  try {
    $disk = Get-Disk | Where-Object { $_.Location -like "*$([IO.Path]::GetFileName($vhdx))*" } | Select-Object -First 1
    if (-not $disk) { throw "挂载后未找到磁盘" }
    $drive = (Get-Partition -DiskNumber $disk.Number | Where-Object { $_.DriveLetter } | Select-Object -First 1).DriveLetter
    if (-not $drive) { throw "未找到分区盘符" }
    $target = "$($drive):\Variable"
    New-Ai5Directory -Path $target | Out-Null
    robocopy $EngineDir $target /E /R:1 /W:1 /NFL /NDL /NJH /NJS | Out-Null
    Write-Ai5 "已拷贝 Variable Engine -> $target" "Ok"

    $hive = "$($drive):\Windows\System32\config\SOFTWARE"
    reg load "HKLM\VariableOffline" $hive | Out-Null
    try {
      reg add "HKLM\VariableOffline\Microsoft\Windows NT\CurrentVersion\Winlogon" /v Shell /t REG_SZ /d "C:\Variable\variable.exe" /f | Out-Null
      reg add "HKLM\VariableOffline\Microsoft\Windows NT\CurrentVersion\Winlogon" /v ShellBackup /t REG_SZ /d "explorer.exe" /f | Out-Null
      Write-Ai5 "已写入 Shell=C:\Variable\variable.exe（备份值 ShellBackup=explorer.exe，回退可用）" "Ok"
    } finally {
      reg unload "HKLM\VariableOffline" | Out-Null
    }
  } finally {
    Dismount-VHD -Path $vhdx -ErrorAction SilentlyContinue
  }
  Write-Ai5 "阶段3 完成，启动虚拟机即为 Variable 桌面" "Ok"
  return 0
}

function Invoke-Stage4 {
  Write-Ai5 "阶段4 上盘（主计划 12.4，目标 10 分钟）" "Step"
  if (-not (Test-Path -LiteralPath $Src)) { Write-Ai5 "源目录不存在: $Src" "Err"; return 1 }
  $problems = @(Test-Ai5UsbTarget -Drive $Dst)
  if ($AllowFixedTarget) { $problems = @($problems | Where-Object { $_ -notmatch "固定磁盘" }) }
  if ($problems.Count -gt 0) {
    foreach ($p in $problems) { Write-Ai5 $p "Err" }
    return 1
  }
  $srcGB = Get-Ai5FolderGB -Path $Src
  $freeGB = Get-Ai5FreeGB -Path $Dst
  Write-Ai5 "源体积 ${srcGB}GB -> $Dst 可用 ${freeGB}GB"
  if ($freeGB -ge 0 -and $srcGB -gt $freeGB) { Write-Ai5 "目标空间不足" "Err"; return 1 }

  # 不用 /MIR：绝不镜像删除目标盘上已有文件
  $rcArgs = @($Src, $Dst, "/E", "/R:2", "/W:2", "/MT:8", "/XD", "Cache", "Temp", "Tests\_bench", "Tests\_fill", "/XF", "*.log", "*.tmp", "/NP")
  if ($DryRun) { $rcArgs += "/L" }
  if (-not $DryRun) {
    if (-not (Confirm-Ai5Dangerous -What "robocopy $Src -> $Dst （增量复制，不删除目标已有文件）" -Yes:$Yes)) { return 1 }
  }
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  robocopy @rcArgs | Out-Host
  $rc = $LASTEXITCODE
  $sw.Stop()
  # robocopy 退出码 0-7 都是成功语义，>=8 才是失败
  if ($rc -ge 8) { Write-Ai5 "robocopy 失败，退出码 $rc" "Err"; return 1 }
  $mins = [math]::Round($sw.Elapsed.TotalMinutes, 1)
  Write-Ai5 ("上盘完成，用时 {0} 分钟（robocopy 码 {1}）" -f $mins, $rc) "Ok"
  if ($mins -le 10) { Write-Ai5 "满足主计划 12.4「10 分钟上盘」" "Ok" } else { Write-Ai5 "超过 10 分钟目标，检查接口/线材" "Warn" }
  return 0
}

function Invoke-Verify {
  Write-Ai5 "成品盘核验（主计划 扩充19 交付清单）" "Step"
  if (-not (Test-Path -LiteralPath $Dst)) { Write-Ai5 "目标盘不存在: $Dst" "Err"; return 1 }
  $need = @(
    @{ p = "Variable-OS.vhdx"; why = "系统盘镜像" },
    @{ p = "Data"; why = "读写分离数据区" },
    @{ p = "Data\Apps"; why = "绿色软件实体" },
    @{ p = "Data\Exchange"; why = "受控摆渡通道" },
    @{ p = "Data\Backup"; why = "User.vhdx 备份位" },
    @{ p = "PortableVM"; why = "便携虚拟机 + 启动器" }
  )
  $rows = @(); $bad = 0
  foreach ($n in $need) {
    $full = Join-Path $Dst $n.p
    $ok = Test-Path -LiteralPath $full
    $rows += [pscustomobject]@{ path = $n.p; ok = $ok; why = $n.why }
    Write-Ai5 ("{0} {1,-22} {2}" -f $(if ($ok) { "✅" } else { "❌" }), $n.p, $n.why) $(if ($ok) { "Ok" } else { "Err" })
    if (-not $ok) { $bad++ }
  }
  $vhdx = Join-Path $Dst "Variable-OS.vhdx"
  if (Test-Path -LiteralPath $vhdx) {
    $len = (Get-Item -LiteralPath $vhdx).Length
    $rows += [pscustomobject]@{ path = "Variable-OS.vhdx 大小"; ok = ($len -gt 1GB); why = "$([math]::Round($len / 1GB, 2))GB" }
    Write-Ai5 ("VHDX 实占 {0}GB" -f [math]::Round($len / 1GB, 2))
  }
  Save-Ai5Json -Object ([pscustomobject]@{ tool = "Deploy-To-USB.ps1"; action = "Verify"; at = (Get-Date -Format "o"); dst = $Dst; rows = $rows }) `
    -Path (Join-Path (Get-Ai5EvidenceRoot -DataDrive $DataDrive) "deliver-verify.json") | Out-Null
  if ($bad -gt 0) { Write-Ai5 "缺 $bad 项交付物" "Err"; return 1 }
  Write-Ai5 "成品盘核验通过：A 模式双击 PortableVM\启动.exe，B 模式重启按 F12 选 U 盘" "Ok"
  return 0
}

function Invoke-All {
  $code = Invoke-Preflight; if ($code -ne 0) { return $code }
  $code = Invoke-Stage1; if ($code -ne 0) { return $code }
  $code = Invoke-Stage2; if ($code -ne 0) { return $code }
  $code = Invoke-Stage3; if ($code -ne 0) { return $code }
  $code = Invoke-Stage4; if ($code -ne 0) { return $code }
  return (Invoke-Verify)
}

$exit = 0
switch ($Action) {
  "Preflight" { $exit = Invoke-Preflight }
  "Stage1"    { $exit = Invoke-Stage1 }
  "Stage2"    { $exit = Invoke-Stage2 }
  "Stage3"    { $exit = Invoke-Stage3 }
  "Stage4"    { $exit = Invoke-Stage4 }
  "Deploy"    { $exit = Invoke-Stage4 }
  "Verify"    { $exit = Invoke-Verify }
  "All"       { $exit = Invoke-All }
}
exit $exit
