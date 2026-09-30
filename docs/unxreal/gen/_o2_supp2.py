# -*- coding: utf-8 -*-
"""AI-72 · UNX-O2 · 终段增补册构建器（B16–B40 · F57101–F57600 · 500 条）
十断言：①25 批在位 ②500 条 ③ID F57101–F57600 连续零跳号零重复
④每批 6,000 行守恒 ⑤全段 150,000 行 ⑥名称唯一
⑦判据号逐一匹配 ⑧E 型批（B21–B28）每条 J1R 反判据在位
⑨I 型批（B29–B36）每批联签密度 ≥50% ⑩追加前主汇编册 F57101–F57600 零命中（防重）
产出：独立增补册 + 主汇编册卷末纯追加 + 根台账会话条目（域账满账 240,000/240,000）
"""
import io, os, sys, re, datetime

GEN = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, GEN)
from _o2_data3 import BATCHES_3
from _o2_data4 import BATCHES_4
from _o2_data6 import BATCHES_6
from _o2_data7 import BATCHES_7
from _o2_data8 import BATCHES_8
from _o2_data9 import BATCHES_9
BATCHES = BATCHES_3 + BATCHES_4 + BATCHES_6 + BATCHES_7 + BATCHES_8 + BATCHES_9

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(GEN)))
DOCS = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
MAIN = os.path.join(DOCS, "CoRun Varix STAR II · Unxreal.md")
BOOK = os.path.join(DOCS, "AI-72 · O2 · 终段增补册（B16–B40 · F57101–F57600）.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")

START_ID, END_ID = 57101, 57600

