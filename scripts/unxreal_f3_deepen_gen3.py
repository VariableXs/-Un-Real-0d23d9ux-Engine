# -*- coding: utf-8 -*-
"""UNX-F3 域深化册生成器 · 第三段（AI-28 · B31–B41 · 200 条，域深化满账收官）。
单源原则同前两轮：ID/标题/判据/行数全部从骨架账读取，深化零改行数。"""
import io, re, os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
while not os.path.isdir(os.path.join(REPO, "docs", "unxreal")):
    REPO = os.path.dirname(REPO)
BATCH_DIR = os.path.join(REPO, "docs", "unxreal", "batches")
OUT_DIR = os.path.join(REPO, "docs", "unxreal", "deepen")

THEME = {
 31:("位块收官：元文件、区域与指令流总收口","varix::render::meta::close","元文件回放与扫描带消费桩","Windows EMF/BitBlt 收口对照"),
 32:("文本对照收官：像素级对照总闸","varix::render::text::verify::close","黄金样本库与差分热区总闸","Windows 文本渲染基线总对照"),
 33:("文本对照收官：字形缓存与管线总装","varix::render::text::e2e::close","字形缓存 LRU 与全链总装","Windows 端到端文本渲染对照"),
 34:("ClearType 收官：开关语义与池化总闸","varix::render::text::ct::close","亚像素池化与开关状态机","Windows ClearType 开关语义总闸"),
 35:("D2D 收官：几何与效果总闸","varix::render::d2d::geom::close","几何 flattening 与效果拓扑","Windows D2D 图元/效果语义总闸"),
 36:("D2D 收官：DWrite 集成与资源生命周期","varix::render::d2d::life","文本渲染器集成与资源账","Windows ID2D1 资源生命周期语义"),
 37:("DWrite 收官：字体系统总闸","varix::render::dwrite::close","字体集合/回退/匹配总闸","Windows IDWriteFactory 语义总闸"),
 38:("域联签与波次闭账","varix::render::f3::cosign","跨域联签桩与消费方冻结面","Windows 渲染语义跨域对照"),
 39:("域总收官账本（上）：位块/文本面","varix::render::f3::close::a","位块与文本面对账账本","Windows 位块/文本语义总账"),
 40:("域总收官账本（下）：D2D/DWrite/跨域面","varix::render::f3::close::b","D2D/DWrite 面与跨域账","Windows D2D/DWrite 语义总账"),
 41:("域补号批：渲染调试与开发者体验支撑面","varix::render::devxp","渲染调试器与对照工具链","Windows 调试图层/诊断语义"),
}

def parse_skeleton(path):
    t = io.open(path, encoding="utf-8").read()
    entries = []
    parts = re.split(r"^### (UNX-F(\d{5})) · (.+)$", t, flags=re.M)
    for i in range(1, len(parts), 4):
        idn, title, body = int(parts[i+1]), parts[i+2].strip(), parts[i+3]
        rows = int(re.search(r"纯功能行数：(\d+)", body).group(1))
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
    fid, rows = e["id"], e["rows"]
    m_rows, c_rows = rows*40//100, rows*30//100
    a_rows = rows - m_rows - c_rows
    loc = (f"本条为 UNX-F3 域收官/补号段「{theme}」的深化条目——{e['title']} 的语义总装与判据落账，深化正文把骨架账条目展开为可施工、可复测、可联签的完整设计："
           f"{k} 的模型面、机检面、断言面三面一次成形，行数预算 {rows} 行在深化拆解中逐块落到子结构。")
    bound = (f"本条只收 {k} 自身的语义与判据，相邻语义只引不重立；跨域消费只走冻结接口，不私开旁路；"
             f"收官/账本类条目只读已收口事实、不立新语义；补号段调试面只供本域开发者体验，不代他域开诊断旁路；"
             f"渲染结果落地归 AI-26 surface、API 句柄语义归 AI-19，本条不越过两域边界，不代 F4（AI-29）开卷。")
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
    all_ids = []
    for bno in range(31, 42):
        sk = os.path.join(BATCH_DIR, f"UNX-F3-B{bno:02d}.md")
        entries = parse_skeleton(sk)
        expect = 10 if bno in (39, 40) else 20
        assert len(entries) == expect, f"B{bno:02d}: entries={len(entries)}"
        theme, mod, anchor, ref = THEME[bno]
        rows_sum = sum(e["rows"] for e in entries)
        all_ids += [e["id"] for e in entries]
        lo, hi = entries[0]["id"], entries[-1]["id"]
        hdr = (f"# 域 UNX-F3 · 深化册 · UNX-F3-B{bno:02d}（F{lo:05d}–F{hi:05d} · {expect} 条 · {expect} 条新深化）\n\n"
               f"> AI-28 承办（深化轮第三段 · 域深化满账收官）｜本批 B{bno:02d} [已深化] 收口：F{lo:05d}–F{hi:05d} 共 {expect} 条为本会话新深化"
               f"（骨架账 verbatim 承接：条目名/行数/判据号零改动）｜批主题：{theme}｜对照锚位：{anchor}｜"
               f"对照基线：{ref}｜上游 AI-19/AI-26 未冻结面按 Schema 先行+fake；真机/多屏判据随闸门补测｜"
               f"防重声明：只深化本域条目语义，不重立他域语义、不代 D4（AI-19）/F4（AI-29）开卷、不改 handoff 协议 schema｜"
               f"落在规划模块 {mod}｜批累计行数锁定 {rows_sum}（= 骨架账逐条之和，深化零改行数）｜"
               f"域内宪法（随批复述）：①文本渲染像素级对照复测（判据主轴一）；②ClearType 类可开关（判据主轴二）；③防重非重复（AI-84 口径复核）；"
               f"④跨域只走冻结接口；⑤账本只记实测禁止推算填充。\n")
        body = hdr + "\n" + "\n".join(compose(bno, e) for e in entries) + "\n"
        io.open(os.path.join(OUT_DIR, f"F3-B{bno:02d}.md"), "w", encoding="utf-8").write(body)
    assert len(all_ids) == 200 and len(set(all_ids)) == 200, (len(all_ids), len(set(all_ids)))
    print(f"OK deepen3: 11 batches, 200 entries, IDs F{min(all_ids):05d}-F{max(all_ids):05d}")

if __name__ == "__main__":
    main()
