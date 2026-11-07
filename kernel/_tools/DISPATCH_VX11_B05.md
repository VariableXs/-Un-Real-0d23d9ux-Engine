# VX11-1105 派工卡 B05（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9242 | `unx9242_matrix_mmap_addr.rs` | 0x7AA00 | 360 | mmap 坏 addr 提示与坏映射探针矩阵 |
| 2 | UNX-F9243 | `unx9243_matrix_mmap_einval.rs` | 0x7AA40 | 360 | mmap 未对齐 EINVAL 矩阵 |
| 3 | UNX-F9244 | `unx9244_matrix_mmap_enomem.rs` | 0x7AA80 | 360 | mmap ENOMEM 矩阵（超限/无空间） |
| 4 | UNX-F9245 | `unx9245_matrix_munmap_mprotect.rs` | 0x7AAC0 | 360 | munmap/mprotect 坏区间矩阵 |
| 5 | UNX-F9246 | `unx9246_matrix_brk_einval.rs` | 0x7AB00 | 320 | brk 坏值矩阵（越下界 EINVAL） |
| 6 | UNX-F9247 | `unx9247_matrix_mremap_einval.rs` | 0x7AB40 | 340 | mremap 错误矩阵（EINVAL/ENOMEM/EPERM 联 B17） |
| 7 | UNX-F9248 | `unx9248_matrix_madvise_einval.rs` | 0x7AB80 | 340 | madvise 错误矩阵（EINVAL/ENOMEM 联 B17） |
| 8 | UNX-F9249 | `unx9249_matrix_boundary_declare.rs` | 0x7ABC0 | 340 | mlock/mprotect 权限边界矩阵（与 B24 分界声明） |

## UNX-F9242 mmap 坏 addr 提示与坏映射探针矩阵
- 文件：`varix/src/unxstar/unx9242_matrix_mmap_addr.rs`
- 函数：`pub fn run_unx9242_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AA00;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9242-J1 mmap addr 非法提示值注入判据过，提示值零副作用正判据格过，映射后访问坏区间 SIGSEGV 路径格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9243 mmap 未对齐 EINVAL 矩阵
- 文件：`varix/src/unxstar/unx9243_matrix_mmap_einval.rs`
- 函数：`pub fn run_unx9243_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AA40;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9243-J1 非页对齐 addr/length 注入矩阵判据过，全 EINVAL 逐格检出（MAP_FIXED 时全 EINVAL），页掩码对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9244 mmap ENOMEM 矩阵（超限/无空间）
- 文件：`varix/src/unxstar/unx9244_matrix_mmap_enomem.rs`
- 函数：`pub fn run_unx9244_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AA80;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9244-J1 地址空间耗尽注入判据过，全 ENOMEM 格过，max_map_count 上限格过，与 RLIMIT_AS 联动格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9245 munmap/mprotect 坏区间矩阵
- 文件：`varix/src/unxstar/unx9245_matrix_munmap_mprotect.rs`
- 函数：`pub fn run_unx9245_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AAC0;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9245-J1 未对齐区间注入判据过，munmap 全 EINVAL 格过，mprotect 坏 prot 全 EINVAL 格过，区间不存在全 ENOMEM 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9246 brk 坏值矩阵（越下界 EINVAL）
- 文件：`varix/src/unxstar/unx9246_matrix_brk_einval.rs`
- 函数：`pub fn run_unx9246_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AB00;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9246-J1 低于初始堆底注入判据过，全 EINVAL 格过，返回新 break 正判据格过，与 B07 brk 档对账一致；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9247 mremap 错误矩阵（EINVAL/ENOMEM/EPERM 联 B17）
- 文件：`varix/src/unxstar/unx9247_matrix_mremap_einval.rs`
- 函数：`pub fn run_unx9247_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AB40;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9247-J1 三错误码注入矩阵判据过，非对齐 old_addr 全 EINVAL 格过，与 B17 矩阵零重复分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9248 madvise 错误矩阵（EINVAL/ENOMEM 联 B17）
- 文件：`varix/src/unxstar/unx9248_matrix_madvise_einval.rs`
- 函数：`pub fn run_unx9248_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AB80;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9248-J1 未知 advice 全 EINVAL 判据过，越界区间全 ENOMEM 格过，与 B17 矩阵零重复分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9249 mlock/mprotect 权限边界矩阵（与 B24 分界声明）
- 文件：`varix/src/unxstar/unx9249_matrix_boundary_declare.rs`
- 函数：`pub fn run_unx9249_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7ABC0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9249-J1 锁页越限全 EPERM 判据过，与 B24 权限矩阵分界声明格过，零重复判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
