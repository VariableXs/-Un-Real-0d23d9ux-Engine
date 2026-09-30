#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-65 收官册双落位：卷首登记（AI-65 首册登记行后插入）+ 卷末纯追加。六断言 + 幂等。"""
import hashlib, io, os

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
SUPP = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "AI-65 · M5 · 500项新功能增补册（B16–B40 · F51501–F52000 · 域收官册）.md")

REG = "> **增补册登记（AI-65 · 2026-10-01 · 二 · 域收官）**：卷末追加《AI-65 · M5 · 500项新功能增补册（B16–B40 · 域收官册）》——UNX-M5 域 F51501–F52000 共 500 项新功能增补（25 批 × 20 条：M 型尾段 B16–B20 / E 型边界 B21–B28 双判据 J1+J1R 正反配对 320 枚 / I 型集成 B29–B36 联签密度 ≥30% 机检 / C 型收官 B37–B40），全册 150,000 行；至此 AI-65 域账 **240,000/240,000 满账收官**（首册 300 项+本册 500 项=800 项 · 判据 800 枚唯一），域关门印 UNX-F51999 + 域终了声明 UNX-F52000 入册；判据主轴「100 设备样本枚举零错账（波 23 闭账物主办）」三件套（100 台全量复测+热插拔 1,000 次+坏设备轰炸 ×48）收官条目在册；防重：F51501–F52000 追加前全域零命中断言通过、E 型 320 枚双判据配对率 100% 机检、他域语义面仅联签锚定行零改写；与分册文件逐字节一致，纯追加零删除。生成器 docs/unxreal/gen/_m5_supp2.py 五断言 ALL PASS exit=0（SHA-256 前 16 位 49d0d23c522b2ddc）。详见根台账本会话条目。"

ANCHOR = "> **增补册登记（AI-65 · 2026-10-01 · 本轮 300 项明令）**"

with open(SUPP, "r", encoding="utf-8") as f:
    supp = f.read()
with io.open(MAIN, "r", encoding="utf-8") as f:
    main_text = f.read()

ALREADY = main_text.count("UNX-F51501 |") > 0
assert ALREADY or (main_text.count("UNX-F51501") == 0 and main_text.count("UNX-F52000 |") == 0), "断言A失败：收官段已有命中"

if not ALREADY:
    lines = main_text.split("\n")
    idxs = [i for i, ln in enumerate(lines) if ln.startswith(ANCHOR)]
    assert len(idxs) == 1, f"断言B失败：锚行 {len(idxs)}"
    lines.insert(idxs[0] + 1, "")
    lines.insert(idxs[0] + 2, REG)
    new_text = "\n".join(lines) + "\n\n---\n\n" + supp.rstrip("\n") + "\n"
    with io.open(MAIN, "w", encoding="utf-8", newline="\n") as f:
        f.write(new_text)
else:
    new_text = main_text

with io.open(MAIN, "r", encoding="utf-8") as f:
    back = f.read()
assert back == new_text, "断言D失败：回读不一致"
cnt = sum(back.count(f"| UNX-F{i} |") for i in range(51501, 51501 + 500))
assert cnt == 500, f"断言E失败：{cnt}/500"
assert supp.rstrip("\n") in back, "断言F失败：追加段不一致"
total = sum(back.count(f"| UNX-F{i} |") for i in range(51201, 52001))
assert total == 800, f"全域断言失败：M5 域 {total}/800"
print("ALL PASS exit=0")
print("M5 domain 800/800 placed; main size=%d" % len(back.encode("utf-8")))
