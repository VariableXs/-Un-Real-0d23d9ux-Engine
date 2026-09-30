# -*- coding: utf-8 -*-
"""UNX-H3 首产段校验器（AI-38）——六查：
1) 批册 15 件在位且批批 20 条；2) ID 连续唯一 F29601–F29900；
3) 批内行数求和 = 6,000 且在 120–600 区间；4) 判据号唯一且与 ID 一一对应（UNX-F####-J1…）；
5) 条目名/判据在域内零重复；6) 主汇编册 H3 卷收录 300 条且 ID 集合与批册一致。
exit 0 = ALL PASS。"""
import os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")
COMP = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

entry_re = re.compile(r"^### UNX-F(\d{5}) · (.+)$", re.M)
meta_re = re.compile(r"^- 域/批：H3/(B\d{2})｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F(\d{5})-J\d+.*)$", re.M)

errors = []
entries = []  # (fid, name, batch, rows, criterion)
for n in range(1, 16):
    path = os.path.join(BATCH_DIR, f"UNX-H3-B{n:02d}.md")
    if not os.path.exists(path):
        errors.append(f"missing batch file B{n:02d}"); continue
    text = open(path, encoding="utf-8").read()
    heads = entry_re.findall(text)
    metas = meta_re.findall(text)
    if len(heads) != 20: errors.append(f"B{n:02d}: {len(heads)} entries (expect 20)")
    if len(metas) != 20: errors.append(f"B{n:02d}: {len(metas)} meta lines (expect 20)")
    for (h_id, name), (batch, rows, crit, m_id) in zip(heads, metas):
        fid = int(h_id)
        if h_id != m_id: errors.append(f"B{n:02d} F{h_id}: header/meta id mismatch")
        if batch != f"B{n:02d}": errors.append(f"F{h_id}: wrong batch {batch}")
        rows = int(rows)
        if not (120 <= rows <= 600): errors.append(f"F{h_id}: rows {rows} out of range")
        if not crit.startswith(f"UNX-F{h_id}-J"): errors.append(f"F{h_id}: criterion id mismatch")
        entries.append((fid, name, batch, rows, crit))

# 2) continuity & uniqueness
ids = sorted(e[0] for e in entries)
if ids != list(range(29601, 29901)):
    errors.append(f"ID set broken: n={len(ids)}, min={min(ids) if ids else '-'}, max={max(ids) if ids else '-'}")

# 3) per-batch sum
from collections import defaultdict
bsum = defaultdict(int)
for fid, name, batch, rows, crit in entries:
    bsum[batch] += rows
for n in range(1, 16):
    if bsum.get(f"B{n:02d}", 0) != 6000:
        errors.append(f"B{n:02d}: sum {bsum.get(f'B{n:02d}')}")
if sum(bsum.values()) != 90000:
    errors.append(f"domain first-prod sum {sum(bsum.values())} != 90000")

# 4/5) uniqueness of criteria and names
crits = [e[4] for e in entries]; names = [e[1] for e in entries]
if len(set(crits)) != len(crits): errors.append("duplicate criteria")
if len(set(names)) != len(names): errors.append("duplicate entry names")

# 6) compendium coverage
comp = open(COMP, encoding="utf-8").read()
m = re.search(r"## UNX-H3 首产段卷.*", comp, re.S)
if not m:
    errors.append("compendium H3 volume missing")
else:
    vol = m.group(0)
    vol_ids = sorted(int(x) for x in re.findall(r"\| UNX-F(\d{5}) \|", vol))
    if vol_ids != ids:
        errors.append(f"compendium ids mismatch: {len(vol_ids)} vs {len(ids)}")
    if "UNX-H3-B15" not in vol:
        errors.append("compendium missing B15 section")

if errors:
    print("FAIL:"); [print(" -", e) for e in errors]; sys.exit(1)
print(f"UNX-H3 skeleton check: 15 batches / {len(entries)} entries / {sum(bsum.values())} rows / IDs F29601-F29900 continuous / criteria & names unique / compendium volume aligned — ALL PASS")
sys.exit(0)
