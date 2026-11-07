# VX11 施工契约（B21–B30 · 200 单）

本文件是并行施工代理的唯一规范。违反任一条 = 返工。

## 0. 任务来源

- 计划表：`kernel/_tools/_vx11_plan.json`（200 条，字段：id / num / file / slug / seg / lines / title / accept）。
- 你只写分配给自己的 ID 区间，**不要领取或改动其他区间**。
- 只写功能代码；**不要改 `mod.rs`、不要写任务板、不要建脚本**。

## 1. 文件落点与命名

`kernel/varix/src/unxstar/<file>`，文件名严格取计划表的 `file` 字段。

文件头必须是 `//!` 锚点注释，至少包含：
- 卡号 + 标题；
- `锚点：<ID>-J1 <判据原文>`；
- 数据段 `SEG_BASE = <seg>` 与诊断码闭集说明；
- 本模块的结构清单（数据结构/状态机/错误路径/注入核）。

示例：

```rust
//! UNX-F50003 黑点补偿开关语义（显式登记不静默）（0x42ce0 段）
//!
//! 锚点：UNX-F50003-J1 开关 ×2 档输出差异显式落账；判据锚定 §AI-63 §2 专题十二。
//! ...
```

## 2. 依赖与导入（no_std）

每个文件必须自包含，统一使用：

```rust
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::{fabsf, sqrtf, powf, expf, sinf, cosf, atan2f, floorf, ceilf};
```

- 浮点超越函数只能走 `libm`（core 无 sqrt/powf/sin）。
- `f32::abs()`、`f32::sqrt()` 在 no_std 下不可用，用 `libm::fabsf/sqrtf`。
- 唯一允许的跨模块依赖：B21（F50002–F50020）可 `use super::unx50001_gamutmap::{...}`；其余批次一律自包含（不要引用别的卡文件）。

## 3. 功能深度（硬指标）

- 文件总行数 **≥ 计划表 `lines`**（卡面下限，用 `wc -l` 自查，建议超出 15%）。
- 必须有真实数据结构（不是只有函数堆砌）：
  - 状态机 / 账本 / 注册表 / 解析器 / 调度器 四类里至少两类；
  - 显式错误枚举 + `code()` 返回 `SEG_BASE + n`；
  - 边界条件逐个处理（空输入、容量满、越界、重复、乱序、超时）；
  - 至少 1 个「注入核」：构造 N 类坏样本，逐类断言被显式拒绝。
- 禁止空壳、`todo!()`、恒真断言、只返回常量的假实现。
- 内核路径**零 panic**：非测试代码不得出现 `unwrap` / `expect` / `panic!` / 裸下标越界。用 `get()`、显式区间校验、`Result<_, Err>`。
- 不得使用 `std`；不得引入新 crate。

## 4. 判据（run_checks）规范

每个文件必须提供：

```rust
pub fn run_unx<num>_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F<num>");
    // ... 8–20 条
    s
}
```

- `CheckSet::add(name: &'static str, passed: bool, detail: &'static str)`；
- 运行期文本用 `CheckSet::add_dyn(name, passed, &detail_string)`；
- 每条判据必须真正测实现：优先断言**性质**（守恒、单调、幂等、确定性、往返一致、非法必拒），不要写依赖猜测的魔数；
- 文件末尾必须有：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx<num>_checks_green() {
        let set = run_unx<num>_checks();
        assert!(set.all_passed(), "UNX-F<num> red: {}", set.red_items_text());
    }
}
```

- 判据数量与样本量匹配，注意 `MAX_CHECKS = 112`，单模块 ≤ 20 条。

## 5. 质量红线

- 中文注释解释「为什么」，不写废话；禁止大段模板化注释。
- 每个公开函数有 `///` 一行说明（做什么 + 关键契约）。
- 常量写死并注明来源/推导（例如 `knee = 0.82 × boundary`）。
- 算法必须可复算：同样的输入两次调用结果逐位一致。
- 不要生成「批量相似」的 8 个文件——每张卡的主题不同，数据结构与算法必须针对主题真实落地。

## 6. 自查清单（交付前逐项）

1. `wc -l <file>` ≥ 卡面行数；
2. `grep -c "pub fn run_unx" <file>` == 1，函数名与卡号一致；
3. `grep -c "unwrap\|expect(" <file>` 在 `#[cfg(test)]` 之前为 0；
4. 文件里没有 `mod.rs` / 任务板写入；
5. 只使用第 2 节允许的导入；
6. 8–20 条判据，且注入核覆盖 ≥3 类坏样本；
7. 回复给主代理：文件路径 + 行数 + 判据条数 + 主题一句话。

## 7. 云端自验（必做，不许跳过）

参考实现：`kernel/varix/src/unxstar/unx50001_gamutmap.rs`（1,400 行，18 条判据全绿，
`cargo test --lib` 3/3 通过）。写之前先通读该文件，照它的密度与风格写，不要照抄主题。

每写完 1~2 个文件，在 `kernel/_tools` 目录下执行（`<tag>` 用你的组号）：

```bash
MSYS2_ARG_CONV_EXCL='*' python _vx11_check_remote.py <tag> <num> [<num> ...]
```

- 首次编译约 30~60 秒；输出末尾必须是 `TOTAL=n RED=0`，随后 `cargo test --lib` 必须 ok。
- 出现编译错误按行号修，不许注释掉判据、不许删测试。
- B21（F50002–F50020）的探测列表里必须同时带上 `50001`（共享依赖）。
- 单文件自查：`python _tools/_vx11_linecount.py` 不需要；直接 `wc -l`。

## 8. 已知坑（照做，别重踩）

- `Result<T, E>` 不能与 `Err(e)` 直接 `==`（T 未实现 PartialEq 时编译失败），用
  `.err() == Some(e)`。
- `f32` 的 `abs/sqrt` 在 no_std 不可用；用 `libm::fabsf/sqrtf`。
- 判据里的魔数必须来自实现推导并写注释，不要写「猜一个能过」的值；断言优先性质。
- `#[cfg(test)]` 里的断言与判据必须真的能过——探针会跑 `cargo test --lib`。
- 文件内不得出现 `std::`、`println!`、`unwrap`（测试块除外）。
