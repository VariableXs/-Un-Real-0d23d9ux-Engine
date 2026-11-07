# VX12 施工简报（AI-VX12-1106 · 台账域 B40 · F81001–F81200 共 200 单）

仓库根：`D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel`

## 必读（按序）
1. 本文件全部条款。
2. `kernel/_vx11_ops/VX11_CONTRACT.md`（施工契约 §1–§7）。
3. 风格模板：`kernel/varix/src/unxstar/unx81001_ledgerschema.rs`（665 行，本批首单）。
4. 已验收参考：`kernel/varix/src/unxstar/unx50001_gamutmap.rs`。

## 文件落点与命名
- 落点：`kernel/varix/src/unxstar/`，文件名 `unx<N>_<slug>.rs`（N=派工卡号）。
- 文件头（`//!`）逐字包含：
  1. `//! UNX-F<N+1> <卡面标题> · <SEG>段`
  2. `//! 判据原文（派工卡 <N> 逐字锚定）：` + ①②③ 三条（①行数下限；②判据原文
     UNX-F<N+1>-J1 + 卡面判据正文；③完成时写回）。
  3. 设计要点 5–7 条（为什么这样设计，中文）。
- `pub const SEG_BASE: u32 = 0x91000 + 0x30 * (N - 81001);`（写成具体十六进制值）。
- 判据函数：`pub fn run_unx<N>_checks() -> CheckSet`，判据名 `F<N+1>-01 … F<N+1>-NN`
  （8–20 条），末尾 `#[cfg(test)] mod tests`，测试名 `unx<N>_checks_green`。
- 判据必须真测实现：性质断言（守恒/单调/幂等/确定性/往返一致/非法必拒）优先，
  注入核 ≥10 类坏样本逐类断言具体诊断码（注入必抓）。

## 质量红线
- 内核路径零 panic：无 `unwrap/expect/panic!/todo!/unimplemented!`（`unwrap_or` 允许）。
- 无 `std::`；不引入新 crate；浮点超越函数只走 `libm`（无浮点需求就零浮点）。
- 自包含：不引用其他新写模块（只可用 crate 公共设施 `checks`/`alloc` 别名）。
- 行数：`wc -l` ≥ 卡面行数（建议 +15%）。
- 全文件 grep `if False else`、`if True else`、`todo!` 必须为 0 命中（自检）。

## 禁止
- 禁止改 `mod.rs`、任务板、任何非本单文件；禁止写脚本；禁止删除既有文件。

## 卡面来源
派工卡面（标题/行数/判据正文）从 `kernel/_vx11_ops/vx12_cards_81xxx.txt` 提取：
文件按 `## [UNX-F<卡号>]` 分块，每块含 `- 行数:`、`- 域:`、`- 验收:` 行。
你只负责自己的卡号区间，**逐字引用自己卡面的判据正文**（验收行里的 ② 判据原文…部分）。

## 交付回复格式（每文件一行）
`路径 行数 判据条数 主题一句话`
