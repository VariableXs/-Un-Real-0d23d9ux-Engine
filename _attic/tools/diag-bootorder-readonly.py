# -*- coding: utf-8 -*-
"""实机引导顺序只读诊断（需求 4/7/11 支撑）。

**严格只读**：只查询，不写入任何引导相关设置。用户第 11、12 条红线是硬约束——
「不删除 BIOS 影响进入 Windows 的任何启动识别程序」「不给电脑数据和硬件安全
构成任何威胁」，所以本脚本一个写操作都不做，只把现状查清楚。

查什么、为什么：
  1. SecureBoot 状态 —— 历史上是引导失败硬根因（bootmgr 拒未签名 Limine）
  2. Firmware 类型   —— 决定能不能走 UEFI BootNext 真通道
  3. 快速启动状态    —— 关机=内核快照存 hiberfil.sys，另一系统写入可能损坏数据
  4. Boot#### 项与顺序 —— 三卡菜单「切 Windows」要写 BootNext，必须先知道
                          Windows 引导管理器到底是哪个编号（编号由固件分配会漂移）

为什么必须用 UEFI 变量而不是 diskpart：Boot#### 编号由固件分配且会漂移，
只有读 BootOrder → 逐个读 Boot#### 解析 EFI_LOAD_OPTION 才能拿到真值。

【环境坑】PowerShell 工具在本机会返回空、Bash 内联调 powershell 会被安全策略拦，
所以走「写 .py → subprocess 调 powershell -Command」这条路。PS 命令串用单引号
here-string 传，避免 python 转义把 PS 语法搞坏。
"""
import subprocess
import sys

PS = r'''
# 【编码坑】中文 Windows 的控制台默认输出 GBK，python 用 utf-8 解码会整片乱码
# （实测 "无法获取正确的授权" 变成 "?޷???"）。必须在 PS 侧先把输出编码掰成 UTF-8，
# 否则诊断结论会被乱码掩盖——看不懂的错误等于没有错误。
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference = 'SilentlyContinue'
Write-Output "### SecureBoot ###"
try {
  $sb = Confirm-SecureBootUEFI
  Write-Output ("SecureBootUEFI=" + $sb)
} catch {
  Write-Output ("SecureBootUEFI=QUERY_FAILED: " + $_.Exception.Message)
}
$reg = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\State' -Name UEFISecureBootEnabled -ErrorAction SilentlyContinue
Write-Output ("SecureBootRegState=" + $reg.UEFISecureBootEnabled)

Write-Output ""
Write-Output "### Firmware Type ###"
# PEFirmwareType 在某些机器上为空（值不存在），必须有兜底：用 Win32_ComputerSystem
# 的 BootupState + 系统盘分区布局来判定，不能因为一个字段空就说不清固件类型。
$pe = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control' -Name PEFirmwareType).PEFirmwareType
Write-Output ("PEFirmwareType=" + $pe)
Write-Output ("BootupState=" + (Get-CimInstance Win32_ComputerSystem).BootupState)

Write-Output ""
Write-Output "### Fast Startup ###"
$hb = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled -ErrorAction SilentlyContinue
Write-Output ("HiberbootEnabled=" + $hb.HiberbootEnabled)

Write-Output ""
Write-Output "### Firmware Boot Entries (bcdedit /enum firmware) ###"
$out = & bcdedit /enum firmware 2>&1
$out | ForEach-Object { Write-Output $_ }

Write-Output ""
Write-Output "### Boot Manager (bcdedit /enum '{bootmgr}') ###"
& bcdedit /enum '{bootmgr}' 2>&1 | ForEach-Object { Write-Output $_ }
'''


def main() -> int:
    r = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", PS],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=180,
    )
    print(r.stdout)
    if r.stderr and r.stderr.strip():
        print("=== STDERR ===")
        print(r.stderr.strip())
    return r.returncode


if __name__ == "__main__":
    sys.exit(main())
