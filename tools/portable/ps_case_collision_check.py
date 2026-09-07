#!/usr/bin/env python3
"""扫描 PowerShell 脚本中「仅大小写不同」的变量名。

PowerShell 变量名大小写不敏感：$Evidence 与 $evidence 是同一个变量。
本检查曾在 Accept-Gate.ps1 抓到真实缺陷 —— 脚本级 $Evidence（证据根目录）
被循环内每行的 $evidence = "" 覆盖，导致 Join-Path 收到空串。

函数参数会新建作用域，与外层同名变量不冲突，故默认排除（--strict 可显示）。
用法: python3 ps_case_collision_check.py <目录> [--strict]
"""
import sys, re, glob, collections

root = sys.argv[1] if len(sys.argv) > 1 else 'portable'
strict = '--strict' in sys.argv
bad = 0
for f in sorted(glob.glob(f'{root}/**/*.ps1', recursive=True)):
    t = open(f, encoding='utf-8-sig').read()
    code = '\n'.join(l for l in t.split('\n') if not l.strip().startswith('#'))
    # $script: 之类是作用域限定符，不是变量名
    code = re.sub(r'\$(?:script|global|local|private|using|env):', '$__scope__:', code)
    params = set()
    # 不能用非贪婪 (.*?)\) —— [Parameter(Mandatory = $true)] 里的 ) 会提前截断。
    # 手工配平括号取出完整 param(...) 内容。
    for m in re.finditer(r'param\s*\(', t):
        i = m.end(); depth = 1
        while i < len(t) and depth:
            if t[i] == '(':
                depth += 1
            elif t[i] == ')':
                depth -= 1
            i += 1
        params.update(re.findall(r'\$([A-Za-z][A-Za-z0-9_]*)', t[m.end():i - 1]))
    names = set(re.findall(r'\$([A-Za-z][A-Za-z0-9_]*)', code))
    groups = collections.defaultdict(set)
    for n in names:
        groups[n.lower()].add(n)
    for k, v in groups.items():
        if len(v) < 2:
            continue
        risky = [x for x in v if x not in params]
        if len(risky) < 2 and not strict:
            continue          # 参数 vs 外层变量：不同作用域，安全
        bad += 1
        print(f'  [风险] {f}: {sorted(v)}  (非参数: {sorted(risky)})')
print('== 未发现同名大小写冲突 ==' if bad == 0 else f'== {bad} 组需人工确认 ==')
sys.exit(1 if bad else 0)
