# VX11-1105 派工卡 B06（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9250 | `unx9250_matrix_sigsegv.rs` | 0x7AC00 | 340 | 地址空间越界访问 SIGSEGV 路径矩阵 |
| 2 | UNX-F9251 | `unx9251_matrix_boundary.rs` | 0x7AC40 | 340 | 页对齐边界值矩阵（4K/2M 大页） |
| 3 | UNX-F9252 | `unx9252_matrix_boundary.rs` | 0x7AC80 | 320 | 零长度映射边界矩阵 |
| 4 | UNX-F9253 | `unx9253_matrix_map_fixed_enoent.rs` | 0x7ACC0 | 340 | MAP_FIXED 冲突矩阵（ENOENT/EEXIST） |
| 5 | UNX-F9254 | `unx9254_eprobe_suite_mem.rs` | 0x7AD00 | 320 | 内存族 E 型参数探针总成 |
| 6 | UNX-F9255 | `unx9255_etype_vs_reconcile_mem.rs` | 0x7AD40 | 320 | 内存族 E 型与 B06/B07 深化册对账格 |
| 7 | UNX-F9256 | `unx9256_etype_vs_compare_mem.rs` | 0x7AD80 | 320 | 内存族 E 型与 Windows VirtualAlloc 错误码对照格 |
| 8 | UNX-F9257 | `unx9257_etype_recheck_declare.rs` | 0x7ADC0 | 320 | E 型判据可复测性声明（10/10 次检出） |

## UNX-F9250 地址空间越界访问 SIGSEGV 路径矩阵
- 文件：`varix/src/unxstar/unx9250_matrix_sigsegv.rs`
- 函数：`pub fn run_unx9250_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AC00;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9250-J1 越界读写触发 #PF 判据过，SIGSEGV 投递可观测格过，si_addr 逐格正确格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9251 页对齐边界值矩阵（4K/2M 大页）
- 文件：`varix/src/unxstar/unx9251_matrix_boundary.rs`
- 函数：`pub fn run_unx9251_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AC40;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9251-J1 4K 页界逐位注入判据过，2M 大页边界格过，跨界映射拒绝语义格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9252 零长度映射边界矩阵
- 文件：`varix/src/unxstar/unx9252_matrix_boundary.rs`
- 函数：`pub fn run_unx9252_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AC80;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9252-J1 零长度 mmap 正判据（返回合法地址零副作用）判据过，零长度 munmap 全 EINVAL 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9253 MAP_FIXED 冲突矩阵（ENOENT/EEXIST）
- 文件：`varix/src/unxstar/unx9253_matrix_map_fixed_enoent.rs`
- 函数：`pub fn run_unx9253_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7ACC0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9253-J1 MAP_FIXED 覆盖语义判据过，与现存映射冲突覆盖可观测格过，MAP_FIXED_NOREPLACE 全 EEXIST 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9254 内存族 E 型参数探针总成
- 文件：`varix/src/unxstar/unx9254_eprobe_suite_mem.rs`
- 函数：`pub fn run_unx9254_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AD00;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9254-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9255 内存族 E 型与 B06/B07 深化册对账格
- 文件：`varix/src/unxstar/unx9255_etype_vs_reconcile_mem.rs`
- 函数：`pub fn run_unx9255_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AD40;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9255-J1 对账判据过，与 mmap 三联/brk 档深化册逐条判据零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9256 内存族 E 型与 Windows VirtualAlloc 错误码对照格
- 文件：`varix/src/unxstar/unx9256_etype_vs_compare_mem.rs`
- 函数：`pub fn run_unx9256_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AD80;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9256-J1 ERROR_INVALID_ADDRESS 映射判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9257 E 型判据可复测性声明（10/10 次检出）
- 文件：`varix/src/unxstar/unx9257_etype_recheck_declare.rs`
- 函数：`pub fn run_unx9257_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7ADC0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9257-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
