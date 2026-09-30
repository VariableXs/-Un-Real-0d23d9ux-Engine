# -*- coding: utf-8 -*-
"""行数归一器：逐批压回 6,000 行（削减最大项，下限 200），原子回写。"""
import re, io, sys
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
BOOK = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\AI-90 · 治理线波次官 · 300项新功能增补册（B01–B15 · GOV90-001–GOV90-300）.md"
txt = open(BOOK, encoding="utf-8").read()

pat = re.compile(r"^(\| GOV90-(\d{3}) \| [^|]+ \| )(\d+)( \| 增补 \|)", re.M)
items = [(int(m.group(2)), m.start(3), m.end(3), int(m.group(3))) for m in pat.finditer(txt)]
rows = {n: v for n, _, _, v in items}
new_vals = dict(rows)
for lo in range(1, 301, 20):
    grp = [n for n in rows if lo <= n <= lo+19]
    over = sum(rows[n] for n in grp) - 6000
    while over > 0:
        # 削减当前最大且 >200 的项
        cand = sorted((n for n in grp if new_vals[n] > 200), key=lambda n: -new_vals[n])
        n = cand[0]
        cut = min(over, new_vals[n]-200, 20)
        new_vals[n] -= cut
        over -= cut
assert all(v == 6000 for b in range(1,301,20) for v in [sum(new_vals[n] for n in range(b, b+20))])

out, last = [], 0
for n, s, e, v in sorted(items, key=lambda x: x[1]):
    out.append(txt[last:s]); out.append(str(new_vals[n])); last = e
out.append(txt[last:])
open(BOOK, "w", encoding="utf-8", newline="\n").write("".join(out))
print("归一完成，15 批均 6,000")
