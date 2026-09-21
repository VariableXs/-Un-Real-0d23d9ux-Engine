<#
.SYNOPSIS
  VARIX 三体系统 · AI-2 W2 工具链 —— Base 母本制作（官方镜像装进 Base.vhdx，一次成型后只读封存）。

.DESCRIPTION
  对应《方案详解-原理与步骤》4.2 步骤 2 / S3.1 配套。产出 = 可引导的 Gen2 VM 系统盘：
    Base.vhdx (GPT) = ESP(260MB FAT32) + MSR(16MB) + Windows(NTFS，官方 install.wim 应用)
  链关系：Base(只读封存) <- Apps <- User（由 Engine-Chain.ps1 建立；VM 只挂 User 层启动）。
  幂等：Base 已存在时拒绝覆盖（-Force 才重建）；完成后只读封存。
  后端：Real（管理员：diskpart+dism+bcdboot）/ Mock（零管理员，普通文件模拟，逻辑同源）。

.EXAMPLE
  .\Base-Mother.ps1 -Action Plan  -IsoPath D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso
  .\Base-Mother.ps1 -Action Build -IsoPath D:\VarixDeploy\...iso -Index 6
  .\Base-Mother.ps1 -Action Verify
  .\Base-Mother.ps1 -Action Build -Backend Mock -OutDir D:\tmp\engine-chain -Index 6

.NOTES
  编码纪律：UTF-8 BOM；PS 5.1 兼容；盘上只写 WIN_ENGINE 的 Engine 目录（真后端），
  Mock 一律 -OutDir 隔离绝不写真卷。挂载 ISO 与 dism 应用镜像需管理员。
#>
[CmdletBinding()]
param(
  [ValidateSet('Plan', 'Build', 'Verify')]
  [string]$Action = 'Plan',
  [string]$IsoPath = '',
  [int]$Index = 0,
  [ValidateSet('Real', 'Mock')]
  [string]$Backend = 'Real',
  [string]$ConfigPath = '',
  [string]$OutDir = '',
  [switch]$Force
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- 配置与落点

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $ConfigPath) { $ConfigPath = Join-Path $PSScriptRoot 'engine-chain.json' }
$Cfg = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json
if ($Cfg.version -ne 1) { throw "engine-chain.json version 必须=1" }

function Find-EngineVolume {
  param([string]$Label)
  $v = Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($v -and $v.DriveLetter) { return ("$($v.DriveLetter):") }
  return $null
}

