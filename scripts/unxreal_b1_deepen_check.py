#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-B1 finalize 机械校验器（AI-06 · 波 08-M09 收口留痕 · B40 满账版）

六项检查（对齐 finalize 五步断言链的字数与行数口径 + 骨架-深化守恒交叉轴）：
  1. 册子齐备：deepen/B1-B01..B40.md 40 件存在；
  2. 条目数：每册 20 条（### UNX-F4#### 标题），全域 800 条；
  3. 判据 ID：UNX-F4001..F4800 每号恰出现一次（唯一且连续，自指 J1）；
  4. 正文下限：每条 `- 正文：` 行内容 ≥300 字（awk 同口径：length($0)-5）；
  5. 行数守恒：每册头部 `求和 X` 递推 `域累计 Y/240,000`，终值 240,000（满账）；
  6. 骨架-深化守恒：每批骨架册 batches/UNX-B1-B%02d.md 与深化册逐条
     F ID 同名 + 纯功能行数逐条相等（双轴 diff=0），且深化逐条求和
     与册头 `求和` 声明一致（R-C1-001 批小计逐条求和唯一真值口径）。

用法：python scripts/unxreal_b1_deepen_check.py
退出码：0=全绿；1=存在违例（逐条列出）。
"""
import io
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCHES_DIR = os.path.join(REPO, "docs", "unxreal", "batches")
N_BATCHES = 40
FINAL_CUM = 240000

viol = []
entries_total = 0
sum_chain = []  # (batch, row_sum, cum_declared)


def extract_entries(lines):
    """提取 [(fid, rows)]：`### UNX-F#### ` 标题行定 ID，其后首条含
    `｜纯功能行数：N` 的判据行取行数（骨架与深化两格式同口径兼容）。"""
    out = []
    cur = None
    for ln in lines:
        m = re.match(r"^### UNX-(F\d{4}) ", ln)
        if m:
            cur = m.group(1)
            continue
        if cur is None:
            continue
        m2 = re.search(r"｜纯功能行数：([\d,]+)", ln)
        if m2:
            out.append((cur, int(m2.group(1).replace(",", ""))))
            cur = None  # 一条一行，取后即清
    return out


for n in range(N_BATCHES):
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
    declared_sum = None
    if not m1 or not m2:
        viol.append("%s: 头部行数声明缺失" % bname)
    else:
        s = int(m1.group(1).replace(",", ""))
        c = int(m2.group(1).replace(",", ""))
        sum_chain.append((bname, s, c))
        declared_sum = s

    # 6) 骨架-深化守恒交叉校验（双轴：ID 同名 + 行数逐条相等）
    skel_path = os.path.join(BATCHES_DIR, "UNX-B1-%s.md" % bname)
    if not os.path.exists(skel_path):
        viol.append("%s: 骨架册缺失 %s" % (bname, skel_path))
    else:
        with io.open(skel_path, "r", encoding="utf-8") as f:
            skel_lines = f.read().splitlines()
        skel = extract_entries(skel_lines)
        deep = extract_entries(lines)
        if len(skel) != 20:
            viol.append("%s: 骨架册判据行 %d != 20" % (bname, len(skel)))
        if len(deep) != 20:
            viol.append("%s: 深化册判据行 %d != 20" % (bname, len(deep)))
        skel_map = dict(skel)
        deep_map = dict(deep)
        for fid, rows in skel:
            if fid not in deep_map:
                viol.append("%s: 骨架条 %s 在深化册缺失" % (bname, fid))
            elif deep_map[fid] != rows:
                viol.append("%s: %s 行数骨架 %d != 深化 %d" % (bname, fid, rows, deep_map[fid]))
        for fid in deep_map:
            if fid not in skel_map:
                viol.append("%s: 深化条 %s 在骨架册多出" % (bname, fid))
        # 批级守恒：深化逐条求和 == 册头求和声明（R-C1-001 判例口径）
        if declared_sum is not None and deep:
            row_sum = sum(r for _, r in deep)
            if row_sum != declared_sum:
                viol.append("%s: 深化逐条求和 %d != 头部求和 %d（R-C1-001 口径）" % (bname, row_sum, declared_sum))

# 递推校验
cum = 0
for bname, s, c in sum_chain:
    cum += s
    if cum != c:
        viol.append("%s: 域累计声明 %d != 递推 %d" % (bname, c, cum))
if sum_chain and sum_chain[-1][2] != FINAL_CUM:
    viol.append("终值域累计 %d != %s" % (sum_chain[-1][2], format(FINAL_CUM, ",")))

print("== UNX-B1 finalize 机械校验（B01-B40 满账版）==")
print("册子: %d/%d | 条目: %d/%d | 行数递推终值: %s/%s" % (
    len(sum_chain), N_BATCHES, entries_total, N_BATCHES * 20,
    sum_chain[-1][2] if sum_chain else "N/A", format(FINAL_CUM, ",")))
if viol:
    print("违例 %d 条:" % len(viol))
    for v in viol:
        print("  - " + v)
    sys.exit(1)
print("全绿：六项检查通过（册齐备 40/条目 800/ID 连续唯一/正文 >=300 字零残留/行数递推守恒至 240,000 满账/骨架-深化双轴守恒）")
sys.exit(0)
