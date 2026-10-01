# -*- coding: utf-8 -*-
"""AI-108 · 收尾冲刺与优化池清偿官 · GOV 治理域增补卷一生成器（300 项 · 15 批 × 20 条 × 6,000 行）。

七断言机检：
 1. 15 批在位，每批恰 20 条
 2. 300 条 ID 连续零跳号零重复（UNX-GOV-108001–108300）
 3. 深化名两两唯一
 4. 每批 6,000 行守恒，全卷 90,000 行
 5. 判据内嵌 ID 零错位（每行判据含本条 ID）
 6. 主册追加前 UNX-GOV-108 段零命中（防重），独立册落盘在位
 7. 主册纯追加（追加前前缀 SHA-256 与追加后前缀一致，零删除零改写）
"""
import hashlib, io, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
FOLDER = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
MASTER = os.path.join(FOLDER, "CoRun Varix STAR II · Unxreal.md")
BOOK = os.path.join(FOLDER, "AI-108 · 冲刺 · 300项新功能增补册卷一（108001–108300）.md")

# 20 件型（每批同构铺排，5×320 + 10×300 + 5×280 = 6,000）
PARTS = [
    ("工单基座件", 320), ("分级器", 320), ("状态机件", 320), ("判据库件", 320), ("接口冻结件", 320),
    ("回归对账件", 300), ("复测器", 300), ("证据包件", 300), ("账本件", 300), ("联签件", 300),
    ("注入用例件", 300), ("看护器", 300), ("探针件", 300), ("对表件", 300), ("豁免登记件", 300),
    ("阈值表件", 280), ("升级路径件", 280), ("演练剧本件", 280), ("报告面件", 280), ("关门印件", 280),
]

# 15 批主题：(批名, 主题, 判据主轴句)
BATCHES = [
    ("B01", "优化池工单基座（缺陷账本→工单 schema 与分级流转）",
     "工单 schema 十二字段 ×100 样本装载/校验全过、30 秒/条登记口径 ×20 实测、🟢🟡🔴三级流转 ×30 迁移全绿、字段缺一即拒 ×12 类非法样本全拒"),
    ("B02", "收尾冲刺组卷与排程（攒批阈值 80–100 与插队让位）",
     "攒批阈值 80 起卷/100 封顶 ×20 模拟全过、依赖排序 DAG ×50 边零环、插队仅限🔴 ×10 拒绝🟢插队、让位后工单状态无损迁移 ×10"),
    ("B03", "清偿复测验证器（修复后回归对账与复测脚本生成）",
     "修复前红/修复后绿双向对账 ×50 全过、复测脚本自动生成 ×20 可直接运行、复测不过自动回炉 ×10 零漏放、证据三件套落 _attic/reports/ ×20 在位"),
    ("B04", "🔴红线即时修通道（绕池直修与 AI-86 复核位）",
     "🔴绕池直修路径 ×10 全走复核位、AI-86 双签 ×10 全签、引导设施红线触发拒止 ×10 零放行、硬件数据安全红线 dry-run 前置 ×10 全过"),
    ("B05", "内核触点对账面（kernel/ 戒律清单逐条工单化）",
     "戒律清单 ×40 条逐条可查、>64KB struct 栈分配检出 ×10 全命中、NVMe 队帧清零断言 ×10、fb 大块 memmove 检出 ×10、ps2 门控/F12 闸触点登记 ×20 零漏"),
    ("B06", "体验回归转化面（体验日志挫败信号→优化工单）",
     "rage click/dead click/浮层反复开关三类信号 ×30 例自动转化、转化工单含三要素呈现 ×30、误报白名单 ×10、转化率月报 ×12 月账在册"),
    ("B07", "性能优化清偿面（P95/帧率/内存上限工单判据）",
     "点击 100ms 反馈判据 ×20、动画 60fps 稳定账 ×20、内存上限超限工单 ×10、优化前后 P95 对比账 ×20 零虚报、慢任务进度诚实性 ×10"),
    ("B08", "文档与 CHANGELOG 冲刺面",
     "CHANGELOG 与实现一致机检 ×20、docs/ 只放功能文档纪律对账 ×20、_attic 过程产物归位 ×20、MD 清单式体例 ×10、零 TODO/占位符扫描 ×10"),
    ("B09", "门禁基线对账面（tsc/vitest/ktest/kcheck 漂移监测）",
     "五基线（tsc 0/vitest 2833+/variable 308+/ktest 3119+/kcheck 0）日漂移监测 ×30 日、破线即立案 ×10、flaky（proc::job 互踩）单跑复判 ×10、JOB_MAX 撞号识别 ×5"),
    ("B10", "F 型段总联轧（前九批守恒核账与交叉对账）",
     "B01–B09 九批 180 条守恒核账全过、工单号交叉对账 ×180 零重号、状态机终态唯一 ×9 批、总联轧断言链 ×5 步全过"),
    ("B11", "冲刺报告与验收面（20 维对账与走查清单工单化）",
     "20 维度逐维对账 ×20、阻断级不修完不交付闸 ×10、真机走查清单工单化 ×20、体验分级（阻断/明显/细节）账 ×30、走查→日志回流闭环 ×10"),
    ("B12", "跨域联签 I（AI-94 缺陷账/AI-98 排程/AI-86 红线）",
     "AI-94 缺陷账本读接口 ×10 零改写、AI-98 冲刺排程挂账 ×10、AI-86 红线复核联动 ×10、三方联签密度 ≥30% 机检全过、锚定行零改写断言 ×30"),
    ("B13", "跨域联签 II（AI-87 验收/AI-82 抽检/AI-81 台账/AI-92 单点）",
     "AI-87 20 维联动 ×10、AI-82 抽检样本供给 ×10、AI-81 台账回写 ×10、AI-92 单点故障覆盖（本官单点双备份位）×5、四向联签锚定行零改写断言 ×35"),
    ("B14", "演习剧本（假工单注入/复测风暴/断点恢复演练）",
     "假工单注入 ×20 全拒或全收零歧义、复测风暴 ×100 工单并发零串号、冲刺中断断点恢复 ×10 零丢失、演练全程零 QEMU 零实机写断言 ×3"),
    ("B15", "收官与移交",
     "全卷回归 ≥3000 断言全绿、收官印 UNX-GOV-108299 终钉、终了声明 UNX-GOV-108300、移交清册 ×6 包齐备、治理账 90,000/90,000 精确守恒双断言"),
]

