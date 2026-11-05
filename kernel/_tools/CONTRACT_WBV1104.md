# WBV1104 施工契约（子代理必读，逐条遵守）

工作目录：`D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel`
crate：`varix`（`no_std` + `extern crate alloc`），模块目录 `varix/src/unxstar/`。

## 0. 红线（违反即返工）

1. 只写分派的文件，**不得**碰 `mod.rs`、`dist/taskboard.md`、git、他人模块文件。
2. 全文禁止 `std::`、`unsafe`、`.unwrap()`、`.expect()`、`panic!`、`assert!`、`assert_eq!`（生产代码）；测试区（`#[cfg(test)] mod tests` 内）允许 `assert!/assert_eq!`。
3. 取值一律 `match` / `if let` / `.get()` / `.get_mut()` / `checked_*` / `saturating_*`，不许裸下标索引。
4. 零浮点、零 IO、零墙钟、零随机（LCG 种子固定可用）；整数运算；中文注释与中文诊断文案。
5. 取值未用变量用 `_` 前缀或重命名；不留死代码、不留 `#[allow]`（探针 main.rs 除外）。
6. 不许脚本生成、不许模板复制同构段落：每模块的域模型、闸门、语料、文案按判据定制手写。
7. 文件名 `unx####_名称.rs`，编号与卡号一致，不许自造编号。

## 1. 模块骨架（固定契约）

- 第 1 行：`//! UNX-F<编号> <卡标题> · 0x<SEG>段`
- 中文文档头必须含：**判据原文**（逐字摘自分派表的验收判据）+ **设计要点**（3~6 条）。
- 导入（按需取用，未用的不导入）：
  ```rust
  use alloc::{format, string::{String, ToString}, vec::Vec};
  use crate::checks::CheckSet;
  ```
- `pub const SEG_BASE: u32 = 0xXXXXX;`（分派表给定值，全仓唯一）。
- 诊断码闭集 enum：变体带中文 doc，`val()/detail()/ALL` 数组 + `pub fn detail(code: u32) -> &'static str` 查表闭集返回；Err 用 `Result<T, u32>`。
- 域模型：真实 struct/enum + 冻结常量表 + 单调状态机（倒退显性拒）+ 错误路径带可操作中文建议（建议写入 detail 文案）。
- 检查节：`pub fn run_unx<四位编号>_checks() -> CheckSet`，全部用 `s.add_dyn(name, passed, &detail)`，条目 ≥12 条，覆盖：正常流、边界、注入必抓（人为构造违约必被拒）、双跑一致、段位守卫（`SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0`）。
- 判据谓词全部私有 `fn xxx_ok() -> bool`；注意断言方向：`if cond { return false; }` 的 cond 是违约条件，末行是成立条件。

## 2. 深度要求

- 每模块 **≥550 行**（配额仅是下限，按 2000+ 行深度方向写足内容密度；≥550 行即达标，允许更长）。
- 真实数据结构、真实状态机、真实错误路径——不许空壳、不许注水注释。
- 每个 enum 闭集 + ALL 数组闭集返回；常量来源注释（手册章节/规范引用）。
- 每模块尾部 `#[cfg(test)] mod tests`：≥8 条 `#[test]`，含注入必抓、双跑一致、边界。

## 3. 探针自检（必须真跑，不许只看代码）

探针 crate 由主会话预建：`_tools/vx_sel_w7/`（已含 Cargo.toml + src/main.rs，#[path] 已指向本批 12 个模块）。
模块写完后：

```bash
cd D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/_tools/vx_sel_w7
cargo check 2>&1 | tail -30
cargo run -q --bin vx_sel_w7      # 必须打印 TOTAL=12 RED=0
cargo test -q                     # 必须 0 failed
```

只归因你自己的模块的错误；他人模块的错误不背账、不修他人文件，在汇报中列出即可。
修到自己的文件 0 error 0 warning 才算完。
