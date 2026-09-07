param(
  [ValidateSet("Status", "Optimize", "Backup", "Restore", "Schedule", "Tune")]
  [string]$Action = "Status",
  [string]$VhdxDir = "D:\Variable-USB",
  [string]$UserVhdx = "",                 # 留空取 $VhdxDir\User.vhdx
  [string]$DataDrive = "D:",
  [int]$KeepBackups = 3,                  # 主计划 14.3：保留 3 份
  [string]$BackupDir = "",                # 留空取 Data\Backup
  [switch]$DryRun,
  [switch]$Yes
)
# AI-5 交付核 / 第12.5章 运维 + 扩充22 运维手册 + 扩充26 性能调优清单
# 用法:
#   .\Maintenance.ps1 -Action Status                 # 只读：VHDX/Data/备份/计划任务现状
#   .\Maintenance.ps1 -Action Optimize               # Optimize-VHD -Mode Full（月度）
#   .\Maintenance.ps1 -Action Backup                 # User.vhdx 日备到 Data\Backup，保留 3 份
#   .\Maintenance.ps1 -Action Restore -DryRun        # 一键还原预演；去掉 -DryRun 并 -Yes 才真还原
#   .\Maintenance.ps1 -Action Schedule               # 注册每月 Optimize + 每日 Backup 计划任务
#   .\Maintenance.ps1 -Action Tune                   # 打印 26.1-26.4 调优清单与命令（不自动改宿主）
# 纪律: Restore 会覆盖 User.vhdx，必须先确认；本脚本不改宿主注册表/服务，只给命令。
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "AI5-Lib.ps1")

function Get-UserVhdx {
  if ($UserVhdx) { return $UserVhdx }
  return (Join-Path $VhdxDir "User.vhdx")
}
function Get-BackupDir {
  if ($BackupDir) { return $BackupDir }
  return (Join-Path (Get-Ai5DataRoot -DataDrive $DataDrive) "Backup")
}

function Show-Status {
  Write-Ai5 "运维现状 (主计划 12.5)" "Step"
  $rows = @()
  foreach ($v in @("Base.vhdx", "Apps.vhdx", "User.vhdx", "Variable-OS.vhdx")) {
    $p = Join-Path $VhdxDir $v
    if (Test-Path -LiteralPath $p) {
      $i = Get-Item -LiteralPath $p
      $rows += [pscustomobject]@{ file = $v; sizeGB = [math]::Round($i.Length / 1GB, 2); modified = $i.LastWriteTime.ToString("yyyy-MM-dd HH:mm") }
    }
  }
  if ($rows.Count -eq 0) { Write-Ai5 "$VhdxDir 下没有 VHDX（先跑 Deploy-To-USB.ps1 -Action Stage1）" "Warn" }
  else { $rows | Format-Table -AutoSize | Out-Host }

  if (Test-Ai5Command "Get-VHD") {
    foreach ($r in $rows) {
      try {
        $vhd = Get-VHD -Path (Join-Path $VhdxDir $r.file) -ErrorAction Stop
        Write-Ai5 ("{0}: 虚拟 {1}GB / 实占 {2}GB / 类型 {3} / 碎片率 {4}" -f $r.file, `
          [math]::Round($vhd.Size / 1GB, 1), [math]::Round($vhd.FileSize / 1GB, 1), $vhd.VhdType, `
          $(if ($null -ne $vhd.FragmentationPercentage) { "$($vhd.FragmentationPercentage)%" } else { "n/a" }))
      } catch { Write-Ai5 "$($r.file): Get-VHD 失败（可能已挂载或格式不支持）" "Warn" }
    }
  } else { Write-Ai5 "Get-VHD 不可用（无 Hyper-V 模块）" "Warn" }

  $bk = Get-BackupDir
  if (Test-Path -LiteralPath $bk) {
    $bks = @(Get-ChildItem -LiteralPath $bk -Filter "*.vhdx" -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending)
    Write-Ai5 "备份: $bk 共 $($bks.Count) 份（保留策略 $KeepBackups 份）"
    foreach ($b in ($bks | Select-Object -First 5)) {
      Write-Host ("    {0}  {1}GB  {2}" -f $b.Name, [math]::Round($b.Length / 1GB, 2), $b.LastWriteTime.ToString("yyyy-MM-dd HH:mm"))
    }
  } else { Write-Ai5 "备份目录不存在: $bk" "Warn" }

  foreach ($t in @("VariableOS-Optimize", "VariableOS-Backup")) {
    $task = $null
    try { $task = Get-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue } catch { }
    Write-Ai5 ("计划任务 {0}: {1}" -f $t, $(if ($task) { "已注册" } else { "未注册（-Action Schedule）" }))
  }
  return 0
}

