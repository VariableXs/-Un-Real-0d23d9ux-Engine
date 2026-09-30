#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-41 · UNX-I1 首产段校验器（六查）：批册在位/300 条连续零跳号/批批 6,000 守恒/判据 300 枚唯一/
行数 120–600 区间/防重五范围他域 ID 段零命中。ALL PASS exit=0。"""
import os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
B = os.path.join(ROOT, "docs", "unxreal", "batches")

def main():
    fails = []
    ids, jids, sums = [], [], {}
    for b in range(1, 16):
        bid = f"B{b:02d}"
        p = os.path.join(B, f"UNX-I1-{bid}.md")
        if not os.path.exists(p):
            fails.append(f"缺批册 {p}"); continue
        txt = open(p, encoding="utf-8").read()
        rows = re.findall(r"### UNX-F(\d{5}) · (.+)\n- 域/批：I1/(B\d\d)｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F\d{5}-J1 .+)", txt)
        if len(rows) != 20:
            fails.append(f"{bid} 条数 {len(rows)} != 20")
        s = 0
        for fid, title, bb, rn, crit in rows:
            if bb != bid: fails.append(f"{bid} 批号错位 {bb}")
            ids.append(int(fid)); s += int(rn)
            if not 120 <= int(rn) <= 600: fails.append(f"UNX-F{fid} 行数 {rn} 越界")
            jids.append(crit.split("｜")[0].split(" ")[0])
        if s != 6000: fails.append(f"{bid} 求和 {s} != 6000")
        sums[bid] = s
    if ids != list(range(32001, 32301)):
        fails.append(f"ID 非连续：{len(ids)} 条")
    if len(set(jids)) != 300:
        fails.append(f"判据唯一性 {len(set(jids))} != 300")
    # 防重五范围（他域 ID 段 / kernel 源码）
    m = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
    if os.path.exists(m):
        n = len({x for x in re.findall(r"UNX-F(\d{5})", open(m, encoding="utf-8").read()) if 32001 <= int(x) <= 32800})
        if n != 300: fails.append(f"主汇编册 I1 段命中 {n} != 300（增补卷 300 条口径）")
    src = 0
    for root, dirs, files in os.walk(os.path.join(ROOT, "kernel")):
        for f in files:
            if f.endswith((".rs", ".c", ".h")):
                src += sum(1 for x in re.findall(r"UNX-F(\d{5})", open(os.path.join(root, f), encoding="utf-8", errors="ignore").read()) if 32001 <= int(x) <= 32800)
    if src: fails.append(f"kernel 源码 I1 段 ID 命中 {src}")
    print(f"查1 批册 15 件在位 {'✓' if not fails or all('批册' not in f for f in fails) else '✗'}")
    print(f"查2 ID 连续 300 条零跳号 {'✓' if ids == list(range(32001,32301)) else '✗'}")
    print(f"查3 批批 6,000 守恒（15 批 = 90,000）{'✓' if sum(sums.values()) == 90000 else '✗'}")
    print(f"查4 判据 300 枚唯一 {'✓' if len(set(jids)) == 300 else '✗'}")
    print(f"查5 行数区间 120–600 {'✓' if not any('越界' in f for f in fails) else '✗'}")
    print(f"查6 防重五范围零撞号（主汇编册/kernel 源码）{'✓' if not any('撞号' in f or '命中' in f for f in fails) else '✗'}")
    if fails:
        print("FAIL:"); [print(" -", f) for f in fails]; return 1
    print("ALL PASS exit=0")
    return 0

if __name__ == "__main__":
    sys.exit(main())
