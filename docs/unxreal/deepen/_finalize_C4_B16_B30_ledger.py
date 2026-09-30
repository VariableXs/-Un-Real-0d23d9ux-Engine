# -*- coding: utf-8 -*-
"""AI-14 波08-M19 finalize 收口：B16–B30 台账三处回填（根台账四处/总纲三处/handoff 五处）
沿 _finalize_C4_root_ledger.py 先例：锚点唯一性断言 + append-only，防盲替换。"""
import io
import json
import re

ROOT_LEDGER = "CoRun Varix STAR II · Unxreal.md"
ZONGGANG = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal · 总纲与施工书.md"
HANDOFF = "docs/unxreal/handoff.json"

BATCHES = [
    ("B16", "F10701–F10720", "unix socket 连接建立深水：backlog/非阻塞 connect/SO_ERROR/竞态", 19068, 6500),
    ("B17", "F10721–F10740", "unix socket 地址与关闭语义：bind 路径/accept 继承/close×linger", 17961, 6400),
    ("B18", "F10741–F10760", "SOL_SOCKET 选项族全语义：读写链/双倍记账/KEEPALIVE/并发竞态", 18662, 6300),
    ("B19", "F10761–F10780", "epoll 深水：ADD-MOD-DEL 幂等/LT-ET/ONESHOT/fork 共享/嵌套环", 17625, 6400),
    ("B20", "F10781–F10800", "M 型机制总收口：交互总矩阵/全机制压测场/冻结件清单/判据全量重放", 16313, 6400),
    ("B21", "F10801–F10820", "E 型·管道 FIFO 错误矩阵：EPIPE 双判据/SIGPIPE 三态/PIPE_BUF/竞态窗", 16486, 6900),
    ("B22", "F10821–F10840", "E 型·信号错误矩阵：不可捕获/kill 三错误/不排队合并/SA_RESTART 总矩阵", 16453, 6800),
    ("B23", "F10841–F10860", "E 型·pty/termios 错误矩阵：EIO 分账/VMIN-VTIME 四格/控制终端", 16307, 6900),
    ("B24", "F10861–F10880", "E 型·unix socket 错误矩阵一：ECONNREFUSED 终态冻结/SCM_RIGHTS/判定总序表冻结件", 17324, 6900),
    ("B25", "F10881–F10900", "E 型·unix socket 错误矩阵二：backlog 溢出冻结/SCMR 深水环/序表挂接核销", 16484, 7000),
    ("B26", "F10901–F10920", "E 型·复用器错误矩阵：epoll_ctl 四码/五类型混注/三系交叉对照", 16007, 6800),
    ("B27", "F10921–F10940", "E 型·SysV IPC 错误矩阵：msgget 四错误/SEM_UNDO 回滚/SHM_RMID 延迟回收", 15932, 6900),
    ("B28", "F10941–F10960", "E 型·POSIX IPC 错误矩阵：mq 五错误/sem EOVERFLOW/能力表增类 ADR", 16181, 6800),
    ("B29", "F10961–F10980", "I 型·vim 全屏联测：pty+termios+信号+epoll 全链/背压/风暴韧性", 14887, 7200),
    ("B30", "F10981–F11000", "I 型二段·多进程 IPC 压测场：四机制混合/批量 kill/域 300 条总收口", 15366, 7300),
]
rows_sum = sum(b[4] for b in BATCHES)
chars_sum = sum(b[3] for b in BATCHES)
assert rows_sum == 101500, "行数求和 != 101,500: %d" % rows_sum
assert chars_sum == 251056, "字数求和 != 251,056: %d" % chars_sum
print("预算验算：行数 %d = 101,500 ✓；字数合计 %d = 251,056 ✓" % (rows_sum, chars_sum))

# ============ A. 根台账四处 ============
text = io.open(ROOT_LEDGER, encoding="utf-8").read()
n_ops = 0

# A1. §二 列表行 UNX-C4 描述滚动
old_a1 = "UNX-C4（AI-14，波 08：B01–B15 全批深化收口 300 条/85,680 行，R-C4-001 勘误留痕）"
new_a1 = ("UNX-C4（AI-14，波 08：B01–B15 全批深化收口 300 条/85,680 行，R-C4-001 勘误留痕；"
          "波08-M19 续轮：B16–B30 续领深化收口 300 条/101,500 行，域累计 600 条/187,180 行——B31–B40 收官待领）")
assert text.count(old_a1) == 1, "A1 锚点不唯一: %d" % text.count(old_a1)
text = text.replace(old_a1, new_a1, 1)
n_ops += 1
print("A1 §二 列表行 UNX-C4 描述滚动: applied")

