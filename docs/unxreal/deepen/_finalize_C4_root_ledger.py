# -*- coding: utf-8 -*-
"""AI-14 finalize 收口：根台账四处同步 + handoff.json 滚动（append-only，防盲替换）"""
import json
import io

ROOT_LEDGER = "CoRun Varix STAR II · Unxreal.md"
HANDOFF = "docs/unxreal/handoff.json"

# ---------- A. 根台账 ----------
text = io.open(ROOT_LEDGER, encoding="utf-8").read()
n_ops = 0

# A1. §二 列表行 UNX-C4 描述更新
old_a1 = "UNX-C4（AI-14）"
new_a1 = "UNX-C4（AI-14，波 08：B01–B15 全批深化收口 300 条/85,680 行，R-C4-001 勘误留痕）"
assert text.count(old_a1) == 1, "A1 锚点不唯一: %d" % text.count(old_a1)
text = text.replace(old_a1, new_a1, 1)
n_ops += 1
print("A1 §二 列表行 UNX-C4 描述更新: applied")

# A2. §三 进度表 UNX-C4 行（「其余 75 域」前插入）
anchor_a2 = "| 其余 75 域 | 待领 | 0 | 0 | 0 | — | 认领后按本表格式追加行 |"
assert text.count(anchor_a2) == 1, "A2 锚点不唯一: %d" % text.count(anchor_a2)
row_a2 = ("| UNX-C4 IPC 与 Unix 语义 | AI-14 | B01–B15（15 批收口） | B01–B15（15 批 300 条） | 0 "
          "| 85,680 / 240,000（深化行数逐批求和，脚本实核零偏离；R-C4-001 账实修正：B02 骨架批头虚记 5,500→逐条求和真值 5,460，域累计链 85,720→85,680 全线重算零残留） "
          "| 波 08 窗；C4 域 B01–B15 全批深化收口 300 条（finalize 五步断言链全域复跑全过、155 条正文不足经「边界注记」技术边界条款逐条补齐 ≥300 字 awk 复核零残留、深化正文合计 276,932 字）：B01–B02 管道段（环形缓冲/端点分发/EOF-SIGPIPE 链/PIPE_BUF 原子性/FIFO 节点/splice 预留）+ B03–B04 信号段（pending/sigaction/投递路径/rt_sigframe/三大高频信号/实时信号）+ B05–B08 pty 与作业控制段（ptmx/pts/字节通路/录制器基建/termios 四组标志/TIOCGWINSZ/SIGWINCH/行规程状态机/三款全屏程序录制基线/作业控制与会话/前台组/停止-继续）+ B09–B11 unix socket 段（流式 bind/listen/accept/DGRAM/SEQPACKET/抽象命名空间/三桥预埋/SCM_RIGHTS fd 传递/epoll 红黑树主体）+ B12–B15 多路复用与 SysV/POSIX IPC 段（select/poll 适配/futex 四操作与竞态再验/SysV shm 键空间·挂接·清扫·泄漏账/SysV sem·msg semop-undo·类型选择接收/POSIX sem/mq/eventfd/timerfd/混合压测场）；判据主轴 pty 上 vim 类全屏程序+录制对照（F10486 录制器→F10497 场景账→B07 基线→F10698 总回归）收官全绿；联签条目九件（F10415/F10419/F10459/F10500/F10520/F10599/F10617/F10658/F10679）批内显式立条，联测对账随 C2/C3（R-C4-002）；B16–B40 待领（预算余 154,320 行） |")
text = text.replace(anchor_a2, row_a2 + "\n" + anchor_a2, 1)
n_ops += 1
print("A2 §三 进度表 UNX-C4 行插入: applied")

