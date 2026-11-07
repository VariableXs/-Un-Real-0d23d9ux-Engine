# VX11-1105 派工卡 B03（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9226 | `unx9226_charter_domain_enoent.rs` | 0x7A600 | 360 | ENOENT 判定域总纲（路径解析错误面） |
| 2 | UNX-F9227 | `unx9227_matrix_open_stat.rs` | 0x7A640 | 340 | open/stat/unlink ENOENT 矩阵 |
| 3 | UNX-F9228 | `unx9228_matrix_enoent_enotdir.rs` | 0x7A680 | 340 | 目录操作 ENOENT/ENOTDIR/EISDIR 矩阵 |
| 4 | UNX-F9229 | `unx9229_matrix_boundary_at_fdcwd.rs` | 0x7A6C0 | 320 | 相对路径与 AT_FDCWD 边界矩阵 |
| 5 | UNX-F9230 | `unx9230_matrix_dirfd_at_empty_path.rs` | 0x7A700 | 320 | dirfd 坏值矩阵（AT_EMPTY_PATH/AT_SYMLINK_FOLLOW） |
| 6 | UNX-F9231 | `unx9231_order_ebadf_enoent.rs` | 0x7A740 | 340 | EBADF vs ENOENT 判定次序协议（先 fd 后路径） |
| 7 | UNX-F9232 | `unx9232_race_fd_close.rs` | 0x7A780 | 340 | fd 复用竞态注入（close-then-use TOCTOU） |
| 8 | UNX-F9233 | `unx9233_err.rs` | 0x7A7C0 | 320 | 错误码返回路径探针（负值编码与 err 链路） |

## UNX-F9226 ENOENT 判定域总纲（路径解析错误面）
- 文件：`varix/src/unxstar/unx9226_charter_domain_enoent.rs`
- 函数：`pub fn run_unx9226_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A600;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9226-J1 路径解析错误面判据过，ENOENT 触发三态（末段缺失/中间段缺失/空组件）逐格落账，与 B1 VFS 冻结接口消费格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9227 open/stat/unlink ENOENT 矩阵
- 文件：`varix/src/unxstar/unx9227_matrix_open_stat.rs`
- 函数：`pub fn run_unx9227_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A640;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9227-J1 三 syscall 缺失路径注入矩阵判据过，全 ENOENT 逐格检出，O_CREAT 正判据（存在即打开）格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9228 目录操作 ENOENT/ENOTDIR/EISDIR 矩阵
- 文件：`varix/src/unxstar/unx9228_matrix_enoent_enotdir.rs`
- 函数：`pub fn run_unx9228_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A680;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9228-J1 三错误码注入矩阵判据过，目录尾段缺失全 ENOENT 格过，文件作目录全 ENOTDIR 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9229 相对路径与 AT_FDCWD 边界矩阵
- 文件：`varix/src/unxstar/unx9229_matrix_boundary_at_fdcwd.rs`
- 函数：`pub fn run_unx9229_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A6C0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9229-J1 AT_FDCWD 正判据判据过，坏 dirfd 全 EBADF 格过，绝对路径忽略 dirfd 语义格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9230 dirfd 坏值矩阵（AT_EMPTY_PATH/AT_SYMLINK_FOLLOW）
- 文件：`varix/src/unxstar/unx9230_matrix_dirfd_at_empty_path.rs`
- 函数：`pub fn run_unx9230_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A700;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9230-J1 两标志判据过，坏 dirfd 全 EBADF 格过，非目录 fd 全 ENOTDIR 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9231 EBADF vs ENOENT 判定次序协议（先 fd 后路径）
- 文件：`varix/src/unxstar/unx9231_order_ebadf_enoent.rs`
- 函数：`pub fn run_unx9231_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A740;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9231-J1 判序协议判据过，坏 fd 优先返回 EBADF 逐格检出，判序与 Linux 逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9232 fd 复用竞态注入（close-then-use TOCTOU）
- 文件：`varix/src/unxstar/unx9232_race_fd_close.rs`
- 函数：`pub fn run_unx9232_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A780;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9232-J1 close-then-use 竞态注入判据过，槽位复用窗口可观测格过，100 次注入零越权写格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9233 错误码返回路径探针（负值编码与 err 链路）
- 文件：`varix/src/unxstar/unx9233_err.rs`
- 函数：`pub fn run_unx9233_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7A7C0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9233-J1 负值编码探针判据过，-Errno 逐码编码一致格过，err 链路与 B13 编码器单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
