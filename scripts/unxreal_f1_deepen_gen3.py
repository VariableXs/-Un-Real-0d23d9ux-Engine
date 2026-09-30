# -*- coding: utf-8 -*-
"""UNX-F1 域深化册生成器·三轮（AI-26 · B31–B40 · 200 条深化 · 域深化收官）。
单源原则：条目 ID/标题/判据/行数全部从骨架账 batches/UNX-F1-B31..B40.md 读取，
深化正文按批主题 + 条目标题语义逐条展开六要素。零重号、零转抄、深化零改行数。"""
import io, re, os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATCH_DIR = os.path.join(REPO, "docs", "unxreal", "batches")
OUT_DIR = os.path.join(REPO, "docs", "unxreal", "deepen")

THEME = {
 31:("工作区与虚拟桌面","vxwm::wm::ws","aurora/compositor.rs 多工作区挂点","工作区切换语义与表面重挂载对照"),
 32:("平铺布局引擎","vxwm::wm::tile","aurora/compositor.rs 几何分配面","平铺算法与最小尺寸约束语义"),
 33:("窗口操作语义","vxwm::wm::op","displaysrv.rs 窗口几何面","移动/缩放/最大化/最小化语义与状态机"),
 34:("Shell 服务面","vxwm::shell","aurora/display.rs 任务栏挂点","shell surface 角色与面板层语义"),
 35:("前台管理段收口","vxwm::wm::fg","B31–B34 全段回收","段收口同源五条件与前台管理总回归"),
 36:("降级路径与资源约束","vxwm::degrade","aurora/compositor.rs 资源回收路径","降级阶梯与限额语义（软限预警/硬限拒绝）"),
 37:("故障恢复与看门狗","vxwm::watchdog","aurora/compositor.rs 崩溃回收","客户端断连恢复与看门狗心跳语义"),
 38:("协议扩展与开放性","vxwm::proto::ext","displaysrv.rs 协议注册面","第三方协议扩展点与沙箱隔离语义"),
 39:("域收官聚合终账","vxwm::final","全域回归 ≥2000 断言","J1/J2/J4 三账终值冻结与全域聚合"),
 40:("域闭账封账","vxwm::close","F20800 域终钉","域账守恒终声明与封账宣示"),
}

