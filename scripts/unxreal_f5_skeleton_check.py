# -*- coding: utf-8 -*-
"""UNX-F5 首产段机械校验器（AI-30 · 波09）五查：
①批册 15 册齐装、逐册 20 条 ②ID F23201–F23500 连续唯一 ③行数守恒批批 6,000/总 90,000
④判据 J1 齐备 300/300 ⑤汇编册域节与批册一致 + 防重（与 E 增补卷判据文本零重复抽样）
"""
import re, sys, glob

BATCH_DIR = "docs/unxreal/batches"
MD = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
fails = []

# ①②③④ 批册
ids, total = [], 0
files = sorted(glob.glob(f"{BATCH_DIR}/UNX-F5-B*.md"))
if len(files) != 15:
    fails.append(f"批册数 {len(files)} != 15")
for fp in files:
    txt = open(fp, encoding="utf-8").read()
    bid = re.search(r"UNX-F5-(B\d{2})", fp).group(1)
    rows = re.findall(r"^### UNX-F(\d+) · (.+)$\n- 域/批：F5/"+bid+r"｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F\1-J1 (.+)$", txt, re.M)
    if len(rows) != 20:
        fails.append(f"{bid} 条数 {len(rows)} != 20")
    s = sum(int(r[2]) for r in rows)
    if s != 6000:
        fails.append(f"{bid} 行数和 {s} != 6000")
    total += s
    for r in rows:
        ids.append(int(r[0]))
        if not r[3].strip():
            fails.append(f"F{r[0]} 判据为空")
if ids != list(range(23201, 23501)):
    fails.append("ID 区间非 F23201–F23500 连续唯一")
if total != 90000:
    fails.append(f"总轧 {total} != 90000")

# ⑤ 汇编册域节一致
md = open(MD, encoding="utf-8").read()
md_rows = re.findall(r"^\| UNX-F(2\d{4}) \| (.+?) \| (\d+) \| 骨架 \| UNX-F\1-J1 (.+?) \|$", md, re.M)
md_ids = [int(x[0]) for x in md_rows if 23201 <= int(x[0]) <= 23500]
if md_ids != list(range(23201, 23501)):
    fails.append(f"汇编册域节 ID {len(md_ids)} != 300 连续")
# 抽样 30 条与批册逐字一致
import random
random.seed(42)
sample = random.sample(range(23201, 23501), 30)
for fid in sample:
    mrow = [x for x in md_rows if int(x[0]) == fid][0]
    brow = [r for r in (re.findall(r"^### UNX-F(\d+) · (.+)$\n- 域/批：F5/B\d+｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F\d+-J1 (.+)$", open(f"{BATCH_DIR}/UNX-F5-B{(fid-23200+19)//20:02d}.md", encoding="utf-8").read(), re.M)) if int(r[0]) == fid][0]
    if mrow[1] != brow[1] or mrow[2] != brow[2] or mrow[3] != brow[3]:
        fails.append(f"F{fid} 汇编与批册不一致")

# 防重：与 E 增补卷判据文本零重复（整行判据级）
e_crit = set(re.findall(r"UNX-F5-E\d{3}-J1 (.+?) \|", md, re.M))
b_crit = [x[3] for x in md_rows if 23201 <= int(x[0]) <= 23500]
dup = [c for c in b_crit if c in e_crit]
if dup:
    fails.append(f"与 E 增补卷判据重复 {len(dup)} 条")

if fails:
    print("FAIL", len(fails))
    [print(" -", f) for f in fails[:20]]
    sys.exit(1)
print("unxreal_f5_skeleton_check: 五查 ALL PASS exit 0（15 册/300 条/90,000 行/判据 300/汇编一致/防重零重复）")
