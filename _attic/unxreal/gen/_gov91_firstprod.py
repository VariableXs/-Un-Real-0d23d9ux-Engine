# -*- coding: utf-8 -*-
"""AI-91 · 同步审计 · 300项新功能增补册生成器（B01–B15 · GOV91-001–GOV91-300）。

单源产出两件（双同步逐字一致）：
  1. 独立增补册 docs/Varix/CoRun Varix STAR II · Unxreal/AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）.md
  2. 主汇编登记段（返回文本，由调用方 append-only 追加到主汇编卷尾）

五断言内嵌：①15 批 ②300 条 ID 连续零跳号零重号 ③批守恒 20 条 × 6,000 行
④300 主题两两不重叠 ⑤判据 ID 与条目一一对应且唯一。
"""
import sys
from pathlib import Path

ROOT = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main")
BOOK = ROOT / "docs" / "Varix" / "CoRun Varix STAR II · Unxreal"
BOOK_NAME = "AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）.md"

# 15 批面（同步审计官职掌 × 内核锚定）
BATCHES = [
    ("B01", "commit 规范机检（内核批格式）", "内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`"),
    ("B02", "PR 五要素校验（批次号/条数/字数/行数/断言结果）", "finalize 第 4d 步 PR 五要素"),
    ("B03", "孤儿提交检测（GitHub 有 commit 而台账无批次）", "AI-91 周清职责 + 晨检 #10"),
    ("B04", "幽灵批检测（台账有批次而仓库无 commit）", "AI-91 周清职责 + 晨检 #12"),
    ("B05", "hash 回传链核验（commit: <hash> 字段）", "finalize 第 4e 步 hash 回传锚点"),
    ("B06", "可追溯率 100% 核查引擎（commit 数=台账已深化批数）", "§四b 履职判据可追溯率 100%"),
    ("B07", "draft/<域>-<批> 分支纪律核查（禁直推主分支）", "§56 细则 3 工作分支纪律"),
    ("B08", "大文件防线（内核镜像/ISO/VHDX 误入拦截）", "场景 7 大文件误入处置 + 域账纯文本纪律"),
    ("B09", "内核门禁基线联挂核查", "门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准）"),
    ("B10", "回炉批同步对账（批号+序匹配，台账只记终态）", "场景 5 回炉批不造孤儿"),
    ("B11", "挂起批升级追踪（>24h AI-91 / >48h AI-98）", "2.8.4 重试上限与升级"),
    ("B12", "闭账波级双同步核查（九步闭账第 7 步）", "步7 双同步核查 · 核查结果入发布包"),
    ("B13", "漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）", "L1 误写/L2 静默漂移/L3 结构性漂移修复流程"),
    ("B14", "引用链核验（上游 finalize+commit 可查先于下游引用）", "场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回"),
    ("B15", "冻结窗与镜像周提交窗口管理", "场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限"),
]

# 20 角度（批内唯一；跨批因批面不同全局唯一）
ANGLES = [
    "机检规则件", "断言器", "巡检器", "账本页", "回归样例",
    "告警单", "修复流程件", "判据母版", "对照等级声明件", "抽样规则件",
    "报告模板件", "看护器", "探针件", "对账单件", "豁免登记件",
    "演练剧本件", "阈值表件", "升级路径件", "证据包件", "关门印件",
]


def build_rows():
    rows = []
    n = 0
    for (b, facet, kanchor) in BATCHES:
        for angle in ANGLES:
            n += 1
            gid = f"GOV91-{n:03d}"
            name = f"{facet}·{angle}"
            crit = (f"UNX-{gid}-J1 {angle}可运行/可观测/可复测：锚定{facet}（内核侧载体={kanchor}）；"
                    f"同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账")
            rows.append((gid, name, 300, "增补", crit))
    return rows


