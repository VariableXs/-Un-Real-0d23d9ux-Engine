# VX11-1105 派工卡 B12（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9298 | `unx9298_etype_takeover_note_proc.rs` | 0x7B800 | 280 | 进程族 E 型升级接管注记 |
| 2 | UNX-F9299 | `unx9299_etype_vs_consumer_proc.rs` | 0x7B840 | 320 | 进程族 E 型与 glibc fork/exec 错误路径消费核 |
| 3 | UNX-F9300 | `unx9300_ktest_syscall_b25.rs` | 0x7B880 | 440 | ktest syscall 面 B25 批断言集 |
| 4 | UNX-F9301 | `unx9301_charter_eintr.rs` | 0x7B8C0 | 420 | EINTR 语义总纲（慢系统调用中断面） |
| 5 | UNX-F9302 | `unx9302_matrix_sa_restart_syscall.rs` | 0x7B900 | 420 | SA_RESTART 重启语义矩阵（逐 syscall 重启面） |
| 6 | UNX-F9303 | `unx9303_matrix_read_write.rs` | 0x7B940 | 400 | read/write EINTR 矩阵（部分读写残留量） |
| 7 | UNX-F9304 | `unx9304_matrix_nanosleep_eintr.rs` | 0x7B980 | 380 | nanosleep EINTR remaining 回写矩阵（联 B20） |
| 8 | UNX-F9305 | `unx9305_matrix_wait_eintr.rs` | 0x7B9C0 | 320 | wait EINTR 矩阵（联 B18） |

## UNX-F9298 进程族 E 型升级接管注记
- 文件：`varix/src/unxstar/unx9298_etype_takeover_note_proc.rs`
- 函数：`pub fn run_unx9298_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B800;`
- 判据原文：① 纯功能代码 ≥280 行；② 判据原文：UNX-F9298-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9299 进程族 E 型与 glibc fork/exec 错误路径消费核
- 文件：`varix/src/unxstar/unx9299_etype_vs_consumer_proc.rs`
- 函数：`pub fn run_unx9299_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B840;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9299-J1 fork 返回 -1 errno 透传判据过，exec 失败子进程 _exit(127) 惯例格过，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9300 ktest syscall 面 B25 批断言集
- 文件：`varix/src/unxstar/unx9300_ktest_syscall_b25.rs`
- 函数：`pub fn run_unx9300_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B880;`
- 判据原文：① 纯功能代码 ≥440 行；② 判据原文：UNX-F9300-J1 本批 19 条判据聚合判据过，进程族 E 型矩阵一次跑通，域累计 132,700 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9301 EINTR 语义总纲（慢系统调用中断面）
- 文件：`varix/src/unxstar/unx9301_charter_eintr.rs`
- 函数：`pub fn run_unx9301_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B8C0;`
- 判据原文：① 纯功能代码 ≥420 行；② 判据原文：UNX-F9301-J1 慢系统调用中断面判据过，EINTR 触发三条件（信号到达/无 SA_RESTART/阻塞中）逐格落账，与 Linux 语义一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9302 SA_RESTART 重启语义矩阵（逐 syscall 重启面）
- 文件：`varix/src/unxstar/unx9302_matrix_sa_restart_syscall.rs`
- 函数：`pub fn run_unx9302_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B900;`
- 判据原文：① 纯功能代码 ≥420 行；② 判据原文：UNX-F9302-J1 SA_RESTART 逐 syscall 重启面矩阵判据过，可重启族逐格重启可观测，不可重启族逐格 EINTR 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9303 read/write EINTR 矩阵（部分读写残留量）
- 文件：`varix/src/unxstar/unx9303_matrix_read_write.rs`
- 函数：`pub fn run_unx9303_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B940;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9303-J1 部分读写后中断注入判据过，已传字节数返回正判据格过，零字节残留 EINTR 格过，与 B03/B04 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9304 nanosleep EINTR remaining 回写矩阵（联 B20）
- 文件：`varix/src/unxstar/unx9304_matrix_nanosleep_eintr.rs`
- 函数：`pub fn run_unx9304_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B980;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9304-J1 中断注入 remaining 回写判据过，剩余量逐 ns 可观测格过，与 B20 休眠族对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9305 wait EINTR 矩阵（联 B18）
- 文件：`varix/src/unxstar/unx9305_matrix_wait_eintr.rs`
- 函数：`pub fn run_unx9305_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B9C0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9305-J1 阻塞 wait 中断注入判据过，全 EINTR 格过，WNOHANG 零中断正判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
