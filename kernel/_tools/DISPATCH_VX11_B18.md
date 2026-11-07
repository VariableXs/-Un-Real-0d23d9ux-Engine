# VX11-1105 派工卡 B18（8 单 · C2 syscall 错误面）

先完整阅读 `_tools/CONTRACT_VX11.md`，红线与深度要求全文有效。
只写下列 8 个文件，逐卡手写定制，禁止模板复制。

| # | 卡号 | stem | SEG | 配额 | 标题 |
|---|------|------|-----|------|------|
| 1 | UNX-F9346 | `unx9346_matrix_global_nosys.rs` | 0x7C400 | 400 | 全域 ENOSYS 未实装号矩阵（号面全扫） |
| 2 | UNX-F9347 | `unx9347_assembly_efault_ebadf.rs` | 0x7C440 | 380 | 错误码优先级链总装（EFAULT>EBADF>EINVAL 判序） |
| 3 | UNX-F9348 | `unx9348_assembly_errno_errno.rs` | 0x7C480 | 400 | errno 透传全链路总装（Errno→lxerrno→glibc） |
| 4 | UNX-F9349 | `unx9349_injector_assembly_fuzz.rs` | 0x7C4C0 | 380 | 坏值注入器总装（fuzz 值域全集） |
| 5 | UNX-F9350 | `unx9350_boundary_equiv_assembly.rs` | 0x7C500 | 380 | 边界值等价类总装（零/负/上限/上限+1） |
| 6 | UNX-F9351 | `unx9351_equiv_assembly_null.rs` | 0x7C540 | 380 | 指针等价类总装（NULL/-1/非映射/只读映射） |
| 7 | UNX-F9352 | `unx9352_equiv_assembly_fd.rs` | 0x7C580 | 360 | fd 等价类总装（负值/未开/已关/越限） |
| 8 | UNX-F9353 | `unx9353_race_assembly_eintr.rs` | 0x7C5C0 | 380 | 竞态注入总装（EINTR×五族） |

## UNX-F9346 全域 ENOSYS 未实装号矩阵（号面全扫）
- 文件：`varix/src/unxstar/unx9346_matrix_global_nosys.rs`
- 函数：`pub fn run_unx9346_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C400;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9346-J1 号面全扫注入判据过，未实装号全 ENOSYS 逐格检出，与 18 号注册面/COMPAT_TABLE 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9347 错误码优先级链总装（EFAULT>EBADF>EINVAL 判序）
- 文件：`varix/src/unxstar/unx9347_assembly_efault_ebadf.rs`
- 函数：`pub fn run_unx9347_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C440;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9347-J1 判序链逐例总装判据过，多错误并发时取高优先逐格检出，与 Linux 判序逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9348 errno 透传全链路总装（Errno→lxerrno→glibc）
- 文件：`varix/src/unxstar/unx9348_assembly_errno_errno.rs`
- 函数：`pub fn run_unx9348_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C480;`
- 判据原文：① 纯功能代码 ≥400 行；② 判据原文：UNX-F9348-J1 全链路逐码总装判据过，Errno→sysret→glibc errno 逐码一致格过，与 B13 编码器零漂移格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9349 坏值注入器总装（fuzz 值域全集）
- 文件：`varix/src/unxstar/unx9349_injector_assembly_fuzz.rs`
- 函数：`pub fn run_unx9349_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C4C0;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9349-J1 fuzz 值域全集注入判据过（零/负/上限/上限+1/全 F/全 0），检出率统计格过，零内核 panic 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9350 边界值等价类总装（零/负/上限/上限+1）
- 文件：`varix/src/unxstar/unx9350_boundary_equiv_assembly.rs`
- 函数：`pub fn run_unx9350_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C500;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9350-J1 四等价类逐类总装判据过，逐类期望值逐格检出，等价类账落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9351 指针等价类总装（NULL/-1/非映射/只读映射）
- 文件：`varix/src/unxstar/unx9351_equiv_assembly_null.rs`
- 函数：`pub fn run_unx9351_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C540;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9351-J1 四指针等价类总装判据过，逐类期望 EFAULT 逐格检出，只读映射写路径格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9352 fd 等价类总装（负值/未开/已关/越限）
- 文件：`varix/src/unxstar/unx9352_equiv_assembly_fd.rs`
- 函数：`pub fn run_unx9352_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C580;`
- 判据原文：① 纯功能代码 ≥360 行；② 判据原文：UNX-F9352-J1 四 fd 等价类总装判据过，逐类期望 EBADF 逐格检出，等价类账落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。

## UNX-F9353 竞态注入总装（EINTR×五族）
- 文件：`varix/src/unxstar/unx9353_race_assembly_eintr.rs`
- 函数：`pub fn run_unx9353_checks() -> CheckSet`
- `pub const SEG_BASE: u32 = 0x7C5C0;`
- 判据原文：① 纯功能代码 ≥380 行；② 判据原文：UNX-F9353-J1 EINTR×五族竞态总装判据过，逐族 100 次统计格过，与 B26 注入器单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
- 深度抓手：按契约第 2 节建真实判定器/状态机/注入器/对账表；本卡必须体现其标题所指的错误面，不许泛泛而谈。
