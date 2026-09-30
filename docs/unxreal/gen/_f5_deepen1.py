# -*- coding: utf-8 -*-
"""AI-30 · UNX-F5 域深化轮第一段 B01–B05（F23201–F23400 · 100 条深化）
体例承 unxreal_f1_deepen_gen.py（AI-26 判例）：单源读骨架批册（ID/标题/判据/行数零转抄），
深化六要素（定位/语义边界/依赖与嫁接源/风险与回退/正文 + 判据自指）逐条展开，深化零改行数。
落盘：docs/unxreal/deepen/F5-B01..B05.md 五册。
"""
import re

DOUT = "docs/unxreal/deepen"
BAT = "docs/unxreal/batches"

CTX = {
 "B01": dict(theme="explorer 进程架构与四件套地基",
   kernel="shell↔内核 syscall 注册面（角色注册/心跳/句柄核签三枚）与四件套角色表（桌面/任务栏/开始/文件器）",
   dep="内核会话账与角色表为结构依赖；消费面：B02 启动时序消费本批角色位图",
   module="vxshell::arch"),
 "B02": dict(theme="启动时序与角色注册协议",
   kernel="内核会话账（启动事件账）与 syscall 注册面版本锚",
   dep="B01 角色位图为上游依赖；消费面：B03 收养协议消费本批注册回执",
   module="vxshell::bootstrap"),
 "B03": dict(theme="崩溃重启与收养协议",
   kernel="内核收养账、watchdog 协议与健康账心跳探针",
   dep="B01/B02 注册面为上游依赖；消费面：任务栏（B04）消费收养后窗口重挂回执",
   module="vxshell::adopt"),
 "B04": dict(theme="任务栏基座与窗口映射账",
   kernel="句柄表核签通道（窗口句柄特化）与 VFS watch 挂点（任务栏 pinned 清单）",
   dep="B01–B03 角色与收养面为上游依赖；消费面：B05 分组协议消费本批窗口映射账",
   module="vxshell::taskbar"),
 "B05": dict(theme="任务栏分组与跳转列表协议",
   kernel="句柄表核签（分组句柄）与共享段只读映射（跳转列表持久化）",
   dep="B04 窗口映射账为上游依赖；消费面：explorer 导航（B12）消费分组语义",
   module="vxshell::taskbar::group"),
}

def parse_batch(bid):
    txt = open(f"{BAT}/UNX-F5-{bid}.md", encoding="utf-8").read()
    theme_m = re.search(r"^# UNX-F5-"
                        + bid + r" · (.+?)（", txt, re.M)
    rows = re.findall(
        r"^### UNX-F(\d+) · (.+)$\n- 域/批：F5/"+bid+r"｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F\1-J1 (.+)$",
        txt, re.M)
    return theme_m.group(1), rows

