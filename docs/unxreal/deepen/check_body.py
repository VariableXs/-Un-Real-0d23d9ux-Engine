# -*- coding: utf-8 -*-
import re, sys, glob

files = [
    r"docs/unxreal/deepen/C2-B16.md",
    r"docs/unxreal/deepen/C2-B17.md",
    r"docs/unxreal/deepen/C2-B18.md",
    r"docs/unxreal/deepen/C2-B19.md",
    r"docs/unxreal/deepen/C2-B20.md",
]

TARGET = 320
han = re.compile(r'[\u4e00-\u9fff]')

def check(path):
    with open(path, encoding='utf-8') as f:
        lines = f.read().splitlines()
    results = []
    cur_id = None
    i = 0
    while i < len(lines):
        m = re.match(r'^### (UNX-F\d+)', lines[i])
        if m:
            cur_id = m.group(1)
        if lines[i].startswith('- **正文**：'):
            # paragraph until next blank line
            para = [lines[i]]
            j = i + 1
            while j < len(lines) and lines[j].strip() != '':
                para.append(lines[j])
                j += 1
            text = ''.join(para)
            n = len(han.findall(text))
            results.append((cur_id, n, 'FAIL' if n < TARGET else 'ok'))
            i = j
            continue
        i += 1
    return results

total_fail = 0
for path in files:
    res = check(path)
    fails = [r for r in res if r[2] == 'FAIL']
    total_fail += len(fails)
    mn = min((r[1] for r in res), default=0)
    print(f"== {path} : {len(res)} 条, 最小 {mn}, 不达标 {len(fails)} 条")
    for fid, n, st in fails:
        print(f"   {fid}: {n}")
print(f"\nTOTAL FAIL: {total_fail}")
