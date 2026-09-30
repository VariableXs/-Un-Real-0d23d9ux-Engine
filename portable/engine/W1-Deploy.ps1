<#
.SYNOPSIS
  VARIX 三体系统 · AI-2 W1 部署执行脚本（路线三：官方 25H2 干净装到 WIN_ENGINE）。

.DESCRIPTION
  等价于 Hasleo WTG 的内核步骤（dism Apply-Image），全程带落点实证闸门：
    闸门（全过才动盘，否则退出零写入）：
      G1 卷标签=WIN_ENGINE 且 BusType=USB
      G2 容量 300±30GB
      G3 空置（已用 < 5GB）
      G4 ISO 在场且含 sources\install.wim
    步骤：G 闸门 → 枚举 WIM 选版 → format X(按标签解析) NTFS(保留卷标签)
          → dism /Apply-Image → 完工自检 → ready 标记。
    明确不做：不碰 ESP（S1.3 辖区）、不碰内置盘、不重启。
  日志：D:\VarixDeploy\w1-deploy.log（UTF-8；结束行 W1-DEPLOY-DONE / W1-DEPLOY-FAIL）。

.EXAMPLE
  .\W1-Deploy.ps1 -IsoPath D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso -Edition Pro
#>
[CmdletBinding()]
param(
  [string]$IsoPath = 'D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso',
  [string]$Edition = '专业版',
  [string]$LogPath = 'D:\VarixDeploy\w1-deploy.log'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Log {
  param([string]$Msg)
  $line = ("[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $Msg)
  Write-Host $line
  Add-Content -LiteralPath $LogPath -Value $line -Encoding UTF8
}

function Finish {
  param([string]$Tag, [string]$Msg)
  Log ("W1-DEPLOY-" + $Tag + " | " + $Msg)
  if ($Tag -eq 'DONE') { exit 0 } else { exit 4 }
}

try { New-Item -ItemType Directory -Path (Split-Path $LogPath) -Force | Out-Null } catch {}
Log ("W1 部署开始 pid=" + $pid + " elevated=True edition=" + $Edition)
$script:IsoMounted = $false

# ---- 主体（全局异常捕获：任何未预期终止都落日志）----
try {

# ---- G1-G3 落点实证（按标签定位，绝不猜盘符）----
$vol = Get-Volume -FileSystemLabel 'WIN_ENGINE' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $vol) { Finish 'FAIL' 'G1 FAIL: 未找到 WIN_ENGINE 卷' }
$L = "$($vol.DriveLetter):"
Log ("G1 PASS 卷=" + $L + " label=WIN_ENGINE")
$part = Get-Partition -DriveLetter $vol.DriveLetter -ErrorAction SilentlyContinue
$disk = if ($part) { Get-Disk -Number $part.DiskNumber -ErrorAction SilentlyContinue } else { $null }
if (-not $disk -or $disk.BusType -ne 'USB') { Finish 'FAIL' "G1 FAIL: 非 USB 盘（BusType=$($disk.BusType)）" }
Log ("G1 PASS BusType=USB disk=" + $disk.FriendlyName)
# 容量/空置口径统一走 Win32_LogicalDisk（本机 Get-Volume 的 Size/FreeSpace 属性不可靠——
# 严格模式下曾实测 FreeSpace 属性缺失直接抛异常）。
$ld = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='$L'"
$sizeGB = [math]::Round($ld.Size / 1GB, 1)
if ([math]::Abs($sizeGB - 300) -gt 30) { Finish 'FAIL' "G2 FAIL: 容量 $sizeGB GB 偏离 300±30" }
Log ("G2 PASS 容量=" + $sizeGB + "GB")
Log "G3a 进入空置检查"
$usedGB = [math]::Round(($ld.Size - $ld.FreeSpace) / 1GB, 2)
Log ("G3b usedGB=" + $usedGB)
if ($usedGB -gt 5) { Finish 'FAIL' "G3 FAIL: 非空置（已用 $usedGB GB）" }
Log ("G3 PASS 空置（已用 $usedGB GB）")

# ---- G4 ISO 实证 ----
if (-not (Test-Path -LiteralPath $IsoPath)) { Finish 'FAIL' "G4 FAIL: ISO 不存在 $IsoPath" }
Log ("G4 PASS ISO=" + $IsoPath)

# ---- 选版：枚举 WIM 索引（临时提权窗口内 dism 可用）----
$isoMount = Mount-DiskImage -ImagePath $IsoPath -PassThru
$script:IsoMounted = $true
try {
  $isoL = ($isoMount | Get-Volume).DriveLetter
  $wim = "$($isoL):\sources\install.wim"
  if (-not (Test-Path -LiteralPath $wim)) { Finish 'FAIL' "ISO 内无 sources\install.wim" }
  Log "枚举 WIM 索引..."
  $raw = (& dism /Get-WimInfo /wimFile:$wim | Out-String)
  Add-Content -LiteralPath $LogPath -Value $raw -Encoding UTF8
  $idx = 0; $cur = 0; $curName = ''
  foreach ($line in ($raw -split "`r?`n")) {
    if ($line -match '^索引\s*:\s*(\d+)') { $cur = [int]$Matches[1] }
    if ($line -match '^名称\s*:\s*(.+)$') {
      $curName = $Matches[1].Trim()
      if ($curName -like "*$Edition*") { $idx = $cur }
    }
  }
  if ($idx -le 0) { Finish 'FAIL' "未找到名称含 '$Edition' 的镜像索引" }
  Log ("选定 index=" + $idx + " name=" + $curName)

  # ---- 格式化（保留卷标签 WIN_ENGINE）----
  Log ("格式化 $L (NTFS 快速, 卷标签保持 WIN_ENGINE)...")
  & format.com "$L" /FS:NTFS /Q /V:WIN_ENGINE /Y | Out-Null
  if ($LASTEXITCODE -ne 0) { Finish 'FAIL' "format 失败 exit=$LASTEXITCODE" }
  $re = Get-Volume -FileSystemLabel 'WIN_ENGINE' -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $re -or "$($re.DriveLetter):" -ne $L) { Finish 'FAIL' '格式化后卷标签/盘符复核失败' }
  Log "格式化完成，卷标签复核 PASS"

  # ---- 应用镜像（重活 20-40 分钟）----
  Log "dism /Apply-Image 开始（约 20-40 分钟，勿动此窗口）..."
  & dism /Apply-Image /ImageFile:$wim /Index:$idx /ApplyDir:"$L\" | Out-Null
  if ($LASTEXITCODE -ne 0) { Finish 'FAIL' "dism Apply-Image 失败 exit=$LASTEXITCODE" }
  Log "dism /Apply-Image 完成"

  # ---- 完工自检 ----
  if (-not (Test-Path -LiteralPath "$L\Windows\System32")) { Finish 'FAIL' "自检 FAIL: $L\Windows\System32 缺失" }
  $ld2 = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='$L'"
  $usedAfter = [math]::Round(($ld2.Size - $ld2.FreeSpace) / 1GB, 1)
  Log ("自检 PASS Windows 目录在位；已用 $usedAfter GB")
  $marker = @{
    deployedAt = (Get-Date -Format 'o')
    edition    = $curName
    index      = $idx
    isoName    = (Split-Path $IsoPath -Leaf)
    route      = 'W1-route3-clean'
    espTouched = $false
  } | ConvertTo-Json
  [IO.File]::WriteAllText("$L\varix-w1-ready.txt", $marker, [Text.UTF8Encoding]::new($false))
  Log "ready 标记已写 $L\varix-w1-ready.txt"
  Log "ESP 未触碰（S1.3 辖区：引导接线由 AI-1 S1.3 / 协调会话执行）"
  Finish 'DONE' "部署完成：$L Windows 就绪（edition=$curName index=$idx）"
}
finally {
  if ($script:IsoMounted) { Dismount-DiskImage -ImagePath $IsoPath -ErrorAction SilentlyContinue | Out-Null }
}
}
catch {
  Log ("EXC|" + $_.Exception.GetType().FullName + "|" + $_.Exception.Message)
  if ($_.InvocationInfo) { Log ("AT|" + $_.InvocationInfo.PositionMessage) }
  Finish 'FAIL' "未预期异常（见 EXC 行）"
}
