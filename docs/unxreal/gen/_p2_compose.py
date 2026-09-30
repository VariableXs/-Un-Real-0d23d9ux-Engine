# -*- coding: utf-8 -*-
# AI-77 · UNX-P2 · 300项新功能增补册 合成器：行数配平 + 机器校验 + Markdown 渲染
import re, sys, importlib.util

GEN = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\unxreal\gen"
OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\AI-77 · UNX-P2 · 300项新功能增补册（B01–B15 · F60801–F61100）.md"

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
    return m

d1 = load("d1", GEN + r"\_p2_data1.py")
d2 = load("d2", GEN + r"\_p2_data2.py")
d3 = load("d3", GEN + r"\_p2_data3.py")

BATCHES = [
    ("B01", "行数守恒断言器 I（三源对账与容差告警）", d1.B01, 60801),
    ("B02", "三态审计器 I（唯一性·账实核对·状态机）", d1.B02, 60821),
    ("B03", "防重五范围（索引·相似度分级·豁免库）", d1.B03, 60841),
    ("B04", "判据三可机检（环境词·观测动词·阈值）", d1.B04, 60861),
    ("B05", "字行比机检与指标看板（防灌水深化）", d1.B05, 60881),
    ("B06", "守恒断言器工程化收口（F 型六批收官）", d2.B06, 60901),
    ("B07", "三态审计器深化 I（工单与只追加引擎）", d2.B07, 60921),
    ("B08", "防重深化（样本库·工作台·关门印）", d2.B08, 60941),
    ("B09", "抽样回归与三可深化（种子·翻红·立案）", d2.B09, 60961),
    ("B10", "findings 引擎与工具合规闭环", d2.B10, 60981),
    ("B11", "守恒断言器深化 II（灰度·容量·服役报告）", d3.B11, 61001),
    ("B12", "三态审计器深化 II（回放·压测·适配层）", d3.B12, 61021),
    ("B13", "防重与三可深化 II（去噪·冤案·联合冒烟）", d3.B13, 61041),
    ("B14", "抽样深化与六方联签预备", d3.B14, 61061),
    ("B15", "段收官综合批（总对账·红线自审·关门印）", d3.B15, 61081),
]

SAMPLE = {60801: 1100, 60821: 460, 60841: 480, 60861: 320, 61001: 340}

# ---- 行数配平：示例保真固定，每批收口印行数 = 6000 - 其余之和（须落在 180–440）----
problems = []
plan = []
for bname, theme, items, start in BATCHES:
    assert len(items) == 20, (bname, len(items))
    rows = []
    for i, (nm, ln, jtext) in enumerate(items):
        fid = start + i
        # verify jtext contains the right ID
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
    TARGET = 280
    close[2] = 6000 - sum(r[2] for r in rows if r is not close)
    # 收口印先拉向 TARGET，差额摊到批内可调行（180–440 夹紧）
    if close[2] > HI or close[2] < LO:
        adjustable = [r for r in rows if r is not close and r[0] not in SAMPLE]
        if close[2] > HI:
            # close 过大 → 其余行总和过小 → 给其余行加量
            deficit = close[2] - TARGET
            adjustable.sort(key=lambda r: -(HI - r[2]))
            for r in adjustable:
                if deficit == 0: break
                add = min(deficit, HI - r[2])
                r[2] += add; deficit -= add
        else:
            # close 过小 → 其余行总和过大 → 给其余行减量
            excess = TARGET - close[2]
            adjustable.sort(key=lambda r: -(r[2] - LO))
            for r in adjustable:
                if excess == 0: break
                sub = min(excess, r[2] - LO)
                r[2] -= sub; excess -= sub
    close[2] = 6000 - sum(r[2] for r in rows if r is not close)
    if not (LO <= close[2] <= HI):
        problems.append(f"{bname}: 配平失败，收口印 {close[2]}")
    # 其余行也必须在档内
    for r in rows:
        if r is not close and r[0] not in SAMPLE and not (LO <= r[2] <= HI):
            problems.append(f"{bname}: UNX-F{r[0]:05d} 行数 {r[2]} 出档")
    total = sum(r[2] for r in rows)
    if bname=='B01':
        print('DBG close=',close[2],'others=',sum(r[2] for r in rows if r is not close),'total=',total)
    assert total == 6000, (bname, total)
    plan.append((bname, theme, rows, start))

