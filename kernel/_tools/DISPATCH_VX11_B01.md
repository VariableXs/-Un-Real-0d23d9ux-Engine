# VX11-1105 派工卡 B01（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9210 | `unx9210_zerolen_matrix_boundary.rs` | 0x7A200 | 300 | 文件族零长度读写边界矩阵 |
| 2 | UNX-F9211 | `unx9211_badfd_sweep_global_fd.rs` | 0x7A240 | 340 | 坏 fd 注入跨号段扫描（fd 空间全域） |
| 3 | UNX-F9212 | `unx9212_efault_halfcopy_copy_to_user.rs` | 0x7A280 | 340 | EFAULT 拷贝半程探针（copy_to_user 半途） |
| 4 | UNX-F9213 | `unx9213_errno_encoder_matrix_errno.rs` | 0x7A2C0 | 340 | 错误码编码器一致性矩阵（Errno→sysret 逐码） |
| 5 | UNX-F9214 | `unx9214_eprobe_suite_file.rs` | 0x7A300 | 300 | 文件族 E 型参数探针总成 |
| 6 | UNX-F9215 | `unx9215_etype_vs_reconcile_file.rs` | 0x7A340 | 320 | 文件族 E 型与 B03–B05 深化册对账格 |
| 7 | UNX-F9216 | `unx9216_etype_vs_compare_file.rs` | 0x7A380 | 300 | 文件族 E 型与 Windows GetLastError 对照格 |
| 8 | UNX-F9217 | `unx9217_etype_recheck_declare.rs` | 0x7A3C0 | 300 | E 型判据可复测性声明（10/10 次检出） |

## UNX-F9210 文件族零长度读写边界矩阵
- 文件：`varix/src/unxstar/unx9210_zerolen_matrix_boundary.rs`
- 函数：`pub fn run_unx9210_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A200;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9210-J1 零长度 read/write 正判据（零副作用返回 0）判据过，反向判序（坏 fd 优先于零长度）格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9211 坏 fd 注入跨号段扫描（fd 空间全域）
- 文件：`varix/src/unxstar/unx9211_badfd_sweep_global_fd.rs`
- 函数：`pub fn run_unx9211_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A240;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9211-J1 fd 空间全域扫描注入判据过，未开槽位全 EBADF 逐格检出，越上限与 RLIMIT_NOFILE 分界格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9212 EFAULT 拷贝半程探针（copy_to_user 半途）
- 文件：`varix/src/unxstar/unx9212_efault_halfcopy_copy_to_user.rs`
- 函数：`pub fn run_unx9212_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A280;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9212-J1 半途拷贝注入判据过，已拷贝字节数返回语义与 Linux 一致格过，半程残留可观测格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9213 错误码编码器一致性矩阵（Errno→sysret 逐码）
- 文件：`varix/src/unxstar/unx9213_errno_encoder_matrix_errno.rs`
- 函数：`pub fn run_unx9213_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A2C0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9213-J1 Errno 全集逐码注入判据过，sysret 编码逐码一致格过，与 B13 映射账零漂移格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9214 文件族 E 型参数探针总成
- 文件：`varix/src/unxstar/unx9214_eprobe_suite_file.rs`
- 函数：`pub fn run_unx9214_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A300;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9214-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针总成单源；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9215 文件族 E 型与 B03–B05 深化册对账格
- 文件：`varix/src/unxstar/unx9215_etype_vs_reconcile_file.rs`
- 函数：`pub fn run_unx9215_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A340;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9215-J1 对账判据过，与 open/close/lseek/stat 深化册逐条判据零冲突，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9216 文件族 E 型与 Windows GetLastError 对照格
- 文件：`varix/src/unxstar/unx9216_etype_vs_compare_file.rs`
- 函数：`pub fn run_unx9216_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A380;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9216-J1 ERROR_INVALID_HANDLE/ERROR_NOACCESS 映射表判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9217 E 型判据可复测性声明（10/10 次检出）
- 文件：`varix/src/unxstar/unx9217_etype_recheck_declare.rs`
- 函数：`pub fn run_unx9217_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A3C0;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9217-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
