<#
tools/release.ps1 — M-87 发版演练（Release Rehearsal）

流程（顺序固定，任一 FAIL 即中止退出 1）：
  1) 版本号一致性：package.json = tauri.conf.json = src-tauri/Cargo.toml
  2) 全量门禁：tsc → vitest → audit.cjs → keymap-audit.cjs → link-check.cjs → 前端 build
  3) M-80 release 剔除验证：dist/ 内 "ipcTrace" 零命中
  4) cargo check（后端）
  5) tauri build（NSIS 安装包）+ 产物存在校验（-DryRun / -SkipBuild 时如实标 SKIP）
  6) CHANGELOG 草稿（M-61 gen-changelog.cjs）
  7) ROUTES / SETTINGS 再生成一致性（M-55/56：--check，漂移则再生成并标 FIXED）
  8) 检查清单输出 + 归档 docs/acceptance/release-rehearsal-<date>.md

用法：
  powershell -ExecutionPolicy Bypass -File tools/release.ps1            # 全流程
  powershell -ExecutionPolicy Bypass -File tools/release.ps1 -DryRun    # 空跑演练：门禁全跑，构建跳过
  powershell -ExecutionPolicy Bypass -File tools/release.ps1 -SkipBuild # 仅跳过 NSIS 构建

BOM 纪律：本文件含中文注释，必须以 UTF-8 with BOM 保存（否则 pwsh 乱码）。
#>
param(
  [switch]$DryRun,
  [switch]$SkipBuild,
  [string]$ArchiveDir = "docs/acceptance"
)

$ErrorActionPreference = "Stop"
$Root = (Split-Path -Parent $PSScriptRoot)
Set-Location $Root

$script:Results = New-Object System.Collections.Generic.List[object]
$script:Failed = $false

function Add-Check {
  param([string]$Step, [string]$Status, [string]$Note = "")
  $script:Results.Add([pscustomobject]@{ Step = $Step; Status = $Status; Note = $Note })
  if ($Status -eq "FAIL") { $script:Failed = $true }
  Write-Host ("  [{0,-4}] {1}{2}" -f $Status, $Step, $(if ($Note) { " — $Note" } else { "" }))
}

function Invoke-Step {
  param([string]$Step, [scriptblock]$Body, [string]$SkipNote = "")
  if ($SkipNote) { Add-Check $Step "SKIP" $SkipNote; return }
  # 原生命令（npm/node/cargo）的 stderr（如 vite chunk 体积警告、npm.ps1 包装层）
  # 在 PS5.1 + ErrorActionPreference=Stop 下会被转成终止性 NativeCommandError；
  # 门禁成败只看退出码，故步骤体内临时切 Continue。
  $prev = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    & $Body
    Add-Check $Step "PASS"
  } catch {
    Add-Check $Step "FAIL" (($_.Exception.Message -split "`n")[0])
    throw
  } finally {
    $ErrorActionPreference = $prev
  }
}

Write-Host "== M-87 发版演练 开始 $(if ($DryRun) { '（DryRun 空跑）' } else { '' }) =="

