# VX11-1105 派工卡 B10（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9282 | `unx9282_matrix_kill_pid.rs` | 0x7B400 | 360 | kill 坏 pid 矩阵（ESRCH/EPERM/EINVAL） |
| 2 | UNX-F9283 | `unx9283_matrix_wait_pid.rs` | 0x7B440 | 320 | wait 坏 pid ECHILD 矩阵（联 B18 对账） |
| 3 | UNX-F9284 | `unx9284_matrix_fork_clone.rs` | 0x7B480 | 380 | fork/clone EAGAIN 矩阵（RLIMIT_NPROC 联动） |
| 4 | UNX-F9285 | `unx9285_matrix_fork_clone.rs` | 0x7B4C0 | 340 | fork/clone ENOMEM 矩阵（地址空间耗尽） |
| 5 | UNX-F9286 | `unx9286_matrix_setpgid_pid.rs` | 0x7B500 | 320 | setpgid 坏 pid ESRCH 矩阵 |
| 6 | UNX-F9287 | `unx9287_matrix_setsid_eperm.rs` | 0x7B540 | 320 | setsid EPERM 矩阵（会话首规则联 B18） |
| 7 | UNX-F9288 | `unx9288_matrix_execve_eacces.rs` | 0x7B580 | 380 | execve 错误矩阵（EACCES/ENOEXEC/E2BIG/EFAULT） |
| 8 | UNX-F9289 | `unx9289_matrix_e2big_argv.rs` | 0x7B5C0 | 340 | 参数长度 E2BIG 矩阵（argv/envp 上限） |

## UNX-F9282 kill 坏 pid 矩阵（ESRCH/EPERM/EINVAL）
- 文件：`varix/src/unxstar/unx9282_matrix_kill_pid.rs`
- 函数：`pub fn run_unx9282_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B400;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9282-J1 三错误码注入矩阵判据过，pid=1 EPERM 特例格过，非法 sig 全 EINVAL 格过，不存在 pid 全 ESRCH 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9283 wait 坏 pid ECHILD 矩阵（联 B18 对账）
- 文件：`varix/src/unxstar/unx9283_matrix_wait_pid.rs`
- 函数：`pub fn run_unx9283_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B440;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9283-J1 无子进程注入判据过，全 ECHILD 格过，与 B18 wait 矩阵零重复分层格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9284 fork/clone EAGAIN 矩阵（RLIMIT_NPROC 联动）
- 文件：`varix/src/unxstar/unx9284_matrix_fork_clone.rs`
- 函数：`pub fn run_unx9284_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B480;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9284-J1 进程数触顶注入判据过，全 EAGAIN 格过，与 B19 NPROC 联动格过，释放后恢复正判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9285 fork/clone ENOMEM 矩阵（地址空间耗尽）
- 文件：`varix/src/unxstar/unx9285_matrix_fork_clone.rs`
- 函数：`pub fn run_unx9285_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B4C0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9285-J1 地址空间耗尽注入判据过，全 ENOMEM 格过，COW 挂点依赖声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9286 setpgid 坏 pid ESRCH 矩阵
- 文件：`varix/src/unxstar/unx9286_matrix_setpgid_pid.rs`
- 函数：`pub fn run_unx9286_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B500;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9286-J1 坏 pid 注入判据过，全 ESRCH 格过，跨会话 EPERM 格过，与 B18 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9287 setsid EPERM 矩阵（会话首规则联 B18）
- 文件：`varix/src/unxstar/unx9287_matrix_setsid_eperm.rs`
- 函数：`pub fn run_unx9287_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B540;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9287-J1 会话首注入判据过，全 EPERM 格过，与 B18 选举语义对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9288 execve 错误矩阵（EACCES/ENOEXEC/E2BIG/EFAULT）
- 文件：`varix/src/unxstar/unx9288_matrix_execve_eacces.rs`
- 函数：`pub fn run_unx9288_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B580;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9288-J1 四错误码注入矩阵判据过，坏路径全 EFAULT 格过，非脚本非 ELF 全 ENOEXEC 格过，判序与 Linux 一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9289 参数长度 E2BIG 矩阵（argv/envp 上限）
- 文件：`varix/src/unxstar/unx9289_matrix_e2big_argv.rs`
- 函数：`pub fn run_unx9289_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B5C0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9289-J1 超限 argv 注入判据过，全 E2BIG 格过，单参数上限 MAX_ARG_STRLEN 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
