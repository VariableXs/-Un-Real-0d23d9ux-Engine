# AI-08 基础工具组 交付报告

> 范围：Z-22…Z-28、U-17、U-18、V-97、V-98（共 11 项）。只做 AI-08 任务，未动其他组任务。

## 交付清单

### Z-22 时钟中心（Clock Hub）
- `src/system/tools/clockhub.ts`：纯逻辑（多时区、闹钟、秒表、倒计时、世界时钟换算）
- `src/system/tools/ClockHubApp.tsx`：VWM 工具窗应用（四大标签页）
- `src/system/tools/__tests__/clockhub.test.ts`：15 项单测
- `src/styles/ai08-clock.css`

### Z-23 天气信息卡（Weather Card）
- `src/system/taskbar/weatherService.ts`：wttr.in 免费源 + 本地缓存 + 失败降级（纯逻辑）
- `src/system/taskbar/WeatherBadge.tsx`：任务栏小组件（配置未启用时不渲染、不挂定时器）
- `src/system/taskbar/__tests__/weatherService.test.ts`：10 项单测
- `src/styles/ai08-weather.css`
- 集成：`Taskbar.tsx` 单点挂载

### Z-24 字符与 Emoji 面板
- `src/system/tools/emojiData.ts`：Emoji 数据集与关键词搜索（中英）
- `src/system/tools/EmojiPanelApp.tsx`：分类浏览 + 搜索 + 最近使用 + 复制
- `src/system/tools/__tests__/emoji.test.ts`：14 项单测
- `src/styles/ai08-emoji.css`

### Z-25 放大镜与取色器
- `src/system/tools/magnifierCore.ts`：放大档位/形状/取色格式化纯逻辑
- `src/system/tools/MagnifierApp.tsx`：倍率 1–16×、取色 HEX/RGB/HSL、复制
- `src/system/tools/__tests__/magnifierCore.test.ts`：15 项单测
- `src/styles/ai08-magnifier.css`

### Z-26 换算中心（Converter Hub）
- `src/lib/convert.ts`：长度/重量/温度/面积/体积/速度/数据量/时间 8 大类纯逻辑
- `src/system/tools/ConverterApp.tsx`：双向换算 + 实时精度
- `src/lib/__tests__/convert.test.ts`：23 项单测
- `src/styles/ai08-convert.css`

### Z-27 系统信息面板
- `src/system/tools/SysInfoApp.tsx`：sysinfo IPC 汇总（CPU/内存/磁盘/系统版本），只读

### Z-28 运行对话框（Run Dialog）
- `src/system/tools/runParse.ts`：别名/路径/URI/可执行名分类与历史（≤20，localStorage）
- `src/system/tools/RunDialog.tsx`：全局浮层，Enter 执行 / Esc 关 / ↑↓ 补全 / Tab 确认
- `src/system/tools/__tests__/runParse.test.ts`：14 项单测
- `src/styles/ai08-run.css`
- 热键：`ctrl+alt+r`（Win+R 被系统保留 → 降级口径），Rust `winman.rs` 分发 `sys://open-run`，`DesktopShell.tsx` 挂载浮层
- 集成：`shortcuts.ts`（SHORTCUT_ACTIONS）

### U-17 全局拖放总线（Global Drop Bus）
- `src/lib/dnd/bus.ts`：会话总线（发布/订阅/目标注册，无会话零开销）
- `src/lib/dnd/DragGhost.tsx`：拖拽幽灵 + 收藏托盘视觉层
- `src/lib/dnd/__tests__/bus.test.ts`：26 项单测
- `src/styles/ai08-dnd.css`
- 集成：`DesktopShell.tsx` 挂载 `<DndLayer>`

### U-18 迷你应用框架（Mini Apps Framework）
- `src/system/vwm/miniframe.tsx`：胶囊头小窗框架（拖动/置顶/折叠）
- `src/apps/mini/registry.ts`：注册表（id/名称/默认尺寸）
- 五件首发：`MiniWorldClock` / `MiniPomodoro` / `MiniCalculator` / `MiniNotes` / `MiniCountdown`
- `src/apps/mini/__tests__/registry.test.ts`：6 项单测
- `src/styles/ai08-mini.css`
- 集成：`DesktopShell.tsx` 挂载 `<MiniAppsLayer>`

### V-97 右键打印
- 集成：`ctxMenu.ts`（print 项）+ `ExplorerWindow.tsx`（printFiles：关联检查走注册表，无关联如实置灰/提示；批量 >5 需确认）
- 后端 `print_files` / `print_assoc_check` 已在 HEAD 注册

### V-98 打印队列查看器
- `src/system/tools/PrintQueueApp.tsx`：队列列表 + 暂停/恢复/取消/取消全部，轮询刷新
- `src/styles/ai08-print.css`
- VWM 注册：`printqueue` 工具（vwm.ts / StartMenu.tsx / VwmAppContent.tsx 已在 HEAD）

## 共享文件改动
- `src-tauri/src/shell/winman.rs`：`runDialog` 热键分发（+5 行）
- `src-tauri/src/shell/print.rs`：修正 `printer_status_text` 测试断言（0x10=paper-out、0x400=printing）
- `src/lib/shortcuts.ts`：+runDialog 动作
- `src/styles/global.css`：+ai08-print.css import
- `src/system/desktop/DesktopShell.tsx`：DndLayer / MiniAppsLayer / RunDialog 挂载
- `src/system/taskbar/Taskbar.tsx`：WeatherBadge 挂载
- `src/system/windows/VwmAppContent.tsx`：六件工具渲染分支

## 自检结果
- vitest（AI-08 8 个测试文件）：**123/123 通过**
- tsc --noEmit：AI-08 文件**零错误**（现存错误均在其他组文件：compat/lockscreen/multiselect/snap2 等）
- tools/audit.cjs：无 AI-08 缺失键（MISSING ZH/EN 均为其他组的 cp*/scene* 等键）
- cargo test print：**被 HEAD 既有损坏阻塞**——已验证纯 HEAD 不含 AI-08 改动即有 31 条编译错误（lib.rs 引用 compat_* 命令但 compat.rs 实现未提交 + AI-11 sysprobe/winpower 未提交文件），非 AI-08 引入；print.rs 的 Rust 单测断言修正已随本次交付提交

## 已知边界（如实）
- Z-23 天气：wttr.in 免费接口，离线时显示缓存/降级文案，不做付费源
- Z-25 放大镜：webview 内放大（无内核级全屏放大）
- Z-28：Win+R 为系统保留键，采用 ctrl+alt+r 降级口径；Win+R 转译属键位纪律组（Z-09）领地
- U-17：拖放总线仅覆盖前端层（浏览器 drag 会话），内核级文件锁不涉及
- V-97：右键打印依赖系统打印关联（无关联时如实提示，不瞎猜）
- V-98：仅枚举打印队列（暂停/恢复/取消单个任务），打印机首选项设置不涉及
