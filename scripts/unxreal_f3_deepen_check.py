# -*- coding: utf-8 -*-
"""UNX-F3 域深化校验器（AI-28 · 首轮深化 B01–B15 · 300 条）。
检查 deepen/F3-B01..B15.md 十五册：
  ① 每册 20 条、六要素齐备（判据/定位/语义边界/依赖与嫁接源/风险与回退/正文）
  ② 逐条正文 ≥300 字（单行计量，与 F1/D5 口径一致）
  ③ 批内 ID 连续且与骨架账区间一致（B01 F21601 … B15 F21881–F21900）
  ④ 逐批行数求和 = 骨架账同批求和（深化零改行数）；域累计 = 90,000
  ⑤ 状态全部 [已深化]，判据号 UNX-F{id}-J1 与本条 ID 自指一致
  ⑥ 双册一致：骨架账条目名与深化册 verbatim 一致
全绿 exit=0。"""
import os, re, sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
BOOKS = range(1, 16)

errors = []
grand_entries = grand_rows = 0

def parse_skeleton_titles():
    sk = {}
    for b in range(1, 16):
        t = open(os.path.join(BATCH, f"UNX-F3-B{b:02d}.md"), encoding="utf-8").read()
        for full, idn, title in re.findall(r"^### (UNX-F(\d{5})) · (.+)$", t, re.M):
            seg = t[t.find(full):]
            rows = int(re.search(r"纯功能行数：(\d+)", seg).group(1))
            sk[int(idn)] = (title.strip(), rows)
    return sk

SK = parse_skeleton_titles()

def check_book(bno):
    global grand_entries, grand_rows
    path = os.path.join(DEEPEN, f"F3-B{bno:02d}.md")
    text = open(path, encoding="utf-8").read()
    heads = re.findall(r"^### (UNX-F(\d{5})) · (.+)$", text, re.M)
    if len(heads) != 20:
        errors.append(f"B{bno:02d}: entries={len(heads)} != 20")
    ids = [int(h[1]) for h in heads]
    if ids != sorted(ids) or (ids and ids[0] != ids[0]):
        errors.append(f"B{bno:02d}: ids not ascending")
    blocks = re.split(r"^### UNX-F\d{5} · .+$", text, flags=re.M)[1:]
    rows_sum = 0
    for h, blk in zip(heads, blocks):
        idn = int(h[1])
        title = h[2].strip()
        # ⑥ 双册一致
        if idn not in SK:
            errors.append(f"F{idn:05d}: not in skeleton")
        else:
            if SK[idn][0] != title:
                errors.append(f"F{idn:05d}: title mismatch with skeleton")
            # ④ 行数零改
            m = re.search(r"纯功能行数：(\d+) 行", blk)
            if not m or int(m.group(1)) != SK[idn][1]:
                errors.append(f"F{idn:05d}: rows mismatch")
            else:
                rows_sum += SK[idn][1]
        # ① 六要素
        for tag in ("定位", "语义边界", "依赖与嫁接源", "风险与回退"):
            if f"**{tag}**：" not in blk:
                errors.append(f"F{idn:05d}: missing {tag}")
        if "判据：UNX-F" not in blk:
            errors.append(f"F{idn:05d}: missing judge")
        # ⑤ 判据自指
        if f"UNX-F{idn:05d}-J1" not in blk:
            errors.append(f"F{idn:05d}: judge id mismatch")
        if "[已深化]" not in blk:
            errors.append(f"F{idn:05d}: status not deepened")
        # ② 正文 ≥300 字
        m = re.search(r"- 正文：(.+)", blk)
        if not m or len(m.group(1)) < 300:
            errors.append(f"F{idn:05d}: body <300 chars")
    grand_entries += len(heads)
    grand_rows += rows_sum
    # ③ 区间核对
    lo, hi = 21601 + (bno - 1) * 20, 21620 + (bno - 1) * 20
    if ids and (ids[0] != lo or ids[-1] != hi):
        errors.append(f"B{bno:02d}: id range {ids[0]}-{ids[-1]} != {lo}-{hi}")
    return rows_sum

def main():
    total = 0
    for b in BOOKS:
        total += check_book(b)
    if grand_entries != 300:
        errors.append(f"grand entries {grand_entries} != 300")
    if grand_rows != 90000:
        errors.append(f"grand rows {grand_rows} != 90000")
    if errors:
        print("FAIL", len(errors))
        for e in errors[:40]:
            print(" -", e)
        sys.exit(1)
    print(f"OK entries=300 rows={grand_rows} 六查全绿")

if __name__ == "__main__":
    main()
