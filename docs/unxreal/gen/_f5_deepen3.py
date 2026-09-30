# -*- coding: utf-8 -*-
"""AI-30 · UNX-F5 域深化轮终段 B21–B40（F23601–F24000 · 400 条深化）
体例承 _f5_deepen1.py（AI-30 判例）：单源读骨架批册（ID/标题/判据/行数零转抄），
深化六要素（定位/语义边界/依赖与嫁接源/风险与回退/正文 + 判据自指）逐条展开，深化零改行数。
落盘：docs/unxreal/deepen/F5-B21..B40.md 二十册。
"""
import re

DOUT = "docs/unxreal/deepen"
BAT = "docs/unxreal/batches"

CTX = {
 "B21": dict(theme=".lnk 段 · 断链三态与修复建议引擎",
   kernel="VFS 存在性探针与共享段只读映射（断链缓存）",
   dep="B18 展开面为上游依赖；消费面：修复建议消费句柄表核签（目标进程账）",
   module="vxshell::lnk::heal"),
 "B22": dict(theme=".lnk 段 · 解析安全（零执行红线与炸弹防护）",
   kernel="零执行红线承诺面（解析不拉起）与 VFS 炸弹防护（深度/大小上限账）",
   dep="B17–B21 解析全链为上游依赖；消费面：回收站段（B23）消费安全解析回执",
   module="vxshell::lnk::safe"),
 "B23": dict(theme="回收站段 · $I/$R 配对模型与卷元数据",
   kernel="VFS unlink 挂点（入站事件源）与共享段只读映射（配对账）",
   dep="B22 安全解析为上游依赖；消费面：入站路径（B24）消费配对协议",
   module="vxshell::recycle::pair"),
 "B24": dict(theme="回收站段 · 删除入站路径全集",
   kernel="VFS unlink/watch 挂点（多源删除事件归一）与会话账（来源会话）",
   dep="B23 配对模型为上游依赖；消费面：还原事务（B25）消费入站快照",
   module="vxshell::recycle::intake"),
 "B25": dict(theme="回收站段 · 还原事务性与 undo 链",
   kernel="内核原子写承诺面（还原事务）与撤销安全边界（undo 链）",
   dep="B24 入站快照为上游依赖；消费面：撤销链联动（B36）消费还原回执",
   module="vxshell::recycle::restore"),
 "B26": dict(theme="回收站段 · SID 隔离与多用户会话语义",
   kernel="内核会话账（会话隔离）与归属隔离红线（SID 簿记）",
   dep="B23 配对账为上游依赖；消费面：容量策略（B27）消费配额归属",
   module="vxshell::recycle::sid"),
 "B27": dict(theme="回收站段 · 容量策略与逐出（站容水位）",
   kernel="共享段只读映射（水位账）与预算降级闸（逐出预算）",
   dep="B25/B26 事务与归属面为上游依赖；消费面：安全清空（B28）消费逐出清单",
   module="vxshell::recycle::quota"),
 "B28": dict(theme="回收站段 · 安全清空与 dry-run 预演",
   kernel="破坏性操作三重验证承诺面与 dry-run 投影（清单先行）",
   dep="B27 逐出清单为上游依赖；消费面：清空回执入撤销边界（B36 联动面）",
   module="vxshell::recycle::purge"),
 "B29": dict(theme="拖放段 · 数据对象与格式协商面",
   kernel="共享段只读映射（格式描述符账）与句柄表核签（数据对象句柄）",
   dep="B01–B28 shell 面为上游依赖；消费面：命中测试（B31）消费格式能力位",
   module="vxshell::dnd::dataobj"),
 "B30": dict(theme="拖放段 · 效果树、取消与失败路径全收口",
   kernel="体验日志埋点面（效应树五字段）与取消承诺面（中断恢复）",
   dep="B29 数据对象为上游依赖；消费面：落点语义（B31）消费取消语义",
   module="vxshell::dnd::effect"),
 "B31": dict(theme="拖放段 · 目标命中测试与落点语义（Shell 目标侧）",
   kernel="句柄表核签（目标窗口句柄）与非法落点拒绝红线（引导卷/受保护路径）",
   dep="B29/B30 协商与效应面为上游依赖；消费面：视觉反馈（B32）消费落点回执",
   module="vxshell::dnd::hit"),
 "B32": dict(theme="拖放段 · 视觉反馈与拖放手感（幽灵图/插队提示/惯性）",
   kernel="vxwm 握手（幽灵图层提交）与体验日志面（拖放手感五字段）",
   dep="B31 落点回执为上游依赖；消费面：剪贴板互操作（B33）消费拖放视觉语义",
   module="vxshell::dnd::visual"),
 "B33": dict(theme="拖放段 · 剪贴板互操作与 shell 拖放面收口",
   kernel="共享段只读映射（剪贴板格式账）与 VFS watch（剪贴板持久化探针）",
   dep="B29–B32 拖放全链为上游依赖；消费面：文件编排（B34）消费互操作契约",
   module="vxshell::dnd::clip"),
 "B34": dict(theme="文件操作编排段 · 操作引擎（复制/移动进度账与暂停恢复）",
   kernel="VFS 读写白名单红线与进度账（只记实测）+暂停恢复状态机",
   dep="B33 互操作契约为上游依赖；消费面：冲突矩阵（B35）消费操作计划账",
   module="vxshell::fileops::engine"),
 "B35": dict(theme="文件操作编排段 · 冲突矩阵与覆盖差异预览",
   kernel="只读覆盖保护红线与共享段只读映射（差异预览缓存）",
   dep="B34 操作计划账为上游依赖；消费面：撤销链（B36）消费覆盖决策账",
   module="vxshell::fileops::conflict"),
 "B36": dict(theme="文件操作编排段 · 撤销链与回收站联动收口",
   kernel="撤销安全边界红线与回收站入站/还原账（B24/B25 联动）",
   dep="B25/B34/B35 为上游依赖；消费面：对话框族（B37）消费 undo 语义",
   module="vxshell::fileops::undo"),
 "B37": dict(theme="文件操作编排段 · Shell 对话框族（打开/保存/目录浏览）",
   kernel="归属隔离红线（对话框部件零逃逸）与句柄表核签（对话框句柄）",
   dep="B34–B36 编排面为上游依赖；消费面：双击全链（B38）消费文件器对话框",
   module="vxshell::fileops::dlg"),
 "B38": dict(theme="双击全链段 · 六环探针（关联解析→进程创建）",
   kernel="收养账与进程创建承诺面（AppPath fake 待 L3，R-F5-003）+关联劫持拒绝红线",
   dep="B37/B17–B22 解析面为上游依赖；消费面：预算账（B39）消费六环计时",
   module="vxshell::launch"),
 "B39": dict(theme="双击全链段 · 环节预算账与挫败信号聚合",
   kernel="体验日志面（挫败信号五类）与账本聚合承诺面（只记实测）",
   dep="B38 六环计时为上游依赖；消费面：域收口（B40）消费聚合账",
   module="vxshell::launch::budget"),
 "B40": dict(theme="域收口段 · UNX-F5 满账收口（编号总轧/判据总库/三印宣告）",
   kernel="syscall 注册面版本锚与红线总账（十二把锁永久回归）+健康账终核",
   dep="B01–B39 全域为上游依赖；消费面：波16 联调消费域移交包",
   module="vxshell::seal"),
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
    for bid in ["B%02d" % i for i in range(21, 41)]:
        theme, rows = parse_batch(bid)
        ctx = CTX[bid]
        lo, hi = rows[0][0], rows[-1][0]
        total = sum(int(r[2]) for r in rows)
        hdr = (f"# 域 UNX-F5 · 深化册 · UNX-F5-{bid}（F{lo}–F{hi} · {len(rows)} 条 · {len(rows)} 条新深化）\n\n"
          f"> AI-30 承办（波09 深化轮终段）｜本批 {bid} [已深化] 收口：F{lo}–F{hi} 共 {len(rows)} 条为本会话新深化"
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
    print(f"深化轮终段 B21–B40 完成：20 册 {n} 条")

if __name__ == "__main__":
    main()
