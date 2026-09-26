# 实机引导项只读枚举（需求 4/7 支撑）——**严格只读，一个写操作都没有**。
#
# 为什么必须提权：bcdedit /enum firmware 与 Confirm-SecureBootUEFI 都要求管理员，
# 普通权限下 bcdedit 报 "The boot configuration data store could not be opened.
# Access is denied."，拿不到任何 Boot#### 数据。
#
# 红线（用户第 11、12 条）：本脚本**只读**。不新增、不删除、不修改任何引导项，
# 不碰 BIOS 设置，不写 BCD，不写 UEFI 变量。只把现状打印出来供判定。
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Write-Output "=== BCD-ENUM-READONLY BEGIN ==="

Write-Output ""
Write-Output "--- SecureBoot (authoritative) ---"
try {
  $sb = Confirm-SecureBootUEFI
  Write-Output ("Confirm-SecureBootUEFI=" + $sb)
} catch {
  Write-Output ("Confirm-SecureBootUEFI=ERR: " + $_.Exception.Message)
}
$reg = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\State' -Name UEFISecureBootEnabled
Write-Output ("UEFISecureBootEnabled=" + $reg.UEFISecureBootEnabled)

Write-Output ""
Write-Output "--- Fast Startup ---"
$hb = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled
Write-Output ("HiberbootEnabled=" + $hb.HiberbootEnabled)
$hfb = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HibernateEnabled
Write-Output ("HibernateEnabled=" + $hfb.HibernateEnabled)

Write-Output ""
Write-Output "--- Firmware Boot Entries (Boot####) ---"
# Boot#### 编号由固件分配且会漂移，只有这里能拿到真值。
$fw = & bcdedit /enum firmware 2>&1
$fw | ForEach-Object { Write-Output $_ }

Write-Output ""
Write-Output "--- Boot Manager ---"
& bcdedit /enum '{bootmgr}' 2>&1 | ForEach-Object { Write-Output $_ }

Write-Output ""
Write-Output "--- Current OS Entry ---"
& bcdedit /enum '{current}' 2>&1 | ForEach-Object { Write-Output $_ }

Write-Output ""
Write-Output "--- Boot Order (UEFI NVRAM via .NET, read-only) ---"
# 用 .NET 调 GetFirmwareEnvironmentVariable 读 BootOrder。
# 注意：普通权限下 SeSystemEnvironmentPrivilege 未启用会失败，这是预期的——
# 失败就如实打印，绝不伪造顺序。
try {
  Add-Type -Namespace Varix -Name Nv -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern uint GetFirmwareEnvironmentVariableExW(
    string lpName, string lpGuid, byte[] pBuffer, uint nSize, out uint pdwAttributes);
'@
  $buf = New-Object byte[] 512
  $attr = 0
  $rc = [Varix.Nv]::GetFirmwareEnvironmentVariableExW(
      "BootOrder", "{8BE4DF61-93CA-11D2-AA0D-00E098032B8C}", $buf, 512, [ref]$attr)
  if ($rc -eq 0) {
    Write-Output ("BootOrder=READ_FAILED GetLastError=" +
        [System.Runtime.InteropServices.Marshal]::GetLastWin32Error())
  } else {
    $nums = @()
    for ($i = 0; $i + 1 -lt $rc; $i += 2) {
      $nums += ("0x{0:X4}" -f ([uint16]($buf[$i] -bor ($buf[$i + 1] * 256))))
    }
    Write-Output ("BootOrderBytes=" + $rc)
    Write-Output ("BootOrder=" + ($nums -join ", "))
  }
} catch {
  Write-Output ("BootOrder=EXCEPTION: " + $_.Exception.Message)
}

Write-Output ""
Write-Output "=== BCD-ENUM-READONLY DONE ==="