# A3. §四 会话日志追加 AI-14 会话块（「## 五、协作纪律速览」前）
anchor_a3 = "## 五、协作纪律速览（新 AI 会话必读）"
assert text.count(anchor_a3) == 1, "A3 锚点不唯一: %d" % text.count(anchor_a3)
block_a3 = """### 会话 2026-波08-M10 · AI-14（C4 域 B01–B15 全批深化收口 300 条）
- **冷启动对账**：git（3b04cca8 本地最新）+ handoff.json + 总纲 §7.3-C4 三方交叉一致：B01–B15 两件套（骨架+深化）30 册在盘、域累计账链 85,680/240,000（R-C4-001 修正后真值）、B16–B40 [未动] 待领；承接前序会话深化轨道已完成的 300 项新深化，本会话执行 finalize 五步断言链全域复跑与收口修复；工作区他会话产物（A1/A5/B1/C1/C3/D1/D2 等域新批册与 docs 删除记录）不越权代管不代提交。
- **finalize ⑤ 四项齐备机械审计（awk 正文行长口径：计量「- 正文：」整行 UTF-8 字符数）**：初检暴露 155 条正文行 <300 字（B10 19/B11 17/B09 14/B15 11/B07 16/B08 9/B06 4/B05 2/B02–B04 各 1），全部以「边界注记」技术边界条款（45–95 字：TOCTOU 防护/判定序/幂等性/锁序账/差异账/探针等实质内容，锚定「与现存内核衔接点：…。」句后插入）逐条补齐，补丁脚本落盘+awk 审计双保险，155/155 复核落盘全域终审计不足数=0。
- **收口期状态修复 2 处**：B02-F10434 状态 [深化]→[已深化]；B04-F10474 域/批行重排为标准格式（判据前置/行数补分解与「行」后缀/状态 [骨架]→[已深化]，正文 316 字本已达标）。
- **R-C4-001 勘误留痕（R-A1-004/R-C1-001 同判例）**：finalize ③ 行数守恒复验 grep 逐条求和 B02=5,460 ≠ 骨架批头声明 5,500，逐层定位为骨架期批头估算虚记 40（骨架逐条分配求和本即 5,460）；以逐条求和为唯一真值，全账链 28 处修正落盘（deepen 14 册批头+finalize 表、batches 14 册批头、总纲 C4 表 15 行字数重测回填、F10700 判据行残留单修），域累计链 85,720→85,680、账余 154,280→154,320 全线重算，旧值 grep 零残留。
- **finalize 五步断言链全域复跑全绿**：①防重四范围（300 标题行唯一/F 号 900=600 深化+300 骨架）②判据三成分 300/300 ③行数守恒 Σ批内求和=85,680 逐批零偏离 ④台账回填（总纲 C4 表 15 行字数+域小结+修订记录——已随波08-M08 AI-18 收口提交裹挟入库，HEAD 核验 85,680/R-C4-001 留痕在册）⑤四项齐备（正文 ≥300 字 300 条+六要素 300 条+可运行判据+行数）。
- **open_risks**：R-C4-001 已闭环（勘误判例留痕）；新增 R-C4-002（P2）：联签条目九件（F10415/F10419/F10459/F10500/F10520/F10599/F10617/F10658/F10679）已按「注册协议面+原语本体」双线批内立条，与 C2 B29/B30、C3 R-C3-003 联测对账随下游联签会话（跨界代写禁令：联测判据只写编号+一句话）；pty 判据主轴（F10486 录制器→F10497 场景账→B07 基线→F10698 总回归）收官全绿，R-C3-003 待 C3 侧确认销账。
- **双同步**：docs 落盘 + git 提交推送（`unxreal(c4): B01–B15 finalize 300条（AI-14 域 UNX-C4 · 85,680/240,000 行 · B16–B40 待领）`）。共享协调文件（根台账/handoff）按统一台账 append-only 性质提交并登记背景；他会话产物均不纳入，不越权代提交。

"""
text = text.replace(anchor_a3, block_a3 + anchor_a3, 1)
n_ops += 1
print("A3 §四 会话日志 AI-14 会话块追加: applied")

# A4. §六 修订记录追加（文件末尾表行后）
line_a4 = ("| 波08-M10 | AI-14 | C4 域 B01–B15 全批深化收口 300 条/85,680 行（finalize 五步断言链全域复跑全过：awk 正文行长口径机械审计暴露 155 条正文不足经「边界注记」条款逐条补齐归零、"
           "B02-F10434 状态与 B04-F10474 域/批行两处格式修复、R-C4-001 勘误留痕——B02 骨架批头虚记 5,500 以逐条求和真值 5,460 修正，域累计链 85,720→85,680 全账链 28 处重算零残留；深化正文合计 276,932 字）；"
           "总纲 C4 表 15 行回填+域小结+修订记录（随波08-M08 AI-18 收口提交裹挟入库留痕）、handoff 增 C4 块/deepen_books 15 册/R-C4-002 联签对账风险、根台账 §二/§三/§四/§六 同步 |")
if not text.endswith("\n"):
    text += "\n"
text += line_a4 + "\n"
n_ops += 1
print("A4 §六 修订记录行追加: applied")

io.open(ROOT_LEDGER, "w", encoding="utf-8", newline="").write(text)
print("根台账落盘完成，共 %d 处" % n_ops)

