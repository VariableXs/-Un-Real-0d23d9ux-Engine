<#
.SYNOPSIS
  AI-P 模块自测二（任务 7 / 8 / 11）——作用于真实交付文件与真实构建产物

  T8  Build-ESP：VARIX 链组装 + Windows 链组装 + 清单校验 + 篡改检出
  T9  Deploy-Varix-USB：PlanOnly 预演
  T11 Manage-Versions：Snapshot 单调递增 / New-Diff 差集 / Compare 相同性 / Retire 回收站
  退出码 0 = 全过。
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
$failures = @()
$checks = 0
$exe = if (Get-Command pwsh -ErrorAction SilentlyContinue) { 'pwsh' } else { 'powershell' }

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail = '')
  $script:checks++
  if ($Ok) { Write-Host "  PASS  $Name" -ForegroundColor Green }
  else {
    Write-Host "  FAIL  $Name  $Detail" -ForegroundColor Red
    $script:failures += "$Name：$Detail"
  }
}

function Invoke-Script {
  param([string]$File, [string[]]$ScriptArgs)
  $prevEap = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    $out = (& $exe -NoProfile -ExecutionPolicy Bypass -File $File @ScriptArgs 2>&1) | ForEach-Object { "$_" }
  }
  finally {
    $ErrorActionPreference = $prevEap
  }
  return @{ Code = $LASTEXITCODE; Out = ($out -join "`n") }
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("ai-p2-test-" + (Get-Date -Format 'yyyyMMddHHmmss'))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
  Write-Host '== T8 Build-ESP ==' -ForegroundColor Cyan
  $esp = Join-Path $tmp 'esp'
  New-Item -ItemType Directory -Force -Path $esp | Out-Null
  $r = Invoke-Script (Join-Path $here 'Build-ESP.ps1') @('-EspPath', $esp)
  Add-Check 'VARIX 链组装退出码 0' ($r.Code -eq 0) $r.Out
  foreach ($p in @('EFI\BOOT\BOOTX64.EFI', 'limine-bios.sys', 'limine.conf', 'kernel\varix', 'initrd.img', 'ESP-MANIFEST.json')) {
    Add-Check "产出 $p" (Test-Path (Join-Path $esp $p)) $r.Out
  }

  # Windows 链（本地构造真实引导源：bootmgfw.efi + BCD 占位文件，来源内容不影响组装逻辑验证）
  $winSrc = Join-Path $tmp 'win-src\EFI\Microsoft\Boot'
  New-Item -ItemType Directory -Force -Path $winSrc | Out-Null
  Set-Content (Join-Path $winSrc 'bootmgfw.efi') 'fake-bootmgfw' -Encoding ASCII
  Set-Content (Join-Path $winSrc 'BCD') 'fake-bcd' -Encoding ASCII
  $r = Invoke-Script (Join-Path $here 'Build-ESP.ps1') @('-EspPath', $esp, '-WindowsBootDir', (Join-Path $tmp 'win-src'))
  Add-Check 'Windows 链组装退出码 0' ($r.Code -eq 0) $r.Out
  Add-Check 'Windows 链落位 EFI\Microsoft\Boot\bootmgfw.efi' (Test-Path (Join-Path $esp 'EFI\Microsoft\Boot\bootmgfw.efi')) $r.Out
  Add-Check 'Windows 链落位 EFI\Microsoft\Boot\BCD' (Test-Path (Join-Path $esp 'EFI\Microsoft\Boot\BCD')) $r.Out

  $r = Invoke-Script (Join-Path $here 'Build-ESP.ps1') @('-EspPath', $esp, '-VerifyOnly')
  Add-Check 'VerifyOnly 通过' ($r.Code -eq 0) $r.Out

  # 幂等重入：重跑 VARIX 链后 Windows 链仍在
  $r = Invoke-Script (Join-Path $here 'Build-ESP.ps1') @('-EspPath', $esp)
  Add-Check '重入退出码 0' ($r.Code -eq 0) $r.Out
  Add-Check '重入后 Windows 链未被破坏' (Test-Path (Join-Path $esp 'EFI\Microsoft\Boot\BCD')) $r.Out

  # 篡改检出
  Set-Content (Join-Path $esp 'limine.conf') 'tampered' -Encoding ASCII
  $r = Invoke-Script (Join-Path $here 'Build-ESP.ps1') @('-EspPath', $esp, '-VerifyOnly')
  Add-Check '篡改被 VerifyOnly 检出' ($r.Code -ne 0) $r.Out

  Write-Host '== T9 Deploy-Varix-USB PlanOnly ==' -ForegroundColor Cyan
  $r = Invoke-Script (Join-Path $here 'Deploy-Varix-USB.ps1') @('-PlanOnly')
  Add-Check 'PlanOnly 退出码 0' ($r.Code -eq 0) $r.Out
  foreach ($s in @('Preflight', 'Partition', 'ESP', 'Shared', 'SysFiles', 'Verify', 'Report')) {
    Add-Check "预演含 $s" ($r.Out -match $s) $r.Out
  }
  Add-Check '预演含断点状态文件路径' ($r.Out -match 'deploy-state') $r.Out

  Write-Host '== T11 Manage-Versions ==' -ForegroundColor Cyan
  $vroot = Join-Path $tmp 'usbroot'
  New-Item -ItemType Directory -Force -Path (Join-Path $vroot 'kernel') | Out-Null
  Set-Content (Join-Path $vroot 'kernel\varix') 'kernel-v1' -Encoding ASCII
  Set-Content (Join-Path $vroot 'limine.conf') 'conf-v1' -Encoding ASCII

  $r = Invoke-Script (Join-Path $here 'Manage-Versions.ps1') @('-Action', 'Snapshot', '-Root', $vroot, '-Label', 'first-burn')
  Add-Check 'Snapshot v1.0 退出码 0' ($r.Code -eq 0) $r.Out
  Add-Check 'MANIFEST 产出' (Test-Path (Join-Path $vroot 'MANIFEST.json')) $r.Out

  Set-Content (Join-Path $vroot 'kernel\varix') 'kernel-v2' -Encoding ASCII
  $r = Invoke-Script (Join-Path $here 'Manage-Versions.ps1') @('-Action', 'Snapshot', '-Root', $vroot)
  Add-Check '重 Snapshot 退出码 0（MINOR+1）' ($r.Code -eq 0) $r.Out
  $mf = Get-Content (Join-Path $vroot 'MANIFEST.json') -Raw | ConvertFrom-Json
  Add-Check '版本单调递增到 1.1' ($mf.version -eq '1.1') "version=$($mf.version)"

  $r = Invoke-Script (Join-Path $here 'Manage-Versions.ps1') @('-Action', 'New-Diff', '-Root', $vroot)
  Add-Check 'New-Diff 退出码 0' ($r.Code -eq 0) $r.Out
  $diffs = @(Get-ChildItem (Join-Path $vroot '_versions') -Filter 'diff-*.json')
  Add-Check '差分清单产出' ($diffs.Count -eq 1) $r.Out
  $diff = Get-Content $diffs[0].FullName -Raw | ConvertFrom-Json
  Add-Check '差集检出 1 处变更' (@($diff.changed).Count -eq 1 -and $diff.changed[0] -eq 'kernel\varix') ($diff | ConvertTo-Json -Depth 5)

  # Compare：同盘克隆一致；克隆改文件后重 Snapshot 再比对（清单哈希语义）必须拒绝
  $clone = Join-Path $tmp 'usbroot2'
  Copy-Item $vroot $clone -Recurse -Force
  $r = Invoke-Script (Join-Path $here 'Manage-Versions.ps1') @('-Action', 'Compare', '-Root', $vroot, '-OtherRoot', $clone)
  Add-Check 'Compare 相同盘一致' ($r.Code -eq 0) $r.Out
  Set-Content (Join-Path $clone 'kernel\varix') 'kernel-clone-diff' -Encoding ASCII
  $r = Invoke-Script (Join-Path $here 'Manage-Versions.ps1') @('-Action', 'Snapshot', '-Root', $clone)
  $r = Invoke-Script (Join-Path $here 'Manage-Versions.ps1') @('-Action', 'Compare', '-Root', $vroot, '-OtherRoot', $clone)
  Add-Check 'Compare 差异盘拒绝' ($r.Code -eq 1) $r.Out

  # Retire：进回收站区不删除
  $r = Invoke-Script (Join-Path $here 'Manage-Versions.ps1') @('-Action', 'Retire', '-Root', $vroot, '-Files', 'limine.conf')
  Add-Check 'Retire 退出码 0' ($r.Code -eq 0) $r.Out
  Add-Check '原文件已移走' (-not (Test-Path (Join-Path $vroot 'limine.conf'))) $r.Out
  $trashed = @(Get-ChildItem (Join-Path $vroot '_trash') -Recurse -File -Filter 'limine.conf')
  Add-Check '_trash 区留档' ($trashed.Count -eq 1) $r.Out

  # Show：无清单盘拒绝
  $r = Invoke-Script (Join-Path $here 'Manage-Versions.ps1') @('-Action', 'Show', '-Root', $tmp)
  Add-Check 'Show 无清单退出码非 0' ($r.Code -ne 0) $r.Out
}
finally {
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($failures.Count -gt 0) {
  Write-Host "AI-P 自测二：$checks 项中 $($failures.Count) 项失败" -ForegroundColor Red
  $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
  exit 1
}
Write-Host "AI-P 自测二：$checks 项全部通过" -ForegroundColor Green
exit 0
