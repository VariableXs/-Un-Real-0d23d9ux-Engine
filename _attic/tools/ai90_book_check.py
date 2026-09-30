# -*- coding: utf-8 -*-
"""AI-90 增补册五断言校验 v2：300 条连续/唯一、判据一一对应、批守恒 6,000 行、总轧 90,000、主册零撞号、F 系零表行。"""
import re, sys, io, json
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

BOOK = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\AI-90 · 治理线波次官 · 300项新功能增补册（B01–B15 · GOV90-001–GOV90-300）.md"
MAIN = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\CoRun Varix STAR II · Unxreal.md"

book = open(BOOK, encoding="utf-8").read()
rows = re.findall(r"^\| (GOV90-(\d{3})) \| (.+?) \| (\d+) \| 增补 \| GOV90-\2-J1 ", book, re.M)
nums = [int(r[1]) for r in rows]

a1 = len(rows) == 300 and sorted(nums) == list(range(1, 301))
a2 = len(set(r[0] for r in rows)) == 300
a3 = len(rows) == 300  # 判据一一对应已并入行正则（不匹配即不计数）

# 批守恒：按 ID 段分组（B01=001-020 ... B15=281-300）
batch_sums = {}
for lo in range(0, 300, 20):
    bn = f"{lo//20+1:02d}"
    batch_sums[bn] = sum(int(r[3]) for r in rows if lo < int(r[1]) <= lo+20)
a4 = len(batch_sums) == 15 and all(v == 6000 for v in batch_sums.values())
a5_total = sum(batch_sums.values()) == 90000

main_txt = open(MAIN, encoding="utf-8").read()
collide = [r[0] for r in rows if r[0] in main_txt]
a6 = len(collide) == 0
f_rows = re.findall(r"^\| UNX-F\d+ ", book, re.M)
a7 = len(f_rows) == 0  # 表行零触碰（联签锚定行内引用允许）

print("断言1 300条连续零跳号:", "PASS" if a1 else f"FAIL n={len(rows)}")
print("断言2 ID唯一零重号:", "PASS" if a2 else "FAIL")
print("断言3 判据ID一一对应:", "PASS" if a3 else "FAIL")
print("断言4 批守恒15批x6000:", "PASS" if a4 else f"FAIL {batch_sums}")
print("断言5 总轧90,000行:", "PASS" if a5_total else f"FAIL {sum(batch_sums.values())}")
print("断言6 主册零撞号:", "PASS" if a6 else f"FAIL {collide[:5]}")
print("断言7 F系域账零表行:", "PASS" if a7 else f"FAIL {f_rows[:3]}")
ok = all([a1, a2, a3, a4, a5_total, a6, a7])
print("ALL", "PASS" if ok else "FAIL")
json.dump({"rows": len(rows), "batches": batch_sums, "total": sum(batch_sums.values())},
          open(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\reports\ai90_book_check.json", "w"), ensure_ascii=False)
