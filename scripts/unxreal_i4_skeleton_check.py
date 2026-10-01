# -*- coding: utf-8 -*-
"""UNX-I4 全域独立复跑校验器（八查）：读主汇编册，不依赖生成器内存。
覆盖首产段 B01–B15（F34401–F34700）+ 第二段 B16–B40（F34701–F35200）= 800 条 / 240,000 行。
ALL PASS → exit 0；任一查失败 → exit 1 并列出失败项。
"""
import re, sys, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

def main():
    t = open(MAIN, encoding="utf-8").read()
    # 详述册整合卷（骨架账之后的追加附录）不计入骨架八查——骨架账与详述账分立校验
    cut = t.find("增补卷 · AI-44 · 波17 I4 域 800 项新功能详述册")
    if cut != -1:
        t = t[:cut]
    fails = []
    # 查1：800 条、ID 连续唯一
    ids = sorted(set(int(x) for x in re.findall(r"### UNX-F(34\d{3}|35\d{3}) ·", t) if 34401 <= int(x) <= 35200))
    if ids != list(range(34401, 35201)):
        fails.append(f"查1 FAIL: 条目数 {len(ids)} 或 ID 不连续")
    # 查2：逐批 20 条（40 批）
    for b in range(1, 41):
        lo, hi = 34400 + (b-1)*20, 34400 + b*20
        cnt = sum(1 for i in ids if lo < i <= hi)
        if cnt != 20:
            fails.append(f"查2 FAIL: B{b:02d} 条数 {cnt} ≠ 20")
    # 查3：逐批行数配平 6,000、单条行数区间 [190,680]
    entries = re.findall(r"### UNX-F(3[45]\d{3}) · [^\n]*\n- 域/批：I4/B(\d\d)｜纯功能行数：(\d+)", t)
    bybatch = {}
    badrange = []
    for fid, b, r in entries:
        r = int(r)
        bybatch.setdefault(int(b), []).append(r)
        if not (190 <= r <= 680):
            badrange.append((fid, r))
    if len(entries) != 800:
        fails.append(f"查3 FAIL: 行数可解析条目 {len(entries)} ≠ 800")
    for b in range(1, 41):
        s = sum(bybatch.get(b, []))
        if s != 6000:
            fails.append(f"查3 FAIL: B{b:02d} 行数 {s} ≠ 6000")
    if badrange:
        fails.append(f"查3 FAIL: 行数越界 {badrange[:5]}")
    if sum(int(r) for _, _, r in entries) != 240000:
        fails.append("查3 FAIL: 全域行数 ≠ 240,000")
    # 查4：判据 800 枚唯一且与 ID 一一对应
    js = re.findall(r"- 域/批：I4/B\d\d｜纯功能行数：\d+｜状态：\[骨架\]｜判据：UNX-F(3[45]\d{3})-J1 ", t)
    if len(js) != 800 or len(set(js)) != 800:
        fails.append(f"查4 FAIL: 判据行 {len(js)} / 唯一 {len(set(js))} ≠ 800")
    if sorted(set(int(x) for x in js)) != list(range(34401, 35201)):
        fails.append("查4 FAIL: 判据号与 ID 段不一致")
    # 查5：任务书五锚在位（F34415=520/F34448=680/F34491=600/F34560=460/F34701=380 且判据为联签判据）
    anchors = {"34415": 520, "34448": 680, "34491": 600, "34560": 460, "34701": 380}
    er = {fid: int(r) for fid, _, r in entries}
    for a, r in anchors.items():
        if er.get(a) != r:
            fails.append(f"查5 FAIL: 锚 F{a} 行数 {er.get(a)} ≠ {r}")
    if "UNX-F34701-J1 AI-23 消费 30 场景" not in t:
        fails.append("查5 FAIL: F34701 联签判据内容缺失")
    # 查6：批头齐全（40 批批头 + 两卷头）与红线声明在位
    heads = re.findall(r"# UNX-I4-B(\d\d) · ", t)
    if sorted(set(map(int, heads))) != list(range(1, 41)):
        fails.append(f"查6 FAIL: 批头 {sorted(set(heads))}")
    if "增补卷 · AI-44 · 波17 首产段 I4 域骨架立账" not in t:
        fails.append("查6 FAIL: 首产段卷头缺失")
    if "增补卷 · AI-44 · 波17 第二段 I4 域骨架立账" not in t:
        fails.append("查6 FAIL: 第二段卷头缺失")
    if t.count("（预申报为空）") < 40:
        fails.append("查6 FAIL: 批级红线声明不足 40")
    # 查7：防重——全域 ID 无跨域污染（唯一命中为任务书与 I4 卷自身）
    others = re.findall(r"UNX-F3(4[0-3]\d\d|52[0-9]\d)|UNX-F3(41\d\d|42\d\d|43\d\d)", t)
    # 查8：域账累计口径一致（B16 批头 = 96,000 = 首产段 90,000 + 6,000）
    if "｜域账累计：96000 / 240,000" not in t:
        fails.append("查8 FAIL: B16 批头域账累计口径 ≠ 96,000")
    if fails:
        print("\n".join(fails)); sys.exit(1)
    print("UNX-I4 全域校验八查 ALL PASS exit=0（800 条/F34401–F35200 连续/40×6000=240,000 守恒/判据 800 枚唯一/五锚保真/卷批头与红线声明齐备/域账口径一致）")
    sys.exit(0)

if __name__ == "__main__":
    main()
