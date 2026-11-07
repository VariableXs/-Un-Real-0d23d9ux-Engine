# VX11 施工合同（每个模块必须逐条满足）

## 0. 边界

- 只允许新建/改写你自己的目标文件：`varix/src/unxstar/<stem>.rs`。
- 禁止修改 `mod.rs`、`checks.rs`、Cargo.toml、任何其他模块、任何工具脚本。
- 禁止在本机运行 `cargo` / `rustc` / 任何编译（用户机器性能不足，编译统一在云端做）。
- 禁止用脚本或模板批量生成代码；每个模块必须按自己的 errno 语义手写，数据结构、判定路径、不变量都要不同。
- 不写 README/说明文档，不留临时文件。

## 1. 文件骨架

```rust
//! UNX-F#### <标题> · 0x<SEG>段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F#### 逐字）：
//!   <把派工卡里的判据原文抄进来>
//!
//! 设计要点：
//!   ① ...
//!   ② ...
//!   ...（写清判定模型、状态机、顺序、容量、指纹、注入轮次）
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F#### 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x<SEG>;

... 实现 ...

pub fn run_unx####_checks() -> CheckSet {
    let mut s = CheckSet::new("unx####");
    // >= 12 条
    s.add("静态标签", bool_expr, "静态明细");
    s.add_dyn("动态标签", bool_expr, &format!(...));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1() { ... }
    // >= 8 个
}
```

- 首行的 `####` 必须是四位卡号；`0x<SEG>段` 必须与派工卡的段基址一致。
- `pub const SEG_BASE: u32 = 0x...;` 必须与派工卡一致。
- 检查函数名固定为 `run_unx####_checks`（#### 四位，和卡号完全一致），签名 `pub fn ...() -> CheckSet`。
- `CheckSet::new` 的域名字符串用 `"unx####"`。
- 每个模块至少 **12 条判据**、至少 **8 个 `#[test]`**。
- `CheckSet` 上限 112 条；`add` 的标签/明细是 `&'static str`，动态明细用 `add_dyn` + `alloc::format!`。
- 判据必须真的能全绿：写完后逐条用 `#[test]` 复算同样的表达式，确保没有常量自相矛盾。

## 2. no_std 红线（违反即返工）

- 不出现：`std::`、`unsafe`、`.unwrap()`、`.expect()`、`panic!`、`unimplemented!`、`assert!` / `assert_eq!`（生产代码）；测试代码可以用 `assert_eq!`。
- 不用浮点、文件/网络 IO、墙钟、系统随机数、线程、锁；允许固定 LCG（自写整数同余）。
- 不裸索引 `a[i]`：一律 `.get(i)` / `.get_mut(i)` / `checked_add` / `checked_sub` / `checked_mul` / `saturating_*` / `wrapping_*`（仅在语义明确时）。
- 不 `collect()` 到固定数组；用 `Vec` 或固定容量数组 + 计数。
- 需要的 `alloc` 导入按用途写全：
  - `use alloc::vec::Vec;`（类型）
  - `use alloc::vec;`（宏，若用 `vec![]`）
  - `use alloc::string::{String, ToString};`（若用）
  - `use alloc::format;`（若用 `format!`）

## 3. 质量要求（判据之外）

- 模块必须是一个**可执行的判定模型**：至少包含一个状态机或表驱动矩阵，输入→判定→账本→指纹，路径覆盖正常/边界/错误/顺序/竞态。
- 错误码必须与 Linux errno 数值一致（EBADF=9、EFAULT=14、EINVAL=22、ENOENT=2、EMFILE=24、ENFILE=23、EPERM=1、EACCES=13、ESRCH=3、ECHILD=10、EAGAIN=11、ENOMEM=12、EINTR=4、E2BIG=7、ENOEXEC=8、EROFS=30、EBUSY=16、EEXIST=17、ENOTDIR=20、EISDIR=21、ENOTTY=25、ENOSYS=38）。
- 诊断码：`#[repr(u16)] enum XxxCode { ... }` + `pub const ALL: &[XxxCode]` + `pub fn code(self) -> u16 { self as u16 }` + `pub fn detail(self) -> &'static str`（中文一句话建议）。码值用 `SEG_BASE + 序号` 并保持两两唯一。
- 注入/复算类模块：固定轮次（如 10 轮），账本必须满足 `injected == detected + escaped`，并断言 `escaped == 0`。
- 指纹：自写 FNV-1a（32 位），逐字节喂入，双跑相等。
- 行数目标 700~1000 行（深度优先，宁多勿少）；注释只写「为什么/不变量」，不写废话。

## 4. 常见编译雷（务必自查）

1. 函数返回 `Result<T, E>` 用 `?` 时，返回类型必须一致；自定义 `pub type SysResult<T> = Result<T, Errno>;`。
2. `describe()` 返回元组时不能直接和标量比较：先解构 `let (mode, gen) = ...;`。
3. 借用冲突：在 `&mut self` 的方法里，先把需要的字段读进局部变量，再改其他字段（E0506/E0503）。
4. `Err` 类型没有 `PartialEq` 时不要写 `== Err(x)`，用 `.err() == Some("...")` 或 `matches!`。
5. `[None; N]` 要求 `T: Copy`；自定义结构用 `const INIT: T = ...;` + `[INIT; N]`。
6. `u64` 实参不要传给 `i64` 形参；统一用 `u64` 表示地址/长度，`i64` 表示偏移时显式 `as i64` 并保证值域。
7. 数组 `len()` 是 `usize`，和 `u32` 比较要显式转换。
8. 不要写 `let _ = ...;` 掩盖必须使用的返回值（会导致判据恒真）。
9. 测试里不要用 `assert!(checks.all_passed())` 之外的空断言；至少一个测试要调用 `run_unx####_checks()` 并断言全绿。
10. 不要引用其他 unx 模块的任何符号；模块必须自包含。

## 5. 交付自检（写完必须做）

对每个文件执行并报告：

- `wc -l` 行数
- `grep -c "#\[test\]"` 测试数
- `grep -c "s.add"` 判据数（含 `add_dyn`）
- `grep -n "std::\|unsafe\|unwrap()\|expect(\|panic!\|assert!"` 必须为空（测试块里的 `assert_eq!` 除外）
- 首行、`SEG_BASE`、`run_unx####_checks` 三处核对

最后用一段话报告 5 个文件的：文件名 / 行数 / 判据数 / 测试数 / SEG。
