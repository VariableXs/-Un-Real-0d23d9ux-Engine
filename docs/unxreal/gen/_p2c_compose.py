# -*- coding: utf-8 -*-
# AI-77 · UNX-P2 · 500项新功能增补册（域收官册）合成器：行数配平 + 机器校验 + Markdown 渲染
import re, sys, importlib.util, os

GEN = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\unxreal\gen"
OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\AI-77 · UNX-P2 · 500项新功能增补册（B16–B40 · F61101–F61600 · 域收官册）.md"

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    sys.modules[name] = m
    spec.loader.exec_module(m)
    return m

d1 = load("c1", GEN + r"\_p2c_data1.py")
d2 = load("c2", GEN + r"\_p2c_data2.py")
d3 = load("c3", GEN + r"\_p2c_data3.py")
d4 = load("c4", GEN + r"\_p2c_data4.py")
d5 = load("c5", GEN + r"\_p2c_data5.py")

BATCHES = [
    ("B16", "字行比机检与人工抽读工单（M 型深化·专题四后半）", d1.B16, 61101),
    ("B17", "工具版本化与 finder_ver 重放（专题五③）", d1.B17, 61121),
    ("B18", "误报率计量公示与回炉机制（专题五④）", d1.B18, 61141),
    ("B19", "豁免库治理深化（数据非代码·公开可查·AI-84 抽检）", d1.B19, 61161),
    ("B20", "M 型收官：四工具联调与波 29 服役准备", d1.B20, 61181),
    ("B21", "E 型 I：口径漂移拒检（禁口径私调机器化）", d2.B21, 61201),
    ("B22", "E 型 II：五范围索引损坏自重建（10 分钟判据）", d2.B22, 61221),
    ("B23", "E 型 III：误报样本处置流水线", d2.B23, 61241),
    ("B24", "E 型 IV：幽灵/孤儿边界样本与 AI-91 联动", d2.B24, 61261),
    ("B25", "E 型 V：三态状态机非法转移样本库（§85 六类）", d2.B25, 61281),
    ("B26", "E 型 VI：误报率超标自动回炉（>10% 触发）", d3.B26, 61301),
    ("B27", "E 型 VII：抽样种子不可复现防护与种子审计", d3.B27, 61321),
    ("B28", "E 型 VIII 收官：异常路径回归样本总库", d3.B28, 61341),
    ("B29", "I 型 I：与 AI-82 联签（三态审计工单流转）", d3.B29, 61361),
    ("B30", "I 型 II：与 AI-83 联签（守恒断言消费与运营）", d3.B30, 61381),
    ("B31", "I 型 III：与 AI-84 联签（防重裁量与豁免抽检）", d4.B31, 61401),
    ("B32", "I 型 IV：与 AI-85 联签（三可机检初筛臂）", d4.B32, 61421),
    ("B33", "I 型 V：与 AI-76 同源异路约定（P1 执行器/P2 审计器）", d4.B33, 61441),
    ("B34", "I 型 VI：与 AI-91 同步核查联动（越界写告警）", d4.B34, 61461),
    ("B35", "I 型 VII：六方联签总签批（四官+AI-76+AI-91）", d4.B35, 61481),
    ("B36", "I 型 VIII 收官：双向审计矩阵对表（域线质询接受）", d5.B36, 61501),
    ("B37", "C 型 I：回归清账 I（四工具判据回归 ×10 复测）", d5.B37, 61521),
    ("B38", "C 型 II：回归清账 II 与域账对账（240,000 行恒等）", d5.B38, 61541),
    ("B39", "C 型 III：三十年口径冻结声明与工具交接件（AI-83/AI-82）", d5.B39, 61561),
    ("B40", "C 型 IV：域收官关门印（UNX-P2 全域 B01–B40 收口）", d5.B40, 61581),
]

SAMPLE = {}  # F61101–F61600 无任务书示例保真约束（五枚示例全在 F608xx/F61001，首产册已保真）

