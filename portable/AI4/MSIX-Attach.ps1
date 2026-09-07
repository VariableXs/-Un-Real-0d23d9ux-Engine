param(
  [ValidateSet("Package", "Install", "Mount", "Dismount", "Uninstall", "Status")]
  [string]$Action = "Status",
  [string]$PackagePath = "",          # 源目录或 .msix/.appx 路径
  [string]$OutDir = "D:\Data\MSIX",   # 挂载/存放目录
  [string]$MountDir = "D:\Data\MSIX\Mount",
  [string]$CertPath = "D:\Data\MSIX\Variable-OS.pfx",
  [string]$CertPassword = "",
  [switch]$Force
)
# AI-4 拓展核 / 第8.2章 MSIX App Attach + 第13.5章 MSIX挂载细节 + 第18.2章签名校验
# 用法:
#   .\MSIX-Attach.ps1 -Action Package -PackagePath C:\src\Blender.app -OutDir D:\Data\MSIX
#   .\MSIX-Attach.ps1 -Action Install -PackagePath D:\Data\MSIX\Blender.msix
#   .\MSIX-Attach.ps1 -Action Mount   -PackagePath D:\Data\MSIX\Blender.msix -MountDir D:\Data\MSIX\Mount
#   .\MSIX-Attach.ps1 -Action Dismount -PackagePath D:\Data\MSIX\Blender.msix
#   .\MSIX-Attach.ps1 -Action Uninstall -PackagePath D:\Data\MSIX\Blender.msix
#   .\MSIX-Attach.ps1 -Action Status
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Find-Tool {
  param([string]$Name, [string[]]$FallbackPaths)
  $cmd = Get-Command $Name -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  foreach ($p in $FallbackPaths) { if (Test-Path $p) { return $p } }
  throw "$Name 未找到"
}

function Get-MsixExtension {
  param([string]$Path)
  if ($Path -match "\.msixbundle$") { return "msixbundle" }
  if ($Path -match "\.msix$") { return "msix" }
  if ($Path -match "\.appxbundle$") { return "appxbundle" }
  if ($Path -match "\.appx$") { return "appx" }
  return ""
}

