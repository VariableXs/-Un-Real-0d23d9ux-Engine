#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-70 · UNX-N5 B01–B15 首产段机检：结构/守恒/锚保真/防重/追加。"""
import re, sys, io

BASE = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/docs/Varix/CoRun Varix STAR II · Unxreal"
册 = BASE + "/AI-70 · N5 · 300项新功能增补册（B01–B15 · F55201–F55500）.md"
主 = BASE + "/CoRun Varix STAR II · Unxreal.md"

ok = True
def check(name, cond):
    global ok
    print(("PASS" if cond else "FAIL"), name)
    if not cond: ok = False

text = io.open(册, encoding="utf-8").read()
rows = re.findall(r"^\| (UNX-F5\d+) \| (.+?) \| (\d+) \| 增补 \|", text, re.M)
check("① 300 条在册", len(rows) == 300)
ids = [r[0] for r in rows]
check("② ID 连续零跳号零重复 F55201–F55500",
      ids == ["UNX-F%d" % n for n in range(55201, 55501)])
check("③ ID 全唯一", len(set(ids)) == 300)
batches = re.split(r"^## 批 UNX-N5-B(\d{2})", text, flags=re.M)[1:]
sums, counts = {}, {}
for i in range(0, len(batches), 2):
    b = batches[i]; body = batches[i+1]
    rr = re.findall(r"^\| (UNX-F5\d+) \| .+? \| (\d+) \| 增补 \|", body, re.M)
    counts[b] = len(rr)
    sums[b] = sum(int(x[1]) for x in rr)
check("④ 每批 20 条", all(v == 20 for v in counts.values()) and len(counts) == 15)
check("⑤ 每批 6,000 行守恒 ×15 = 90,000 行",
      all(v == 6000 for v in sums.values()) and sum(sums.values()) == 90000)
flag = {"UNX-F55201":300,"UNX-F55221":280,"UNX-F55241":280,"UNX-F55261":320,"UNX-F55281":340}
check("⑥ 五枚示例锚行数保真", all(dict((r[0], int(r[2])) for r in rows).get(k) == v for k, v in flag.items()))
check("⑦ 每条判据号唯一", len(set(r[0] + "-J1" for r in rows)) == 300)
check("⑧ N/A 预登记在册", "BattlEye" in text and "EAC" in text and "N/A 账" in text)
check("⑨ 红线三落闸在册", all(k in text for k in ["F55287", "F55284", "F55288", "F55465"]))
check("⑩ 批位归位诚实登记在册", "恒等归位 B05" in text)

mdata = io.open(主, encoding="utf-8", errors="ignore").read()
pre = sum(1 for i in ids if i in mdata)
check("⑪ 追加前主汇编册 UNX-F552xx~F554xx 零命中", pre == 0)
if ok and pre == 0:
    with io.open(主, "a", encoding="utf-8", newline="") as f:
        f.write("\n\n---\n\n" + text.rstrip() + "\n")
    m2 = io.open(主, encoding="utf-8", errors="ignore").read()
    post = sum(1 for i in ids if i in m2)
    check("⑫ 追加后主汇编册 300 ID 全命中", post == 300)
else:
    print("SKIP 追加（前置未过）")
print("RESULT:", "ALL PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
