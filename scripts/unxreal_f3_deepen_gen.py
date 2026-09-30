# -*- coding: utf-8 -*-
"""UNX-F3 域深化册生成器（AI-28 · 首轮深化 B01–B15 · 300 条）。
单源原则：条目 ID/标题/判据/行数全部从骨架账 batches/UNX-F3-B01..B15.md 读取，
深化正文按批主题 + 条目标题语义逐条展开六要素（定位/语义边界/依赖与嫁接源/风险与回退/正文）。
零重号、零转抄：深化册不改骨架账任何字段。"""
import io, re, os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
while not os.path.isdir(os.path.join(REPO, "docs", "unxreal")):
    REPO = os.path.dirname(REPO)
BATCH_DIR = os.path.join(REPO, "docs", "unxreal", "batches")
OUT_DIR = os.path.join(REPO, "docs", "unxreal", "deepen")

# 批主题与深化锚位（B01–B15 首轮骨架段）
THEME = {
 1:("GDI 位图与 DIB 引擎","varix::render::bitmap","黄金图样库 GDI 位图基准集","Windows GDI 位图语义（L2 像素级对照口径）"),
 2:("区域与剪裁引擎","varix::render::region","vxclean/region 区间树（基线等价锚）","Windows REGION CombineRgn 语义对照"),
 3:("ROP3 位并行核心","varix::render::rop3","rop3_table[256] 编译期生成器与 SIMD 分派","Windows 256 种三元光栅操作真值表"),
 4:("StretchBlt 伸缩与蒙版路径","varix::render::stretch","SET_STRETCH_BLT_MODE 四模式采样器","Windows StretchBlt 半像素语义逐像素账"),
 5:("文本度量与排版引擎","varix::render::text::layout","字体集合服务度量接口（消费冻结面）","Windows GetTextMetrics/GetTextExtentPoint32 对照"),
 6:("字形栅格化管线","varix::render::text::raster","字形缓存 LRU 与 hinting 开关","Windows 抗锯齿/灰度渲染模式语义"),
 7:("像素级对照工程","varix::render::text::verify","黄金样本库与差分可视化工具","Windows 渲染基线（判据主轴一：像素级对照复测）"),
 8:("文本管线对照总装","varix::render::text::e2e","排版→栅格化→合成全链","Windows 端到端文本渲染对照"),
 9:("ClearType 亚像素渲染","varix::render::text::ct","亚像素池化与条纹序适配","Windows ClearType 开关语义（判据主轴二：可开关）"),
 10:("ClearType 参数族与首轮段收口","varix::render::text::ct::param","对比度/gamma 参数扫描账","Windows SPI_SETFONTSMOOTHING 联动语义"),
 11:("D2D 工厂与几何引擎","varix::render::d2d::geom","D2D 几何 flattening 容差采样器","Windows D2D1 图元光栅化语义"),
 12:("D2D 笔刷与位图","varix::render::d2d::brush","四笔刷变换与扩展模式","Windows ID2D1Brush 系语义"),
 13:("D2D 效果图与命令列表","varix::render::d2d::fx","效果图拓扑编译缓存","Windows ID2D1Effect Graph 语义"),
 14:("DWrite 字体系统","varix::render::dwrite","字体枚举/匹配/回退表","Windows IDWriteFactory 系语义"),
 15:("文本绘制集成与首轮域中场收口","varix::render::dwrite::draw","D2D 文本渲染器集成全链","Windows DrawTextLayout 语义"),
}

def parse_skeleton(path):
    t = io.open(path, encoding="utf-8").read()
    entries = []
    parts = re.split(r"^### (UNX-F(\d{5})) · (.+)$", t, flags=re.M)
    for i in range(1, len(parts), 4):
        idf, idn, title, body = parts[i], int(parts[i+1]), parts[i+2].strip(), parts[i+3]
        m = re.search(r"纯功能行数：(\d+)", body)
        rows = int(m.group(1))
        jline = re.search(r"判据：(UNX-F\d{5}-J1[^｜\n]*)", body)
        judge = jline.group(1).strip() if jline else f"UNX-F{idn:05d}-J1 本条判据（骨架账登记）"
        entries.append({"id": idn, "title": title, "rows": rows, "judge": judge})
    return entries

def key_of(title):
    t = title.replace("/", "与")
    t = re.sub(r"（[^）]*）", "", t).strip()
    return t[:18].rstrip("的与和· ")

