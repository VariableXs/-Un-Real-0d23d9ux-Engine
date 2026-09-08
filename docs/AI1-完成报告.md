# AI-01 窗口手感组 交付报告

范围：Z-36…Z-42、M-01…M-09（不做其他 AI 任务）。

## 交付清单

| 编号 | 功能 | 实现位置 |
| --- | --- | --- |
| Z-36 | 不透明度/置顶控制（标题栏右键菜单 + 逐应用透明度记忆） | `VirtualWindowFrame.tsx`、`winfeelMenu.ts`、`vwm.ts(setVwmOpacity/setVwmTopmost)` |
| Z-37 | 几何记忆（窗口位置/尺寸按应用恢复） | `vwm.ts`（既有 settle 扩展） |
| Z-38 | 滚轮行为（Alt+滚轮置顶/穿透热区） | `VirtualWindowFrame.tsx`、`settings.ts` |
| Z-39 | 标题栏自定义（关闭键 hover 规范、右键系统菜单共用样式） | `vwm.css`、`VirtualWindowFrame.tsx` |
| Z-40 | 布局快照（保存/恢复窗口布局） | `vwm.ts(applyLayoutSnapshot/saveLayoutSnapshot)`、`VirtualWindowFrame.tsx` |
| Z-41 | 鼠标手势（右键拖 上=最小化 / 下=关窗，24px 容差） | `winfeel.ts(detectGesture)`、`VirtualWindowFrame.tsx` |
| Z-42 | 虚拟桌面切换器（右缘热区 + Ctrl+Alt+G） | `DesktopSwitcher.tsx`、`VirtualWindowManager.tsx` |
| M-01 | Aero Shake 摇一摇最小化（Ctrl+Alt+D 还原） | `winfeel.ts(detectShake)`、`vwm.ts(shakeMinimizeOthers)` |
| M-02 | 窗口卷帘（标题栏滚轮 roll-up） | `vwm.ts(rollVwmWin)`、`VirtualWindowFrame.tsx` |
| M-03 | 最小化抽屉（底部任务栏右侧面板） | `MinimizedDrawer.tsx` |
| M-04 | 窗口体检（IsHungAppWindow 3s 轮询，"未响应"徽标） | `winman.rs(win_health_scan)`、`ipc.ts(winHealthScan)`、`VirtualWindowManager.tsx` |
| M-05 | 跨屏摆渡（拖到屏幕边缘飞往邻屏） | `winfeel.ts(screenShift)`、`vwm.ts(ferryVwmWin)` |
| M-06 | 窗口挂起/恢复（第三方窗口冻结，退出自动恢复） | `winman.rs(win_suspend/win_resume)`、`winfeelMenu.ts(suspensionStore)` |
| M-07 | 对齐参考线（边缘/中线吸附，8px 阈值） | `winfeel.ts(computeGuide)`、`vwmStore.guides` |
| M-08 | 精细 Alt+Tab（键盘选择器） | `VirtualWindowManager.tsx` |
| M-09 | X-Mouse 跟随焦点（悬停激活，可设延迟） | `vwm.ts(pointerFocusVwm)`、`settings.ts` |

设置面板：`src/features/settings/WinFeelTab.tsx`（已注册到设置弹窗），i18n zh/en 全键补齐。

## 验证结果

- vitest：`winfeel.test.ts` 22/22、`vwm-feel.test.ts` 20/20 通过；全量 831 passed，2 个失败用例均在 AI9 领域（`explorer/ctxMenu`、模板中心），与本次改动无关。
- tsc：AI1 相关文件（system/windows、features/settings、lib/settings、lib/ipc、i18n）零错误；现存错误均在其他 AI 未提交领域（datavault 等）。
- cargo check：通过（仅既有 warning）。

## 已知限制

- M-04 体检仅覆盖嵌入第三方进程窗口（IsHungAppWindow 依赖真实句柄）。
- M-06 挂起仅对嵌入 webview 生效；引擎退出时强制恢复所有已挂起窗口。
- M-05 摆渡基于显示器坐标映射，DPI 缩放差异下按系统缩放换算。
- Z-41 手势默认关闭，需在"窗口手感"设置中开启。
