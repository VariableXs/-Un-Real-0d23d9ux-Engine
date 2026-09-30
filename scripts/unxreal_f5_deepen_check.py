# -*- coding: utf-8 -*-
"""UNX-F5 深化册机械校验器（AI-30 · B01–B40 满账口径）六查：
①六要素齐备（定位/语义边界/依赖/风险/正文/判据自指）②正文 ≥300 字
③ID 连续唯一 ④行数与骨架账 verbatim 一致 ⑤判据自指（J1 原文一致）
⑥双册条目名 verbatim 一致（深化零改条目名）
"""
import re, sys, glob

DEEP = "docs/unxreal/deepen"
BAT = "docs/unxreal/batches"
BIDS = ["B%02d" % i for i in range(1, 41)]
fails = []

ids, deep_cache = [], {}
for bid in BIDS:
    dtxt = open(f"{DEEP}/F5-{bid}.md", encoding="utf-8").read()
    bt = open(f"{BAT}/UNX-F5-{bid}.md", encoding="utf-8").read()
    brows = {int(r[0]): r for r in re.findall(
        r"^### UNX-F(\d+) · (.+)$\n- 域/批：F5/"+bid+r"｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F\1-J1 (.+)$", bt, re.M)}
    blocks = re.split(r"(?=^### UNX-F\d+ · )", dtxt, flags=re.M)[1:]
    for blk in blocks:
        m = re.match(r"^### UNX-F(\d+) · (.+)$\n- 域/批：F5/"+bid+r"｜判据：UNX-F\1-J1 (.+?)｜纯功能行数：(\d+) 行", blk, re.M)
        if not m:
            fails.append(f"{bid} 块头解析失败"); continue
        fid, title, judge, p = int(m.group(1)), m.group(2), m.group(3), int(m.group(4))
        ids.append(fid)
        # ① 六要素
        for k in ["**定位**","**语义边界**","**依赖与嫁接源**","**风险与回退**","- 正文："]:
            if k not in blk: fails.append(f"F{fid} 缺要素 {k}")
        if f"UNX-F{fid}-J1" not in blk: fails.append(f"F{fid} 判据自指缺失")
        # ② 正文 ≥300 字
        body = re.search(r"- 正文：(.+)", blk)
        if not body or len(body.group(1)) < 300: fails.append(f"F{fid} 正文不足 300 字")
        # ④⑤⑥ 与骨架 verbatim
        br = brows.get(fid)
        if not br: fails.append(f"F{fid} 骨架账无此条"); continue
        if title != br[1]: fails.append(f"F{fid} 条目名与骨架不一致")
        if p != int(br[2]): fails.append(f"F{fid} 行数 {p} != 骨架 {br[2]}")
        if judge.strip() != br[3].strip(): fails.append(f"F{fid} 判据与骨架不一致")
if ids != list(range(23201, 24001)):
    fails.append(f"ID 非 F23201–F24000 连续唯一（实得 {len(ids)}）")

if fails:
    print("FAIL", len(fails)); [print(" -", f) for f in fails[:20]]; sys.exit(1)
print(f"unxreal_f5_deepen_check: 六查 ALL PASS exit 0（40 册满账/{len(ids)} 条深化/行数 verbatim/判据自指 800/正文 300 字全过）")
