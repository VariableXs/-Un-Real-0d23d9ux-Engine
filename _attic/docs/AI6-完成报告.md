# AI-06 完成报告 — 输入手感组（U-58/U-59、V-61…V-70）

> 状态：全部 12 项已实现并通过验证。
> 分工依据：《ENGINE-Version-01XHI9DN.1.5xw-AI分工图》AI-06 区段。

## 一、交付清单

| 需求 | 内容 | 实现位置 |
| --- | --- | --- |
| U-58 | 键盘覆盖审计 + Ctrl+/ 速查浮层 | `src/lib/inputFeel.ts`（KEYBOARD_COVERAGE）、`src/features/inputFeel/InputFeelRuntime.tsx`、`src/lib/shortcuts.ts`（KNOWN_KEYS 放行 `/`、`f`,） |
| U-59 | 触控适配（44px 目标、长按右键菜单、触控模式） | `src/lib/inputFeel.ts`（touchModeActive、LONG_PRESS_CONTEXT_MS）、`InputFeelRuntime.tsx`（长按合成 contextmenu、:touch CSS 类） |
| V-61 | 鼠标参数面板（速度/双击/滚轮行数/左右键互换，含写回系统与一键回滚） | `src-tauri/src/shell/mousefeel.rs`（mouse_params_get/write/rollback）、`src/lib/ipc.ts`、`src/features/settings/InputFeelTab.tsx` |
| V-62 | 指针方案（系统/大号/高对比/自定义 + 轨迹） | `InputFeelTab.tsx`（方案切换）、`InputFeelRuntime.tsx`（pointer-scheme 类、V-63 轨迹） |
| V-63 | 指针轨迹（canvas 覆盖层，三档，跟随 reduceMotion） | `InputFeelRuntime.tsx` + `src/styles/input-feel.css` |
| V-64 | 点击涟漪（respect reduceMotion → 缩短时长） | 同上 |
| V-65 | CapsLock 大小写提示 | 同上（getModifierState 实时检测 + 浮层提示） |
| V-66 | 按键重映射（单键→单键/修饰键，桌面层 capture 翻译） | `src/lib/inputFeel.ts`（matchRemap/validateRemaps）、`InputFeelRuntime.tsx`（capture 层合成事件，含死循环防护） |
| V-67 | 滚轮方向（自然滚动，含鼠标/触控板启发式区分） | `inputFeel.ts`（shouldInvertWheel）、`InputFeelRuntime.tsx`（wheel 翻转） |
| V-68 | 打字音效（四档：关/机械/薄膜/水滴，WebAudio 本地合成） | `inputFeel.ts`（TYPING_SOUND_PROFILES）、`InputFeelRuntime.tsx` |
| V-69 | 精准模式（按住修饰键临时降指针速度 + 十字辅助线） | `mousefeel.rs`（pointer_speed_temp/restore）、`InputFeelRuntime.tsx` |
| V-70 | 拖拽容差（阈值可调 2–10px，桌面图标与虚拟窗口统一生效） | `inputFeel.ts`（isDragStart/liveDragThreshold）、`DesktopIcons.tsx`、`VirtualWindowFrame.tsx` |

## 二、架构与边界（诚实口径）

- **默认零改变**：全部功能默认关闭或等于现状；设置持久化于 `Settings.inputFeel`（`src/lib/settings.ts`，含 coerce 兜底）。
- **挂载点**：`DesktopShell.tsx` 挂载 `InputFeelRuntime` 一次，经 `setLiveInputFeel` 快照供无 props 调用点（DesktopIcons / VirtualWindowFrame）读取拖拽阈值。
- **键位红线**：U-58 速查浮层经 Z-08 键位注册表（`src/lib/keymap/registry.ts`）注册（id `u58-cheatsheet`，priority 10），未经仲裁不得注册。
- **V-66 诚实边界**：重映射在桌面环境 webview 的 capture 层生效；宿主级低级键盘钩子被系统静默忽略（见 `shell/kbdhook.rs` 注释），「对四大独立窗口全局生效」登记为待验清单。
- **V-63/64 边界**：轨迹与涟漪只覆盖桌面环境窗口（本 webview），不越权注入四大内置应用窗口。
- **V-67 边界**：浏览器不暴露滚轮来源，以「近期鼠标移动」启发式区分；外接鼠标移动过则恒为鼠标语义。
- **平台**：`mousefeel.rs` 全部命令 Windows 实现，非 Windows 返回 `not-supported`（前端如实提示）。

## 三、验证结果

- `npx vitest run src/lib/__tests__/inputFeel.test.ts`：**23/23 通过**（参数钳制、方案校验、重映射校验与匹配、自然滚动方向、精准模式、拖拽阈值、触控模式、键盘覆盖审计）。
- `npx tsc --noEmit`：AI-06 涉及文件**零错误**（仓库现存 keymap/timeline 相关错误属其他 AI 区段，未越权处理）。
- i18n 词典：zh/en 各新增 104 键，audit 通过。

## 四、变更文件

新增：`src/lib/inputFeel.ts`、`src/lib/__tests__/inputFeel.test.ts`、`src/features/inputFeel/InputFeelRuntime.tsx`、`src/features/settings/InputFeelTab.tsx`、`src/styles/input-feel.css`、`src-tauri/src/shell/mousefeel.rs`、本报告。
修改：`src/lib/settings.ts`、`src/lib/shortcuts.ts`、`src/lib/ipc.ts`、`src/i18n/dictionaries.ts`、`src/features/settings/SettingsModal.tsx`、`src/system/desktop/DesktopShell.tsx`、`src/system/desktop-icons/DesktopIcons.tsx`、`src/system/windows/VirtualWindowFrame.tsx`、`src-tauri/src/lib.rs`、`src-tauri/src/shell/mod.rs`。
