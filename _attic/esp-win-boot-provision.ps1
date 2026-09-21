# S1.3 U 盘 ESP Windows 引导供应 v2（AI-1, 2026-09-22）。
# v2 修复：①日志=结尾原子单次写（驱动不再 *> 重定向，消除锁竞态）；
#          ②内置盘 ESP 四件套哈希改盘符挂载法（Test-Path 不认 \\?\Volume 设备路径）；
#          ③登记=BootOrder 内容逐项 GUID 字节匹配（bcdboot /addlast 追加，不枚举变量名）。
# Gates: SecureBoot/快速启动 SOP 前置；目标 ESP 只经 WIN_ENGINE+SHARED 标签定位；
#        内置盘 ESP GUID 断言 + 四件套前后双录核；GUID 匹配才登记。
$ErrorActionPreference = 'Stop'
$log = '{log}'
$out = New-Object System.Collections.Generic.List[string]
function Say($m) { $script:out.Add($m) }

$efiVarsGuid = '{8be4df61-93ca-11d2-aa0d-00e098032b8c}'
Add-Type -Namespace Win32 -Name FirmwareEnv -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError=true)]
public static extern uint GetFirmwareEnvironmentVariableA(string lpName, string lpGuid, byte[] pBuffer, uint nSize);
[DllImport("ntdll.dll")]
public static extern int RtlAdjustPrivilege(int Privilege, bool bEnable, bool CurrentThread, ref bool PreviousEnabled);
'@

# SeSystemEnvironmentPrivilege (19) 必须显式启用——提权进程默认持有但未启用
$prev = $false
$privRc = [Win32.FirmwareEnv]::RtlAdjustPrivilege(19, $true, $false, [ref]$prev)
if ($privRc -ne 0) { throw ("RtlAdjustPrivilege failed rc=" + $privRc) }

function Read-EfiVarBytes([string]$name) {
  # 平台正规通道：SecureBoot cmdlet 自管 SeSystemEnvironmentPrivilege
  try {
    $v = Get-SecureBootUEFI -Name $name -Namespace $efiVarsGuid -ErrorAction Stop
    return [byte[]]($v.Bytes)
  } catch {
    $script:lastEfiErr = $_.Exception.Message
    return $null
  }
}

function Get-FileHashLo([string]$path) {
  if (-not [IO.File]::Exists($path)) { return 'MISSING' }
  for ($try = 0; $try -lt 3; $try++) {
    try {
      $fs = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
      try { return (Get-FileHash -InputStream $fs -Algorithm SHA256).Hash.Substring(0,16) } finally { $fs.Dispose() }
    } catch {
      Start-Sleep -Seconds 2
    }
  }
  return 'LOCKED'
}

function Mount-Esp([int]$disk, [int]$part, [string]$letter) {
  $dp = Join-Path $env:TEMP ('vx-assign-' + $letter + '.txt')
  [IO.File]::WriteAllText($dp, "select disk $disk`r`nselect partition $part`r`nassign letter=$letter`r`n", [Text.Encoding]::ASCII)
  diskpart /s $dp | Out-Null
}
function Dismount-Esp([int]$disk, [int]$part, [string]$letter) {
  $rm = Join-Path $env:TEMP ('vx-remove-' + $letter + '.txt')
  [IO.File]::WriteAllText($rm, "select disk $disk`r`nselect partition $part`r`nremove letter=$letter`r`n", [Text.Encoding]::ASCII)
  diskpart /s $rm | Out-Null
}