# A2. §三 进度表 UNX-C4 行整行替换
m_a2 = re.search(r"\| UNX-C4 IPC 与 Unix 语义 \| AI-14 \|[^\n]*", text)
assert m_a2, "A2 未找到 UNX-C4 进度行"
row_a2 = ("| UNX-C4 IPC 与 Unix 语义 | AI-14 | B01–B30（30 批收口） | B01–B30（30 批 600 条） | 0 "
          "| 187,180 / 240,000（深化行数逐批求和，脚本实核零偏离；B01–B15 85,680 + B16–B30 101,500；"
          "R-C4-001 账实修正：B02 骨架批头虚记 5,500→逐条求和真值 5,460，域累计链 85,720→85,680 全线重算零残留） "
          "| 波 08 窗；首轮 B01–B15 300 条（管道/信号/pty 作业控制/unix socket 三型/SCM_RIGHTS/epoll 主体/"
          "select-poll/futex/SysV 三件/POSIX sem·mq/eventfd·timerfd/混合压测场）"
          "+ 续轮 B16–B30 300 条（B16–B20 M 型 socket 连接深水/SOL_SOCKET 选项族/epoll 深水/M 型总收口 100 条 32,000 行 "
          "+ B21–B28 E 型正反双判据错误矩阵八批 160 条 54,800 行——管道 FIFO/信号/pty termios/unix socket 两批/复用器/SysV/POSIX "
          "+ B29–B30 I 型集成 40 条 14,500 行——vim 全屏联测/多进程 IPC 压测场·域 300 条总收口）；"
          "finalize 五步断言链全域复跑 13 项全过（防重四范围 300 条唯一/判据三成分 300 条双册同步/行数守恒 101,500 总对平/"
          "四项齐备 300 条正文 ≥300 字；R-C4-003 检查器口径勘误留痕——5 位 ID 正则/E2BIG 数字抽取/判据边界三处检查器缺陷修正后复跑，数据零缺陷）；"
          "B31–B40 待领（200 条收官段，账余 52,820 行） |")
text = text[:m_a2.start()] + row_a2 + text[m_a2.end():]
n_ops += 1
print("A2 §三 进度表 UNX-C4 行整行替换: applied")

# A3. §四 会话日志追加 AI-14 波08-M19 会话块（「## 五、协作纪律速览」前）
anchor_a3 = "## 五、协作纪律速览（新 AI 会话必读）"
assert text.count(anchor_a3) == 1, "A3 锚点不唯一: %d" % text.count(anchor_a3)
block_a3 = """### 会话 2026-波08-M19 · AI-14（C4 域 B16–B30 续领深化收口 300 条）
- **冷启动对账**：git + handoff.json + 总纲 §7.3-C4 三方交叉一致：B16–B30 [骨架] 写锁在案（波08-M10 续领批注，预算 101,500 行）、B31–B40 [未动] 待领；承接 Variable「一次对话必须写 300 项新功能」明令第二批（B16–B30），本会话执行 B17 修复 + B18–B30 十三批全新深化 + finalize 五步断言链全域复跑 + 台账三处回填 + 双同步；他会话产物（A2/A1/C3/C5 等域新批册）不越权代管不代提交。
- **产出账**：B17 F10740 正文 294→309 字修复收口；B18–B30 十三批 260 条两件套全新落盘（深化册合计 233,075 字）；域累计 85,680→187,180/240,000（B16–B30 段 101,500 行逐批零偏离）。三段封存：M 型机制段 B16–B20（32,000 行）/E 型错误矩阵段 B21–B28（54,800 行）/I 型集成段 B29–B30（14,500 行，F11000 域 300 条总收口断言在册）。
- **预审计提效法**（B26 八连败教训固化）：写数据→python 逐条「- 正文：」整行预审计→一次补齐→统一执行→awk 终审；B28 两条/B29 十四条/B30 五条全部预审计归零，全域 300 条正文 ≥300 字零残留。
- **R-C4-003 检查器口径勘误留痕**：finalize 首轮三处 FAIL 全为检查脚本口径缺陷而非数据缺陷——①C4 段 5 位 ID 误用 B3 域 \\d{4} 正则（防重/判据两处检查空转）；②fz 分项数字抽取误计 E2BIG 内嵌数字「2」（140+120+90 实=350）；③判据边界 [^｜] 缺失致行数锁定行误判。修正口径后 13/13 全过，数据零缺陷。判定纪律：先证伪检查器再动手改数据（与 R-C4-001「逐条求和为唯一真值」同源判例）。
- **finalize 五步断言链全域复跑 13 项全绿**：①防重四范围（深化册/批册 300 条唯一、跨域零侵入、C4 全域 600 ID 表头级唯一）②判据三成分 300/300 双册同步 ③行数守恒 15 批逐批=批头声明=预算，合计 101,500，域累计 187,180/240,000 ④台账回填（总纲 C4 表 15 行+域小结+修订记录；根台账 §二/§三/§四/§六；handoff C4 块滚动——本块）⑤四项齐备（正文 ≥300 字 300 条+六要素 300 条+可运行判据+行数锁定行 300 条）。
- **open_risks**：R-C4-002 维持（联签条目九件对账随下游联签会话）；R-C4-001/R-C4-003 勘误判例均已闭环留痕；B31–B40 收官预算 52,820 行（B31–B36 I 型后段各 5,330 + B37–B40 C 型收官各 5,210）在册待领。
- **双同步**：docs 落盘 + git 提交推送（`unxreal(c4): B16–B30 骨架/深化 300条（AI-14 域 UNX-C4 · 187,180/240,000 行 · B31–B40 待领）`）。共享协调文件（根台账/handoff）按 append-only 性质提交并登记背景；他会话产物均不纳入，不越权代提交。

"""
text = text.replace(anchor_a3, block_a3 + anchor_a3, 1)
n_ops += 1
print("A3 §四 会话日志 AI-14 波08-M19 会话块追加: applied")