function Resolve-BasePath {
  if ($Backend -eq 'Mock') {
    if (-not $OutDir) { throw "Mock 后端必须给 -OutDir（模拟文件绝不写真卷）" }
    if (-not (Test-Path -LiteralPath $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
    return (Join-Path $OutDir $Cfg.base.file)
  }
  $vol = Find-EngineVolume -Label $Cfg.volumeLabel
  if (-not $vol) { throw "未找到 $($Cfg.volumeLabel) 卷（三重确认纪律：绝不猜盘符）" }
  return (Join-Path (Join-Path $vol $Cfg.engineDir) $Cfg.base.file)
}

function Test-Admin {
  $p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
  return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-FreeLetters {
  $used = @(Get-Volume | ForEach-Object { "$($_.DriveLetter)" })
  return 'YZWVUTSRQPONMLKJIHGFED'.ToCharArray() | Where-Object { $used -notcontains [string]$_ } | Select-Object -First 4
}

function Get-WimInfoSafe {
  param([string]$Iso, [int]$Idx)
  $mounted = Mount-DiskImage -ImagePath $Iso -PassThru
  $drive = ($mounted | Get-Volume).DriveLetter
  $wim = "$($drive):\sources\install.wim"
  if (-not (Test-Path -LiteralPath $wim)) { throw "ISO 内未找到 sources\install.wim" }
  if ($Idx -gt 0) {
    $info = & dism /Get-WimInfo /wimFile:$wim /Index:$Idx | Out-String
    Dismount-DiskImage -ImagePath $Iso | Out-Null
    return $info
  }
  $info = & dism /Get-WimInfo /wimFile:$wim | Out-String
  Dismount-DiskImage -ImagePath $Iso | Out-Null
  return $info
}

# ---------------------------------------------------------------- 动作

$base = Resolve-BasePath

if ($Action -eq 'Plan') {
  Write-Host ">>> Base 母本 Plan（后端=$Backend）" -ForegroundColor Cyan
  Write-Host ("  目标: {0}  sizeGB={1} type={2}" -f $base, $Cfg.base.sizeGB, $Cfg.base.type)
  Write-Host ("  已存在: {0}" -f (Test-Path -LiteralPath $base))
  Write-Host ("  ISO 参数: {0}" -f $(if ($IsoPath) { $IsoPath } else { '<未给>' }))
  if ($IsoPath -and (Test-Path -LiteralPath $IsoPath)) {
    $isoSize = [math]::Round((Get-Item -LiteralPath $IsoPath).Length / 1GB, 2)
    Write-Host ("  ISO 大小: {0} GB" -f $isoSize)
  }
  Write-Host ("  管理员: {0}（Real 的 Build 需要）" -f (Test-Admin))
  if ($IsoPath -and (Test-Path -LiteralPath $IsoPath) -and (Test-Admin)) {
    Write-Host "  WIM 镜像索引：" -ForegroundColor Cyan
    Get-WimInfoSafe -Iso $IsoPath -Idx 0 | Write-Host
  } else {
    Write-Host "  （WIM 索引枚举需 ISO 在场+管理员；Plan 不挂载）"
  }
  exit 0
}

if ($Backend -eq 'Mock' -and -not $OutDir) { throw "Mock 后端必须给 -OutDir" }
if ($Backend -eq 'Real' -and -not (Test-Admin)) { throw "Build/Verify(Real) 需要管理员权限" }
if ($Backend -eq 'Real' -and (-not $IsoPath -or -not (Test-Path -LiteralPath $IsoPath))) { throw "ISO 路径无效：$IsoPath" }

if ($Action -eq 'Build') {
  if ((Test-Path -LiteralPath $base) -and -not $Force) {
    throw "Base 已存在（幂等保护）：$base —— 重建需显式 -Force"
  }
  Write-Host ">>> Base 母本 Build（后端=$Backend index=$Index）" -ForegroundColor Cyan
  if ($Backend -eq 'Mock') {
    # Mock：普通文件 + 标记内容（与 Engine-Chain Mock 同源模拟法）。
    [IO.File]::WriteAllText($base, "mother|base|index=$Index|sizeGB=$($Cfg.base.sizeGB)", [Text.UTF8Encoding]::new($false))
    (Get-Item -LiteralPath $base -Force).IsReadOnly = $true
    Write-Host ">>> [Mock] Base 母本成型并只读封存：$base" -ForegroundColor Green
    exit 0
  }
  # ---- Real：diskpart 建 GPT 三分区 → dism 应用镜像 → bcdboot → 卸盘封存 ----
  $iso = Resolve-Path -LiteralPath $IsoPath
  $mounted = Mount-DiskImage -ImagePath $iso -PassThru
  try {
    $isoDrive = ($mounted | Get-Volume).DriveLetter
    $wim = "$($isoDrive):\sources\install.wim"
    if (-not (Test-Path -LiteralPath $wim)) { throw "ISO 内未找到 sources\install.wim" }
    if ($Index -le 0) { throw "必须显式 -Index（先跑 Plan 查索引）" }

    $letters = Get-FreeLetters
    if ($letters.Count -lt 2) { throw "可用盘符不足（ESP+Windows 分区各需一个）" }
    $espL = $letters[0]; $winL = $letters[1]

    $dp = Join-Path $env:TEMP ("bm-dp-" + [Guid]::NewGuid().ToString('N') + ".txt")
    $type = if ($Cfg.base.type -eq 'fixed') { 'FIXED' } else { 'EXPANDABLE' }
    $script = @"
create vdisk file="$base" maximum=$($Cfg.base.sizeGB * 1024) type=$type
select vdisk file="$base"
attach vdisk
convert gpt
create partition efi size=260
format quick fs=fat32 label=VBASE_ESP
assign letter=$espL
create partition msr size=16
create partition primary
format quick fs=ntfs label=VARIX_BASE
assign letter=$winL
"@
    [IO.File]::WriteAllText($dp, ($script -replace "`n", "`r`n"))
    Write-Host "  [1/5] diskpart：建盘+挂载+三分区..." -ForegroundColor Cyan
    & diskpart /s $dp | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "diskpart 失败（exit=$LASTEXITCODE）" }
    Remove-Item -LiteralPath $dp -Force

    Write-Host "  [2/5] dism 应用镜像（index=$Index，约 10-25 分钟）..." -ForegroundColor Cyan
    & dism /Apply-Image /ImageFile:$wim /Index:$Index /ApplyDir:$($winL):\ | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "dism Apply-Image 失败（exit=$LASTEXITCODE）" }

    Write-Host "  [3/5] bcdboot 引导写入..." -ForegroundColor Cyan
    & bcdboot "$($winL):\Windows" /s "$($espL):" /f UEFI | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "bcdboot 失败（exit=$LASTEXITCODE）" }

    Write-Host "  [4/5] 卸载差分母盘..." -ForegroundColor Cyan
    $dp2 = Join-Path $env:TEMP ("bm-dp2-" + [Guid]::NewGuid().ToString('N') + ".txt")
    [IO.File]::WriteAllText($dp2, "select vdisk file=`"$base`"`r`ndetach vdisk`r`n")
    & diskpart /s $dp2 | Out-Null
    Remove-Item -LiteralPath $dp2 -Force
  }
  finally {
    Dismount-DiskImage -ImagePath $iso | Out-Null
  }
  Write-Host "  [5/5] 只读封存 Base..." -ForegroundColor Cyan
  Set-ItemProperty -LiteralPath $base -Name IsReadOnly -Value $true
  Write-Host ">>> Base 母本成型：$base（GPT+ESP+Windows，只读封存）" -ForegroundColor Green
  Write-Host "    下一步：Engine-Chain.ps1 -Action New 建差分链（Apps/User 将差分于此）" -ForegroundColor Green
}
elseif ($Action -eq 'Verify') {
  Write-Host ">>> Base 母本 Verify" -ForegroundColor Cyan
  $fail = 0
  $ok = Test-Path -LiteralPath $base
  if (-not $ok) { $fail += 1 }
  Write-Host ("  {0} 存在: {1}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $base) -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
  if ($ok) {
    $sizeGB = [math]::Round((Get-Item -LiteralPath $base).Length / 1GB, 1)
    $ro = (Get-Item -LiteralPath $base -Force).IsReadOnly
    Write-Host ("  {0} 只读封存: {1}" -f $(if ($ro) { 'PASS' } else { 'FAIL' }), $ro) -ForegroundColor $(if ($ro) { 'Green' } else { 'Red' })
    if (-not $ro) { $fail += 1 }
    if ($Backend -eq 'Mock') {
      $marker = Get-Content -LiteralPath $base -Raw
      $mok = $marker -like 'mother|base|*'
      Write-Host ("  {0} 母本标记: {1}" -f $(if ($mok) { 'PASS' } else { 'FAIL' }), ($marker -replace '\r?\n', '')) -ForegroundColor $(if ($mok) { 'Green' } else { 'Red' })
      if (-not $mok) { $fail += 1 }
    } else {
      Write-Host ("  INFO 体积: {0} GB（Real 模式引导验证移交 VM 首启）" -f $sizeGB)
    }
  }
  if ($fail -gt 0) { exit 1 }
  Write-Host ">>> Verify PASS" -ForegroundColor Green
}
