# VX11-1105 派工卡 B13（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9306 | `unx9306_matrix_poll_epoll.rs` | 0x7BA00 | 380 | poll/epoll EINTR 矩阵（就绪前中断） |
| 2 | UNX-F9307 | `unx9307_matrix_hook_eintr.rs` | 0x7BA40 | 340 | 阻塞挂点 EINTR 矩阵（waitqueue 等待中断窗） |
| 3 | UNX-F9308 | `unx9308_matrix_flock_lockf.rs` | 0x7BA80 | 320 | flock/lockf EINTR 矩阵（锁等待中断） |
| 4 | UNX-F9309 | `unx9309_matrix_race.rs` | 0x7BAC0 | 400 | 信号投递时机竞态矩阵（阻塞前后双窗） |
| 5 | UNX-F9310 | `unx9310_matrix_eintr.rs` | 0x7BB00 | 320 | EINTR 与线程取消联动矩阵 |
| 6 | UNX-F9311 | `unx9311_matrix_bytes_copied.rs` | 0x7BB40 | 380 | 部分完成语义矩阵（bytes-copied-before-signal） |
| 7 | UNX-F9312 | `unx9312_injector_eintr.rs` | 0x7BB80 | 400 | EINTR 错误注入器（信号发生器协议） |
| 8 | UNX-F9313 | `unx9313_order_eintr.rs` | 0x7BBC0 | 340 | EINTR 判定次序协议（先完成后中断） |

## UNX-F9306 poll/epoll EINTR 矩阵（就绪前中断）
- 文件：`varix/src/unxstar/unx9306_matrix_poll_epoll.rs`
- 函数：`pub fn run_unx9306_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BA00;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9306-J1 就绪前中断注入判据过，全 EINTR 格过，重启后重入正确格过，与 B09 四件套对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9307 阻塞挂点 EINTR 矩阵（waitqueue 等待中断窗）
- 文件：`varix/src/unxstar/unx9307_matrix_hook_eintr.rs`
- 函数：`pub fn run_unx9307_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BA40;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9307-J1 等待队列中断注入判据过，全 EINTR 格过，唤醒零丢失格过，与 A3 fake 原语对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9308 flock/lockf EINTR 矩阵（锁等待中断）
- 文件：`varix/src/unxstar/unx9308_matrix_flock_lockf.rs`
- 函数：`pub fn run_unx9308_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BA80;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9308-J1 锁等待中断注入判据过，全 EINTR 格过，锁状态零破坏格过，与 B16 记录锁挂点对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9309 信号投递时机竞态矩阵（阻塞前后双窗）
- 文件：`varix/src/unxstar/unx9309_matrix_race.rs`
- 函数：`pub fn run_unx9309_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BAC0;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9309-J1 阻塞前后双窗注入判据过，双窗逐格可观测，信号不丢失 100 次统计格过，与 B11/B12 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9310 EINTR 与线程取消联动矩阵
- 文件：`varix/src/unxstar/unx9310_matrix_eintr.rs`
- 函数：`pub fn run_unx9310_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BB00;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9310-J1 取消点注入判据过，取消点集逐格落账，实装归 C1 线程联签声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9311 部分完成语义矩阵（bytes-copied-before-signal）
- 文件：`varix/src/unxstar/unx9311_matrix_bytes_copied.rs`
- 函数：`pub fn run_unx9311_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BB40;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9311-J1 半程拷贝后中断注入判据过，已拷贝字节返回语义格过，与 EFAULT 半程探针对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9312 EINTR 错误注入器（信号发生器协议）
- 文件：`varix/src/unxstar/unx9312_injector_eintr.rs`
- 函数：`pub fn run_unx9312_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BB80;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9312-J1 信号发生器注入协议判据过，注入时序可控可观测，100 次注入统计复测格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9313 EINTR 判定次序协议（先完成后中断）
- 文件：`varix/src/unxstar/unx9313_order_eintr.rs`
- 函数：`pub fn run_unx9313_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BBC0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9313-J1 判序协议判据过，完成优先于中断逐格检出，与 Linux 语义一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
