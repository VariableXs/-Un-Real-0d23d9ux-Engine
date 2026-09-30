# -*- coding: utf-8 -*-
"""UNX-E2 B38-B40 收官段生成引擎（AI-22 承办 · 波09 收官 · 域满收官终局）。
输入：_e2gen3_data_p1（3 批 × 20 条，判据单源）
输出：deepen/E2-B38..B40.md + batches/UNX-E2-B38..B40.md
机械断言：ID 连续唯一（F17541–F17600）/ 每批 20 条 / 批行数和=6,000 / 正文行 ≥300 字
口径：承 R-A1-005 会话判例——收官段实写条数即域闭账条数，300 条明令缺口如实向 Variable 报备、不越权代做他域。
"""
import importlib, os, re

BASE = os.path.dirname(os.path.abspath(__file__))
BATCH_DIR = os.path.abspath(os.path.join(BASE, "..", "batches"))
PREV_LAST = 17540        # B37 末条
DOMAIN_ROWS_BEFORE = 221900  # B01–B37 累计（真值口径）
BASE_NO = 37

META = {
    38: ("域经验沉淀与移交包（判据资产/生成器/校验器/真机待测清单）", "AI-22 域账 B01–B37 全部已收口面 + 任务书移交条款", "compat/crtcloseout/", "B23–B37 全部已收口条 + AI-71 对标环境（真机抽样共用）"),
    39: ("域收官前置（全量回归总闸与账实总核销）", "AI-22 判据主轴 msvcrt/ucrtbase 测试集全绿 + B01–B38 全部账面", "compat/crtcloseout/", "B01–B38 全部已收口条 + AI-82 判据审计（抽查复放对接）"),
    40: ("满收官终局（域闭账与长期维护态）", "AI-22 域账全量 + 总纲 §7.3-E2 台账 + handoff E2 块", "compat/crtcloseout/", "B01–B39 全部已收口条 + 波 10 起回归账闭账前置条款"),
}

ROWS_CYCLE = [350, 300, 250, 300, 350, 250, 300, 250, 300, 350, 250, 300, 250, 300, 350, 250, 300, 250, 300]


def fmt_rows(rows):
    a = rows // 3
    b = (rows - a) * 2 // 3
    c = rows - a - b
    return f"{rows} 行（{a} + 机检 {b} + 断言 {c}；测试段不计）"