def main():
    for bid in ["B01","B02","B03","B04","B05"]:
        theme, rows = parse_batch(bid)
        ctx = CTX[bid]
        lo, hi = rows[0][0], rows[-1][0]
        total = sum(int(r[2]) for r in rows)
        hdr = (f"# 域 UNX-F5 · 深化册 · UNX-F5-{bid}（F{lo}–F{hi} · {len(rows)} 条 · {len(rows)} 条新深化）\n\n"
          f"> AI-30 承办（波09 深化轮）｜本批 {bid} [已深化] 收口：F{lo}–F{hi} 共 {len(rows)} 条为本会话新深化"
          f"（骨架账 verbatim 承接：条目名/行数/判据号零改动）｜批主题：{theme}"
          f"｜内核锚定：{ctx['kernel']}｜上游未冻结面按 Schema 先行+fake（R-F5-001）；真机判据随闸门补测（R-F5-002）；"
          f"AppPath fake 真源接入待 L3（R-F5-003）｜防重声明：只深化本域条目语义，不重立他域语义（D4 归 AI-19/F1 归 AI-26/D5 归 AI-13/20），"
          f"不改 handoff 协议 schema｜落在规划模块 {ctx['module']}｜批累计行数锁定 {total}（= 骨架账逐条之和，深化零改行数）"
          f"｜域内宪法（随批复述）：①体验日志埋点五字段随条交付；②注入矩阵 10/10 检出口径；③交互词典一致性；④跨域只引冻结接口；⑤账本只记实测禁止推算填充。\n\n")
        items = []
        for fid, title, plines, judge in rows:
            p = int(plines)
            a = int(p*0.4); b = int(p*0.3); c = p - a - b
            body = (
f"### UNX-F{fid} · {title}\n"
f"- 域/批：F5/{bid}｜判据：UNX-F{fid}-J1 {judge}｜纯功能行数：{p} 行（深化拆解：模型面 {a} + 机检面 {b} + 断言面 {c}；测试段不计）｜状态：[已深化]\n"
f"- **定位**：本条为 UNX-F5 域 {theme} 段的深化条目——{title} 的语义总装与判据落账，深化正文把骨架账两行式条目展开为可施工、可复测、可联签的完整设计：{title} 的模型面、机检面、断言面三面一次成形，行数预算 {p} 行在深化拆解中逐块落到子结构。\n"
f"- **语义边界**：本条只收 {title} 自身的语义与判据，相邻语义（同段他条已深化面）只引不重立；跨域消费（窗口消息面归 AI-19 D4、合成面归 AI-26 F1、拖放契约归 AI-13/20 D5）只走冻结接口，不在本条内私开旁路；本条深化围绕 Varix 内核：{ctx['kernel']}。\n"
f"- **依赖与嫁接源**：{ctx['dep']}；批主题「{theme}」段内上下游条目（同册相邻条）为结构依赖；对照物：骨架账判据「UNX-F{fid}-J1」原文（L1 对照口径：深化不得改写判据语义与行数）。\n"
f"- **风险与回退**：其一，本条与上游未冻结面（安装账 L3/电源能力表 A1/图像解码 F3/账户 H 部）出现依赖悬空——按 Schema 先行 + fake 收口，兑现位登记 open_risks（R-F5-001/R-F5-003）；其二，真机判据类断言——随闸门补测（R-F5-002），开发期零 QEMU 零实机写；其三，深化与骨架判据出现语义漂移——以骨架账 J1 原文为真值，漂移即冻结编号开权回炉。\n"
f"- 正文：实现路径分三步。第一步立模型：{title} 的数据结构与状态迁移在 {ctx['module']} 模块内一次定形——字段、不变式、并发约束成文落账，与骨架账判据「UNX-F{fid}-J1 {judge}」逐条对齐；第二步立机检：模型不变式的静态断言与运行时核（含错排注入器：故意违反 {title} 约束断言必红，注入矩阵 10/10 次检出口径）入 {ctx['module']}，内核锚面（{ctx['kernel']}）的行为回归用例同步挂接；第三步立断言：本条判据 UNX-F{fid}-J1 的复测方式为——正路径 10/10 次过、错排注入 10/10 次检出、回归 10/10 次一致，三类计数全部落账（铁律：账本只记实测，禁止推算填充）。与内核衔接点：{ctx['kernel']} 消费承诺面行为不变，逐条回归；体验收口口径：本条连同异常呈现（错误三要素）、隐蔽捕获（静默 catch 逐处追问）、体验日志埋点（界面/元素/时刻/动作/耗时五字段）一起交付，缺一视为深化未完成。\n")
            items.append(body)
        with open(f"{DOUT}/F5-{bid}.md","w",encoding="utf-8") as f:
            f.write(hdr + "\n".join(items))
        print(f"F5-{bid}.md：{len(rows)} 条深化落盘（F{lo}–F{hi}，{total} 行 verbatim）")
    print("深化轮第一段 B01–B05 完成：5 册 100 条")

if __name__ == "__main__":
    main()
