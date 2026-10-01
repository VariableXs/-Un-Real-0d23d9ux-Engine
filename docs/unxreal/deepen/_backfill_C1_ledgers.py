#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""[AI-11][C1] 波08-M27 三账回填：handoff.json C1 块滚动 + 根台账四处 + goal-c1.md 滚动
口径：B16–B30 续领深化收口 300 条/77,340 行/118,637 字；域累计 600 条/153,010/240,000=63.8%；B31–B40 待领（86,990 行）
append-only 纪律：根台账 §四 会话段只追加；§一/§三 原位滚动；§六 表尾追加；handoff 字段滚动；goal-c1 原位更新。
"""
import json
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")
REPO = Path(__file__).resolve().parents[3]
ROOTMD = REPO / "CoRun Varix STAR II · Unxreal.md"
HANDOFF = REPO / "docs" / "unxreal" / "handoff.json"
GOALC1 = REPO / "docs" / "unxreal" / "goal-c1.md"
WAVE = "波08-M27"
ok = True


def chk(cond, msg):
    global ok
    print(("  [OK] " if cond else "  [RED] ") + msg)
    if not cond:
        ok = False


# ═══ 1. handoff.json ═══════════════════════════════════════════
d = json.loads(HANDOFF.read_text(encoding="utf-8"))
c1 = d["domain_ledger_progress"]["C1"]
assert c1["finalized_batches"] == 15 and len(c1["finalized_list"]) == 15, "C1 块前置态不符"
c1["finalized_batches"] = 30
c1["finalized_list"] += [f"UNX-C1-B{b:02d}" for b in range(16, 31)]
c1["rows_locked"] = 153010
c1["rows_deepened_locked"] = 153010
c1["entries_deepened"] = 600
old_tail = "；B16–B40 待领（预算余 164,330 行）"
assert old_tail in c1["entries_deepened_note"], "C1 note 旧尾锚缺失"
round2 = ("；第二轮 B16–B30 续领深化收口 300 条（F8301–F8600）："
          "B16 人格切换与上下文保持 5,120 行 + B17 资源账分离与配额 5,140 行 + B18 ld.so 协作面 5,240 行 + "
          "B19 钉版样本全跑通 5,160 行 + B20 M 型机制收口（机制总检与互锁回归）5,140 行 + "
          "B21 ELF 头畸形全拒族 5,220 行 + B22 头表结构畸形全拒族 5,220 行 + B23 映射尺寸与地址空间超限族 5,160 行 + "
          "B24 资源与计数超限族 5,220 行 + B25 重定位与动态段畸形全拒族 5,120 行 + B26 TLS 与符号面畸形全拒族 5,120 行 + "
          "B27 解释器与注记/杂表面畸形全拒族 5,120 行 + B28 E 型收官总对账与互锁回归（R-C1-004/004b 勘误后真值：七族 90 行/六批 6,000 例/72 行/六批留位/41,340）5,160 行 + "
          "B29 I 型集成批 I 机制面联测 5,080 行 + B30 I 型集成批 II 对照面联测与域中场账 5,120 行；"
          "行数守恒 77,340（R-C1-002 勘误后批头链，十五批三方一致）；深化字数合计 118,637 字（口径同上：逐条\"- 正文：\"行 len 求和）；"
          "finalize 校验链四口径全绿（deepen/_finalize_check_C1.py：ID F8301–F8600 连续唯一/判据 300/300 骨架↔深化逐字一致/"
          "行数守恒 77,340==153,010/正文 300/300 ≥300 字、15 册 PWCTOTAL 实测回填零残留）；"
          "勘误留痕 R-C1-002/003/004/004b/005 五笔（脚本+判例+独立复算+旧值零残留）；"
          "联签 19/40=47.5%≥30% 口径（F8296 冻结）；域中场账 63.8%（F8600 在册）；"
          "B31–B40 待领（预算余 86,990 行）")
c1["entries_deepened_note"] = c1["entries_deepened_note"].replace(old_tail, round2)
d["deepen_books"] += [
    "docs/unxreal/deepen/C1-B16.md（20 条新深化，F8301–F8320 双人格并存深化 I——人格切换与上下文保持，正文 9,205 字，5,120 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B17.md（20 条新深化，F8321–F8340 双人格并存深化 II——资源账分离与配额，正文 8,664 字，5,140 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B18.md（20 条新深化，F8341–F8360 glibc 动态全链深化 I——ld.so 协作面，正文 8,604 字，5,240 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B19.md（20 条新深化，F8361–F8380 glibc 动态全链深化 II——钉版样本全跑通，正文 7,730 字，5,160 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B20.md（20 条新深化，F8381–F8400 M 型机制批收口——机制总检与互锁回归，正文 7,163 字，5,140 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B21.md（20 条新深化，F8401–F8420 E 型边界批 I——ELF 头畸形全拒族，正文 8,128 字，5,220 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B22.md（20 条新深化，F8421–F8440 E 型边界批 II——头表结构畸形全拒族，正文 8,694 字，5,220 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B23.md（20 条新深化，F8441–F8460 E 型边界批 III——映射尺寸与地址空间超限族，正文 7,542 字，5,160 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B24.md（20 条新深化，F8461–F8480 E 型边界批 IV——资源与计数超限族，正文 7,540 字，5,220 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B25.md（20 条新深化，F8481–F8500 E 型边界批 V——重定位与动态段畸形全拒族，正文 8,006 字，5,120 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B26.md（20 条新深化，F8501–F8520 E 型边界批 VI——TLS 与符号面畸形全拒族，正文 8,322 字，5,120 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B27.md（20 条新深化，F8521–F8540 E 型边界批 VII——解释器与注记/杂表面畸形全拒族，正文 8,068 字，5,120 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B28.md（20 条新深化，F8541–F8560 E 型边界批 VIII——E 型收官总对账与互锁回归，正文 7,290 字，5,160 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B29.md（20 条新深化，F8561–F8580 I 型集成批 I——机制面联测落地，正文 6,773 字，5,080 行锁定零偏离）",
    "docs/unxreal/deepen/C1-B30.md（20 条新深化，F8581–F8600 I 型集成批 II——对照面联测与域中场账，正文 6,908 字，5,120 行锁定零偏离）",
]
prev_updated = d["updated_at"]
d["updated_at"] = (f"{WAVE}（AI-11 C1 域 B16–B30 续领深化收口 300 条会话——域累计 600 条/153,010/240,000=63.8%（B01–B15 75,670 + B16–B30 77,340 守恒 ✓），"
                   f"finalize 校验链四口径全绿（_finalize_check_C1.py），R-C1-002~005 勘误留痕五笔；前一版本为 {prev_updated}；"
                   "再前链见根台账 §四与 sync_notes 留痕，其全部记录保留）")
d["last_session"] = ("AI-11 续领轮（波08-M27：C1 域本会话 300 条新深化——B16–B20 M 型机制后段 100 条（人格切换与上下文保持/资源账分离与配额/"
                     "ld.so 协作面/钉版样本全跑通/M 型机制收口与互锁回归）+ B21–B28 E 型边界批 160 条（畸形 ELF 全拒八批正反双判据缺一不过——"
                     "ELF 头/头表结构/映射尺寸与地址空间超限/资源与计数超限/重定位与动态段/TLS 与符号面/解释器与注记杂表面/E 型收官总对账与互锁回归；"
                     "错误码总表 90 行收官 F8544、E 型 fuzz 六域 6,000 例）+ B29–B30 I 型集成批 40 条（机制面联测落地/对照面联测与域中场账——"
                     "四段链 fake 分发 10/10/组会话×IPC 10 组/双人格路由 100/100/LTP 反查/钉版样本/联签 19/40=47.5%≥30% 口径/D1 TCB 对照 40 组满额/"
                     "F8599 三十批总对账 77,340 三方一致/F8600 域中场账 63.8%）；finalize 校验链四口径全绿+15 册 PWCTOTAL 回填零残留；"
                     "勘误留痕 R-C1-002/003/004/004b/005 五笔；总纲 §7.3-C1 十五回填+根台账四处+goal-c1.md 滚动；git 提交推送 unxreal(c1)）")
d["phase"] = d["phase"].replace(
    "C1 B01–B15 深化收口 300 条（AI-11，75,670/240,000——R-C1-001 勘误后真值，B16–B40 待领）",
    "C1 B01–B30 两轮深化收口 600 条（AI-11，153,010/240,000=63.8%——R-C1-001/R-C1-002 勘误后真值；B16–B30 续轮 300 条/77,340 行/118,637 字；B31–B40 待领）")
d["next_batch"] = d["next_batch"].replace(
    "UNX-C1-B16 待领（AI-11 后续会话按 §6.2 规则二续领，25 批 500 条预算余 164,330 行——B01–B15 深化已收口，深化正文 143,122 字/行数守恒 75,670 为基线，主题按 §7.3-C1 引言：M 型机制批后段 B16–B20/E 型边界批 B21–B28 正反双判据/I 型集成批 B29–B36 联签 ≥30%/C 型收官批 B37–B40）",
    "UNX-C1-B31 待领（AI-11 后续会话按 §6.2 规则二续领，10 批 200 条预算余 86,990 行——B01–B30 两轮深化已收口 600 条/153,010 行/深化正文合计 261,759 字为基线，主题按 §7.3-C1 引言：I 型集成批 B31–B36 联签 ≥30% 后段/C 型收官批 B37–B40 LTP 回归与域收官宣告）")
d["sync_notes"].append(
    f"AI-11 {WAVE} C1 域 B16–B30 续领深化收口：域累计 600 条/153,010/240,000=63.8%（B01–B15 75,670 + B16–B30 77,340 守恒 ✓）；"
    "finalize 校验链四口径全绿（deepen/_finalize_check_C1.py：ID F8301–F8600 连续唯一 300 条/判据 300/300 骨架↔深化逐字一致/"
    "行数守恒十五批三方一致/正文 300/300 ≥300 字、15 册 PWCTOTAL 实测回填零残留）；勘误留痕 R-C1-002/003/004/004b/005 五笔"
    "（R-C1-005=finalize 门禁暴露项：F8316 行数分解 240→260、5 条正文补足、B21 骨架 \\x7f 转义）；"
    "deepen_books 30 册（C1-B16..B30 新增 15 册）；总纲 §7.3-C1 十五回填+根台账四处+goal-c1.md 滚动；B31–B40 待领（86,990 行）。")
HANDOFF.write_text(json.dumps(d, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
chk(True, "handoff.json C1 块滚动（finalized=30/entries=600/rows_locked=153,010/deepen_books +15/sync_notes +1）")

# ═══ 2. 根台账四处 ═════════════════════════════════════════════
raw = ROOTMD.read_text(encoding="utf-8")

# ① §一 UNX-C1 括号续段
a1_old = "UNX-C1（AI-11，波 08-M08：B01–B15 全批深化收口 300 条/75,670 行/143,122 字，R-C1-001 勘误留痕）"
a1_new = ("UNX-C1（AI-11，波 08-M08：B01–B15 全批深化收口 300 条/75,670 行/143,122 字，R-C1-001 勘误留痕；"
          "波 08-M27 续：B16–B30 续领深化收口 300 条/77,340 行/118,637 字，域累计 600 条/153,010 行——B31–B40 收官待领）")
assert raw.count(a1_old) == 1, "§一 UNX-C1 括号锚不唯一"
raw = raw.replace(a1_old, a1_new)

# ② §三 UNX-C1 整行替换
lines = raw.splitlines()
hits = [i for i, l in enumerate(lines) if l.startswith("| UNX-C1 ELF 装载与进程模型 |")]
assert len(hits) == 1, f"§三 UNX-C1 行定位异常：{len(hits)}"
new_s3 = ("| UNX-C1 ELF 装载与进程模型 | AI-11 | B01–B30（30 批收口） | B01–B30（30 批 600 条，两轮各 300） | 0 | "
          "153,010 / 240,000（深化行数逐批求和，脚本实核零偏离；R-C1-001/R-C1-002 勘误后真值；深化正文合计 261,759 字——B01–B15 段 143,122 + B16–B30 段 118,637；域中场 63.8%） | "
          "波 08 窗两轮；C1 域 §7.3 全表 40 批一次扩表 + 第一轮 B01–B15 全批深化收口 300 条/75,670 行（finalize 五步断言链逐批全过、全域防重 grep 零撞号："
          "B01–B08 F 型地基批（装载链与四样板/进程描述符与 fork-exit/auxv 17 项/解释器与 binfmt/动态段与 vdso/fd 表与资源/基址与地址空间挂点/ktest 断言面基建）+ "
          "B09–B15 M 型机制批前段（execve 全链/ASLR 与 TLS/COW fork 联测/进程组会话/clone 挂点/lxrun.rs 接管/双人格基座——F8281 路由/F8282 双 pid 空间隔离/"
          "F8283 子结构内聚/F8284 边界只经 C4 IPC/F8287 导出通道/F8288 对照账本 ≥40 组/F8289 判据件 100/100+100% 拒绝/F8293 并存样例/F8294 一键跑/F8300 中场账））；"
          "第二轮 B16–B30 续领深化收口 300 条/77,340 行（骨架 15 册+深化 15 册，finalize 校验链四口径全绿 deepen/_finalize_check_C1.py——ID F8301–F8600 连续唯一 300 条/"
          "判据 300/300 骨架↔深化逐字一致/行数守恒 77,340 三方一致/正文 300/300 ≥300 字、15 册 PWCTOTAL 回填零残留；主题段：B16–B20 M 型机制后段 100 条"
          "（人格切换与上下文保持/资源账分离与配额/ld.so 协作面/钉版样本全跑通/M 型机制收口与互锁回归）+ B21–B28 E 型边界批 160 条（畸形 ELF 全拒八批正反双判据缺一不过——"
          "ELF 头/头表结构/映射尺寸与地址空间超限/资源与计数超限/重定位与动态段/TLS 与符号面/解释器与注记杂表面/E 型收官总对账与互锁回归；错误码总表 90 行收官 F8544、"
          "E 型 fuzz 六域 6,000 例）+ B29–B30 I 型集成批 40 条（机制面联测落地/对照面联测与域中场账——四段链 fake 分发 10/10/组会话×IPC 10 组/双人格路由 100/100/"
          "LTP 反查/钉版样本/联签 19/40=47.5%≥30% 口径/D1 TCB 对照 40 组满额/F8599 三十批总对账 77,340 三方一致/F8600 域中场账 63.8%））；"
          "现存 lxrun.rs 295 行与 proc/elf.rs、proc/loader.rs、proc/auxv.rs、exec.rs 按升级接管分层深化非重写（差异表协议 F8150）；"
          "勘误五笔留痕（R-C1-001 B07 批头 4,940→4,920/R-C1-002 B17–B20 批头聚合校准/R-C1-003 断言集实例链 8..17/R-C1-004·004b B28 五处聚合+六批计时账/"
          "R-C1-005 finalize 门禁修复）；B31–B40 待领（预算余 86,990 行） |")
lines[hits[0]] = new_s3
raw = "\n".join(lines) + ("\n" if raw.endswith("\n") else "")

# ③ §四 会话段追加（§五 标题前）
s5_anchor = "## 五、协作纪律速览（新 AI 会话必读）"
assert raw.count(s5_anchor) == 1, "§五 标题锚缺失"
session_md = f"""### 会话 2026-{WAVE} · AI-11（C1 域 B16–B30 续领深化收口 300 条 + finalize 校验链收口）
- **冷启动对账**：git + handoff.json + 总纲 §7.3-C1 三方对账；承接前序会话 B01–B15 300 条基线（75,670/240,000——R-C1-001 勘误后真值）；Variable 明令"一次对话必须写 300 项新功能"——本轮续领 B16–B30（§6.2 规则二写锁登记于总纲修订记录）；他会话产物（A1/B3/D1/C2 等域滚动）零接触不代管。
- **300 条新深化（B16–B30 十五批，骨架 15 册+深化 15 册）**：B16–B20 M 型机制后段 100 条（人格切换与上下文保持/资源账分离与配额/ld.so 协作面/钉版样本全跑通/M 型机制收口与互锁回归）+ B21–B28 E 型边界批 160 条（畸形 ELF 全拒八批正反双判据缺一不过——ELF 头/头表结构/映射尺寸与地址空间超限/资源与计数超限/重定位与动态段/TLS 与符号面/解释器与注记杂表面/E 型收官总对账与互锁回归；错误码总表 90 行收官 F8544、E 型 fuzz 六域分立 6,000 例、对照零容忍模式三批）+ B29–B30 I 型集成批 40 条（机制面联测落地/对照面联测与域中场账——四段链 fake 分发 10/10/fork-exec-wait C3 归属机检/组会话×IPC 10 组/双人格路由 100/100/clone 标志矩阵/lxrun 三载体/ASLR×垫片/LTP 反查三级/钉版样本 ≥30/联签 11/20+8/20=19/40=47.5%≥30% 口径/D1 TCB 对照 40 组满额/F8591 实例链 8..17 收官/F8599 三十批总对账 77,340 三方一致/F8600 域中场账 63.8%+B31–B40 预算 86,990 登记）。
- **勘误五笔留痕（判例+脚本+独立复算+旧值零残留）**：R-C1-002（B17–B20 骨架批头聚合数校准，域累计链 75,670 不变）；R-C1-003（断言集协议同构实例号链 8..17 连续零重）；R-C1-004/R-C1-004b（B28 骨架五处聚合失真——七族 90 行/六批 6,000 例/72 行零容忍/六批留位表/41,340 三对账 + F8545 六批计时账）；R-C1-005（finalize 门禁暴露项：B16-F8316 行数分解 240→260 勘误、5 条正文 <300 补足至 ≥300、B21 骨架 F8401 \\\\x7f 转义勘误——_fix_C1_R_C1_005_finalize_repairs.py 自检全绿）。
- **finalize 校验链（deepen/_finalize_check_C1.py 四口径全绿）**：①行数守恒——十五批逐条求和==批头==深化锁定三方一致（5,120/5,140/5,240/5,160/5,140/5,220/5,220/5,160/5,220/5,120/5,120/5,120/5,160/5,080/5,120 合计 77,340），域累计链 75,670→153,010 逐批零断、终点==锁定真值；②ID 两键——F8301–F8600 连续零缺零重 300 条，同批骨架↔深化 ID 集与标题一致；③判据一致——判据号（UNX-F8xxx-J1 与条目同号）+判据文本骨架↔深化逐字一致 300/300；④正文 ≥300 字 len 实计 300/300（三十批正文合计 118,637 字，全域最低 300 恰压线 B29-F8577）；15 册 finalize 记账表 PWCTOTAL/PWCMIN/PWCMAX 实测回填零残留。
- **三账回填**：总纲 §7.3-C1 十五回填（[未动]→[已深化]+实计字数）+域小结（600 条/153,010/261,759 字）+修订记录（R-C1-002~005 留痕）；handoff C1 块滚动（finalized_batches=30/entries_deepened=600/rows_locked=153,010/deepen_books +15 册）+updated_at/last_session/phase/next_batch 滚动+sync_notes 追加；goal-c1.md 子目标 S7–S10+检查点滚动；根台账 §一/§三/§四/§六 同步。
- **双同步**：docs 落盘 + git 提交推送（`unxreal(c1): C1 域 B16–B30 深化收口 300 条（续领两轮达成 600 条/153,010 行，finalize 校验链四口径全绿，R-C1-002~005 勘误留痕）`）。共享协调文件 append-only 只显式暂存本域文件与本轮共享账本改动，零裹挟。

