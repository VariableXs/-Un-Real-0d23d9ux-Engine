# VX11 施工契约（AI-VX11-1105 · C2-B21~B30 · 200 单）

工作目录：`D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel`
crate：`varix`（`no_std` + `extern crate alloc`），模块目录 `varix/src/unxstar/`。
所有编译/测试在云端 4090D 执行；本地只落文件，不跑 cargo。

## 0. 红线（违反即返工）

1. 只写分派给你的 8 个文件；**不得**碰 `mod.rs`、任务板、git、他人模块。
2. 生产代码禁止：`std::`、`unsafe`、`.unwrap()`、`.expect()`、`panic!`、`assert!`、`assert_eq!`、浮点、IO、墙钟、随机（固定 LCG 可用）。
3. 取值一律 `match` / `if let` / `.get()` / `.get_mut()` / `checked_*` / `saturating_*`；禁止裸下标。
4. 不许脚本生成、不许模板复制同构段落。每个模块的域模型、状态机、语料、诊断文案按该卡判据**手写定制**。
5. 每模块 ≥ 分派表配额行（配额 280~480，实际目标 650~1000 行），≥ 12 条 `s.add_dyn`，≥ 8 个 `#[test]`。
6. 文件第一行：`//! <卡号> <标题> · 0x<SEG>段`，随后逐字摘录判据原文 + 设计要点 3~6 条。
7. 中文注释、中文诊断文案；诊断码闭集 enum + `ALL` + `detail()`。

## 1. 固定骨架

```rust
//! UNX-F9202 ... · 0x7A000段
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7A000; // 分派表给定，独占

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XxxCode { ... }
impl XxxCode {
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    pub fn detail(self) -> &'static str { ... }
}

pub fn run_unx9202_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9202");
    s.add_dyn("名字", cond, "中文明细");
    ...
    s
}

#[cfg(test)]
mod tests { ... }
```

- `CheckSet` 容量 112 条，`add_dyn` 动态明细槽仅 8 个；**优先** `s.add(name, passed, detail)` 放静态明细，动态明细只给最关键的 ≤8 条。
- `CheckSet` 无 `all_passed` 之外的聚合需求；测试里直接断言 `s.all_passed()`。

## 2. 深度要求（本批重点）

本批是 **POSIX/Linux 系统调用错误面**（errno 矩阵），每模块必须包含真实可执行的域模型，不许空壳：

- 真实错误码枚举（EBADF/EFAULT/EINVAL/ENOENT/EMFILE/ENFILE/EPERM/EACCES/ESRCH/ECHILD/EAGAIN/ENOMEM/EINTR/E2BIG/ENOEXEC/EROFS/EBUSY/EEXIST/ENOTDIR/EISDIR/ENOTTY/ENOSYS/EOPNOTSUPP 等，按卡面取用）。
- 真实判定器：输入（fd、指针类别、whence、flags、路径段权限、rlimit、时钟 id、tv_nsec、pid…）→ `Result<效果, 错误码>`，判定次序必须与 Linux 语义一致（如 EBADF 先于 EFAULT、ESRCH 先于 EPERM）。
- 真实状态机：fd 表 / 映射表 / 进程表 / 定时器表 / 等待队列，单调转换与回滚。
- 注入器：人为构造违约（坏 fd、坏指针、坏 flags、竞态、超限），断言**必被抓**（10/10 轮）。
- 对账：与既有册（B03–B30）、Windows 错误码、glibc 消费核的对照表，逐项可复算。
- 可复测声明：同一输入双跑结果一致（确定性），哈希/指纹入册只增不减。

## 3. 每模块必备判据形态（≥12 条）

1. 正常流：合法输入返回预期效果。
2. 边界：零/负/上限/上限+1 四类。
3. 注入必抓：至少 3 类违约各 10 轮全抓。
4. 判定次序：两条错误同时成立时先返回约定码。
5. 状态机：非法倒退/越级被拒。
6. 容量/耗尽：表满返回约定码且账不丢。
7. 双跑一致：同输入两次结果逐字段相等。
8. 段位守卫：`SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0`。
9. 对账一致：与对照表逐项相等。
10. 计数守恒：注入次数 = 检出次数 + 漏检次数，漏检必须为 0。
11. 文案闭集：每个诊断码都能取到非空中文建议。
12. 收口：本模块断言条数/域账只增不减。

## 4. 测试（≥8 个 `#[test]`）

必须覆盖：正常流、四类边界、三类注入、判定次序、双跑一致、容量耗尽、段位守卫。
测试内允许 `assert!/assert_eq!`。

## 5. 探针自检（由主会话统一跑云端）

主会话会把 8 个模块拷进 `/root/varix-work/probe/src/`，执行：
`cargo check` → `cargo run`（打印 `TOTAL=8 RED=0 PANIC=0`）→ `cargo test`。
你只需保证文件自洽、可编译、判据全绿。**不要**自行跑 cargo（本地不跑）。

## 6. 已验证的样板与编译坑（必读）

样板：`varix/src/unxstar/unx9202_rw_badfd_matrix_ebadf.rs`（已在云端 0 error、
24 tests 全过）。写前先读它，照其结构与深度，但**不许复制其域模型**——每卡
必须自己的错误面、自己的状态机、自己的语料。

已实测坑：
1. `FdTable::describe` 这类返回 `(u32, u16)` 的函数，取值必须
   `let (mode, _gen) = ...`，不能直接 `mode & FLAG`（E0369）。
2. 可变借用期间不能同时改 `self.other_field`（E0506/E0503）。先在块内算出
   返回值，块结束再改计数。
3. `Result<T, i32>` 可直接与 `Err(EBADF)` 比较；但 `T` 为含函数指针的结构体
   时不可 derive PartialEq。
4. 未使用的导入会产生 warning，按实际使用裁剪 `use`。
5. 数组重复 `[Item::empty(); N]` 要求 `Item: Copy` 且 `empty()` 是 `const fn`。
6. 测试里可用 `unwrap_or`，生产区禁止 `unwrap`。

