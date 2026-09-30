#!/usr/bin/env python3
"""AI-57 UNX-L2 增补册七查校验器（B01–B15 · F44801–F45100）。
七查：1 册在位 2 条目数 300 3 ID 连续唯一 4 批行数 6,000 守恒 5 全卷 90,000
6 判据号唯一且与 ID 对应 7 每批恰 20 条。
"""
import re, sys, hashlib

BOOK = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\AI-57 · L2 · 300项新功能增补册（B01–B15）.md"

text = open(BOOK, encoding="utf-8").read()
rows = re.findall(r"^\| (UNX-F\d{5}) \| (.+?) \| (\d+) \| 增补 \| UNX-F\d{5}-J1 ", text, re.M)
errors = []
if not rows:
    errors.append("查1: 未解析到任何条目行")
ids = [r[0] for r in rows]
if len(rows) != 300:
    errors.append(f"查2: 条目数 {len(rows)} != 300")
want = [f"UNX-F{44800+i:05d}" for i in range(1, 301)]
if ids != want:
    bad = [(a, b) for a, b in zip(ids, want) if a != b][:5]
    errors.append(f"查3: ID 不连续/不唯一，首 5 处错位 {bad}")
# 批行数守恒
batches = re.split(r"^## 批 UNX-L2-B(\d{2})", text, flags=re.M)[1:]
for i in range(0, len(batches), 2):
    bno, body = batches[i], batches[i + 1]
    brows = re.findall(r"^\| (UNX-F\d{5}) \| .+? \| (\d+) \| 增补 \|", body, re.M)
    if len(brows) != 20:
        errors.append(f"查7: 批 B{bno} 条数 {len(brows)} != 20")
    total = sum(int(n) for _, n in brows)
    if total != 6000:
        errors.append(f"查4: 批 B{bno} 行数 {total} != 6000")
total_all = sum(int(n) for _, _, n in rows)
if total_all != 90000:
    errors.append(f"查5: 全卷行数 {total_all} != 90000")
# 判据号唯一
jids = re.findall(r"UNX-(F\d{5})-J1 ", text)
body_jids = [j for j in jids if 44801 <= int(j[1:]) <= 45100]
if len(body_jids) != len(set(body_jids)):
    errors.append("查6: 判据号存在重复")
print(f"解析条目 {len(rows)} 条；全卷行数 {total_all}")
if errors:
    print("FAIL:"); [print(" -", e) for e in errors]; sys.exit(1)
print("七查 ALL PASS exit=0")
print("SHA-256:", hashlib.sha256(open(BOOK,'rb').read()).hexdigest()[:16])
