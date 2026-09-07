# AI-5 交付核 / 公共库 —— 被同目录脚本 dot-source 使用
#   对应主计划：第11章 测试与验收 + 第12章 交付与运维 + 扩充 19/21/22/28
# 设计纪律：
#   1. 只读动作(List/Plan/Report/Status)任何环境都能跑，绝不写盘；
#   2. 写盘/破坏性动作必须显式开关 + 二次确认，且拒绝在宿主系统盘上执行；
#   3. 环境缺能力(无 Hyper-V / 无 manage-bde / 非 Windows)一律降级提示，不静默失败。

function Write-Ai5 {
  param([string]$Message, [string]$Level = "Info")
  $color = "Gray"
  $tag = "   "
  switch ($Level) {
    "Info"  { $color = "Gray";   $tag = "   " }
    "Ok"    { $color = "Green";  $tag = "OK " }
    "Warn"  { $color = "Yellow"; $tag = "!  " }
    "Err"   { $color = "Red";    $tag = "X  " }
    "Step"  { $color = "Cyan";   $tag = ">> " }
  }
  Write-Host "$tag$Message" -ForegroundColor $color
}

function Test-Ai5Admin {
  try {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $pr = New-Object Security.Principal.WindowsPrincipal($id)
    return $pr.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  } catch { return $false }
}

function Test-Ai5Command {
  param([string]$Name)
  return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Get-Ai5DataRoot {
  param([string]$DataDrive = "D:")
  $d = $DataDrive.TrimEnd('\')
  return (Join-Path $d "Data")
}

function Get-Ai5EvidenceRoot {
  param([string]$DataDrive = "D:")
  return (Join-Path (Get-Ai5DataRoot -DataDrive $DataDrive) "Tests")
}

function New-Ai5Directory {
  param([Parameter(Mandatory = $true)][string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) {
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
  }
  return $Path
}

function Get-Ai5Json {
  param([Parameter(Mandatory = $true)][string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { throw "缺少数据文件: $Path" }
  $raw = Get-Content -LiteralPath $Path -Raw -Encoding UTF8
  return ($raw | ConvertFrom-Json)
}

function Save-Ai5Json {
  param([Parameter(Mandatory = $true)]$Object, [Parameter(Mandatory = $true)][string]$Path)
  New-Ai5Directory -Path (Split-Path -Parent $Path) | Out-Null
  # ConvertTo-Json 在 5.1 默认深度 2，必须显式给深度，否则嵌套结构会被截成类型名
  $json = $Object | ConvertTo-Json -Depth 12
  $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $json, $utf8NoBom)
  return $Path
}

function Save-Ai5Text {
  # [AllowEmptyString()] 是必需的：Mandatory 参数会逐个校验数组元素非空，
  # 而 Markdown 报告本来就含空行（$L += ""），不加这个特性会绑定失败。
  param([Parameter(Mandatory = $true)][AllowEmptyString()][string[]]$Lines, [Parameter(Mandatory = $true)][string]$Path)
  New-Ai5Directory -Path (Split-Path -Parent $Path) | Out-Null
  $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllLines($Path, [string[]]$Lines, $utf8NoBom)
  return $Path
}

function Get-Ai5Timestamp { return (Get-Date -Format "yyyyMMdd-HHmmss") }

function Get-Ai5VerdictIcon {
  param([string]$Status)
  switch ($Status) {
    "pass" { return "✅" }
    "warn" { return "⚠️" }
    "fail" { return "❌" }
    "skip" { return "⏭" }
    "todo" { return "⬜" }
    default { return "·" }
  }
}

# 破坏性动作的统一闸门：CI/无人值守环境下默认拒绝
function Confirm-Ai5Dangerous {
  param([Parameter(Mandatory = $true)][string]$What, [switch]$Yes)
  if ($Yes) { return $true }
  if ($env:AI5_NONINTERACTIVE -eq "1") {
    Write-Ai5 "非交互环境(AI5_NONINTERACTIVE=1)拒绝执行破坏性动作: $What" "Warn"
    return $false
  }
  if (-not [Environment]::UserInteractive) {
    Write-Ai5 "无交互终端，拒绝执行破坏性动作: $What" "Warn"
    return $false
  }
  $ans = Read-Host "即将执行: $What （输入 YES 继续）"
  return ($ans -eq "YES")
}

# 目标盘安全检查：绝不把宿主系统盘/不可移动盘当 U 盘部署
function Test-Ai5UsbTarget {
  param([Parameter(Mandatory = $true)][string]$Drive)
  $problems = @()
  $letter = ($Drive -replace '[\\\/].*$', '').TrimEnd(':')
  if ($letter.Length -ne 1) { $problems += "盘符格式非法: $Drive" ; return $problems }

  $sysDrive = ""
  try { $sysDrive = ([System.IO.Path]::GetPathRoot($env:SystemRoot)).Substring(0, 1) } catch { $sysDrive = "C" }
  if ($letter.ToUpper() -eq $sysDrive.ToUpper()) { $problems += "$letter : 是宿主系统盘，禁止作为部署目标" }

  try {
    $vol = Get-Volume -DriveLetter $letter -ErrorAction Stop
    if ($vol.FileSystem -and @("exFAT", "NTFS", "FAT32") -notcontains $vol.FileSystem) {
      $problems += "$letter : 文件系统 $($vol.FileSystem) 不在允许列表(exFAT/NTFS/FAT32)"
    }
    if ($null -ne $vol.DriveType -and "$($vol.DriveType)" -eq "Fixed") {
      $problems += "$letter : 是固定磁盘(Fixed)，不像 U 盘；确认无误请加 -AllowFixedTarget"
    }
  } catch {
    $problems += "$letter : 无法读取卷信息($($_.Exception.Message))"
  }
  return $problems
}

function Get-Ai5FreeGB {
  param([Parameter(Mandatory = $true)][string]$Path)
  try {
    $root = [System.IO.Path]::GetPathRoot((Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path)
    $d = New-Object System.IO.DriveInfo($root)
    return [math]::Round($d.AvailableFreeSpace / 1GB, 1)
  } catch { return -1 }
}

# 目录体积（robocopy 之前预估用，不递归统计符号链接目标）
function Get-Ai5FolderGB {
  param([Parameter(Mandatory = $true)][string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return 0 }
  $sum = (Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
    Measure-Object -Property Length -Sum).Sum
  if (-not $sum) { return 0 }
  return [math]::Round($sum / 1GB, 2)
}