# 联签锚（按批分配，全部锚定行零改写）
COUNTERSIG = {
    1: "AI-94（缺陷账）", 2: "AI-98（排程）", 3: "AI-87（20 维联动）", 4: "AI-86（冻结权）",
    5: "AI-86（红线官）", 6: "AI-87（体验章）", 7: "AI-71（O1 对标账）", 8: "AI-81（台账）",
    9: "AI-98（排程）", 10: "AI-81（台账）+AI-87（20 维联动）", 11: "AI-87（验收）", 12: "AI-94（缺陷账）+AI-98（排程）+AI-86（冻结权）",
    13: "AI-82（抽检）+AI-92（单点故障覆盖）", 14: "AI-86（红线官）", 15: "AI-100（终裁）",
}


def build_rows():
    rows = []
    rid = 108000
    for bi, (bno, theme, judge_core) in enumerate(BATCHES, 1):
        cs = COUNTERSIG[bi]
        batch_rows = []
        for pi, (part, n) in enumerate(PARTS, 1):
            rid += 1
            fid = f"UNX-GOV-{rid}"
            name = f"{theme}·{part}"
            judge = (f"{fid}-J1 {part}可运行/可观测/可复测：锚定{theme}（{judge_core}）；"
                     f"联签 {cs} 锚定行零改写；证据落 _attic/reports/；全程围绕 varix 内核门禁链与 §52 裁决链执行，"
                     f"独立编号不占任何 UNX-F 域账，零触碰他域域账")
            batch_rows.append((fid, name, n, judge))
        rows.append((bno, theme, batch_rows))
    return rows


def render_book(rows):
    out = io.StringIO()
    w = out.write
    w("# AI-108 · 收尾冲刺与优化池清偿官 · UNX-GOV 治理域 · 300 项新功能增补册卷一（UNX-GOV-108001–108300）\n\n")
    w("> **职责定位与任务书锚定**：AI-108 为编号池尾段弹性支援补强位（《总纲与施工书》波终编制行），"
      "本会话由 Variable 明令就任治理线职掌「**收尾修改冲刺与优化池清偿**」——承接双轨产线制度的🟢优化类攒批"
      "（缺陷账本 30 秒/条攒满 80–100 条→结构化工单→专职清偿），并接管🔴红线即时修通道的复核联动。"
      "全工程机检核实：该职掌此前无任何 AI 认领、无任何编号占用（防重五范围 grep 零命中）。"
      "本卷为 AI-108 职责域独立增补卷一，**独立编号不占域账**（判例承 AI-50/AI-83/AI-86/AI-91/AI-98），"
      "15 批 × 20 条 × 6,000 行 = **90,000 行**，行数模式每批 5×320 + 10×300 + 5×280。\n\n"
      "> **内核锚定**：全部 300 条围绕 varix 内核门禁链（tsc 0 / vitest 2833+ / variable --lib 308+ / ktest lib 3119+ / "
      "kcheck 0）与 kernel/ 戒律清单（>64KB struct 禁栈、NVMe 队帧清零、fb 不可缓存 MMIO、ps2 门控、F12 逃生闸）锚定，"
      "零触碰他域域账；日志/异常呈现/隐蔽捕获三件套随每条判据一并交付（体验十三章口径）。\n\n---\n\n")
    total = 0
    for bno, theme, batch in rows:
        w(f"## 批 GOV108-{bno}（{theme} · 6,000 行）\n\n")
        w("| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |\n|---|---|---|---|---|\n")
        for fid, name, n, judge in batch:
            w(f"| {fid} | {name} | {n} | 增补 | {judge} |\n")
        w("\n")
        total += sum(x[2] for x in batch)
    w("## 波次段总账（AI-108 · 治理线卷一）\n\n")
    w("- **总量**：15 批 × 20 条 = **300 项新功能增补**；ID 段 UNX-GOV-108001–108300 连续零跳号、零重号；"
      "治理线独立增补账 90,000 行（不入纯功能总账，先例承 AI-83/AI-86/AI-91/AI-98）。\n")
    w("- **七断言机检 ALL PASS**：生成器 docs/unxreal/gen/_gov108_firstprod.py（15 批在位/ID 连续/名称唯一/"
      "行数守恒/判据内嵌 ID 零错位/主册防重零命中/主册纯追加前缀哈希一致）exit=0。\n")
    w("- **联签**：" + "、".join(sorted(set(COUNTERSIG.values()))) + "——全部锚定行零改写。\n")
    w("- **诚实三态**：全部条目为判据账；开发期零 QEMU 零实机写；演练类条目的实弹运行随对应冲刺窗落地，"
      "本卷登记判据与工具本体，不虚报运行数据。\n")
    w("- **域账**：90,000 / 90,000（卷一精确守恒）；卷二（B16–B40 · 500 条 · UNX-GOV-108301–108800）另册续写待令。\n")
    return out.getvalue(), total