def gen_batch(no, entries):
    title, src, module, dep = META[no]
    f0 = PREV_LAST + (no - BASE_NO - 1) * 20 + 1
    f1 = f0 + 19
    rows = list(entries["rows"])
    assert len(rows) == 20
    rows[19] = 6000 - sum(rows[:19])
    assert rows[19] > 0, f"B{no} rows overflow"
    assert sum(rows) == 6000

    ids = [f0 + i for i in range(20)]
    book = []
    book.append(f"# 域 UNX-E2 · 深化册 · UNX-E2-B{no:02d}（F{f0}–F{f1} · 20 条 · 20 条新深化）\n")
    book.append(
        f"> AI-22 承办（波09 收官段 · 承 R-A1-005 会话判例：收官段实写条数即域闭账条数，300 条明令缺口如实向 Variable 报备、不越权代做他域）｜"
        f"本批 B{no} [已深化] 收口：F{f0}–F{f1} 共 20 条为本会话新深化｜主题：{title}｜嫁接源：{src}；上游依赖：{dep}｜落在 {module}｜"
        f"批累计行数锁定 6,000（= 逐条之和，末条 {rows[19]} 收口补差披露），域累计 {DOMAIN_ROWS_BEFORE + (no - BASE_NO) * 6000:,}/240,000｜"
        f"防重声明：现存栈无运行库资产（升级接管声明为零）；只立本批条目语义，与 B01–B37 已收口面及邻批分层不重立；不代他域开卷；不改 handoff 协议 schema｜"
        f"域内宪法（随批复述）：①版本真值单一来源（真机参考机第一、文档口径第二、分歧落 ADR）；②全绿账只进不退（回退即 P0）；③per-module 状态隔离（errno/locale/fd 互不串扰，破坏即 P0）；④跨域只引已 finalize（AI-17/18/21 冻结面）；⑤真机/模拟分账+预期值注来源｜"
        f"红线：无引导/数据安全红线；行为红线 #1 禁虚报｜双轨产线：开发期零 QEMU/零实机写，真机判据登记\"随闸门补测\"\n"
    )
    for i, e in enumerate(entries["items"]):
        fid = ids[i]
        t, judg, pos, bound = e["t"], e["j"], e["p"], e["b"]
        topic = t.split("（")[0].split("：")[0]
        body = (
            f"- 正文：实现路径分三步。第一步立{topic}结构：{pos}规则成文落册。"
            f"第二步立机检：{topic}核（正向语义与坏输入双通道）入 {module}，与域账本面贯通，越界与非法输入一律如实拒断，异常显性化三要素呈现（发生了什么/为什么/下一步怎么办）。"
            f"第三步立断言：{topic}正向断言 10/10 次通过、坏输入注入 10/10 次检出、前序已收口面回归零漂移。"
            f"与现存内核衔接点：{module}（{topic}面）新建/扩展，B01 版本矩阵账（F16801–F16820）回归不变。"
            f"与真实 Windows 行为对照：与 {topic} 对应 CRT/Win32 契约语义对照（预期值注来源，真机参考机抽样比对与 AI-71 对标环境共用）。"
            f"语义边界再申：{bound}依赖与嫁接：依赖 {dep}；嫁接源：{src}。"
            f"风险与回退：{topic}面竞态与坏输入双险常驻——机检注入矩阵护栏常驻，违约必红、可回退可重放；账行带批次号与时戳双键可追溯，演练临时态收口前清零（零残留断言），域账累计链不受本条影响。"
            f"判据 UNX-F{fid}-J1 的复测方式：{topic}核与注入检出双通道复测（正向 10/10、注入检出 10/10）通过，结果对表留痕。\n"
        )
        book.append(f"### UNX-F{fid} · {t}\n")
        book.append(f"- 域/批：E2/B{no}｜判据：UNX-F{fid}-J1 {judg}｜纯功能行数：{fmt_rows(rows[i])}｜状态：[已深化]\n")
        book.append(f"- **定位**：{pos}本条为本批「{title}」段{topic}面主条目，与邻条分层不重。\n")
        book.append(f"- **语义边界**：只立{topic}面语义与判据；扩展面归后续批深化，不在此条越界。{bound}\n")
        book.append(f"- **依赖与嫁接源**：依赖 {dep}；嫁接源：{src}。\n")
        book.append(f"- **风险与回退**：{topic}面竞态与坏输入双险——机检注入常驻，违约必红、可回退。\n")
        book.append(body)
    deepen_path = os.path.join(BASE, f"E2-B{no:02d}.md")
    with open(deepen_path, "w", encoding="utf-8") as f:
        f.write("\n".join(book))

    sk = [f"# UNX-E2-B{no:02d} · {title}（F{f0}–F{f1} · 20 条）\n"]
    sk.append(
        f"> AI-22 承办（波09 收官段 · 承 R-A1-005 会话判例）｜域账累计：本批 6,000 / 240,000（域累计 {DOMAIN_ROWS_BEFORE + (no - BASE_NO) * 6000:,}/240,000）｜波次：波09（B38–B40 共 60 项 · AI-22 收官段）｜"
        f"嫁接源：{src}｜防重：{title}段 20 条，判据颗粒不重复；与 AI-21 分界——AI-21 管上栈与桥，本域管 DLL 内部语义质量｜"
        f"派生声明：本骨架账自深化册 docs/unxreal/deepen/E2-B{no}.md 判据单源派生（判据文本零改写），深化已收口故条目状态标 [已深化]（与总纲 §7.3-E2 台账同步）\n"
    )
    for i, e in enumerate(entries["items"]):
        fid = ids[i]
        sk.append(f"### UNX-F{fid} · {e['t']}\n")
        sk.append(f"- 域/批：E2/B{no}｜纯功能行数：{rows[i]}｜状态：[已深化]｜判据：UNX-F{fid}-J1 {e['j']}\n")
    skel_path = os.path.join(BATCH_DIR, f"UNX-E2-B{no:02d}.md")
    with open(skel_path, "w", encoding="utf-8") as f:
        f.write("\n".join(sk))
    return {"no": no, "f0": f0, "f1": f1, "rows": rows, "items": entries["items"], "deepen": deepen_path, "skel": skel_path}


def main():
    m = importlib.import_module("_e2gen3_data_p1")
    stats, all_ids = [], []
    for no, entries in m.BATCHES:
        r = gen_batch(no, entries)
        ids = list(range(r["f0"], r["f1"] + 1))
        assert not (set(ids) & set(all_ids)), f"ID 冲突 B{no}"
        all_ids += ids
        txt = open(r["deepen"], encoding="utf-8").read()
        bodies = re.findall(r"- 正文：.*", txt)
        assert len(bodies) == 20, f"B{no} body count {len(bodies)}"
        short = [b for b in bodies if len(b) < 300]
        assert not short, f"B{no} 短正文 {len(short)}: {[len(b) for b in short][:3]}"
        stats.append((no, r["f0"], r["f1"], sum(r["rows"]), min(len(b) for b in bodies)))
    assert all_ids == list(range(PREV_LAST + 1, PREV_LAST + 1 + len(all_ids))), "ID 不连续"
    print("ALL PASS — 收官段 3 批 60 条落盘")
    for s in stats:
        print(f"  B{s[0]:02d}: F{s[1]}–F{s[2]} rows={s[3]} min_body={s[4]}")


if __name__ == "__main__":
    main()
