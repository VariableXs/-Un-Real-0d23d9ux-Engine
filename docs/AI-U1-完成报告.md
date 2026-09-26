# AI-U1 完成报告 · Varix STAR I · I 通用域·一分队（批次一）

> 分工包：F401-F450（50 项 · 目标上限 52,325 行）。本报告覆盖**批次一**
> 八项（F401/F403/F404/F405/F407/F408/F416/F424）——「系统快捷键与通用
> 交互语义」簇。生成于 2026-09-26。

## 一、交付物清单

| 交付物 | 落位 |
| --- | --- |
| 批次一八项语义核 + 共享底盘 | `kernel/varix/src/uni1/`（10 文件，2,969 行，其中纯功能约 1,547 行） |
| robust.rs 274 域函数指针表接线 | `crate::uni1::run_uni1_checks` 单行注册（域聚合，275 容量） |
| 隔离舱（#[path] 直挂真实文件） | `_attic/aiu1-f401-f450/cabin/`（45 测试全绿） |
| 行数对账与缺陷账本 | `_attic/aiu1-f401-f450/行数对账与缺陷账本.md` |
| CHANGELOG | 批次一条目已补 |

## 二、逐项判据达成（主册判据 → 实装检查名）

| 项 | 模块 | 判据锚 | CheckSet 检查（全绿） |
| --- | --- | --- | --- |
| F401 桌面自动排列 | `autoarrange.rs` | 四序 20 项实测；插入/删除补位；互斥切换；F124 弹性档弹回 | f401-order-{name,size,type,date}-20 ×4、insert-into-seq、delete-reflow、free-no-snapback、auto-snapback、mode-mutex-roundtrip、cell-origin |
| F403 Win+L 锁屏快捷 | `lockhot.rs` | 锁定 <300ms；媒体暂停续播；窗口保持；摘要只计数；F316 同入口 | f403-budget-sum、lock-under-300ms、media-pause-resume、window-states、summary-count-only、unlock-stops-summary、idle-same-entry、over-budget-logged、idempotent |
| F404 Win+E 资源管理器 | `explorehot.rs` | 标签/窗模式；此机页清单钉死；连按行为；首开 <1.5s；骨架先行 | f404-hotkey-registered、this-pc-list、tab-mode-repeat、cold-under-1500、cold-over-budget-logged、window-mode-repeat、skeleton-first、close-window |
| F405 Alt+F4 与关机菜单 | `altf4.rs` | 焦点三场景；三选项默认关机；三问联动；与 × 同语义；F384 陷阱优先 | f405-menu-const、window-clean-close、dirty-triple-ask、ask-cancel-no-close、ask-proceed-close、desktop-power-menu、menu-move-down/wrap/wrap-up/confirm/esc-cancel/confirm-closed-noop、modal-trap-priority |
| F407 Win+I 设置快捷 | `sethot.rs` | 三场景；单例聚焦；搜索框光标就绪；注册表登记 | f407-hotkey-registered、scene-closed-opens、page-focus、scene-open-focuses-search、scene-search-keeps、singleton、latency-budget、latency-over-logged、close-then-reopen |
| F408 Win+X 快捷菜单 | `winxmenu.rs` | 九项对照表；首字母快捷；子菜单二级；打开 <1s | f408-nine-items-table、hotkey-registered、open-under-1s、open-over-logged、letter-s/t/miss、activate-terminal、submenu-parent-noop、submenu-confirm、submenu-esc-peel、closed-noop |
| F416 Win 键开合开始菜单 | `winkey.rs` | 开 <150ms；焦点落搜索框；打字零丢失；Esc/外点关闭；连按稳定 | f416-open-under-150、toggle-close、debounce、real-second-press、esc-close、outside-click-close、typing-zero-lost、over-budget-logged、ime-yield、log-pairs |
| F424 Esc 通用关闭语义 | `escstack.rs` | 四层语义表；逐层剥离；桌面态无副作用；响应 <100ms | f424-semantics-table、peel-popup-then-panel-then-modal、semantics-per-tier、desktop-noop、one-layer-per-press、latency-budget、latency-over-logged、outside-close-targeted、invariant-tier-order |
| 共享底盘 | `ubase.rs` | F244 键位注册唯一落位；浮层栈；事件环；旋钮 | hk-register-two、hk-conflict-rejected、hk-lookup、hk-rebind-user、hk-reset-default、hk-snapshot-shape、ls-empty-desktop-noop、ls-top-is-popup、ls-peel-order、ls-drained、ring-evict-oldest、knob-clamped |

## 三、测试证据

- **隔离舱**：`_attic/aiu1-f401-f450/cabin`（#[path] 直挂真实生产文件 +
  同一份 checks.rs）——**45/45 通过，0 警告**。
- **真实 crate**：`cargo test -p varix --lib uni1::` —— **38/38 通过**
  （含域聚合器 `uni1_batch1_aggregate_all_green`）。
- **全量回归**：4,093 项中 4,078 绿；15 红全部位于 `compatstar2`/
  `stareco`（AI-C2、AI-V2 在途施工模块）及其连锁的 robust 全域聚合——
  **uni1 零失败，本批不回归任何既有判据**。红项归属详见缺陷账本
  （不修、不越界——那是 C2/V2 的活）。
- **提交态验证**：以「HEAD + 仅本批接线」的干净组合临时换入编译验证
  （38/38 绿）后字节级还原工作区——提交进仓库的树保证可编译，且不带
  任何其他分队的在途半成品。

## 四、施工决策（为什么这么做）

1. **语义核先行，渲染层不越界**：I 域判据的「界面与交互 60%」属桌面
   层（TS/合成器）工程；内核侧交付的是判据可机检的语义核（状态机 +
   预算记账 + 事件账），与 K1/K2 批次同架构。渲染接线随闸门登记。
2. **F244 键位注册唯一落位**：本批 6 处「键位注册（F244）」判据全部
   收口到 `ubase::HotkeyTable`（登记/冲突拒/改键/恢复默认/快照），
   一处一事实，批次二沿用。
3. **隔离舱**：多 AI 并行施工期主 crate 常被其他分队的中间态挡住编译；
   隔离舱 #[path] 直挂真实文件，让本队判据验证不排队、不被卡（防卡死
   纪律第 ②③条的直接落地）。
4. **诚实记账贯穿**：所有延迟/预算判据（300ms/1.5s/1s/150ms/100ms）
   的实测值由调用方注入，超线如实计数不静默——「异常零静默」红线。

## 五、行数对账（如实登记，偏差说明）

批次一目标上限合计 6,565 行，实收纯功能约 1,547 行（23.6%）。偏差主因：
主册工程量口径含「界面与交互 ~60%」——该部分在本仓库架构中落桌面层，
本批为语义核实装层。**不做注水补齐**（铁律 2 优先于铁律 1 的数字）；
批次二扩列时对每模块按真实状态机路径补深化（撤销链、多窗表、持久化
快照等真实功能面），逐项把比例抬上 90% 线。逐项明细见缺陷账本。

## 六、遗留与批次二计划（42 项）

- F402/F406/F409/F410-F415/F417-F423/F425-F450 待续建（判据摘文已在
  分工书钉死，接缝注入口已就位：F244 表、F424 栈、F072/F074 引擎参数）。
- F402 任务管理器形制界面等「会话表格」拍板依赖项，按主册第 7 部分第
  4134 行条款预留，拍板后对齐。
- 依赖项：F284/F354/F291 等跨域注入口以参数承接（跨泳道借力 = 0）。
