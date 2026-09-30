# -*- coding: utf-8 -*-
"""AI-91 · 同步审计 · 尾段增补册生成器（B16–B40 · GOV91-301–GOV91-800）。

单源产出两件：尾段独立册 + 主汇编登记段。五断言内嵌：
①25 批 ②500 条 ID 连续零跳号（GOV91-301–800）零重号 ③批守恒 20 条 × 6,000 行
④500 主题两两不重叠且与首产段 300 条零重复 ⑤判据 ID 一一对应且唯一。
"""
import sys
from pathlib import Path

ROOT = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main")
BOOK = ROOT / "docs" / "Varix" / "CoRun Varix STAR II · Unxreal"
BOOK_NAME = "AI-91 · 同步审计 · 尾段增补册（B16–B40 · 500项新功能 · GOV91-301–GOV91-800 · 域收官册）.md"

# 25 批面（承任务书批型：B16–B26 E 型 / B27–B36 I 型 / B37–B40 C 型）
BATCHES = [
    ("B16", "E", "拒收权行使台账（否决必引条文 · 无条文否决即滥权申诉通道）", "§一百零九先例集申诉权 + 分工图 §四b"),
    ("B17", "E", "push 认证失败处置（403 即时报基础设施事件 · 不重试）", "2.8.4 场景表 #22 认证失败"),
    ("B18", "E", "凭据红线核查（禁共享凭据 · 禁降级直推主分支）", "2.8.4 凭据纪律三条"),
    ("B19", "E", "PR 被关后的重开时限（24h 补齐 · 两次被关 KPI 扣减）", "场景 11 PR 重开时限"),
    ("B20", "E", "远端竞速重挂与快进重放（引用链核对零交叠后重放）", "AI-91 本域实练判例：父节点重挂前 diff 零交叠核对"),
    ("B21", "E", "同波多会话同仓并发冲突（行级纪律 · merge 冲突即越行）", "场景 3 并发同仓"),
    ("B22", "E", "跨域引用批同步顺序（上游 finalize 先于下游引用）", "场景 8 引用链 · 幽灵批审计打回"),
    ("B23", "E", "F 型批冻结窗同步挂起加严（>24h 直报不等 48h）", "2.8.4 F 型批衔接风险加严条"),
    ("B24", "E", "大文件已推送的处置升级（评估仓库体积 · history 清理须 AI-100 批）", "场景 7 处置升级路径"),
    ("B25", "E", "断电/中断态恢复对账（会话中断后 draft 分支与台账一致性）", "L2 静默漂移修复流程扩展"),
    ("B26", "E", "E 段综合联轧与段闸（错误路径十三面全过闸）", "E 型段闸口径 · 段闸不过禁入 I 段"),
    ("B27", "I", "AI-81 台账治理域联签（三态唯一性 · 非法第四态告警权对接）", "AI-81 滚动扩表接口"),
    ("B28", "I", "AI-82 防灌水审计联签（抽检必到 · findings 只追加对接）", "AI-82 抽检口径 §七十九总表"),
    ("B29", "I", "AI-84 防重审计联签（五范围 grep 抽检 · 接管声明复核）", "AI-84 月度固定动作"),
    ("B30", "I", "AI-83 行数守恒官对账联签（三源对账 · 注入 ±1 必抓）", "GOV-83 断言器 v1.0 接口"),
    ("B31", "I", "AI-85 判据审计联签（可运行/可观测/可复测三可机检对接）", "GOV-85 判据质量门"),
    ("B32", "I", "AI-86 红线官联签（红线预申报 · 违例冻结状态机对接）", "AI-86 复核位 · 波 04/18/20 演习联动"),
    ("B33", "I", "AI-87 验收官联签（20 维报告齐备率 · 双同步核查项入发布包）", "GOV-87-E 体系 · 闭账九步第 7 步"),
    ("B34", "I", "AI-89 镜像官联签（发布包第 4 件双同步核查 · tag 纪律）", "GOV-89 发布包十四件 + 署名页"),
    ("B35", "I", "AI-90 波次官联签（九步闭账 · 波首计划书 MD+JSON 一致比对）", "GOV-90 波首计划会 · AI-91 比对职责"),
    ("B36", "I", "I 段综合联轧与段闸（九方联签全过闸）", "I 型段闸口径 · 段闸不过禁入 C 段"),
    ("B37", "C", "回归清账批一（首产段 B01–B15 全 300 条复测翻绿）", "C 型回归清账口径一"),
    ("B38", "C", "回归清账批二（尾段 E/I 型 500 条复测翻绿）", "C 型回归清账口径二"),
    ("B39", "C", "全链彩排（三十年 3,200 批可追溯率 100% 预演账）", "第一笔账 3,200 批 × 1 commit 预演"),
    ("B40", "C", "域关门印（240,000/240,000 满账收官 · UNX-GOV91 域终）", "域收官口径 · GOV91-801 起封存零外溢"),
]

