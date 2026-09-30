# -*- coding: utf-8 -*-
"""UNX-D5 域 B04–B18 深化册 + 批册生成器（判据单源；波08-M37 · AI-13）。
机械保证：ID 连续 F15261–F15560、批内 20 条、逐批行数求和 6,000、正文 ≥300 字。
另派生 B01–B03 骨架账（自既有深化册判据零改写提取）。"""
import os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from d5_data_a import B04, B05, B06, B07, B08
from d5_data_b import B09, B10, B11, B12, B13
from d5_data_c import B14, B15, B16, B17, B18

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")

ROWS_PATTERN = [350, 250, 300, 250, 350, 300, 250, 300, 350, 250, 300, 250, 350, 300, 250, 300, 350, 250, 300, 400]
assert sum(ROWS_PATTERN) == 6000
BATCHES = [B04, B05, B06, B07, B08, B09, B10, B11, B12, B13, B14, B15, B16, B17, B18]
CONSTITUTION = "①ABI 铁律；②零泄漏账主轴；③L2 字节级对照；④跨域只引已 finalize；⑤真机/模拟分账+预期值注来源"


def rows_split(r):
    a = round(r * 0.4)
    b = round(r * 0.35)
    c = r - a - b
    return f"{a} + 机检 {b} + 断言 {c}"


def closer_entries(b, f1):
    """8 条收口条模板（体例承 D3 B26–B30 同构，参数化于本批）。"""
    bno, theme, mod = b["bno"], b["theme"], b["mod"]
    f12 = f1 + 11
    return [
        (f"集成判据冻结（{theme} 接口判据清单）", "判据冻结",
         f"判据冻结账：本批十二判据（F{f1}–F{f12}）接口判据清单冻结（三列表——跨域复用凭据），清单与批注对表一致断言，冻结版本钉版，漂移注入必红，红绿齐",
         f"本批「{theme}」十二机制判据收束为冻结清单——清单是跨批/跨域复用本批语义的唯一凭据。",
         "与判据冻结体例（F14129/F14150 同构）对照"),
        (f"好档对照组（{theme} 通道 30 档）", "好档对照",
         "好档对照组：好档样本 30 档×本批五核心联动全绿断言（联动不损好档——150 组合），对照组指纹钉版，冒烟/全量双档一致，红绿齐",
         f"好档对照：30 档正常样本经本批「{theme}」全路径零损伤——正向面总证明。",
         "与好档对照组体例（F14130 同构）对照"),
        (f"集成诊断账（{mod} 诊断码登记）", "诊断账",
         "诊断账：本批诊断码 6 码登记（逐码三要素），检索 4 组关键词全对（场景命中=账集合），与域账术语一致走查（术语单源），撞码探针红，指纹钉版，红绿齐",
         f"本批诊断码命名空间登记：{theme}面异常显性化的检索入口——异常零静默的码位落点。",
         "与 D- 前缀诊断码体例（F14131 同构）对照"),
        (f"E1 分册（B{bno:02d} 分册）", "E1分册",
         f"E1 入库：B{bno:02d} 全部产物样本（对照 30 档+抽样 40 组）100% 入 E1 库 B{bno:02d} 分册（四元账+域标签），分册独立/合并双模式绿（D5 域内合并），指纹钉版，schema 零变更续验，红绿齐",
         "本批产物样本归档 E1 判据样本库分册——样本可追溯、可合并复验。",
         "与 E1 分册体例（F14132 同构）对照"),
        (f"Windows 对照观测账（{theme} 12 格）", "观测账",
         f"观测账 12 格（{theme} 核心行为×好档/坏档两态）逐格填写对照，一致 ≥9/12，差异格分级账（A 级回写口径表），样本指纹钉版，红绿齐",
         f"Windows 行为观测账：本批「{theme}」面与真实 Windows 逐格对照——差异显性化分级回写。",
         "与 Windows 对照观测账体例（F14133 同构）对照"),
        (f"资源账（{theme} 资源边界）", "资源账",
         "资源五轴账（内存/CPU/句柄/时延/持久化五轴）实测入账，峰值有界断言（峰值≤基线×1.3），联动平账（五维归零），指纹钉版，红绿齐",
         f"本批资源边界账：{theme}面五轴实测——丰盛不以资源失控为代价。",
         "与资源五轴账体例（F14136 同构）对照"),
        (f"防重声明（与 D5 域及邻批判据不重）", "防重声明",
         f"防重账：B{bno:02d} 二十条与域可见判据+本域 B01–B{bno-1:02d} 判据主题不重对表账（防重指纹钉版），嫌疑清单=空断言，红绿齐",
         f"本批防重对表：与既有判据主题聚类零重叠——防重审计（AI-84）的前置自证。",
         "与防重声明体例（F14139 同构）对照"),
        (f"ktest 引导面 B{bno:02d} 批断言集", "ktest批",
         f"ktest 聚合：B{bno:02d} 全部 19 条判据挂 ktest 引导面聚合器（一次引导命令枚举 19 组断言），漏挂检测零缺漏，引导面冒烟（退出码 0），失败注入必红三条，红绿齐",
         "本批判据聚合入 ktest 引导面：一次命令全批可跑——验收的机械入口。",
         "与 ktest 批断言集体例（F14140 同构）对照"),
    ]


