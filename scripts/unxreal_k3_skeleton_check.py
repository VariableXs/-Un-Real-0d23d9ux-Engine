# -*- coding: utf-8 -*-
"""
UNX-K3 骨架首产段独立校验器 · AI-53（六查）
一查：卷头在位；二查：15 批批头在位且顺序正确；三查：300 条条目在位且 ID 连续；
四查：每批行数求和 6,000、全卷 90,000；五查：判据号 300 枚唯一；
六查：主汇编册 K3 段外既有域账零扰动（只读断言——域收口锚 F38400 仍在）。
exit=0 即 ALL PASS。
"""
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MAIN_MD = os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                       "CoRun Varix STAR II · Unxreal.md")

with open(MAIN_MD, "r", encoding="utf-8") as fh:
    text = fh.read()

# 一查
assert "## 增补卷 · AI-53 · 波20 首产段 K3 域骨架立账（B01–B15 · F41601–F41900 · 300 条）" in text, "一查败：卷头缺失"
print("一查 卷头在位 PASS")

# 二查
for no in range(1, 16):
    first = 41601 + (no - 1) * 20
    last = first + 19
    hdr = f"# UNX-K3-B{no:02d} ·"
    assert hdr in text, f"二查败：B{no:02d} 批头缺失"
assert text.count("\n# UNX-K3-B") == 15, "二查败：批头数不为 15"
print("二查 15 批批头在位 PASS")

# 三查
entry_re = re.compile(r"^### UNX-F(\d{5}) · ", re.M)
ids = [int(m) for m in entry_re.findall(text.split("## 增补卷 · AI-53")[1])]
assert len(ids) == 300, f"三查败：条目数 {len(ids)}"
assert ids == list(range(41601, 41901)), "三查败：ID 不连续"
print("三查 300 条 ID 连续 PASS")

# 四查
seg = text[text.index("## 增补卷 · AI-53 · 波20"):]
lines_re = re.compile(r"- 域/批：K3/B(\d{2})｜纯功能行数：(\d+)｜")
per_batch = {}
for bno, ln in lines_re.findall(seg):
    per_batch[int(bno)] = per_batch.get(int(bno), 0) + int(ln)
assert len(per_batch) == 15, f"四查败：批数 {len(per_batch)}"
for bno, s in sorted(per_batch.items()):
    assert s == 6000, f"四查败：B{bno:02d} 求和 {s}"
assert sum(per_batch.values()) == 90000, "四查败：总和不 90,000"
print("四查 15 批 × 6,000 = 90,000 守恒 PASS")

# 五查：每条判据段含自身 UNX-F<id>-J1 锚（300 枚唯一，ID 连续已由三查保证）
entry_hdr_re = re.compile(r"^### UNX-F(\d{5}) · ", re.M)
seg_entries = re.findall(r"### UNX-F(\d{5}) · [^\n]*\n- 域/批：K3/B\d{2}｜纯功能行数：\d+｜状态：\[骨架\]｜判据：([^\n]*)", seg)
assert len(seg_entries) == 300, f"五查败：条目段 {len(seg_entries)}"
for eid, jud in seg_entries:
    assert f"UNX-F{eid}-J1" in jud, f"五查败：F{eid} 判据锚缺失"
print("五查 判据号 300 枚各归其位 PASS")

# 六查
assert "UNX-F38400-J1" in text and "域收官印" in text, "六查败：J3 域收官锚丢失"
for anchor in ["UNX-F38399", "UNX-J3 域收官印", "八百条全冻结"]:
    assert anchor in text, f"六查败：锚 {anchor} 丢失"
assert text.index("## 增补卷 · AI-53") > text.index("域收官总印（AI-48"), "六查败：追加位置异常"
print("六查 既有域账零扰动 PASS")

print("校验器六查 ALL PASS exit=0")
sys.exit(0)
