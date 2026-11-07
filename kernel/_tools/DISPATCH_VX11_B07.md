# VX11-1105 派工卡 B07（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9258 | `unx9258_etype_takeover_note_mem.rs` | 0x7AE00 | 280 | 内存族 E 型升级接管注记 |
| 2 | UNX-F9259 | `unx9259_etype_vs_consumer_mem.rs` | 0x7AE40 | 340 | 内存族 E 型与 glibc malloc 失败路径消费核 |
| 3 | UNX-F9260 | `unx9260_ktest_syscall_b23.rs` | 0x7AE80 | 380 | ktest syscall 面 B23 批断言集 |
| 4 | UNX-F9261 | `unx9261_charter_domain_perm.rs` | 0x7AEC0 | 420 | 权限错误面总纲（EPERM vs EACCES 判定域） |
| 5 | UNX-F9262 | `unx9262_matrix_open_eacces.rs` | 0x7AF00 | 360 | open EACCES 矩阵（rwx 三位逐格） |
| 6 | UNX-F9263 | `unx9263_matrix_chmod_chown.rs` | 0x7AF40 | 340 | chmod/chown EPERM 矩阵（非属主） |
| 7 | UNX-F9264 | `unx9264_matrix_perm_st_mode.rs` | 0x7AF80 | 380 | 权限位模型（st_mode 九位逐格矩阵） |
| 8 | UNX-F9265 | `unx9265_matrix_setuid_setgid.rs` | 0x7AFC0 | 340 | setuid/setgid 文件位与执行路径矩阵 |

## UNX-F9258 内存族 E 型升级接管注记
- 文件：`varix/src/unxstar/unx9258_etype_takeover_note_mem.rs`
- 函数：`pub fn run_unx9258_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AE00;`
- 判据原文：① 纯功能代码 ≥280 行；② 判据原文：UNX-F9258-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9259 内存族 E 型与 glibc malloc 失败路径消费核
- 文件：`varix/src/unxstar/unx9259_etype_vs_consumer_mem.rs`
- 函数：`pub fn run_unx9259_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AE40;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9259-J1 malloc 返回 NULL 路径判据过，ENOMEM 透传可观测格过，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9260 ktest syscall 面 B23 批断言集
- 文件：`varix/src/unxstar/unx9260_ktest_syscall_b23.rs`
- 函数：`pub fn run_unx9260_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AE80;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9260-J1 本批 19 条判据聚合判据过，内存族 E 型矩阵一次跑通，域累计 119,200 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9261 权限错误面总纲（EPERM vs EACCES 判定域）
- 文件：`varix/src/unxstar/unx9261_charter_domain_perm.rs`
- 函数：`pub fn run_unx9261_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AEC0;`
- 判据原文：① 纯功能代码 ≥420 行；② 判据原文：UNX-F9261-J1 两错误码判定域判据过，EPERM（操作特权缺失）与 EACCES（对象权限不足）分界逐格落账，与 Linux 语义逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9262 open EACCES 矩阵（rwx 三位逐格）
- 文件：`varix/src/unxstar/unx9262_matrix_open_eacces.rs`
- 函数：`pub fn run_unx9262_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AF00;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9262-J1 rwx 三位逐格注入判据过，缺 r 读全 EACCES 格过，缺 w 写全 EACCES 格过，缺 x 执行全 EACCES 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9263 chmod/chown EPERM 矩阵（非属主）
- 文件：`varix/src/unxstar/unx9263_matrix_chmod_chown.rs`
- 函数：`pub fn run_unx9263_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AF40;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9263-J1 非属主注入判据过，全 EPERM 格过，特权路径正判据格过，与 B05 三身份档对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9264 权限位模型（st_mode 九位逐格矩阵）
- 文件：`varix/src/unxstar/unx9264_matrix_perm_st_mode.rs`
- 函数：`pub fn run_unx9264_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AF80;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9264-J1 九位逐格矩阵判据过，ugo×rwx 逐格可观测，S_IFMT 类型位掩码格过，与 stat 结构单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9265 setuid/setgid 文件位与执行路径矩阵
- 文件：`varix/src/unxstar/unx9265_matrix_setuid_setgid.rs`
- 函数：`pub fn run_unx9265_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7AFC0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9265-J1 s 位语义挂点判据过，目录 setgid 位继承挂点格过，实装归 C1/B1 联签声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
