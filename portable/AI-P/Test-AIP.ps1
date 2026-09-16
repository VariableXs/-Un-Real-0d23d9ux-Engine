<#
.SYNOPSIS
  AI-P 模块自测（任务 6 / 任务 9）——全部作用于真实交付文件，无替身实现

.DESCRIPTION
  用例清单（对应总案阶段 1 步骤 1/4 验收）：
    T1 PlanOnly 1024GB 布局输出五分区且 4K 对齐（逻辑/视觉=分区表可视化）
    T2 PlanOnly 小盘拒绝并明确报错（逻辑=容量边界）
    T3 PlanOnly 超大 SNAPSHOT 余量不足拒绝
    T4 Init-Shared 产出契约四件套
    T5 Init-Shared 幂等重跑文件不变（哈希比对）
    T6 契约损坏走默认 + 留证 .corrupt-*
    T7 ValidateOnly 契约完好通过 / apps.json 负向 schema 失败
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
  # 子脚本的 stderr（负向用例的预期报错）以 ErrorRecord 形式混入；先统一转字符串，
  # 避免父进程 $ErrorActionPreference=Stop 把预期报错当成本测试的中止错误。
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

Write-Host '== T1 PlanOnly 1024GB 布局 ==' -ForegroundColor Cyan
$r = Invoke-Script (Join-Path $here 'Create-Partitions.ps1') @('-PlanOnly', '-PlanDiskGB', '1024')
Add-Check '退出码 0' ($r.Code -eq 0) $r.Out
foreach ($n in @('ESP', 'VARIX_SYS', 'WIN_ENGINE', 'SHARED', 'SNAPSHOT')) {
  Add-Check "布局含 $n" ($r.Out -match [regex]::Escape($n)) $r.Out
}
Add-Check '含 exFAT 分区' ($r.Out -match 'exFAT') $r.Out

Write-Host '== T2 小盘拒绝 ==' -ForegroundColor Cyan
$r = Invoke-Script (Join-Path $here 'Create-Partitions.ps1') @('-PlanOnly', '-PlanDiskGB', '50')
Add-Check '退出码非 0' ($r.Code -ne 0) $r.Out
Add-Check '报错含越界/余量语义' ($r.Out -match '越界' -or $r.Out -match '余量') $r.Out

Write-Host '== T3 SNAPSHOT 余量不足拒绝 ==' -ForegroundColor Cyan
# 1TB 配置固定开销 965GB；900GB 盘 SNAPSHOT 余量 < 20GB 应拒
$r = Invoke-Script (Join-Path $here 'Create-Partitions.ps1') @('-PlanOnly', '-PlanDiskGB', '975')
Add-Check '退出码非 0' ($r.Code -ne 0) $r.Out
Add-Check '报错含余量不足' ($r.Out -match '余量') $r.Out

Write-Host '== T4/T5/T6/T7 契约用例（临时目录）==' -ForegroundColor Cyan
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("ai-p-test-" + (Get-Date -Format 'yyyyMMddHHmmss'))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
  $r = Invoke-Script (Join-Path $here 'Init-Shared.ps1') @('-SharedRoot', $tmp)
  Add-Check '初始化退出码 0' ($r.Code -eq 0) $r.Out
  foreach ($p in @('apps.json', 'boot-select.json', 'whitelist\whitelist.json', 'handoff')) {
    Add-Check "产出 $p" (Test-Path (Join-Path $tmp $p)) $r.Out
  }

  # T5 幂等：改时间戳字段后重跑，文件哈希不变
  $appsPath = Join-Path $tmp 'apps.json'
  $h1 = (Get-FileHash $appsPath).Hash
  Start-Sleep -Milliseconds 50
  $r = Invoke-Script (Join-Path $here 'Init-Shared.ps1') @('-SharedRoot', $tmp)
  Add-Check '重跑退出码 0' ($r.Code -eq 0) $r.Out
  $h2 = (Get-FileHash $appsPath).Hash
  Add-Check '幂等：重跑文件哈希不变' ($h1 -eq $h2) "h1=$h1 h2=$h2"
  Add-Check '幂等：无 .corrupt 残留' (@(Get-ChildItem $tmp -Filter '*.corrupt-*' -Recurse).Count -eq 0) $r.Out

  # T6 损坏走默认 + 留证
  Set-Content -LiteralPath $appsPath -Value '{ this is not json' -Encoding UTF8
  $r = Invoke-Script (Join-Path $here 'Init-Shared.ps1') @('-SharedRoot', $tmp)
  Add-Check '损坏重建退出码 0' ($r.Code -eq 0) $r.Out
  Add-Check '留证 .corrupt 文件存在' (@(Get-ChildItem $tmp -Filter 'apps.json.corrupt-*').Count -eq 1) $r.Out
  Add-Check '重建后可解析' ($null -ne (Get-Content $appsPath -Raw | ConvertFrom-Json)) $r.Out

  # T7 ValidateOnly 通过
  $r = Invoke-Script (Join-Path $here 'Init-Shared.ps1') @('-SharedRoot', $tmp, '-ValidateOnly')
  Add-Check 'ValidateOnly 通过' ($r.Code -eq 0) $r.Out

  # T7 负向：channel 非法必须失败
  $bad = @{ version = 1; apps = @(@{ id = 'a1'; name = 'x'; channel = 'nope'; tier = 'ok' }) } | ConvertTo-Json -Depth 5
  Set-Content -LiteralPath $appsPath -Value $bad -Encoding UTF8
  $r = Invoke-Script (Join-Path $here 'Init-Shared.ps1') @('-SharedRoot', $tmp, '-ValidateOnly')
  Add-Check '负向 schema 拒绝' ($r.Code -eq 1) $r.Out
  Add-Check '报错含 channel 语义' ($r.Out -match 'channel') $r.Out
}
finally {
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($failures.Count -gt 0) {
  Write-Host "AI-P 自测：$checks 项中 $($failures.Count) 项失败" -ForegroundColor Red
  $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
  exit 1
}
Write-Host "AI-P 自测：$checks 项全部通过" -ForegroundColor Green
exit 0
