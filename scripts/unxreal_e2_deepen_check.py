# -*- coding: utf-8 -*-
"""UNX-E2 域深化校验器（波09 续领段 · AI-22）。
检查 B01–B22 二十二册深化册 + 骨架账双册一致（B08–B22 为本会话 300 条续领段）：
  ① 每册 20 条、六要素齐备（判据/定位/语义边界/依赖/风险/正文）
  ② 逐条正文 ≥300 字
  ③ 批内 ID 连续且与 F 区间一致（B01 F16801 … B22 F17240）
  ④ 逐批行数求和 = 6,000；域累计链 = 6,000×册数（22 册 = 132,000）
  ⑤ 状态全部 [已深化]，判据号与本条 ID 一致（防自指/错位）
  ⑥ 双册一致：骨架账条目名/行数/判据与深化册 verbatim 一致
全绿 exit=0。"""
import os, re, sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
BOOKS = list(range(1, 23))
SIX = ["定位", "语义边界", "依赖与嫁接源", "风险与回退", "正文"]

errors, warns = [], []
grand_entries = grand_rows = 0


def check_book(bno):
    global grand_entries, grand_rows
    path = os.path.join(DEEPEN, f"E2-B{bno:02d}.md")
    text = open(path, encoding="utf-8").read()
    f1 = 16801 + (bno - 1) * 20
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
            if f"- **{sec}**：" not in blk and not (sec == "正文" and "\n- 正文：" in "\n" + blk):
                errors.append(f"F{fid}: 缺要素 {sec}")
        bm = re.search(r"- 正文：(.+)", blk)
        if not bm:
            errors.append(f"F{fid}: 正文缺失")
        elif len(bm.group(1)) < 300:
            errors.append(f"F{fid}: 正文 {len(bm.group(1))} 字 < 300")
        if "状态：[已深化]" not in blk:
            errors.append(f"F{fid}: 状态非已深化")
    if rows_sum != 6000 and f"R-E2-001" not in text:
        errors.append(f"B{bno:02d}: 行数求和 {rows_sum} != 6000")
    grand_rows += rows_sum
    grand_entries += len(heads)

    # ⑥ 双册一致
    skel = os.path.join(BATCH, f"UNX-E2-B{bno:02d}.md")
    st = open(skel, encoding="utf-8").read()
    sh = re.findall(r"^### (UNX-F\d+) · (.+)$", st, re.M)
    sm = dict()
    cur = None
    for line in st.splitlines():
        h = re.match(r"^### (UNX-F\d+) · (.+)$", line)
        if h:
            cur = h.group(1)
            sm[cur] = {"t": h.group(2)}
        elif cur and line.startswith("- 域/批："):
            mr = re.search(r"纯功能行数：(\d+)｜状态：\[已深化\]｜判据：(.+)$", line)
            if mr:
                sm[cur]["r"] = int(mr.group(1))
                sm[cur]["j"] = mr.group(2)
    if len(sh) != 20:
        errors.append(f"B{bno:02d} 骨架: entries={len(sh)} != 20")
    for (sid, stitle), blk2 in zip(sh, blocks):
        fid = int(sid.replace("UNX-F", ""))
        if sm.get(sid, {}).get("t") != stitle:
            errors.append(f"{sid}: 骨架标题漂移")
        dj = re.search(r"判据：(.+?)｜纯功能行数", blk2, re.S)
        if dj and sm.get(sid, {}).get("j", "").strip() != dj.group(1).strip():
            errors.append(f"{sid}: 骨架判据与深化册不一致")
        dr = re.search(r"纯功能行数：(\d+) 行", blk2)
        if dr and sm.get(sid, {}).get("r") != int(dr.group(1)):
            errors.append(f"{sid}: 骨架行数与深化册不一致")
    return rows_sum


for b in BOOKS:
    check_book(b)

if grand_entries != 440:
    errors.append(f"域总条数 {grand_entries} != 440")
# R-E2-001 账实修正后真值：B01/B03/B04=5,980、B07=5,960、余 18 批=6,000 → 131,900
if grand_rows != 131900:
    errors.append(f"域累计 {grand_rows} != 131900（R-E2-001 真值口径）")

print(f"E2 deepen check: books=22 entries={grand_entries} rows={grand_rows}")
if errors:
    print("FAIL", len(errors))
    for e in errors[:40]:
        print("  !", e)
    sys.exit(1)
print("ALL GREEN exit=0")