function Invoke-Optimize {
  if (-not (Test-Ai5Command "Optimize-VHD")) { Write-Ai5 "Optimize-VHD 不可用（需 Hyper-V 模块）" "Err"; return 1 }
  $targets = @()
  foreach ($v in @("User.vhdx", "Apps.vhdx", "Variable-OS.vhdx")) {
    $p = Join-Path $VhdxDir $v
    if (Test-Path -LiteralPath $p) { $targets += $p }
  }
  if ($targets.Count -eq 0) { Write-Ai5 "$VhdxDir 下没有可优化的 VHDX" "Warn"; return 1 }
  foreach ($t in $targets) {
    if ($DryRun) { Write-Ai5 "[dry-run] Optimize-VHD -Path $t -Mode Full" "Warn"; continue }
    $before = [math]::Round((Get-Item -LiteralPath $t).Length / 1GB, 2)
    Write-Ai5 "Optimize-VHD -Mode Full: $t (优化前 ${before}GB)" "Step"
    try { Optimize-VHD -Path $t -Mode Full | Out-Null } catch { Write-Ai5 "  失败: $($_.Exception.Message)（VHDX 可能处于挂载状态）" "Warn"; continue }
    $after = [math]::Round((Get-Item -LiteralPath $t).Length / 1GB, 2)
    Write-Ai5 ("  优化后 {0}GB（回收 {1}GB）" -f $after, [math]::Round($before - $after, 2)) "Ok"
  }
  return 0
}

function Invoke-Backup {
  $src = Get-UserVhdx
  if (-not (Test-Path -LiteralPath $src)) { Write-Ai5 "没有 $src（差分盘尚未创建或未指定 -UserVhdx）" "Warn"; return 1 }
  $bk = Get-BackupDir
  New-Ai5Directory -Path $bk | Out-Null
  $stamp = Get-Date -Format "yyyy-MM-dd"
  $dest = Join-Path $bk ("User-$stamp.vhdx")
  if ((Test-Path -LiteralPath $dest) -and -not $Yes) {
    Write-Ai5 "今日备份已存在: $dest（加 -Yes 覆盖）" "Warn"; return 0
  }
  if ($DryRun) { Write-Ai5 "[dry-run] 将复制 $src -> $dest" "Warn"; return 0 }
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  Copy-Item -LiteralPath $src -Destination $dest -Force
  $sw.Stop()
  Write-Ai5 ("已备份 {0} ({1}GB, {2}s)" -f $dest, [math]::Round((Get-Item -LiteralPath $dest).Length / 1GB, 2), [math]::Round($sw.Elapsed.TotalSeconds, 1)) "Ok"

  # 保留 N 份，其余删除（只删 Backup 目录内的 User-*.vhdx）
  $all = @(Get-ChildItem -LiteralPath $bk -Filter "User-*.vhdx" -File | Sort-Object LastWriteTime -Descending)
  if ($all.Count -gt $KeepBackups) {
    foreach ($old in ($all | Select-Object -Skip $KeepBackups)) {
      Write-Ai5 "超出保留策略，删除 $($old.Name)" "Warn"
      Remove-Item -LiteralPath $old.FullName -Force -ErrorAction SilentlyContinue
    }
  }
  return 0
}

function Invoke-Restore {
  $bk = Get-BackupDir
  $dstFile = Get-UserVhdx
  $all = @(Get-ChildItem -LiteralPath $bk -Filter "User-*.vhdx" -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending)
  if ($all.Count -eq 0) { Write-Ai5 "$bk 下没有备份可还原" "Err"; return 1 }
  $newest = $all[0]
  Write-Ai5 "一键还原: $($newest.FullName) -> $dstFile" "Step"
  if ($DryRun) { Write-Ai5 "[dry-run] 未做任何改动" "Warn"; return 0 }
  if (-not (Confirm-Ai5Dangerous -What "覆盖 $dstFile（当前差分盘内容会丢失）" -Yes:$Yes)) { return 1 }
  if (Test-Path -LiteralPath $dstFile) {
    $safety = "$dstFile.pre-restore-$(Get-Ai5Timestamp)"
    Rename-Item -LiteralPath $dstFile -NewName (Split-Path $safety -Leaf) -Force
    Write-Ai5 "原文件已改名保留: $safety" "Ok"
  }
  Copy-Item -LiteralPath $newest.FullName -Destination $dstFile -Force
  Write-Ai5 "还原完成: $dstFile" "Ok"
  return 0
}

