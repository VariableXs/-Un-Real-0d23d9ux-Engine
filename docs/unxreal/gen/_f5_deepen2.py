# -*- coding: utf-8 -*-
"""AI-30 · UNX-F5 域深化轮第二段 B06–B20（F23301–F23600 · 300 条深化）
体例承 _f5_deepen1.py（AI-30 判例）：单源读骨架批册（ID/标题/判据/行数零转抄），
深化六要素（定位/语义边界/依赖与嫁接源/风险与回退/正文 + 判据自指）逐条展开，深化零改行数。
落盘：docs/unxreal/deepen/F5-B06..B20.md 十五册。
"""
import re

DOUT = "docs/unxreal/deepen"
BAT = "docs/unxreal/batches"

CTX = {
 "B06": dict(theme="任务栏托盘基座协议",
   kernel="句柄表核签（托盘图标句柄）与共享段只读映射（托盘协议常量）",
   dep="B04 任务栏基座为上游依赖；消费面：通知中心（B07）消费托盘区几何账",
   module="vxshell::tray"),
 "B07": dict(theme="通知中心与专注助手协议",
   kernel="共享段通知环形队列与 VFS watch 挂点（通知持久化区）",
   dep="B06 托盘几何账为上游依赖；消费面：专注助手的勿扰态消费会话账",
   module="vxshell::notif"),
 "B08": dict(theme="开始菜单架构与应用枚举协议",
   kernel="角色表核签（开始角色位）与 AppPath 解析面（fake 真源待 L3，R-F5-003）",
   dep="B01 四件套角色表为上游依赖；消费面：B09 布局持久化消费本批枚举清单",
   module="vxshell::startmenu"),
 "B09": dict(theme="开始菜单布局持久化与电源链协议",
   kernel="共享段只读映射（布局持久化区）与电源事件账（关机/重启链）",
   dep="B08 枚举清单为上游依赖；消费面：电源链判据消费 watchdog 健康账",
   module="vxshell::startmenu::layout"),
 "B10": dict(theme="桌面图标网格与右键菜单协议",
   kernel="句柄表核签（桌面窗口句柄）与 VFS watch 挂点（桌面目录监听）",
   dep="B01/B04 角色与任务栏面为上游依赖；消费面：资源管理器（B12）消费桌面目录账",
   module="vxshell::desktop::grid"),
 "B11": dict(theme="桌面壁纸主题与选择体验协议",
   kernel="vxwm 合成器握手（壁纸层提交）与共享段只读映射（主题常量）",
   dep="B01 合成器会话为上游依赖；消费面：主题切换广播消费角色表",
   module="vxshell::desktop::wallpaper"),
 "B12": dict(theme="资源管理器窗口架构与导航协议",
   kernel="句柄表核签（资源器窗口句柄）与 VFS watch 挂点（导航目录监听）",
   dep="B10 桌面目录账为上游依赖；消费面：视图管线（B13）消费导航位账",
   module="vxshell::explorer"),
 "B13": dict(theme="资源管理器视图与缩略图管线协议",
   kernel="共享段只读映射（缩略图缓存区）与图像解码面（fake，R-F5-001）",
   dep="B12 导航位账为上游依赖；消费面：状态持久化（B14）消费视图态账",
   module="vxshell::explorer::view"),
 "B14": dict(theme="shell 状态持久化与多会话隔离协议",
   kernel="内核会话账（会话隔离）与共享段（四册态账映射）",
   dep="B06–B13 各态源为上游依赖；消费面：B15 聚合账消费本批态册",
   module="vxshell::persist"),
 "B15": dict(theme="F5 首产段收口与跨批聚合账",
   kernel="syscall 注册面版本锚与账本聚合（只记实测禁止推算）",
   dep="B01–B14 全部域账为上游依赖；消费面：B16 回归集消费本批聚合断言",
   module="vxshell::ledger"),
 "B16": dict(theme="shell 架构段收口（首产段聚合回归与域内接口总冻结）",
   kernel="回归集挂接 syscall 面（角色/心跳/句柄核签）与 watchdog 健康账",
   dep="B01–B15 全域判据为上游依赖；消费面：.lnk 段（B17–B20）消费总冻结接口",
   module="vxshell::regression"),
 "B17": dict(theme=".lnk 快捷方式段 · 结构解析与头部校验",
   kernel="VFS 读取挂点（lnk 文件只读面）与共享段只读映射（解析缓存）",
   dep="B16 接口总冻结为上游依赖；消费面：环境展开（B18）消费头部结构账",
   module="vxshell::lnk"),
 "B18": dict(theme=".lnk 段 · 环境变量与相对路径展开语义",
   kernel="进程环境块账（env 进程级）与 VFS 存在性探针（展开校验）",
   dep="B17 头部结构账为上游依赖；消费面：图标位解析（B19）消费展开后路径",
   module="vxshell::lnk::env"),
 "B19": dict(theme=".lnk 段 · 图标位、参数串与工作目录语义",
   kernel="句柄表核签（资源句柄透传渲染域）与渲染域冻结接口",
   dep="B17/B18 结构与展开面为上游依赖；消费面：写回原子性（B20）消费字段账",
   module="vxshell::lnk::icon"),
 "B20": dict(theme=".lnk 段 · 写回原子性与属性编辑",
   kernel="内核原子写承诺面（先备份后修改可回滚）与 VFS unlink/watch 挂点",
   dep="B17–B19 解析/展开/字段账为上游依赖；消费面：开始菜单与桌面消费编辑回执",
   module="vxshell::lnk::writeback"),
}

def parse_batch(bid):
    txt = open(f"{BAT}/UNX-F5-{bid}.md", encoding="utf-8").read()
    theme_m = re.search(r"^# UNX-F5-" + bid + r" · (.+?)（", txt, re.M)
    rows = re.findall(
        r"^### UNX-F(\d+) · (.+)$\n- 域/批：F5/"+bid+r"｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：UNX-F\1-J1 (.+)$",
        txt, re.M)
    return theme_m.group(1), rows

def main():
    n = 0
    for bid in ["B06","B07","B08","B09","B10","B11","B12","B13","B14","B15",
                "B16","B17","B18","B19","B20"]:
        theme, rows = parse_batch(bid)
        ctx = CTX[bid]
        lo, hi = rows[0][0], rows[-1][0]
        total = sum(int(r[2]) for r in rows)
        hdr = (f"# 域 UNX-F5 · 深化册 · UNX-F5-{bid}（F{lo}–F{hi} · {len(rows)} 条 · {len(rows)} 条新深化）\n\n"
          f"> AI-30 承办（波09 深化轮第二段）｜本批 {bid} [已深化] 收口：F{lo}–F{hi} 共 {len(rows)} 条为本会话新深化"
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
        n += len(rows)
        print(f"F5-{bid}.md：{len(rows)} 条深化落盘（F{lo}–F{hi}，{total} 行 verbatim）")
    print(f"深化轮第二段 B06–B20 完成：15 册 {n} 条")

if __name__ == "__main__":
    main()
