<#
.SYNOPSIS
  V-8：四阶段交付真机（PORTABLE 第 12 章）
  Deploy-To-USB.ps1 -Action Stage1..4 + Verify：
    Stage1 底座：U 盘格式化（exFAT→视容量, 64K 簇）+ 目录骨架（Variable/ Uxv/）
    Stage2 引擎：引导器 + 引擎 exe + 脚本层复制入盘（SHA-256 清单）
    Stage3 镜像：VHDX 基础盘入盘 + 三层差分链建立（Layer-Chain）+ BitLocker 可选
    Stage4 收口：登录即 Variable（VM 档 agent 自启）+ 自检 + 拔插换机核验清单
  证据截图目录：docs/acceptance/v8/

.EXAMPLE
  .\Deploy-To-USB.ps1 -Action Stage1 -Usb E: -Source D:\build\dist
  .\Deploy-To-USB.ps1 -Action Verify -Usb E:
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet('Stage1', 'Stage2', 'Stage3', 'Stage4', 'Verify')]
  [string]$Action,
  [Parameter(Mandatory = $true)]
  [string]$Usb,                        # U 盘盘符，如 E:
  [string]$Source = "",                # 引擎构建产物目录（Stage2）
  [string]$BaseVhdx = "",              # 基础盘源文件（Stage3）
  [switch]$BitLocker,                  # Stage3 可选：交付即加密
  [switch]$Format                      # Stage1 必须显式 -Format 才动盘（防误格式化）
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$usbRoot = "$Usb\"
$varDir = Join-Path $usbRoot 'Variable'
$uxvDir = Join-Path $usbRoot 'Uxv'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent

switch ($Action) {
  'Stage1' {
    if (-not (Test-Path -LiteralPath $usbRoot)) { throw "U 盘不存在：$Usb" }
    if ($Format) {
      Write-Host "!!! 即将格式化 $Usb —— 确认无数据后继续（Ctrl+C 取消）" -ForegroundColor Red
      Read-Host '输入 YES 继续' | ForEach-Object { if ($_ -ne 'YES') { throw '已取消' } }
      Format-Volume -DriveLetter ($Usb -replace ':', '') -FileSystem exFAT -AllocationUnitSize 65536 -Force | Out-Null
      Write-Host "    已格式化 exFAT / 64K 簇（V-3 大文件盘簇建议）" -ForegroundColor Green
    }
    foreach ($d in @($varDir, $uxvDir, (Join-Path $uxvDir 'apps'), (Join-Path $uxvDir 'prefetch'), (Join-Path $uxvDir 'vault'))) {
      New-Item -ItemType Directory -Force -Path $d | Out-Null
    }
    Write-Host ">>> Stage1 完成：目录骨架 Variable/ Uxv/{apps,prefetch,vault}" -ForegroundColor Green
  }
  'Stage2' {
    if (-not $Source -or -not (Test-Path -LiteralPath $Source)) { throw "产物目录不存在：$Source" }
    Copy-Item -Path (Join-Path $Source '*') -Destination $varDir -Recurse -Force
    # SHA-256 清单（换机核验 + 杀软申诉双用）
    $manifest = Get-ChildItem $varDir -Recurse -File | Get-FileHash -Algorithm SHA256 |
      ForEach-Object { "{0}  {1}" -f $_.Hash, (Resolve-Path -LiteralPath $_.Path -RelativeBasePath $varDir).Path.Substring(2) }
    $manifest | Set-Content -LiteralPath (Join-Path $varDir 'SHA256SUMS.txt') -Encoding UTF8
    Write-Host ">>> Stage2 完成：引擎已复制，SHA-256 清单 Variable\SHA256SUMS.txt" -ForegroundColor Green
  }
  'Stage3' {
    if (-not $BaseVhdx -or -not (Test-Path -LiteralPath $BaseVhdx)) { throw "基础盘不存在：$BaseVhdx" }
    $dst = Join-Path $varDir 'Variable-OS-base.vhdx'
    Write-Host "    复制基础盘（10GB 级，1000MB/s 盘约 1-2 分钟）..." -ForegroundColor Cyan
    Copy-Item -LiteralPath $BaseVhdx -Destination $dst -Force
    & (Join-Path $PSScriptRoot 'Layer-Chain.ps1') -Action New -Base $dst
    if ($BitLocker) {
      & (Join-Path $PSScriptRoot 'Protect-VHDX.ps1') -Action Enable -VhdxDrive $Usb -KeyDir (Join-Path $uxvDir 'vault')
    }
    Write-Host ">>> Stage3 完成：三层差分链就绪$(if ($BitLocker) { ' + BitLocker' })" -ForegroundColor Green
  }
  'Stage4' {
    # VM agent 自启已在镜像内（Install-VMAgent.ps1 登录自启项）；此处做收口自检 + 换机清单
    & (Join-Path $PSScriptRoot 'Self-Check.ps1')
    @(
      '# V-8 换机核验清单（docs/acceptance/v8/ 留截图）',
      '- [ ] 引导器→探测（probe-cache.json 生成）',
      '- [ ] Hyper-V / VirtualBox / 轻量三档自动降级（degrade 决策日志）',
      '- [ ] VM 启动→agent 心跳 47631→登录即 Variable（D-1）',
      '- [ ] 四大软件打开 + 嵌入（webview）',
      '- [ ] 退出 → 拔盘 → 插入另一台机器重复以上',
      '- [ ] SHA256SUMS.txt 换机后校验一致'
    ) -join "`r`n" | Set-Content -LiteralPath (Join-Path $repo 'docs\acceptance\v8\swap-machine-checklist.md') -Encoding UTF8
    Write-Host ">>> Stage4 完成：换机核验清单已生成 docs\acceptance\v8\" -ForegroundColor Green
  }
  'Verify' {
    $checks = @(
      @{ n = '引导器'; p = "$varDir\Variable-Launcher.exe" },
      @{ n = 'SHA256 清单'; p = "$varDir\SHA256SUMS.txt" },
      @{ n = '基础盘'; p = "$varDir\Variable-OS-base.vhdx" },
      @{ n = 'app 层'; p = "$varDir\Variable-OS-app.vhdx" },
      @{ n = 'user 层'; p = "$varDir\Variable-OS-user.vhdx" },
      @{ n = '数据容器'; p = "$uxvDir\apps" }
    )
    $fail = 0
    foreach ($c in $checks) {
      $ok = Test-Path -LiteralPath $c.p
      Write-Host ("[{0}] {1}: {2}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $c.n, $c.p) -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
      if (-not $ok) { $fail++ }
    }
    if ($fail -gt 0) { exit 1 }
    Write-Host ">>> 交付盘结构完整（功能核验按换机清单走真机）" -ForegroundColor Cyan
  }
}
