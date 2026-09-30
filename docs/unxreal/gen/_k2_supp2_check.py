# -*- coding: utf-8 -*-
"""AI-52 · K2 终段校验器（七查）：独立册/主册 B16–B40 全断言，exit 0 = ALL PASS"""
import io, re, sys, os

BASE = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
DIR = os.path.join(BASE, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
SA = io.open(os.path.join(DIR, "AI-52 · K2 · 500项新功能终段增补册（B16–B40）.md"), encoding="utf-8").read()
MN = io.open(os.path.join(DIR, "CoRun Varix STAR II · Unxreal.md"), encoding="utf-8").read()

fails = []
def chk(name, ok):
    print(("PASS " if ok else "FAIL ") + name)
    if not ok: fails.append(name)

# 1) 25 批在位
batches = re.findall(r"^## 批 UNX-K2-B(\d{2})（F(\d+)–F(\d+) · (I?E?M?C? ?型尾?) · ", SA, re.M)
chk("1 终段 25 批在位 B16–B40", len(batches) == 25 and [int(b[0]) for b in batches] == list(range(16, 41)))

# 2) 500 条 ID 连续
ids = [int(x) for x in re.findall(r"^\| UNX-F(\d+) \|", SA, re.M)]
chk("2 独立册 500 条 ID 连续 F41101–F41600", ids == list(range(41101, 41601)))

# 3) 行数守恒
rows = [int(x) for x in re.findall(r"^\| UNX-F\d+ \|.*?\| (\d+) \|", SA, re.M)]
chk("3a 行数合计 150,000", sum(rows) == 150000 and len(rows) == 500)
pat = [320]*5 + [300]*10 + [280]*5
ok_pat = all(rows[i*20:(i+1)*20] == pat for i in range(25))
chk("3b 每批行数模式 5×320+10×300+5×280", ok_pat)

# 4) 判据逐条：E 型批每条 J1+J1R
ok_e = True
for b in range(21, 29):
    seg = SA.split(f"## 批 UNX-K2-B{b:02d}（")[1].split("## 批 UNX-K2-B")[0]
    n1 = len(re.findall(r"UNX-F\d+-J1[ ；;]", seg)) + len(re.findall(r"UNX-F\d+-J1$", seg, re.M))
    nr = len(re.findall(r"-J1R", seg))
    if nr < 20: ok_e = False
chk("4a E 型 8 批每批 ≥20 条 J1R 反判据", ok_e)
chk("4b 判据总数 ≥600（500 J1 + 160 J1R + 其余）", SA.count("-J1") >= 660)

# 5) 独立册与主册一致（主册含终段卷且逐条 ID 一致）
chk("5a 主册含 AI-52 终段卷标", "增补卷 · AI-52 · 波19–20 终段 K2 域 500 项新功能" in MN)
mseg = MN.split("增补卷 · AI-52 · 波19–20 终段 K2 域 500 项新功能")[1]
mids = [int(x) for x in re.findall(r"^\| UNX-F(\d+) \|", mseg, re.M)]
chk("5b 主册终段卷 500 条 ID 与独立册一致", mids == list(range(41101, 41601)))
chk("5c 主册首产段卷仍在（未删除）", "增补卷 · AI-52 · 波19 首产段 K2 域 300 项新功能" in MN)

# 6) I 型联签密度 ≥30%（每批 ≥6 条含"联签"）
ok_i = True
for b in range(29, 37):
    seg = SA.split(f"## 批 UNX-K2-B{b:02d}（")[1].split("## 批 UNX-K2-B")[0]
    n = sum(1 for line in seg.splitlines() if line.startswith("| UNX-F") and "联签" in line)
    if n < 6: ok_i = False; print("  B%02d 联签条目=%d" % (b, n))
chk("6 I 型 8 批联签密度各 ≥6 条（≥30%）", ok_i)

# 7) 域关门关键印在册
chk("7 域关门印与终了声明在册", "UNX-F41599-J1" in SA and "UNX-F41600-J1" in SA and "240,000/240,000" in SA)

print()
if fails:
    print("ALL FAIL:", fails); sys.exit(1)
print("ALL PASS (7 checks) · exit=0")