# A4. §六 修订记录行追加（文件末尾表行后）
line_a4 = ("| 波08-M19 | AI-14 | C4 域 B16–B30 续领深化收口 300 条/101,500 行，域累计 600 条/187,180/240,000"
           "（B16–B20 M 型 socket 深水/选项族/epoll 深水/M 型总收口 100 条+B21–B28 E 型正反双判据错误矩阵八批 160 条"
           "+B29–B30 I 型集成 vim 全屏联测/多进程 IPC 压测场 40 条·域 300 条总收口；finalize 五步断言链全域复跑 13 项全过，"
           "R-C4-003 检查器口径勘误留痕——数据零缺陷，深化正文合计 251,056 字）；"
           "总纲 C4 表 15 行回填+域小结+修订记录、根台账 §二/§三/§四/§六 同步、handoff C4 块滚动 |")
if not text.endswith("\n"):
    text += "\n"
text += line_a4 + "\n"
n_ops += 1
print("A4 §六 修订记录行追加: applied")

io.open(ROOT_LEDGER, "w", encoding="utf-8", newline="").write(text)
print("根台账落盘完成，共 %d 处" % n_ops)

# ============ B. 总纲三处 ============
zg = io.open(ZONGGANG, encoding="utf-8").read()
n_ops_b = 0

# B1. C4 批表 B16–B30 十五行 [骨架]→[已深化]
for bid, frange, theme, chars, rws in BATCHES:
    old_row = "| UNX-C4-%s | %s | 20 | [骨架] | —（骨架 %s 行） | AI-14 |" % (bid, frange, format(rws, ","))
    new_row = ("| UNX-C4-%s | %s | 20 | [已深化] | 正文 %s 字（wc -m 实计）· %s 行锁定零偏离（%s） "
               "| AI-14（finalize 断言链五步全过，见 deepen/C4-%s.md） |"
               % (bid, frange, format(chars, ","), format(rws, ","), theme, bid))
    assert zg.count(old_row) == 1, "B1 %s 锚点不唯一: %d" % (bid, zg.count(old_row))
    zg = zg.replace(old_row, new_row, 1)
    n_ops_b += 1
print("B1 总纲 C4 批表 15 行 [骨架]→[已深化]: applied")