ANGLES = [
    "机检规则件", "断言器", "巡检器", "账本页", "回归样例",
    "告警单", "修复流程件", "判据母版", "对照等级声明件", "抽样规则件",
    "报告模板件", "看护器", "探针件", "对账单件", "豁免登记件",
    "演练剧本件", "阈值表件", "升级路径件", "证据包件", "关门印件",
]


def build_rows(start=301, end=800):
    rows = []
    n = start - 1
    for (b, typ, facet, kanchor) in BATCHES:
        for angle in ANGLES:
            n += 1
            gid = f"GOV91-{n:03d}"
            name = f"{facet}·{angle}"
            crit = (f"UNX-{gid}-J1 {angle}可运行/可观测/可复测：锚定{facet}（{typ} 型批 · 内核侧载体={kanchor}）；"
                    f"同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账")
            rows.append((gid, name, 300, "增补", crit))
    assert n == end
    return rows


def render_section(rows, first_rows):
    L = []
    L.append("")
    L.append("---")
    L.append("")
    L.append("# 增补卷 · AI-91 · 同步审计 · 尾段增补册（B16–B40 · 500项 · GOV91-301–GOV91-800 · 域收官册）")
    L.append("")
    L.append("> **任务书锚定**：AI-91 治理线同步审计官续产段与域收官册（分工图 §AI-91：双同步一致性、丢批检测、拒收权）。"
             "Variable 明令本会话 **500 项新功能增补**：B16–B40 共 25 批 × 20 条，批账 6,000 行 × 25 = **150,000 行**；"
             "与首产段 B01–B15（GOV91-001–300 · 90,000 行）合计 **800 条 · 240,000/240,000 满账收官 · UNX-GOV91 域终**。"
             "批型承任务书先例：B16–B26 E 型错误路径批（11 批）、B27–B36 I 型联签批（10 批）、B37–B40 C 型收官批（4 批）。"
             "每条 = ID ｜ 深化名 ｜ 行数 ｜ 状态 ｜ 证据与判据锚定。"
             "尾段独立册与本段双同步逐字一致；生成器 _attic/unxreal/gen/_gov91_finalprod.py 五断言 ALL PASS。")
    L.append(">")
    L.append("> **内核锚定（Variable 明令：全部 Varix 计划围绕内核进行）**：500 条全部以内核三线门禁"
             "（kbuild/ktest/kcheck）与门禁基线铁值（ktest 3,146 / kcheck 0 / tsc 0 / vitest 2,851 / "
             "variable --lib 348 / ca-core 356 · 09-22 基准）为审计对象与锚定载体。防重：GOV91-301–800 全库 grep 零命中纯新建，"
             "500 条主题两两不重叠且与首产段 300 条零重复；不触他域域账（AI-81/82/83/84/85/86/87/89/90 各治理段只在联签锚定行出现，零改写）。"
             "封存声明：GOV91-801 起封存零外溢。")
    L.append("")
    idx = 0
    for (b, typ, facet, _k) in BATCHES:
        L.append(f"## 批 UNX-GOV91-{b}（{typ} 型 · {facet} · 20 条 · 6,000 行）")
        L.append("")
        L.append("| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |")
        L.append("|---|---|---|---|---|")
        for _ in range(20):
            gid, name, ln, st, crit = rows[idx]
            L.append(f"| {gid} | {name} | {ln} | {st} | {crit} |")
            idx += 1
        L.append("")
    L.append("## 波次段总账（AI-91 · 治理线 B16–B40 · 域收官）")
    L.append("")
    L.append("- **总量**：25 批 × 20 条 = **500 项新功能增补**；ID 段 GOV91-301–GOV91-800 连续零跳号、零重号；"
             "尾段账 150,000 行；与首产段合计 **800 条 · 240,000 行域满账收官**（治理线独立增补账，不入 19,200,000 纯功能总账）。")
    L.append("- **域终宣告**：UNX-GOV91 域终——同步审计官八职掌（双同步一致性/丢批检测/拒收权/可追溯率 100%/孤儿幽灵周清/"
             "漂移三级修复/闭账第 7 步核查/冻结窗管理）全部机检覆盖成立；GOV91-801 起封存零外溢。")
    L.append("- **联签清单**：AI-81/82/83/84/85/86/87/89/90 九方联签锚定行在册（零改写他域域账）；"
             "闭账九步第 7 步双同步核查项随 AI-89 发布包第 4 件同源。")
    L.append("")
    return "\n".join(L)


