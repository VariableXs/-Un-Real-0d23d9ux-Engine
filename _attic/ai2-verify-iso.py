# -*- coding: utf-8 -*-
"""AI-2 · ISO 完整性验证（只读）：挂载 → dism 枚举 install.wim 索引 → 卸载。

非管理员可跑（ISO 挂载与 dism /Get-WimInfo 均为只读操作）。
"""
import subprocess
import sys

ISO = r"D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso"

PS = (
    "$ErrorActionPreference='Stop'\n"
    "$iso = Mount-DiskImage -ImagePath '" + ISO + "' -PassThru\n"
    "try {\n"
    "  $dl = ($iso | Get-Volume).DriveLetter\n"
    "  Write-Output ('MOUNT=' + $dl + ':')\n"
    "  $wim = \"$($dl):\\sources\\install.wim\"\n"
    "  Write-Output ('WIM_EXISTS=' + (Test-Path -LiteralPath $wim))\n"
    "  $info = & dism /Get-WimInfo /wimFile:$wim | Out-String\n"
    "  Write-Output $info\n"
    "  $boot = Test-Path -LiteralPath \"$($dl):\\sources\\boot.wim\"\n"
    "  Write-Output ('BOOT_WIM=' + $boot)\n"
    "  $efisys = Test-Path -LiteralPath \"$($dl):\\efi\\microsoft\\boot\\efisys.bin\"\n"
    "  Write-Output ('EFISYS=' + $efisys)\n"
    "}\n"
    "finally { Dismount-DiskImage -ImagePath '" + ISO + "' | Out-Null }\n"
)

out = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
                     capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=300)
sys.stdout.write(out.stdout)
if out.stderr:
    sys.stderr.write(out.stderr[-500:])
sys.exit(out.returncode or 0)
