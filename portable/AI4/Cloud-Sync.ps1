param(
  [ValidateSet("Setup", "Sync", "Restore", "Schedule", "Diff", "Prune", "Backup", "Status")]
  [string]$Action = "Status",
  [string]$Local = "D:\Data",
  [string]$Remote = "remote:VariableBackup",
  [string]$ConfigDir = "D:\Data\Sync",
  [string]$CloudExcludes = "*.tmp,*.log,.git,node_modules,Cache,Dumps",
  [int]$Transfers = 4,
  [int]$BwLimit = 10,              # MB/s
  [string]$ScheduleTime = "30",    # 分钟
  [string]$RemoteFile = "D:\Data\Backup\VariableData.zip",
  [string]$BackupSource = "D:\Variable-USB\User.vhdx",  # 14.3 每日 Checkpoint 对象
  [int]$KeepBackups = 3,           # 14.3 User.vhdx 保留 3 份
  [int]$RetentionDays = 7,         # 14.3 云端 _archive 版本保留 7 天
  [switch]$Crypt,                  # 8.5 启用 rclone crypt 加密层
  [switch]$DryRun
)
# AI-4 拓展核 / 第8.5章 云拓展(离线+增量+加密) + 第14.3章 云同步与版本
#   要求已配置 rclone remote(如 OneDrive/坚果云/S3)。
# 用法:
#   .\Cloud-Sync.ps1 -Action Setup -Remote "onedrive:VariableBackup" -Crypt   # 生成 crypt 加密 remote
#   .\Cloud-Sync.ps1 -Action Sync                    # 增量上行(变更/删除文件自动入 _archive 保留7天)
#   .\Cloud-Sync.ps1 -Action Diff -DryRun            # 只看差异
#   .\Cloud-Sync.ps1 -Action Restore -RemoteFile ... # 恢复
#   .\Cloud-Sync.ps1 -Action Schedule -ScheduleTime 30
#   .\Cloud-Sync.ps1 -Action Prune                   # 清理云端超过 7 天的历史版本
#   .\Cloud-Sync.ps1 -Action Backup                  # User.vhdx -> Data\Backup\User-YYYYMMDD.vhdx 保留3份
#   .\Cloud-Sync.ps1 -Action Status
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$CryptRemoteName = "VariableCrypt"   # rclone crypt 包装 remote 的固定名字

function Require-Rclone {
  if (-not (Get-Command rclone.exe -ErrorAction SilentlyContinue)) {
    throw "未找到 rclone.exe。请安装并配置 remote:  https://rclone.org/ "
  }
}

function Parse-Excludes {
  param([string]$Csv)
  return ($Csv -split "," | ForEach-Object { $_.Trim() } | Where-Object { $_ } | ForEach-Object { "--exclude=" + $_ })
}

function Get-SyncConf {
  # 读取 Setup 写入的配置; 返回 @{ Remote=...; Crypt=bool }
  $conf = @{ Remote = $Remote; Crypt = $false }
  $confFile = Join-Path $ConfigDir "rclone.conf"
  if (Test-Path $confFile) {
    foreach ($line in Get-Content $confFile) {
      if ($line -match "^remote\s*=\s*(.+)$") { $conf.Remote = $Matches[1].Trim() }
      if ($line -match "^crypt\s*=\s*1$")     { $conf.Crypt = $true }
    }
  }
  return $conf
}

function Get-EffectiveRemote {
  # 已启用加密 -> 走 crypt remote(内容加密后落盘到底层 remote)
  param([hashtable]$Conf)
  if ($Conf.Crypt -and (Test-Path (Join-Path $ConfigDir "crypt.key"))) {
    return "${CryptRemoteName}:VariableBackup"
  }
  return $Conf.Remote
}

function New-RandomPass {
  # 32 字节随机 -> base64, 作为 rclone crypt 密码
  $b = New-Object byte[] 32
  [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($b)
  return [Convert]::ToBase64String($b)
}

function Setup-Cloud {
  Require-Rclone
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  if (-not $Remote) { throw "-Remote 必填, 例如 onedrive:VariableBackup" }
  $lines = @(
    "# Variable OS 云同步配置 (由 Cloud-Sync.ps1 -Action Setup 生成)",
    "local = $Local",
    "remote = $Remote",
    "transfers = $Transfers",
    "bwlimit = ${BwLimit}M",
    "excludes = $CloudExcludes",
    "interval_min = $ScheduleTime"
  )
  if ($Crypt) {
    Write-Host ">>> 生成 rclone crypt 加密 remote ($CryptRemoteName -> $Remote)" -ForegroundColor Cyan
    $pass1 = New-RandomPass
    $pass2 = New-RandomPass
    # 密钥文件只落 Data(随盘走), 丢失则云端密文永久不可解 -> 必须离线再备份一份
    Set-Content -Path (Join-Path $ConfigDir "crypt.key") -Value @($pass1, $pass2) -Encoding ASCII
    & rclone.exe config create $CryptRemoteName crypt remote=$Remote filename_encryption=standard directory_name_encryption=true password=$pass1 password2=$pass2 --obscure | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "rclone config create crypt remote 失败 (exit $LASTEXITCODE)" }
    $lines += "crypt = 1"
    Write-Host "    密钥已写入 $(Join-Path $ConfigDir 'crypt.key') (请抄写离线保管, 丢失=云端密文不可恢复)" -ForegroundColor Yellow
  }
  Set-Content -Path (Join-Path $ConfigDir "rclone.conf") -Value $lines -Encoding UTF8
  Write-Host ">>> 已写入 $(Join-Path $ConfigDir 'rclone.conf')" -ForegroundColor Green
  $probeRemote = if ($Crypt) { "${CryptRemoteName}:" } else { $Remote }
  Write-Host "    先验证: rclone lsd $probeRemote" -ForegroundColor Yellow
}

