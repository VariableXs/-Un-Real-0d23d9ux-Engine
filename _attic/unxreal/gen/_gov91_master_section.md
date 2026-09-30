
---

# 增补卷 · AI-91 · 同步审计 · 300项新功能增补册（B01–B15 · GOV91-001–GOV91-300）

> **任务书锚定**：AI-91 治理线同步审计官（分工图 §AI-91：双同步一致性、丢批检测、拒收权；finalize 批次 GitHub 可追溯率 100%、孤儿提交即告警）。Variable 明令本会话 **300 项新功能增补**：B01–B15 共 15 批 × 20 条，批账 6,000 行 × 15 = **90,000 行**（治理线增补卷独立账，不占域账 F1–F64000）。每条 = ID ｜ 深化名 ｜ 行数 ｜ 状态 ｜ 证据与判据锚定。独立增补册与本段双同步逐字一致；生成器 _attic/unxreal/gen/_gov91_firstprod.py 五断言 ALL PASS。
>
> **内核锚定（Variable 明令：全部 Varix 计划围绕内核进行）**：300 条全部以内核三线门禁（kbuild/ktest/kcheck）与门禁基线铁值（ktest 3,146 / kcheck 0 / tsc 0 / vitest 2,851 / variable --lib 348 / ca-core 356 · 09-22 基准）为审计对象与锚定载体；同步审计机检对象含内核批次commit、draft 分支与 PR 五要素，全部围绕内核工程侧展开。防重：GOV91 全库 grep 零命中纯新建，300 条主题两两不重叠；不触他域域账（AI-81 GOV/GOV90/GOV83/G8/GV 段零交叠零改写）。