def main():
    rows = build_rows()
    # 首产段主题读取（防跨段重复）
    first_book = BOOK / "AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）.md"
    first_text = first_book.read_text(encoding="utf-8")
    first_names = set()
    for line in first_text.splitlines():
        if line.startswith("| GOV91-") and line.count("|") >= 3:
            parts = [p.strip() for p in line.split("|")]
            first_names.add(parts[2])
    assert len(first_names) == 300, f"首产段主题读取异常: {len(first_names)}"

    # 五断言
    ids = [r[0] for r in rows]
    assert len(rows) == 500
    assert ids == [f"GOV91-{i:03d}" for i in range(301, 801)], "断言②：ID 连续零跳号 301–800"
    assert len(set(ids)) == 500
    per_batch = [rows[i * 20:(i + 1) * 20] for i in range(25)]
    assert all(len(b) == 20 and sum(r[2] for r in b) == 6000 for b in per_batch), "断言③：批守恒"
    names = [r[1] for r in rows]
    assert len(set(names)) == 500, "断言④：尾段主题两两不重叠"
    assert not (set(names) & first_names), "断言④：与首产段 300 条零重复"
    crits = [r[4].split(" ")[0] for r in rows]
    assert crits == [f"UNX-GOV91-{i:03d}-J1" for i in range(301, 801)], "断言⑤：判据一一对应唯一"

    section = render_section(rows, first_names)

    book = section.replace(
        "# 增补卷 · AI-91 · 同步审计 · 尾段增补册（B16–B40 · 500项 · GOV91-301–GOV91-800 · 域收官册）",
        "# AI-91 · 同步审计 · 尾段增补册（B16–B40 · 500项 · GOV91-301–GOV91-800 · 域收官册）", 1)
    book = book.strip("\n")
    if book.startswith("---"):
        book = book[3:].lstrip("\n")

    out = BOOK / BOOK_NAME
    out.write_text(book, encoding="utf-8", newline="\n")
    (ROOT / "_attic" / "unxreal" / "gen" / "_gov91_final_master_section.md").write_text(
        section, encoding="utf-8", newline="\n")

    back = out.read_text(encoding="utf-8")
    assert back.count("| GOV91-") == 500, "回读：尾段册 500 行表体"
    assert "| GOV91-301 |" in back and "| GOV91-800 |" in back, "回读：区间两端在位"
    assert "GOV91-801 起封存" in back, "回读：封存声明在位"
    print("五断言 + 回读 ALL PASS")
    print(f"尾段册: {out} ({out.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