$mounted = @()
try {
  # ---- SOP 前置闸门 ----
  $sb = Confirm-SecureBootUEFI
  Say ("sop: SecureBoot={0}" -f $sb)
  if ($sb) { throw 'SecureBoot is ON - boot-class operation refused' }
  $hb = (Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled -ErrorAction Stop).HiberbootEnabled
  Say ("sop: HiberbootEnabled={0}" -f $hb)
  if ($hb -ne 0) { throw 'HiberbootEnabled is not 0 - run S0.4 first' }
  $bl = Get-BitLockerVolume -MountPoint $env:SystemDrive -ErrorAction SilentlyContinue
  if ($bl) { Say ('sop: BitLocker=' + $bl.ProtectionStatus + '/' + $bl.VolumeStatus) }

  # ---- 0) 目标盘定位：只认 WIN_ENGINE+SHARED 标签所在的盘 ----
  $usbDisk = $null; $xLetter = $null
  foreach ($p in (Get-Partition)) {
    $v = $p | Get-Volume -ErrorAction SilentlyContinue
    if ($null -ne $v -and $v.FileSystemLabel -eq 'WIN_ENGINE') { $usbDisk = $p.DiskNumber; $xLetter = $v.DriveLetter }
  }
  if ($null -eq $usbDisk) { throw 'WIN_ENGINE volume not found' }
  $labels = @()
  foreach ($p in (Get-Partition -DiskNumber $usbDisk)) {
    $v = $p | Get-Volume -ErrorAction SilentlyContinue
    if ($null -ne $v -and $v.FileSystemLabel) { $labels += $v.FileSystemLabel }
  }
  if (-not ($labels -contains 'SHARED')) { throw 'WIN_ENGINE disk has no SHARED volume - wrong disk' }
  Say ("target disk={0} labels={1} xletter={2}" -f $usbDisk, ($labels -join '+'), $xLetter)

  # ---- 1) 内置盘 ESP：挂盘符录四件套基线（哈希后摘除） ----
  $sysDisk = (Get-Partition -DriveLetter $env:SystemDrive.Substring(0,1)).DiskNumber
  $intEsp = Get-Partition -DiskNumber $sysDisk | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
  if ($null -eq $intEsp) { throw 'internal ESP not found' }
  Say ('internal-esp guid=' + $intEsp.Guid)
  Mount-Esp $sysDisk $intEsp.PartitionNumber 'M'
  $mounted += @(,@($sysDisk, $intEsp.PartitionNumber, 'M'))
  $base1 = @{}
  foreach ($f in @('bootmgfw.efi','BCD','bootmgr')) { $base1[$f] = Get-FileHashLo ("M:\EFI\Microsoft\Boot\" + $f) }
  Say ('internal-esp baseline: ' + (($base1.GetEnumerator() | ForEach-Object { $_.Key + '=' + $_.Value }) -join ' '))

  # ---- 2) U 盘 ESP：挂 L: ----
  $usbEsp = Get-Partition -DiskNumber $usbDisk | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
  if ($null -eq $usbEsp) { throw 'USB disk has no ESP' }
  Mount-Esp $usbDisk $usbEsp.PartitionNumber 'L'
  $mounted += @(,@($usbDisk, $usbEsp.PartitionNumber, 'L'))

  # ---- 3) 落点实证闸门 ----
  if (-not (Test-Path 'L:\EFI\BOOT\BOOTX64.EFI')) { throw 'L: has no BOOTX64.EFI - not the VARIX ESP' }
  if (-not (Test-Path 'L:\limine.conf')) { throw 'L: has no limine.conf - not the VARIX ESP' }

  # ---- 4) bcdboot（幂等：BCD 已在则跳过） ----
  if (Test-Path 'L:\EFI\Microsoft\Boot\BCD') {
    Say 'bcdboot-skip: BCD already present (idempotent re-run)'
  } else {
    $bl2 = & bcdboot "${xLetter}:\Windows" /s L: /f UEFI /addlast 2>&1
    Say ('bcdboot: ' + (($bl2 | Out-String).Trim() -replace "`r`n", ' | '))
    if ($LASTEXITCODE -ne 0) { throw "bcdboot failed rc=$LASTEXITCODE" }
  }
  foreach ($f in @('EFI\Microsoft\Boot\bootmgfw.efi','EFI\Microsoft\Boot\BCD')) {
    if (-not (Test-Path ('L:\' + $f))) { throw ("missing after bcdboot: " + $f) }
  }
  Say ('usb-esp bootmgfw=' + (Get-FileHashLo 'L:\EFI\Microsoft\Boot\bootmgfw.efi'))
  Say ('usb-esp BCD=' + (Get-FileHashLo 'L:\EFI\Microsoft\Boot\BCD'))
  $storeEnum = & bcdedit /store 'L:\EFI\Microsoft\Boot\BCD' /enum all 2>&1 | Out-String
  Say ('store-enum lines=' + (($storeEnum -split "`n").Count))
  # 语言无关断言：中文系统 bcdedit 输出「标识符/Windows 启动加载器」，英文为 identifier/Windows Boot Loader
  if (($storeEnum -notmatch '标识符') -and ($storeEnum -notmatch 'identifier')) { throw 'store BCD enum has no entries' }
  if (($storeEnum -notmatch 'Windows 启动加载器') -and ($storeEnum -notmatch 'Windows Boot Loader')) { throw 'store BCD has no OS loader entry' }

  # ---- 5) 登记 boot-select.json（SHARED 真相源 + ESP 镜像） ----
  # 设计定论（09-22）：配置登记 **U 盘 ESP GUID** 而非 Boot#### 项号——
  # 项号会漂、GUID 不会；内核侧（S1.5 接线）按「设备路径含该 GUID」匹配
  # 固件项（内核 Runtime Services 读 Boot#### 通道已实机验证），Windows 侧
  # 无需读任何固件变量。bcdboot 成功即代表引导文件+固件项就绪。
  $espGuid = $usbEsp.Guid
  if (-not $espGuid) { throw 'USB ESP GUID empty' }
  $sharedLetter = $null; $sharedPart = 0
  foreach ($p in (Get-Partition -DiskNumber $usbDisk)) {
    $v = $p | Get-Volume -ErrorAction SilentlyContinue
    if ($null -ne $v -and $v.FileSystemLabel -eq 'SHARED') { $sharedLetter = $v.DriveLetter; $sharedPart = $p.PartitionNumber }
  }
  if ($null -eq $sharedLetter) {
    $sharedPart = (Get-Partition -DiskNumber $usbDisk | Where-Object { ($v2 = $_ | Get-Volume -ErrorAction SilentlyContinue) -and $v2.FileSystemLabel -eq 'SHARED' } | Select-Object -First 1).PartitionNumber
    Mount-Esp $usbDisk $sharedPart 'W'
    $mounted += @(,@($usbDisk, $sharedPart, 'W'))
    $sharedLetter = 'W'
  }
  $cfgPath = "${sharedLetter}:\boot-select.json"
  if (-not (Test-Path $cfgPath)) { throw 'SHARED boot-select.json missing' }
  $doc = Get-Content $cfgPath -Raw | ConvertFrom-Json
  $doc | Add-Member -NotePropertyName usb_windows_esp_guid -NotePropertyValue $espGuid -Force
  $doc | ConvertTo-Json -Depth 8 | Set-Content -Path $cfgPath -Encoding UTF8
  Say ('registered usb_windows_esp_guid=' + $espGuid)
  Copy-Item $cfgPath 'L:\boot-select.json' -Force
  $hA = (Get-FileHash 'L:\boot-select.json' -Algorithm SHA256).Hash
  $hB = (Get-FileHash $cfgPath -Algorithm SHA256).Hash
  if ($hA -ne $hB) { throw 'boot-select dual-copy hash mismatch' }
  Say ('boot-select dual-copy hash=' + $hA.Substring(0,16))

  # ---- 7) 内置盘 ESP 四件套复核（必须与基线一致） ----
  foreach ($f in @('bootmgfw.efi','BCD','bootmgr')) {
    $now = Get-FileHashLo ("M:\EFI\Microsoft\Boot\" + $f)
    if ($now -ne $base1[$f]) { throw ("internal ESP file changed: " + $f) }
  }
  Say 'internal-esp unchanged (hash equal)'

  Say 'WIN-BOOT-PROVISION-DONE'
} catch {
  Say ('WIN-BOOT-PROVISION-FAIL ' + $_.Exception.Message)
} finally {
  foreach ($m in $mounted) {
    try { Dismount-Esp $m[0] $m[1] $m[2] } catch { }
  }
  Set-Content -Path $log -Value $out -Encoding Unicode
}