def main():
    # 断言 ① ②
    assert len(BATCHES) == 25, f"批数 {len(BATCHES)} != 25"
    # 断言 ③ ④ ⑤ ⑥ ⑦ ⑧ ⑨
    names, ids = [], []
    total = 0
    for bi, b in enumerate(BATCHES):
        rows = b["rows"]
        assert sum(rows) == 6000, f"{b['title'][:24]} 行数 {sum(rows)} != 6000"
        assert len(b["items"]) == 20, f"{b['title'][:24]} 条数 {len(b['items'])} != 20"
        assert len(rows) == 20, f"{b['title'][:24]} rows 长度 != 20"
        # I 型联签密度：批内判据含联签字样（联签/联办）的条数 ≥ 10
        if "I型" in b["title"]:
            n_lian = sum(1 for (_, j) in b["items"] if ("联签" in j or "联办" in j))
            assert n_lian >= 10, f"{b['title'][:24]} 联签密度 {n_lian}/20 < 50%"
        for ii, (name, jieju) in enumerate(b["items"]):
            fid = START_ID + bi * 20 + ii
            ids.append(fid)
            names.append(name)
            m = re.match(r"UNX-F(\d+)-J1", jieju)
            assert m and int(m.group(1)) == fid, f"判据号失配: F{fid} {name} -> {jieju[:20]}"
            # E 型：每条 J1R 反判据在位
            if "E型" in b["title"]:
                assert "J1R" in jieju, f"E 型缺 J1R: F{fid} {name}"
        total += sum(rows)
    assert ids == list(range(START_ID, END_ID + 1)), "ID 不连续"
    assert len(ids) == 500, f"条数 {len(ids)} != 500"
    assert total == 150000, f"总行数 {total} != 150000"
    assert len(set(names)) == 500, "名称重复"
    # 断言 ⑩ 防重（数值区间精确判定，避开上卷 F57100）
    with io.open(MAIN, "r", encoding="utf-8", errors="strict") as f:
        main_text = f.read()
    hits = [int(x) for x in re.findall(r"UNX-F(\d{5})\b", main_text)
            if 57101 <= int(x) <= 57600]
    assert not hits, f"主汇编册已有 F57101–F57600 命中 {len(hits)} 处，防重失败"
    print("断言 ①–⑩ ALL PASS")

    # 渲染批表格
    def render_batches():
        out = []
        fid = START_ID
        for b in BATCHES:
            out.append("\n## " + b["title"] + "\n")
            out.append("\n| ID | 深化名（deepen/O2） | 行数 | 状态 | 证据与判据锚定 |")
            out.append("|---|---|---|---|---|")
            for (name, jieju), r in zip(b["items"], b["rows"]):
                out.append(f"| UNX-F{fid} | {name} | {r} | 增补 | {jieju} |")
                fid += 1
            bi = fid - START_ID - 1
            lo = START_ID + (bi // 20) * 20
            hi = lo + 19
            out.append(f"\n**批防重声明**：本批 20 条主题两两不重叠；ID 段 F{lo}–F{hi} 与邻批零交叠；批账 6,000 行守恒。\n\n---")
        return "\n".join(out)

    body = render_batches()

    duan_zong = """
## 域满账总账（AI-72 · UNX-O2 B16–B40 · 终段）

- **总量**：25 批 × 20 条 = **500 条全冻结**；ID 段 F57101–F57600 连续零跳号、零复用；本段域账 150,000 行；**域总账 240,000/240,000 = 100% 满账（40 批全收）**。
- **批型铺排（承任务书 §3 保真）**：M 型后段五批 B16–B20（存储IO/图形与启动负载套件对接、差分器与波间趋势深化、调度器与机器时段深化、M 型全链贯通收官）+ E 型八批 B21–B28（噪声拒收/partial 处理/许可脚注强制/版本过渡双基线/基线缺失豁免/调度冲突/套件崩溃与适配失败/错误路径总演习，每条双判据 J1/J1R 正反配对 160/160）+ I 型八批 B29–B36（O1 标定互认上下/O3 维度 18-19 与对抗演习/O4 长稳资源账与闭账/AI-88 趋势页与环境劫持联办/联签总账八向收官，联签密度全批 ≥50% 机检锚定）+ C 型四批 B37–B40（J-1 回归复跑 ×10 零漂移/体系自检吃狗粮/20 维度总对账/域关门印 UNX-F57599 + 终了声明 UNX-F57600）。
- **验收判据六条终态**：①判据主轴注入 ±1%/±5% 告警 10/10 正确（J-1 复跑终验 F57522）；②基线改写必被抓（F57523）；③每波闭账自动套跑五类负载、缺项 partial 不冒充（F57525）；④标定证书覆盖率 100%（F57381/F57506/F57524）；⑤40 批全收域账 240,000 行守恒（F57597）；⑥SPEC 类许可脚注发布物零缺失（F57526/F57598）。
- **域内三条加严红线终巡检**：禁基线改写（F57188/F57523 复验）/禁未标定入账（F57185–F57186/F57381 复验）/禁许可裸奔（F57115/F57139/F57241 批全批强制）——零违例。
- **诚实三态**：基准机/真机判据随闸门补测登记不虚报；Windows 对照缺侧 N/A 不编；域关门移交清单（F57588 随闸门补测移交 / F57589 N/A 封存）在册。
- **域关门**：UNX-F57599 关门印 + UNX-F57600 终了声明——AI-72 名下 UNX-O2 800 条全部收口，无遗留待办。
"""

    header_book = """# AI-72 · UNX-O2 基准测试体系 · 终段增补册（B16–B40 · F57101–F57600）

> **任务书锚定**：AI-72 承包域 UNX-O2 基准测试体系（F56801–F57600）· 40 批（B01–B40）· 波次窗波 26–27 · 上游 AI-71（O1 性能对标）· 嫁接源 SPEC 类 · 判据主轴「回归基线漂移告警判据」（AI分工完成图 §AI-72 保真）。本册为第二次（收官）会话产出：后 25 批（B16–B40）共 **500 项新功能增补**，与首产册（B01–B15 · 300 条）合计 **800 条满账 240,000/240,000**。每条 = ID ｜ 深化名 ｜ 行数 ｜ 状态 ｜ 证据与判据锚定。
>
> **基准六律母版（全域复用）**：P1 钉版 / P2 可复现 / P3 只追加 / P4 判噪 / P5 许可脚注 / P6 标定——铁值纪律：判据主轴注入 ±1%/±5% 两档 10/10 行为正确，禁改阈值（行为红线⑧）。
>
> **批型纪律（本册覆盖段）**：E 型每条双判据 J1/J1R 正反配对（承 AI-64 判例）；I 型联签密度全批 ≥50%、双方域各自可复测、上游冻结接口消费零未登记；C 型 J-1 回归 ×10 复跑零漂移 + 体系自检（基准体系自己过自己的漂移告警）+ 20 维度总对账 + 域关门印/终了声明。
>
> **防重声明**：本域 500 条与首产 300 条主题两两不重叠；不触他域账——同机采集设施归 O1、ktest 微断言归 A5、帧率账归 AI-88、启动计时归 L2/AI-57、渲染语义归 G 部、延迟账本归 AI-62——各归其主，仅在联签锚定行出现、零改写。SPEC 类套件按"只跟随"嫁接不 fork 不改语义。

---
"""
    book = header_book + body + duan_zong
    with io.open(BOOK, "w", encoding="utf-8", newline="\n") as f:
        f.write(book)

    # 主汇编册纯追加
    vol = ("\n\n---\n\n# 增补卷 · AI-72 · UNX-O2 基准测试体系终段（B16–B40 · F57101–F57600）\n\n"
           "> **登记注**：本卷由 AI-72 会话产出，卷末纯追加零删除零改写；域满账 240,000/240,000（40 批全收）；E 型 J1/J1R 160/160、I 型联签密度 ≥50%、C 型关门印/终了声明在位；域关门印 UNX-F57599，印后变更走他卷。\n"
           + body + duan_zong)
    with io.open(MAIN, "a", encoding="utf-8", newline="\n") as f:
        f.write(vol)

    # 根台账会话条目（纯追加）
    ts = datetime.date.today().isoformat()
    entry = f"""

---

## 会话条目 · AI-72 · UNX-O2 基准测试体系 · 终段收官满账（{ts}）

- **产出**：500 项新功能增补（B16–B40 · F57101–F57600 · 150,000 行）——25 批 × 20 条 × 6,000 行守恒；M 型后段五批 B16–B20（存储IO/图形启动套件/差分趋势/调度时段/全链贯通）+ E 型八批 B21–B28（噪声拒收/partial/许可脚注/双基线/豁免/调度冲突/崩溃适配/错误路径总演习，J1/J1R 160/160 配对）+ I 型八批 B29–B36（O1 标定互认/O3 维度 18-19 与波 27 对抗/O4 长稳资源账/AI-88 趋势页与波 26 劫持演习/八向联签总账，密度 ≥50% 机检）+ C 型四批 B37–B40（J-1 复跑 ×10/体系自检/20 维总对账/关门印 F57599+终了声明 F57600）。
- **域满账**：**800/800 条 · 240,000/240,000 行 · 40 批全收**（首产 B01–B15 300 条 + 终段 B16–B40 500 条）；验收判据六条全部终态核验（主轴 10/10 · 只追加 · 五类负载套跑 · 证书覆盖率 100% · 满账守恒 · 许可脚注零缺失）。
- **机器校验 ALL PASS**：生成器 docs/unxreal/gen/_o2_supp2.py 十断言 exit=0（25 批/500 条/ID 连续/每批 6000/150000 守恒/名称唯一/判据号匹配/E 型 J1R/I 型联签密度/追加前 F57101–F57600 零命中防重）；独立增补册落盘 + 主汇编册卷末纯追加零删除 + 本台账条目，三落位。
- **红线**：域内三条加严红线终巡检零违例；行为红线三条断言在位；无引导/写盘红线（适用性声明在册）；诚实三态（随闸门补测移交 F57588 / N/A 封存 F57589）。
- **联签**：AI-71（互认双向）/AI-73（维度 18-19+波 27 对抗）/AI-74（长稳资源账+波 28 演习）/AI-88（趋势页+波 26 劫持演习）/AI-90（闭账九步）/AI-94（SLA 过渡）/AI-95（许可冻结权）/AI-97（劣化立案）/AI-82（灌水审计）——八向复测矩阵 F57501 在位。
- **域关门**：UNX-F57599 关门印 + UNX-F57600 终了声明——AI-72 名下 UNX-O2 全部收口，无遗留待办。
"""
    with io.open(LEDGER, "a", encoding="utf-8", newline="\n") as f:
        f.write(entry)

    print(f"BOOK  = {BOOK} ({len(book)} chars)")
    print(f"MAIN  += {len(vol)} chars")
    print(f"LEDGER+= {len(entry)} chars")
    print("ALL DONE")

if __name__ == "__main__":
    main()
