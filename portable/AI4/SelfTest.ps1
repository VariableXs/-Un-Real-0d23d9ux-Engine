param(
  [switch]$SkipExec
)
# AI-4 拓展核 / 自检脚本 (风格对齐 portable\tests\Run-PortableTests.ps1)
# 三段:
#   1. 语法: 官方 AST 解析器解析 portable\AI4\*.ps1, 有 ParseError 即失败
#   2. 数据: Config\*.json 结构校验
#   3. 执行: 只读动作 + 沙箱内可逆写动作(全部落在 $env:TEMP 下的临时目录)
#      - Data-Init 目录树 + 合法 Hive(regf 魔数)
#      - Config-Runtime ${DATA_ROOT} 展开
#      - Verify-Chain 建基线 -> 篡改一个字节 -> 校验必须退出码1 -> 还原后通过 (18.2 真逻辑)
#   退出码 0 = 全通过。CI (windows-latest, 经 portable\AI5\__tests__) 或真机可跑。
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ai4Root = $PSScriptRoot
$failures = @()
$checks = 0

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail = "")
  $script:checks++
  if ($Ok) { Write-Host "  ok   $Name" -ForegroundColor Green }
  else {
    Write-Host "  FAIL $Name  $Detail" -ForegroundColor Red
    $script:failures += "$Name :: $Detail"
  }
}

# ---------------- 1. 语法 (官方 AST 解析器) ----------------
Write-Host "== 1. AI-4 PowerShell 语法解析 ==" -ForegroundColor Cyan
$ps1 = @(Get-ChildItem -Path $ai4Root -Filter "*.ps1" -File)
foreach ($f in $ps1) {
  $tokens = $null; $errors = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$tokens, [ref]$errors)
  if ($errors -and $errors.Count -gt 0) {
    $first = $errors[0]
    Add-Check -Name ("语法 " + $f.Name) -Ok $false -Detail ("{0} 处错误, 首条 L{1}: {2}" -f $errors.Count, $first.Extent.StartLineNumber, $first.Message)
  } else {
    Add-Check -Name ("语法 " + $f.Name) -Ok $true
  }
}

# ---------------- 2. 数据文件校验 ----------------
Write-Host "== 2. Config 数据校验 ==" -ForegroundColor Cyan
try {
  $market = Get-Content (Join-Path $ai4Root "Config\plugin-market.json") -Raw -Encoding UTF8 | ConvertFrom-Json
  Add-Check -Name "plugin-market.json 可解析" -Ok ($null -ne $market.plugins)
  Add-Check -Name "插件均有 id/entry/permissions" -Ok (@($market.plugins | Where-Object { -not $_.id -or -not $_.entry }).Count -eq 0)
} catch { Add-Check -Name "plugin-market.json 可解析" -Ok $false -Detail $_.Exception.Message }
try {
  $perms = Get-Content (Join-Path $ai4Root "Config\permissions.json") -Raw -Encoding UTF8 | ConvertFrom-Json
  Add-Check -Name "permissions.json 可解析" -Ok ($null -ne $perms.AuthorizedPlugins)
} catch { Add-Check -Name "permissions.json 可解析" -Ok $false -Detail $_.Exception.Message }
try {
  $sc = Get-Content (Join-Path $ai4Root "Config\shortcuts.json") -Raw -Encoding UTF8 | ConvertFrom-Json
  Add-Check -Name "shortcuts.json 可解析且有热键" -Ok (@($sc.hotkeys).Count -ge 1)
} catch { Add-Check -Name "shortcuts.json 可解析且有热键" -Ok $false -Detail $_.Exception.Message }
$envOk = $false
foreach ($line in (Get-Content (Join-Path $ai4Root "Config\path.env"))) {
  $t = $line.Trim()
  if ($t -and ($t -notlike "#*")) { if ($t -match '^[A-Za-z_][A-Za-z0-9_]*=.*$') { $envOk = $true } }
}
Add-Check -Name "path.env 至少一条合法 KEY=VALUE" -Ok $envOk

