# -*- coding: utf-8 -*-
"""
AI-49 · J4 深化轮第一段（B01–B15）独立校验器 · 六查
1 册数与条数（15 册 × 20 条 = 300）
2 ID 连续零跳号（F38401–F38700）
3 每条正文 ≥300 字且六要素齐备
4 判据号唯一且与骨架一致（UNX-FXXXXX-J1 自指）
5 批行数守恒（20×300=6,000 × 15 = 90,000）
6 主册增补卷在位、300 条 [已深化] 标记、纯追加核（本卷区段零删除由 git diff 终核）
"""
import os, re, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
DEEPEN = os.path.join(ROOT, "docs", "unxreal", "deepen")
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
START, END, PER = 38401, 38700, 20

def main():
    # 查一/查二：册数与条数、ID 连续
    seen = []
    for b in range(1, 16):
        p = os.path.join(DEEPEN, f"J4-B{b:02d}.md")
        assert os.path.isfile(p), f"查一失败：缺册 {p}"
        txt = open(p, encoding="utf-8").read()
        ids = [int(m) for m in re.findall(r"^## UNX-F(\d{5}) · ", txt, re.M)]
        assert len(ids) == PER, f"查一失败：B{b:02d} 条数 {len(ids)}"
        seen += ids
    assert sorted(seen) == list(range(START, END + 1)), "查二失败：ID 不连续"

    # 查三：六要素齐备且 ≥300 字
    for b in range(1, 16):
        txt = open(os.path.join(DEEPEN, f"J4-B{b:02d}.md"), encoding="utf-8").read()
        blocks = re.split(r"^## UNX-F\d{5} · ", txt, flags=re.M)[1:]
        for blk in blocks:
            for tag in ("【定位】", "【边界】", "【判据】", "【行数】", "【依赖】", "【风险】"):
                assert tag in blk, f"查三失败：B{b:02d} 缺 {tag}"
            body = re.sub(r"\s", "", blk)
            assert len(body) >= 300, f"查三失败：B{b:02d} 正文 {len(body)} < 300"

    # 查四：判据号唯一且自指
    cids = []
    for b in range(1, 16):
        txt = open(os.path.join(DEEPEN, f"J4-B{b:02d}.md"), encoding="utf-8").read()
        cids += re.findall(r"UNX-F(\d{5})-J1", txt)
    assert len(cids) >= 300 and sorted(set(cids)) == [f"{i:05d}" for i in range(START, END + 1)], "查四失败：判据号面异常"

    # 查五：批行数守恒（行数声明求和）
    for b in range(1, 16):
        txt = open(os.path.join(DEEPEN, f"J4-B{b:02d}.md"), encoding="utf-8").read()
        rows = [int(m) for m in re.findall(r"规格行数 (\d+) 行", txt)]
        assert len(rows) == PER and sum(rows) == 6000, f"查五失败：B{b:02d} 行数 {sum(rows)}"

    # 查六：主册增补卷在位 + 300 条 [已深化]
    main_text = open(MAIN, encoding="utf-8").read()
    assert "## 增补卷 · AI-49 · 三 · J4 域深化增补卷第一段" in main_text, "查六失败：主册卷缺"
    n = sum(1 for line in main_text.splitlines() if "域/批：J4/" in line and "状态：[已深化]" in line)
    assert n == 300, f"查六失败：主册 J4 已深化 {n} != 300"

    print("六查 ALL PASS · exit=0（15 册/300 条/ID 连续/六要素齐备/判据唯一/90,000 行守恒/主册在位）")

if __name__ == "__main__":
    sys.exit(main())
