<#
.SYNOPSIS
  V-7：VHDX 整盘加密 + 杀软白名单申诉包（PORTABLE 第 10/18 章）
  加密：BitLocker-to-Go（manage-bde 脚本化，恢复密钥落 Vault/B-14 目录）。
  备选：容器层 XChaCha20（B-14）—— 由引擎容器加密承担，本脚本只做状态核查。
  杀软：生成申诉包骨架（附录 E）：文件清单 + SHA-256 + 各杀软提交入口说明。

.EXAMPLE
  .\Protect-VHDX.ps1 -Action Enable  -VhdxDrive E: -KeyDir E:\Uxv\vault
  .\Protect-VHDX.ps1 -Action Status  -VhdxDrive E:
  .\Protect-VHDX.ps1 -Action Unlock  -VhdxDrive E: -RecoveryKey 123456-...
  .\Protect-VHDX.ps1 -Action AVPackage -LauncherExe D:\USB\Variable\launcher\Variable-Launcher.exe
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet('Enable', 'Status', 'Unlock', 'Lock', 'AVPackage')]
  [string]$Action,
  [string]$VhdxDrive = "",
  [string]$KeyDir = "",
  [string]$RecoveryKey = "",
  [string]$LauncherExe = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

switch ($Action) {
  'Enable' {
    if (-not $VhdxDrive) { throw '需要 -VhdxDrive（VHDX 挂载后的盘符，如 E:）' }
    if (-not $KeyDir) { throw '需要 -KeyDir（恢复密钥落点，建议 Uxv\vault）' }
    New-Item -ItemType Directory -Force -Path $KeyDir | Out-Null
    $keyFile = Join-Path $KeyDir ("bitlocker-recovery-{0}.txt" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
    # BitLocker-to-Go：数据盘可移动介质加密（U 盘/挂载 VHDX 均适用）
    $proc = Start-Process -FilePath 'manage-bde.exe' -ArgumentList @('-on', $VhdxDrive, '-RecoveryPassword', '-SkipHardwareTest') -Wait -PassThru -NoNewWindow -RedirectStandardOutput "$env:TEMP\mbde-out.txt"
    if ($proc.ExitCode -ne 0) { Get-Content "$env:TEMP\mbde-out.txt"; throw "manage-bde 失败（退出码 $($proc.ExitCode)）" }
    (manage-bde -protectors -get $VhdxDrive | Out-String) | Set-Content -LiteralPath $keyFile -Encoding UTF8
    Write-Host ">>> BitLocker 已启动加密（后台进行）。恢复密钥已存：$keyFile" -ForegroundColor Green
    Write-Host "    密钥文件建议同步进引擎 Vault（B-14 XChaCha20 容器加密为第二道保险）" -ForegroundColor Cyan
  }
  'Status' {
    if (-not $VhdxDrive) { throw '需要 -VhdxDrive' }
    manage-bde -status $VhdxDrive
  }
  'Unlock' {
    if (-not $VhdxDrive -or -not $RecoveryKey) { throw '需要 -VhdxDrive 和 -RecoveryKey' }
    $secure = ConvertTo-SecureString ($RecoveryKey -replace '-', '') -AsPlainText -Force
    Unlock-BitLocker -MountPoint $VhdxDrive -RecoveryPassword $RecoveryKey | Out-Null
    Write-Host ">>> 已解锁 $VhdxDrive" -ForegroundColor Green
  }
  'Lock' {
    if (-not $VhdxDrive) { throw '需要 -VhdxDrive' }
    Lock-BitLocker -MountPoint $VhdxDrive -ForceDismount | Out-Null
    Write-Host ">>> 已上锁 $VhdxDrive" -ForegroundColor Green
  }
  'AVPackage' {
    if (-not $LauncherExe -or -not (Test-Path -LiteralPath $LauncherExe)) { throw '需要 -LauncherExe 指向已签名/未签名引导器 exe' }
    $repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
    $pkgDir = Join-Path $repo 'docs\acceptance\av-package'
    New-Item -ItemType Directory -Force -Path $pkgDir | Out-Null
    $hash = (Get-FileHash -LiteralPath $LauncherExe -Algorithm SHA256).Hash
    $info = Get-Item -LiteralPath $LauncherExe
    $sig = Get-AuthenticodeSignature -LiteralPath $LauncherExe
    @(
      '# 杀软白名单申诉包（附录 E）',
      '',
      "## 文件：$($info.Name)",
      "- 大小: $($info.Length) 字节",
      "- SHA-256: ``$hash``",
      "- 签名: $(if ($sig.Status -eq 'Valid') { "有效（$($sig.SignerCertificate.Subject)）" } else { "未签名/无效: $($sig.Status) —— 申诉前先做代码签名" })",
      '',
      '## 误报提交入口',
      '- Microsoft Defender: https://www.microsoft.com/en-us/wdsi/filesubmission',
      '- 360: https://fu.360.cn/  （或 opensoft.360.cn 开发者认证）',
      '- 火绒: https://www.huorong.cn/feedback.html  或邮件 hr-feedback@huorong.cn',
      '',
      '## 实测排除项（真机验证后补充截图到 docs/acceptance/v7/）',
      '- [ ] Defender 排除：Windows 安全中心 → 病毒防护 → 排除项（目录级排除整盘）',
      '- [ ] 360 信任区：添加引导器 + VHDX 目录',
      '- [ ] 火绒信任区：同上',
      '- [ ] 申诉通过后在此记录工单号与生效日期'
    ) -join "`r`n" | Set-Content -LiteralPath (Join-Path $pkgDir 'README.md') -Encoding UTF8
    Write-Host ">>> 申诉包骨架已生成：$pkgDir\README.md（SHA-256: $hash）" -ForegroundColor Green
  }
}