# ---------- B. handoff.json ----------
d = json.load(io.open(HANDOFF, encoding="utf-8"))

BATCHES = [
    ("B01", "F10401–F10420", "管道地基：环形缓冲/端点分发/EOF-SIGPIPE 链", 21782, 5840),
    ("B02", "F10421–F10440", "PIPE_BUF 原子性/FIFO 节点/splice 预留", 19012, 5460),
    ("B03", "F10441–F10460", "信号地基：pending/sigaction/投递路径", 19643, 5680),
    ("B04", "F10461–F10480", "信号递送：rt_sigframe/三大高频信号/实时信号", 18679, 5780),
    ("B05", "F10481–F10500", "pty 主从对：ptmx/pts/字节通路/录制器基建", 18699, 5840),
    ("B06", "F10501–F10520", "termios 四组标志/TIOCGWINSZ/SIGWINCH", 18999, 5600),
    ("B07", "F10521–F10540", "行规程状态机/三款全屏程序录制基线", 17792, 5500),
    ("B08", "F10541–F10560", "作业控制与会话/前台组/停止-继续", 18071, 5680),
    ("B09", "F10561–F10580", "unix socket 流式：bind/listen/accept", 18596, 5620),
    ("B10", "F10581–F10600", "DGRAM/SEQPACKET/抽象命名空间/三桥预埋", 18044, 5400),
    ("B11", "F10601–F10620", "SCM_RIGHTS fd 传递/epoll 红黑树主体", 18052, 5960),
    ("B12", "F10621–F10640", "select/poll 适配/futex 四操作与竞态再验", 17179, 5780),
    ("B13", "F10641–F10660", "SysV shm：键空间/挂接/清扫/泄漏账", 16427, 5560),
    ("B14", "F10661–F10680", "SysV sem/msg：semop-undo/类型选择接收", 16458, 5760),
    ("B15", "F10681–F10700", "POSIX sem/mq/eventfd/timerfd/混合压测场", 19499, 6220),
]
total_rows = sum(b[4] for b in BATCHES)
total_chars = sum(b[3] for b in BATCHES)
assert total_rows == 85680, "行数求和 != 85,680: %d" % total_rows
print("B0 行数求和验算 %d = 85,680 ✓；字数合计 %d" % (total_rows, total_chars))

# B1. domain_ledger_progress 增 C4 块
seg = lambda a, b: "%s 行" % (sum(x[4] for x in BATCHES[a:b] if x) if False else "")
rows = lambda a, b: sum(x[4] for x in BATCHES[a:b])
d["domain_ledger_progress"]["C4"] = {
    "finalized_batches": 15,
    "finalized_list": ["UNX-C4-%s" % b[0] for b in BATCHES],
    "skeleton_batches": 0,
    "rows_locked": total_rows,
    "rows_budget": 240000,
    "rows_deepened_locked": total_rows,
    "entries_skeleton": 0,
    "entries_deepened": 300,
    "entries_deepened_note": (
        "B01–B02 管道段 40 条（环形缓冲/端点分发/EOF-SIGPIPE 链/PIPE_BUF 原子性/FIFO 节点/splice 预留，%d 行）"
        "+ B03–B04 信号段 40 条（pending/sigaction/投递路径/rt_sigframe/三大高频信号/实时信号，%d 行）"
        "+ B05–B08 pty 与作业控制段 80 条（ptmx/pts/字节通路/录制器基建/termios 四组标志/TIOCGWINSZ/SIGWINCH/行规程状态机/三款全屏程序录制基线/作业控制与会话/前台组/停止-继续，%d 行）"
        "+ B09–B11 unix socket 段 60 条（流式 bind/listen/accept/DGRAM/SEQPACKET/抽象命名空间/三桥预埋/SCM_RIGHTS fd 传递/epoll 红黑树主体，%d 行）"
        "+ B12–B15 多路复用与 SysV/POSIX IPC 段 80 条（select/poll 适配/futex 四操作与竞态再验/SysV shm 键空间·挂接·清扫·泄漏账/SysV sem·msg semop-undo·类型选择接收/POSIX sem/mq/eventfd/timerfd/混合压测场，%d 行）；"
        "finalize 五步断言链逐批全过（155 条正文不足经「边界注记」条款补齐 ≥300 字零残留）；R-C4-001 勘误留痕（B02 骨架批头虚记 5,500→逐条求和真值 5,460，域累计 85,720→85,680）；B16–B40 待领（预算余 154,320 行）"
        % (rows(0, 2), rows(2, 4), rows(4, 8), rows(8, 11), rows(11, 15))
    ),
    "open_bugs": 0,
}
print("B1 handoff domain_ledger_progress 增 C4 块: applied")

