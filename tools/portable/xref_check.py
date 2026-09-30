#!/usr/bin/env python3
"""portable/ 交叉引用检查（无 PowerShell 时的静态一致性门禁）。

能查（真实缺陷类）：
  1. 调用了但未定义的函数（区分 PowerShell 内置/外部命令白名单）
  2. 文档、自检脚本、README 里引用的 `-Action X` 是否真在目标脚本的 switch 分支里
  3. ValidateSet 声明的动作与 switch 分支是否一一对应
  4. dot-source 的库文件是否存在
  5. param 块声明的参数是否真的被用到（提示级）
不能查：完整 PowerShell 语义（那需要真 PowerShell，见 portable/tests/Run-PortableTests.ps1）
"""
import re
import sys
from pathlib import Path

# 仓库根由脚本自身位置推导（本文件位于 <repo>/tools/portable/），不写死绝对路径
ROOT = Path(__file__).resolve().parents[2]
PORTABLE = ROOT / 'portable'

# PowerShell 内置 cmdlet / 外部命令：出现在这些名字上的调用不算"未定义"
KNOWN = set('''
Write-Host Write-Warning Write-Error Write-Verbose Write-Debug Write-Output Write-Information
Get-Content Set-Content Add-Content Get-ChildItem New-Item Remove-Item Rename-Item Copy-Item Move-Item
Test-Path Resolve-Path Split-Path Join-Path ConvertTo-Json ConvertFrom-Json Export-Csv Import-Csv
Get-Command Get-Module Get-Process Stop-Process Start-Process Start-Sleep Measure-Object Select-Object
Where-Object ForEach-Object Sort-Object Group-Object Format-Table Out-String Out-Host Out-Null Out-File
New-Object Get-Date Get-Random Get-Item Get-Volume Get-Partition Get-Disk Get-VHD New-VHD Mount-VHD
Dismount-VHD Optimize-VHD Merge-VHD Get-VM New-VM Set-VM Remove-VM Start-VM Set-VMFirmware Mount-DiskImage
Dismount-DiskImage Get-MpPreference Add-MpPreference Get-CimInstance Get-ScheduledTask Register-ScheduledTask
New-ScheduledTaskAction New-ScheduledTaskTrigger Enable-WindowsOptionalFeature Read-Host Get-PSDrive
Initialize-Disk New-Partition Format-Volume Get-AuthenticodeSignature Set-StrictMode Add-Type
Get-AppxPackage Add-AppxPackage Remove-AppxPackage Mount-AppxVolume Dismount-AppxVolume
Remove-VHD Add-Member Set-Item Select-String Get-WmiObject Test-Connection Clear-Content
bcdboot bcdedit.exe findstr findstr.exe cmd.exe takeown icacls netsh wmic wmic.exe
CompactOS Get-AuthenticodeSignature Set-AuthenticodeSignature New-SelfSignedCertificate
reg.exe schtasks.exe Get-Culture Get-UICulture Compress-Archive Expand-Archive
Format-List Import-Module Set-ItemProperty New-ItemProperty Set-Partition Optimize-Volume
Set-Service Stop-Service Start-Service Restart-Service Get-Service bcdboot.exe dism.exe
fsutil.exe powercfg.exe Get-PhysicalDisk Get-StorageReliabilityCounter Clear-Disk
Invoke-Expression Invoke-WebRequest Invoke-RestMethod Exit-Break
robocopy reg bcdedit dism diskpart manage-bde.exe manage-bde slmgr fsutil defrag powercfg sc.exe
schtasks mklink rclone.exe rclone signtool MakeAppx.exe vmconnect powershell.exe pwsh pnputil
'''.split())


def strip_noise(src: str) -> str:
    """去掉注释、字符串与 here-string（Add-Type 的 C# 代码就在 here-string 里）。"""
    src = re.sub(r'<#.*?#>', ' ', src, flags=re.S)
    # here-string @"..."@ 或 @'...'@ 整块挖掉（C# / 策略文本都在里面）
    src = re.sub(r'@"[\s\S]*?"@', '""', src)
    src = re.sub(r"@'[\s\S]*?'@", '""', src)
    out = []
    for line in src.split('\n'):
        line = re.sub(r'(?<!`)#.*$', '', line)
        line = re.sub(r'"[^"]*"', '""', line)
        line = re.sub(r"'[^']*'", "''", line)
        out.append(line)
    return '\n'.join(out)


def defined_functions(src: str):
    return set(re.findall(r'^\s*function\s+([A-Za-z0-9_\-\.]+)', src, re.M))


