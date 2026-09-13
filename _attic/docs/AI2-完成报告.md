# AI-02 交付报告 —— 窗口编排组（NEXT-40 · 化境）

> 版本：ENGINE-Version-01XHI9DN.1.5xw · 日期：2026-09-08
> 范围：仅 AI-02 领地任务（未触碰其他 AI 任务文件；仅在阻塞整体编译时做了最小修复，见「越界修复说明」）。

## 一、交付清单（17 项全部完成）

### 窗口编排核心（NEXT-40）
| 任务 | 内容 | 关键文件 |
|---|---|---|
| N-01 | 窗口时间机器：快照/命名/一键恢复（三级恢复策略）/自动快照去抖 8s/FIFO 200 条 | `src/system/windows/timeline.ts` |
| N-02 | 舞台管理器：StageRail 侧幕、整组上台、组间轮换 Ctrl+Shift+←/→、热区错开 24px | `src/system/windows/stages.ts`、`StageRail.tsx` |
| N-03 | 窗口规则引擎：触发器（应用/标题）、12 条内置模板、priority+specificity 裁决、透明度 30–100 安全护栏、L4/管理员窗口全拒、裁决日志环形 500 条 | `src/system/windows/rules.ts`、`rulesApply.ts` |
| N-04 | 画中画 PiP：进入/退出/移动 | `src/system/windows/pip.ts` |
| N-05 | 工作区场景：视觉包/行为包/编排快照引用、未保存守卫、失败全量回滚 | `src/system/windows/scenes.ts` |
| N-06 | 动效编排器：stagger 30ms 错峰、单 rAF 批次合并、帧预算 8ms 静默降级、reduce-motion ≤80ms、FLIP 引擎 | `src/lib/motion/orchestrate.ts` |
| U-14 | 智能吸附 2.0：五种分区方案库、指针分区预测（幽灵预览与落位同源）、Shift 临时禁用、开关持久化 | `src/system/windows/snap2.ts` |

### 窗口增强（化境 V 路）
| 任务 | 内容 | 关键文件 |
|---|---|---|
| V-21 | 经典系统菜单（标题栏右键/Alt+Space）+ 键盘微调模式 | `src/system/windows/systemMenu.ts` |
| V-22 | 失联窗口救援：healthCheck 体检 + 启动/工作区变化自动拉回 + 手动一键 | `src/system/windows/rescue.ts` |
| V-23 | 实时几何提示：尺寸/坐标 hover 提示、Shift 等比 | `src/system/windows/sizeHint.ts` |
| V-24 | 多选编组：Ctrl+点击多选、整组平移（相对位置误差 0px）、组贴靠、组最小化、组关闭（确认门）、Esc 退出、订阅机制 | `src/system/windows/multiselect.ts` |
| V-25 | 焦点历史：栈深 20、去重、Ctrl+Alt+[/] 回溯/前进、新聚焦截断前进分支 | `src/system/windows/focusHistory.ts` |
| V-26 | 窗口位置互换：同尺寸对调 / 异尺寸并集居中 | `src/system/windows/multiselect.ts` |
| V-27 | 嵌入窗口焦点联动（视觉态） | `src/system/windows/embedFocusLink.ts` |
| V-28 | 焦点跟随鼠标（pointerFocusVwm） | `src/system/windows/vwm.ts` |
| V-29 | 窗口色带标记：按应用着色标题栏，编排中心可配置 | `src/system/windows/colorBand.ts` |
| V-30 | 拖拽中断回弹：Esc 取消拖拽、DRAG_CANCEL_MS 回弹动画 | `src/system/windows/dragCancel.ts` |

### 集成入口
- 编排中心 `WindowOrchestrator.tsx`（Ctrl+Alt+O 呼出）：时间机器/场景/规则三页签 + U-14/V-29 编排设置。
- `VirtualWindowManager.tsx`：N-03 钩子安装、N-01 自动快照订阅、V-22 启动体检、V-25/V-24/N-02 热键、多选工具条。
- `VirtualWindowFrame.tsx`：U-14 预测、V-21/23/24/26/29/30、N-04、M-06 挂起恢复接线。

## 二、协作修复（阻塞项）
1. **后端补命令 `proc_suspend` / `proc_resume`**（`src-tauri/src/shell/taskman.rs` + `lib.rs` 注册 + `ipc.ts` 绑定）：前端 `VirtualWindowFrame` 的 M-06 挂起/恢复原先调用不存在的 `winResume/winSuspend`，改为 ntdll `NtSuspendProcess/NtResumeProcess` 动态加载实现，已注册。
2. **WeatherBadge.tsx 204 行语法损坏**（`<cur.size? Cur />`）：阻塞整体 `tsc` 编译，按上下文恢复为数据图标条件渲染（该文件属 AI-03，仅做最小语法修复）。

## 三、自检结果
- **单元测试**：`vitest run src/system/windows/__tests__ src/lib/motion/__tests__/orchestrate.test.ts`
  → **15 个测试文件 / 159 用例全部通过**（rules/scenes/timeline/rescue/pip/stages/multiselect/snap2/winops/winfeel/vwm-feel/embedFocusLink/snapshots/vwm/orchestrate）。
- **tsc**：AI-02 域内文件 0 错误。全仓 tsc 尚有其他 AI 领地错误（WinFeelTab、datavault、ExplorerWindow、taskbar 等，属并发会话在途工作，未越界处理）。
- **i18n**：AI-02 全部新键 zh/en 双语补齐（orch*/scene*/rules*/v24*/v26* 等）；修复 `scApply` 与既有键重复（TS1117）。
- **并发提示**：本次会话期间检测到其他 AI 会话对工作区的并发回滚，AI-02 文件已复核为最终态。

## 四、已知限制（如实）
- N-01 嵌入窗口恢复仅重摆几何（VWM 层快照），embed 域深度恢复待后端批次接管。
- N-05 行为包（勿扰/声音）为空操作（勿扰属 AI-16 通知域）。
- U-14 幽灵预览与最终落位同源，但多显示器混合 DPI 下预测以主工作区为准。
- 规则引擎 stage 动作引用的舞台组不存在时该动作被拒绝并记日志（不静默）。