function Invoke-Schedule {
  if (-not (Test-Ai5Command "Register-ScheduledTask")) { Write-Ai5 "计划任务模块不可用" "Err"; return 1 }
  $self = (Join-Path $PSScriptRoot "Maintenance.ps1")
  if ($DryRun) {
    Write-Ai5 "[dry-run] 将注册 VariableOS-Optimize(每月1日 03:00) 与 VariableOS-Backup(每日 02:00)" "Warn"; return 0
  }
  if (-not (Test-Ai5Admin)) { Write-Ai5 "注册计划任务需要管理员" "Err"; return 1 }

  $a1 = New-ScheduledTaskAction -Execute "powershell.exe" `
    -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$self`" -Action Optimize -VhdxDir `"$VhdxDir`""
  $t1 = New-ScheduledTaskTrigger -Weekly -DaysOfWeek Sunday -At 3am
  Register-ScheduledTask -TaskName "VariableOS-Optimize" -Action $a1 -Trigger $t1 -Description "月度 Optimize-VHD（主计划 12.5）" -Force | Out-Null
  Write-Ai5 "已注册 VariableOS-Optimize（每周日 03:00 触发，脚本内部按月度节奏执行）" "Ok"

  $a2 = New-ScheduledTaskAction -Execute "powershell.exe" `
    -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$self`" -Action Backup -VhdxDir `"$VhdxDir`" -DataDrive `"$DataDrive`""
  $t2 = New-ScheduledTaskTrigger -Daily -At 2am
  Register-ScheduledTask -TaskName "VariableOS-Backup" -Action $a2 -Trigger $t2 -Description "每日 User.vhdx 备份（主计划 12.5 / 22.2）" -Force | Out-Null
  Write-Ai5 "已注册 VariableOS-Backup（每日 02:00）" "Ok"
  return 0
}

function Show-Tune {
  Write-Ai5 "性能调优清单（主计划 26.1-26.4）—— 只打印命令，不自动改宿主" "Step"
  $lines = @(
    "# 26.1 注册表（在虚拟系统内执行，不动宿主）",
    'reg add "HKLM\SYSTEM\CurrentControlSet\Control\FileSystem" /v NtfsDisableLastAccessUpdate /t REG_DWORD /d 1 /f',
    'reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" /v "Max Cached Icons" /t REG_DWORD /d 4096 /f',
    'reg add "HKLM\SYSTEM\CurrentControlSet\Services\SysMain" /v Start /t REG_DWORD /d 4 /f   # 禁用 Superfetch',
    "",
    "# 26.2 服务：禁用 SysMain / WSearch（对 VHDX）/ DiagTrack，Windows Update 改手动",
    "sc.exe config SysMain start= disabled",
    "sc.exe config WSearch start= demand",
    "sc.exe config DiagTrack start= disabled",
    "sc.exe config wuauserv start= demand",
    "",
    "# 26.3 电源：U盘不休眠 + 关休眠文件省 8GB",
    "powercfg /change standby-timeout-ac 0",
    "powercfg /hibernate off",
    "",
    "# 26.4 碎片与 TRIM",
    "fsutil behavior set DisableDeleteNotify 0",
    "fsutil behavior set DisableLastAccess 1",
    "defrag C: /O /V          # 每月一次",
    "Optimize-Volume -DriveLetter C -ReTrim",
    "",
    "# 9.4 系统裁剪",
    "Dism /Online /Cleanup-Image /StartComponentCleanup /ResetBase",
    "compact /compactos:always"
  )
  foreach ($l in $lines) { Write-Host $l }
  Write-Host ""
  Write-Ai5 "上述命令均在便携虚拟系统内执行；宿主保持不动，符合主计划 10.4 合规要求。" "Warn"
  return 0
}

$exit = 0
switch ($Action) {
  "Status"   { $exit = Show-Status }
  "Optimize" { $exit = Invoke-Optimize }
  "Backup"   { $exit = Invoke-Backup }
  "Restore"  { $exit = Invoke-Restore }
  "Schedule" { $exit = Invoke-Schedule }
  "Tune"     { $exit = Show-Tune }
}
exit $exit