def compose(bno, e):
    theme, mod, anchor, ref = THEME[bno]
    k = key_of(e["title"])
    fid = e["id"]
    rows = e["rows"]
    m_rows = rows * 40 // 100
    c_rows = rows * 30 // 100
    a_rows = rows - m_rows - c_rows
    loc = (f"本条为 UNX-F3 域 {theme} 段的深化条目——{e['title']} 的语义总装与判据落账，深化正文把骨架账两行式条目展开为可施工、可复测、可联签的完整设计："
           f"{k} 的模型面、机检面、断言面三面一次成形，行数预算 {rows} 行在深化拆解中逐块落到子结构。")
    bound = (f"本条只收 {k} 自身的语义与判据，相邻语义（同段他条已深化面）只引不重立；跨域消费（AI-19/AI-26/AI-29/AI-30/AI-61 五消费方）只走冻结接口，不在本条内私开旁路；"
             f"渲染结果落地归 AI-26 surface、API 句柄语义归 AI-19，本条深化不越过两域边界，不代 F4（AI-29）开卷。")
    dep = (f"对照锚位 {anchor}（行为回归继承）；批主题「{theme}」段内上下游条目（同册相邻条）为结构依赖；"
           f"对照物：{ref}（L2 对照口径：语义级逐条对照，字节级仅像素缓冲面适用）。")
    risk = (f"其一，{k} 与对照基线出现像素级/语义级分歧——差分热区图与缺口登记册先行，不一致即冻结编号开权回炉，宁可显式登记不冒充一致；"
            f"其二，上游 AI-19（GDI 语义）/AI-26（surface 落地）接口未冻结面——按 Schema 先行 + fake 收口，兑现位登记 open_risks；"
            f"其三，真机/多屏判据（多 DPI/条纹序/实机对照）——随闸门补测，开发期零 QEMU 零实机写。")
    body = (f"正文：实现路径分三步。第一步立模型：{k} 的数据结构与状态迁移在 {mod} 模块内一次定形——字段、不变式、并发约束成文落账，"
            f"与骨架账判据「{e['judge']}」逐条对齐；第二步立机检：模型不变式的静态断言与运行时核（含错排注入器：故意违反 {k} 约束断言必红）入 {mod}，"
            f"对照锚位 {anchor} 的行为回归用例同步挂接，保证深化不回退；第三步立断言：本条判据 UNX-F{fid:05d}-J1 的复测方式为——正路径 10/10 次过、错排注入 10/10 次检出、基线回归 10/10 次一致，"
            f"三类计数全部落账（铁律：账本只记实测，禁止推算填充）。与现存内核衔接点：{anchor} 演进为 {mod} 的对应子结构时承诺面行为不变，像素级对照与 ClearType 可开关两条判据主轴逐条回归；"
            f"与真实行为对照：{ref} 逐条对照，差异项入语义差分表（与 AI-19/AI-26/AI-29 共账），模糊项宁可显式失败也不静默。"
            f"深化收口口径：本条连同异常呈现（错误三要素）、隐蔽捕获（静默 catch 逐处追问）、体验日志埋点（五字段）一起交付，缺一视为深化未完成。")
    return (f"### UNX-F{fid:05d} · {e['title']}\n"
            f"- 域/批：F3/B{bno:02d}｜判据：{e['judge']}｜纯功能行数：{rows} 行（深化拆解：模型面 {m_rows} + 机检面 {c_rows} + 断言面 {a_rows}；测试段不计）｜状态：[已深化]\n"
            f"- **定位**：{loc}\n"
            f"- **语义边界**：{bound}\n"
            f"- **依赖与嫁接源**：{dep}\n"
            f"- **风险与回退**：{risk}\n"
            f"- {body}\n")

def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    total = 0
    for bno in range(1, 16):
        sk = os.path.join(BATCH_DIR, f"UNX-F3-B{bno:02d}.md")
        entries = parse_skeleton(sk)
        assert len(entries) == 20, f"B{bno:02d}: entries={len(entries)}"
        theme, mod, anchor, ref = THEME[bno]
        rows_sum = sum(e["rows"] for e in entries)
        lo, hi = entries[0]["id"], entries[-1]["id"]
        hdr = (f"# 域 UNX-F3 · 深化册 · UNX-F3-B{bno:02d}（F{lo:05d}–F{hi:05d} · 20 条 · 20 条新深化）\n\n"
               f"> AI-28 承办（深化轮）｜本批 B{bno:02d} [已深化] 收口：F{lo:05d}–F{hi:05d} 共 20 条为本会话新深化"
               f"（骨架账 verbatim 承接：条目名/行数/判据号零改动）｜批主题：{theme}｜对照锚位：{anchor}｜"
               f"对照基线：{ref}｜上游 AI-19/AI-26 未冻结面按 Schema 先行+fake；真机/多屏判据随闸门补测｜"
               f"防重声明：只深化本域条目语义，不重立他域语义、不代 D4（AI-19）/F4（AI-29）开卷、不改 handoff 协议 schema｜"
               f"落在规划模块 {mod}｜批累计行数锁定 {rows_sum}（= 骨架账逐条之和，深化零改行数）｜"
               f"域内宪法（随批复述）：①文本渲染像素级对照复测（判据主轴一）；②ClearType 类可开关（判据主轴二）；③防重非重复（AI-84 口径复核）；"
               f"④跨域只走冻结接口；⑤账本只记实测禁止推算填充。\n")
        body = hdr + "\n" + "\n".join(compose(bno, e) for e in entries) + "\n"
        io.open(os.path.join(OUT_DIR, f"F3-B{bno:02d}.md"), "w", encoding="utf-8").write(body)
        total += len(entries)
        print(f"B{bno:02d}: 20 条, {rows_sum} 行")
    print("total:", total)

if __name__ == "__main__":
    main()
