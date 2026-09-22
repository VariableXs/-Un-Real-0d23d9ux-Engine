# nvram-watch-readonly.ps1 — NVRAM 引导项哨兵体检（2026-09-22，严格只读）。
#
# 目的：判「U 盘 Windows 引导自愈是否已重写固件 NVRAM」——
# Windows Boot Manager 固件项的设备路径若嵌入 **U 盘 ESP 的分区 GUID**，
# 即为被重写的征兆（拔盘后 F12 只剩 PXE 的前兆）。
#
# 方法：BootOrder → 逐项读 Boot#### 原始字节 → 提取 UTF-16 描述 →
# 用 EFI 混合端序 GUID 字节匹配内置/U 盘 ESP（S1.5 内核 HandoffTarget 同一
# 字节级契约）。**零写入**：只调 GetFirmwareEnvironmentVariable 读通道。
# 需要管理员（SeSystemEnvironmentPrivilege）。
# 标记：NVRAM-WATCH-DONE（含 VERDICT 行）。
$ErrorActionPreference = 'Stop'
$log = '{log}'
$out = New-Object System.Collections.Generic.List[string]
function Say($m) { $script:out.Add($m) }

$efiVarsGuid = '{8be4df61-93ca-11d2-aa0d-00e098032b8c}'
Add-Type -Namespace Win32 -Name FirmwareEnv2 -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError=true)]
public static extern uint GetFirmwareEnvironmentVariableA(string lpName, string lpGuid, byte[] pBuffer, uint nSize);
[DllImport("ntdll.dll")]
public static extern int RtlAdjustPrivilege(int Privilege, bool bEnable, bool CurrentThread, ref bool PreviousEnabled);
'@

$prev = $false
$privRc = [Win32.FirmwareEnv2]::RtlAdjustPrivilege(19, $true, $false, [ref]$prev)
if ($privRc -ne 0) { throw ("RtlAdjustPrivilege failed rc=" + $privRc) }

function Read-EfiVar([string]$name) {
  # Get-SecureBootUEFI 为平台正规读通道（自管特权）；失败回退 raw API。
  try {
    $v = Get-SecureBootUEFI -Name $name -Namespace $efiVarsGuid -ErrorAction Stop
    return [byte[]]($v.Bytes)
  } catch {
    $buf = New-Object byte[] 4096
    $n = [Win32.FirmwareEnv2]::GetFirmwareEnvironmentVariableA($name, $efiVarsGuid, $buf, $buf.Length)
    if ($n -eq 0 -or $n -gt 4096) { return $null }
    return $buf[0..([int]($n - 1))]
  }
}

function GuidToEfiBytes([string]$g) {
  $p = $g.Trim('{}').Split('-')
  if ($p.Count -ne 5) { throw "bad guid $g" }
  $bytes = New-Object System.Collections.Generic.List[byte]
  foreach ($i in 0,1,2) {
    $s = $p[$i]
    for ($j = $s.Length - 2; $j -ge 0; $j -= 2) { $bytes.Add([Convert]::ToByte($s.Substring($j,2),16)) }
  }
  foreach ($i in 3,4) {
    $s = $p[$i]
    for ($j = 0; $j -lt $s.Length; $j += 2) { $bytes.Add([Convert]::ToByte($s.Substring($j,2),16)) }
  }
  return $bytes.ToArray()
}

function Bytes-Contains([byte[]]$hay, [byte[]]$needle) {
  if ($hay.Length -lt $needle.Length) { return $false }
  for ($i = 0; $i -le $hay.Length - $needle.Length; $i++) {
    $ok = $true
    for ($j = 0; $j -lt $needle.Length; $j++) {
      if ($hay[$i + $j] -ne $needle[$j]) { $ok = $false; break }
    }
    if ($ok) { return $true }
  }
  return $false
}

