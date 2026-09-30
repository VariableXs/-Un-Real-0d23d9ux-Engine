# -*- coding: utf-8 -*-
"""AI-49 · J4 深化轮第二段独立校验器（六查，与生成器互不共享代码路径）"""
import os, re, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
DEEPEN = os.path.join(ROOT, "docs", "unxreal", "deepen")

ok = True
def check(name, cond, detail=""):
    global ok
    print(("PASS" if cond else "FAIL"), name, detail)
    if not cond:
        ok = False

# 查一：25 册在位、每册 20 条
books = [f"J4-B{b:02d}.md" for b in range(16, 41)]
n_books = sum(1 for b in books if os.path.isfile(os.path.join(DEEPEN, b)))
check("一 25 册在位", n_books == 25, f"{n_books}/25")

total_entries = 0
for b in books:
    p = os.path.join(DEEPEN, b)
    if not os.path.isfile(p):
        continue
    t = open(p, encoding="utf-8").read()
    n = len(re.findall(r"^## UNX-F\d{5} · ", t, re.M))
    total_entries += n
    check(f"  {b} 条数 20", n == 20, str(n))
check("二 总条数 500", total_entries == 500, str(total_entries))

# 查三：deepen 册 ID 连续 F38701–F39200 零跳号
ids = []
for b in books:
    p = os.path.join(DEEPEN, b)
    if os.path.isfile(p):
        ids += [int(x) for x in re.findall(r"^## UNX-F(\d{5}) · ", open(p, encoding="utf-8").read(), re.M)]
check("三 ID 连续零跳号", ids == list(range(38701, 39201)), f"{len(ids)} 条")

# 查四：六要素齐备（每条正文含六个标签且 ≥300 字）
min_len = 10 ** 9
six_ok = True
for b in books:
    p = os.path.join(DEEPEN, b)
    if not os.path.isfile(p):
        six_ok = False
        continue
    t = open(p, encoding="utf-8").read()
    bodies = re.split(r"^## UNX-F\d{5} · .+$", t, flags=re.M)[1:]
    for body in bodies:
        if not all(f"【{k}】" in body for k in ("定位", "边界", "判据", "行数", "依赖", "风险")):
            six_ok = False
        min_len = min(min_len, len(body.strip()))
check("四 六要素齐备且 ≥300 字", six_ok and min_len >= 300, f"最短 {min_len}")

# 查五：主册第二段卷在位 + J4 已深化终态 800 条 + 第二段 500 条判据唯一自指
mt = open(MAIN, encoding="utf-8").read()
check("五a 主册第二段卷在位", "J4 域深化增补卷第二段" in mt)
n_done = sum(1 for line in mt.splitlines() if "域/批：J4/" in line and "状态：[已深化]" in line)
check("五b 主册 J4 已深化 800/800", n_done == 800, str(n_done))
crits = re.findall(r"域/批：J4/B\d\d｜判据：UNX-F(\d{5})-J1", mt)
c500 = [c for c in crits if 38701 <= int(c) <= 39200]
check("五c 第二段判据 500 枚唯一自指", len(c500) == 500 and len(set(c500)) == 500, f"{len(c500)}")

# 查六：主册第二段卷 500 条与 25 册一一对应零增删
vol = mt.split("J4 域深化增补卷第二段", 1)[1] if "J4 域深化增补卷第二段" in mt else ""
vol_ids = [int(x) for x in re.findall(r"^### UNX-F(\d{5}) · ", vol, re.M)]
check("六 主册卷 500 条一一对应零增删", vol_ids == list(range(38701, 39201)), f"{len(vol_ids)} 条")

print("ALL PASS" if ok else "HAS FAILURES")
sys.exit(0 if ok else 1)
