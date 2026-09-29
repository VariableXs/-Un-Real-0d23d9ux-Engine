#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-B1 finalize 机械校验器（AI-06 · 波 08-M09 收口留痕）

五项检查（对齐 finalize 五步断言链的字数与行数口径）：
  1. 册子齐备：deepen/B1-B01..B15.md 15 件存在；
  2. 条目数：每册 20 条（### UNX-F4#### 标题），全域 300 条；
  3. 判据 ID：UNX-F4001..F4300 每号恰出现一次（唯一且连续，自指 J1）；
  4. 正文下限：每条 `- 正文：` 行内容 ≥300 字（awk 同口径：length($0)-5）；
  5. 行数守恒：每册头部 `求和 X` 递推 `域累计 Y/240,000`，终值 81,780。

用法：python scripts/unxreal_b1_deepen_check.py
退出码：0=全绿；1=存在违例（逐条列出）。
"""
import io
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCHES = [("B%02d" % i, "F%d" % (4001 + i * 20), "F%d" % (4020 + i * 20)) for i in range(0, 15)]
# B01 -> F4001–F4020 ... B15 -> F4281–F4300
BATCHES = [(b, "F%d" % (4001 + n * 20), "F%d" % (4019 + n * 20)) for n, (b, _, _) in enumerate(BATCHES)]

viol = []
entries_total = 0
sum_chain = []  # (batch, row_sum, cum_declared)

for n in range(15):
    bname = "B%02d" % (n + 1)
    first, last = 4001 + n * 20, 4020 + n * 20
    path = os.path.join(DEEPEN, "B1-%s.md" % bname)
    if not os.path.exists(path):
        viol.append("%s: 册子缺失 %s" % (bname, path))
        continue
    with io.open(path, "r", encoding="utf-8") as f:
        lines = f.read().splitlines()

    # 2) 条目数与判据 ID
    ids = []
    for ln in lines:
        m = re.match(r"^### UNX-(F\d{4}) ", ln)
        if m:
            ids.append(m.group(1))
    entries_total += len(ids)
    if len(ids) != 20:
        viol.append("%s: 条目数 %d != 20" % (bname, len(ids)))
    expect = ["F%d" % x for x in range(first, last + 1)]
    if ids != expect:
        viol.append("%s: 判据 ID 区间不连续或不符（期望 %s..%s）" % (bname, expect[0], expect[-1]))

    # 4) 正文下限
    for i, ln in enumerate(lines, 1):
        if ln.startswith("- 正文："):
            c = len(ln) - 5
            if c < 300:
                viol.append("%s:%d: 正文 %d 字 < 300" % (bname, i, c))

    # 5) 行数声明
    head = lines[2] if len(lines) > 2 else ""
    m1 = re.search(r"求和 ([\d,]+)", head)
    m2 = re.search(r"域累计 ([\d,]+)/240,000", head)
    if not m1 or not m2:
        viol.append("%s: 头部行数声明缺失" % bname)
    else:
        s = int(m1.group(1).replace(",", ""))
        c = int(m2.group(1).replace(",", ""))
        sum_chain.append((bname, s, c))

# 递推校验
cum = 0
for bname, s, c in sum_chain:
    cum += s
    if cum != c:
        viol.append("%s: 域累计声明 %d != 递推 %d" % (bname, c, cum))
if sum_chain and sum_chain[-1][2] != 81780:
    viol.append("终值域累计 %d != 81,780" % sum_chain[-1][2])

print("== UNX-B1 finalize 机械校验 ==")
print("册子: %d/15 ｜ 条目: %d/300 ｜ 行数递推终值: %s/81,780" % (
    len(sum_chain), entries_total, sum_chain[-1][2] if sum_chain else "N/A"))
if viol:
    print("违例 %d 条:" % len(viol))
    for v in viol:
        print("  - " + v)
    sys.exit(1)
print("全绿：五项检查通过（册齐备/条目 300/ID 连续唯一/正文 ≥300 字零残留/行数递推守恒）")
sys.exit(0)
