#!/usr/bin/env python3
r"""只读查询：Variable 的开机自启当前有没有开启，以及注册的命令行是什么。

背景：方案 B（不插 U 盘 / 内核交接过来）的落点是 HKCU Run 键
`VariableDesktop` —— Variable 起来后自动全屏进驻桌面。内核侧要做「A 卡
→ 交接 Windows → Variable 自动全屏」就必须确认这一项真的已开启、
且命令行的可执行文件真的存在（否则交接过去只会看到普通 Windows 桌面）。

全只读：只读注册表值 + 只做 os.path.isfile 存在性判断，不写任何东西。
"""
import os
import subprocess

PS = r"""
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$k = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
Write-Output '--- HKCU Run 键（全部值） ---'
$p = Get-ItemProperty -Path $k
if ($p) {
  $p.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
    Write-Output ('  ' + $_.Name + ' = ' + $_.Value)
  }
} else { Write-Output '  (无)' }
Write-Output ''
Write-Output '--- VariableDesktop 单项 ---'
$v = (Get-ItemProperty -Path $k -Name VariableDesktop).VariableDesktop
if ($v) { Write-Output ('  ' + $v) } else { Write-Output '  (未设置 —— 开机不会自动进 Variable)' }
Write-Output ''
Write-Output '--- 若已发布为应用，检查是否有 Variable 的 exe 正在跑 ---'
Get-Process | Where-Object { $_.ProcessName -match 'Variable' } | ForEach-Object {
  Write-Output ('  pid=' + $_.Id + ' name=' + $_.ProcessName + ' path=' + $_.Path)
}
"""


def main() -> int:
    p = subprocess.run(
        ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", PS],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120,
    )
    out = (p.stdout or "").strip()
    print(out if out else "(空输出) rc=%d" % p.returncode)
    # 命令行里如果带引号路径，检查文件是否存在（只读判断）
    for line in out.splitlines():
        if "VariableDesktop = " in line or line.strip().endswith(".exe") or ".exe" in line:
            q = line.split("=", 1)[-1].strip().strip('"')
            exe = q.split('"')[0] if q.startswith('"') else q.split(" ")[0]
            if exe.lower().endswith(".exe"):
                print(f"  -> 可执行文件存在: {os.path.isfile(exe)}  [{exe}]")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
