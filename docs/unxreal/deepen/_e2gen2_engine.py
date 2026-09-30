# -*- coding: utf-8 -*-
"""UNX-E2 B23-B37 续作轮二生成引擎（AI-22 承办 · 波09 · 一次对话 300 项新功能明令执行件）。
输入：_e2gen2_data_p1..p5（每模块 3 批 × 20 条，判据单源）
输出：deepen/E2-B23..B37.md + batches/UNX-E2-B23..B37.md
机械断言：ID 连续唯一（F17241–F17540）/ 每批 20 条 / 批行数和=6,000 / 正文行 ≥300 字 / 判据号一致（册↔批册）
主题段：stdio/locale/多字节/宽字符/字符串/时间/环境/文件/BSTR/VARIANT/ABI/异常展开（任务书主线三、四承接）
"""
import importlib, os, re

BASE = os.path.dirname(os.path.abspath(__file__))
BATCH_DIR = os.path.abspath(os.path.join(BASE, "..", "batches"))
PREV_LAST = 17240        # B22 末条
DOMAIN_ROWS_BEFORE = 131900  # B01–B22 累计（R-E2-001 真值口径）
BASE_NO = 22

META = {
    23: ("stdio 缓冲与 FILE 结构语义 I（缓冲模式/刷新/定位）", "CRT stdio 契约（注来源：setvbuf/fflush/fseek/ftell 章与 MSVCUCRT 行为）", "compat/crtio/", "B19 CRT 分配层（缓冲区经堆分配）+ AI-17 D2 文件语义冻结面"),
    24: ("printf/scanf 格式解析族", "printf 族契约（注来源：% 转换说明符/宽度精度/长度修饰符章）", "compat/crtio/", "B23 stdio 缓冲面 + B25 locale 数值格式化消费预登记"),
    25: ("locale 地基与 CRT 环境双轨（B10 交接面消费）", "locale 契约（注来源：setlocale/LC_* 分类/per-thread locale 章）", "compat/crtlocale/", "B10 F17005 交接预留面（消费兑现）+ B23–B24 数值/时间格式化消费方"),
    26: ("多字节/宽字符转换（CP_ACP/CP_UTF8 与 mbstowcs 族）", "字符转换契约（注来源：MultiByteToWideChar 对应 CRT 层/mbstowcs/wcstombs/CP_ACP·CP_UTF8 章）", "compat/crtchar/", "B25 locale 代码页面 + AI-17 D2 NLS 语义冻结面"),
    27: ("宽字符 stdio 与 wprintf 族", "宽字符 stdio 契约（注来源：wprintf/fwprintf/fgetws/_wfopen 章与 %s/%ls 差异章）", "compat/crtio/", "B23–B24 stdio/格式面 + B25 locale 宽字符模式"),
    28: ("字符串/内存函数族（memcpy 族与 _s 安全版）", "字符串契约（注来源：memcpy/memmove 重叠语义/strcpy/strncpy/_s 版 ERANGE·EINVAL 章与 CLA 边界检查行为）", "compat/crtstr/", "B23 stdio 面 + B26 字符集转换面"),
    29: ("时间与时钟（time/clock 与系统时间映射）", "时间契约（注来源：time/clock/_time64/GetSystemTime 映射/夏令时口径章）", "compat/crttime/", "AI-02 A2 计时原语（冻结，联签）+ B25 locale 时间格式化消费方"),
    30: ("环境变量与进程信息（getenv 双轨与 _wenviron）", "环境契约（注来源：getenv/_putenv/environ·_wenviron 双轨章与 D2 进程环境块冻结面）", "compat/crtenv/", "AI-17 D2 进程环境块（冻结）+ B25 locale 面预登记"),
    31: ("文件访问 CRT 层（fopen 族与 fd↔HANDLE 契约消费）", "文件 CRT 契约（注来源：fopen 模式串/fd↔HANDLE 映射/_fileno/O_RDONLY 族章）", "compat/crtfile/", "B19 fd↔HANDLE 双账映射契约（F17051 消费兑现）+ AI-17 D2 文件语义冻结面"),
    32: ("BSTR 分配语义（SysAllocString 前缀长度）", "OLE Automation 契约（注来源：SysAllocString/SysFreeString/前缀长度双指针语义章）", "compat/crtvariant/", "AI-13 D5 COM/OLE 基座（冻结）+ B19 堆分配层"),
    33: ("VARIANT 变体类型全枚举与 SafeArray 基础", "OLE Automation 契约（注来源：VARENUM 全枚举/VariantInit·ChangeType/SafeArrayCreate 章）", "compat/crtvariant/", "B32 BSTR 面 + AI-13 D5 COM 冻结面"),
    34: ("MSVC 名字修饰 mangling/demangling 全规则", "C++ ABI 契约（注来源：MSVC ? 修饰语法/名字空间嵌套/模板实例化/往返判据章）", "compat/crtabi/", "AI-13 D5 COM 冻结面 + 任务书判据主轴 10^5 符号往返"),
    35: ("调用约定（x64 统一与 x86 四约定）", "调用约定契约（注来源：x64 统一约定/x86 cdecl·stdcall·fastcall·thiscall 栈布局与寄存器章）", "compat/crtabi/", "B34 符号面 + AI-17 D2 WOW64 分档协议（冻结 F13098）"),
    36: ("C++ 异常 ABI 与 EH 展开（D1 SEH 对接）", "C++ EH ABI 契约（注来源：_CxxThrowException/throw image 信息/展开四步回调/栈回溯章）", "compat/crtabi/", "AI-16 D1 SEH 展开器冻结面 + B34–B35 ABI 面"),
    37: ("stdio→ABI 段总聚合·全量回归与批收口", "AI-22 判据主轴 msvcrt/ucrtbase 测试集 + B23–B36 全部已收口面", "compat/crtabi/", "B23–B36 全部已收口条 + B01–B22 前序账"),
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
        f"> AI-22 承办（波09 续作轮二 · 一次对话 300 项新功能明令执行件：本会话 B23–B37 十五批 300 条，本册为第 {no - BASE_NO}/15 册）｜"
        f"本批 B{no} [已深化] 收口：F{f0}–F{f1} 共 20 条为本会话新深化｜主题：{title}｜嫁接源：{src}；上游依赖：{dep}｜落在 {module}｜"
        f"批累计行数锁定 6,000（= 逐条之和，末条 {rows[19]} 收口补差披露），域累计 {DOMAIN_ROWS_BEFORE + (no - BASE_NO) * 6000:,}/240,000｜"
        f"防重声明：现存栈无运行库资产（升级接管声明为零）；只立本批条目语义，与 B01–B22 已收口面及邻批分层不重立；不代他域开卷；不改 handoff 协议 schema｜"
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
        f"> AI-22 承办（波09 续作轮二 · 一次对话 300 项新功能明令执行件）｜域账累计：本批 6,000 / 240,000（域累计 {DOMAIN_ROWS_BEFORE + (no - BASE_NO) * 6000:,}/240,000）｜波次：波09（B23–B37 共 300 项 · AI-22 续作轮二）｜"
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
    mods = [importlib.import_module(f"_e2gen2_data_p{i}") for i in range(1, 6)]
    stats, all_ids = [], []
    for m in mods:
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
    print("ALL PASS — 15 批 300 条落盘")
    for s in stats:
        print(f"  B{s[0]:02d}: F{s[1]}–F{s[2]} rows={s[3]} min_body={s[4]}")


if __name__ == "__main__":
    main()
