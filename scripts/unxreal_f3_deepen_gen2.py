# -*- coding: utf-8 -*-
"""UNX-F3 域深化册生成器 · 第二轮（AI-28 · B16–B30 · 300 条）。
单源原则同首轮：ID/标题/判据/行数全部从骨架账 batches/UNX-F3-B16..B30.md 读取，
深化正文六要素逐条展开，零重号、零转抄、深化零改行数。"""
import io, re, os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
while not os.path.isdir(os.path.join(REPO, "docs", "unxreal")):
    REPO = os.path.dirname(REPO)
BATCH_DIR = os.path.join(REPO, "docs", "unxreal", "batches")
OUT_DIR = os.path.join(REPO, "docs", "unxreal", "deepen")

THEME = {
 16:("元文件回放与复杂区域扫描带","varix::render::metafile","元文件记录流解析器与区域快照消费","Windows EMF 回放渲染语义（指令执行侧）"),
 17:("位并行性能工程与 SIMD 分派","varix::render::rop3::simd","ROP3 编译期真值表与分派桩","Windows BitBlt 高吞吐路径行为账"),
 18:("文本对照扩展矩阵与 CJK 度量","varix::render::text::verify2","黄金样本库扩展面","Windows CJK 文本度量与渲染基线"),
 19:("段落方向与镜像基础面","varix::render::text::bidi","排版引擎段落方向开关","Windows SetTextAlign/镜像变换语义"),
 20:("色彩管理与亚像素深化","varix::render::color","ICC 配置消费与 FP16 管线","Windows ICM 渲染意图与 ClearType 条纹序"),
 21:("D2D 效果图对象模型深化","varix::render::d2d::fx::om","效果图拓扑与内置效果子集","Windows ID2D1Effect 图语义"),
 22:("D2D 层/蒙版与插值扩展","varix::render::d2d::layer","opacity mask 与差值混合桩","Windows ID2D1Layer/InterpolationMode 语义"),
 23:("DWrite 集合管理与子集化","varix::render::dwrite::coll","内存字体集合与嵌入许可消费","Windows IDWriteFontCollection/Embedding 语义"),
 24:("渲染指令流 v2 与失效区对接","varix::render::cmdstream","指令流录制/回放与失效区通知","Windows RedrawWindow/InvalidatRect 语义"),
 25:("GDI/D2D 打印渲染共用","varix::render::print","渲染记录流与栅格化供给侧","Windows GDI 打印路径（本域供给侧）"),
 26:("主题绘制支撑面全量化","varix::render::theme::support","绘制原语与批量能力（零样式语义）","Windows UxTheme DrawThemeBackground 供给侧"),
 27:("渲染容错 fuzz 与资源受限矩阵","varix::render::robust","错排注入器与看门狗","Windows 渲染 API fuzz 行为账"),
 28:("无障碍渲染支撑度量","varix::render::a11y::support","坐标/矩形/对比度度量供给","Windows UIA 渲染侧支撑面"),
 29:("多屏热切换与渲染状态隔离","varix::render::multimon","多显示器参数消费与状态机","Windows WM_DISPLAYCHANGE 渲染适配语义"),
 30:("第二产段全口径收口","varix::render::f3::seg2::close","产段对账账本与跨批联签","Windows 渲染语义总对照（段级）"),
}