# ---- 1. 版本号一致性（不一致 = 立即拦截） ----
$pkgVersion = (Get-Content "package.json" -Raw | ConvertFrom-Json).version
$confVersion = (Get-Content "src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json).version
$cargoMatch = (Select-String -Path "src-tauri/Cargo.toml" -Pattern '(?m)^version\s*=\s*"([^"]+)"' | Select-Object -First 1)
$cargoVersion = if ($cargoMatch) { $cargoMatch.Matches[0].Groups[1].Value } else { "<未找到>" }
if ($pkgVersion -eq $confVersion -and $confVersion -eq $cargoVersion) {
  Add-Check "1.版本号一致性" "PASS" "v$pkgVersion（package.json = tauri.conf.json = Cargo.toml）"
} else {
  Add-Check "1.版本号一致性" "FAIL" "package.json=$pkgVersion · tauri.conf.json=$confVersion · Cargo.toml=$cargoVersion"
  Write-Error "版本号不一致：发版被 M-87 门禁拦截（三处必须一致后再演练）。"
}

# ---- 2. 全量门禁 ----
Invoke-Step "2a.tsc 类型检查" {
  npm run typecheck *> $null
  if ($LASTEXITCODE -ne 0) { throw "typecheck 退出码 $LASTEXITCODE" }
}
Invoke-Step "2b.vitest 单测" {
  npm test -- --run *> $null
  if ($LASTEXITCODE -ne 0) { throw "vitest 退出码 $LASTEXITCODE" }
}
Invoke-Step "2c.静态审计 audit.cjs" {
  node tools/audit.cjs *> $null
  if ($LASTEXITCODE -ne 0) { throw "audit.cjs 退出码 $LASTEXITCODE" }
}
Invoke-Step "2d.键位门禁 keymap-audit" {
  node tools/keymap-audit.cjs *> $null
  if ($LASTEXITCODE -ne 0) { throw "keymap-audit 退出码 $LASTEXITCODE" }
}
Invoke-Step "2e.文档链接 link-check" {
  node tools/link-check.cjs *> $null
  if ($LASTEXITCODE -ne 0) { throw "link-check 退出码 $LASTEXITCODE" }
}
Invoke-Step "2f.前端构建" {
  npm run build *> $null
  if ($LASTEXITCODE -ne 0) { throw "vite build 退出码 $LASTEXITCODE" }
}

# ---- 3. M-80 release 剔除验证 ----
Invoke-Step "3.M-80 IPC 追踪剔除" {
  $hits = Get-ChildItem "dist" -Recurse -Include "*.js", "*.html" -File | Select-String -Pattern "ipcTrace" -List
  if ($hits) { throw "dist/ 内发现 $($hits.Count) 处 ipcTrace 残留（dev-only 代码泄漏进 release）" }
}

# ---- 4. 后端 cargo check ----
Invoke-Step "4.cargo check" {
  cargo check --manifest-path src-tauri/Cargo.toml --quiet *> $null
  if ($LASTEXITCODE -ne 0) { throw "cargo check 退出码 $LASTEXITCODE" }
}

# ---- 5. tauri build（NSIS） ----
$buildSkip = if ($DryRun) { "DryRun 空跑：跳过 NSIS 构建（真发版必跑）" } elseif ($SkipBuild) { "-SkipBuild：按参数跳过" } else { "" }
if ($buildSkip) {
  Add-Check "5.NSIS 安装包构建" "SKIP" $buildSkip
  Add-Check "5b.NSIS 产物校验" "SKIP" "随 5 跳过"
} else {
  Invoke-Step "5.NSIS 安装包构建" {
    npm run tauri build *> $null
    if ($LASTEXITCODE -ne 0) { throw "tauri build 退出码 $LASTEXITCODE" }
  }
  $nsis = Get-ChildItem "src-tauri/target/release/bundle/nsis/*.exe" -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if ($nsis) { Add-Check "5b.NSIS 产物校验" "PASS" "$($nsis.Name)（$([math]::Round($nsis.Length / 1MB, 1)) MB）" }
  else { Add-Check "5b.NSIS 产物校验" "FAIL" "bundle/nsis 下未找到安装包 exe" }
}

# ---- 6. CHANGELOG 草稿（M-61） ----
Invoke-Step "6.CHANGELOG 草稿生成" {
  node tools/gen-changelog.cjs *> $null
  if ($LASTEXITCODE -ne 0) { throw "gen-changelog 退出码 $LASTEXITCODE" }
}

# ---- 7. ROUTES / SETTINGS 再生成一致性（M-55/56） ----
function Invoke-GenCheck {
  param([string]$Name, [string]$Tool)
  # 同 Invoke-Step：PS5.1 下 node 的 stderr 输出须以 Continue 对待（成败看退出码）
  $prev = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    node $Tool --check *> $null
    if ($LASTEXITCODE -eq 0) {
      Add-Check $Name "PASS" "生成物与源同步"
      return
    }
    # 漂移：再生成并标记 FIXED（演练口径：漂移 = 上次提交未同步生成物，提交前须复核 diff）
    node $Tool *> $null
    if ($LASTEXITCODE -ne 0) { Add-Check $Name "FAIL" "$Tool 再生成失败"; return }
    Add-Check $Name "FIXED" "存在漂移，已再生成（提交前须复核 diff）"
  } finally {
    $ErrorActionPreference = $prev
  }
}
Invoke-GenCheck "7a.ROUTES.md" "tools/gen-routes.cjs"
Invoke-GenCheck "7b.SETTINGS.md" "tools/gen-settings-doc.cjs"

# ---- 8. 清单输出与归档 ----
$date = Get-Date -Format "yyyy-MM-dd"
$summary = if ($script:Failed) { "FAIL（存在未通过/漂移项，发版中止）" } else { "PASS（演练全绿）" }
Write-Host ""
Write-Host "== M-87 发版演练 检查清单 =="
$script:Results | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "结论：$summary"

$archive = Join-Path $ArchiveDir "release-rehearsal-$date.md"
$lines = @(
  "# M-87 发版演练记录 — $date",
  "",
  "- 模式：$(if ($DryRun) { "DryRun 空跑（门禁全跑、NSIS 构建跳过）" } else { "全流程" })",
  "- 结论：$summary",
  "",
  "| 步骤 | 状态 | 备注 |",
  "| --- | --- | --- |"
)
foreach ($r in $script:Results) { $lines += "| $($r.Step) | $($r.Status) | $($r.Note) |" }
New-Item -ItemType Directory -Force -Path $ArchiveDir | Out-Null
Set-Content -Path $archive -Value ($lines -join "`n") -Encoding UTF8
Write-Host "清单已归档：$archive"

if ($script:Failed) { exit 1 } else { exit 0 }
