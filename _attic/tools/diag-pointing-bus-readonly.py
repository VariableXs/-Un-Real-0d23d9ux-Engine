#!/usr/bin/env python3
r"""只读查询本机指向设备（鼠标/触控板）挂在哪条总线上。

为什么查这个：内核目前**只有 i8042 PS/2 AUX 一条鼠标通路**，既没有 xHCI
（USB 主机控制器）真驱动，也没有 I2C/SMBus 驱动。所以"触控板能不能用"
完全取决于它挂在哪条总线上：
  - `*PNP0F03` / `*PNP0F0E` / `*PNP0F13`  → PS/2（i8042 AUX）→ **能用**
  - `ACPI\...` + 父设备为 I2C/SPI        → I2C HID          → **用不了（需 I2C 栈）**
  - `USB\VID_...`                        → USB HID          → **用不了（需 xHCI 栈）**

全只读：只枚举设备 ID、服务名与父总线关系，不做任何改动。
"""
import subprocess
import sys

PS = r"""
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Write-Output '===== 指向设备（Win32_PointingDevice） ====='
Get-CimInstance Win32_PointingDevice | ForEach-Object {
  Write-Output ('NAME   : ' + $_.Name)
  Write-Output ('DEVID  : ' + $_.DeviceID)
  Write-Output ('PNP    : ' + $_.PNPDeviceID)
  Write-Output ('IFACE  : ' + $_.HardwareType)
  Write-Output ''
}
Write-Output '===== 名称含 TouchPad/Mouse/Pointing 的 PnP 实体 ====='
Get-CimInstance Win32_PnPEntity | Where-Object {
  $_.Name -match 'TouchPad|Touchpad|Touch Pad|Mouse|Pointing|HID-compliant'
} | ForEach-Object {
  Write-Output ('NAME   : ' + $_.Name)
  Write-Output ('DEVID  : ' + $_.DeviceID)
  Write-Output ('SERVICE: ' + $_.Service)
  Write-Output ''
}
Write-Output '===== i8042 / PS2 控制器 ====='
Get-CimInstance Win32_PnPEntity | Where-Object {
  $_.DeviceID -match 'PNP0F03|PNP0F13|PNP0F0E|PNP0303|i8042'
} | ForEach-Object {
  Write-Output ('NAME   : ' + $_.Name)
  Write-Output ('DEVID  : ' + $_.DeviceID)
  Write-Output ('STATUS : ' + $_.Status)
  Write-Output ''
}
Write-Output '===== I2C / SPI 控制器（触控板常见父总线） ====='
Get-CimInstance Win32_PnPEntity | Where-Object {
  $_.DeviceID -match 'I2C|SPI' -and $_.Name -match 'Controller|Host'
} | ForEach-Object {
  Write-Output ('NAME   : ' + $_.Name)
  Write-Output ('DEVID  : ' + $_.DeviceID)
  Write-Output ''
}
"""


def main() -> int:
    p = subprocess.run(
        ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", PS],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120,
    )
    out = (p.stdout or "").strip()
    print(out if out else "(空输出) rc=%d" % p.returncode)
    if p.stderr:
        err = p.stderr.strip()
        if err:
            print("--- stderr ---")
            print(err[:2000])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