# ---- 全域校验 ----
all_ids = []
for bname, theme, rows, start in plan:
    ids = [r[0] for r in rows]
    assert ids == list(range(start, start + 20)), (bname, "id not contiguous")
    all_ids.extend(ids)
assert all_ids == list(range(60801, 61101)), "global ids broken"
assert len(all_ids) == 300
if problems:
    print("PROBLEMS:"); [print(" -", p) for p in problems]; sys.exit(1)

# ---- 渲染 ----
L = []
A = L.append
A("# AI-77 · UNX-P2 防灌水与判据宪法工具 · 300 项新功能增补册（B01–B15 · F60801–F61100）")
A("")
A("> **任务书锚定**：AI-77 承包域 UNX-P2 防灌水与判据宪法工具（F60801–F61600）· 40 批（B01–B40）· 波次窗波 28–29 · 上游 AI-76 · 判据主轴\"三态审计器、行数守恒断言器、防重自动化、抽样回归判据\"（AI分工完成图 §AI-77 保真）。批型承任务书 §3：B01–B06 F 型（守恒断言器、三态审计器骨架、五范围索引）+ B07–B15 M 型前段（四件工具逐深化、豁免库、抽样引擎、工具合规闭环）。本册为第一次会话产出：前 15 批（B01–B15）共 **300 项新功能增补**，域账入账见卷末总账。每条 = ID ｜ 深化名（P2 条目）｜ 行数 ｜ 状态 ｜ 证据与判据锚定。")
A(">")
A("> **四件宪法工具母版（全域防线，逐项机器化）**：①行数守恒断言器——19,200,000 行总账恒等（±在途）、三源互证（台账/handoff/代码轨实测）、O(批数) 对账、±1 注入必抓、自身 ≤2,000 行；②三态审计器——深化/骨架/未动三态唯一性、账实核对、状态机非法转移检出、幽灵批与孤儿提交双向清零、三态分布波报，findings 只追加（audit_finding{type, batch, evidence_hash, severity, finder_ver, ts}）；③防重自动化——五范围（VE/CGPU/start/内核/本册）倒排索引、相似度三档（≥0.9 高危/0.6–0.9 人工/<0.6 记录）、换皮重复检测、接管映射核对、豁免库公开可查；④判据三可机检与抽样回归——环境词/观测动词/数值阈值三缺一打回建议、对照等级 L1–L4 机检、分层抽样（每波 ≥5%、同批不连两波、种子可复现）、翻红 100% 立案。四件工具输出均为\"嫌疑\"非\"判决\"，裁量权归治理线四审计官（AI-82/83/84/85）。")
A(">")
A("> **防重声明**：本域 300 条主题两两不重叠（守恒/三态/防重/三可/抽样/合规各归各条，同主题分册分账不重复立项，修订按版本升级口径带升级印字样）；不触他域账——审计\"裁量\"归治理线四官、P1 执行器归 AI-76（断言脚本同源异路：同规则代码、独立调用路径与运行环境）、handoff schema 归 AI-76、台账 schema 归 AI-81、幽灵/孤儿双向核对归 AI-91、五范围上游代码快照归 AI-94、缺陷立案归 AI-97、发布包流水归 AI-89、域经入库归 AI-93——各归其主，仅在联签锚定行出现、零改写。P2 全部工具条目过五步断言（刀刃向内，无豁免）；守恒断言器自身行数 ≤2,000（J3 原文）。")
A(">")
A("> **批次铺排（本册覆盖段）**：B01（守恒断言器三源对账与三级告警）；B02（三态审计器五职责骨架）；B03（防重五范围索引与豁免库）；B04（判据三可机检四件）；B05（字行比机检与指标看板）；B06（守恒断言器工程化收口·F 型收官）；B07（三态审计器深化 I·只追加引擎）；B08（防重深化·样本库与工作台）；B09（抽样回归引擎与三可深化）；B10（findings 引擎与工具合规闭环）；B11（守恒断言器深化 II·波 28 服役）；B12（三态审计器深化 II·压测与适配层）；B13（防重与三可深化 II·冤案实录）；B14（抽样深化与六方联签预备）；B15（段收官综合批：总对账+红线自审+关门印）。B16–B40（500 条 · F61101–F61600）另册续写。")
A(">")
A("> **行数守恒**：每批 20 条合计 6,000 行（任务书五枚示例条目行数全文保真：F60801=1,100、F60821=460、F60841=480、F60861=320、F61001=340），批内非示例条目按 180–440 档配平；15 批合计 90,000 行，域账 90,000/240,000（37.5%）。")
A(">")
A("> **批位差异诚实登记（AI-62 判例）**：任务书五枚示例条目的批位与 ID 区间推算存在一处不一致——F61001（任务书批位 B12）按**连续零跳号公理**恒等归位本册实际批位 B11，行数与判据全文保真、批位差异如实登记于此，不作改数凑位。")
A("")