function Sync-Cloud {
  Require-Rclone
  $conf = Get-SyncConf
  $target = Get-EffectiveRemote $conf
  # 14.3 版本保留: 被覆盖/删除的文件先移入云端 _archive/<日期>, 保留 RetentionDays 天
  $archive = "$target/_archive/$(Get-Date -Format yyyyMMdd)"
  $rArgs = @("sync", $Local, $target, "--transfers", "$Transfers", "--bwlimit", "${BwLimit}M",
             "--backup-dir", $archive)
  foreach ($x in (Parse-Excludes $CloudExcludes)) { $rArgs += $x }
  if ($DryRun) {
    $rArgs += "--dry-run"
    Write-Host ">>> 预演同步 (dry-run) $Local -> $target" -ForegroundColor Cyan
  } else {
    Write-Host ">>> 增量同步 $Local -> $target (旧版本入 _archive)" -ForegroundColor Cyan
  }
  & rclone.exe @rArgs
  if ($LASTEXITCODE -ne 0) { throw "rclone sync 失败 (exit $LASTEXITCODE)" }
  Write-Host ">>> 同步完成" -ForegroundColor Green
}

function Prune-Cloud {
  # 14.3: 清理云端 _archive 中超过 RetentionDays 天的历史版本
  Require-Rclone
  $conf = Get-SyncConf
  $target = Get-EffectiveRemote $conf
  $archiveRoot = "$target/_archive"
  Write-Host ">>> 清理 $archiveRoot 中超过 $RetentionDays 天的版本" -ForegroundColor Cyan
  $dirs = & rclone.exe lsd $archiveRoot 2>$null
  if ($LASTEXITCODE -ne 0) { Write-Warning "无法列出 $archiveRoot (可能尚无历史版本)"; return }
  $cutoff = (Get-Date).AddDays(-$RetentionDays)
  $removed = 0
  foreach ($d in $dirs) {
    # rclone lsd 输出第2列为修改日期, 目录名为 yyyyMMdd
    if ($d -match "(\d{8})") {
      $stamp = $Matches[1]
      try { $dirDate = [datetime]::ParseExact($stamp, "yyyyMMdd", $null) } catch { continue }
      if ($dirDate -lt $cutoff.Date) {
        Write-Host "    清理 $stamp (早于 $(Get-Date $cutoff -Format yyyy-MM-dd))" -ForegroundColor Yellow
        if (-not $DryRun) { & rclone.exe purge "$archiveRoot/$stamp" | Out-Null }
        $removed++
      }
    }
  }
  if ($removed -eq 0) { Write-Host "    无需清理" -ForegroundColor Green }
  else { Write-Host ">>> 已清理 $removed 个历史版本目录 $(if($DryRun){'(dry-run, 未实际删除)'})" -ForegroundColor Green }
}

function Diff-Cloud {
  Require-Rclone
  $conf = Get-SyncConf
  $target = Get-EffectiveRemote $conf
  Write-Host ">>> 差异检查 $Local vs $target" -ForegroundColor Cyan
  & rclone.exe check $Local $target --compare size,modtime,checksum --one-way --exclude "*.tmp" --exclude "*.log" --exclude ".git"
  Write-Host ">>> 差异检查结束" -ForegroundColor Green
}

function Restore-Cloud {
  param([string]$File)
  Require-Rclone
  $conf = Get-SyncConf
  $target = Get-EffectiveRemote $conf
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  if ($File) {
    if (-not (Test-Path $File)) { throw "恢复源不存在: $File" }
    Write-Host ">>> 恢复本地压缩包 $File -> $Local" -ForegroundColor Cyan
    Write-Host "    请先确认目标盘有足够空间。实际恢复命令示例:" -ForegroundColor Yellow
    Write-Host "    Expand-Archive $File -DestinationPath $Local -Force" -ForegroundColor Yellow
    return
  }
  Write-Host ">>> 从 $target 恢复到 $Local (U盘丢失后新盘重建, 14.3)" -ForegroundColor Cyan
  $rArgs = @("copy", $target, $Local, "--transfers", "$Transfers", "--bwlimit", "${BwLimit}M")
  foreach ($x in (Parse-Excludes $CloudExcludes)) { $rArgs += $x }
  & rclone.exe @rArgs
  if ($LASTEXITCODE -ne 0) { throw "rclone copy 失败" }
  Write-Host ">>> 恢复完成, 请重挂 VHDX/重启系统验证。" -ForegroundColor Green
}

