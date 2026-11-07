# VX11-1105 派工卡 B23（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9386 | `unx9386_jointest_poll_select.rs` | 0x7CE00 | 380 | 多路复用四件套联测（poll/select/ppoll/pselect × C4 事件面） |
| 2 | UNX-F9387 | `unx9387_cosign_hook_pty.rs` | 0x7CE40 | 340 | pty 缺席 ENOTTY 联签（B16 终端挂点 × C4 pty 面） |
| 3 | UNX-F9388 | `unx9388_cosign_hook_b11.rs` | 0x7CE80 | 360 | 信号挂点联签（B11/B12 挂点 × C4 futex 唤醒链） |
| 4 | UNX-F9389 | `unx9389_jointest_a3_fake.rs` | 0x7CEC0 | 340 | 阻塞唤醒原语联测（A3 fake × C4 等待队列） |
| 5 | UNX-F9390 | `unx9390_race_jointest_stats.rs` | 0x7CF00 | 380 | futex 竞态注入联测（唤醒丢失窗 100 次统计） |
| 6 | UNX-F9391 | `unx9391_matrix_jointest_hook.rs` | 0x7CF40 | 340 | EINTR 重启联测（B26 矩阵 × C4 挂点） |
| 7 | UNX-F9392 | `unx9392_cosign_freeze_c2.rs` | 0x7CF80 | 360 | 号面冻结互认联签（C2 F9400 冻结 × C4 台账） |
| 8 | UNX-F9393 | `unx9393_cosign_freeze_list.rs` | 0x7CFC0 | 320 | 联签交接包（F9400 冻结面移交清单） |

## UNX-F9386 多路复用四件套联测（poll/select/ppoll/pselect × C4 事件面）
- 文件：`varix/src/unxstar/unx9386_jointest_poll_select.rs`
- 函数：`pub fn run_unx9386_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CE00;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9386-J1 四件套联测判据过，逐件与 C4 事件面对接可观测，timeout 语义逐件一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9387 pty 缺席 ENOTTY 联签（B16 终端挂点 × C4 pty 面）
- 文件：`varix/src/unxstar/unx9387_cosign_hook_pty.rs`
- 函数：`pub fn run_unx9387_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CE40;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9387-J1 缺席联签判据过，pty 缺席全 ENOTTY 逐格联测可观测，C4 pty 上线后翻转挂点声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9388 信号挂点联签（B11/B12 挂点 × C4 futex 唤醒链）
- 文件：`varix/src/unxstar/unx9388_cosign_hook_b11.rs`
- 函数：`pub fn run_unx9388_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CE80;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9388-J1 唤醒链联签判据过，信号到达唤醒 futex 等待逐格联测可观测，零丢唤醒声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9389 阻塞唤醒原语联测（A3 fake × C4 等待队列）
- 文件：`varix/src/unxstar/unx9389_jointest_a3_fake.rs`
- 函数：`pub fn run_unx9389_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CEC0;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9389-J1 原语联测判据过，A3 fake 原语与 C4 等待队列对接可观测，Schema 先行声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9390 futex 竞态注入联测（唤醒丢失窗 100 次统计）
- 文件：`varix/src/unxstar/unx9390_race_jointest_stats.rs`
- 函数：`pub fn run_unx9390_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CF00;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9390-J1 竞态联测判据过，唤醒丢失窗 100 次注入统计格过，零丢失声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9391 EINTR 重启联测（B26 矩阵 × C4 挂点）
- 文件：`varix/src/unxstar/unx9391_matrix_jointest_hook.rs`
- 函数：`pub fn run_unx9391_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CF40;`
- 判据原文：① 纯功能代码 ≥340 行；② 判据原文：UNX-F9391-J1 重启联测判据过，B26 EINTR 矩阵逐格与 C4 挂点联测可观测，SA_RESTART 语义一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9392 号面冻结互认联签（C2 F9400 冻结 × C4 台账）
- 文件：`varix/src/unxstar/unx9392_cosign_freeze_c2.rs`
- 函数：`pub fn run_unx9392_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CF80;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9392-J1 互认联签判据过，F9101–F9400 号面冻结与 C4 台账互认可观测，零重编声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9393 联签交接包（F9400 冻结面移交清单）
- 文件：`varix/src/unxstar/unx9393_cosign_freeze_list.rs`
- 函数：`pub fn run_unx9393_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7CFC0;`
- 判据原文：① 纯功能代码 ≥320 行；② 判据原文：UNX-F9393-J1 交接包判据过，F9400 冻结面移交清单逐项落账，接手会话就绪格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
