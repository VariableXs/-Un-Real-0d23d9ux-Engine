# -*- coding: utf-8 -*-
"""UNX-E4 增补卷校验器（AI-24 · 卷一 E01–E15 + 卷二 E16–E30 三十册）。
① 15 册、每册 20 条 ② ID UNX-E4-E001–E600 连续零跳号唯一 ③ 批批 6,000 行、两卷 180,000
④ 状态列「增补」 ⑤ 判据号与本条 ID 一致（防自指/错位） ⑥ 判据唯一性（600 枚零重复）
全绿 exit=0。
"""
import os, re, sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
NAMES = [f"E{i:02d}" for i in range(1, 31)]

errors = []
grand_entries = grand_rows = 0
seen_crit = set()

for idx, ename in enumerate(NAMES):
    path = os.path.join(BATCH, f"UNX-E4-{ename}.md")
    text = open(path, encoding="utf-8").read()
    f1 = idx * 20 + 1
    f2 = f1 + 19
    rows = re.findall(r"^\| (UNX-E4-E(\d{3})) \| ([^|]+?) \| (\d+) \| 增补 \| (.+?) \|$", text, re.M)
    if len(rows) != 20:
        errors.append(f"E{ename}: entries={len(rows)} != 20")
    ids = [int(r[1]) for r in rows]
    if ids and (ids[0] != f1 or ids[-1] != f2 or ids != sorted(ids) or len(set(ids)) != 20):
        errors.append(f"E{ename}: ID 不连续/越界 {ids[0]}..{ids[-1]}")
    rows_sum = 0
    for (eid, idn, title, rws, crit) in rows:
        fid = int(idn)
        rows_sum += int(rws)
        j1 = f"{eid}-J1"
        if not crit.startswith(j1):
            errors.append(f"{eid}: 判据号自指/错位")
        if j1 in seen_crit:
            errors.append(f"{eid}: 判据号重复")
        seen_crit.add(j1)
    if rows_sum != 6000:
        errors.append(f"E{ename}: 行数求和 {rows_sum} != 6000")
    grand_rows += rows_sum
    grand_entries += len(rows)

if grand_entries != 600:
    errors.append(f"总条目 {grand_entries} != 600")
if grand_rows != 180000:
    errors.append(f"全卷行数 {grand_rows} != 180,000")

if errors:
    print("FAIL:")
    for e in errors:
        print(" -", e)
    sys.exit(1)
print(f"ALL PASS: 30 册 / 600 条 / UNX-E4-E001–E600 连续零跳号唯一 / 批批 6,000 / 两卷 180,000 行 / 状态「增补」/ 判据 600 枚唯一")
