#!/usr/bin/env python3
r"""方案 C 开工第一步 —— 实机只读诊断（零写入）。

纪律：本脚本**只查询**，不创建/修改/删除任何文件或设置。
任何一步拿不到答案就如实记 "n/a（需提权）"，绝不猜测、绝不以读为名写入。
用于回答四个决定性问题：
  1) 这台机器现在是 UEFI 引导还是老式（Legacy/CSM）引导？
  2) SecureBoot 是开还是关？（开 = 未签名引导器必被拒）
  3) Windows 快速启动是开还是关？（开 = BCD 菜单看不见 + 硬盘休眠态）
  4) BCD 里现在有哪些引导项？U 盘在不在、ESP 内容是什么？
"""
import subprocess
import sys


def ps(cmd: str, timeout: int = 120) -> str:
    """跑一段 PowerShell 并取回文本（只读命令）。"""
    r = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", cmd],
        capture_output=True,
        timeout=timeout,
    )
    out = r.stdout.decode("gbk", "replace")
    err = r.stderr.decode("gbk", "replace")
    text = (out + err).strip()
    return text if text else "(空输出)"


def section(title: str) -> None:
    print("\n" + "=" * 66)
    print(title)
    print("=" * 66)


def main() -> int:
    section("1) 固件引导模式（决定三卡菜单能不能真切换）")
    # PEFirmwareType: 1=BIOS(Legacy)  2=UEFI  —— 只读注册表，无需提权
    print(ps(
        "$v = Get-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Control' "
        "-Name PEFirmwareType -ErrorAction SilentlyContinue;"
        "if ($v) { $m = @{1='BIOS / Legacy（老式）';2='UEFI'}; "
        "  Write-Output ('FirmwareType = ' + $v.PEFirmwareType + '  -> ' + $m[[int]$v.PEFirmwareType]) } "
        "else { Write-Output 'n/a（读不到 PEFirmwareType）' }"
    ))

    section("2) SecureBoot 状态（开 = 未签名引导器必被拒）")
    print(ps(
        "$s = Get-ItemProperty -Path "
        "'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State' "
        "-Name UEFISecureBootEnabled -ErrorAction SilentlyContinue;"
        "if ($s) { Write-Output ('UEFISecureBootEnabled = ' + $s.UEFISecureBootEnabled "
        "+ '  (1=开 0=关)') } else { Write-Output 'n/a（读不到，可能非 UEFI 或需提权）' }"
    ))

    section("3) Windows 快速启动状态（开 = 菜单看不见 + 硬盘休眠态）")
    print(ps(
        "$p = 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power';"
        "$hb = Get-ItemProperty -Path $p -Name HiberbootEnabled -ErrorAction SilentlyContinue;"
        "$he = Get-ItemProperty -Path $p -Name HibernateEnabled -ErrorAction SilentlyContinue;"
        "Write-Output ('HiberbootEnabled(快速启动) = ' + $(if($hb){$hb.HiberbootEnabled}else{'n/a'}));"
        "Write-Output ('HibernateEnabled(休眠)     = ' + $(if($he){$he.HibernateEnabled}else{'n/a'}))"
    ))

    section("4) BCD 现有引导项（读不到就标需提权，不猜）")
    print(ps("bcdedit /enum 2>&1 | Out-String"))

    section("5) BCD 固件级启动项（UEFI NVRAM 启动顺序，通常需提权）")
    print(ps("bcdedit /enum firmware 2>&1 | Out-String"))

    section("6) 磁盘与卷（U 盘在不在、分区布局）")
    print(ps(
        "Get-Disk | Select-Object Number,FriendlyName,BusType,PartitionStyle,"
        "@{n='SizeGB';e={[math]::Round($_.Size/1GB,1)}},"
        "@{n='Boot';e={$_.BootFromDisk}} | Format-Table -AutoSize | Out-String"
    ))
    print(ps(
        "Get-Volume | Where-Object { $_.DriveLetter } | "
        "Select-Object DriveLetter,FileSystemLabel,FileSystem,"
        "@{n='SizeGB';e={[math]::Round($_.Size/1GB,2)}},HealthStatus | "
        "Format-Table -AutoSize | Out-String"
    ))

    section("7) BitLocker 状态（如需恢复密钥要提前知道）")
    print(ps(
        "$c = Get-Command manage-bde -ErrorAction SilentlyContinue;"
        "if ($c) { manage-bde -status 2>&1 | Out-String } "
        "else { Write-Output 'n/a（manage-bde 不可用）' }"
    ))

    section("8) U 盘 ESP 探查（按标签定位，只读；找到才列内容）")
    # 按标签找 VARIX-ESP；找不到就列出所有可移动盘供人工判断，绝不硬写盘符
    print(ps(
        "$v = Get-Volume | Where-Object { $_.FileSystemLabel -like '*VARIX*' -or "
        "$_.FileSystemLabel -like '*ESP*' };"
        "if (-not $v) { Write-Output '未找到标签含 VARIX/ESP 的卷（U 盘可能没插）'; "
        "  Get-Volume | Where-Object { $_.DriveLetter } | Select-Object DriveLetter,"
        "FileSystemLabel | Format-Table -AutoSize | Out-String } "
        "else { foreach ($x in $v) { if ($x.DriveLetter) { "
        "  $L = $x.DriveLetter + ':'; Write-Output ('卷 ' + $L + ' 标签=' + $x.FileSystemLabel); "
        "  Write-Output ('  BOOTX64.EFI : ' + (Test-Path ($L + '\\EFI\\BOOT\\BOOTX64.EFI'))); "
        "  Write-Output ('  kernel/varix: ' + (Test-Path ($L + '\\kernel\\varix'))); "
        "  Write-Output ('  limine.conf : ' + (Test-Path ($L + '\\limine.conf'))); "
        "} } }"
    ))
    print("-- limine.conf 内容（若存在）--")
    print(ps(
        "$v = Get-Volume | Where-Object { $_.FileSystemLabel -like '*VARIX*' -or "
        "$_.FileSystemLabel -like '*ESP*' };"
        "foreach ($x in $v) { if ($x.DriveLetter) { $f = $x.DriveLetter + ':\\limine.conf'; "
        "if (Test-Path $f) { Write-Output ('--- ' + $f + ' ---'); Get-Content $f -Raw } } }"
    ))

    section("9) 已打包的 Variable 产物（决定能不能秒开，不重新编译）")
    print(ps(
        "$a='D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main\\dist-portable\\Variable.exe';"
        "$b='D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main\\src-tauri\\target\\release\\variable.exe';"
        "foreach ($p in @($a,$b)) { if (Test-Path $p) { $i=Get-Item $p; "
        "Write-Output ('OK  ' + [math]::Round($i.Length/1MB,1) + ' MB  ' + $i.LastWriteTime + '  ' + $p) } "
        "else { Write-Output ('MISSING  ' + $p) } }"
    ))

    print("\n" + "=" * 66)
    print("诊断结束（全程只读，未写入任何内容）")
    print("=" * 66)
    return 0


if __name__ == "__main__":
    sys.exit(main())
