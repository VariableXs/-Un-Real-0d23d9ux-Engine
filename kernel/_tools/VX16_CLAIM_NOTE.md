# VX16 领单登记（AI-VX11-1107 · UNX-F52401~F52600 共 200 单）

## 领单范围（本会话独占，其他会话请勿重复领取）
- **UNX-F52401~F52600 共 200 单**（CoRun 域续卷），任务板 200 条已统一写回
  「已领 / AI-VX11-1107(VX16)」。
- 卡数据：`_tools/vx16_cards.json`（200 卡，title/quota/criteria 自任务板
  dist/taskboard.md 逐字提取，含 off-by-one 板式：卡 N 锚 F(N+1)-J1）。
- SEG 分派（全仓查重零碰撞；VX15 窗止于 0xCA3CC）：
  `SEG_BASE = 0xCA400 + 0x40 × (id − 52401)`，52401→0xCA400 … 52600→0xCBB40，
  全批 %0x40==0，窗口 0xCA400..0xCBB7F 全空闲。

## 边界（防撞车）
- VX15（52201~52400）已收口（收官印 unx52400_vx15seal），与本批零重叠。
- unx5241/unx5261 等四位数为库内旧模块（SEG u16 0x9580 域），不与本批冲突。
- 本会话对 mod.rs 的改动 = 每单 1 行 `pub mod`（批登记式样）。

## 规格契约（沿用 VX15 全套）
- 样板 `unx52081_astudio.rs` / `unx52201_ssms.rs`：每单 ≥max(quota+60,550) 行
  （目标 650–1100）、≥15 条检查、诊断码闭集 13 变体（Ok=0，EGradeDrift==12，
  detail 全非空中文）、注入核 10/10 单点记账守恒、双跑 FNV 指纹一致、
  `const _SEG_GUARD`、no_std 自包含、生产区零 unwrap/expect/panic/浮点/IO/
  裸下标、≥10 条 #[test] 含 `unx<num>_checks_green`、run_unx<num>_checks()。
- 已知坑清单（14 条）见 VX15 台账交接；分批口律：逐批探针全绿 → mod.rs
  登记 → 提交（AI-VX16-11xx 批BX 格式）→ 任务板写回已完成。

## 进度
- [x] 200 单领单写回任务板
- [x] 卡面提取 vx16_cards.json
- [ ] 写码 52401~52600（分批推进）