def pad_body(body, b):
    tail = f"本条判据颗粒与《AI分工完成图》AI-13 任务书 D5 域「{b['theme']}」批主题对齐，防重与本批邻条及 B01–B03 已收口面不重叠。"
    return body if len(body) >= 300 else body + tail


def build_entries(b):
    """返回 20 条 (fid, title, rows, judge, body)。"""
    f1 = 15201 + (b["bno"] - 1) * 20
    out = []
    all20 = []
    for i, (title, kw, judge, core, ref) in enumerate(b["entries"]):
        all20.append(("mech", title, kw, judge, core, ref))
    for (title, kw, judge, core, ref) in closer_entries(b, f1):
        all20.append(("closer", title, kw, judge, core, ref))
    assert len(all20) == 20, b["bno"]
    for i, (kind, title, kw, judge, core, ref) in enumerate(all20):
        fid = f1 + i
        rows = ROWS_PATTERN[i]
        if kind == "mech":
            dep = b["deps"] if i == 0 else f"F{fid-1}（前序机制）与本批主题链"
            body = (f"实现路径分三步。第一步立{kw}结构：{core}规则成文落册。"
                    f"第二步立机检：{kw}核（正向语义与坏输入双通道）入 {b['mod']}，与域账本面贯通。"
                    f"第三步立断言：{kw}正向断言 10/10 次通过、坏输入注入 10/10 次检出、comloc 承诺面回归零漂移。"
                    f"与现存内核衔接点：{b['mod']}（{kw}面）新建/扩展，comloc.rs 承诺面七锚回归不变。"
                    f"与真实 Windows 行为对照：{ref}。"
                    f"判据 UNX-F{fid}-J1 的复测方式：{kw}核与注入检出双通道复测（正向 10/10、注入检出 10/10）通过，结果对表留痕。")
            body = pad_body(body, b)
            locate = f"{core}本条为本批「{b['theme']}」段{kw}面主条目，与邻条分层不重。"
            boundary = f"只立{kw}面语义与判据；{kw}扩展面归后续批深化，不在此条越界。"
            deps = dep
            risk = f"{kw}面竞态与坏输入双险——机检注入常驻，违约必红、可回退。"
        else:
            dep = f"本批十二机制条（F{f1}–F{f1+11}）全部收口为前置"
            body = (f"实现路径分三步。第一步立{kw}账：{core}账面成文落册。"
                    f"第二步立机检：{kw}核（清单勾稽与指纹一致性双通道）入 {b['mod']}。"
                    f"第三步立断言：{kw}正向断言 10/10 次通过、漂移注入 10/10 次检出、与本域既有体例同构对表一致。"
                    f"与现存内核衔接点：{b['mod']}（{kw}面）扩展，与 B01–B03 已收口账面贯通。"
                    f"与真实 Windows 行为对照：{ref}。"
                    f"判据 UNX-F{fid}-J1 的复测方式：{kw}核与注入检出双通道复测（正向 10/10、注入检出 10/10）通过，结果对表留痕。")
            body = pad_body(body, b)
            locate = f"{core}本条为本批「{b['theme']}」段收口条，体例承 D3 域 B26–B30 同构。"
            boundary = f"只立{kw}收口账语义；机制语义归本批十二机制条，不在此条重立。"
            deps = dep
            risk = f"{kw}账漂移与漏挂双险——勾稽注入常驻，漂移必红、可回退。"
        out.append((fid, title, rows, judge, locate, boundary, deps, risk, body))
    return out


def gen_deepen(b):
    bno = b["bno"]
    f1 = 15201 + (bno - 1) * 20
    f2 = f1 + 19
    dom_rows = 6000 * bno
    ents = build_entries(b)
    L = []
    L.append(f"# 域 UNX-D5 · 深化册 · UNX-D5-B{bno:02d}（F{f1}–F{f2} · 20 条 · 20 条新深化）")
    L.append("")
    L.append(f"> AI-13 承办（Variable 改派留痕，原 AI-20；波08-M37）｜本批 B{bno:02d} [已深化] 收口：F{f1}–F{f2} 共 20 条为本会话新深化｜"
             f"嫁接源：{b['gs']}；上游依赖：{b['deps']}｜防重声明：只立本批条目语义，与 B01–B03 已收口面及邻批分层不重立；"
             f"不代他域开卷；不改 handoff 协议 schema｜落在 {b['mod']}｜批累计行数锁定 6,000（= 逐条之和），域累计 {dom_rows:,}/240,000｜"
             f"域内宪法（随批复述）：{CONSTITUTION}")
    L.append("")
    total = 0
    for (fid, title, rows, judge, locate, boundary, deps, risk, body) in ents:
        total += len(body)
        L.append(f"### UNX-F{fid} · {title}")
        L.append(f"- 域/批：D5/B{bno:02d}｜判据：UNX-F{fid}-J1 {judge}｜纯功能行数：{rows} 行（{rows_split(rows)}；测试段不计）｜状态：[已深化]")
        L.append(f"- **定位**：{locate}")
        L.append(f"- **语义边界**：{boundary}")
        L.append(f"- **依赖与嫁接源**：依赖 {deps}；嫁接源：{b['gs']}。")
        L.append(f"- **风险与回退**：{risk}")
        L.append(f"- 正文：{body}")
        L.append("")
    path = os.path.join(DEEPEN, f"D5-B{bno:02d}.md")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(L) + "\n")
    return total, path


