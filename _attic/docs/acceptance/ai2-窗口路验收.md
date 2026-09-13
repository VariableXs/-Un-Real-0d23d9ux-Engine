# AI-2 窗口路验收证据索引（W-* / C-* / B-27 / D-3）

> 批次顺序：W-1 → W-2 → W-3 → C-1 → C-2 → C-6 → C-3 → C-4 → C-5 → C-8 → W-4 → W-5 → B-27 → D-3
> 日期：2026-09-08 ｜ 本轮 AI-2 会话：修复过期单测、补 C-8 嵌入验证工具段、补契约单测、证据归档。

## 代码级完成度（本仓可验证部分）

| 批次 | 内容 | 主要文件 | 单测/验证 | 真机项 |
|---|---|---|---|---|
| W-1 | 多嵌入并发 + EmbedRegistry | `src-tauri/src/shell/embed.rs` | `registry_multi_embed_semantics` ✓（本轮修复缺 `capture` 字段） | [ ] ≥3 窗口同嵌实测 |
| W-2 | 嵌入 DPI 修正 | `embed.rs`（per-monitor/forward_dpichanged） | 代码在位 | [ ] 清单见 `w2-dpi-regression.md` |
| W-3 | 退出/崩溃占位卡 + 监护 | `embed.rs`（WAIT 状态机） | 代码在位 | [ ] 手动杀进程实测 |
| C-1 | L1 常驻监护 WinEventHook | `embed.rs`（事件泵/自动重嵌/popup 广播） | 代码在位 | [ ] Chrome 新弹窗实测 |
| C-2 | 捕获可靠性 | `embed.rs`（自适应超时/标题正则/证据包/框选） | 代码在位 | [ ] Top30 命中率 ≥90% |
| C-6 | 分级探测器 | `compat_probe.rs` | `effective_tier_priority` ✓ | [ ] 与人工判定一致率 ≥85% |
| C-3 | L2 容器包裹引擎 | `container.rs` | `child_of_map_semantics` ✓（本轮修复 super::win 引用） | [ ] OBS L2 实测 |
| C-4 | L3 画面捕获 + 输入转发 | `capture.rs`（WinRT/帧池/SendInput） | 代码在位 | [ ] UWP 实测 + P95 <50ms |
| C-5 | L4 智能让位 | `winman.rs`（全屏/反作弊看护） | 代码在位 | [ ] 独占游戏让位实测 |
| C-8 | 兼容矩阵实测回填 | `portable/AI5/Compat-Matrix.ps1`（本轮新增 Run-Embed/Report-Embed） | PS5.1 真跑 4 路径 ✓ | [ ] 见 `compat-top200.md` |
| W-4 | 布局快照 | `src/system/windows/snapshots.ts` + `SnapshotManager.tsx` | vitest 全绿 | [ ] 拔屏恢复实测 |
| W-5 | 显示器热切换 + 标签页化 | `winman.rs` display watcher + `vwm.ts` TabStrip | 代码在位 | [ ] 拔插 HDMI 5 次 |
| B-27 | 应用生态 2.0 | `ecosystem.rs`（评估/搬迁/Steam/AUMID/fileAssociations） | 代码在位 | [ ] 7-Zip/NSIS 安装器实测 |
| D-3 | 全域接管看门狗 | `shell_watch.rs` | `whitelist_contract_15_unique_lowercase` + `policy_enum_complete` ✓（本轮新增） | [ ] VM 档收编实测 |

## 本轮修复与新增（2026-09-08）

1. **cargo test 编译失败修复**：`container.rs` 测试引用已移除的 `super::win` 路径 → 改 `super::`；
   `embed.rs` 两处 `EmbedSession` 初始化缺 `capture` 字段 → 补齐。修复后 lib 单测 103+5 全过
   （仅剩 `sysmaint.rs` 2 个失败属 AI-5 F-6 域，未越界修改）；
