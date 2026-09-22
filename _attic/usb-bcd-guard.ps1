# usb-bcd-guard.ps1 — U 盘 Windows 引导自愈防护（2026-09-22，引导设施红线同源）。
#
# 背景：U 盘 Windows 被引导运行后，其引导自愈（Startup Repair）可能在
# 引导失败时重写固件 NVRAM 的 Windows Boot Manager 设备路径到 U 盘——
# 拔盘后 F12 只剩 PXE（内置盘项失效，09-22 实机实证）。
#
# 防护：对 **U 盘 ESP 的 BCD 文件**（store 参数）关闭该 OS 项的恢复环境与
# 失败修复流——自愈无从发起，NVRAM 不再被 U 盘 Windows 触碰。**零内置盘
# 写入**：内置盘 ESP 只挂盘符录哈希基线并前后复核（红线闸门），本脚本对
# 内置盘唯一动作 = 读。
#
# 项：bcdedit /store L:\EFI\Microsoft\Boot\BCD
#   /set {default} recoveryenabled No
#   /set {default} bootstatuspolicy IgnoreAllFailures
# 幂等：重复运行安全；标记 USB-BCD-GUARD-DONE。
$ErrorActionPreference = 'Stop'
$log = '{log}'
$out = New-Object System.Collections.Generic.List[string]
function Say($m) { $script:out.Add($m) }

function Get-FileHashLo([string]$path) {
  if (-not [IO.File]::Exists($path)) { return 'MISSING' }
  for ($try = 0; $try -lt 3; $try++) {
    try {
      $fs = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
      try { return (Get-FileHash -InputStream $fs -Algorithm SHA256).Hash.Substring(0,16) } finally { $fs.Dispose() }
    } catch { Start-Sleep -Seconds 2 }
  }
  return 'LOCKED'
}
function Mount-Esp([int]$disk, [int]$part, [string]$letter) {
  $dp = Join-Path $env:TEMP ('vx-guard-assign-' + $letter + '.txt')
  [IO.File]::WriteAllText($dp, "select disk $disk`r`nselect partition $part`r`nassign letter=$letter`r`n", [Text.Encoding]::ASCII)
  diskpart /s $dp | Out-Null
}
function Dismount-Esp([int]$disk, [int]$part, [string]$letter) {
  $rm = Join-Path $env:TEMP ('vx-guard-remove-' + $letter + '.txt')
  [IO.File]::WriteAllText($rm, "select disk $disk`r`nselect partition $part`r`nremove letter=$letter`r`n", [Text.Encoding]::ASCII)
  diskpart /s $rm | Out-Null
}

