# VX11-1105 派工卡 B22（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9378 | `unx9378_consumer_jointest_glibc.rs` | 0x7CC00 | 320 | 联调与 glibc 消费核联测面 |
| 2 | UNX-F9379 | `unx9379_declare.rs` | 0x7CC40 | 300 | I 型联调判据可复测性声明 |
| 3 | UNX-F9380 | `unx9380_ktest_syscall_b29.rs` | 0x7CC80 | 340 | ktest syscall 面 B29 批断言集 |
| 4 | UNX-F9381 | `unx9381_charter_cosign_hook.rs` | 0x7CCC0 | 400 | I 型联调总纲二（C2×C4 联签协议与挂点面） |
| 5 | UNX-F9382 | `unx9382_cosign_hook_futex.rs` | 0x7CD00 | 400 | futex 四操作挂点联签（WAIT/WAKE/FD/REQUEUE 协议） |
| 6 | UNX-F9383 | `unx9383_matrix_jointest_hook.rs` | 0x7CD40 | 360 | futex EAGAIN/EINVAL 矩阵联测（C2 错误矩阵 × C4 挂点） |
| 7 | UNX-F9384 | `unx9384_cosign_hook_epoll_ctl.rs` | 0x7CD80 | 380 | epoll_ctl/epoll_wait 挂点联签（就绪队列协议） |
| 8 | UNX-F9385 | `unx9385_matrix_jointest_epoll.rs` | 0x7CDC0 | 340 | epoll 错误矩阵联测（EBADF/EINVAL/EPERM） |

## UNX-F9378 联调与 glibc 消费核联测面
- 文件：`varix/src/unxstar/unx9378_consumer_jointest_glibc.rs`
- 函数：`pub fn run_unx9378_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CC00;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9378-J1 消费核联测判据过，glibc 消费清单与联测面对接可观测，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9379 I 型联调判据可复测性声明
- 文件：`varix/src/unxstar/unx9379_declare.rs`
- 函数：`pub fn run_unx9379_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CC40;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9379-J1 联测判据 10 次重复判据过，10/10 次通过统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9380 ktest syscall 面 B29 批断言集
- 文件：`varix/src/unxstar/unx9380_ktest_syscall_b29.rs`
- 函数：`pub fn run_unx9380_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CC80;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9380-J1 本批 19 条判据聚合判据过，C2×C3 联调一次跑通，域累计 162,000 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9381 I 型联调总纲二（C2×C4 联签协议与挂点面）
- 文件：`varix/src/unxstar/unx9381_charter_cosign_hook.rs`
- 函数：`pub fn run_unx9381_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CCC0;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9381-J1 联签协议判据过，C2（阻塞挂点）×C4（futex/epoll/pty 面）双方挂点面互认格过，跨界代写禁令声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9382 futex 四操作挂点联签（WAIT/WAKE/FD/REQUEUE 协议）
- 文件：`varix/src/unxstar/unx9382_cosign_hook_futex.rs`
- 函数：`pub fn run_unx9382_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CD00;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9382-J1 四操作挂点联签判据过，C4 futex 四操作协议逐操作对接可观测，挂点单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9383 futex EAGAIN/EINVAL 矩阵联测（C2 错误矩阵 × C4 挂点）
- 文件：`varix/src/unxstar/unx9383_matrix_jointest_hook.rs`
- 函数：`pub fn run_unx9383_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CD40;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9383-J1 矩阵联测判据过，EAGAIN 耗尽格与 EINVAL 参数格逐格联测可观测，与 B23 判定域一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9384 epoll_ctl/epoll_wait 挂点联签（就绪队列协议）
- 文件：`varix/src/unxstar/unx9384_cosign_hook_epoll_ctl.rs`
- 函数：`pub fn run_unx9384_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CD80;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9384-J1 两挂点联签判据过，就绪队列协议对接可观测，与 B09 四件套语义一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9385 epoll 错误矩阵联测（EBADF/EINVAL/EPERM）
- 文件：`varix/src/unxstar/unx9385_matrix_jointest_epoll.rs`
- 函数：`pub fn run_unx9385_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CDC0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9385-J1 三错误码联测判据过，逐码与 C4 挂点联测一致格过，与 B22 判定域一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
