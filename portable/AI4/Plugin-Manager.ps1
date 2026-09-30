param(
  [ValidateSet("Init", "Add", "Remove", "List", "Verify", "Install")]
  [string]$Action = "List",
  [string]$Id = "",
  [string]$Version = "1.0.0",
  [string]$Entry = "",
  [string]$DllPath = "",
  [string]$Permissions = "spawn",
  [string]$Signature = "",
  [switch]$Force
)
# AI-4 拓展核 / 第14.1章 插件市场
#   Data/Plugins/market.json 为插件清单, Core 启动校验 SHA256 + 签名, 权限走 JobObject。
# 用法:
#   .\Plugin-Manager.ps1 -Action Init
#   .\Plugin-Manager.ps1 -Action Add -Id com.variable.wallpaper -Version 2.1.0 -Entry wallpaper.dll -Permissions "spawn,overlay"
#   .\Plugin-Manager.ps1 -Action Install -Id com.variable.wallpaper -DllPath D:\dl\wallpaper.dll -Signature 官方证书指纹
#   .\Plugin-Manager.ps1 -Action List
#   .\Plugin-Manager.ps1 -Action Verify
#   .\Plugin-Manager.ps1 -Action Remove -Id com.variable.wallpaper
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$PluginRoot = "D:\Data\Plugins"
$MarketFile  = "$PSScriptRoot\Config\plugin-market.json"
$PluginsDir  = Join-Path $PluginRoot "installed"
New-Item -ItemType Directory -Force -Path $PluginsDir | Out-Null

function Read-Market {
  if (Test-Path $MarketFile) {
    $m = Get-Content $MarketFile -Raw | ConvertFrom-Json
    if (-not ($m.PSObject.Properties.Name -contains "Plugins")) { $m | Add-Member -NotePropertyName Plugins -NotePropertyValue @() -Force }
    if (-not $m.Plugins) { $m.Plugins = @() }
    return $m
  }
  return [pscustomobject]@{ Plugins = @() }
}
function Write-Market { param($M) ; Set-Content -Path $MarketFile -Value ($M | ConvertTo-Json -Depth 8) -Encoding UTF8 }

function Init-Market {
  $m = Read-Market
  if ($m.Plugins -and $m.Plugins.Count -gt 0) {
    if (-not $Force) { throw "market.json 已存在插件, 可用 -Force 重置" }
  }
  $m = [pscustomobject]@{
    schema  = "https://variable-os.dev/plugin-market/1.0"
    updated = (Get-Date -Format o)
    plugins = @()
  }
  Write-Market $m
  Write-Host ">>> 已初始化 $MarketFile" -ForegroundColor Green
}

function Add-MarketPlugin {
  if (-not $Id) { throw "-Id 必填" }
  if (-not $Entry) { $Entry = "$Id.dll" }
  $m = Read-Market
  $permList = ($Permissions -split "," | ForEach-Object { $_.Trim() }) | Where-Object { $_ }
  $rec = [pscustomobject]@{
    id          = $Id
    version     = $Version
    entry       = $Entry
    permissions = @($permList)
    publisher   = ""
    sha256      = ""
    signature   = $Signature
  }
  $existing = $m.Plugins | Where-Object { $_.id -eq $Id }
  if ($existing) {
    if (-not $Force) { throw "插件已存在: $Id ; 用 -Force 更新" }
    $m.Plugins = @($m.Plugins | Where-Object { $_.id -ne $Id })
  }
  $m.Plugins += $rec
  $m.updated = Get-Date -Format o
  Write-Market $m
  Write-Host ">>> 已登记插件 $Id v$Version -> $Entry (permissions: $($permList -join ','))" -ForegroundColor Green
  Write-Host "    下一步: 放置 DLL 到 $PluginRoot\, 然后 -Action Verify 计算 sha256" -ForegroundColor Yellow
}

function Set-PluginHash {
  param($Plugin)
  $dll = Join-Path $PluginRoot $Plugin.entry
  if (-not (Test-Path $dll)) { throw "入口不存在: $dll" }
  $Plugin | Add-Member -NotePropertyName sha256 -NotePropertyValue (Get-FileHash $dll -Algorithm SHA256).Hash -Force
}