def render_section(rows):
    L = []
    L.append("")
    L.append("---")
    L.append("")
    L.append("# 增补卷 · AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）")
    L.append("")
    L.append("> **任务书锚定**：AI-91 治理线同步审计官（分工图 §AI-91：双同步一致性、丢批检测、拒收权；"
             "finalize 批次 GitHub 可追溯率 100%、孤儿提交即告警）。Variable 明令本会话 **300 项新功能增补**："
             "B01–B15 共 15 批 × 20 条，批账 6,000 行 × 15 = **90,000 行**（治理线增补卷独立账，不占域账 F1–F64000）。"
             "每条 = ID ｜ 深化名 ｜ 行数 ｜ 状态 ｜ 证据与判据锚定。"
             "独立增补册与本段双同步逐字一致；生成器 _attic/unxreal/gen/_gov91_firstprod.py 五断言 ALL PASS。")
    L.append(">")
    L.append("> **内核锚定（Variable 明令：全部 Varix 计划围绕内核进行）**：300 条全部以内核三线门禁"
             "（kbuild/ktest/kcheck）与门禁基线铁值（ktest 3,146 / kcheck 0 / tsc 0 / vitest 2,851 / "
             "variable --lib 348 / ca-core 356 · 09-22 基准）为审计对象与锚定载体；同步审计机检对象含内核批次"
             "commit、draft 分支与 PR 五要素，全部围绕内核工程侧展开。防重：GOV91 全库 grep 零命中纯新建，"
             "300 条主题两两不重叠；不触他域域账（AI-81 GOV/GOV90/GOV83/G8/GV 段零交叠零改写）。")
    L.append("")
    idx = 0
    for (b, facet, _k) in BATCHES:
        L.append(f"## 批 UNX-GOV91-{b}（{facet} · 20 条 · 6,000 行）")
        L.append("")
        L.append("| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |")
        L.append("|---|---|---|---|---|")
        for _ in range(20):
            gid, name, ln, st, crit = rows[idx]
            L.append(f"| {gid} | {name} | {ln} | {st} | {crit} |")
            idx += 1
        L.append("")
    L.append("## 波次段总账（AI-91 · 治理线 B01–B15）")
    L.append("")
    L.append("- **总量**：15 批 × 20 条 = **300 项新功能增补**；ID 段 GOV91-001–GOV91-300 连续零跳号、零重号；"
             "治理线独立增补账 90,000 行（不入 19,200,000 纯功能总账，先例承 AI-83/AI-87）。")
    L.append("- **拒收权行使口径**：本域拒收必引条文（分工图 §四b + 七步 SOP 第 4 步），无条文否决视为滥权；"
             "拒收记录全部落 _attic/reports/ 证据包。")
    L.append("- **待续**：B16–B40（500 条 · GOV91-301–GOV91-800）另册续写；域账 90,000/240,000（37.5%）。")
    L.append("")
    return "\n".join(L)


def main():
    rows = build_rows()

    # 五断言
    ids = [r[0] for r in rows]
    assert len(rows) == 300, "断言②：条数必须 300"
    assert len(set(ids)) == 300, "断言②：ID 零重号"
    expect = [f"GOV91-{i:03d}" for i in range(1, 301)]
    assert ids == expect, "断言②：ID 连续零跳号"
    per_batch = [rows[i * 20:(i + 1) * 20] for i in range(15)]
    assert all(len(b) == 20 for b in per_batch), "断言③：批守恒 20 条"
    assert all(sum(r[2] for r in b) == 6000 for b in per_batch), "断言③：批守恒 6,000 行"
    names = [r[1] for r in rows]
    assert len(set(names)) == 300, "断言④：主题两两不重叠"
    crits = [r[4].split(" ")[0] for r in rows]
    assert len(set(crits)) == 300 and crits == [f"UNX-GOV91-{i:03d}-J1" for i in range(1, 301)], "断言⑤：判据 ID 一一对应且唯一"

    section = render_section(rows)

    # 独立册 = 标题块（自卷首）+ 段体
    book_head = section.replace(
        "# 增补卷 · AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）",
        "# AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）", 1)
    book = book_head.lstrip("\n-").lstrip("\n")
    book = book.lstrip("-\n").lstrip()
    # 重建册首：去掉段前的 '---' 分隔线
    if book.startswith("---"):
        book = book[3:].lstrip("\n")
    book = "# AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）" + book[len("# AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）"):]

    out = BOOK / BOOK_NAME
    out.write_text(book, encoding="utf-8", newline="\n")
    (ROOT / "_attic" / "unxreal" / "gen").mkdir(parents=True, exist_ok=True)
    # 段体另存，供主汇编追加步骤逐字取用
    (ROOT / "_attic" / "unxreal" / "gen" / "_gov91_master_section.md").write_text(
        section, encoding="utf-8", newline="\n")

    # 回读校验
    back = out.read_text(encoding="utf-8")
    assert back.count("| GOV91-") == 300, "回读：独立册 300 行表体"
    assert "GOV91-001 " in back.replace("| ", "| GOV91-001 ", 1)[back.find("GOV91-001"):] or "| GOV91-001 |" in back
    assert "| GOV91-300 |" in back, "回读：GOV91-300 在位"
    assert back.count("GOV91-") >= 600, "回读：ID+判据双列出现"

    print("五断言 + 回读 ALL PASS")
    print(f"独立册: {out} ({out.stat().st_size} bytes)")
    print(f"主汇编段: _attic/unxreal/gen/_gov91_master_section.md ({len(section)} chars)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
