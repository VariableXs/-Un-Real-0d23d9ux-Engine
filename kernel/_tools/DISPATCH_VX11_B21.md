# VX11-1105 派工卡 B21（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9370 | `unx9370_cosign_freeze_list.rs` | 0x7CA00 | 460 | 号面冻结清单联签（F9600 冻结协议与 C3 台账互认） |
| 2 | UNX-F9371 | `unx9371_jointest_list_ltp.rs` | 0x7CA40 | 440 | LTP 子集清单联测（C2 号面覆盖率账） |
| 3 | UNX-F9372 | `unx9372_ktest_jointest_baseline.rs` | 0x7CA80 | 440 | 回归基线联测（ktest -f c2 与 C3 回归 runner 对接） |
| 4 | UNX-F9373 | `unx9373_jointest.rs` | 0x7CAC0 | 420 | 联测判据共享账（只写编号+一句话协议逐条） |
| 5 | UNX-F9374 | `unx9374_cosign_handoff_f9095.rs` | 0x7CB00 | 380 | 联签交接包（F9095/F9097 移交包升级版） |
| 6 | UNX-F9375 | `unx9375_declare_dedup_c2.rs` | 0x7CB40 | 320 | 联调防重声明（C2×C3 号面零冲突 grep） |
| 7 | UNX-F9376 | `unx9376_declare_deps_c3.rs` | 0x7CB80 | 320 | 联调上游依赖登记（C3 未 finalize 依赖声明） |
| 8 | UNX-F9377 | `unx9377_jointest_windows.rs` | 0x7CBC0 | 300 | 联调与 Windows 对照账联测面 |

## UNX-F9370 号面冻结清单联签（F9600 冻结协议与 C3 台账互认）
- 文件：`varix/src/unxstar/unx9370_cosign_freeze_list.rs`
- 函数：`pub fn run_unx9370_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CA00;`
- 判据原文：① 纯功能代码 ≥460 行；② 判据原文：UNX-F9370-J1 冻结协议联签判据过，F8801–F9600 号面冻结清单与 C3 台账互认可观测，零重编声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9371 LTP 子集清单联测（C2 号面覆盖率账）
- 文件：`varix/src/unxstar/unx9371_jointest_list_ltp.rs`
- 函数：`pub fn run_unx9371_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CA40;`
- 判据原文：① 纯功能代码 ≥440 行；② 判据原文：UNX-F9371-J1 覆盖率账联测判据过，C2 号面 LTP 子集覆盖率逐号落账，缺口清单登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9372 回归基线联测（ktest -f c2 与 C3 回归 runner 对接）
- 文件：`varix/src/unxstar/unx9372_ktest_jointest_baseline.rs`
- 函数：`pub fn run_unx9372_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CA80;`
- 判据原文：① 纯功能代码 ≥440 行；② 判据原文：UNX-F9372-J1 基线联测判据过，ktest -f c2 与 C3 runner 对接可观测，fail==0 判据联测格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9373 联测判据共享账（只写编号+一句话协议逐条）
- 文件：`varix/src/unxstar/unx9373_jointest.rs`
- 函数：`pub fn run_unx9373_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CAC0;`
- 判据原文：① 纯功能代码 ≥420 行；② 判据原文：UNX-F9373-J1 共享账判据过，逐条只写编号+一句话协议格过，跨界代写零发生声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9374 联签交接包（F9095/F9097 移交包升级版）
- 文件：`varix/src/unxstar/unx9374_cosign_handoff_f9095.rs`
- 函数：`pub fn run_unx9374_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CB00;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9374-J1 交接包升级判据过，B15 移交包逐项升级可观测，交接清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9375 联调防重声明（C2×C3 号面零冲突 grep）
- 文件：`varix/src/unxstar/unx9375_declare_dedup_c2.rs`
- 函数：`pub fn run_unx9375_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CB40;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9375-J1 防重判据过，四范围 grep 零冲突格过，条目名零命中格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9376 联调上游依赖登记（C3 未 finalize 依赖声明）
- 文件：`varix/src/unxstar/unx9376_declare_deps_c3.rs`
- 函数：`pub fn run_unx9376_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CB80;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9376-J1 依赖声明判据过，C3 上游未 finalize 显式登记格过，fake 开发路径格过（§52 防卡死规则一）；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9377 联调与 Windows 对照账联测面
- 文件：`varix/src/unxstar/unx9377_jointest_windows.rs`
- 函数：`pub fn run_unx9377_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CBC0;`
- 判据原文：① 纯功能代码 ≥300 行；② 判据原文：UNX-F9377-J1 对照账联测判据过，Windows 对照格与联测面对接可观测，差异账复用格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