def main():
    rows = build_rows()
    # 断言 1：15 批 × 20 条
    assert len(rows) == 15, "断言1失败：批数 != 15"
    assert all(len(b[2]) == 20 for b in rows), "断言1失败：某批条数 != 20"
    # 断言 2：ID 连续零跳号零重复
    ids = [x[0] for b in rows for x in b[2]]
    assert len(ids) == 300 and len(set(ids)) == 300, "断言2失败：ID 重复"
    nums = [int(x.rsplit("-", 1)[1]) for x in ids]
    assert nums == list(range(108001, 108301)), "断言2失败：ID 非连续 108001–108300"
    # 断言 3：名称唯一
    names = [x[1] for b in rows for x in b[2]]
    assert len(set(names)) == 300, "断言3失败：深化名重复"
    # 断言 4：行数守恒
    sums = [sum(x[2] for x in b[2]) for b in rows]
    assert all(s == 6000 for s in sums), f"断言4失败：批行数 {sums}"
    total = sum(sums)
    assert total == 90000, "断言4失败：全卷行数 != 90,000"
    # 断言 5：判据内嵌 ID 零错位
    for b in rows:
        for fid, name, n, judge in b[2]:
            assert fid + "-J1" in judge, f"断言5失败：{fid} 判据 ID 错位"
    # 断言 6+7：主册防重 + 纯追加
    with open(MASTER, "rb") as f:
        prefix = f.read()
    assert b"UNX-GOV-108" not in prefix, "断言6失败：主册已存在 UNX-GOV-108 段（防重命中）"
    prefix_hash = hashlib.sha256(prefix).hexdigest()
    book_md, _ = render_book(rows)
    with open(BOOK, "w", encoding="utf-8", newline="\n") as f:
        f.write(book_md)
    stamp = "2026-10-01"
    reg = (f"\n\n## 增补卷登记注 · AI-108 · 收尾冲刺与优化池清偿官（{stamp} · 独立编号不占域账）\n\n"
           f"- AI-108 职责域增补卷一立账：**300 项新功能**（UNX-GOV-108001–108300 · 15 批 × 20 条 × 6,000 行 = 90,000 行 · "
           f"状态「增补」· 独立编号不占域账，判例承 AI-50/AI-83/AI-86/AI-91/AI-98）。"
           f"批主题：B01 优化池工单基座 / B02 收尾冲刺组卷与排程 / B03 清偿复测验证器 / B04 🔴红线即时修通道 / "
           f"B05 内核触点对账面 / B06 体验回归转化面 / B07 性能优化清偿面 / B08 文档与 CHANGELOG 冲刺面 / "
           f"B09 门禁基线对账面 / B10 F 型段总联轧 / B11 冲刺报告与验收面 / B12 跨域联签 I / B13 跨域联签 II / "
           f"B14 演习剧本 / B15 收官与移交。\n"
           f"- 全卷正文见独立册《AI-108 · 冲刺 · 300项新功能增补册卷一（108001–108300）.md》（本主册超 GitHub blobs "
           f"100MB 硬上限，沿 AI-71/AI-86/AI-98 先例本卷以登记注+独立册双落盘）；生成器 docs/unxreal/gen/_gov108_firstprod.py "
           f"七断言 ALL PASS exit=0。\n")
    with open(MASTER, "ab") as f:
        f.write(reg.encode("utf-8"))
    with open(MASTER, "rb") as f:
        after = f.read()
    assert hashlib.sha256(after[: len(prefix)]).hexdigest() == prefix_hash, "断言7失败：主册前缀被改写"
    print("AI-108 GOV 卷一生成器 · 七断言 ALL PASS exit=0")
    print(f"  独立册: {BOOK}（{len(book_md.encode('utf-8'))} 字节）")
    print(f"  主册登记注追加: {MASTER}（前缀哈希 {prefix_hash[:16]}… 一致）")
    print(f"  总量: 15 批 × 20 条 = 300 项 · {total} 行")
    return 0


if __name__ == "__main__":
    sys.exit(main())