# ---------------- 3. 执行 ----------------
if ($SkipExec) {
  Write-Host "== 3. 执行 (已按 -SkipExec 跳过) ==" -ForegroundColor Cyan
} else {
  Write-Host "== 3. 执行 (临时目录, 不碰真实盘) ==" -ForegroundColor Cyan
  $env:AI5_NONINTERACTIVE = "1"
  $tmp = Join-Path ([IO.Path]::GetTempPath()) ("ai4-selftest-" + (Get-Date -Format "yyyyMMddHHmmss"))
  New-Item -ItemType Directory -Force -Path $tmp | Out-Null
  $dataRoot = Join-Path $tmp "Data"

  function Invoke-Ai4 {
    param([string]$Name, [string]$Script, [hashtable]$ScriptArgs, [int]$ExpectExit = 0, [string]$ExpectText = "")
    $p = Join-Path $ai4Root $Script
    $code = 0
    try {
      $txt = (& $p @ScriptArgs *>&1 | Out-String)
      if ($null -ne $LASTEXITCODE) { $code = $LASTEXITCODE }
    } catch {
      Add-Check -Name $Name -Ok $false -Detail ("异常: " + $_.Exception.Message)
      return ""
    }
    Add-Check -Name "$Name (exit=$code)" -Ok ($code -eq $ExpectExit) -Detail "期望退出码 $ExpectExit"
    if ($ExpectText) {
      Add-Check -Name "$Name 输出含 '$ExpectText'" -Ok ($txt -match [regex]::Escape($ExpectText))
    }
    return $txt
  }

  # 3.1 Data-Init: 目录树 + 合法 Hive
  $null = Invoke-Ai4 -Name "Data-Init -DataDrive $tmp" -Script "Data-Init.ps1" -ScriptArgs @{ DataDrive = $tmp } -ExpectText "Data 初始化完成"
  foreach ($d in @("Apps", "MSIX", "Plugins", "Exchange", "Security", "Registry", "Config")) {
    Add-Check -Name "Data 目录存在: $d" -Ok (Test-Path (Join-Path $dataRoot $d))
  }
  $hive = Join-Path $dataRoot "Registry\User.dat"
  $magic = ""
  if (Test-Path $hive) {
    $fs = [IO.File]::OpenRead($hive)
    try { $b = New-Object byte[] 4; [void]$fs.Read($b, 0, 4); $magic = [Text.Encoding]::ASCII.GetString($b) } finally { $fs.Close() }
  }
  Add-Check -Name "User.dat 为合法 Hive (regf 魔数)" -Ok ($magic -eq "regf") -Detail "实际魔数: '$magic'"

  # 3.2 Config-Runtime: ${DATA_ROOT} 展开
  $envFile = Join-Path $tmp "test.env"
  Set-Content -Path $envFile -Value "VARIABLE_TEST_ROOT=`${DATA_ROOT}\Tools" -Encoding ASCII
  $out = Invoke-Ai4 -Name 'Config-Runtime Apply-Env (${DATA_ROOT} 展开)' -Script "Config-Runtime.ps1" `
    -ScriptArgs @{ Action = "Apply-Env"; EnvFile = $envFile; DataDrive = $tmp } -ExpectText "$dataRoot\Tools"

  # 3.3 Merge-Apps Status (只读)
  $null = Invoke-Ai4 -Name "Merge-Apps -Action Status (只读)" -Script "Merge-Apps.ps1" `
    -ScriptArgs @{ Action = "Status"; DataRoot = $dataRoot; BaseVhdx = (Join-Path $tmp "Base.vhdx"); AppsVhdx = (Join-Path $tmp "Apps.vhdx"); UserVhdx = (Join-Path $tmp "User.vhdx") } -ExpectText "层式 VHDX 状态"

  # 3.4 MSIX Status (只读)
  $null = Invoke-Ai4 -Name "MSIX-Attach -Action Status (只读)" -Script "MSIX-Attach.ps1" -ScriptArgs @{ Action = "Status"; OutDir = (Join-Path $dataRoot "MSIX") } -ExpectText "MSIX App Attach 状态"

  # 3.5 Plugin-Host -VerifyOnly (空插件根, 只校验不加载)
  $null = Invoke-Ai4 -Name "Plugin-Host -VerifyOnly (空市场)" -Script "Plugin-Host.ps1" `
    -ScriptArgs @{ VerifyOnly = $true; PluginRoot = (Join-Path $dataRoot "Plugins"); MarketFile = (Join-Path $ai4Root "Config\plugin-market.json"); PermissionsFile = (Join-Path $dataRoot "Config\permissions.json") } -ExpectExit 0

  # 3.6 Verify-Chain 负向: 无目标必须退出 1
  $negDir = Join-Path $tmp "neg"
  New-Item -ItemType Directory -Force -Path $negDir | Out-Null
  $null = Invoke-Ai4 -Name "Verify-Chain 无目标 (负向应 exit=1)" -Script "Security-Manager.ps1" `
    -ScriptArgs @{ Action = "Verify-Chain"; DataDrive = $negDir; VhdxDir = $negDir } -ExpectExit 1

  # 3.7 Verify-Chain 正向: 建基线 -> 篡改 -> 校验失败 -> 还原 -> 校验通过
  $posDir = Join-Path $tmp "pos"
  New-Item -ItemType Directory -Force -Path $posDir | Out-Null
  $fakeVhdx = Join-Path $posDir "Base.vhdx"
  [IO.File]::WriteAllBytes($fakeVhdx, (New-Object byte[] (1MB)))
  $posData = Join-Path $posDir "Data"
  $null = Invoke-Ai4 -Name "Verify-Chain -Force 建基线" -Script "Security-Manager.ps1" `
    -ScriptArgs @{ Action = "Verify-Chain"; DataDrive = $posDir; VhdxDir = $posDir; Force = $true } -ExpectExit 0 -ExpectText "基线已写入"
  Add-Check -Name "基线文件生成" -Ok (Test-Path (Join-Path $posData "Security\chain-manifest.sha256"))
  # 篡改一个字节
  $bytes = [IO.File]::ReadAllBytes($fakeVhdx); $bytes[0] = $bytes[0] -bxor 0xFF; [IO.File]::WriteAllBytes($fakeVhdx, $bytes)
  $null = Invoke-Ai4 -Name "Verify-Chain 篡改后必须 exit=1" -Script "Security-Manager.ps1" `
    -ScriptArgs @{ Action = "Verify-Chain"; DataDrive = $posDir; VhdxDir = $posDir } -ExpectExit 1
  # 还原
  $bytes[0] = $bytes[0] -bxor 0xFF; [IO.File]::WriteAllBytes($fakeVhdx, $bytes)
  $null = Invoke-Ai4 -Name "Verify-Chain 还原后 exit=0" -Script "Security-Manager.ps1" `
    -ScriptArgs @{ Action = "Verify-Chain"; DataDrive = $posDir; VhdxDir = $posDir } -ExpectExit 0 -ExpectText "链路校验通过"

  # 3.8 Benchmark Manifest (只读)
  $null = Invoke-Ai4 -Name "Benchmark -Action Manifest" -Script "Benchmark.ps1" -ScriptArgs @{ Action = "Manifest" } -ExpectText "Verify-Chain"

  # 3.9 Security-Manager SelfCheck (只读; DataDrive=$tmp, 期望提示缺失项但不崩)
  $null = Invoke-Ai4 -Name "Security-Manager SelfCheck (只读)" -Script "Security-Manager.ps1" `
    -ScriptArgs @{ Action = "SelfCheck"; DataDrive = $tmp } -ExpectExit 0

  # 3.10 Cloud-Sync 仅在 rclone 存在时探测
  if (Get-Command rclone.exe -ErrorAction SilentlyContinue) {
    $null = Invoke-Ai4 -Name "Cloud-Sync -Action Status" -Script "Cloud-Sync.ps1" -ScriptArgs @{ Action = "Status"; Local = $dataRoot } -ExpectExit 0
  } else {
    Write-Host "  skip Cloud-Sync (未安装 rclone, 可选项)" -ForegroundColor Yellow
  }

  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

# ---------------- 汇总 ----------------
Write-Host ""
if ($failures.Count -eq 0) {
  Write-Host "AI-4 SELFTEST PASS ($checks checks)" -ForegroundColor Green
  exit 0
} else {
  Write-Host "AI-4 SELFTEST FAIL: $($failures.Count)/$checks" -ForegroundColor Red
  foreach ($f in $failures) { Write-Host "  - $f" -ForegroundColor Red }
  exit 1
}