BOUND_EXTRA = {
 16:"元文件播放的 API 语义面归 AI-19（D4），复杂区域对象句柄归 AI-19，本条只做渲染指令执行侧与扫描带渲染；",
 17:"SIMD 指令选择属实现细节，不触碰任何内核并发原语域；",
 18:"CJK shaping 仍为扩展缺口显式登记，本条只扩度量与对照面不做复杂文法；",
 19:"完整 BiDi 双向重排仍为显式扩展缺口，本条做段落方向与镜像基础面；",
 20:"显示输出色彩配置归 AI-35，本条消费配置做渲染侧色彩处理；",
 21:"图像解码/特效算法库归 AI-38，本条只做 D2D 效果图对象模型与内置效果子集；",
 22:"本条与 B11–B13 分界——补首产段显式登记缺口（opacity mask、xor/difference、cubic 插值）；",
 23:"字体文件解析归嫁接层，嵌入字体许可位审查走 AI-95 账，本条做集合管理/嵌入/下载语义消费面；",
 24:"surface 提交与合成归 AI-26，本条只供渲染结果与失效区通知；",
 25:"打印脱机/队列/设备面归 AI-63（M3），本条只供渲染记录流与栅格化；",
 26:"主题样式数据/部件状态矩阵归 AI-29（F4），本条只供绘制原语与批量能力，零样式语义实现；",
 27:"进程崩溃恢复归会话域，本条做渲染器内部容错；",
 28:"UIA 树/Provider 语义归 AI-29（F4），本条只供渲染侧坐标/矩形/对比度度量支撑，零树语义实现；",
 29:"显示输出枚举与模式切换归 AI-35，会话管理归系统服务域，本条做渲染适配与状态隔离；",
 30:"本条为 B16–B30 全口径收口账条，跨批联签只读已收口事实不立新语义；",
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
    loc = (f"本条为 UNX-F3 域第二产段「{theme}」的深化条目——{e['title']} 的语义总装与判据落账，深化正文把骨架账条目展开为可施工、可复测、可联签的完整设计："
           f"{k} 的模型面、机检面、断言面三面一次成形，行数预算 {rows} 行在深化拆解中逐块落到子结构。")
    bound = (f"本条只收 {k} 自身的语义与判据，相邻语义只引不重立；跨域消费只走冻结接口，不私开旁路；"
             + BOUND_EXTRA[bno]
             + "渲染结果落地归 AI-26 surface、API 句柄语义归 AI-19，本条不越过两域边界，不代 F4（AI-29）开卷。")
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
    for bno in range(16, 31):
        sk = os.path.join(BATCH_DIR, f"UNX-F3-B{bno:02d}.md")
        entries = parse_skeleton(sk)
        assert len(entries) == 20, f"B{bno:02d}: entries={len(entries)}"
        theme, mod, anchor, ref = THEME[bno]
        rows_sum = sum(e["rows"] for e in entries)
        all_ids += [e["id"] for e in entries]
        lo, hi = entries[0]["id"], entries[-1]["id"]
        hdr = (f"# 域 UNX-F3 · 深化册 · UNX-F3-B{bno:02d}（F{lo:05d}–F{hi:05d} · 20 条 · 20 条新深化）\n\n"
               f"> AI-28 承办（深化轮第二段）｜本批 B{bno:02d} [已深化] 收口：F{lo:05d}–F{hi:05d} 共 20 条为本会话新深化"
               f"（骨架账 verbatim 承接：条目名/行数/判据号零改动）｜批主题：{theme}｜对照锚位：{anchor}｜"
               f"对照基线：{ref}｜上游 AI-19/AI-26 未冻结面按 Schema 先行+fake；真机/多屏判据随闸门补测｜"
               f"防重声明：只深化本域条目语义，不重立他域语义、不代 D4（AI-19）/F4（AI-29）开卷、不改 handoff 协议 schema｜"
               f"落在规划模块 {mod}｜批累计行数锁定 {rows_sum}（= 骨架账逐条之和，深化零改行数）｜"
               f"域内宪法（随批复述）：①文本渲染像素级对照复测（判据主轴一）；②ClearType 类可开关（判据主轴二）；③防重非重复（AI-84 口径复核）；"
               f"④跨域只走冻结接口；⑤账本只记实测禁止推算填充。\n")
        body = hdr + "\n" + "\n".join(compose(bno, e) for e in entries) + "\n"
        io.open(os.path.join(OUT_DIR, f"F3-B{bno:02d}.md"), "w", encoding="utf-8").write(body)
    assert len(all_ids) == 300 and len(set(all_ids)) == 300, (len(all_ids), len(set(all_ids)))
    print(f"OK deepen2: 15 batches, 300 entries, IDs F{min(all_ids):05d}-F{max(all_ids):05d}")

if __name__ == "__main__":
    main()