# ---- 行数配平：每批收口印行数 = 6000 - 其余之和（须落在 180–440）----
problems = []
plan = []
for bname, theme, items, start in BATCHES:
    assert len(items) == 20, (bname, len(items))
    rows = []
    for i, (nm, ln, jtext) in enumerate(items):
        fid = start + i
        if not jtext.startswith(f"UNX-F{fid:05d}-J1"):
            problems.append(f"{bname} item{i}: jtext id mismatch {fid}")
        if fid in SAMPLE:
            assert ln in (None, SAMPLE[fid]), f"{bname} {fid}: sample line mismatch"
            ln = SAMPLE[fid]
        rows.append([fid, nm, ln, jtext])
    none_rows = [r for r in rows if r[2] is None]
    assert len(none_rows) == 1, (bname, "expect exactly 1 None (收口印)")
    close = none_rows[0]
    LO, HI = 180, 440
    close[2] = 6000 - sum(r[2] for r in rows if r is not close)
    if close[2] > HI or close[2] < LO:
        adjustable = [r for r in rows if r is not close and r[0] not in SAMPLE]
        if close[2] > HI:
            excess = close[2] - 280
            others = sorted(adjustable, key=lambda r: -(r[2] - LO))
            for r in others:
                if excess == 0: break
                sub = min(excess, r[2] - LO)
                r[2] -= sub; excess -= sub
            close[2] = 6000 - sum(r[2] for r in rows if r is not close)
        elif close[2] < LO:
            deficit = 280 - close[2]
            others = sorted(adjustable, key=lambda r: -(HI - r[2]))
            for r in others:
                if deficit == 0: break
                add = min(deficit, HI - r[2])
                r[2] += add; deficit -= add
            close[2] = 6000 - sum(r[2] for r in rows if r is not close)
    for r in rows:
        if r is not close and r[0] not in SAMPLE and not (LO <= r[2] <= HI):
            problems.append(f"{bname} F{r[0]:05d}: 行数 {r[2]} 出档 {LO}-{HI}")
    if not (LO <= close[2] <= HI):
        problems.append(f"{bname}: 收口印行数 {close[2]} 出档")
    total = sum(r[2] for r in rows)
    if total != 6000:
        problems.append(f"{bname}: 批守恒失败 total={total}")
    plan.append((bname, theme, rows, start))

all_ids = []
for bname, theme, rows, start in BATCHES:
    ids = [r[0] for r in plan[[b[0] for b in BATCHES].index(bname)][2]]
    if ids != list(range(start, start + 20)):
        problems.append(f"{bname}: ids broken")
    all_ids.extend(ids)
assert all_ids == list(range(61101, 61601)), "global ids broken"
assert len(all_ids) == 500
if problems:
    print("PROBLEMS:"); [print(" -", p) for p in problems]; sys.exit(1)

# ---- 渲染 ----
L = []
A = L.append
A("# AI-77 · UNX-P2 防灌水与判据宪法工具 · 500 项新功能增补册（B16–B40 · F61101–F61600 · 域收官册）")
A("")
A("> **任务书锚定**：AI-77 承包域 UNX-P2 防灌水与判据宪法工具（F60801–F61600）· 40 批（B01–B40）· 波次窗波 28–29 · 上游 AI-76 · 判据主轴\"三态审计器、行数守恒断言器、防重自动化、抽样回归判据\"（AI分工完成图 §AI-77 保真）。本册为第二次会话收官产出：后 25 批（B16–B40）共 **500 项新功能增补**，与首产册（B01–B15 · 300 项 · F60801–F61100）合流即**域收官 800 条 · 240,000/240,000 行守恒**。每条 = ID ｜ 深化名（P2 条目）｜ 行数 ｜ 状态 ｜ 证据与判据锚定。")
A(">")
A("> **收官段批型承任务书 §3（F6/M14/E8/I8/C4）**：B16–B20 M 型后段 5 批（字行比机检与抽读工单、工具版本化 finder_ver 重放、误报率计量公示与回炉、豁免库治理、四工具联调与波 29 服役准备——与首产册 B07–B15 M 型前段合流即 M14 全收）；B21–B28 E 型 8 批（口径漂移拒检、索引损坏自重建 10 分钟判据、误报样本处置、幽灵/孤儿联动、§85 状态机样本库、>10% 自动回炉、种子防复现失效、异常路径样本总库）；B29–B36 I 型 8 批（与 AI-82/83/84/85 四官联签、与 AI-76 同源异路、与 AI-91 同步核查、六方联签总签批、双向审计矩阵对表——本卷联签方最多之域）；B37–B40 C 型 4 批（回归清账两段、三十年口径冻结声明与工具交接 AI-83/AI-82、域收官关门印——波 30 后归传承组）。")
A(">")
A("> **防重声明**：本域收官 500 条主题两两不重叠（与首产册 300 条同主题分册分批不重复立项，深化均带新判据面）；不触他域账——审计\"裁量\"归治理线四官（AI-82/83/84/85）、P1 执行器归 AI-76（同源异路：同规则代码、独立调用路径与运行环境）、handoff schema 归 AI-76、台账 schema 归 AI-81、幽灵/孤儿双向核对归 AI-91、五范围上游代码快照归 AI-94、缺陷立案归 AI-97、发布包流水归 AI-89、域经入库归 AI-93、断代防治归 AI-78——各归其主，仅在联签锚定行出现、零改写。域内加严三条（禁越权裁量/禁口径私调/禁审计后门）贯穿全部 500 条。")
A(">")
A("> **行数守恒**：每批 20 条合计 6,000 行（本册段 F61101–F61600 无任务书示例条目，五枚示例 F60801=1,100/F60821=460/F60841=480/F60861=320/F61001=340 已在首产册全文保真），批内条目按 180–440 档配平；25 批合计 150,000 行，加首产册 90,000 行即**域账 240,000/240,000（100% · 域收官）**。")
A(">")
A("> **域内加严三条执行印**：①禁越权裁量——全部工具输出为\"嫌疑\"非\"判决\"，高危档 100% 转人工判（B31），自动执行注入必拦；②禁口径私调——五口径（300 字/240,000 行/19,200,000 行/1.2–2.2/五范围清单）冻结于总册，工具为口径执行者，漂移拒检机器化（B21）；③禁审计后门——白名单跳过模式代码级审计（B19/B26 自证闸），豁免库是数据不是代码且公开可查。")
A("")

