# -*- coding: utf-8 -*-
"""UNX-I4 首产段独立复跑校验器（六查）：读主汇编册，不依赖生成器内存。
ALL PASS → exit 0；任一查失败 → exit 1 并列出失败项。
"""
import re, sys, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

def main():
    t = open(MAIN, encoding="utf-8").read()
    fails = []
    # 查1：300 条、ID 连续唯一
    ids = sorted(set(int(x) for x in re.findall(r"### UNX-F(34\d{3}) ·", t) if 34401 <= int(x) <= 34700))
    if ids != list(range(34401, 34701)):
        fails.append(f"查1 FAIL: 条目数 {len(ids)} 或 ID 不连续")
    # 查2：逐批 20 条
    for b in range(1, 16):
        lo, hi = 34400 + (b-1)*20, 34400 + b*20
        cnt = sum(1 for i in ids if lo < i <= hi)
        if cnt != 20:
            fails.append(f"查2 FAIL: B{b:02d} 条数 {cnt} ≠ 20")
    # 查3：逐批行数配平 6,000、单条行数区间 [190,680]
    entries = re.findall(r"### UNX-F(34[4-7]\d\d) · [^\n]*\n- 域/批：I4/B(\d\d)｜纯功能行数：(\d+)", t)
    bybatch = {}
    badrange = []
    for fid, b, r in entries:
        r = int(r)
        bybatch.setdefault(int(b), []).append(r)
        if not (190 <= r <= 680):
            badrange.append((fid, r))
    if len(entries) != 300:
        fails.append(f"查3 FAIL: 行数可解析条目 {len(entries)} ≠ 300")
    for b in range(1, 16):
        s = sum(bybatch.get(b, []))
        if s != 6000:
            fails.append(f"查3 FAIL: B{b:02d} 行数 {s} ≠ 6000")
    if badrange:
        fails.append(f"查3 FAIL: 行数越界 {badrange[:5]}")
    if sum(int(r) for _, r in [(f, int(x)) for f, x in [(e[0], e[2]) for e in entries]]) != 90000:
        fails.append("查3 FAIL: 全卷行数 ≠ 90,000")
    # 查4：判据 300 枚唯一且与 ID 一一对应
    js = re.findall(r"- 域/批：I4/B\d\d｜纯功能行数：\d+｜状态：\[骨架\]｜判据：UNX-F(34[4-7]\d\d)-J1 ", t)
    if len(js) != 300 or len(set(js)) != 300:
        fails.append(f"查4 FAIL: 判据行 {len(js)} / 唯一 {len(set(js))} ≠ 300")
    if sorted(set(int(x) for x in js)) != list(range(34401, 34701)):
        fails.append("查4 FAIL: 判据号与 ID 段不一致")
    # 查5：任务书锚在位（F34415=520/F34448=680/F34491=600/F34560=460）
    anchors = {"34415": 520, "34448": 680, "34491": 600, "34560": 460}
    er = {fid: int(r) for fid, _, r in entries}
    for a, r in anchors.items():
        if er.get(a) != r:
            fails.append(f"查5 FAIL: 锚 F{a} 行数 {er.get(a)} ≠ {r}")
    # 查6：批头齐全（15 批批头 + 卷头）与红线声明在位
    heads = re.findall(r"# UNX-I4-B(\d\d) · ", t)
    if sorted(set(map(int, heads))) != list(range(1, 16)):
        fails.append(f"查6 FAIL: 批头 {sorted(set(heads))}")
    if "增补卷 · AI-44 · 波17 首产段 I4 域骨架立账" not in t:
        fails.append("查6 FAIL: 卷头缺失")
    if t.count("（预申报为空）") < 15:
        fails.append("查6 FAIL: 批级红线声明不足 15")
    if fails:
        print("\n".join(fails)); sys.exit(1)
    print("UNX-I4 首产段校验六查 ALL PASS exit=0（300 条/F34401–F34700 连续/15×6000=90,000/判据 300 枚唯一/四锚行数保真/卷批头与红线声明齐备）")
    sys.exit(0)

if __name__ == "__main__":
    main()