function Parse-Desc([byte[]]$b) {
  # EFI_LOAD_OPTION：u32 属性 + u16 FilePathListLength + UTF-16 描述到 U+0000。
  if ($b.Length -lt 8) { return '(short)' }
  $desc = New-Object System.Text.StringBuilder
  $i = 6
  while ($i + 1 -lt $b.Length) {
    $ch = $b[$i] -bor ($b[$i + 1] -shl 8)
    if ($ch -eq 0) { break }
    [void]$desc.Append([char]$ch)
    $i += 2
  }
  return $desc.ToString()
}

try {
  $id = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
  if (-not $id.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'must run elevated (admin)' }

  # ---- ESP GUID 基准：内置盘 + U 盘（盘不在则只有内置基准） ----
  $sysDisk = (Get-Partition -DriveLetter $env:SystemDrive.Substring(0,1)).DiskNumber
  $intEsp = Get-Partition -DiskNumber $sysDisk | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
  if ($null -eq $intEsp) { throw 'internal ESP not found' }
  $intGuid = $intEsp.Guid.Trim('{}')
  $intBytes = GuidToEfiBytes $intGuid
  Say ("internal-esp guid={0}" -f $intGuid)

  $usbDisk = $null
  foreach ($p in (Get-Partition)) {
    $v = $p | Get-Volume -ErrorAction SilentlyContinue
    if ($null -ne $v -and $v.FileSystemLabel -eq 'WIN_ENGINE') { $usbDisk = $p.DiskNumber }
  }
  $usbGuid = $null; $usbBytes = $null
  if ($null -ne $usbDisk) {
    $usbEsp = Get-Partition -DiskNumber $usbDisk | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
    if ($null -ne $usbEsp) { $usbGuid = $usbEsp.Guid.Trim('{}'); $usbBytes = GuidToEfiBytes $usbGuid }
  }
  if ($null -ne $usbGuid) { Say ("usb-esp guid={0} (disk={1})" -f $usbGuid, $usbDisk) } else { Say 'usb-esp absent (USB not plugged) - internal-only watch' }

  # ---- BootOrder → 逐项体检 ----
  $order = Read-EfiVar 'BootOrder'
  if ($null -eq $order -or $order.Length -lt 2) { throw 'BootOrder unreadable (privilege/firmware?)' }
  $nEntries = [int]($order.Length / 2)
  Say ("BootOrder entries={0}" -f $nEntries)
  $verdict = 'OK'
  for ($e = 0; $e -lt $nEntries; $e++) {
    $num = [int]$order[$e * 2] -bor ([int]$order[$e * 2 + 1] -shl 8)
    $name = 'Boot{0:X4}' -f $num
    $b = Read-EfiVar $name
    if ($null -eq $b) { Say ("{0}: UNREADABLE" -f $name); continue }
    $desc = Parse-Desc $b
    $loc = 'other'
    if (Bytes-Contains $b $intBytes) { $loc = 'internal-esp' }
    elseif ($null -ne $usbBytes -and (Bytes-Contains $b $usbBytes)) { $loc = 'usb-esp' }
    Say ("{0}: {1} -> {2}" -f $name, $desc, $loc)
    if ($loc -eq 'usb-esp' -and $desc -match 'Windows Boot Manager|Windows 启动管理器|启动管理器') {
      Say ("ALERT: {0} (Windows Boot Manager) device path points at USB ESP - firmware NVRAM was rewritten by USB Windows self-heal" -f $name)
      $verdict = 'ALERT'
    }
  }
  Say ("NVRAM-WATCH VERDICT: {0}" -f $verdict)
  if ($verdict -eq 'ALERT') {
    Say 'REMEDY (user-approved only): WinRE `bcdboot C:\Windows /s S: /f UEFI` restores internal ESP entry - never run without explicit user sign-off.'
  }
  Say 'NVRAM-WATCH-DONE'
} catch {
  Say ('NVRAM-WATCH-FAIL ' + $_.Exception.Message)
} finally {
  Set-Content -Path $log -Value $out -Encoding Unicode
}
