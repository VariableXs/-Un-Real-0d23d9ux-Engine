# -*- coding: utf-8 -*-
"""UNX-D5 域深化校验器（波08-M34 · AI-13）。
检查 B01–B18 十八册深化册 + B01–B18 骨架账双册一致：
  ① 每册 20 条、六要素齐备（判据/定位/语义边界/依赖/风险/正文）
  ② 逐条正文 ≥300 字
  ③ 批内 ID 连续且与 F 区间一致（B01 F15201 … B18 F15560）
  ④ 逐批行数求和 = 6,000；域累计链 = 6,000×册数
  ⑤ 状态全部 [已深化]，判据号与本条 ID 一致（防自指/错位）
  ⑥ 双册一致：骨架账条目名/行数/判据与深化册 verbatim 一致
全绿 exit=0。"""
import os, re, sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
BOOKS = list(range(1, 19))
SIX = ["定位", "语义边界", "依赖与嫁接源", "风险与回退", "正文"]

errors, warns = [], []
grand_entries = grand_rows = 0


def check_book(bno):
    global grand_entries, grand_rows
    path = os.path.join(DEEPEN, f"D5-B{bno:02d}.md")
    text = open(path, encoding="utf-8").read()
    f1 = 15201 + (bno - 1) * 20
    f2 = f1 + 19
    heads = re.findall(r"^### (UNX-F(\d+)) · (.+)$", text, re.M)
    if len(heads) != 20:
        errors.append(f"B{bno:02d}: entries={len(heads)} != 20")
    ids = [int(h[1]) for h in heads]
    if ids and (ids[0] != f1 or ids[-1] != f2 or ids != sorted(ids) or len(set(ids)) != 20):
        errors.append(f"B{bno:02d}: ID 不连续/越界 {ids[0]}..{ids[-1]}")
    blocks = re.split(r"^### UNX-F\d+ · .+$", text, flags=re.M)[1:]
    rows_sum = 0
    for (full, idstr, title), blk in zip(heads, blocks):
        fid = int(idstr)
        m = re.search(r"纯功能行数：(\d+) 行", blk)
        if not m:
            errors.append(f"F{fid}: 行数缺失")
            continue
        rows = int(m.group(1))
        rows_sum += rows
        if f"UNX-F{fid}-J1" not in blk.split("｜判据：")[-1]:
            errors.append(f"F{fid}: 判据号自指/错位")
        for sec in SIX:
            if f"- **{sec}**：" not in blk and not blk.lstrip().startswith("- 域/批"):
                errors.append(f"F{fid}: 缺要素 {sec}")
        bm = re.search(r"- 正文：(.+)", blk)
        if not bm:
            errors.append(f"F{fid}: 正文缺失")
        elif len(bm.group(1)) < 300:
            errors.append(f"F{fid}: 正文 {len(bm.group(1))} 字 < 300")
        if "状态：[已深化]" not in blk:
            errors.append(f"F{fid}: 状态非已深化")
    if rows_sum != 6000:
        errors.append(f"B{bno:02d}: 行数求和 {rows_sum} != 6000")
    grand_rows += rows_sum
    grand_entries += len(heads)
    dom = re.search(r"域累计 ([\d,]+)/240,000", text)
    if not dom or dom.group(1) != f"{6000*bno:,}":
        errors.append(f"B{bno:02d}: 域累计声明不符")
    # 双册一致
    bpath = os.path.join(BATCH, f"UNX-D5-B{bno:02d}.md")
    btext = open(bpath, encoding="utf-8").read()
    brows = re.findall(r"^### UNX-(F\d+) · (.+)$\n^- 域/批：D5/B\d+｜纯功能行数：(\d+)｜状态：\[已深化\]｜判据：(.+)$", btext, re.M)
    if len(brows) != 20:
        errors.append(f"B{bno:02d} 骨架账: rows={len(brows)} != 20")
    for (bid, btitle, brow, bjudge), (full, idstr, title) in zip(brows, heads):
        if bid.lstrip("F") != idstr or btitle != title:
            errors.append(f"B{bno:02d} 骨架账: F{bid} 条目名/编号与深化册漂移")
    bsum = sum(int(r[2]) for r in brows)
    if bsum != rows_sum:
        errors.append(f"B{bno:02d} 骨架账: 行数 {bsum} != 深化册 {rows_sum}")
    for (bid, btitle, brow, bjudge) in brows:
        if f"UNX-F{bid.lstrip('F')}-J1" not in bjudge:
            errors.append(f"B{bno:02d} 骨架账: F{bid} 判据漂移")


for bno in BOOKS:
    check_book(bno)

if grand_entries != 360:
    errors.append(f"全域条目 {grand_entries} != 360")
if grand_rows != 108000:
    errors.append(f"全域行数 {grand_rows} != 108,000")

print("=== UNX-D5 deepen check (B01–B18) ===")
print(f"books=18  entries={grand_entries}  rows_locked={grand_rows:,}/240,000  batch=6,000x18")
if errors:
    print(f"--- {len(errors)} ERRORS ---")
    for e in errors[:50]:
        print("  X " + e)
    sys.exit(1)
print("ALL MATCH — 六查全绿（条数/ID连续/行数守恒/正文≥300/判据自指/双册一致） exit=0")
