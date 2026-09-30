#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-G3 域账骨架校验器（全域口径 B01–B40）· 五查：
①800 条连续唯一 F25601–F26400 ②批批 6,000 行求和=批头登记
③判据号 800 枚唯一 ④四十件批册在位且批号/ID 区间一致 ⑤防重多范围（他域 ID 段零命中）
ALL PASS exit 0。
"""
import io, os, re, sys

BASE = os.path.normpath(os.path.join(os.path.dirname(__file__), ".."))
BAT = os.path.join(BASE, "docs", "unxreal", "batches")
ok = True
def chk(cond, msg):
    global ok
    print(("PASS " if cond else "FAIL ") + msg)
    if not cond: ok = False

ids, judges = [], set()
pat_id = re.compile(r"^### UNX-F(\d+) · ", re.M)
pat_row = re.compile(r"^- 域/批：G3/B(\d+)｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F\d+-J1) ", re.M)

texts = {}
for b in range(1, 41):
    p = os.path.join(BAT, "UNX-G3-B%02d.md" % b)
    chk(os.path.isfile(p), "批册存在 UNX-G3-B%02d.md" % b)
    if not os.path.isfile(p):
        texts[b] = ""
        continue
    t = io.open(p, encoding="utf-8").read()
    texts[b] = t
    ids += [int(x) for x in pat_id.findall(t)]
    rows = pat_row.findall(t)
    chk(len(rows) == 20, "B%02d 20 条" % b)
    s = sum(int(r[1]) for r in rows)
    chk(s == 6000, "B%02d 行数守恒 6000（实 %d）" % (b, s))
    for _, _, j in rows:
        if j in judges: chk(False, "判据重复 %s" % j)
        judges.add(j)

chk(ids == list(range(25601, 26401)), "ID 段 F25601–F26400 连续唯一 800 条（实 %d）" % len(ids))
chk(len(judges) == 800, "判据号 800 枚唯一（实 %d）" % len(judges))

# 防重多范围：他域 ID 段在本域批册零命中
foreign = [re.compile(r"UNX-F%d" % lo) for lo in
           (20001, 20801, 23201, 24001, 24801, 26401, 28001, 15201, 1521, 23601)]
hits = []
for b in range(1, 41):
    for rx in foreign:
        if rx.search(texts[b]): hits.append("B%02d:%s" % (b, rx.pattern))
chk(not hits, "防重多范围：他域 ID 段零命中（命中：%s）" % hits)

print("ALL PASS" if ok else "FAILED")
sys.exit(0 if ok else 1)