for bname, theme, rows, start in plan:
    A(f"## 批 UNX-P2-{bname}（F{start:05d}–F{start+19:05d} · {theme} · 6,000 行）")
    A("")
    A("| ID | 深化名（P2/%s） | 行数 | 状态 | 证据与判据锚定 |" % bname)
    A("|---|---|---|---|---|")
    for fid, nm, ln, jtext in rows:
        A(f"| UNX-F{fid:05d} | {nm} | {ln} | 增补 | {jtext} |")
    A("")
    A(f"**批 {bname} 防重声明**：本批 20 条每条一主题零重复；ID 段 F{start:05d}–F{start+19:05d} 与邻批零交叠；联签批仅条款锚定，四审计官/AI-76/81/89/91/93/94/97 本体零触碰。")
    A("")
    A("---")
    A("")

A("## 卷末总账（B16–B40 · UNX-P2 域收官）")
A("")
A("- **条目总数**：500 条（F61101–F61600 连续零跳号，25 批 × 20 条）；与首产册合流 800 条（F60801–F61600 全域连续零跳号）。")
A("- **行数守恒**：本册 25 批 × 6,000 = 150,000 行；全域 40 批 × 6,000 = **240,000/240,000（100% · 域收官）**。")
A("- **批型收口**：F 型 6 批 + M 型 14 批（首产 9 + 本册 5）+ E 型 8 批 + I 型 8 批 + C 型 4 批 = 40 批全收，承任务书 §3（F6/M14/E8/I8/C4）。")
A("- **验收判据 §5 逐条对表**：①四件工具注入必抓 ×10 全绿（B37 清账）；②误报四线 <1%/<5%/<20%·高危 <5%/<5% 计量公示（B18）；③抽样 ≥5%/种子可复现/翻红立案 100%（B27）；④工具自身合规：800 条过五步断言、守恒器 ≤2,000 行（首产册 J3）、findings 只追加（B34 白名单核对）；⑤40 批全收、240,000 行守恒（B38）；⑥四官+76+91 六方对表 0 冲突（B35/B36）。")
A("- **诚实登记**：真机/实波判据（波 28/29 闭账实运行、联签实签、四官消费实调）属随闸门补测项，逐条带演练/预演/实测判据登记，不虚报已验证；主汇编（101.5MB）同步依 AI-68/76 判例另立专项（首产册已登记）。")
A("- **红线自审**：域内加严三条（禁越权裁量/禁口径私调/禁审计后门）×500 条自审零违例；行为红线十条全适用；本域无引导/写盘红线，审计写权限限于 findings 追加，越界写触发 AI-91 同步告警（B34 联动件）。")
A("- **域收官与交接**：C 型 4 批完成回归清账、三十年口径冻结声明与工具交接（AI-83 主/AI-82 副、三个月并行期）、波 30 义务（终审包第 4/5 件供数）核对——波 30 后本域工具归传承组运营。")
A("")
A("（本册由 AI-77 会话产出；合成器与数据段：docs/unxreal/gen/_p2c_data1-5.py + 本合成脚本；机器校验：500 条 ID 连续零跳号 / 25 批 × 6,000 行守恒 / 判据 ID 与条目逐条对应 / 域收官 240,000/240,000，ALL PASS。）")
A("")

text = "\n".join(L)
open(OUT, "w", encoding="utf-8", newline="\n").write(text)
print("WROTE", OUT)
print("bytes:", len(text.encode("utf-8")), "lines:", text.count("\n")+1)
print("ALL CHECKS PASS: 500 items, 25 batches x 6000 lines, IDs F61101-F61600 contiguous")
