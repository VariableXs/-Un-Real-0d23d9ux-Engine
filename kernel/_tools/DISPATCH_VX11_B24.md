# VX11-1105 派工卡 B24（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9394 | `unx9394_declare_dedup_c2.rs` | 0x7D000 | 320 | 联调防重声明（C2×C4 号面零冲突 grep） |
| 2 | UNX-F9395 | `unx9395_declare_deps_c4.rs` | 0x7D040 | 320 | 联调上游依赖登记（C4 未 finalize 依赖声明） |
| 3 | UNX-F9396 | `unx9396_windows_waitabletimer_iocp.rs` | 0x7D080 | 280 | 联调与 Windows WaitableTimer/IOCP 对照账 |
| 4 | UNX-F9397 | `unx9397_consumer_glibc_pthread.rs` | 0x7D0C0 | 320 | 联调与 glibc pthread futex 消费核 |
| 5 | UNX-F9398 | `unx9398_declare.rs` | 0x7D100 | 280 | I 型联调判据可复测性声明 |
| 6 | UNX-F9399 | `unx9399_jointest_list_glibc.rs` | 0x7D140 | 320 | I 型与 glibc 消费清单逐号核联测 |
| 7 | UNX-F9400 | `unx9400_ktest_closure_syscall.rs` | 0x7D180 | 460 | ktest syscall 面 B30 批断言集 + B16–B30 半域收口断言 |
| 8 | UNX-F9401 | `unx9401_charter_cosign_c2.rs` | 0x7D1C0 | 400 | I 型联调总纲三（C2×C3 联签协议与对账面） |

## UNX-F9394 联调防重声明（C2×C4 号面零冲突 grep）
- 文件：`varix/src/unxstar/unx9394_declare_dedup_c2.rs`
- 函数：`pub fn run_unx9394_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7D000;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9394-J1 防重判据过，四范围 grep 零冲突格过，条目名零命中格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9395 联调上游依赖登记（C4 未 finalize 依赖声明）
- 文件：`varix/src/unxstar/unx9395_declare_deps_c4.rs`
- 函数：`pub fn run_unx9395_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7D040;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9395-J1 依赖声明判据过，C4 上游未 finalize 显式登记格过，fake 开发路径格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9396 联调与 Windows WaitableTimer/IOCP 对照账
- 文件：`varix/src/unxstar/unx9396_windows_waitabletimer_iocp.rs`
- 函数：`pub fn run_unx9396_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7D080;`
- 判据原文：① 纯功能代码 ≥280 行；② 判据原文：UNX-F9396-J1 对照账判据过，WaitableTimer/IOCP 映射表落账，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9397 联调与 glibc pthread futex 消费核
- 文件：`varix/src/unxstar/unx9397_consumer_glibc_pthread.rs`
- 函数：`pub fn run_unx9397_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7D0C0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9397-J1 消费核联测判据过，pthread futex 消费链对接可观测，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9398 I 型联调判据可复测性声明
- 文件：`varix/src/unxstar/unx9398_declare.rs`
- 函数：`pub fn run_unx9398_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7D100;`
- 判据原文：① 纯功能代码 ≥280 行；② 判据原文：UNX-F9398-J1 联测判据 10 次重复判据过，10/10 次通过统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9399 I 型与 glibc 消费清单逐号核联测
- 文件：`varix/src/unxstar/unx9399_jointest_list_glibc.rs`
- 函数：`pub fn run_unx9399_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7D140;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9399-J1 逐号核联测判据过，消费清单逐号联测可观测，零号面冲突格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9400 ktest syscall 面 B30 批断言集 + B16–B30 半域收口断言
- 文件：`varix/src/unxstar/unx9400_ktest_closure_syscall.rs`
- 函数：`pub fn run_unx9400_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7D180;`
- 判据原文：① 纯功能代码 ≥460 行；② 判据原文：UNX-F9400-J1 本批 19 条判据聚合判据过，B16–B30 十五批半域收口一次跑通断言过，域累计 169,000 收口断言（B31–B40 预算 71,000 待领，域账守恒 240,000）；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9401 I 型联调总纲三（C2×C3 联签协议与对账面）
- 文件：`varix/src/unxstar/unx9401_charter_cosign_c2.rs`
- 函数：`pub fn run_unx9401_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7D1C0;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9401-J1 联签协议判据过，C2（错误矩阵面）×C3（errno 账本面）双方对账面互认格过，跨界代写禁令声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
