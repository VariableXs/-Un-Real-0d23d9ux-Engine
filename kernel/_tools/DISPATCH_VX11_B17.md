# VX11-1105 派工卡 B17（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9338 | `unx9338_etype_takeover_note_timeres.rs` | 0x7C200 | 280 | 时间/资源族 E 型升级接管注记 |
| 2 | UNX-F9339 | `unx9339_etype_vs_consumer_timeres.rs` | 0x7C240 | 340 | 时间/资源族 E 型与 glibc clock 家族消费核 |
| 3 | UNX-F9340 | `unx9340_ktest_syscall_b27.rs` | 0x7C280 | 440 | ktest syscall 面 B27 批断言集 |
| 4 | UNX-F9341 | `unx9341_charter_matrix_assembly.rs` | 0x7C2C0 | 420 | E 型总装总纲（全错误码矩阵收口协议） |
| 5 | UNX-F9342 | `unx9342_matrix_assembly_global.rs` | 0x7C300 | 400 | 全域 EFAULT 矩阵总装（跨五族逐格） |
| 6 | UNX-F9343 | `unx9343_matrix_assembly_global.rs` | 0x7C340 | 380 | 全域 EBADF 矩阵总装 |
| 7 | UNX-F9344 | `unx9344_matrix_assembly_global.rs` | 0x7C380 | 380 | 全域 EINVAL 矩阵总装 |
| 8 | UNX-F9345 | `unx9345_matrix_assembly_global.rs` | 0x7C3C0 | 380 | 全域 EPERM/EACCES 矩阵总装 |

## UNX-F9338 时间/资源族 E 型升级接管注记
- 文件：`varix/src/unxstar/unx9338_etype_takeover_note_timeres.rs`
- 函数：`pub fn run_unx9338_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C200;`
- 判据原文：① 纯功能代码 ≥280 行；② 判据原文：UNX-F9338-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9339 时间/资源族 E 型与 glibc clock 家族消费核
- 文件：`varix/src/unxstar/unx9339_etype_vs_consumer_timeres.rs`
- 函数：`pub fn run_unx9339_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C240;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9339-J1 clock_gettime errno 透传判据过，失效时钟全 EINVAL 可观测，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9340 ktest syscall 面 B27 批断言集
- 文件：`varix/src/unxstar/unx9340_ktest_syscall_b27.rs`
- 函数：`pub fn run_unx9340_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C280;`
- 判据原文：① 纯功能代码 ≥440 行；② 判据原文：UNX-F9340-J1 本批 19 条判据聚合判据过，时间资源族 E 型矩阵一次跑通，域累计 146,800 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9341 E 型总装总纲（全错误码矩阵收口协议）
- 文件：`varix/src/unxstar/unx9341_charter_matrix_assembly.rs`
- 函数：`pub fn run_unx9341_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C2C0;`
- 判据原文：① 纯功能代码 ≥420 行；② 判据原文：UNX-F9341-J1 八批错误矩阵收口协议判据过，全错误码账逐码落账，收口判序（EFAULT>EBADF>EINVAL>EPERM）声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9342 全域 EFAULT 矩阵总装（跨五族逐格）
- 文件：`varix/src/unxstar/unx9342_matrix_assembly_global.rs`
- 函数：`pub fn run_unx9342_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C300;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9342-J1 五族坏指针逐格总装判据过，全 EFAULT 逐格检出，拷贝方向（in/out）双向逐格格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9343 全域 EBADF 矩阵总装
- 文件：`varix/src/unxstar/unx9343_matrix_assembly_global.rs`
- 函数：`pub fn run_unx9343_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C340;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9343-J1 五族坏 fd 逐格总装判据过，全 EBADF 逐格检出，fd 空间全域扫描格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9344 全域 EINVAL 矩阵总装
- 文件：`varix/src/unxstar/unx9344_matrix_assembly_global.rs`
- 函数：`pub fn run_unx9344_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C380;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9344-J1 五族非法参数逐格总装判据过，全 EINVAL 逐格检出，与 Linux 逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9345 全域 EPERM/EACCES 矩阵总装
- 文件：`varix/src/unxstar/unx9345_matrix_assembly_global.rs`
- 函数：`pub fn run_unx9345_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C3C0;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9345-J1 五族权限注入逐格总装判据过，EPERM/EACCES 分界逐格检出，与 B24 判定域对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
