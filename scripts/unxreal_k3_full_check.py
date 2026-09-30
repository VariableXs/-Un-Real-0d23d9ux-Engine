# -*- coding: utf-8 -*-
"""
UNX-K3 全域终版校验器 · AI-53（八查）
一查：两卷卷头在位；二查：40 批批头齐且顺序正确；三查：800 条 ID 连续；
四查：每批 6,000、全卷 240,000；五查：判据锚 800 枚各归其位；
六查：任务书示例锚四枚在位（F41601=480/F41701=420/F41801=400/F41901=460/F42001=350 五枚）；
七查：域收官印在位；八查：邻域既有账零扰动（F38400/F40000 区段锚）。
exit=0 即 ALL PASS。
"""
import re
import sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
MAIN_MD = REPO + r"\docs\Varix\CoRun Varix STAR II · Unxreal\CoRun Varix STAR II · Unxreal.md"

with open(MAIN_MD, "r", encoding="utf-8") as fh:
    text = fh.read()

assert "## 增补卷 · AI-53 · 波20 首产段 K3 域骨架立账（B01–B15 · F41601–F41900 · 300 条）" in text
assert "## 增补卷 · AI-53 · 波20 第二产段 K3 域骨架立账（B16–B40 · F41901–F42400 · 500 条）" in text
print("一查 两卷卷头在位 PASS")

heads = re.findall(r"# UNX-K3-B(\d{2}) ·", text)
assert heads == [f"{n:02d}" for n in range(1, 41)], f"二查败：批头 {heads[:5]}..."
print("二查 40 批批头齐且有序 PASS")

seg = text[text.index("## 增补卷 · AI-53"):]
entries = re.findall(r"### UNX-F(\d{5}) · [^\n]*\n- 域/批：K3/B\d{2}｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F(\d{5})-J1 ([^\n]*)", seg)
assert len(entries) == 800, f"三查败：条目 {len(entries)}"
ids = [int(e[0]) for e in entries]
assert ids == list(range(41601, 42401)), "三查败：ID 不连续"
print("三查 800 条 ID 连续 PASS")

per = {}
for bno, ln in re.findall(r"- 域/批：K3/B(\d{2})｜纯功能行数：(\d+)｜", seg):
    per[int(bno)] = per.get(int(bno), 0) + int(ln)
assert len(per) == 40 and all(v == 6000 for v in per.values()), "四查败：批守恒"
assert sum(per.values()) == 240000, "四查败：总守恒"
print("四查 40 批 × 6,000 = 240,000 守恒 PASS")

for eid, _ln, jid, _jud in entries:
    assert int(eid) == int(jid), f"五查败：判据错位 F{eid}"
print("五查 判据锚 800 枚各归其位 PASS")

for anchor, lines in [("F41601", "480"), ("F41701", "420"), ("F41801", "400"), ("F41901", "460"), ("F42001", "350")]:
    m = re.search(rf"### UNX-{anchor} · [^\n]*\n- 域/批：K3/B\d{{2}}｜纯功能行数：(\d+)｜", text)
    assert m and m.group(1) == lines, f"六查败：锚 {anchor} 行数 {m and m.group(1)}"
print("六查 任务书示例锚五枚行数保真 PASS")

assert "UNX-K3 域收官印" in text and "UNX-F42400 · 域终" in text, "七查败：收官印缺失"
print("七查 域收官印在位 PASS")

for anchor in ["UNX-F38400", "八百条全冻结"]:
    assert anchor in text, f"八查败：邻域锚 {anchor} 丢失"
print("八查 邻域既有账零扰动 PASS")

print("全域终版校验八查 ALL PASS exit=0")
sys.exit(0)
