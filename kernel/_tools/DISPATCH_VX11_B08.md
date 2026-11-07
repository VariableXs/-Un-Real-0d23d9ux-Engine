# VX11-1105 派工卡 B08（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9266 | `unx9266_matrix_perm_eacces.rs` | 0x7B000 | 360 | 目录搜索权限 EACCES 矩阵（路径逐段） |
| 2 | UNX-F9267 | `unx9267_matrix_eperm_root.rs` | 0x7B040 | 340 | EPERM 特权操作矩阵（root-only 全集） |
| 3 | UNX-F9268 | `unx9268_matrix_cap_eperm.rs` | 0x7B080 | 360 | 能力位 CAP_* 与 EPERM 判定矩阵 |
| 4 | UNX-F9269 | `unx9269_matrix_erofs.rs` | 0x7B0C0 | 320 | 只读文件系统 EROFS 矩阵 |
| 5 | UNX-F9270 | `unx9270_matrix_ebusy_eexist.rs` | 0x7B100 | 320 | EBUSY/EEXIST 冲突矩阵（独占创建） |
| 6 | UNX-F9271 | `unx9271_matrix_eperm_b20.rs` | 0x7B140 | 320 | 身份翻转 EPERM 矩阵（联 B20 对账） |
| 7 | UNX-F9272 | `unx9272_matrix_ioctl_ioperm.rs` | 0x7B180 | 320 | ioctl ioperm/iopl 禁用格 EPERM 矩阵 |
| 8 | UNX-F9273 | `unx9273_matrix_rlimit_eperm.rs` | 0x7B1C0 | 320 | rlimit 硬限提升 EPERM 矩阵（联 B19 对账） |

## UNX-F9266 目录搜索权限 EACCES 矩阵（路径逐段）
- 文件：`varix/src/unxstar/unx9266_matrix_perm_eacces.rs`
- 函数：`pub fn run_unx9266_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B000;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9266-J1 路径逐段缺 x 注入判据过，全 EACCES 格过，错误点（首段缺失优先）判序格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9267 EPERM 特权操作矩阵（root-only 全集）
- 文件：`varix/src/unxstar/unx9267_matrix_eperm_root.rs`
- 函数：`pub fn run_unx9267_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B040;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9267-J1 root-only 操作全集注入判据过，非 root 全 EPERM 逐格检出，CAP 消费声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9268 能力位 CAP_* 与 EPERM 判定矩阵
- 文件：`varix/src/unxstar/unx9268_matrix_cap_eperm.rs`
- 函数：`pub fn run_unx9268_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B080;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9268-J1 guard::CAP_* 能力位逐位注入判据过，缺 CAP 全 EPERM 逐格检出，能力位与 uid 判序格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9269 只读文件系统 EROFS 矩阵
- 文件：`varix/src/unxstar/unx9269_matrix_erofs.rs`
- 函数：`pub fn run_unx9269_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B0C0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9269-J1 EROFS 注入判据过，写路径全 EROFS 逐格检出，与 EACCES 判序（先挂载只读后权限）格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9270 EBUSY/EEXIST 冲突矩阵（独占创建）
- 文件：`varix/src/unxstar/unx9270_matrix_ebusy_eexist.rs`
- 函数：`pub fn run_unx9270_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B100;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9270-J1 O_EXCL 冲突注入判据过，全 EEXIST 格过，EBUSY 独占冲突格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9271 身份翻转 EPERM 矩阵（联 B20 对账）
- 文件：`varix/src/unxstar/unx9271_matrix_eperm_b20.rs`
- 函数：`pub fn run_unx9271_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B140;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9271-J1 非特权提限注入判据过，全 EPERM 逐格检出，与 B20 身份矩阵零重复分层格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9272 ioctl ioperm/iopl 禁用格 EPERM 矩阵
- 文件：`varix/src/unxstar/unx9272_matrix_ioctl_ioperm.rs`
- 函数：`pub fn run_unx9272_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B180;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9272-J1 ioperm/iopl 注入判据过，全 EPERM 拒绝格过，与 B16 红线注记（禁用格）对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9273 rlimit 硬限提升 EPERM 矩阵（联 B19 对账）
- 文件：`varix/src/unxstar/unx9273_matrix_rlimit_eperm.rs`
- 函数：`pub fn run_unx9273_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7B1C0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9273-J1 硬限提升注入判据过，非特权全 EPERM 格过，与 B19 资源矩阵零重复分层格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
