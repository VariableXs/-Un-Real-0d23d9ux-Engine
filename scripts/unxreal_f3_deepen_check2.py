# -*- coding: utf-8 -*-
"""UNX-F3 深化轮第二段六查校验器（AI-28 · B16–B30）。全绿 exit=0。"""
import io, re, os, sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
while not os.path.isdir(os.path.join(REPO, "docs", "unxreal")):
    REPO = os.path.dirname(REPO)
D = os.path.join(REPO, "docs", "unxreal", "deepen")
B = os.path.join(REPO, "docs", "unxreal", "batches")

errors = []
dp_ids, dp_titles, dp_rows = [], {}, {}

for bno in range(16, 31):
    t = io.open(os.path.join(D, f"F3-B{bno:02d}.md"), encoding="utf-8").read()
    sk = io.open(os.path.join(B, f"UNX-F3-B{bno:02d}.md"), encoding="utf-8").read()
    # 骨架账单源：ID/标题/行数
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
        dp_titles[idn] = title
        # 查1 六要素
        for k in ("**定位**", "**语义边界**", "**依赖与嫁接源**", "**风险与回退**", "正文："):
            if k not in blk:
                errors.append(f"B{bno:02d} {fid}: 缺要素 {k}")
        # 查2 正文 ≥300 字
        bm = re.search(r"- 正文：(.+)", blk)
        if not bm or len(bm.group(1)) < 300:
            errors.append(f"B{bno:02d} {fid}: 正文不足300字")
        # 查3 判据自指
        if f"UNX-F{idn:05d}-J1" not in blk:
            errors.append(f"B{bno:02d} {fid}: 判据号不自指")
        # 查4 行数与骨架一致 + 条目名 verbatim
        rm = re.search(r"纯功能行数：(\d+) 行", blk)
        if not rm or int(rm.group(1)) != sk_map[idn][1]:
            errors.append(f"B{bno:02d} {fid}: 行数与骨架不一致")
        if title != sk_map[idn][0]:
            errors.append(f"B{bno:02d} {fid}: 条目名与骨架不一致")
    if n != 20:
        errors.append(f"B{bno:02d}: 条目数 {n} != 20")

# 查5 ID 唯一且落在 B16–B30 骨架号域（F21901–F22220，其中 F22041–F22060 属补号批 B41，留深化后续段）
s = sorted(dp_ids)
expect = [i for i in range(21901, 22221) if not (22041 <= i <= 22060)]
if s != expect:
    missing = [i for i in expect if i not in set(dp_ids)]
    errors.append(f"ID 非连续或缺号: {missing[:10]} 共{len(missing)}")
if len(set(dp_ids)) != 300:
    errors.append("存在重号")

print(f"deepen2 check: 300 entries, errors={len(errors)}")
for e in errors[:20]:
    print(" ERR:", e)
sys.exit(1 if errors else 0)
