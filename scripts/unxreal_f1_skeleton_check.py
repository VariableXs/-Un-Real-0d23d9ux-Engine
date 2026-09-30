#!/usr/bin/env python3
"""UNX-F1 域骨架校验器（AI-26 · B01–B30 两轮 600 条）
五查：①ID 连续唯一 F20001–F20300；②每批 20 条；③批内行数求和=批头登记（域累计链逐批校验）；
④判据号与条目 ID 配对；⑤条目名/判据号全域唯一（防重）。
exit 0 = 全绿；任何偏离 exit 1。
"""
import re, sys, glob

BATCH_DIR = "docs/unxreal/batches"
ID_LO, ID_HI = 20001, 20600
ROWS_PER_BATCH_HEADER = re.compile(r"域账累计：本批 ([\d,]+) / 240,000（B01.B\d{2} 骨架累计 ([\d,]+)）")
ENTRY = re.compile(r"^### UNX-F(\d{5}) · (.+)$")
META = re.compile(r"纯功能行数：(\d+)｜状态：\[([^\]]+)\]｜判据：UNX-F(\d{5})-J1")

def fail(msg):
    print(f"[FAIL] {msg}")
    sys.exit(1)

files = sorted(glob.glob(f"{BATCH_DIR}/UNX-F1-B*.md"))
if len(files) != 30:
    fail(f"批册数 {len(files)} != 30")

all_names, all_jids = {}, {}
prev_hi = ID_LO - 1
cum_prev = 0
total_rows = 0
report = []

for f in files:
    m = re.search(r"UNX-F1-B(\d{2})\.md$", f)
    bn = int(m.group(1))
    text = open(f, encoding="utf-8").read()
    entries = []
    for line in text.splitlines():
        em = ENTRY.match(line)
        if em:
            entries.append([int(em.group(1)), em.group(2), None])
            continue
        mm = META.search(line)
        if mm and entries:
            entries[-1][2] = mm.groups()
    if len(entries) != 20:
        fail(f"B{bn:02d} 条目数 {len(entries)} != 20")
    ids = [e[0] for e in entries]
    lo, hi = min(ids), max(ids)
    if lo != prev_hi + 1 or ids != list(range(lo, hi + 1)):
        fail(f"B{bn:02d} ID 不连续：{lo}–{hi}（前批终 {prev_hi}）")
    prev_hi = hi
    rows = 0
    for eid, name, meta in entries:
        if meta is None:
            fail(f"F{eid} 缺元数据行")
        r, st, jid = int(meta[0]), meta[1], int(meta[2])
        if jid != eid:
            fail(f"F{eid} 判据号错配 UNX-F{jid:05d}-J1")
        if eid in all_jids or name in all_names:
            fail(f"F{eid} 或条目名重复（防重五范围违例）")
        all_jids[eid] = f"B{bn:02d}"; all_names[name] = eid
        rows += r
    hm = ROWS_PER_BATCH_HEADER.search(text)
    if not hm:
        fail(f"B{bn:02d} 批头域账累计行缺失")
    claimed = int(hm.group(1).replace(",", ""))
    cum_claimed = int(hm.group(2).replace(",", ""))
    if rows != claimed:
        fail(f"B{bn:02d} 批头行数 {claimed} != 条目实和 {rows}")
    total_rows += rows
    if cum_claimed != total_rows:
        fail(f"B{bn:02d} 域累计 {cum_claimed} != 实和 {total_rows}")
    cum_prev = total_rows
    report.append(f"  B{bn:02d}: 20 条  {rows:>6,} 行  累计 {total_rows:,}")

if prev_hi != ID_HI:
    fail(f"终 ID {prev_hi} != {ID_HI}")
print("UNX-F1 骨架校验器 · 五查全绿")
for line in report:
    print(line)
print(f"两轮 600 条 · 域账累计 {total_rows:,} / 240,000 · 待领 B31–B40（200 条 / 余 {240000-total_rows:,} 行）")
sys.exit(0)
