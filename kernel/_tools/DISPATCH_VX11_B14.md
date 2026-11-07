# VX11-1105 派工卡 B14（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9314 | `unx9314_race_stats_declare.rs` | 0x7BC00 | 340 | E 型竞态可复测性声明（竞态 100 次注入统计） |
| 2 | UNX-F9315 | `unx9315_reconcile_hook_eintr.rs` | 0x7BC40 | 320 | EINTR 与 B12 事件挂点族对账格 |
| 3 | UNX-F9316 | `unx9316_reconcile_eintr_a1.rs` | 0x7BC80 | 320 | EINTR 与 A1 调度让出协议对账格 |
| 4 | UNX-F9317 | `unx9317_compare_eintr_windows.rs` | 0x7BCC0 | 300 | EINTR 与 Windows ERROR_INTERRUPTED 对照格 |
| 5 | UNX-F9318 | `unx9318_consumer_eintr_glibc.rs` | 0x7BD00 | 320 | EINTR 与 glibc TEMP_FAILURE_RETRY 消费核 |
| 6 | UNX-F9319 | `unx9319_race_suite.rs` | 0x7BD40 | 300 | E 型竞态参数探针总成 |
| 7 | UNX-F9320 | `unx9320_ktest_syscall_b26.rs` | 0x7BD80 | 480 | ktest syscall 面 B26 批断言集 |
| 8 | UNX-F9321 | `unx9321_charter_domain_einval.rs` | 0x7BDC0 | 400 | 时间资源错误面总纲（EINVAL 判定域） |

## UNX-F9314 E 型竞态可复测性声明（竞态 100 次注入统计）
- 文件：`varix/src/unxstar/unx9314_race_stats_declare.rs`
- 函数：`pub fn run_unx9314_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BC00;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9314-J1 竞态 100 次注入统计判据过，统计分布落账格过，零 flaky 阈值声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9315 EINTR 与 B12 事件挂点族对账格
- 文件：`varix/src/unxstar/unx9315_reconcile_hook_eintr.rs`
- 函数：`pub fn run_unx9315_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BC40;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9315-J1 对账判据过，与 B12 四输入判定器逐条零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9316 EINTR 与 A1 调度让出协议对账格
- 文件：`varix/src/unxstar/unx9316_reconcile_eintr_a1.rs`
- 函数：`pub fn run_unx9316_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BC80;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9316-J1 对账判据过，让出协议单源格过，上游依赖登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9317 EINTR 与 Windows ERROR_INTERRUPTED 对照格
- 文件：`varix/src/unxstar/unx9317_compare_eintr_windows.rs`
- 函数：`pub fn run_unx9317_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BCC0;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9317-J1 映射判据过，APC 中断差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9318 EINTR 与 glibc TEMP_FAILURE_RETRY 消费核
- 文件：`varix/src/unxstar/unx9318_consumer_eintr_glibc.rs`
- 函数：`pub fn run_unx9318_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BD00;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9318-J1 重试宏消费链判据过，重试后语义正确可观测，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9319 E 型竞态参数探针总成
- 文件：`varix/src/unxstar/unx9319_race_suite.rs`
- 函数：`pub fn run_unx9319_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BD40;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9319-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9320 ktest syscall 面 B26 批断言集
- 文件：`varix/src/unxstar/unx9320_ktest_syscall_b26.rs`
- 函数：`pub fn run_unx9320_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BD80;`
- 判据原文：① 纯功能代码 ≥480 行；② 判据原文：UNX-F9320-J1 本批 19 条判据聚合判据过，EINTR 竞态矩阵一次跑通，域累计 139,900 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9321 时间资源错误面总纲（EINVAL 判定域）
- 文件：`varix/src/unxstar/unx9321_charter_domain_einval.rs`
- 函数：`pub fn run_unx9321_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BDC0;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9321-J1 时间资源错误面判据过，EINVAL 触发域（非法时钟/非法标志/值域越界）逐格落账，与 Linux 语义一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