$mounted = @()
try {
  $id = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
  if (-not $id.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'must run elevated (admin)' }

  # ---- U 盘定位：只认 WIN_ENGINE+SHARED 标签（部署链同一契约） ----
  $usbDisk = $null
  foreach ($p in (Get-Partition)) {
    $v = $p | Get-Volume -ErrorAction SilentlyContinue
    if ($null -ne $v -and $v.FileSystemLabel -eq 'WIN_ENGINE') { $usbDisk = $p.DiskNumber }
  }
  if ($null -eq $usbDisk) { throw 'WIN_ENGINE volume not found - varix USB not plugged' }
  $labels = @()
  foreach ($p in (Get-Partition -DiskNumber $usbDisk)) {
    $v = $p | Get-Volume -ErrorAction SilentlyContinue
    if ($null -ne $v -and $v.FileSystemLabel) { $labels += $v.FileSystemLabel }
  }
  if (-not ($labels -contains 'SHARED')) { throw 'WIN_ENGINE disk has no SHARED volume - wrong disk' }
  Say ("target disk={0} labels={1}" -f $usbDisk, ($labels -join '+'))

  # ---- 内置盘 ESP 基线（只读哈希；红线闸门：结束时必须与基线一致） ----
  $sysDisk = (Get-Partition -DriveLetter $env:SystemDrive.Substring(0,1)).DiskNumber
  $intEsp = Get-Partition -DiskNumber $sysDisk | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
  if ($null -eq $intEsp) { throw 'internal ESP not found' }
  Mount-Esp $sysDisk $intEsp.PartitionNumber 'M'
  $mounted += @(,@($sysDisk, $intEsp.PartitionNumber, 'M'))
  $base = @{}
  foreach ($f in @('bootmgfw.efi','BCD','bootmgr')) { $base[$f] = Get-FileHashLo ("M:\EFI\Microsoft\Boot\" + $f) }
  Say ('internal-esp baseline: ' + (($base.GetEnumerator() | ForEach-Object { $_.Key + '=' + $_.Value }) -join ' '))

  # ---- U 盘 ESP 挂 L: + VARIX ESP 断言 ----
  $usbEsp = Get-Partition -DiskNumber $usbDisk | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
  if ($null -eq $usbEsp) { throw 'USB disk has no ESP' }
  Mount-Esp $usbDisk $usbEsp.PartitionNumber 'L'
  $mounted += @(,@($usbDisk, $usbEsp.PartitionNumber, 'L'))
  if (-not (Test-Path 'L:\EFI\BOOT\BOOTX64.EFI')) { throw 'L: has no BOOTX64.EFI - not the VARIX ESP' }
  if (-not (Test-Path 'L:\limine.conf')) { throw 'L: has no limine.conf - not the VARIX ESP' }
  $store = 'L:\EFI\Microsoft\Boot\BCD'
  if (-not (Test-Path $store)) { throw 'USB BCD missing - run esp-win-boot-provision first' }
  Say ('usb-esp mounted, BCD hash before=' + (Get-FileHashLo $store))

  # ---- 防护设置（只动 U 盘 BCD 文件；{default}=该 store 的 OS loader） ----
  $r1 = & bcdedit /store $store /set '{default}' recoveryenabled No 2>&1
  Say ('recoveryenabled: ' + (($r1 | Out-String).Trim() -replace "`r`n", ' | '))
  if ($LASTEXITCODE -ne 0) { throw "recoveryenabled failed rc=$LASTEXITCODE" }
  $r2 = & bcdedit /store $store /set '{default}' bootstatuspolicy IgnoreAllFailures 2>&1
  Say ('bootstatuspolicy: ' + (($r2 | Out-String).Trim() -replace "`r`n", ' | '))
  if ($LASTEXITCODE -ne 0) { throw "bootstatuspolicy failed rc=$LASTEXITCODE" }

  # ---- 复核：store 可枚举 + 设置落地 ----
  $enum = & bcdedit /store $store /enum '{default}' 2>&1 | Out-String
  if (($enum -notmatch 'identifier') -and ($enum -notmatch '标识符')) { throw 'post-set enum failed' }
  Say ('post-set enum: ' + (($enum | Select-String 'recoveryenabled|bootstatuspolicy') -join ' | '))
  Say ('usb-esp BCD hash after=' + (Get-FileHashLo $store))

  # ---- 红线闸门：内置盘 ESP 前后哈希一致（本脚本对内置盘零写入） ----
  foreach ($f in @('bootmgfw.efi','BCD','bootmgr')) {
    $now = Get-FileHashLo ("M:\EFI\Microsoft\Boot\" + $f)
    if ($now -ne $base[$f]) { throw ("internal ESP file changed: " + $f) }
  }
  Say 'internal-esp unchanged (hash equal)'

  Say 'USB-BCD-GUARD-DONE'
} catch {
  Say ('USB-BCD-GUARD-FAIL ' + $_.Exception.Message)
} finally {
  foreach ($m in $mounted) {
    try { Dismount-Esp $m[0] $m[1] $m[2] } catch { }
  }
  Set-Content -Path $log -Value $out -Encoding Unicode
}
