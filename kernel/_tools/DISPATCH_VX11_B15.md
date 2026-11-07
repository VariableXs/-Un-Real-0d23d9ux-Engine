# VX11-1105 派工卡 B15（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9322 | `unx9322_matrix_gettimeofday_clock_gettim.rs` | 0x7BE00 | 380 | gettimeofday/clock_gettime 坏指针矩阵（EFAULT） |
| 2 | UNX-F9323 | `unx9323_matrix_clock_settime_einval.rs` | 0x7BE40 | 360 | clock_settime 非法时钟 EINVAL 矩阵 |
| 3 | UNX-F9324 | `unx9324_matrix_timer_create_timer_settim.rs` | 0x7BE80 | 380 | timer_create/timer_settime 错误矩阵（EINVAL/EAGAIN） |
| 4 | UNX-F9325 | `unx9325_matrix_posix_eagain.rs` | 0x7BEC0 | 360 | POSIX 定时器耗尽 EAGAIN 矩阵（RLIMIT_SIGPENDING 联动） |
| 5 | UNX-F9326 | `unx9326_matrix_setitimer.rs` | 0x7BF00 | 320 | setitimer 非法间隔矩阵 |
| 6 | UNX-F9327 | `unx9327_matrix_clock_nanosleep.rs` | 0x7BF40 | 360 | clock_nanosleep 非法标志/时钟矩阵 |
| 7 | UNX-F9328 | `unx9328_matrix_getrusage_b15.rs` | 0x7BF80 | 340 | getrusage 坏指针矩阵（联 B15/B19 对账） |
| 8 | UNX-F9329 | `unx9329_matrix_setrlimit.rs` | 0x7BFC0 | 340 | setrlimit 坏结构指针矩阵 |

## UNX-F9322 gettimeofday/clock_gettime 坏指针矩阵（EFAULT）
- 文件：`varix/src/unxstar/unx9322_matrix_gettimeofday_clock_gettim.rs`
- 函数：`pub fn run_unx9322_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BE00;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9322-J1 坏指针注入判据过，全 EFAULT 格过，10/10 次检出格过，与 B06 时间族对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9323 clock_settime 非法时钟 EINVAL 矩阵
- 文件：`varix/src/unxstar/unx9323_matrix_clock_settime_einval.rs`
- 函数：`pub fn run_unx9323_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BE40;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9323-J1 非法 clockid 注入判据过，全 EINVAL 格过，CLOCK_REALTIME 正判据格过，特权检查挂点格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9324 timer_create/timer_settime 错误矩阵（EINVAL/EAGAIN）
- 文件：`varix/src/unxstar/unx9324_matrix_timer_create_timer_settim.rs`
- 函数：`pub fn run_unx9324_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BE80;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9324-J1 两错误码注入矩阵判据过，非法 sigevent 全 EINVAL 格过，定时器耗尽全 EAGAIN 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9325 POSIX 定时器耗尽 EAGAIN 矩阵（RLIMIT_SIGPENDING 联动）
- 文件：`varix/src/unxstar/unx9325_matrix_posix_eagain.rs`
- 函数：`pub fn run_unx9325_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BEC0;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9325-J1 定时器上限注入判据过，全 EAGAIN 格过，与 B19 SIGPENDING 联动格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9326 setitimer 非法间隔矩阵
- 文件：`varix/src/unxstar/unx9326_matrix_setitimer.rs`
- 函数：`pub fn run_unx9326_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BF00;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9326-J1 负值/超上界注入判据过，全 EINVAL 格过，ITIMER_REAL 三类正判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9327 clock_nanosleep 非法标志/时钟矩阵
- 文件：`varix/src/unxstar/unx9327_matrix_clock_nanosleep.rs`
- 函数：`pub fn run_unx9327_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BF40;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9327-J1 非法标志注入判据过，全 EINVAL/ENOTSUP 逐格检出，非法时钟全 EINVAL 格过，与 B20 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9328 getrusage 坏指针矩阵（联 B15/B19 对账）
- 文件：`varix/src/unxstar/unx9328_matrix_getrusage_b15.rs`
- 函数：`pub fn run_unx9328_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BF80;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9328-J1 坏指针注入判据过，全 EFAULT 格过，非法 who 全 EINVAL 格过，与 B19 对账零重复格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9329 setrlimit 坏结构指针矩阵
- 文件：`varix/src/unxstar/unx9329_matrix_setrlimit.rs`
- 函数：`pub fn run_unx9329_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7BFC0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9329-J1 坏 rlim 指针注入判据过，全 EFAULT 格过，10/10 次检出格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