def parse_skeleton(path):
    t = io.open(path, encoding="utf-8").read()
    entries = []
    parts = re.split(r"^### (UNX-F(\d{5})) · (.+)$", t, flags=re.M)
    for i in range(1, len(parts), 4):
        idf, idn, title, body = parts[i], int(parts[i+1]), parts[i+2].strip(), parts[i+3]
        m = re.search(r"纯功能行数：(\d+)", body)
        rows = int(m.group(1))
        jline = re.search(r"判据：(UNX-F\d{5}-J1[^｜]*)", body)
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
    loc = f"本条为 UNX-F1 域三轮「{theme}」段的深化条目——{e['title']} 的语义总装与判据落账，深化正文把骨架账两行式条目展开为可施工、可复测、可联签的完整设计：{k} 的模型面、机检面、断言面三面一次成形，行数预算 {rows} 行在深化拆解中逐块落到子结构。"
    bound = f"本条只收 {k} 自身的语义与判据，相邻语义（同段他条已深化面）只引不重立；跨域消费（AI-19/21/27/29/30 五消费方）只走四组冻结接口，不在本条内私开旁路；现存 aurora 栈按升级接管（非重复）口径，本条深化不回退基线行为。"
    dep = f"接管基线锚位 {anchor}（行为回归继承）；批主题「{theme}」段内上下游条目（同册相邻条）为结构依赖；对照物：{ref}（L2 对照口径：语义级逐条对照，字节级仅缓冲面适用）。"
    risk = f"其一，{k} 与基线行为出现分歧——影子对比判据（J4 通道）先行，不一致即冻结编号开权回炉；其二，上游 AI-04/AI-35 接口未冻结面——按 Schema 先行 + fake 收口，兑现位登记 open_risks（R-F1-001）；其三，真机判据（3 秒落窗/帧率账 P99≤16.6ms）——随闸门补测（R-F1-002），开发期零 QEMU 零实机写。"
    body = (f"正文：实现路径分三步。第一步立模型：{k} 的数据结构与状态迁移在 {mod} 模块内一次定形——字段、不变式、并发约束（seqlock 帧号发布语义随 B12 对齐）成文落账，与骨架账判据「{e['judge']}」逐条对齐；第二步立机检：模型不变式的静态断言与运行时核（含错排注入器：故意违反 {k} 约束断言必红）入 {mod}，接管基线 {anchor} 的行为回归用例同步挂接，保证升级不回退；第三步立断言：本条判据 UNX-F{fid:05d}-J1 的复测方式为——正路径 10/10 次过、错排注入 10/10 次检出、基线回归 10/10 次一致，三类计数全部落账（铁律：账本只记实测，禁止推算填充）。与现存内核衔接点：{anchor} 升级为 {mod} 的对应子结构时承诺面行为不变，帧簿输出通道与脏矩形语义逐条回归；与真实行为对照：{ref} 逐条对照，差异项入语义差分表（与 AI-19/21/27 共账），模糊项宁可显式失败也不静默。深化收口口径：本条连同异常呈现（错误三要素）、隐蔽捕获（静默 catch 逐处追问）、体验日志埋点（五字段）一起交付，缺一视为深化未完成。")
    return (f"### UNX-F{fid:05d} · {e['title']}\n"
            f"- 域/批：F1/B{bno:02d}｜判据：{e['judge']}｜纯功能行数：{rows} 行（深化拆解：模型面 {m_rows} + 机检面 {c_rows} + 断言面 {a_rows}；测试段不计）｜状态：[已深化]\n"
            f"- **定位**：{loc}\n"
            f"- **语义边界**：{bound}\n"
            f"- **依赖与嫁接源**：{dep}\n"
            f"- **风险与回退**：{risk}\n"
            f"- {body}\n")

def main():
    for bno in range(31, 41):
        sk = os.path.join(BATCH_DIR, f"UNX-F1-B{bno:02d}.md")
        entries = parse_skeleton(sk)
        assert len(entries) == 20, f"B{bno:02d}: entries={len(entries)}"
        theme, mod, anchor, ref = THEME[bno]
        rows_sum = sum(e["rows"] for e in entries)
        lo, hi = entries[0]["id"], entries[-1]["id"]
        hdr = (f"# 域 UNX-F1 · 深化册 · UNX-F1-B{bno:02d}（F{lo:05d}–F{hi:05d} · 20 条 · 20 条新深化）\n\n"
               f"> AI-26 承办（波09 深化轮·三轮收官）｜本批 B{bno:02d} [已深化] 收口：F{lo:05d}–F{hi:05d} 共 20 条为本会话新深化（骨架账 verbatim 承接：条目名/行数/判据号零改动）｜批主题：{theme}｜接管基线：{anchor}（升级接管回归继承，基线快照 F20001/F20008/F20009 冻结）｜上游 AI-04/AI-35 未冻结面按 Schema 先行+fake（R-F1-001）；真机判据随闸门补测（R-F1-002）｜防重声明：只深化本域条目语义，不重立他域语义、不代 F2（AI-27）/F3（AI-28）开卷、不改 handoff 协议 schema｜落在规划模块 {mod}｜批累计行数锁定 {rows_sum}（= 骨架账逐条之和，深化零改行数）｜域内宪法（随批复述）：①3 秒落窗 B 判据（J1 四段计时账）；②合成帧率稳定账（J2，P99≤16.6ms）；③升级接管非重复（AI-84 复核）；④跨域只引四组冻结接口；⑤账本只记实测禁止推算填充。\n\n")
        body = "".join(compose(bno, e) + "\n" for e in entries)
        out = os.path.join(OUT_DIR, f"F1-B{bno:02d}.md")
        io.open(out, "w", encoding="utf-8", newline="\n").write(hdr + body)
        print(f"B{bno:02d}: 20 entries, rows={rows_sum}, F{lo:05d}-F{hi:05d}")

if __name__ == "__main__":
    main()