# B2. 域 C4 台账小结整行替换
m_b2 = re.search(r"\*\*域 C4 台账小结\*\*[^\n]*", zg)
assert m_b2, "B2 未找到域 C4 台账小结"
new_b2 = ("**域 C4 台账小结**（AI-14 波08-M19 收口时点）：40 批 · 800 条 · 域账 240,000 行。"
          "B01–B30 已深化 600 条/187,180 行（AI-14 承接，Variable 明令「一次对话必须写 300 项新功能」两批全量达成——"
          "首轮 B01–B15 300 条/85,680 行，续轮 B16–B30 300 条/101,500 行：B16–B20 M 型 socket 连接深水/SOL_SOCKET 选项族/"
          "epoll 深水/M 型总收口 + B21–B28 E 型正反双判据错误矩阵八批（管道 FIFO/信号/pty termios/unix socket 两批/复用器/SysV/POSIX）"
          "+ B29–B30 I 型集成（vim 全屏联测/多进程 IPC 压测场·F11000 域 300 条总收口断言），30 批两件套齐备，"
          "finalize 五步断言链逐批全过（R-C4-001 勘误留痕：B02 骨架批头虚记 5,500→逐条真值 5,460；"
          "R-C4-003 检查器口径勘误留痕：5 位 ID 正则/E2BIG 数字抽取/判据边界三处检查器缺陷修正后 13/13 全过，数据零缺陷），"
          "三态/字数/行数已回填上表）· B31–B40 待领（200 条收官段承接，账余 52,820 行）。"
          "本域无引导/数据安全红线条目；行为红线十条全量适用（ABI 细节禁凭记忆注出处、压测数据只记实测）。"
          "上游 C2 挂号注册协议（futex/epoll/SysV IPC 族经分发表注册）为联签前置，联签条目（F10415/F10419/F10459/F10500/F10520/"
          "F10599/F10617/F10658/F10679）在各批内显式立条，联测对账随 C2/C3（R-C4-002）；"
          "判据主轴（pty 上 vim 类全屏程序 + 录制对照：F10486 录制器→F10497 场景账→B07 基线→F10698 总回归→B29 vim 全屏联测段）收官全绿。")
zg = zg[:m_b2.start()] + new_b2 + zg[m_b2.end():]
n_ops_b += 1
print("B2 域 C4 台账小结整行替换: applied")

# B3. 修订记录新行追加（既有 AI-14 深化收口会话修订行后）
m_b3 = re.search(r"> 修订记录（AI-14 深化收口会话）：[^\n]*", zg)
assert m_b3, "B3 未找到 C4 既有修订记录行"
line_b3 = ("> 修订记录（AI-14 续领收口会话·波08-M19）：B16–B30 十五行 [骨架]→[已深化] 回填"
           "（深化正文合计 251,056 字 wc -m 实计；行数锁定 101,500 逐批零偏离，域累计 187,180/240,000）；"
           "finalize 五步断言链全域复跑 13 项全过（①防重四范围：深化册/批册 300 条唯一、跨域零侵入、C4 全域 600 ID 表头级唯一；"
           "②判据三成分 300 条双册同步；③行数守恒 15 批零偏离 101,500 总对平；④台账回填本表+根台账四处+handoff 滚动；"
           "⑤四项齐备 300 条）；R-C4-003 检查器口径勘误留痕（C4 段 5 位 ID 误用 4 位正则/E2BIG 内嵌数字误计/判据边界缺失"
           "三处检查器缺陷修正后复跑，数据零缺陷——判定纪律：先证伪检查器再改数据）；"
           "B31–B40 仍 [未动] 待领（200 条收官段，账余 52,820 行）。")
zg = zg[:m_b3.end()] + "\n" + line_b3 + zg[m_b3.end():]
n_ops_b += 1
print("B3 总纲 C4 修订记录行追加: applied")

io.open(ZONGGANG, "w", encoding="utf-8", newline="").write(zg)
print("总纲落盘完成，共 %d 处" % n_ops_b)

# ============ C. handoff.json 五处 ============
d = json.load(io.open(HANDOFF, encoding="utf-8"))

# C1. domain_ledger_progress.C4 替换
seg_rows = lambda a, b: sum(x[4] for x in BATCHES[a:b])
d["domain_ledger_progress"]["C4"] = {
    "finalized_batches": 30,
    "finalized_list": ["UNX-C4-%s" % b[0] for b in
                       [("B%02d" % i, ) for i in range(1, 31)]],
    "skeleton_batches": 0,
    "rows_locked": 187180,
    "rows_budget": 240000,
    "rows_deepened_locked": 187180,
    "entries_skeleton": 0,
    "entries_deepened": 600,
    "entries_deepened_note": (
        "首轮 B01–B15 300 条/85,680 行（管道/信号/pty 作业控制/unix socket 三型/SCM_RIGHTS/epoll 主体/select-poll/futex/"
        "SysV 三件/POSIX sem·mq/eventfd·timerfd/混合压测场）"
        "+ 续轮 B16–B30 300 条/101,500 行：B16–B20 M 型段 100 条（socket 连接深水/地址与关闭/SOL_SOCKET 选项族/epoll 深水/"
        "M 型总收口，%d 行）+ B21–B28 E 型段 160 条（管道 FIFO/信号/pty termios/unix socket 两批/复用器/SysV/POSIX 八批正反双判据错误矩阵，%d 行）"
        "+ B29–B30 I 型段 40 条（vim 全屏联测/多进程 IPC 压测场·F11000 域 300 条总收口断言，%d 行）；"
        "finalize 五步断言链全域复跑 13 项全过（预审计提效法固化：zw 整行预审→一次补齐→统一执行）；"
        "R-C4-003 检查器口径勘误留痕（5 位 ID 正则/E2BIG 数字抽取/判据边界三处检查器缺陷修正后复跑，数据零缺陷）；"
        "B31–B40 待领（200 条收官段，账余 52,820 行：B31–B36 I 型后段各 5,330 + B37–B40 C 型收官各 5,210）"
        % (seg_rows(0, 5), seg_rows(5, 13), seg_rows(13, 15))
    ),
    "open_bugs": 0,
}
print("C1 handoff domain_ledger_progress.C4 滚动 30 批/600 条/187,180: applied")

