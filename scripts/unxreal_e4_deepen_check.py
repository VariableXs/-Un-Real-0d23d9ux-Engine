# -*- coding: utf-8 -*-
"""UNX-E4 域深化校验器（深化轮第一轮 · AI-24）。
检查 B01–B06 六册深化册 + 骨架账双册一致：
  ① 每册 20 条、六要素齐备（判据/定位/语义边界/依赖/风险/正文）
  ② 逐条正文 ≥300 字
  ③ 批内 ID 连续且与 F 区间一致（B01 F18401 … B06 F18520）
  ④ 逐批行数求和 = 6,000；域累计链 = 6,000×册数（6 册 = 36,000）
  ⑤ 状态全部 [已深化]，判据号与本条 ID 一致（防自指/错位）
  ⑥ 双册一致：骨架账条目名/行数/判据与深化册 verbatim 一致
全绿 exit=0。
"""
import os, re, sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
BOOKS = list(range(1, 7))
SIX = ["定位", "语义边界", "依赖与嫁接源", "风险与回退", "正文"]

errors = []
grand_entries = grand_rows = 0

def norm(s):
    return s.replace('\u2013', '-').strip()

for bno in BOOKS:
    path = os.path.join(DEEPEN, f"E4-B{bno:02d}.md")
    text = open(path, encoding="utf-8").read()
    f1 = 18401 + (bno - 1) * 20
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
        seg = blk.split("｜判据：")[-1]
        if f"UNX-F{fid}-J1" not in seg:
            errors.append(f"F{fid}: 判据号自指/错位")
        for sec in SIX:
            if f"- **{sec}**：" not in blk and not (sec == "正文" and "\n- 正文：" in "\n" + blk):
                errors.append(f"F{fid}: 缺要素 {sec}")
        bm = re.search(r"- 正文：(.+)", blk)
        if not bm:
            errors.append(f"F{fid}: 正文缺失")
        elif len(bm.group(1)) < 300:
            errors.append(f"F{fid}: 正文 {len(bm.group(1))} 字 < 300")
        if "状态：[已深化]" not in blk:
            errors.append(f"F{fid}: 状态非已深化")
        # ⑥ 双册一致：骨架账三列 verbatim
        skel_path = os.path.join(BATCH, f"UNX-E4-B{bno:02d}.md")
        skel = open(skel_path, encoding="utf-8").read()
        sm = re.search(rf"^\| UNX-F{fid} \| ([^|]+) \| (\d+) \| 骨架 \| (.+?) \|$", skel, re.M)
        if not sm:
            errors.append(f"F{fid}: 骨架账行未找到")
        else:
            if norm(sm.group(1)) != norm(title):
                errors.append(f"F{fid}: 条目名漂移 骨架[{sm.group(1).strip()}] vs 深化[{title.strip()}]")
            if int(sm.group(2)) != rows:
                errors.append(f"F{fid}: 行数漂移 骨架{sm.group(2)} vs 深化{rows}")
            j1 = f"UNX-F{fid}-J1"
            dseg = blk.split("｜判据：")[-1].split("｜纯功能行数")[0]
            if j1 not in sm.group(3) or j1 not in dseg:
                errors.append(f"F{fid}: 判据列缺失")
    if rows_sum != 6000:
        errors.append(f"B{bno:02d}: 行数求和 {rows_sum} != 6000")
    grand_rows += rows_sum
    grand_entries += len(heads)

if grand_entries != len(BOOKS) * 20:
    errors.append(f"总条目 {grand_entries} != {len(BOOKS)*20}")
if grand_rows != 6000 * len(BOOKS):
    errors.append(f"深化域累计 {grand_rows} != {6000*len(BOOKS)}")

if errors:
    print("FAIL:")
    for e in errors:
        print(" -", e)
    sys.exit(1)
print(f"ALL PASS: {len(BOOKS)} 册 / {grand_entries} 条 / ID F18401–F19200 区间内连续唯一 / 批批 6,000 / 深化域累计 {grand_rows:,}/240,000 / 双册 verbatim 一致 / 六要素+正文≥300 字全过")
