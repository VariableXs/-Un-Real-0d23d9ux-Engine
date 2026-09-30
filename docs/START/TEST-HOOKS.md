# TEST-HOOKS — 测试钩子规范（M-60）

> AI-15 开放工具组 · 域 11 开放生态。本文是 e2e/集成测试选择器的唯一权威来源。
> 选择器约定：e2e 只允许用 `data-testid` 与「e2e 哨兵事件」，**不允许**依赖 className / 文案 / DOM 层级（它们随样式重构漂移）。

## 1. 命名规范

格式：**`{组件}-{语义}`**，全小写、连字符分隔、英语单词。

| 规则 | 正例 | 反例 |
|---|---|---|
| 组件前缀 = 组件文件名（去掉 Virtual/System 等冗余词后仍需可辨识） | `taskbar`、`startmenu` | `root-div`、`bar1` |
| 语义后缀 = 用途而非外观 | `settings-modal` | `gray-box` |
| 列表项可带索引/动态后缀 | `ot-result`、`vw-frame` | |
| 全局唯一（同一文档树内不得重复渲染同名 testid） | | `panel` 同时用于 QuickPanel 与 StartMenu |

## 2. 15 条主路径钩子（已落地）

| data-testid | 文件 | 主路径 |
|---|---|---|
| `boot-screen` | src/system/boot/BootScreen.tsx | 开机动画 |
| `desktop-shell` | src/system/desktop/DesktopShell.tsx | 桌面就绪 |
| `desktop-icons` | src/system/desktop-icons/DesktopIcons.tsx | 图标区 |
| `taskbar` | src/system/taskbar/Taskbar.tsx | 任务栏 |
| `startmenu` | src/system/startmenu/StartMenu.tsx | 开始菜单 |
| `vwm` | src/system/windows/VirtualWindowManager.tsx | 虚拟窗口层 |
| `settings-modal` | src/features/settings/SettingsModal.tsx | 设置中心 |
| `quick-panel` | src/system/tray/QuickPanel.tsx | 快捷面板/通知中心 |
| `command-palette`* | src/system/palette/CommandPalette.tsx | 命令面板（已有） |
| `lockscreen` | src/system/lockscreen/LockScreen.tsx | 锁屏 |
| `explorer-window` | src/system/explorer/ExplorerWindow.tsx | 文件管理器 |
| `window-orchestrator` | src/system/windows/WindowOrchestrator.tsx | 窗口编排中心 |
| `run-dialog` | src/system/tools/RunDialog.tsx | 运行对话框 |
| `toast-host` | src/components/ToastHost.tsx | 全局提示 |
| `ot-tab` | src/features/settings/OpenToolsTab.tsx | 开放工具页（AI-15） |

\* CommandPalette / OpenToolsTab 在各自文件内已带条目级 testid（如 `palette-input`、`ot-result`）。

## 3. e2e 就绪哨兵（boot ready sentinel）

BootScreen 的 `finish()` 在真实启动完成、退出编排开始前派发一次性事件：

```ts
window.dispatchEvent(new CustomEvent("variable:boot-ready", { detail: { anim } }));
```

e2e 用法（Playwright）：

```ts
await page.waitForFunction(() => (window as any).__bootReady === true);
```

或在测试入口预挂监听：

```ts
await page.evaluate(() => {
  (window as any).__bootReady = false;
  window.addEventListener("variable:boot-ready", () => { (window as any).__bootReady = true; });
});
```

**禁止**用固定 sleep 等待启动完成——进度时间线由真实事件驱动，时长不定。

## 4. 门禁（audit.cjs §5）

`node tools/audit.cjs` 的第 5 节做静态检查：

- 扫描 `src/**/*.{ts,tsx}` 中所有 `data-testid="..."`；
- **同一文件内重复 testid = 失败**（退出码 1）；
- 跨文件重名仅提示（组件可能互斥渲染，人工确认）。

## 5. 迁移与验收

- 既有 e2e / dogfood 脚本逐步迁移到本表选择器；迁移后「零修改通过」= 验收标准。
- 新组件上线时：先在此表登记 testid，再写用例；未登记的 testid 会在 code review 被打回。
