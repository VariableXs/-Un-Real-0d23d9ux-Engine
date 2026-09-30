# -*- coding: utf-8 -*-
"""UNX-F3 深化轮第三段六查校验器（AI-28 · B31–B41 · 200 条，域深化满账收官）。全绿 exit=0。"""
import io, re, os, sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
while not os.path.isdir(os.path.join(REPO, "docs", "unxreal")):
    REPO = os.path.dirname(REPO)
D = os.path.join(REPO, "docs", "unxreal", "deepen")
B = os.path.join(REPO, "docs", "unxreal", "batches")

errors = []
dp_ids = []

for bno in range(31, 42):
    t = io.open(os.path.join(D, f"F3-B{bno:02d}.md"), encoding="utf-8").read()
    sk = io.open(os.path.join(B, f"UNX-F3-B{bno:02d}.md"), encoding="utf-8").read()
    sk_map = {}
    parts = re.split(r"^### (UNX-F(\d{5})) · (.+)$", sk, flags=re.M)
    for i in range(1, len(parts), 4):
        sk_map[int(parts[i+1])] = (parts[i+2].strip(), int(re.search(r"纯功能行数：(\d+)", parts[i+3]).group(1)))
    blocks = re.split(r"(?=^### UNX-F\d{5} · )", t, flags=re.M)
    n = 0
    for blk in blocks:
        m = re.match(r"^### (UNX-F(\d{5})) · (.+)$", blk, re.M)
        if not m:
            continue
        n += 1
        fid, idn, title = m.group(1), int(m.group(2)), m.group(3).strip()
        dp_ids.append(idn)
        for k in ("**定位**", "**语义边界**", "**依赖与嫁接源**", "**风险与回退**", "正文："):
            if k not in blk:
                errors.append(f"B{bno:02d} {fid}: 缺要素 {k}")
        bm = re.search(r"- 正文：(.+)", blk)
        if not bm or len(bm.group(1)) < 300:
            errors.append(f"B{bno:02d} {fid}: 正文不足300字")
        if f"UNX-F{idn:05d}-J1" not in blk:
            errors.append(f"B{bno:02d} {fid}: 判据号不自指")
        rm = re.search(r"纯功能行数：(\d+) 行", blk)
        if not rm or int(rm.group(1)) != sk_map[idn][1]:
            errors.append(f"B{bno:02d} {fid}: 行数与骨架不一致")
        if title != sk_map[idn][0]:
            errors.append(f"B{bno:02d} {fid}: 条目名与骨架不一致")
    expect = 10 if bno in (39, 40) else 20
    if n != expect:
        errors.append(f"B{bno:02d}: 条目数 {n} != {expect}")

expect_ids = set()
for bno in range(31, 42):
    sk = io.open(os.path.join(B, f"UNX-F3-B{bno:02d}.md"), encoding="utf-8").read()
    expect_ids |= {int(x) for x in re.findall(r"^### UNX-F(\d{5}) · ", sk, re.M)}
if set(dp_ids) != expect_ids:
    errors.append(f"ID 集合与骨架不一致: 缺{sorted(expect_ids-set(dp_ids))[:5]} 多{sorted(set(dp_ids)-expect_ids)[:5]}")
if len(set(dp_ids)) != len(dp_ids):
    errors.append("存在重号")
if len(dp_ids) != 200:
    errors.append(f"总数 {len(dp_ids)} != 200")

print(f"deepen3 check: 200 entries, errors={len(errors)}")
for e in errors[:20]:
    print(" ERR:", e)
sys.exit(1 if errors else 0)