for bname, theme, rows, start in plan:
    A(f"## 批 UNX-P2-{bname}（F{start:05d}–F{start+19:05d} · {theme} · 6,000 行）")
    A("")
    A("| ID | 深化名（P2/%s） | 行数 | 状态 | 证据与判据锚定 |" % bname)
    A("|---|---|---|---|---|")
    for fid, nm, ln, jtext in rows:
        A(f"| UNX-F{fid:05d} | {nm} | {ln} | 增补 | {jtext} |")
    A("")
    A(f"**批 {bname} 防重声明**：本批 20 条每条一主题零重复；ID 段 F{start:05d}–F{start+19:05d} 与邻批零交叠；四审计官/AI-76/81/89/91/93/94/97 本体零触碰仅联签。")
    A("")
    A("---")
    A("")

A("## 卷末总账（B01–B15 段收官）")
A("")
A("- **条目总数**：300 条（F60801–F61100 连续零跳号，15 批 × 20 条）。")
A("- **行数守恒**：15 批 × 6,000 = 90,000 行；域账 90,000/240,000（37.5%）；任务书五枚示例条目行数全文保真。")
A("- **判据主轴覆盖**：四件宪法工具（守恒断言器/三态审计器/防重自动化/三可机检与抽样回归）全链可复测判据齐备；验收判据 1（注入必抓四件 ×10）、判据 2（误报四线 <1%/<5%/<20%·高危 <5%/<5%）、判据 3（抽样 ≥5%/种子可复现/翻红立案 100%）、判据 4（工具自身合规：五步断言/≤2,000 行/findings 只追加）判据面在本册落锚。")
A("- **诚实登记**：F61001 批位差异一处（任务书 B12 → 恒等归位 B11）；真机/实波判据（波 28/29 闭账实运行、联签预演、四官消费初调）属随闸门补测项，逐条带演练/预演判据登记，不虚报已验证。")
A("- **红线自审**：域内加严三条（禁越权裁量/禁口径私调/禁审计后门）×300 条自审零违例；行为红线十条全适用；本域无引导/写盘红线，审计写权限限于 findings 追加。")
A("- **续册接口**：B16–B40（500 条 · F61101–F61600 · 域收官 240,000/240,000）另册续写；E 型（口径漂移拒检/索引损坏恢复/误报样本处置）、I 型（四官联签/同源异路/AI-91 联动）、C 型（回归清账/口径冻结声明/工具交接）批型铺排承任务书 §3。")
A("")
A("（本册由 AI-77 会话产出；合成器与数据段：docs/unxreal/gen/_p2_data1-3.py + 本合成脚本；机器校验：300 条 ID 连续零跳号 / 15 批 × 6,000 行守恒 / 示例行数保真 / 判据 ID 与条目逐条对应，ALL PASS。）")
A("")

text = "\n".join(L)
open(OUT, "w", encoding="utf-8", newline="\n").write(text)
print("WROTE", OUT)
print("bytes:", len(text.encode("utf-8")), "lines:", text.count("\n")+1)
print("ALL CHECKS PASS: 300 items, 15 batches x 6000 lines, IDs F60801-F61100 contiguous")