function New-MsixPackage {
  if (-not $PackagePath) { throw "-PackagePath 必须是带 AppxManifest.xml 的源目录" }
  if (-not (Test-Path "$PackagePath\AppxManifest.xml")) { throw "缺少 AppxManifest.xml: $PackagePath" }
  New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
  $msix = Join-Path $OutDir (([IO.Path]::GetFileName($PackagePath.TrimEnd('\')) + ".msix"))
  $makeAppx = Find-Tool "MakeAppx.exe" @("${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.26100.0\x64\MakeAppx.exe",
                                        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.19041.0\x64\MakeAppx.exe",
                                        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\x64\MakeAppx.exe")
  Write-Host ">>> MakeAppx 打包 $PackagePath -> $msix" -ForegroundColor Cyan
  & $makeAppx pack /d $PackagePath /p $msix /o
  if ($LASTEXITCODE -ne 0) { throw "打包失败 (MakeAppx exit $LASTEXITCODE)" }
  Write-Host ">>> 签名 (SignTool + SHA256, 证书可选)" -ForegroundColor Cyan
  if ($CertPath) {
    $signtool = Find-Tool "signtool.exe" @("${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe",
                                           "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.19041.0\x64\signtool.exe",
                                           "${env:ProgramFiles(x86)}\Windows Kits\10\bin\x64\signtool.exe")
    & $signtool sign /v /fd SHA256 /f $CertPath /p $CertPassword /tr http://timestamp.digicert.com /td SHA256 $msix
    if ($LASTEXITCODE -ne 0 -and -not $Force) { Write-Warning "签名失败, 包为未签名(仅开发者模式可装)" }
  } else {
    Write-Host "    未提供证书, 仅生成未签名包 (开发者模式可装)" -ForegroundColor Yellow
  }
  $hash = (Get-FileHash -Path $msix -Algorithm SHA256).Hash
  Write-Host "    SHA256: $hash" -ForegroundColor Green
  Write-Host ">>> 完成: $msix  |  下一步: -Action Install 或 -Action Mount" -ForegroundColor Green
  Write-Host "    校验: .\MSIX-Attach.ps1 -Action Status" -ForegroundColor Yellow
}

function Test-MsixSignature {
  param([string]$Path)
  if (-not (Test-Path $Path)) { throw "包不存在: $Path" }
  $sig = Get-AuthenticodeSignature $Path
  if ($sig.Status -eq "Valid") {
    Write-Host ">>> 签名校验通过: $($sig.SignerCertificate.Subject)" -ForegroundColor Green
    return $true
  }
  if ($sig.Status -eq "UnknownError" -and $sig.StatusMessage -match "时间戳") {
    Write-Host ">>> 签名校验(时间戳不可达): $($sig.StatusMessage)" -ForegroundColor Yellow
    return $true # 离线环境时间戳失败, 仅提示
  }
  $hash = (Get-FileHash $Path -Algorithm SHA256).Hash
  Write-Warning ">>> 签名无效($($sig.Status)), 但 SHA256: $hash"
  return $false
}

function Install-MsixApp {
  if (-not $PackagePath) { throw "-PackagePath 必填" }
  if (-not (Test-Path $PackagePath)) { throw "包不存在: $PackagePath" }
  $ext = Get-MsixExtension $PackagePath
  if (-not $ext) { throw "仅支持 .msix/.msixbundle/.appx/.appxbundle" }
  Test-MsixSignature $PackagePath | Out-Null
  Write-Host ">>> 安装 (Add-AppxPackage), 包: $PackagePath" -ForegroundColor Cyan
  try {
    Add-AppxPackage -Path $PackagePath -ErrorAction Stop
    Write-Host "    已注册到当前用户, 开始菜单可搜索" -ForegroundColor Green
  } catch {
    if ($_.Exception.Message -match "0x80073CF3" ) {
      Write-Host "    系统未启用旁加载/开发者模式, 提示: 设置 -> 隐私和安全性 -> 开发者选项 -> 允许旁加载" -ForegroundColor Yellow
    } elseif ($_.Exception.Message -match "0x80070005") {
      Write-Host "    需要管理员权限, 请以管理员 PowerShell 重试" -ForegroundColor Yellow
    } else {
      throw
    }
  }
}

function Mount-MsixVolume {
  if (-not $PackagePath) { throw "-PackagePath 必填" }
  if (-not (Test-Path $PackagePath)) { throw "包不存在: $PackagePath" }
  New-Item -ItemType Directory -Force -Path $MountDir | Out-Null
  $ext = Get-MsixExtension $PackagePath
  if ($ext -notin @("msix", "msixbundle", "appx", "appxbundle")) { throw "仅支持 MSIX/AppX 包" }
  Test-MsixSignature $PackagePath | Out-Null
  Write-Host ">>> 挂载 AppxVolume to $MountDir (不安装、不写C盘)" -ForegroundColor Cyan
  Mount-AppxVolume -PackagePath $PackagePath -VolumePath $MountDir -ErrorAction Stop
  Write-Host "    已挂载, 可在 $MountDir 看到内容, 开始菜单可启动" -ForegroundColor Green
}

function Dismount-MsixVolume {
  if (-not $PackagePath) { throw "-PackagePath 必填" }
  if (-not (Test-Path $PackagePath)) { throw "包不存在: $PackagePath" }
  Write-Host ">>> 卸载 AppxVolume (Dismount-AppxVolume)" -ForegroundColor Cyan
  $vol = Get-AppxVolume | Where-Object { $_.PackageFullName -like "*$([IO.Path]::GetFileNameWithoutExtension($PackagePath))*" } | Select-Object -First 1
  if ($vol) { Dismount-AppxVolume -Volume $vol.PackageFullName | Out-Null }
  else { Get-AppxVolume | Where-Object { $_.PackageFullName -like "*$([IO.Path]::GetFileNameWithoutExtension($PackagePath))*" } | Dismount-AppxVolume -ErrorAction SilentlyContinue }
  Write-Host "    已卸载。删除文件即卸载。" -ForegroundColor Green
}

function Uninstall-MsixApp {
  if (-not $PackagePath) { throw "-PackagePath 必填" }
  $name = [IO.Path]::GetFileNameWithoutExtension($PackagePath)
  Write-Host ">>> 卸载 $name (Remove-AppxPackage + Dismount)" -ForegroundColor Cyan
  Get-AppxPackage -Name $name -AllUsers -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "    移除 $($_.PackageFullName)" -ForegroundColor Yellow
    Remove-AppxPackage -Package $_.PackageFullName -AllUsers -ErrorAction SilentlyContinue
  }
  $vols = Get-AppxVolume -ErrorAction SilentlyContinue | Where-Object { $_.PackageFullName -like "*$name*" }
  if ($vols) { Dismount-AppxVolume -Volume $vols.PackageFullName | Out-Null }
  Write-Host "    完成, 删除 $PackagePath 即彻底清理" -ForegroundColor Green
}

function Show-MsixStatus {
  Write-Host "===== MSIX App Attach 状态 =====" -ForegroundColor Cyan
  Write-Host "-- 已安装 Appx 包(当前用户, 含MSIX) --" -ForegroundColor Cyan
  Get-AppxPackage -ErrorAction SilentlyContinue | Where-Object { $_.IsFramework -eq $false } | Select-Object Name, Version, Architecture, InstallLocation | Format-Table -AutoSize
  Write-Host "-- 已挂载 AppxVolume --" -ForegroundColor Cyan
  $vols = Get-AppxVolume -ErrorAction SilentlyContinue
  if ($vols) { $vols | Select-Object PackageFullName, PackageFamilyName, MountPoint, PackageLocation | Format-Table -AutoSize }
  else { "无挂载卷" }
  Write-Host "-- Data\MSIX 文件 (仅列出最近10个) --" -ForegroundColor Cyan
  if (Test-Path $OutDir) { Get-ChildItem $OutDir -File | Sort-Object LastWriteTime -Descending | Select-Object -First 10 Name, Length, LastWriteTime | Format-Table -AutoSize }
}

switch ($Action) {
  "Package"   { New-MsixPackage }
  "Install"   { Install-MsixApp }
  "Mount"     { Mount-MsixVolume }
  "Dismount"  { Dismount-MsixVolume }
  "Uninstall" { Uninstall-MsixApp }
  "Status"    { Show-MsixStatus }
}
