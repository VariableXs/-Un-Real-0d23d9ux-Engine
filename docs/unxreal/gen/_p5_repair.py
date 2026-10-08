# -*- coding: utf-8 -*-
"""AI-80 · P5 首产段修复器：主册段整体替换（修正 F63401 锚位配对 + B11/B12 批序）+ 册件重写 + 台账补条目。
只动 AI-80 本域内容；断言：修后主册段 300 条 ID 连续、F63401=终审证据树组装器、册/主双写一致。
"""
import io, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _p5_firstprod as g

ROOT = g.ROOT

def main():
    rows = g.build_rows()
    # 断言：锚位配对正确
    r63401 = next(r for r in rows if r[0] == 63401)
    assert "终审证据树组装器" in r63401[2] and r63401[3] == 480, f"F63401 配对错误: {r63401}"
    b11 = [r for r in rows if r[1] == "B11"]
    assert all(63401 <= r[0] <= 63420 for r in b11) and "终审" in b11[0][2]
    print("修复前置断言：F63401=终审证据树组装器@B11 首位 · PASS")

    # 1) 主册：截断旧段 → 追加修正段 → 更新登记行
    with io.open(g.MASTER, "r", encoding="utf-8") as f:
        m = f.read()
    start = m.find("# 增补卷 · AI-80 · UNX-P5 里程碑发布与年度镜像 · 首产段 B01–B15（F63201–F63500 · 300 项）")
    assert start > 0, "主册未找到旧 AI-80 段"
    m = m[:start].rstrip()
    if m.endswith("---"):
        m = m[:-3].rstrip()
    # 登记行更新（补 F63401 归位声明）
    new_reg = g.REG_LINE.replace(
        "批位差异（任务书 B09–B20 M 型标注 vs 连续零跳号推算 B07–B18）按连续零跳号公理恒等归位并诚实登记（AI-62 判例）",
        "批位差异（任务书 B09–B20 M 型标注 vs 连续零跳号推算 B07–B18；F63401 任务书批标 B12 vs 推算 B11——终审证据树整批归位 B11、四站批顺移 B12）按连续零跳号公理恒等归位并诚实登记（AI-62/AI-74 判例）",
    ).replace(
        "生成器 docs/unxreal/gen/_p5_firstprod.py 五断言 ALL PASS exit=0。",
        "生成器 docs/unxreal/gen/_p5_firstprod.py（数据）+ _p5_repair.py（主册段整替修复 v2）五断言 ALL PASS exit=0。",
    )
    if g.REG_LINE in m:
        m = m.replace(g.REG_LINE, new_reg, 1)
    elif new_reg not in m:
        raise AssertionError("主册既无旧登记行也无新登记行")
    m = m + "\n\n" + g.build_master_section(rows)
    with io.open(g.MASTER, "w", encoding="utf-8", newline="\n") as f:
        f.write(m)
    print("主册段已整替（v2 修正版）")

    # 2) 册件重写
    with io.open(g.BOOKLET, "w", encoding="utf-8", newline="\n") as f:
        f.write(g.build_booklet(rows))
    print("增补册已重写（v2 修正版）")

    # 3) 总台账补条目（缺席才追加）
    with io.open(g.LEDGER, "r", encoding="utf-8") as f:
        led = f.read()
    if "AI-80 · 2026-10-01 · UNX-P5 首产段" not in led:
        with io.open(g.LEDGER, "a", encoding="utf-8", newline="\n") as f:
            f.write(g.LEDGER_ENTRY.replace(
                "生成器 docs/unxreal/gen/_p5_firstprod.py（单源五断言）",
                "生成器 docs/unxreal/gen/_p5_firstprod.py（数据单源）+ _p5_repair.py（v2 修复：F63401 锚位配对修正·主册段整替）"))
        print("总台账条目已追加")
    else:
        print("总台账条目已在位，跳过")

    # 4) 写后回读断言
    with io.open(g.BOOKLET, "r", encoding="utf-8") as f:
        b = f.read()
    ids = [int(x) for x in re.findall(r"^\| UNX-F(\d+) \|", b, re.M)]
    assert ids == list(range(63201, 63501)), "册件 ID 不连续"
    assert sum(int(r) for r in re.findall(r"^\| UNX-F\d+ \| [^|]+ \| (\d+) \|", b, re.M)) == 90000
    assert "UNX-F63401 | 终审证据树组装器（R1–R10） | 480" in b
    with io.open(g.MASTER, "r", encoding="utf-8") as f:
        m2 = f.read()
    seg = m2[m2.find("# 增补卷 · AI-80 · UNX-P5 里程碑发布与年度镜像 · 首产段 B01–B15（F63201–F63500 · 300 项）"):]
    seg_ids = [int(x) for x in re.findall(r"^\| UNX-F(\d+) \|", seg, re.M)]
    assert seg_ids == list(range(63201, 63501)), "主册段 ID 不连续"
    assert "UNX-F63401 | 终审证据树组装器（R1–R10） | 480" in seg
    assert m2.count("# 增补卷 · AI-80 · UNX-P5") == 1, "主册存在重复 AI-80 段"
    assert len(re.findall(r"^\| UNX-F(632\d\d|63[3-9]\d\d|64000) \|", m2[:m2.find('# 增补卷 · AI-80 · UNX-P5')], re.M)) == 0, "主册前部出现越界 P5 表行"
    print("写后回读五断言：PASS")
    print("ALL PASS exit=0")

if __name__ == "__main__":
    main()