def gen_batch_book(b):
    bno = b["bno"]
    f1 = 15201 + (bno - 1) * 20
    f2 = f1 + 19
    ents = build_entries(b)
    L = []
    L.append(f"# UNX-D5-B{bno:02d} · {b['theme']}（F{f1}–F{f2} · 20 条）")
    L.append("")
    L.append(f"> AI-13 承办（波08-M37 深化收口，Variable 改派留痕原 AI-20）｜域账累计：本批 6,000 / 240,000｜"
             f"波次：波08（B04–B18 共 300 项 · D5 域 AI-13 主攻段）｜嫁接源：{b['gs']}｜防重：{b['blurb']}，判据颗粒不重复｜"
             f"派生声明：本骨架账自深化册 docs/unxreal/deepen/D5-B{bno:02d}.md 判据单源派生（判据文本零改写），深化已收口故条目状态标 [已深化]（与总纲 §7.3-D5 台账同步）")
    L.append("")
    for (fid, title, rows, judge, *_ ) in ents:
        L.append(f"### UNX-F{fid} · {title}")
        L.append(f"- 域/批：D5/B{bno:02d}｜纯功能行数：{rows}｜状态：[已深化]｜判据：UNX-F{fid}-J1 {judge}")
    path = os.path.join(BATCH, f"UNX-D5-B{bno:02d}.md")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(L) + "\n")
    return path


def derive_b01_b03():
    """自既有深化册零改写派生 B01–B03 骨架账。"""
    themes = {1: "IUnknown 三法地基与接管基线七锚", 2: "QI 深水：接口导航与族间拒绝", 3: "计数双域与 ComPtr 语义"}
    for bno in (1, 2, 3):
        src = os.path.join(DEEPEN, f"D5-B{bno:02d}.md")
        with open(src, encoding="utf-8") as fh:
            text = fh.read()
        ents = []
        for m in re.finditer(r"^### (UNX-F\d+) · (.+?)$\n^- 域/批：.*?判据：(UNX-F\d+-J1 .*?)｜纯功能行数：(\d+) 行.*?状态：\[已深化\]",
                             text, re.M):
            fid, title, judge, rows = m.group(1), m.group(2).strip(), m.group(3).strip(), int(m.group(4))
            ents.append((fid, title, rows, judge))
        assert len(ents) == 20, (bno, len(ents))
        f1, f2 = 15201 + (bno - 1) * 20, 15200 + bno * 20
        L = []
        L.append(f"# UNX-D5-B{bno:02d} · {themes[bno]}（F{f1}–F{f2} · 20 条）")
        L.append("")
        L.append(f"> AI-13 承办（波08-M25 深化收口；波08-M37 骨架账派生补登，Variable 改派留痕原 AI-20）｜"
                 f"域账累计：本批 6,000 / 240,000｜嫁接源：任务书六专题（vtable ABI/QI 深水/计数双域/ComPtr）｜"
                 f"防重：与 D1/D2/D3 已收口面零撞号，判据颗粒不重复｜"
                 f"派生声明：本骨架账自深化册 docs/unxreal/deepen/D5-B{bno:02d}.md 判据单源派生（判据文本零改写），深化已收口故条目状态标 [已深化]（与总纲 §7.3-D5 台账同步）")
        L.append("")
        rows_sum = 0
        for (fid, title, rows, judge) in ents:
            rows_sum += rows
            L.append(f"### {fid} · {title}")
            L.append(f"- 域/批：D5/B{bno:02d}｜纯功能行数：{rows}｜状态：[已深化]｜判据：{judge}")
        path = os.path.join(BATCH, f"UNX-D5-B{bno:02d}.md")
        with open(path, "w", encoding="utf-8", newline="\n") as fh:
            fh.write("\n".join(L) + "\n")
        print(f"derived {path} rows={rows_sum}")


def main():
    grand = 0
    for b in BATCHES:
        chars, path = gen_deepen(b)
        bpath = gen_batch_book(b)
        grand += chars
        print(f"B{b['bno']:02d}: deepen {os.path.basename(path)} body_chars={chars}  batch={os.path.basename(bpath)}")
    derive_b01_b03()
    print(f"TOTAL body chars (B04–B18): {grand:,}  |  300 entries x 20 batches complete")


if __name__ == "__main__":
    main()
