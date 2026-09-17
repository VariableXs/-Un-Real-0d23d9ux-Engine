<#
.SYNOPSIS
  VARIX 双域系统 · AI-P 任务 7 —— ESP 组装：VARIX 引导器与 Windows 引导文件双链共存

.DESCRIPTION
  对应 docs/VARIX双域系统施工总案.md 阶段 1 步骤 2：
    VARIX 链：/EFI/BOOT/BOOTX64.EFI(Limine UEFI) + /limine.conf + /kernel/varix (+ /initrd.img 可选)
    Windows 链：/EFI/Microsoft/**（来自 -WindowsBootDir 源目录；含 bootmgfw.efi/BCD）
    - 双链目录互不重叠（总案：两条链互不干扰），各自文件清单与 SHA-256 归档 ESP 根 ESP-MANIFEST.json
    - 脚本独立可重入（幂等）：每次全量重刷本链文件，不动对方链与清单外文件
    - -EspPath 可指向已挂载 ESP 卷（如 S:\）或普通目录（测试/预组装通用，同一代码路径）
    - -VerifyOnly 只按清单校验双链完整性与哈希
  VM 首启验证：产出后用 ..\Test-VM.ps1 挂整盘 VHD 走查（需 Hyper-V 实机，见 README 安全边界）。

.EXAMPLE
  .\Build-ESP.ps1 -EspPath S:\                       # 只组装 VARIX 链
  .\Build-ESP.ps1 -EspPath S:\ -WindowsBootDir D:\esp-win-src
  .\Build-ESP.ps1 -EspPath S:\ -VerifyOnly
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$EspPath,

  # VARIX 链源：内核 ELF / Limine 目录 / limine.conf / initrd（均默认仓库路径，可覆盖）
  [string]$KernelElf = "",
  [string]$LimineDir = "",
  [string]$LimineConf = "",
  [string]$Initrd = "",

  # Windows 链源目录（内含 EFI\Microsoft 结构或散装 bootmgfw.efi/BCD）；缺省只组装 VARIX 链
  [string]$WindowsBootDir = "",

  [switch]$VerifyOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step { param([string]$Msg) Write-Host ">>> $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "    + $Msg" -ForegroundColor Green }
function Write-Note { param([string]$Msg) Write-Host "    ! $Msg" -ForegroundColor Yellow }

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $KernelElf)  { $KernelElf  = Join-Path $repo 'kernel\target\x86_64-unknown-none\release\varix' }
if (-not $LimineDir)  { $LimineDir  = Join-Path $repo 'tools\limine\limine-binary' }
if (-not $LimineConf) { $LimineConf = Join-Path $repo 'limine.conf' }
if (-not $Initrd)     { $Initrd     = Join-Path $repo 'build\initrd.img' }

$espRoot = $EspPath.TrimEnd('\') + '\'
if (-not (Test-Path -LiteralPath $espRoot -PathType Container)) { throw "ESP 目标不存在：$espRoot" }
$manifestPath = Join-Path $espRoot 'ESP-MANIFEST.json'

# 双链布局（目录互斥；清单按链归属记录）
$varixFiles = @(
  @{ Src = (Join-Path $LimineDir 'BOOTX64.EFI');  Dst = 'EFI\BOOT\BOOTX64.EFI' },
  @{ Src = (Join-Path $LimineDir 'limine-bios.sys'); Dst = 'limine-bios.sys' },
  @{ Src = $LimineConf;                           Dst = 'limine.conf' },
  @{ Src = $KernelElf;                            Dst = 'kernel\varix' }
)
if (Test-Path -LiteralPath $Initrd) { $varixFiles += @{ Src = $Initrd; Dst = 'initrd.img' } }
$winPrefix = 'EFI\Microsoft'

function Get-Entry {
  param([string]$Root, [string[]]$RelFiles)
  $list = @()
  foreach ($rel in $RelFiles) {
    $p = Join-Path $Root $rel
    if (-not (Test-Path -LiteralPath $p)) { return $null }
    $list += [pscustomobject]@{ file = $rel; sha256 = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash }
  }
  return $list
}

if ($VerifyOnly) {
  Write-Step "按清单校验双链：$espRoot"
  if (-not (Test-Path -LiteralPath $manifestPath)) { throw "清单缺失：$manifestPath" }
  $mf = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
  $fail = 0
  foreach ($chain in @('varix', 'windows')) {
    $entries = $mf.PSObject.Properties[$chain].Value
    if ($null -eq $entries) {
      if ($chain -eq 'windows') { Write-Note 'windows 链未部署（清单无记录），跳过'; continue }
      Write-Note 'varix 链清单缺失'; $fail++; continue
    }
    foreach ($e in @($entries)) {
      $p = Join-Path $espRoot $e.file
      if (-not (Test-Path -LiteralPath $p)) { Write-Note "缺失 $($e.file)"; $fail++; continue }
      $h = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash
      if ($h -ne $e.sha256) { Write-Note "哈希不一致 $($e.file)"; $fail++ }
    }
    Write-Ok "$chain 链 @($($entries)).Count 个文件校验完成"
  }
  if ($fail -gt 0) { throw "双链校验失败：$fail 处" }
  Write-Ok '双链校验全部通过（VARIX/Windows 互不干扰）'
  exit 0
}

# ------------------------------------------------------------- 组装
Write-Step "组装 ESP：$espRoot"
foreach ($f in $varixFiles) {
  if (-not (Test-Path -LiteralPath $f.Src)) { throw "VARIX 链源缺失：$($f.Src)" }
  $dst = Join-Path $espRoot $f.Dst
  New-Item -ItemType Directory -Force -Path (Split-Path $dst -Parent) | Out-Null
  Copy-Item -LiteralPath $f.Src -Destination $dst -Force
  Write-Ok "VARIX 链 $($f.Dst)"
}

$winEntries = $null
if ($WindowsBootDir) {
  if (-not (Test-Path -LiteralPath $WindowsBootDir -PathType Container)) { throw "Windows 引导源不存在：$WindowsBootDir" }
  $srcEfi = Join-Path $WindowsBootDir 'EFI\Microsoft'
  if (Test-Path -LiteralPath $srcEfi) { $srcBase = $srcEfi } else { $srcBase = $WindowsBootDir }
  # 只拷引导必需物：bootmgfw.efi + BCD（避免把整源目录无差别灌进 ESP）
  $must = @('bootmgfw.efi', 'BCD')
  $winFiles = @()
  foreach ($m in $must) {
    $hit = Get-ChildItem -LiteralPath $srcBase -Recurse -File -Filter $m -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($hit) { $winFiles += $hit.FullName } else { throw "Windows 引导源缺 $m：$srcBase" }
  }
  $winRel = @()
  foreach ($src in $winFiles) {
    $dst = Join-Path $espRoot "$winPrefix\Boot\$([IO.Path]::GetFileName($src))"
    New-Item -ItemType Directory -Force -Path (Split-Path $dst -Parent) | Out-Null
    Copy-Item -LiteralPath $src -Destination $dst -Force
    $winRel += "$winPrefix\Boot\$([IO.Path]::GetFileName($src))"
    Write-Ok "Windows 链 $winPrefix\Boot\$([IO.Path]::GetFileName($src))"
  }
  $winEntries = Get-Entry $espRoot $winRel
}

# 清单：双链文件 + SHA-256（换盘核验/验收归档双用）
$varixEntries = Get-Entry $espRoot @($varixFiles | ForEach-Object { $_.Dst })
$mf = [pscustomobject]@{
  version  = 1
  built    = (Get-Date -Format 'o')
  varix    = $varixEntries
  windows  = $winEntries
}
[IO.File]::WriteAllText($manifestPath, ($mf | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
Write-Ok "清单已写 ESP-MANIFEST.json（varix=@($varixEntries).Count$(if ($winEntries) { '，windows=' + @($winEntries).Count })）"

# 组装后自检：直接走 VerifyOnly 逻辑
& $PSCommandPath -EspPath $espRoot -VerifyOnly
