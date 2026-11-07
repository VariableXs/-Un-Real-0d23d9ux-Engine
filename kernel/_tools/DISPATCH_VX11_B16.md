# VX11-1105 派工卡 B16（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9330 | `unx9330_matrix_rlimit_einval.rs` | 0x7C000 | 340 | rlimit 软硬倒置 EINVAL 矩阵（联 B19 对账） |
| 2 | UNX-F9331 | `unx9331_matrix_sched_setaffinity_einval.rs` | 0x7C040 | 340 | sched_setaffinity 坏掩码 EINVAL 矩阵 |
| 3 | UNX-F9332 | `unx9332_matrix_sched_setscheduler_einval.rs` | 0x7C080 | 340 | sched_setscheduler 非法策略 EINVAL 矩阵 |
| 4 | UNX-F9333 | `unx9333_matrix_boundary_tv_nsec.rs` | 0x7C0C0 | 380 | 时间值规范化边界矩阵（tv_nsec 上限 1e9） |
| 5 | UNX-F9334 | `unx9334_eprobe_suite_timeres.rs` | 0x7C100 | 300 | 时间/资源族 E 型参数探针总成 |
| 6 | UNX-F9335 | `unx9335_etype_vs_reconcile_timeres.rs` | 0x7C140 | 300 | 时间/资源族 E 型与 B06/B19 深化册对账格 |
| 7 | UNX-F9336 | `unx9336_etype_vs_compare_timeres.rs` | 0x7C180 | 300 | 时间/资源族 E 型与 Windows 计时器错误对照格 |
| 8 | UNX-F9337 | `unx9337_etype_recheck_declare.rs` | 0x7C1C0 | 300 | E 型判据可复测性声明（10/10 次检出） |

## UNX-F9330 rlimit 软硬倒置 EINVAL 矩阵（联 B19 对账）
- 文件：`varix/src/unxstar/unx9330_matrix_rlimit_einval.rs`
- 函数：`pub fn run_unx9330_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C000;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9330-J1 软硬倒置注入判据过，全 EINVAL 格过，与 B19 矩阵零重复分层格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9331 sched_setaffinity 坏掩码 EINVAL 矩阵
- 文件：`varix/src/unxstar/unx9331_matrix_sched_setaffinity_einval.rs`
- 函数：`pub fn run_unx9331_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C040;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9331-J1 全零掩码注入判据过，全 EINVAL 格过，坏 mask 指针全 EFAULT 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9332 sched_setscheduler 非法策略 EINVAL 矩阵
- 文件：`varix/src/unxstar/unx9332_matrix_sched_setscheduler_einval.rs`
- 函数：`pub fn run_unx9332_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C080;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9332-J1 非法策略注入判据过，全 EINVAL 格过，非法 param 全 EINVAL 格过，A1 消费声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9333 时间值规范化边界矩阵（tv_nsec 上限 1e9）
- 文件：`varix/src/unxstar/unx9333_matrix_boundary_tv_nsec.rs`
- 函数：`pub fn run_unx9333_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C0C0;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9333-J1 tv_nsec 越界注入判据过，≥1e9 全 EINVAL 格过，规范化（tv_sec 进位）语义格过，与 B20 规范化对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9334 时间/资源族 E 型参数探针总成
- 文件：`varix/src/unxstar/unx9334_eprobe_suite_timeres.rs`
- 函数：`pub fn run_unx9334_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C100;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9334-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9335 时间/资源族 E 型与 B06/B19 深化册对账格
- 文件：`varix/src/unxstar/unx9335_etype_vs_reconcile_timeres.rs`
- 函数：`pub fn run_unx9335_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C140;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9335-J1 对账判据过，与时间族/资源族深化册逐条零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9336 时间/资源族 E 型与 Windows 计时器错误对照格
- 文件：`varix/src/unxstar/unx9336_etype_vs_compare_timeres.rs`
- 函数：`pub fn run_unx9336_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C180;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9336-J1 ERROR_INVALID_PARAMETER 映射判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9337 E 型判据可复测性声明（10/10 次检出）
- 文件：`varix/src/unxstar/unx9337_etype_recheck_declare.rs`
- 函数：`pub fn run_unx9337_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C1C0;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9337-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