function Install-MarketPlugin {
  if (-not $Id) { throw "-Id 必填" }
  if (-not $DllPath) { throw "-DllPath 必填" }
  if (-not (Test-Path $DllPath)) { throw "DLL 不存在: $DllPath" }
  $m = Read-Market
  $plugin = $m.Plugins | Where-Object { $_.id -eq $Id } | Select-Object -First 1
  if (-not $plugin) {
    Write-Host ">>> 清单中不存在 $Id, 自动登记" -ForegroundColor Yellow
    if (-not $Entry) { $Entry = "$Id.dll" }
    $plugin = [pscustomobject]@{ id=$Id; version=$Version; entry=$Entry; permissions=@(($Permissions -split ',') | ForEach-Object { $_.Trim() } | Where-Object {$_}); publisher=""; sha256=""; signature=$Signature }
    $m.Plugins += $plugin
  }
  $entry = $plugin.entry
  if (-not $entry) { $entry = [IO.Path]::GetFileName($DllPath) }
  $dest = Join-Path $PluginRoot $entry
  Copy-Item $DllPath $dest -Force
  $plugin.sha256 = (Get-FileHash $dest -Algorithm SHA256).Hash
  $sig = Get-AuthenticodeSignature $dest
  if ($sig.Status -eq "Valid") { $plugin.signature = $sig.SignerCertificate.Thumbprint }
  $m.updated = Get-Date -Format o
  Write-Market $m
  Write-Host ">>> 已安装 $Id -> $dest" -ForegroundColor Green
  Write-Host "    SHA256: $($plugin.sha256)" -ForegroundColor Green
  Write-Host "    签名:   $(if($plugin.signature){$plugin.signature}else{'未签名'})" -ForegroundColor Yellow
}

function Remove-MarketPlugin {
  if (-not $Id) { throw "-Id 必填" }
  $m = Read-Market
  $plugin = $m.Plugins | Where-Object { $_.id -eq $Id } | Select-Object -First 1
  if ($plugin) {
    $entry = Join-Path $PluginRoot $plugin.entry
    if (Test-Path $entry -and -not $Force) {
      Write-Host ">>> 提示: 安装目录中存在 $entry (自动删除请加 -Force)" -ForegroundColor Yellow
    }
    if ($Force -and (Test-Path $entry)) { Remove-Item $entry -Force -ErrorAction SilentlyContinue }
    $m.Plugins = @($m.Plugins | Where-Object { $_.id -ne $Id })
    $m.updated = Get-Date -Format o
    Write-Market $m
    Write-Host ">>> 已移除插件 $Id" -ForegroundColor Green
  } else {
    throw "未找到插件: $Id"
  }
}

function List-MarketPlugin {
  $m = Read-Market
  if (-not $m.Plugins -or $m.Plugins.Count -eq 0) { Write-Host "无插件"; return }
  $m.Plugins | Select-Object id, version, entry, @{n="permissions";e={$_.permissions -join ","}}, publisher, @{n="sha";e={$_.sha256.Substring(0,12)}}, signature | Format-Table -AutoSize
}

function Verify-MarketPlugin {
  $m = Read-Market
  if (-not $m.Plugins -or $m.Plugins.Count -eq 0) { Write-Host "无插件"; return }
  $bad = @()
  foreach ($p in $m.Plugins) {
    $entry = Join-Path $PluginRoot $p.entry
    if (-not (Test-Path $entry)) { Write-Warning "缺失: $($p.id) $entry"; $bad += $p.id; continue }
    $hash = (Get-FileHash $entry -Algorithm SHA256).Hash
    if ($p.sha256 -and $hash -ne $p.sha256) { Write-Warning "哈希不符: $($p.id)"; $bad += $p.id; continue }
    $sig = Get-AuthenticodeSignature $entry
    if ($sig.Status -ne "Valid") { Write-Warning "签名无效: $($p.id) ($($sig.Status))"; $bad += $p.id; continue }
    Write-Host "OK: $($p.id) $($p.entry)  $($hash.Substring(0,12))...  签名:$($sig.SignerCertificate.Thumbprint)" -ForegroundColor Green
  }
  if ($bad.Count) { Write-Warning ">>> $($bad.Count) 个插件未通过校验: $($bad -join ', ')" } else { Write-Host ">>> 全部插件校验通过" -ForegroundColor Green }
}

switch ($Action) {
  "Init"    { Init-Market }
  "Add"     { Add-MarketPlugin }
  "Install" { Install-MarketPlugin }
  "Remove"  { Remove-MarketPlugin }
  "List"    { List-MarketPlugin }
  "Verify"  { Verify-MarketPlugin }
}