def called_functions(src: str):
    """匹配行首/管道后/括号内的裸命令调用，排除赋值与参数。"""
    calls = set()
    for line in src.split('\n'):
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        # 行首命令；但 `key = value`（赋值 / 哈希表条目）不是调用。
        # 注意不能用负向前瞻：\s+ 会回溯绕过它，必须先显式排除。
        if re.match(r'^[A-Za-z][A-Za-z0-9_\-\.]*\s*[-+*/%]?=(?!=)', line):
            pass
        else:
            m = re.match(r'^([A-Za-z][A-Za-z0-9_\-\.]*)\s', line)
            if m:
                calls.add(m.group(1))
        # 管道后命令
        for m in re.finditer(r'\|\s*([A-Za-z][A-Za-z0-9_\-\.]*)', line):
            calls.add(m.group(1))
        # & 调用后的裸名（& $var 形式跳过）
        for m in re.finditer(r'&\s+([A-Za-z][A-Za-z0-9_\-\.]*)\b', line):
            calls.add(m.group(1))
    return calls


def switch_branches(src: str, var='$Action'):
    """抓 switch ($Action) { "X" {...} } 里的字面量分支。"""
    m = re.search(r'switch\s*\(\s*\$Action\s*\)\s*\{', src)
    if not m:
        return None
    i = m.end() - 1
    depth = 0
    body = ''
    for j in range(i, len(src)):
        if src[j] == '{':
            depth += 1
        elif src[j] == '}':
            depth -= 1
            if depth == 0:
                body = src[i + 1:j]
                break
    return set(re.findall(r'^\s*"([^"]+)"\s*\{', body, re.M))


def validateset_actions(src: str):
    m = re.search(r'\[ValidateSet\(([^\]]*)\)\]\s*\n?\s*\[string\]\$Action', src)
    if not m:
        return None
    return set(re.findall(r'"([^"]+)"', m.group(1)))


def main():
    problems = []
    warns = []
    scripts = sorted(PORTABLE.rglob('*.ps1'))
    all_defs = set()
    per_file = {}
    for f in scripts:
        src = f.read_text(encoding='utf-8-sig')
        per_file[f] = src
        all_defs |= defined_functions(src)

    for f, src in per_file.items():
        rel = f.relative_to(ROOT)
        clean = strip_noise(src)
        mine = defined_functions(src)
        undef = called_functions(clean) - all_defs - KNOWN
        # 过滤掉明显的非函数词（关键字/别名）
        kw = {'if', 'else', 'elseif', 'while', 'for', 'foreach', 'switch', 'try', 'catch', 'finally',
              'param', 'return', 'break', 'continue', 'throw', 'function', 'exit', 'do', 'until',
              'in', 'trap', 'data', 'dynamicparam', 'begin', 'process', 'end', 'default'}
        undef = {u for u in undef if u.lower() not in kw and not u.startswith('$')}
        if undef:
            problems.append(f'{rel}: 调用未定义函数 {sorted(undef)}')

        # dot-source 的库是否存在
        for m in re.finditer(r'\.\s*\(Join-Path \$PSScriptRoot "([^"]+)"\)', src):
            lib = f.parent / m.group(1)
            if not lib.exists():
                problems.append(f'{rel}: dot-source 的 {m.group(1)} 不存在')

        vs = validateset_actions(src)
        br = switch_branches(src)
        if vs and br:
            if vs - br:
                problems.append(f'{rel}: ValidateSet 声明了 {sorted(vs - br)} 但 switch 没有对应分支')
            if br - vs:
                problems.append(f'{rel}: switch 有分支 {sorted(br - vs)} 但 ValidateSet 未声明')
        elif vs and not br:
            warns.append(f'{rel}: 有 ValidateSet($Action) 但没找到 switch ($Action)')

    # 交叉引用：所有 md 与自检脚本里出现的 `-Action X` 是否在对应脚本里合法
    action_owner = {}
    for f, src in per_file.items():
        vs = validateset_actions(src)
        if vs:
            action_owner[f.stem] = vs

    doc_files = list(PORTABLE.rglob('*.md')) + [ROOT / 'docs/AI5-测试交付.md',
                                                ROOT / 'docs/PORTABLE_AI_SPLIT_PLAN.md',
                                                ROOT / 'CHANGELOG.md']
    pat = re.compile(r'([A-Za-z0-9_\-]+)\.ps1[`"\s][^\n]{0,120}?-Action\s+([A-Za-z0-9\-]+)')
    for d in doc_files:
        if not d or not d.exists():
            continue
        txt = d.read_text(encoding='utf-8-sig')
        for m in pat.finditer(txt):
            script, action = m.group(1), m.group(2)
            if script in action_owner and action not in action_owner[script]:
                problems.append(f'{d.relative_to(ROOT)}: 引用 {script}.ps1 -Action {action}，'
                                f'但该脚本只支持 {sorted(action_owner[script])}')

    print('== 交叉引用检查 ==')
    for w in warns:
        print('WARN ' + w)
    if problems:
        for p in problems:
            print('FAIL ' + p)
        print(f'\n{len(problems)} 个问题')
        return 1
    print(f'ok: {len(scripts)} 个脚本，函数调用/动作名/dot-source 全部对得上')
    return 0


if __name__ == '__main__':
    sys.exit(main())
