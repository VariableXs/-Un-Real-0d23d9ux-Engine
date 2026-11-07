# VX11-1105 派工卡 B20（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9362 | `unx9362_c3_ltp.rs` | 0x7C800 | 440 | C3 账本消费协议（LTP 用例号面只读消费） |
| 2 | UNX-F9363 | `unx9363_jointest_regression_ltp.rs` | 0x7C840 | 420 | LTP「可跑」登记联测面（C2 号面 × C3 回归基建） |
| 3 | UNX-F9364 | `unx9364_cosign_table_rs.rs` | 0x7C880 | 460 | 分发表注册协议联签（table.rs 注册接口 × C3 注入点） |
| 4 | UNX-F9365 | `unx9365_jointest_syscall_entry.rs` | 0x7C8C0 | 440 | syscall 路由联测（entry.rs 双门 × C3 探针） |
| 5 | UNX-F9366 | `unx9366_jointest_errno_errno.rs` | 0x7C900 | 440 | errno 编码联测（Errno 全集 × C3 断言库） |
| 6 | UNX-F9367 | `unx9367_jointest_uaccess_c3.rs` | 0x7C940 | 380 | 参数探针联测（uaccess 拷贝 × C3 值域库） |
| 7 | UNX-F9368 | `unx9368_jointest_stats_meter.rs` | 0x7C980 | 420 | meter 计量联测（SyscallMeter × C3 统计账） |
| 8 | UNX-F9369 | `unx9369_jointest_perm_guard.rs` | 0x7C9C0 | 380 | guard 能力位联测（CAP_* × C3 权限判据库） |

## UNX-F9362 C3 账本消费协议（LTP 用例号面只读消费）
- 文件：`varix/src/unxstar/unx9362_c3_ltp.rs`
- 函数：`pub fn run_unx9362_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C800;`
- 判据原文：① 纯功能代码 ≥440 行；② 判据原文：UNX-F9362-J1 消费协议判据过，C3 LTP 用例号面只读消费声明格过，零写入 C3 账本格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9363 LTP「可跑」登记联测面（C2 号面 × C3 回归基建）
- 文件：`varix/src/unxstar/unx9363_jointest_regression_ltp.rs`
- 函数：`pub fn run_unx9363_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C840;`
- 判据原文：① 纯功能代码 ≥420 行；② 判据原文：UNX-F9363-J1 「可跑」登记联测判据过，C2 号面逐号与 C3 回归基建对接可观测，登记口径单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9364 分发表注册协议联签（table.rs 注册接口 × C3 注入点）
- 文件：`varix/src/unxstar/unx9364_cosign_table_rs.rs`
- 函数：`pub fn run_unx9364_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C880;`
- 判据原文：① 纯功能代码 ≥460 行；② 判据原文：UNX-F9364-J1 注册接口联签判据过，table::SyscallDesc/resolve 与 C3 注入点对接可观测，18 号注册面+COMPAT_TABLE 零改动格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9365 syscall 路由联测（entry.rs 双门 × C3 探针）
- 文件：`varix/src/unxstar/unx9365_jointest_syscall_entry.rs`
- 函数：`pub fn run_unx9365_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C8C0;`
- 判据原文：① 纯功能代码 ≥440 行；② 判据原文：UNX-F9365-J1 双门路由联测判据过，SYSCALL/int 0x80 双门逐门可观测，C3 探针注入零改动格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9366 errno 编码联测（Errno 全集 × C3 断言库）
- 文件：`varix/src/unxstar/unx9366_jointest_errno_errno.rs`
- 函数：`pub fn run_unx9366_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C900;`
- 判据原文：① 纯功能代码 ≥440 行；② 判据原文：UNX-F9366-J1 Errno 全集联测判据过，逐码与 C3 断言库对接一致格过，LTP 期望值逐码对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9367 参数探针联测（uaccess 拷贝 × C3 值域库）
- 文件：`varix/src/unxstar/unx9367_jointest_uaccess_c3.rs`
- 函数：`pub fn run_unx9367_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C940;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9367-J1 拷贝探针联测判据过，copy_from_user/copy_to_user 与 C3 值域库对接可观测，坏指针 EFAULT 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9368 meter 计量联测（SyscallMeter × C3 统计账）
- 文件：`varix/src/unxstar/unx9368_jointest_stats_meter.rs`
- 函数：`pub fn run_unx9368_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C980;`
- 判据原文：① 纯功能代码 ≥420 行；② 判据原文：UNX-F9368-J1 计量联测判据过，SyscallMeter 统计与 C3 统计账对接可观测，计数逐号一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9369 guard 能力位联测（CAP_* × C3 权限判据库）
- 文件：`varix/src/unxstar/unx9369_jointest_perm_guard.rs`
- 函数：`pub fn run_unx9369_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C9C0;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9369-J1 能力位联测判据过，CAP_* 与 C3 权限判据库对接可观测，EPERM 判定逐格一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