# C2. deepen_books 追加 15 册
existing = [b for b in d["deepen_books"] if "C4-B" in b]
assert len(existing) == 15, "C4 既有 %d 册，期望 15" % len(existing)
for bid, frange, theme, chars, rws in BATCHES:
    d["deepen_books"].append(
        "docs/unxreal/deepen/C4-%s.md（20 条新深化，%s %s，正文 %s 字，%s 行锁定零偏离）"
        % (bid, frange, theme, format(chars, ","), format(rws, ","))
    )
print("C2 handoff deepen_books 追加 15 册（B16–B30）: applied")

# C3. phase 中 C4 段滚动
old_c3 = "C4 B01–B15 深化收口完成 300 条/85,680 行（B16–B40 待领）"
new_c3 = ("C4 B01–B30 深化收口完成 600 条/187,180 行（B16–B30 续轮 300 条/101,500 行：M 型 32,000+E 型 54,800+I 型 14,500；"
          "B31–B40 待领 52,820 行）")
assert d["phase"].count(old_c3) == 1, "C3 锚点不唯一: %d" % d["phase"].count(old_c3)
d["phase"] = d["phase"].replace(old_c3, new_c3, 1)
print("C3 handoff phase C4 段滚动: applied")

# C4. next_batch 中 UNX-C4-B16 指针替换为 B31
m_c4 = re.search(r"UNX-C4-B16 待领（[^）]*）", d["next_batch"])
assert m_c4, "C4 未找到 UNX-C4-B16 指针"
new_c4 = ("UNX-C4-B31 待领（AI-14 后续会话按 §6.2 规则二续领，10 批 200 条收官预算 52,820 行——"
          "B01–B30 深化已收口 600 条/187,180 行，B31–B36 I 型后段各 5,330/B37–B40 C 型收官各 5,210，"
          "主题以总纲 §7.3-C4 实时状态为准）")
d["next_batch"] = d["next_batch"][:m_c4.start()] + new_c4 + d["next_batch"][m_c4.end():]
print("C4 handoff next_batch UNX-C4-B31 指针: applied")

# C5. last_session / updated_at 滚动（前版本注记链保留）
d["last_session"] = (
    "AI-14（波08-M19：C4 域 B16–B30 续领深化收口 300 条/101,500 行，域累计 600 条/187,180/240,000——"
    "B17 F10740 正文修复+B18–B30 十三批两件套全新落盘+finalize 五步断言链全域复跑 13 项全过+"
    "R-C4-003 检查器口径勘误留痕（5 位 ID 正则/E2BIG 数字抽取/判据边界三处检查器缺陷，数据零缺陷）+"
    "台账三处回填（总纲 C4 表 15 行/根台账四处/handoff 滚动）+双同步 git 提交推送）；"
    "前一版本为波08-M18 AI-02（A2 B16–B30 续领收口）与波08-M17 AI-15（C5 维护轮），其全部记录保留"
)
d["updated_at"] = "波08-M19（AI-14 C4 域 B16–B30 续领深化收口 300 条会话；前一版本为波08-M18（AI-02 A2 续领收口）/波08-M17（AI-15 C5 维护轮），其全部记录保留）"
print("C5 handoff last_session/updated_at 滚动: applied")

io.open(HANDOFF, "w", encoding="utf-8", newline="").write(
    json.dumps(d, ensure_ascii=False, indent=2) + "\n"
)
print("handoff.json 落盘完成")
print("=== 全部落盘成功 ===")
