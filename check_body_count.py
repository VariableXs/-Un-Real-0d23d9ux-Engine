# -*- coding: utf-8 -*-
import re, sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

FILES = [
    "docs/unxreal/deepen/C2-B06.md",
    "docs/unxreal/deepen/C2-B07.md",
    "docs/unxreal/deepen/C2-B08.md",
    "docs/unxreal/deepen/C2-B09.md",
    "docs/unxreal/deepen/C2-B10.md",
]

def han_count(s):
    return sum(1 for ch in s if '\u4e00' <= ch <= '\u9fff')

total_bad = 0
for f in FILES:
    with open(f, encoding='utf-8') as fh:
        lines = fh.read().split('\n')
    print(f"== {f} ==")
    cur_id = None
    bad_list = []
    i = 0
    while i < len(lines):
        line = lines[i]
        m = re.match(r'^### (UNX-F\d+)', line)
        if m:
            cur_id = m.group(1)
        m2 = re.match(r'^- (?:\*\*)?正文(?:\*\*)?：(.*)$', line)
        if m2 and cur_id:
            # collect until blank line
            text = m2.group(1)
            j = i + 1
            while j < len(lines) and lines[j].strip() != '':
                text += lines[j]
                j += 1
            n = han_count(text)
            flag = "OK " if n >= 320 else "BAD"
            print(f"  {flag} {cur_id}: {n}")
            if n < 320:
                bad_list.append((cur_id, n))
                total_bad += 1
            cur_id = None
        i += 1
    if bad_list:
        print(f"  -> 不达标: {len(bad_list)} 条: " + ", ".join(f"{i}({n})" for i, n in bad_list))
    else:
        print("  -> 全部达标")
print(f"\n总计不达标: {total_bad} 条")
