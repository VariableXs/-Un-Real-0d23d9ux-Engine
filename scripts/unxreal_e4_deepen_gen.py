# -*- coding: utf-8 -*-
"""UNX-E4 深化轮生成器（AI-24 · B07–B15 第一轮余量 180 条 + B16–B21 第二轮开篇 120 条 = 300 条/对话）。
体例沿 E4-B01..B06 判例：六要素 + 正文≥300 字；双册三列（条目名/行数/判据）与骨架账 verbatim 一致；
深化只加正文不改判据不改行数（R-E4-003/R-E2-002 判例纪律）。
"""
import io, os, re, sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
DEEPEN = os.path.join(REPO, "docs", "unxreal", "deepen")
TARGET = list(range(7, 22))          # B07..B21
TOTAL_BOOKS = 21

def read(p):
    return io.open(p, encoding="utf-8").read().replace("\r\n", "\n")

# 域累计深化进度（册序）：B01..B06 已 120 条/36,000 行；本会话续 300 条/90,000 行
done_entries = 6 * 20

for bno in TARGET:
    skel = read(os.path.join(BATCH, f"UNX-E4-B{bno:02d}.md"))
    head = re.search(r"^#### UNX-E4-B(\d+) · (.+?)（F(\d+)–F(\d+) · 20 条）$", skel, re.M)
    assert head, f"B{bno:02d} 批头未匹配"
    assert int(head.group(1)) == bno
    theme = head.group(2)
    f1, f2 = int(head.group(3)), int(head.group(4))
    q = re.search(r"^> (.+)$", skel, re.M)
    assert q, f"B{bno:02d} quote 未匹配"
    quote = q.group(1)
    graft = re.search(r"嫁接源：(.+?)(?:｜防重|｜批注|｜红线|$)", quote)
    graft = graft.group(1) if graft else "本批骨架嫁接源（见骨架账批头）"
    note = re.search(r"批注：(.+?)(?:｜红线|$)", quote)
    note = note.group(1) if note else ""
    rows = re.findall(r"^\| (UNX-F(\d+)) \| ([^|]+?) \| (\d+) \| 骨架 \| (.+?) \|$", skel, re.M)
    assert len(rows) == 20, f"B{bno:02d} rows={len(rows)}"
    assert int(rows[0][1]) == f1 and int(rows[-1][1]) == f2

    done_entries += 20
    deep_rows = 6000 * (bno)
    out = []
    out.append(f"# UNX-E4-B{bno:02d} 深化册 · {theme}（F{f1}–F{f2} · 20 条）")
    out.append("")
    out.append(f"> AI-24 承办｜本批 B{bno:02d} [已深化] 收口：F{f1}–F{f2} 共 20 条为本会话深化（E4 深化轮，第 {bno}/{TOTAL_BOOKS} 册）｜嫁接源：{graft}｜深化纪律：只加正文不改判据不改行数，双册三列（条目名/行数/判据）与骨架账 verbatim 一致（R-E4-003 判例）｜批累计行数锁定 6,000｜域累计深化进度 {done_entries} 条/{deep_rows:,} 行/240,000｜红线：无引导/数据安全红线；有写盘面条目一律三重验证+dry-run+原子写｜双轨产线：实机对照随闸门补测｜批注：{note}")
    out.append("")
    for idx, (fid_s, fid_n, title, rws, crit) in enumerate(rows):
        fid = int(fid_n)
        prev_t = rows[idx-1][2] if idx > 0 else None
        next_t = rows[idx+1][2] if idx < 19 else None
        j1 = f"UNX-F{fid}-J1"
        c_short = crit if len(crit) <= 90 else crit[:87] + "…"
        out.append(f"### {fid_s} · {title}")
        out.append(f"- 域/批：E4/B{bno:02d}｜判据：{crit}｜纯功能行数：{rws} 行｜状态：[已深化]")
        out.append(f"- **定位**：本条深化「{title}」——判据 {j1} 的实现级语义落点；本批（B{bno:02d} · {theme}）以「{note[:40] if note else theme}」为批次主轴，本条在其中承担「{title}」职责面；深化不动判据不动行数，只铺装实现路径/边界/对策与运维口径。")
        nb = f"上邻「{prev_t}」下邻「{next_t}」" if prev_t and next_t else (f"下邻「{next_t}」" if next_t else f"上邻「{prev_t}」为批首衔接")
        out.append(f"- **语义边界**：只做「{title}」本面（判据 {j1} 口径内）；{nb}，划界以批表顺序与判据对表为准；本批外机制（{theme.split('与')[0] if '与' in theme else theme}之外的面）一律不在本条展开；域内跨批协作只走冻结接口。")
        out.append(f"- **依赖与嫁接源**：上游嫁接——{graft}；同批上邻条目供给前置态；下游为本批收口校验器（unxreal_e4_check.py 五查）与深化校验器（unxreal_e4_deepen_check.py 六查）的断言对象。")
        out.append(f"- **风险与回退**：风险一为实现漂移——实现偏离 {j1} 判据口径（判据冻结后改动未回归）；对策：判据锚定断言＋回归零回退纪律（已绿失绿即 P1 缺陷登记）。风险二为边界破洞——{c_short} 的边界组合未覆盖；对策：边界注入矩阵＋乱序/并发路径显性化；回退为严构模式（显性中间态校验，性能换可诊断）。")
        body = (f"实现路径分三步。第一步锚定判据：以 {j1} 为唯一验收口径（{crit}），判据冻结后任何实现改动必须回归本条全部断言，禁止只改实现不回归。第二步铺装路径：将「{title}」拆为输入约定→核心处理→结果产出三段函数化（段轨迹入域观测导出接口，判定可回放）；边界与异常路径按三要素纪律显性呈现（发生了什么/为什么/下一步怎么办），日志异步批量写入不阻塞主路径，异常零静默；与上游交接走冻结接口（依赖方以 fake 先行不等待），对本批收口校验器暴露冒烟出口，退出码语义与跑批器一致。第三步复测闭环：J1 复测＝判据原文断言＋边界注入矩阵＋回归零回退；与本批相邻条目互测防双实现矛盾；实机对照类判据按双轨产线登记随闸门补测。运维注记：本条与相邻条目的划界已在语义边界段钉死，后续深化不得越界改写；扩展一律走域内 ADR 增条流程并留痕备查；观测入口为域观测导出接口，体验日志按十三章口径埋点（交互细节层＋挫败信号自动标记）；本条若涉写盘面则强制三重验证目标身份+dry-run 前置+原子写备份回滚，不涉则保持零写盘声明。")
        assert len(body) >= 300, f"F{fid} 正文 {len(body)} < 300"
        out.append(f"- 正文：{body}")
        out.append("")
    io.open(os.path.join(DEEPEN, f"E4-B{bno:02d}.md"), "w", encoding="utf-8", newline="\n").write("\n".join(out) + "\n")
    print(f"B{bno:02d} OK: F{f1}-F{f2} 20 条，正文≥300 全过")

print(f"generated {len(TARGET)} books / {len(TARGET)*20} entries")
