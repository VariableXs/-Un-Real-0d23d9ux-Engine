# -*- coding: utf-8 -*-
"""UNX-F1 域深化校验器（AI-26 · 深化 B01–B30 · 600 条）。
检查 deepen/F1-B01..B30.md 三十册：
  ① 每册 20 条、六要素齐备（判据/定位/语义边界/依赖与嫁接源/风险与回退/正文）
  ② 逐条正文 ≥300 字（单行计量，与 D5 口径一致）
  ③ 批内 ID 连续且与骨架账区间一致（B01 F20001 … B30 F20581–F20600）
  ④ 逐批行数求和 = 骨架账同批求和（深化零改行数）；域累计 = 骨架 161,460
  ⑤ 状态全部 [已深化]，判据号 UNX-F{id}-J1 与本条 ID 自指一致
  ⑥ 双册一致：骨架账条目名与深化册 verbatim 一致
全绿 exit=0。"""
import os, re, sys, glob

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
BOOKS = range(1, 31)
SIX = ["定位", "语义边界", "依赖与嫁接源", "风险与回退"]  # 正文由专用正则查存在性与 ≥300 字

errors = []
grand_entries = grand_rows = 0

def parse_skeleton_titles():
    sk = {}
    for f in sorted(glob.glob(os.path.join(BATCH, "UNX-F1-B*.md"))):
        t = open(f, encoding="utf-8").read()
        for full, idn, title in re.findall(r"^### (UNX-F(\d{5})) · (.+)$", t, re.M):
            sk[int(idn)] = (title.strip(), int(re.search(r"纯功能行数：(\d+)", t[t.find(full):]).group(1)))
    return sk

SK = parse_skeleton_titles()

def check_book(bno):
    global grand_entries, grand_rows
    path = os.path.join(DEEPEN, f"F1-B{bno:02d}.md")
    text = open(path, encoding="utf-8").read()
    heads = re.findall(r"^### (UNX-F(\d{5})) · (.+)$", text, re.M)
    if len(heads) != 20:
        errors.append(f"B{bno:02d}: entries={len(heads)} != 20")
    ids = [int(h[1]) for h in heads]
    lo, hi = 20001 + (bno - 1) * 20, 20020 + (bno - 1) * 20
    if ids and (ids[0] != lo or ids[-1] != hi or ids != sorted(ids) or len(set(ids)) != 20):
        errors.append(f"B{bno:02d}: ID 不连续/越界 {ids[0]}..{ids[-1]}")
    blocks = re.split(r"^### UNX-F\d{5} · .+$", text, flags=re.M)[1:]
    rows_sum = 0
    for (full, idstr, title), blk in zip(heads, blocks):
        fid = int(idstr)
        m = re.search(r"纯功能行数：(\d+)", blk)
        if not m:
            errors.append(f"F{fid}: 行数缺失"); continue
        rows = int(m.group(1)); rows_sum += rows
        if f"UNX-F{fid}-J1" not in blk.split("｜判据：")[-1]:
            errors.append(f"F{fid}: 判据号自指/错位")
        for sec in SIX:
            if f"- **{sec}**：" not in blk:
                errors.append(f"F{fid}: 缺要素 {sec}")
        bm = re.search(r"- 正文：(.+)", blk)
        if not bm:
            errors.append(f"F{fid}: 正文缺失")
        elif len(bm.group(1)) < 300:
            errors.append(f"F{fid}: 正文 {len(bm.group(1))} 字 < 300")
        if "状态：[已深化]" not in blk:
            errors.append(f"F{fid}: 状态非已深化")
        # 双册一致：条目名 verbatim + 行数一致
        if fid in SK:
            if SK[fid][0] != title.strip():
                errors.append(f"F{fid}: 条目名与骨架账不一致")
            if SK[fid][1] != rows:
                errors.append(f"F{fid}: 行数 {rows} != 骨架账 {SK[fid][1]}")
        else:
            errors.append(f"F{fid}: 骨架账无此条")
    sk_sum = sum(SK[i][1] for i in ids)
    if rows_sum != sk_sum:
        errors.append(f"B{bno:02d}: 行数求和 {rows_sum} != 骨架账 {sk_sum}")
    grand_rows += rows_sum
    grand_entries += len(heads)

for b in BOOKS:
    check_book(b)

if grand_entries != 600:
    errors.append(f"总条数 {grand_entries} != 600")
if grand_rows != 161460:
    errors.append(f"域深化累计 {grand_rows} != 骨架 B01–B30 161,460")

if errors:
    print("FAIL:")
    for e in errors[:40]:
        print(" -", e)
    sys.exit(1)
print(f"OK · 30 册 · {grand_entries} 条深化 · 正文全部 ≥300 字 · 行数与骨架账逐条一致 · 域深化累计 {grand_rows:,}/161,460")
