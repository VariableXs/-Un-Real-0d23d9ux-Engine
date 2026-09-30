# -*- coding: utf-8 -*-
"""AI-47 · UNX-J2 全域校验器（一卷 B01–B15 + 二卷 B16–B40）· 八查"""
import re, sys

MD = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
T1 = "## 增补卷 · AI-47 · 波18 首产段 J2 域骨架立账"
T2 = "## 增补卷 · AI-47 · 二轮立账 J2 域 B16–B40"

def cut(t, tit):
    i = t.index(tit)
    j = t.find("\n## ", i)
    return t[i:j if j > 0 else len(t)]

def check(body, lo, hi, nb, per=6000):
    batches = re.findall(r"^# UNX-J2-B(\d{2}) · ", body, re.M)
    assert len(batches) == nb, f"批数 {len(batches)}!={nb}"
    ids = [int(x) for x in re.findall(r"^### UNX-F(\d+) · ", body, re.M)]
    assert ids == list(range(lo, hi + 1)), "ID 连续零空洞"
    perb = {}; cur = None
    for line in body.splitlines():
        m = re.match(r"^# UNX-J2-B(\d{2}) · ", line)
        if m: cur = int(m.group(1)); perb[cur] = 0
        m2 = re.match(r"^- 域/批：J2/B(\d{2})｜纯功能行数：(\d+)｜", line)
        if m2:
            assert int(m2.group(1)) == cur, "条目批号与所在批不一致"
            perb[cur] += int(m2.group(2))
    assert all(v == per for v in perb.values()), f"批批 {per}"
    cr = re.findall(r"^- 域/批：J2/B\d{2}｜纯功能行数：\d+｜(?:状态：\[骨架\]｜)?判据：UNX-F(\d+)-J1", body, re.M)
    assert len(cr) == hi - lo + 1 and len(set(cr)) == len(cr), "判据唯一"

def main():
    t = open(MD, encoding="utf-8").read()
    assert t.count(T1) == 1 and t.count(T2) == 1, "卷标题各恰一份"
    b1 = cut(t, T1); b2 = cut(t, T2)
    check(b1, 36801, 37100, 15)   # 查1-5 一卷
    check(b2, 37101, 37600, 25)   # 查1-5 二卷
    # 查6：全域判据 800 枚唯一
    cr = re.findall(r"^- 域/批：J2/B\d{2}｜纯功能行数：\d+｜(?:状态：\[骨架\]｜)?判据：UNX-F(\d+)-J1", b1 + b2, re.M)
    assert len(cr) == 800 and len(set(cr)) == 800, "全域判据 800 唯一"
    # 查7：五锚归位（一卷）
    for fid, bn in {36805: 1, 36850: 3, 36901: 6, 36965: 9, 37050: 13}.items():
        assert re.search(rf"### UNX-F{fid} · [^\n]*（任务书示例锚·B{bn:02d} 区间承载位）", b1), f"锚 F{fid}"
    # 查8：无垃圾残留 + 总行数守恒
    body = b1 + b2
    assert "REBALANCE" not in body and "SKIP：" not in body, "垃圾残留"
    print("J2 全域校验器八查 ALL PASS exit=0（两卷/40 批/800 条/F36801–F37600 连续/批批 6,000·总 240,000/判据 800 枚唯一/五锚归位/零垃圾）")
    return 0

sys.exit(main())