2. **C-8 嵌入验证段**（主计划 7.8）：`Compat-Matrix.ps1` 新增 `-Action Run-Embed / Report-Embed`，
   tier 决策树镜像 `compat_probe.rs`；`inputOk/dpiOk` 诚实输出 `todo` 待真机人工回填；
   已在真 PowerShell 5.1 验证语法与 skip/空集/报告路径；BOM 保持（Edit 会掉 BOM，已恢复）；
3. **D-3 契约单测**：白名单 15 类唯一小写 + 策略三态 + 默认 ask；
4. **证据归档**：本文档 + `w2-dpi-regression.md` + `compat-top200.md`。

## 第二轮补齐（2026-09-08 深夜，逐条审计后）

5. **C-5 `compat.hint` 字段**（主计划 7.5 第 4 条）：`CompatInfo` 增 `hint: fullscreen | anticheat`
   （serde default 平滑升级旧 apps.json）；探测器区分反作弊特征（EasyAntiCheat/BEService/EACLaunchService/
   STARTUP/BuriedScene → anticheat）与独占全屏引擎窗口（UnrealWindow → fullscreen）；
   新增单测 `l4_hint_classification`（分类分流）+ `compat_info_roundtrip` 扩展 hint 往返与旧文件兼容；
6. **C-4 bench 项**（主计划 7.4 第 5 条）：`tools/bench.cjs` 报告表新增 `captureE2E`（预算 P95 <50ms），
   按 vwmOpen 先例如实标注 SKIPPED 并写明测量协议（真机 L3 会话：ms 计计时器点击→像素变化打点）；
   已实跑 `--no-gui` 生成 `docs/bench/2026-09-08.md` 验证报告落盘；
7. **B-27 Jumbo 图标核实**：`launcher.rs` `icon_n_dataurl`（IShellItemImageFactory 256px）已在位，非缺口；
8. **W-5 TabStrip / W-3 占位卡前端核实**：`vwm.ts` 标签组合并/拆分/保活语义 + `embedState.ts`/
   `VwmAppContent.tsx` 退出/异常/回壳占位卡均在位，非缺口。

## 第三轮补齐（2026-09-08 深夜续，hint 消费闭环）

9. **C-5 `compat.hint` 前端消费闭环**：`LauncherManager.tsx` 层级选择器旁新增 L4 让位归因徽标
   （AC=反作弊 / FS=独占全屏，悬停显示完整让位语义说明，`--warn` 色系）；
   i18n `compatHintFullscreen` / `compatHintAnticheat` zh+en；`desktop.css` `.tp-l4-hint` 样式。
   至此 hint 链路完整：探测器（compat_probe.rs）→ 登记库（apps.json）→ IPC（ipc.ts）→
   启动器徽标（LauncherManager）+ 运行时看护（winman.rs anticheat_watcher 独立进程级兜底）。

## 第四轮修复（2026-09-08 深夜续，L4 路由 bug）

10. **L4 误嵌入 bug 修复**（[embed.rs] 嵌入主路径）：tier 路由 match 的 `_ => {}` 兜底臂同时
    吞掉 L1 与 L4——L4（反作弊/独占全屏）应用会被错误地走重父级强制嵌入（剥样式可能触发
    反作弊检测误判）。补显式 L4 臂：绝不嵌入，`attached:false` + 让位归因 reason
    （`compat.hint` 分流 anticheat/fullscreen），让位语义由运行时看护承担（fullscreen_watcher
    桌面层收起 / anticheat_watcher kbdhook 停用 + 横幅）。修复后 `_` 兜底仅剩 L1 主路径。

## 交叉验证（2026-09-08 本机）

- `cargo check` ✓（仅警告）；`cargo test --lib`：106 通过，AI-2 域全过；
- `npm run typecheck` ✓；`node tools/audit.cjs` AUDIT PASSED ✓；`npx vitest run` 257 passed ✓；
- `node tools/bench.cjs --no-gui` ✓（captureE2E 项落盘）。