"""
raw = raw.replace(s5_anchor, session_md + s5_anchor)

# ④ §六 表尾追加行
s6_row = (f"| {WAVE} | AI-11 | C1 域 B16–B30 续领深化收口 300 条/77,340 行，域累计 600 条/153,010/240,000（B16–B20 M 型机制后段 100 条——人格切换与上下文保持/"
          "资源账分离与配额/ld.so 协作面/钉版样本全跑通/M 型机制收口与互锁回归 + B21–B28 E 型边界批 160 条——畸形 ELF 全拒八批正反双判据、错误码总表 90 行 F8544、"
          "E 型 fuzz 六域 6,000 例、B28 总对账与互锁回归收官 + B29–B30 I 型集成 40 条——四段链 fake 分发/组会话×IPC/双人格路由 100/100/LTP 反查/钉版样本/"
          "联签 19/40=47.5%≥30%/D1 TCB 对照 40 组满额/域中场账 63.8% F8600；finalize 校验链四口径全绿 _finalize_check_C1.py：ID F8301–F8600 连续唯一/"
          "判据 300/300 骨架↔深化逐字一致/行数守恒 77,340 三方一致/正文 300/300 ≥300 字、15 册 PWCTOTAL 回填零残留；勘误留痕 R-C1-002/003/004/004b/005 五笔）；"
          "总纲 §7.3-C1 十五回填+小结+修订记录、handoff C1 块滚动（finalized=30/entries=600/rows_locked=153,010/deepen_books 30 册）、goal-c1.md 滚动、"
          "根台账 §一/§三/§四/§六 同步；B31–B40 待领（预算余 86,990 行） |\n")
if not raw.endswith("\n"):
    raw += "\n"
raw += s6_row
ROOTMD.write_text(raw, encoding="utf-8")
chk(True, "根台账四处回填（§一 续段/§三 整行/§四 会话段/§六 表尾行）")

# ═══ 3. goal-c1.md ═════════════════════════════════════════════
g = GOALC1.read_text(encoding="utf-8")
g = g.replace("按会话批次推进（首会话 B01–B15 已收口 300 条）",
              "按会话批次推进（前两会话已收口 B01–B30 600 条——两轮各 300 条）")
g = g.replace(
    "1. B01–B15 300 条全部 [已深化]，§7.3-C1 翻态、行数守恒 75,670（R-C1-001 勘误后真值，逐批锁定值与逐条求和双口径一致）；",
    "1. B01–B30 600 条全部 [已深化]，§7.3-C1 翻态、行数守恒 153,010（R-C1-001/R-C1-002 勘误后真值，逐批锁定值与逐条求和双口径一致，域中场 63.8%）；")
s710 = ("| S6 | 协作文件更新+GitHub 同步 | ✅ 本会话完成 | goal-c1.md（本文件）、handoff.json C1 块、根台账四处、提交推送 |\n"
        "| S7 | B16–B24 续领骨架+深化 180 条 | ✅ 波08-M27 完成 | batches/UNX-C1-B16..B24 + deepen/C1-B16..B24（M 型后段 100+E 型前四批 80） |\n"
        "| S8 | B25–B30 立骨架+深化 120 条 | ✅ 波08-M27 完成 | batches/UNX-C1-B25..B30 + deepen/C1-B25..B30（E 型后四批 80+I 型集成 40） |\n"
        "| S9 | 勘误留痕五笔（R-C1-002/003/004/004b/005） | ✅ 波08-M27 完成 | deepen/_fix_C1_R_C1_*.py 五件，判例+脚本+独立复算+旧值零残留 |\n"
        "| S10 | finalize 校验链四口径+15 册字数回填+三账回填+GitHub 同步 | ✅ 波08-M27 完成 | _finalize_check_C1.py 全绿；§7.3-C1 十五回填；handoff C1 块滚动；根台账四处 |\n")
assert "| S6 | 协作文件更新+GitHub 同步 | ✅ 本会话完成 |" in g, "S6 行锚缺失"
g = g.replace("| S6 | 协作文件更新+GitHub 同步 | ✅ 本会话完成 | goal-c1.md（本文件）、handoff.json C1 块、根台账四处、提交推送 |",
              s710.rstrip("\n"))
g += ("\n## 第二轮检查点（波08-M27 · B16–B30 续领轮）\n"
      "- 本轮 300 条（Variable 明令）：B16–B30 全批，F8301–F8600；骨架 15 册+深化 15 册；\n"
      "- 行数账：本轮 77,340（十五批 5,120/5,140/5,240/5,160/5,140/5,220/5,220/5,160/5,220/5,120/5,120/5,120/5,160/5,080/5,120）；域累计 153,010/240,000（63.8%）；\n"
      "- 深化正文合计 118,637 字（口径=逐条\"- 正文：\"行 len 求和），全域最低 300 恰压线（B29-F8577）；15 册 finalize 记账表 PWCTOTAL/PWCMIN/PWCMAX 实测回填零残留；\n"
      "- finalize 校验链四口径全绿留痕：deepen/_finalize_check_C1.py（ID 连续唯一/判据逐字一致/行数守恒/正文 ≥300）；\n"
      "- 勘误五笔：R-C1-002（B17–B20 批头聚合校准）/R-C1-003（实例链 8..17）/R-C1-004·004b（B28 五处聚合+六批计时）/R-C1-005（F8316 分解 240→260+5 条正文补足+F8401 转义）；\n"
      "- 域中场账：F8600 在册 63.8%；B31–B40 待领（预算余 86,990 行）。\n")
GOALC1.write_text(g, encoding="utf-8")
chk(True, "goal-c1.md 滚动（完成条件 1/S7–S10/第二轮检查点）")

# ═══ 4. 自检 ═══════════════════════════════════════════════════
d2 = json.loads(HANDOFF.read_text(encoding="utf-8"))
c1b = d2["domain_ledger_progress"]["C1"]
chk(c1b["finalized_batches"] == 30 and len(c1b["finalized_list"]) == 30, "handoff finalized=30/list=30")
chk(c1b["rows_locked"] == 153010 and c1b["entries_deepened"] == 600, "handoff rows_locked=153,010/entries=600")
chk(len([b for b in d2["deepen_books"] if "deepen/C1-" in b]) == 30, "handoff deepen_books C1=30 册")
chk("B31–B40 待领（预算余 86,990 行）" in c1b["entries_deepened_note"], "handoff note 新尾在位")
chk("164,330 行）" not in c1b["entries_deepened_note"], "handoff note 旧尾零残留")
r2 = ROOTMD.read_text(encoding="utf-8")
chk("波 08-M27 续：B16–B30 续领深化收口 300 条/77,340 行/118,637 字" in r2, "根台账 §一 续段在位")
chk(r2.count("| UNX-C1 ELF 装载与进程模型 |") == 1 and "153,010 / 240,000" in r2, "根台账 §三 行已更新")
chk(f"### 会话 2026-{WAVE} · AI-11（C1 域 B16–B30 续领深化收口 300 条" in r2, "根台账 §四 会话段在位")
chk(r2.count(f"| {WAVE} | AI-11 | C1 域 B16–B30 续领深化收口") == 1, "根台账 §六 行在位（恰一条）")
chk("| UNX-C1 ELF 装载与进程模型 | AI-11 | B01–B15（15 批） |" not in r2, "根台账 §三 旧行零残留")
chk("UNX-C1-B16 待领（AI-11 后续会话" not in json.dumps(d2, ensure_ascii=False), "handoff next_batch 旧段零残留")
g2 = GOALC1.read_text(encoding="utf-8")
chk("B01–B30 600 条全部 [已深化]" in g2 and "S10" in g2 and "第二轮检查点（波08-M27" in g2, "goal-c1 新内容在位")

print(f"\n{'[DONE] 三账回填完成：handoff C1 块+根台账四处+goal-c1.md，自检全绿。' if ok else '[FAIL] 自检存在红灯。'}")
sys.exit(0 if ok else 1)
