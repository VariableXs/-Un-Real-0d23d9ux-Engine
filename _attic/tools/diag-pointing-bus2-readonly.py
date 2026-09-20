#!/usr/bin/env python3
r"""只读查询（第 2 轮）：本机到底有没有 PS/2 鼠标端口，触控板挂哪条总线。

第 1 轮（diag-pointing-bus-readonly.py）的结果已经很说明问题：Windows 侧
枚举不到任何 `*PNP0F*`/`*PNP03*` 设备，触控板是 `HID\SYNA2BA6`（Synaptics）。
本轮把口径放宽到**含禁用设备**，把三件事钉死：
  1. 有没有 i8042 PS/2 鼠标端口（PNP0F03/PNP0F13/PNP0F0E）与键盘端口（PNP0303）；
  2. 有没有 I2C 总线设备（触控板的常见父总线）；
  3. Synaptics 触控板（SYNA*）的完整实例清单。

判读口径：
  - 有 `*PNP0F13`（PS/2 触控板端口）→ i8042 AUX 通道可期，内核鼠标有戏；
  - 只有 `HID\SYNA*` 而没有 PNP0F → 触控板是 **HID over I2C**，内核没有
    I2C 栈 → **触控板在内核里用不了**；
  - 外接 USB 鼠标（`USB\VID_*`）= 需要 xHCI 主机控制器驱动 → 同样用不了。

全只读，不做任何改动。
"""
import subprocess

PS = r"""
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$all = Get-PnpDevice -PresentOnly:$false
Write-Output '--- 1) PS/2 端口设备（PNP0F 鼠标 / PNP03 键盘），含禁用 ---'
$hits = $all | Where-Object { $_.InstanceId -match 'PNP0F|PNP03' }
if ($hits) { $hits | ForEach-Object { Write-Output ('  ' + $_.FriendlyName + '  [' + $_.InstanceId + ']  status=' + $_.Status) } }
else { Write-Output '  (无 —— 本机未向系统暴露任何 PS/2 端口设备)' }
Write-Output ''
Write-Output '--- 2) I2C 设备 ---'
$h2 = $all | Where-Object { $_.InstanceId -match 'I2C' }
if ($h2) { $h2 | ForEach-Object { Write-Output ('  ' + $_.FriendlyName + '  [' + $_.InstanceId + ']  status=' + $_.Status) } }
else { Write-Output '  (无)' }
Write-Output ''
Write-Output '--- 3) Synaptics 触控板（SYNA*）实例 ---'
$h3 = $all | Where-Object { $_.InstanceId -match 'SYNA' }
if ($h3) { $h3 | ForEach-Object { Write-Output ('  ' + $_.FriendlyName + '  [' + $_.InstanceId + ']  status=' + $_.Status) } }
else { Write-Output '  (无)' }
Write-Output ''
Write-Output '--- 4) 外接 USB 输入设备 ---'
$h4 = $all | Where-Object { $_.InstanceId -match '^USB\\VID_' -and ($_.FriendlyName -match 'Input|Mouse|Keyboard|Receiver') }
if ($h4) { $h4 | ForEach-Object { Write-Output ('  ' + $_.FriendlyName + '  [' + $_.InstanceId + ']  status=' + $_.Status) } }
else { Write-Output '  (无)' }
"""


def main() -> int:
    p = subprocess.run(
        ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", PS],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=180,
    )
    print((p.stdout or "").strip() or "(空输出) rc=%d" % p.returncode)
    if p.stderr and p.stderr.strip():
        print("--- stderr ---")
        print(p.stderr.strip()[:1500])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