function Backup-UserVhdx {
  # 14.3: User.vhdx 每日 Checkpoint -> Data\Backup\User-YYYYMMDD.vhdx, 保留 KeepBackups 份
  if (-not (Test-Path $BackupSource)) {
    Write-Warning "备份源不存在: $BackupSource (未找到 User.vhdx)"
    exit 1
  }
  $backupDir = Join-Path $Local "Backup"
  New-Item -ItemType Directory -Force -Path $backupDir | Out-Null
  $stamp = Get-Date -Format "yyyyMMdd"
  $dst = Join-Path $backupDir "User-$stamp.vhdx"
  Write-Host ">>> 备份 $BackupSource -> $dst" -ForegroundColor Cyan
  Copy-Item $BackupSource $dst -Force
  # 同名当日重备份 -> 记录大小与哈希, 供恢复前校验
  $hash = (Get-FileHash $dst -Algorithm SHA256).Hash
  "{0},{1},{2}" -f (Get-Date -Format o), (Split-Path $dst -Leaf), $hash |
    Add-Content (Join-Path $backupDir "backup-log.csv")
  # 按文件名日期保留最新 KeepBackups 份
  $old = @(Get-ChildItem $backupDir -Filter "User-*.vhdx" | Sort-Object Name -Descending |
    Select-Object -Skip $KeepBackups)
  foreach ($f in $old) {
    Write-Host "    清理旧备份 $($f.Name) (保留最新 $KeepBackups 份)" -ForegroundColor Yellow
    Remove-Item $f.FullName -Force
  }
  Write-Host ">>> 备份完成: $dst  SHA256=$($hash.Substring(0,12))...  现存 $(@(Get-ChildItem $backupDir -Filter 'User-*.vhdx').Count) 份" -ForegroundColor Green
}

function Setup-Schedule {
  if (-not (Get-Command schtasks.exe -ErrorAction SilentlyContinue)) { throw "schtasks 不可用" }
  $taskName = "VariableOS-CloudSync"
  $script = Join-Path $ConfigDir "Cloud-Sync.ps1"
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  if (-not (Test-Path $script)) {
    Copy-Item $PSCommandPath $script -Force
  }
  $actionCmd = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$script`" -Action Sync -Local `"$Local`" -Remote `"$Remote`""
  Write-Host ">>> 注册计划任务 $taskName (每 $ScheduleTime 分钟)" -ForegroundColor Cyan
  & schtasks.exe /Create /TN $taskName /TR $actionCmd /SC MINUTE /MO $ScheduleTime /F
  if ($LASTEXITCODE -ne 0) { Write-Warning "计划任务注册失败(可能需要管理员)" }
  else { Write-Host "    已注册: $taskName, 每 $ScheduleTime 分钟云同步。" -ForegroundColor Green }
}

function Show-Status {
  Require-Rclone
  $conf = Get-SyncConf
  $target = Get-EffectiveRemote $conf
  Write-Host "===== 云同步状态 =====" -ForegroundColor Cyan
  Write-Host "本地目录:   $Local"
  Write-Host "底层 remote: $($conf.Remote)"
  Write-Host "加密层:     $(if($conf.Crypt){'已启用 (' + $CryptRemoteName + ' crypt, 文件名+内容加密)'}else{'未启用 (可 -Action Setup -Crypt 开启)'})"
  Write-Host "有效目标:   $target"
  Write-Host "版本策略:   变更/删除入 _archive 保留 $RetentionDays 天; User.vhdx 本地备份保留 $KeepBackups 份"
  & rclone.exe lsd $target --max-depth 1 2>&1 | Select-Object -First 20
  $task = schtasks.exe /Query /TN VariableOS-CloudSync 2>$null
  if ($task) { Write-Host "计划任务: 已注册 VariableOS-CloudSync" -ForegroundColor Green } else { Write-Host "计划任务未注册(可运行 -Action Schedule)" -ForegroundColor Yellow }
  if (Test-Path $Local) { (Get-ChildItem $Local -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum / 1GB | ForEach-Object { Write-Host "本地大小: $([math]::Round($_,2)) GB" } }
  $bk = Join-Path $Local "Backup"
  if (Test-Path $bk) {
    Write-Host "本地 User.vhdx 备份:" -ForegroundColor Cyan
    Get-ChildItem $bk -Filter "User-*.vhdx" | Sort-Object Name -Descending | Select-Object -First $KeepBackups Name, @{n="GB";e={[math]::Round($_.Length/1GB,2)}}, LastWriteTime | Format-Table -AutoSize
  }
}

switch ($Action) {
  "Setup"    { Setup-Cloud }
  "Sync"     { Sync-Cloud }
  "Restore"  { Restore-Cloud $RemoteFile }
  "Schedule" { Setup-Schedule }
  "Diff"     { Diff-Cloud }
  "Prune"    { Prune-Cloud }
  "Backup"   { Backup-UserVhdx }
  "Status"   { Show-Status }
}
