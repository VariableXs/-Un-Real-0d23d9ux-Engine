# -*- coding: utf-8 -*-
"""UNX-F3 增补卷二校验器（S101–S400）：六查，exit=0 放行。"""
import io, os, re, sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
while not os.path.isdir(os.path.join(REPO, "docs", "unxreal")):
    REPO = os.path.dirname(REPO)
OUT = os.path.join(REPO, "docs", "unxreal", "batches")

errors = []
ids, batch_of = {}, {}

for b in range(6, 21):
    p = os.path.join(OUT, f"UNX-F3-S{b:02d}.md")
    t = io.open(p, encoding="utf-8").read()
    entries = re.findall(r"^### (UNX-F3-S(\d{3})) · (.+)$\n- 域/批：F3/S(\d{2})｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F3-S\d{3}-J1 .+)$\n- 正文：(.+)$", t, re.M)
    if len(entries) != 20:
        errors.append(f"S{b:02d} 条目数 {len(entries)} != 20")
    for fid, snum, title, bt, lines, jc, body in entries:
        n = int(snum)
        if fid in ids:
            errors.append(f"重号 {fid}")
        ids[fid] = (title, lines, jc, body)
        if int(bt) != b:
            errors.append(f"{fid} 批号错 {bt}!={b}")
        if int(lines) != 300:
            errors.append(f"{fid} 行数 {lines}!=300")
        if not jc.startswith(f"UNX-F3-S{snum}-J1 {title}"):
            errors.append(f"{fid} 判据号/标题不自指")
        if len(body) < 200:
            errors.append(f"{fid} 正文过短")

# 查 ID 连续 S101–S400
snums = sorted(int(k[-3:]) for k in ids)
if snums != list(range(101, 401)):
    missing = [i for i in range(101, 401) if f"UNX-F3-S{i:03d}" not in ids]
    errors.append(f"ID 缺号: {missing[:10]} 共{len(missing)}")

if errors:
    print("FAIL")
    for e in errors[:30]:
        print(" -", e)
    sys.exit(1)
print(f"OK: S101–S400 共 {len(ids)} 条，六查全绿（条目数/行数/判据自指/ID连续/批号/正文）")
sys.exit(0)