# B2. deepen_books 追加 15 册
for bid, frange, theme, chars, rws in BATCHES:
    d["deepen_books"].append(
        "docs/unxreal/deepen/C4-%s.md（20 条新深化，%s %s，正文 %s 字，%s 行锁定零偏离）"
        % (bid, frange, theme, format(chars, ","), format(rws, ","))
    )
print("B2 handoff deepen_books 追加 15 册: applied")

# B3. open_risks 追加 R-C4-002
ids = [r.get("id") for r in d["open_risks"]]
assert "R-C4-002" not in ids, "R-C4-002 已存在，拒绝重复登记"
d["open_risks"].append({
    "id": "R-C4-002",
    "text": ("C4 联签条目九件（F10415/F10419/F10459/F10500/F10520/F10599/F10617/F10658/F10679）已按「注册协议面+原语本体」双线批内显式立条，"
             "与 C2 B29/B30 I 型联调、C3 R-C3-003（C4 pty 挂点交付待冻结）的联测对账随下游联签会话对账（跨界代写禁令：联测判据只写编号+一句话）；"
             "pty 判据主轴（F10486 录制器→F10497 场景账→B07 基线→F10698 总回归）已收官全绿，R-C3-003 待 C3 侧确认销账；R-C4-001 勘误判例已闭环留痕"),
    "owner": "AI-14",
    "level": "P2",
})
print("B3 handoff open_risks 追加 R-C4-002: applied")

# B4. phase 滚动
old_phase_tail = "多域并行不冲突"
assert d["phase"].count(old_phase_tail) == 1
d["phase"] = d["phase"].replace(
    old_phase_tail,
    "C4 B01–B15 深化收口完成 300 条/85,680 行（B16–B40 待领）；多域并行不冲突",
    1,
)
print("B4 handoff phase 滚动: applied")

# B5. next_batch 追加 C4 指针
nb = d["next_batch"]
assert "UNX-C4-B16" not in nb
d["next_batch"] = nb + ("；UNX-C4-B16 待领（AI-14 后续会话按 §6.2 规则二续领，25 批 500 条预算余 154,320 行，"
                        "主题沿 §7.3-C4 引言 B16–B40 段推进：B16–B25 socket 深水区/信号与 IPC 交叉矩阵、B26–B36 E 型正反双判据错误矩阵、B37–B40 C 型收官，以总纲 §7.3-C4 实时状态为准）")
print("B5 handoff next_batch 追加 C4 指针: applied")

# B6. last_session / updated_at
d["last_session"] = (
    "AI-14（CoRun 会话：C4 域 B01–B15 全批深化收口 300 条/85,680 行，finalize 五步断言链全域复跑全过："
    "awk 正文行长口径机械审计暴露 155 条正文不足经「边界注记」技术边界条款逐条补齐 ≥300 字零残留（B10 19/B11 17/B09 14/B15 11/B07 16/B08 9/B06 4/B05 2/B02–B04 各 1）、"
    "B02-F10434 状态与 B04-F10474 域/批行两处格式修复、R-C4-001 勘误留痕——B02 骨架批头虚记 5,500 以逐条求和真值 5,460 修正，域累计链 85,720→85,680 全账链 28 处重算零残留；"
    "深化正文合计 276,932 字；判据主轴 pty 上 vim 类全屏程序+录制对照收官全绿（F10486→F10497→B07 基线→F10698 总回归）；"
    "总纲 C4 表 15 行回填+域小结+修订记录随波08-M08 AI-18 收口提交裹挟入库（HEAD 核验在册）、handoff 增 C4 块/deepen_books 15 册/R-C4-002、根台账 §二/§三/§四/§六 四处同步）"
)
d["updated_at"] = "波08-M10（AI-14 C4 域 B01–B15 全批深化收口 300 条会话；前一版本为波08-M09 AI-06 B1 收口，其全部记录保留）"
print("B6 handoff last_session/updated_at 滚动: applied")

io.open(HANDOFF, "w", encoding="utf-8", newline="").write(
    json.dumps(d, ensure_ascii=False, indent=2) + "\n"
)
print("handoff.json 落盘完成")
print("=== 全部落盘成功 ===")
