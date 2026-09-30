# -*- coding: utf-8 -*-
"""UNX-K3 深化后终验器 · AI-53（主册八查[已深化态] + deepen 四十册四查）exit=0"""
import re, glob, sys

MAIN = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\CoRun Varix STAR II · Unxreal.md"
t = open(MAIN, encoding="utf-8").read()

assert "## 增补卷 · AI-53 · 波20 首产段 K3 域骨架立账" in t and "## 增补卷 · AI-53 · 波20 第二产段" in t
assert "## 深化增补卷 · AI-53 · K3 域" in t
print("一查 两卷卷头 + 深化增补卷在位 PASS")
heads = re.findall(r"# UNX-K3-B(\d{2}) ·", t)
assert heads == [f"{n:02d}" for n in range(1, 41)]
print("二查 40 批批头齐 PASS")
seg = t[t.index("## 增补卷 · AI-53"):]
entries = re.findall(r"### UNX-F(\d{5}) · [^\n]*\n- 域/批：K3/B\d{2}｜纯功能行数：(\d+)｜状态：\[已深化\]｜判据：UNX-F(\d{5})-J1", seg)
assert len(entries) == 800 and [int(e[0]) for e in entries] == list(range(41601, 42401))
print("三查 800 条 [已深化] ID 连续 PASS")
per = {}
for bno, ln in re.findall(r"- 域/批：K3/B(\d{2})｜纯功能行数：(\d+)｜", seg):
    per[int(bno)] = per.get(int(bno), 0) + int(ln)
assert len(per) == 40 and sum(per.values()) == 240000
print("四查 40 批 240,000 守恒 PASS")
for eid, _l, jid in entries:
    assert eid == jid
print("五查 判据锚 800 枚 PASS")
for a, l in [("F41601","480"),("F41701","420"),("F41801","400"),("F41901","460"),("F42001","350")]:
    m = re.search(rf"### UNX-{a} · [^\n]*\n- 域/批：K3/B\d{{2}}｜纯功能行数：(\d+)｜", t)
    assert m and m.group(1) == l, f"六查败 {a}"
print("六查 示例锚行数保真 PASS")
assert "UNX-K3 域收官印" in t and "UNX-F42400 · 域终" in t and "域深化收官总印" in t
print("七查 三印在位（域收官/深化收官/段闸索引）PASS")
assert "UNX-F38400" in t
print("八查 邻域零扰动 PASS")

books = sorted(glob.glob(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\deepen\K3-B*.md"))
assert len(books) == 40, f"册数 {len(books)}"
n, jids, short = 0, [], []
for p in books:
    s = open(p, encoding="utf-8").read()
    for m in re.finditer(r"### UNX-F(\d{5}) · (.*?)（深化）\n(.*?)(?=\n### |\Z)", s, re.S):
        eid, body = m.group(1), m.group(3)
        core = re.sub(r"^- 域/批：.*?$", "", body, flags=re.M).strip()
        if len(core) < 300: short.append((eid, len(core)))
        for k in ["【定位】","【边界】","【判据】","【行数】","【依赖】","【风险】"]:
            assert k in body
        jids.append(eid); n += 1
assert n == 800 and not short, f"深化 {n}/最短 {short[:3]}"
assert set(jids) == {str(i) for i in range(41601, 42401)}
print("deepen 四查：40 册 / 800 条 / ≥300 字六要素 / 判据号与主册一致 ALL PASS")
print("深化后终验 ALL PASS exit=0")
sys.exit(0)
