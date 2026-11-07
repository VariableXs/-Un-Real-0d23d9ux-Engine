# VX11-1105 派工卡 B02（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9218 | `unx9218_etype_takeover_note_file.rs` | 0x7A400 | 280 | 文件族 E 型升级接管注记 |
| 2 | UNX-F9219 | `unx9219_etype_vs_consumer_file.rs` | 0x7A440 | 320 | 文件族 E 型与 glibc errno 透传消费核 |
| 3 | UNX-F9220 | `unx9220_ktest_syscall_b21.rs` | 0x7A480 | 420 | ktest syscall 面 B21 批断言集 |
| 4 | UNX-F9221 | `unx9221_charter_domain_fd.rs` | 0x7A4C0 | 400 | fd 语义域总纲（EBADF 判定域与 fd 空间模型） |
| 5 | UNX-F9222 | `unx9222_matrix_dup_dup2.rs` | 0x7A500 | 340 | dup/dup2/dup3 坏 fd 矩阵 |
| 6 | UNX-F9223 | `unx9223_matrix_syscall_err_pipe.rs` | 0x7A540 | 340 | pipe 系统调用错误矩阵（EFAULT/EMFILE/ENFILE） |
| 7 | UNX-F9224 | `unx9224_matrix_fd_emfile.rs` | 0x7A580 | 360 | fd 表耗尽 EMFILE 矩阵（RLIMIT_NOFILE 联动） |
| 8 | UNX-F9225 | `unx9225_matrix_enfile.rs` | 0x7A5C0 | 320 | 系统级 ENFILE 矩阵（文件表上限） |

## UNX-F9218 文件族 E 型升级接管注记
- 文件：`varix/src/unxstar/unx9218_etype_takeover_note_file.rs`
- 函数：`pub fn run_unx9218_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A400;`
- 判据原文：① 纯功能代码 ≥280 行；② 判据原文：UNX-F9218-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9219 文件族 E 型与 glibc errno 透传消费核
- 文件：`varix/src/unxstar/unx9219_etype_vs_consumer_file.rs`
- 函数：`pub fn run_unx9219_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A440;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9219-J1 glibc errno 读取透传链判据过，perror/strerror 输出逐码一致格过，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9220 ktest syscall 面 B21 批断言集
- 文件：`varix/src/unxstar/unx9220_ktest_syscall_b21.rs`
- 函数：`pub fn run_unx9220_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A480;`
- 判据原文：① 纯功能代码 ≥420 行；② 判据原文：UNX-F9220-J1 本批 19 条判据聚合判据过，文件族 E 型矩阵一次跑通，域累计 105,700 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9221 fd 语义域总纲（EBADF 判定域与 fd 空间模型）
- 文件：`varix/src/unxstar/unx9221_charter_domain_fd.rs`
- 函数：`pub fn run_unx9221_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A4C0;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9221-J1 fd 空间模型（0/1/2 基座+分配序）判据过，EBADF 判定域三条件（越界/未开/已关）逐格落账，判定先于操作语义格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9222 dup/dup2/dup3 坏 fd 矩阵
- 文件：`varix/src/unxstar/unx9222_matrix_dup_dup2.rs`
- 函数：`pub fn run_unx9222_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A500;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9222-J1 oldfd/newfd 坏值注入矩阵判据过，全 EBADF 逐格检出，dup2 equal-fd 返回 newfd 正判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9223 pipe 系统调用错误矩阵（EFAULT/EMFILE/ENFILE）
- 文件：`varix/src/unxstar/unx9223_matrix_syscall_err_pipe.rs`
- 函数：`pub fn run_unx9223_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A540;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9223-J1 三错误码注入矩阵判据过，坏 pipefd 指针全 EFAULT 格过，逐码与 Linux 一致；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9224 fd 表耗尽 EMFILE 矩阵（RLIMIT_NOFILE 联动）
- 文件：`varix/src/unxstar/unx9224_matrix_fd_emfile.rs`
- 函数：`pub fn run_unx9224_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A580;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9224-J1 限额触顶注入判据过，分配全 EMFILE 格过，限额上调后恢复正判据格过，与 B19 资源族联动格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9225 系统级 ENFILE 矩阵（文件表上限）
- 文件：`varix/src/unxstar/unx9225_matrix_enfile.rs`
- 函数：`pub fn run_unx9225_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A5C0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9225-J1 系统级上限注入判据过，全 ENFILE 格过，与进程级 EMFILE 判序（先进程后系统）格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
