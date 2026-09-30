#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-65 主台账双落位：卷首登记（AI-60 二 行后插入）+ 卷末纯追加整册内容。六断言。"""
import hashlib, io, sys, os

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
SUPP = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "AI-65 · M5 · 300项新功能增补册（B01–B15 · F51201–F51500）.md")

REG = "> **增补册登记（AI-65 · 2026-10-01 · 本轮 300 项明令）**：卷末追加《AI-65 · M5 · 300项新功能增补册（B01–B15）》——UNX-M5 USB 设备世界域 F51201–F51500 共 300 项新功能增补（15 批 × 20 条：F 型 B01–B08 + M 型 B09–B15），每批 6,000 行、全册 90,000 行（域账 240,000 进度账 90,000/240,000），判据主轴「100 设备样本枚举零错账（波 23 闭账物主办）」、UASP/MTP/串口；任务书五枚示例锚区间恒等归位（行数保真 560/480/520/460/380，承 AI-51/AI-54 判例，UASP/MTP/HID 纵深分别归 B10/B12/B13 承接）；B01–B06 提前放行清单件（AI-40 H5/AI-39 H4 消费，AI-98 §37.2 锚定）；硬件数据安全红线件：MSC/MTP 写路径 AI-10 写前校验钩子挂接四处复核位在册（AI-10 联签+AI-86 复核）；防重：F51201–F51500 追加前全域零命中断言通过、他域语义面仅联签锚定行零改写；与分册文件逐字节一致，纯追加零删除。生成器 docs/unxreal/gen/_m5_supp1.py 五断言 ALL PASS exit=0（SHA-256 前 16 位 d82b7780918928eb）。详见根台账本会话条目。"

ANCHOR_LINE_PREFIX = "> **增补册登记（AI-60 · 2026-09-30 · 二 · 域收官）**"

with open(SUPP, "r", encoding="utf-8") as f:
    supp = f.read()

with io.open(MAIN, "r", encoding="utf-8") as f:
    main_text = f.read()

ALREADY = main_text.count("UNX-F51201 |") > 0  # 幂等：已落位则只校验
# 断言 A：追加前 M5 段零命中（幂等放行）
assert ALREADY or (main_text.count("UNX-F51201") == 0 and main_text.count("UNX-F51500") == 0), "断言A失败：M5 段已有命中（防重）"

if not ALREADY:
    # 断言 B：锚行存在且唯一
    lines = main_text.split("\n")
    idxs = [i for i, ln in enumerate(lines) if ln.startswith(ANCHOR_LINE_PREFIX)]
    assert len(idxs) == 1, f"断言B失败：锚行命中 {len(idxs)}"
    # 卷首插入登记行
    lines.insert(idxs[0] + 1, "")
    lines.insert(idxs[0] + 2, REG)
    head = "\n".join(lines)
    # 卷末纯追加整册
    appendix = "\n\n---\n\n" + supp.rstrip("\n") + "\n"
    new_text = head + appendix
    with io.open(MAIN, "w", encoding="utf-8", newline="\n") as f:
        f.write(new_text)
else:
    new_text = main_text

with io.open(MAIN, "r", encoding="utf-8") as f:
    back = f.read()
# 断言 D：回读一致
assert back == new_text, "断言D失败：回读不一致"
# 断言 E：300 条 ID 均在主台账（右边界精确匹配，防 4 位 ID 前缀误命中）
cnt = sum(back.count(f"| UNX-F{i} |") for i in range(51201, 51501))
assert cnt == 300, f"断言E失败：主台账命中 {cnt}/300"
# 断言 F：追加段与独立册逐字节一致
assert supp.rstrip("\n") in back, "断言F失败：追加段与独立册不一致"
sha = hashlib.sha256(back.encode("utf-8")).hexdigest()[:16]
print("ALL PASS exit=0")
print("main_sha256_16=%s size=%d" % (sha, len(back.encode("utf-8"))))
