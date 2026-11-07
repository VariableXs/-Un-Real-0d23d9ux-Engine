# VX11-1105 派工卡 B11（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9290 | `unx9290_matrix_boundary_execve.rs` | 0x7B600 | 300 | 零参数 execve 边界矩阵 |
| 2 | UNX-F9291 | `unx9291_race_wait_exit.rs` | 0x7B640 | 340 | 僵尸收割竞态注入（wait vs exit 竞态） |
| 3 | UNX-F9292 | `unx9292_race_sigchld.rs` | 0x7B680 | 340 | 进程退出竞态注入（SIGCHLD 投递竞态） |
| 4 | UNX-F9293 | `unx9293_order_perm_esrch.rs` | 0x7B6C0 | 320 | ESRCH 判定次序协议（pid 存在性先于权限） |
| 5 | UNX-F9294 | `unx9294_eprobe_suite_proc.rs` | 0x7B700 | 300 | 进程族 E 型参数探针总成 |
| 6 | UNX-F9295 | `unx9295_etype_vs_reconcile_proc.rs` | 0x7B740 | 300 | 进程族 E 型与 B04/B08 深化册对账格 |
| 7 | UNX-F9296 | `unx9296_etype_vs_compare_proc.rs` | 0x7B780 | 300 | 进程族 E 型与 Windows 进程句柄错误对照格 |
| 8 | UNX-F9297 | `unx9297_etype_recheck_declare.rs` | 0x7B7C0 | 300 | E 型判据可复测性声明（10/10 次检出） |

## UNX-F9290 零参数 execve 边界矩阵
- 文件：`varix/src/unxstar/unx9290_matrix_boundary_execve.rs`
- 函数：`pub fn run_unx9290_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B600;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9290-J1 空 argv 正判据（argv[0] 可空）判据过，空 envp 正判据格过，坏指针优先判序格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9291 僵尸收割竞态注入（wait vs exit 竞态）
- 文件：`varix/src/unxstar/unx9291_race_wait_exit.rs`
- 函数：`pub fn run_unx9291_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B640;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9291-J1 wait-exit 竞态注入判据过，单次收割唯一性 100 次统计格过，双 wait 第二次全 ECHILD 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9292 进程退出竞态注入（SIGCHLD 投递竞态）
- 文件：`varix/src/unxstar/unx9292_race_sigchld.rs`
- 函数：`pub fn run_unx9292_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B680;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9292-J1 SIGCHLD 投递竞态注入判据过，投递不丢失 100 次统计格过，与 B11 链对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9293 ESRCH 判定次序协议（pid 存在性先于权限）
- 文件：`varix/src/unxstar/unx9293_order_perm_esrch.rs`
- 函数：`pub fn run_unx9293_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B6C0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9293-J1 判序协议判据过，不存在 pid 全 ESRCH 逐格检出（先于 EPERM），与 Linux 逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9294 进程族 E 型参数探针总成
- 文件：`varix/src/unxstar/unx9294_eprobe_suite_proc.rs`
- 函数：`pub fn run_unx9294_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B700;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9294-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9295 进程族 E 型与 B04/B08 深化册对账格
- 文件：`varix/src/unxstar/unx9295_etype_vs_reconcile_proc.rs`
- 函数：`pub fn run_unx9295_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B740;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9295-J1 对账判据过，与 fork 档/进程族地基深化册逐条零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9296 进程族 E 型与 Windows 进程句柄错误对照格
- 文件：`varix/src/unxstar/unx9296_etype_vs_compare_proc.rs`
- 函数：`pub fn run_unx9296_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B780;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9296-J1 ERROR_INVALID_PARAMETER 映射判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9297 E 型判据可复测性声明（10/10 次检出）
- 文件：`varix/src/unxstar/unx9297_etype_recheck_declare.rs`
- 函数：`pub fn run_unx9297_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B7C0;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9297-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