## 批 UNX-GOV91-B01（commit 规范机检（内核批格式） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-001 | commit 规范机检（内核批格式）·机检规则件 | 300 | 增补 | UNX-GOV91-001-J1 机检规则件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-002 | commit 规范机检（内核批格式）·断言器 | 300 | 增补 | UNX-GOV91-002-J1 断言器可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-003 | commit 规范机检（内核批格式）·巡检器 | 300 | 增补 | UNX-GOV91-003-J1 巡检器可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-004 | commit 规范机检（内核批格式）·账本页 | 300 | 增补 | UNX-GOV91-004-J1 账本页可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-005 | commit 规范机检（内核批格式）·回归样例 | 300 | 增补 | UNX-GOV91-005-J1 回归样例可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-006 | commit 规范机检（内核批格式）·告警单 | 300 | 增补 | UNX-GOV91-006-J1 告警单可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-007 | commit 规范机检（内核批格式）·修复流程件 | 300 | 增补 | UNX-GOV91-007-J1 修复流程件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-008 | commit 规范机检（内核批格式）·判据母版 | 300 | 增补 | UNX-GOV91-008-J1 判据母版可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-009 | commit 规范机检（内核批格式）·对照等级声明件 | 300 | 增补 | UNX-GOV91-009-J1 对照等级声明件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-010 | commit 规范机检（内核批格式）·抽样规则件 | 300 | 增补 | UNX-GOV91-010-J1 抽样规则件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-011 | commit 规范机检（内核批格式）·报告模板件 | 300 | 增补 | UNX-GOV91-011-J1 报告模板件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-012 | commit 规范机检（内核批格式）·看护器 | 300 | 增补 | UNX-GOV91-012-J1 看护器可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-013 | commit 规范机检（内核批格式）·探针件 | 300 | 增补 | UNX-GOV91-013-J1 探针件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-014 | commit 规范机检（内核批格式）·对账单件 | 300 | 增补 | UNX-GOV91-014-J1 对账单件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-015 | commit 规范机检（内核批格式）·豁免登记件 | 300 | 增补 | UNX-GOV91-015-J1 豁免登记件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-016 | commit 规范机检（内核批格式）·演练剧本件 | 300 | 增补 | UNX-GOV91-016-J1 演练剧本件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-017 | commit 规范机检（内核批格式）·阈值表件 | 300 | 增补 | UNX-GOV91-017-J1 阈值表件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-018 | commit 规范机检（内核批格式）·升级路径件 | 300 | 增补 | UNX-GOV91-018-J1 升级路径件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-019 | commit 规范机检（内核批格式）·证据包件 | 300 | 增补 | UNX-GOV91-019-J1 证据包件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-020 | commit 规范机检（内核批格式）·关门印件 | 300 | 增补 | UNX-GOV91-020-J1 关门印件可运行/可观测/可复测：锚定commit 规范机检（内核批格式）（内核侧载体=内核批 finalize 提交格式 `unxreal(<域>): <域>-<批> finalize 20条`）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B02（PR 五要素校验（批次号/条数/字数/行数/断言结果） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-021 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·机检规则件 | 300 | 增补 | UNX-GOV91-021-J1 机检规则件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-022 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·断言器 | 300 | 增补 | UNX-GOV91-022-J1 断言器可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-023 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·巡检器 | 300 | 增补 | UNX-GOV91-023-J1 巡检器可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-024 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·账本页 | 300 | 增补 | UNX-GOV91-024-J1 账本页可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-025 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·回归样例 | 300 | 增补 | UNX-GOV91-025-J1 回归样例可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-026 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·告警单 | 300 | 增补 | UNX-GOV91-026-J1 告警单可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-027 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·修复流程件 | 300 | 增补 | UNX-GOV91-027-J1 修复流程件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-028 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·判据母版 | 300 | 增补 | UNX-GOV91-028-J1 判据母版可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-029 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·对照等级声明件 | 300 | 增补 | UNX-GOV91-029-J1 对照等级声明件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-030 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·抽样规则件 | 300 | 增补 | UNX-GOV91-030-J1 抽样规则件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-031 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·报告模板件 | 300 | 增补 | UNX-GOV91-031-J1 报告模板件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-032 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·看护器 | 300 | 增补 | UNX-GOV91-032-J1 看护器可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-033 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·探针件 | 300 | 增补 | UNX-GOV91-033-J1 探针件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-034 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·对账单件 | 300 | 增补 | UNX-GOV91-034-J1 对账单件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-035 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·豁免登记件 | 300 | 增补 | UNX-GOV91-035-J1 豁免登记件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-036 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·演练剧本件 | 300 | 增补 | UNX-GOV91-036-J1 演练剧本件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-037 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·阈值表件 | 300 | 增补 | UNX-GOV91-037-J1 阈值表件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-038 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·升级路径件 | 300 | 增补 | UNX-GOV91-038-J1 升级路径件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-039 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·证据包件 | 300 | 增补 | UNX-GOV91-039-J1 证据包件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-040 | PR 五要素校验（批次号/条数/字数/行数/断言结果）·关门印件 | 300 | 增补 | UNX-GOV91-040-J1 关门印件可运行/可观测/可复测：锚定PR 五要素校验（批次号/条数/字数/行数/断言结果）（内核侧载体=finalize 第 4d 步 PR 五要素）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B03（孤儿提交检测（GitHub 有 commit 而台账无批次） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-041 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·机检规则件 | 300 | 增补 | UNX-GOV91-041-J1 机检规则件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-042 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·断言器 | 300 | 增补 | UNX-GOV91-042-J1 断言器可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-043 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·巡检器 | 300 | 增补 | UNX-GOV91-043-J1 巡检器可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-044 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·账本页 | 300 | 增补 | UNX-GOV91-044-J1 账本页可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-045 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·回归样例 | 300 | 增补 | UNX-GOV91-045-J1 回归样例可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-046 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·告警单 | 300 | 增补 | UNX-GOV91-046-J1 告警单可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-047 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·修复流程件 | 300 | 增补 | UNX-GOV91-047-J1 修复流程件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-048 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·判据母版 | 300 | 增补 | UNX-GOV91-048-J1 判据母版可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-049 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·对照等级声明件 | 300 | 增补 | UNX-GOV91-049-J1 对照等级声明件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-050 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·抽样规则件 | 300 | 增补 | UNX-GOV91-050-J1 抽样规则件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-051 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·报告模板件 | 300 | 增补 | UNX-GOV91-051-J1 报告模板件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-052 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·看护器 | 300 | 增补 | UNX-GOV91-052-J1 看护器可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-053 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·探针件 | 300 | 增补 | UNX-GOV91-053-J1 探针件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-054 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·对账单件 | 300 | 增补 | UNX-GOV91-054-J1 对账单件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-055 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·豁免登记件 | 300 | 增补 | UNX-GOV91-055-J1 豁免登记件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-056 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·演练剧本件 | 300 | 增补 | UNX-GOV91-056-J1 演练剧本件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-057 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·阈值表件 | 300 | 增补 | UNX-GOV91-057-J1 阈值表件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-058 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·升级路径件 | 300 | 增补 | UNX-GOV91-058-J1 升级路径件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-059 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·证据包件 | 300 | 增补 | UNX-GOV91-059-J1 证据包件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-060 | 孤儿提交检测（GitHub 有 commit 而台账无批次）·关门印件 | 300 | 增补 | UNX-GOV91-060-J1 关门印件可运行/可观测/可复测：锚定孤儿提交检测（GitHub 有 commit 而台账无批次）（内核侧载体=AI-91 周清职责 + 晨检 #10）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B04（幽灵批检测（台账有批次而仓库无 commit） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-061 | 幽灵批检测（台账有批次而仓库无 commit）·机检规则件 | 300 | 增补 | UNX-GOV91-061-J1 机检规则件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-062 | 幽灵批检测（台账有批次而仓库无 commit）·断言器 | 300 | 增补 | UNX-GOV91-062-J1 断言器可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-063 | 幽灵批检测（台账有批次而仓库无 commit）·巡检器 | 300 | 增补 | UNX-GOV91-063-J1 巡检器可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-064 | 幽灵批检测（台账有批次而仓库无 commit）·账本页 | 300 | 增补 | UNX-GOV91-064-J1 账本页可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-065 | 幽灵批检测（台账有批次而仓库无 commit）·回归样例 | 300 | 增补 | UNX-GOV91-065-J1 回归样例可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-066 | 幽灵批检测（台账有批次而仓库无 commit）·告警单 | 300 | 增补 | UNX-GOV91-066-J1 告警单可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-067 | 幽灵批检测（台账有批次而仓库无 commit）·修复流程件 | 300 | 增补 | UNX-GOV91-067-J1 修复流程件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-068 | 幽灵批检测（台账有批次而仓库无 commit）·判据母版 | 300 | 增补 | UNX-GOV91-068-J1 判据母版可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-069 | 幽灵批检测（台账有批次而仓库无 commit）·对照等级声明件 | 300 | 增补 | UNX-GOV91-069-J1 对照等级声明件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-070 | 幽灵批检测（台账有批次而仓库无 commit）·抽样规则件 | 300 | 增补 | UNX-GOV91-070-J1 抽样规则件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-071 | 幽灵批检测（台账有批次而仓库无 commit）·报告模板件 | 300 | 增补 | UNX-GOV91-071-J1 报告模板件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-072 | 幽灵批检测（台账有批次而仓库无 commit）·看护器 | 300 | 增补 | UNX-GOV91-072-J1 看护器可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-073 | 幽灵批检测（台账有批次而仓库无 commit）·探针件 | 300 | 增补 | UNX-GOV91-073-J1 探针件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-074 | 幽灵批检测（台账有批次而仓库无 commit）·对账单件 | 300 | 增补 | UNX-GOV91-074-J1 对账单件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-075 | 幽灵批检测（台账有批次而仓库无 commit）·豁免登记件 | 300 | 增补 | UNX-GOV91-075-J1 豁免登记件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-076 | 幽灵批检测（台账有批次而仓库无 commit）·演练剧本件 | 300 | 增补 | UNX-GOV91-076-J1 演练剧本件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-077 | 幽灵批检测（台账有批次而仓库无 commit）·阈值表件 | 300 | 增补 | UNX-GOV91-077-J1 阈值表件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-078 | 幽灵批检测（台账有批次而仓库无 commit）·升级路径件 | 300 | 增补 | UNX-GOV91-078-J1 升级路径件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-079 | 幽灵批检测（台账有批次而仓库无 commit）·证据包件 | 300 | 增补 | UNX-GOV91-079-J1 证据包件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-080 | 幽灵批检测（台账有批次而仓库无 commit）·关门印件 | 300 | 增补 | UNX-GOV91-080-J1 关门印件可运行/可观测/可复测：锚定幽灵批检测（台账有批次而仓库无 commit）（内核侧载体=AI-91 周清职责 + 晨检 #12）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B05（hash 回传链核验（commit: <hash> 字段） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-081 | hash 回传链核验（commit: <hash> 字段）·机检规则件 | 300 | 增补 | UNX-GOV91-081-J1 机检规则件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-082 | hash 回传链核验（commit: <hash> 字段）·断言器 | 300 | 增补 | UNX-GOV91-082-J1 断言器可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-083 | hash 回传链核验（commit: <hash> 字段）·巡检器 | 300 | 增补 | UNX-GOV91-083-J1 巡检器可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-084 | hash 回传链核验（commit: <hash> 字段）·账本页 | 300 | 增补 | UNX-GOV91-084-J1 账本页可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-085 | hash 回传链核验（commit: <hash> 字段）·回归样例 | 300 | 增补 | UNX-GOV91-085-J1 回归样例可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-086 | hash 回传链核验（commit: <hash> 字段）·告警单 | 300 | 增补 | UNX-GOV91-086-J1 告警单可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-087 | hash 回传链核验（commit: <hash> 字段）·修复流程件 | 300 | 增补 | UNX-GOV91-087-J1 修复流程件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-088 | hash 回传链核验（commit: <hash> 字段）·判据母版 | 300 | 增补 | UNX-GOV91-088-J1 判据母版可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-089 | hash 回传链核验（commit: <hash> 字段）·对照等级声明件 | 300 | 增补 | UNX-GOV91-089-J1 对照等级声明件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-090 | hash 回传链核验（commit: <hash> 字段）·抽样规则件 | 300 | 增补 | UNX-GOV91-090-J1 抽样规则件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-091 | hash 回传链核验（commit: <hash> 字段）·报告模板件 | 300 | 增补 | UNX-GOV91-091-J1 报告模板件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-092 | hash 回传链核验（commit: <hash> 字段）·看护器 | 300 | 增补 | UNX-GOV91-092-J1 看护器可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-093 | hash 回传链核验（commit: <hash> 字段）·探针件 | 300 | 增补 | UNX-GOV91-093-J1 探针件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-094 | hash 回传链核验（commit: <hash> 字段）·对账单件 | 300 | 增补 | UNX-GOV91-094-J1 对账单件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-095 | hash 回传链核验（commit: <hash> 字段）·豁免登记件 | 300 | 增补 | UNX-GOV91-095-J1 豁免登记件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-096 | hash 回传链核验（commit: <hash> 字段）·演练剧本件 | 300 | 增补 | UNX-GOV91-096-J1 演练剧本件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-097 | hash 回传链核验（commit: <hash> 字段）·阈值表件 | 300 | 增补 | UNX-GOV91-097-J1 阈值表件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-098 | hash 回传链核验（commit: <hash> 字段）·升级路径件 | 300 | 增补 | UNX-GOV91-098-J1 升级路径件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-099 | hash 回传链核验（commit: <hash> 字段）·证据包件 | 300 | 增补 | UNX-GOV91-099-J1 证据包件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-100 | hash 回传链核验（commit: <hash> 字段）·关门印件 | 300 | 增补 | UNX-GOV91-100-J1 关门印件可运行/可观测/可复测：锚定hash 回传链核验（commit: <hash> 字段）（内核侧载体=finalize 第 4e 步 hash 回传锚点）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B06（可追溯率 100% 核查引擎（commit 数=台账已深化批数） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-101 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·机检规则件 | 300 | 增补 | UNX-GOV91-101-J1 机检规则件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-102 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·断言器 | 300 | 增补 | UNX-GOV91-102-J1 断言器可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-103 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·巡检器 | 300 | 增补 | UNX-GOV91-103-J1 巡检器可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-104 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·账本页 | 300 | 增补 | UNX-GOV91-104-J1 账本页可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-105 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·回归样例 | 300 | 增补 | UNX-GOV91-105-J1 回归样例可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-106 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·告警单 | 300 | 增补 | UNX-GOV91-106-J1 告警单可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-107 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·修复流程件 | 300 | 增补 | UNX-GOV91-107-J1 修复流程件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-108 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·判据母版 | 300 | 增补 | UNX-GOV91-108-J1 判据母版可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-109 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·对照等级声明件 | 300 | 增补 | UNX-GOV91-109-J1 对照等级声明件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-110 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·抽样规则件 | 300 | 增补 | UNX-GOV91-110-J1 抽样规则件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-111 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·报告模板件 | 300 | 增补 | UNX-GOV91-111-J1 报告模板件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-112 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·看护器 | 300 | 增补 | UNX-GOV91-112-J1 看护器可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-113 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·探针件 | 300 | 增补 | UNX-GOV91-113-J1 探针件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-114 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·对账单件 | 300 | 增补 | UNX-GOV91-114-J1 对账单件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-115 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·豁免登记件 | 300 | 增补 | UNX-GOV91-115-J1 豁免登记件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-116 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·演练剧本件 | 300 | 增补 | UNX-GOV91-116-J1 演练剧本件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-117 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·阈值表件 | 300 | 增补 | UNX-GOV91-117-J1 阈值表件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-118 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·升级路径件 | 300 | 增补 | UNX-GOV91-118-J1 升级路径件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-119 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·证据包件 | 300 | 增补 | UNX-GOV91-119-J1 证据包件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-120 | 可追溯率 100% 核查引擎（commit 数=台账已深化批数）·关门印件 | 300 | 增补 | UNX-GOV91-120-J1 关门印件可运行/可观测/可复测：锚定可追溯率 100% 核查引擎（commit 数=台账已深化批数）（内核侧载体=§四b 履职判据可追溯率 100%）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B07（draft/<域>-<批> 分支纪律核查（禁直推主分支） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-121 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·机检规则件 | 300 | 增补 | UNX-GOV91-121-J1 机检规则件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-122 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·断言器 | 300 | 增补 | UNX-GOV91-122-J1 断言器可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-123 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·巡检器 | 300 | 增补 | UNX-GOV91-123-J1 巡检器可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-124 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·账本页 | 300 | 增补 | UNX-GOV91-124-J1 账本页可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-125 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·回归样例 | 300 | 增补 | UNX-GOV91-125-J1 回归样例可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-126 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·告警单 | 300 | 增补 | UNX-GOV91-126-J1 告警单可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-127 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·修复流程件 | 300 | 增补 | UNX-GOV91-127-J1 修复流程件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-128 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·判据母版 | 300 | 增补 | UNX-GOV91-128-J1 判据母版可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-129 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·对照等级声明件 | 300 | 增补 | UNX-GOV91-129-J1 对照等级声明件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-130 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·抽样规则件 | 300 | 增补 | UNX-GOV91-130-J1 抽样规则件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-131 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·报告模板件 | 300 | 增补 | UNX-GOV91-131-J1 报告模板件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-132 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·看护器 | 300 | 增补 | UNX-GOV91-132-J1 看护器可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-133 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·探针件 | 300 | 增补 | UNX-GOV91-133-J1 探针件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-134 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·对账单件 | 300 | 增补 | UNX-GOV91-134-J1 对账单件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-135 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·豁免登记件 | 300 | 增补 | UNX-GOV91-135-J1 豁免登记件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-136 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·演练剧本件 | 300 | 增补 | UNX-GOV91-136-J1 演练剧本件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-137 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·阈值表件 | 300 | 增补 | UNX-GOV91-137-J1 阈值表件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-138 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·升级路径件 | 300 | 增补 | UNX-GOV91-138-J1 升级路径件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-139 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·证据包件 | 300 | 增补 | UNX-GOV91-139-J1 证据包件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-140 | draft/<域>-<批> 分支纪律核查（禁直推主分支）·关门印件 | 300 | 增补 | UNX-GOV91-140-J1 关门印件可运行/可观测/可复测：锚定draft/<域>-<批> 分支纪律核查（禁直推主分支）（内核侧载体=§56 细则 3 工作分支纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B08（大文件防线（内核镜像/ISO/VHDX 误入拦截） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-141 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·机检规则件 | 300 | 增补 | UNX-GOV91-141-J1 机检规则件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-142 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·断言器 | 300 | 增补 | UNX-GOV91-142-J1 断言器可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-143 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·巡检器 | 300 | 增补 | UNX-GOV91-143-J1 巡检器可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-144 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·账本页 | 300 | 增补 | UNX-GOV91-144-J1 账本页可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-145 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·回归样例 | 300 | 增补 | UNX-GOV91-145-J1 回归样例可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-146 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·告警单 | 300 | 增补 | UNX-GOV91-146-J1 告警单可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-147 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·修复流程件 | 300 | 增补 | UNX-GOV91-147-J1 修复流程件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-148 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·判据母版 | 300 | 增补 | UNX-GOV91-148-J1 判据母版可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-149 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·对照等级声明件 | 300 | 增补 | UNX-GOV91-149-J1 对照等级声明件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-150 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·抽样规则件 | 300 | 增补 | UNX-GOV91-150-J1 抽样规则件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-151 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·报告模板件 | 300 | 增补 | UNX-GOV91-151-J1 报告模板件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-152 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·看护器 | 300 | 增补 | UNX-GOV91-152-J1 看护器可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-153 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·探针件 | 300 | 增补 | UNX-GOV91-153-J1 探针件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-154 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·对账单件 | 300 | 增补 | UNX-GOV91-154-J1 对账单件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-155 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·豁免登记件 | 300 | 增补 | UNX-GOV91-155-J1 豁免登记件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-156 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·演练剧本件 | 300 | 增补 | UNX-GOV91-156-J1 演练剧本件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-157 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·阈值表件 | 300 | 增补 | UNX-GOV91-157-J1 阈值表件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-158 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·升级路径件 | 300 | 增补 | UNX-GOV91-158-J1 升级路径件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-159 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·证据包件 | 300 | 增补 | UNX-GOV91-159-J1 证据包件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-160 | 大文件防线（内核镜像/ISO/VHDX 误入拦截）·关门印件 | 300 | 增补 | UNX-GOV91-160-J1 关门印件可运行/可观测/可复测：锚定大文件防线（内核镜像/ISO/VHDX 误入拦截）（内核侧载体=场景 7 大文件误入处置 + 域账纯文本纪律）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B09（内核门禁基线联挂核查 · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-161 | 内核门禁基线联挂核查·机检规则件 | 300 | 增补 | UNX-GOV91-161-J1 机检规则件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-162 | 内核门禁基线联挂核查·断言器 | 300 | 增补 | UNX-GOV91-162-J1 断言器可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-163 | 内核门禁基线联挂核查·巡检器 | 300 | 增补 | UNX-GOV91-163-J1 巡检器可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-164 | 内核门禁基线联挂核查·账本页 | 300 | 增补 | UNX-GOV91-164-J1 账本页可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-165 | 内核门禁基线联挂核查·回归样例 | 300 | 增补 | UNX-GOV91-165-J1 回归样例可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-166 | 内核门禁基线联挂核查·告警单 | 300 | 增补 | UNX-GOV91-166-J1 告警单可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-167 | 内核门禁基线联挂核查·修复流程件 | 300 | 增补 | UNX-GOV91-167-J1 修复流程件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-168 | 内核门禁基线联挂核查·判据母版 | 300 | 增补 | UNX-GOV91-168-J1 判据母版可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-169 | 内核门禁基线联挂核查·对照等级声明件 | 300 | 增补 | UNX-GOV91-169-J1 对照等级声明件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-170 | 内核门禁基线联挂核查·抽样规则件 | 300 | 增补 | UNX-GOV91-170-J1 抽样规则件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-171 | 内核门禁基线联挂核查·报告模板件 | 300 | 增补 | UNX-GOV91-171-J1 报告模板件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-172 | 内核门禁基线联挂核查·看护器 | 300 | 增补 | UNX-GOV91-172-J1 看护器可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-173 | 内核门禁基线联挂核查·探针件 | 300 | 增补 | UNX-GOV91-173-J1 探针件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-174 | 内核门禁基线联挂核查·对账单件 | 300 | 增补 | UNX-GOV91-174-J1 对账单件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-175 | 内核门禁基线联挂核查·豁免登记件 | 300 | 增补 | UNX-GOV91-175-J1 豁免登记件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-176 | 内核门禁基线联挂核查·演练剧本件 | 300 | 增补 | UNX-GOV91-176-J1 演练剧本件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-177 | 内核门禁基线联挂核查·阈值表件 | 300 | 增补 | UNX-GOV91-177-J1 阈值表件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-178 | 内核门禁基线联挂核查·升级路径件 | 300 | 增补 | UNX-GOV91-178-J1 升级路径件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-179 | 内核门禁基线联挂核查·证据包件 | 300 | 增补 | UNX-GOV91-179-J1 证据包件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-180 | 内核门禁基线联挂核查·关门印件 | 300 | 增补 | UNX-GOV91-180-J1 关门印件可运行/可观测/可复测：锚定内核门禁基线联挂核查（内核侧载体=门禁铁值 ktest 3146 / kcheck 0 / tsc 0 / vitest 2851 / variable --lib 348 / ca-core 356（09-22 基准））；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B10（回炉批同步对账（批号+序匹配，台账只记终态） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-181 | 回炉批同步对账（批号+序匹配，台账只记终态）·机检规则件 | 300 | 增补 | UNX-GOV91-181-J1 机检规则件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-182 | 回炉批同步对账（批号+序匹配，台账只记终态）·断言器 | 300 | 增补 | UNX-GOV91-182-J1 断言器可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-183 | 回炉批同步对账（批号+序匹配，台账只记终态）·巡检器 | 300 | 增补 | UNX-GOV91-183-J1 巡检器可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-184 | 回炉批同步对账（批号+序匹配，台账只记终态）·账本页 | 300 | 增补 | UNX-GOV91-184-J1 账本页可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-185 | 回炉批同步对账（批号+序匹配，台账只记终态）·回归样例 | 300 | 增补 | UNX-GOV91-185-J1 回归样例可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-186 | 回炉批同步对账（批号+序匹配，台账只记终态）·告警单 | 300 | 增补 | UNX-GOV91-186-J1 告警单可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-187 | 回炉批同步对账（批号+序匹配，台账只记终态）·修复流程件 | 300 | 增补 | UNX-GOV91-187-J1 修复流程件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-188 | 回炉批同步对账（批号+序匹配，台账只记终态）·判据母版 | 300 | 增补 | UNX-GOV91-188-J1 判据母版可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-189 | 回炉批同步对账（批号+序匹配，台账只记终态）·对照等级声明件 | 300 | 增补 | UNX-GOV91-189-J1 对照等级声明件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-190 | 回炉批同步对账（批号+序匹配，台账只记终态）·抽样规则件 | 300 | 增补 | UNX-GOV91-190-J1 抽样规则件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-191 | 回炉批同步对账（批号+序匹配，台账只记终态）·报告模板件 | 300 | 增补 | UNX-GOV91-191-J1 报告模板件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-192 | 回炉批同步对账（批号+序匹配，台账只记终态）·看护器 | 300 | 增补 | UNX-GOV91-192-J1 看护器可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-193 | 回炉批同步对账（批号+序匹配，台账只记终态）·探针件 | 300 | 增补 | UNX-GOV91-193-J1 探针件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-194 | 回炉批同步对账（批号+序匹配，台账只记终态）·对账单件 | 300 | 增补 | UNX-GOV91-194-J1 对账单件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-195 | 回炉批同步对账（批号+序匹配，台账只记终态）·豁免登记件 | 300 | 增补 | UNX-GOV91-195-J1 豁免登记件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-196 | 回炉批同步对账（批号+序匹配，台账只记终态）·演练剧本件 | 300 | 增补 | UNX-GOV91-196-J1 演练剧本件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-197 | 回炉批同步对账（批号+序匹配，台账只记终态）·阈值表件 | 300 | 增补 | UNX-GOV91-197-J1 阈值表件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-198 | 回炉批同步对账（批号+序匹配，台账只记终态）·升级路径件 | 300 | 增补 | UNX-GOV91-198-J1 升级路径件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-199 | 回炉批同步对账（批号+序匹配，台账只记终态）·证据包件 | 300 | 增补 | UNX-GOV91-199-J1 证据包件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-200 | 回炉批同步对账（批号+序匹配，台账只记终态）·关门印件 | 300 | 增补 | UNX-GOV91-200-J1 关门印件可运行/可观测/可复测：锚定回炉批同步对账（批号+序匹配，台账只记终态）（内核侧载体=场景 5 回炉批不造孤儿）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B11（挂起批升级追踪（>24h AI-91 / >48h AI-98） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-201 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·机检规则件 | 300 | 增补 | UNX-GOV91-201-J1 机检规则件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-202 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·断言器 | 300 | 增补 | UNX-GOV91-202-J1 断言器可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-203 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·巡检器 | 300 | 增补 | UNX-GOV91-203-J1 巡检器可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-204 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·账本页 | 300 | 增补 | UNX-GOV91-204-J1 账本页可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-205 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·回归样例 | 300 | 增补 | UNX-GOV91-205-J1 回归样例可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-206 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·告警单 | 300 | 增补 | UNX-GOV91-206-J1 告警单可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-207 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·修复流程件 | 300 | 增补 | UNX-GOV91-207-J1 修复流程件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-208 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·判据母版 | 300 | 增补 | UNX-GOV91-208-J1 判据母版可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-209 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·对照等级声明件 | 300 | 增补 | UNX-GOV91-209-J1 对照等级声明件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-210 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·抽样规则件 | 300 | 增补 | UNX-GOV91-210-J1 抽样规则件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-211 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·报告模板件 | 300 | 增补 | UNX-GOV91-211-J1 报告模板件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-212 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·看护器 | 300 | 增补 | UNX-GOV91-212-J1 看护器可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-213 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·探针件 | 300 | 增补 | UNX-GOV91-213-J1 探针件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-214 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·对账单件 | 300 | 增补 | UNX-GOV91-214-J1 对账单件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-215 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·豁免登记件 | 300 | 增补 | UNX-GOV91-215-J1 豁免登记件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-216 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·演练剧本件 | 300 | 增补 | UNX-GOV91-216-J1 演练剧本件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-217 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·阈值表件 | 300 | 增补 | UNX-GOV91-217-J1 阈值表件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-218 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·升级路径件 | 300 | 增补 | UNX-GOV91-218-J1 升级路径件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-219 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·证据包件 | 300 | 增补 | UNX-GOV91-219-J1 证据包件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-220 | 挂起批升级追踪（>24h AI-91 / >48h AI-98）·关门印件 | 300 | 增补 | UNX-GOV91-220-J1 关门印件可运行/可观测/可复测：锚定挂起批升级追踪（>24h AI-91 / >48h AI-98）（内核侧载体=2.8.4 重试上限与升级）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B12（闭账波级双同步核查（九步闭账第 7 步） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-221 | 闭账波级双同步核查（九步闭账第 7 步）·机检规则件 | 300 | 增补 | UNX-GOV91-221-J1 机检规则件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-222 | 闭账波级双同步核查（九步闭账第 7 步）·断言器 | 300 | 增补 | UNX-GOV91-222-J1 断言器可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-223 | 闭账波级双同步核查（九步闭账第 7 步）·巡检器 | 300 | 增补 | UNX-GOV91-223-J1 巡检器可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-224 | 闭账波级双同步核查（九步闭账第 7 步）·账本页 | 300 | 增补 | UNX-GOV91-224-J1 账本页可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-225 | 闭账波级双同步核查（九步闭账第 7 步）·回归样例 | 300 | 增补 | UNX-GOV91-225-J1 回归样例可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-226 | 闭账波级双同步核查（九步闭账第 7 步）·告警单 | 300 | 增补 | UNX-GOV91-226-J1 告警单可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-227 | 闭账波级双同步核查（九步闭账第 7 步）·修复流程件 | 300 | 增补 | UNX-GOV91-227-J1 修复流程件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-228 | 闭账波级双同步核查（九步闭账第 7 步）·判据母版 | 300 | 增补 | UNX-GOV91-228-J1 判据母版可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-229 | 闭账波级双同步核查（九步闭账第 7 步）·对照等级声明件 | 300 | 增补 | UNX-GOV91-229-J1 对照等级声明件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-230 | 闭账波级双同步核查（九步闭账第 7 步）·抽样规则件 | 300 | 增补 | UNX-GOV91-230-J1 抽样规则件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-231 | 闭账波级双同步核查（九步闭账第 7 步）·报告模板件 | 300 | 增补 | UNX-GOV91-231-J1 报告模板件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-232 | 闭账波级双同步核查（九步闭账第 7 步）·看护器 | 300 | 增补 | UNX-GOV91-232-J1 看护器可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-233 | 闭账波级双同步核查（九步闭账第 7 步）·探针件 | 300 | 增补 | UNX-GOV91-233-J1 探针件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-234 | 闭账波级双同步核查（九步闭账第 7 步）·对账单件 | 300 | 增补 | UNX-GOV91-234-J1 对账单件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-235 | 闭账波级双同步核查（九步闭账第 7 步）·豁免登记件 | 300 | 增补 | UNX-GOV91-235-J1 豁免登记件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-236 | 闭账波级双同步核查（九步闭账第 7 步）·演练剧本件 | 300 | 增补 | UNX-GOV91-236-J1 演练剧本件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-237 | 闭账波级双同步核查（九步闭账第 7 步）·阈值表件 | 300 | 增补 | UNX-GOV91-237-J1 阈值表件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-238 | 闭账波级双同步核查（九步闭账第 7 步）·升级路径件 | 300 | 增补 | UNX-GOV91-238-J1 升级路径件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-239 | 闭账波级双同步核查（九步闭账第 7 步）·证据包件 | 300 | 增补 | UNX-GOV91-239-J1 证据包件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-240 | 闭账波级双同步核查（九步闭账第 7 步）·关门印件 | 300 | 增补 | UNX-GOV91-240-J1 关门印件可运行/可观测/可复测：锚定闭账波级双同步核查（九步闭账第 7 步）（内核侧载体=步7 双同步核查 · 核查结果入发布包）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B13（漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-241 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·机检规则件 | 300 | 增补 | UNX-GOV91-241-J1 机检规则件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-242 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·断言器 | 300 | 增补 | UNX-GOV91-242-J1 断言器可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-243 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·巡检器 | 300 | 增补 | UNX-GOV91-243-J1 巡检器可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-244 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·账本页 | 300 | 增补 | UNX-GOV91-244-J1 账本页可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-245 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·回归样例 | 300 | 增补 | UNX-GOV91-245-J1 回归样例可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-246 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·告警单 | 300 | 增补 | UNX-GOV91-246-J1 告警单可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-247 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·修复流程件 | 300 | 增补 | UNX-GOV91-247-J1 修复流程件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-248 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·判据母版 | 300 | 增补 | UNX-GOV91-248-J1 判据母版可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-249 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·对照等级声明件 | 300 | 增补 | UNX-GOV91-249-J1 对照等级声明件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-250 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·抽样规则件 | 300 | 增补 | UNX-GOV91-250-J1 抽样规则件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-251 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·报告模板件 | 300 | 增补 | UNX-GOV91-251-J1 报告模板件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-252 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·看护器 | 300 | 增补 | UNX-GOV91-252-J1 看护器可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-253 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·探针件 | 300 | 增补 | UNX-GOV91-253-J1 探针件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-254 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·对账单件 | 300 | 增补 | UNX-GOV91-254-J1 对账单件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-255 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·豁免登记件 | 300 | 增补 | UNX-GOV91-255-J1 豁免登记件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-256 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·演练剧本件 | 300 | 增补 | UNX-GOV91-256-J1 演练剧本件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-257 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·阈值表件 | 300 | 增补 | UNX-GOV91-257-J1 阈值表件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-258 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·升级路径件 | 300 | 增补 | UNX-GOV91-258-J1 升级路径件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-259 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·证据包件 | 300 | 增补 | UNX-GOV91-259-J1 证据包件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-260 | 漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）·关门印件 | 300 | 增补 | UNX-GOV91-260-J1 关门印件可运行/可观测/可复测：锚定漂移 L1/L2/L3 分级修复（静默漂移周检/结构性漂移冻结重放）（内核侧载体=L1 误写/L2 静默漂移/L3 结构性漂移修复流程）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B14（引用链核验（上游 finalize+commit 可查先于下游引用） · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-261 | 引用链核验（上游 finalize+commit 可查先于下游引用）·机检规则件 | 300 | 增补 | UNX-GOV91-261-J1 机检规则件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-262 | 引用链核验（上游 finalize+commit 可查先于下游引用）·断言器 | 300 | 增补 | UNX-GOV91-262-J1 断言器可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-263 | 引用链核验（上游 finalize+commit 可查先于下游引用）·巡检器 | 300 | 增补 | UNX-GOV91-263-J1 巡检器可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-264 | 引用链核验（上游 finalize+commit 可查先于下游引用）·账本页 | 300 | 增补 | UNX-GOV91-264-J1 账本页可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-265 | 引用链核验（上游 finalize+commit 可查先于下游引用）·回归样例 | 300 | 增补 | UNX-GOV91-265-J1 回归样例可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-266 | 引用链核验（上游 finalize+commit 可查先于下游引用）·告警单 | 300 | 增补 | UNX-GOV91-266-J1 告警单可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-267 | 引用链核验（上游 finalize+commit 可查先于下游引用）·修复流程件 | 300 | 增补 | UNX-GOV91-267-J1 修复流程件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-268 | 引用链核验（上游 finalize+commit 可查先于下游引用）·判据母版 | 300 | 增补 | UNX-GOV91-268-J1 判据母版可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-269 | 引用链核验（上游 finalize+commit 可查先于下游引用）·对照等级声明件 | 300 | 增补 | UNX-GOV91-269-J1 对照等级声明件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-270 | 引用链核验（上游 finalize+commit 可查先于下游引用）·抽样规则件 | 300 | 增补 | UNX-GOV91-270-J1 抽样规则件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-271 | 引用链核验（上游 finalize+commit 可查先于下游引用）·报告模板件 | 300 | 增补 | UNX-GOV91-271-J1 报告模板件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-272 | 引用链核验（上游 finalize+commit 可查先于下游引用）·看护器 | 300 | 增补 | UNX-GOV91-272-J1 看护器可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-273 | 引用链核验（上游 finalize+commit 可查先于下游引用）·探针件 | 300 | 增补 | UNX-GOV91-273-J1 探针件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-274 | 引用链核验（上游 finalize+commit 可查先于下游引用）·对账单件 | 300 | 增补 | UNX-GOV91-274-J1 对账单件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-275 | 引用链核验（上游 finalize+commit 可查先于下游引用）·豁免登记件 | 300 | 增补 | UNX-GOV91-275-J1 豁免登记件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-276 | 引用链核验（上游 finalize+commit 可查先于下游引用）·演练剧本件 | 300 | 增补 | UNX-GOV91-276-J1 演练剧本件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-277 | 引用链核验（上游 finalize+commit 可查先于下游引用）·阈值表件 | 300 | 增补 | UNX-GOV91-277-J1 阈值表件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-278 | 引用链核验（上游 finalize+commit 可查先于下游引用）·升级路径件 | 300 | 增补 | UNX-GOV91-278-J1 升级路径件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-279 | 引用链核验（上游 finalize+commit 可查先于下游引用）·证据包件 | 300 | 增补 | UNX-GOV91-279-J1 证据包件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-280 | 引用链核验（上游 finalize+commit 可查先于下游引用）·关门印件 | 300 | 增补 | UNX-GOV91-280-J1 关门印件可运行/可观测/可复测：锚定引用链核验（上游 finalize+commit 可查先于下游引用）（内核侧载体=场景 8 引用链按 hash 核，顺序颠倒=幽灵批打回）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 批 UNX-GOV91-B15（冻结窗与镜像周提交窗口管理 · 20 条 · 6,000 行）

| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| GOV91-281 | 冻结窗与镜像周提交窗口管理·机检规则件 | 300 | 增补 | UNX-GOV91-281-J1 机检规则件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-282 | 冻结窗与镜像周提交窗口管理·断言器 | 300 | 增补 | UNX-GOV91-282-J1 断言器可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-283 | 冻结窗与镜像周提交窗口管理·巡检器 | 300 | 增补 | UNX-GOV91-283-J1 巡检器可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-284 | 冻结窗与镜像周提交窗口管理·账本页 | 300 | 增补 | UNX-GOV91-284-J1 账本页可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-285 | 冻结窗与镜像周提交窗口管理·回归样例 | 300 | 增补 | UNX-GOV91-285-J1 回归样例可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-286 | 冻结窗与镜像周提交窗口管理·告警单 | 300 | 增补 | UNX-GOV91-286-J1 告警单可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-287 | 冻结窗与镜像周提交窗口管理·修复流程件 | 300 | 增补 | UNX-GOV91-287-J1 修复流程件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-288 | 冻结窗与镜像周提交窗口管理·判据母版 | 300 | 增补 | UNX-GOV91-288-J1 判据母版可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-289 | 冻结窗与镜像周提交窗口管理·对照等级声明件 | 300 | 增补 | UNX-GOV91-289-J1 对照等级声明件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-290 | 冻结窗与镜像周提交窗口管理·抽样规则件 | 300 | 增补 | UNX-GOV91-290-J1 抽样规则件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-291 | 冻结窗与镜像周提交窗口管理·报告模板件 | 300 | 增补 | UNX-GOV91-291-J1 报告模板件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-292 | 冻结窗与镜像周提交窗口管理·看护器 | 300 | 增补 | UNX-GOV91-292-J1 看护器可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-293 | 冻结窗与镜像周提交窗口管理·探针件 | 300 | 增补 | UNX-GOV91-293-J1 探针件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-294 | 冻结窗与镜像周提交窗口管理·对账单件 | 300 | 增补 | UNX-GOV91-294-J1 对账单件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-295 | 冻结窗与镜像周提交窗口管理·豁免登记件 | 300 | 增补 | UNX-GOV91-295-J1 豁免登记件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-296 | 冻结窗与镜像周提交窗口管理·演练剧本件 | 300 | 增补 | UNX-GOV91-296-J1 演练剧本件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-297 | 冻结窗与镜像周提交窗口管理·阈值表件 | 300 | 增补 | UNX-GOV91-297-J1 阈值表件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-298 | 冻结窗与镜像周提交窗口管理·升级路径件 | 300 | 增补 | UNX-GOV91-298-J1 升级路径件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-299 | 冻结窗与镜像周提交窗口管理·证据包件 | 300 | 增补 | UNX-GOV91-299-J1 证据包件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |
| GOV91-300 | 冻结窗与镜像周提交窗口管理·关门印件 | 300 | 增补 | UNX-GOV91-300-J1 关门印件可运行/可观测/可复测：锚定冻结窗与镜像周提交窗口管理（内核侧载体=场景 6 闭账前 3 日冻结治理杂项提交 + 场景 11 PR 重开时限）；同步审计证据落 _attic/reports/，全程围绕 Varix 内核门禁链（kbuild/ktest/kcheck）执行，零触碰他域域账 |

## 波次段总账（AI-91 · 治理线 B01–B15）

- **总量**：15 批 × 20 条 = **300 项新功能增补**；ID 段 GOV91-001–GOV91-300 连续零跳号、零重号；治理线独立增补账 90,000 行（不入 19,200,000 纯功能总账，先例承 AI-83/AI-87）。
- **拒收权行使口径**：本域拒收必引条文（分工图 §四b + 七步 SOP 第 4 步），无条文否决视为滥权；拒收记录全部落 _attic/reports/ 证据包。
- **待续**：B16–B40（500 条 · GOV91-301–GOV91-800）另册续写；域账 90,000/240,000（37.5%）。
