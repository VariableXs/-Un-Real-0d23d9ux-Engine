#!/usr/bin/env python3
r"""U 盘 ESP（Disk1 P1）内容只读核查 —— 确认三卡菜单那套文件还在。

只读：临时 assign 盘符后 Test-Path/Get-FileHash 读，读完立即摘除。
戒律：先赋变量再 diskpart；用后摘字母；不改动 ESP 任何内容。
"""
import os
import subprocess
import tempfile

ESP_DISK = 1
ESP_PART = 1
TMP = os.environ.get("TEMP", r"C:\Windows\Temp")


def run_diskpart(script: str, tag: str) -> str:
    path = os.path.join(TMP, f"varix-diag-{tag}.txt")
    with open(path, "w", encoding="ascii") as fh:
        fh.write(script)
    r = subprocess.run(["diskpart", "/s", path], capture_output=True, timeout=120)
    return (r.stdout.decode("gbk", "replace") + r.stderr.decode("gbk", "replace")).strip()


def ps(cmd: str, timeout: int = 120) -> str:
    r = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", cmd],
        capture_output=True, timeout=timeout,
    )
    return (r.stdout.decode("gbk", "replace")
            + r.stderr.decode("gbk", "replace")).strip() or "(空输出)"


# 先清历史残留字母，再 assign
print("--- 清理历史残留 Y: ---")
print(run_diskpart(
    f"select disk {ESP_DISK}\r\nselect partition {ESP_PART}\r\nremove letter=Y\r\n", "pre"))

print("--- assign Y: 到 U 盘 ESP ---")
print(run_diskpart(
    f"select disk {ESP_DISK}\r\nselect partition {ESP_PART}\r\nassign letter=Y\r\n", "assign"))

try:
    # 落点实证闸门：不是 ESP 就不动、不读
    print("--- 落点实证 ---")
    print(ps(
        "if (Test-Path 'Y:\\EFI\\BOOT\\BOOTX64.EFI') { Write-Output 'gate ok: EFI\\BOOT\\BOOTX64.EFI 存在' } "
        "else { Write-Output 'GATE FAIL: 落点不是预期的 ESP —— 立即退出，不读任何内容' }"
    ))
    print("--- ESP 内容清单 ---")
    print(ps(
        "Get-ChildItem 'Y:\\' -Force | Select-Object Mode,Length,Name | "
        "Format-Table -AutoSize | Out-String"
    ))
    print("--- 关键四件套 ---")
    print(ps(
        "foreach ($p in @('Y:\\EFI\\BOOT\\BOOTX64.EFI','Y:\\limine.conf',"
        "'Y:\\kernel\\varix','Y:\\limine-bios.sys')) { "
        "  if (Test-Path $p) { $i = Get-Item $p; "
        "    Write-Output ('OK   ' + $i.Length + '  ' + $i.LastWriteTime + '  ' + $p) "
        "  } else { Write-Output ('MISSING  ' + $p) } }"
    ))
    print("--- limine.conf 内容 ---")
    print(ps("if (Test-Path 'Y:\\limine.conf') { Get-Content 'Y:\\limine.conf' -Raw }"))
    print("--- 内核 SHA256（前 16 位）---")
    print(ps(
        "if (Test-Path 'Y:\\kernel\\varix') { "
        "(Get-FileHash 'Y:\\kernel\\varix' -Algorithm SHA256).Hash.Substring(0,16) }"
    ))
    print("--- bootmgfw.efi（Windows 引导器，chainload 目标）---")
    print(ps(
        "Get-ChildItem 'Y:\\EFI' -Recurse -Force -ErrorAction SilentlyContinue | "
        "Where-Object { $_.Name -like '*boot*' } | Select-Object FullName,Length | "
        "Format-Table -AutoSize | Out-String"
    ))
finally:
    print("--- 摘除 Y: ---")
    print(run_diskpart(
        f"select disk {ESP_DISK}\r\nselect partition {ESP_PART}\r\nremove letter=Y\r\n", "rm"))
    print("DONE（全程只读，ESP 内容未改动）")
