# 设计文档：壁纸中心 + 任务栏智能让位 + Steam 兼容层扩展

日期：2026-09-09
状态：已获用户批准（brainstorming 流程）
来源：用户实机反馈——Steam 版 Wallpaper Engine UI 难以在本地环境运行；打开 Steam 时 Windows 任务栏与 Variable 底栏重叠；Steam 运行时偶发卡死。

## 背景与目标

用户使用 Steam 版 Wallpaper Engine（WE）管理壁纸，存在四个痛点：

1. WE 的 UI（CEF 应用）在 Variable 环境内运行不稳定（libcef 0x80000003 崩溃，compat.rs 已有根因分析：双 Chromium GPU 竞争 + z-order 抖动）。
2. 壁纸管理必须打开 Steam/WE 的 Windows 窗口，脱离 Variable 界面。
3. Steam 打开时 Windows 任务栏浮到 Variable 全屏窗口上方，与 Variable 自己的底栏重叠。
4. Steam 与 Variable 同为 Chromium 系，运行时偶发卡死。

目标（用户已逐项确认）：

- **壁纸中心**：新建独立 Tauri 窗口（用户明确选择独立窗口而非覆盖层，接受 GPU 开销），WE 风格三栏布局；Workshop 独立入口保留（两者都要）。
- **智能让位**：检测到 Windows 任务栏浮现时 Variable 底栏自动上移让位，消失后归位；零系统侵入。
- **卡死缓解**：把现有 WE 兼容层扩展到 Steam（自动取消置顶 + 降级壁纸 GPU 负载）。

## 现有基础（探索结论）

| 能力 | 位置 | 状态 |
|------|------|------|
| 壁纸 8 模式（含 living 活化：图片+粒子+Ken Burns） | `src/system/wallpaper/WallpaperLayer.tsx`、`LivingWallpaper.tsx` | 已有 |
| 壁纸工坊（video/shader/generative/web 四引擎 + .vwp 包） | `src/system/wallpaper/Workshop.tsx` | 已有，覆盖层挂载 |
| WE 创意工坊/项目扫描 | Rust `wp_engine_scan`（wallpaper.rs） | 已有，埋在设置弹窗 |
| 着色器本地 WebGL 渲染 | `SceneShaderWallpaper.tsx` + Rust `wp_scene_shader` | 已有 |
| WE 进程兼容层（3s watcher + alwaysOnTop 让位） | `src-tauri/src/shell/compat.rs` + `CompatBanner.tsx` | 已有，只监控 WE 进程 |
| MPA 多窗口模式（独立 HTML 入口 + WebviewWindow + 几何记忆） | `appWindows.ts`、vite.config、`app-*.html` | 已有，壁纸中心直接复用 |

## 设计一：壁纸中心（独立窗口）

### 窗口架构

- 新 HTML 入口 `wallpaper-center.html`；vite.config `rollupOptions.input` 增加 `"wallpaper-center": html("wallpaper-center")`。
- 窗口 label `wallpaper-center`；`appWindows.ts` 新增 `openWallpaperCenter()`：已存在 → 取消最小化并聚焦；否则 `new WebviewWindow`，`alwaysOnTop: true`（与置顶桌面同 topmost 组，激活序在其上）、`decorations: false`、`dragDropEnabled: true`、几何记忆复用 `variable:win:geom:v1`、默认 1280×800 / min 960×640 / center。
- 懒创建、用户关闭即销毁（`destroy` 而非 `hide`，释放 WebView 进程与 GPU）。
- 入口三处：任务栏菜单（`taskbarMenu.ts` 注册表新增 `wallpaperCenter` 项，`defaultVisible: true`，排序默认插在 launcher 之后）；桌面右键菜单新增「壁纸中心」；Workshop 页脚新增「打开壁纸中心」按钮。
- Workshop 独立入口（`ai04:open-feature` overlay）保留不动。

### 三栏界面（对标 WE 布局）

| 区域 | 内容 |
|------|------|
| 左栏·已安装（默认） | 网格合并四来源：① `wp_engine_scan`（创意工坊 431960 + myprojects）；② 本地图片目录（`wallpaperPoolDir` + 用户在中心内添加的目录）；③ .vwp 包导入；④ 当前 `customBg`。卡片 = 预览图（preview.gif 动图优先，着色器走本地 WebGL 缩略编译）+ 标题 + 类型角标（视频/图片/着色器/网页/包） |
| 左栏·创建 | 内嵌 Workshop 四引擎。做法：从 `Workshop.tsx` 提取引擎面板组件到 `src/system/wallpaper/engines/` 共享目录，Workshop 与壁纸中心共同 import（不复制代码） |
| 右栏·预览/属性 | 大预览（点击播放视频/着色器实时渲染）+ 按类型属性面板：<br>· 静态图 → 活化开关 + 活化强度（粒子密度）、Ken Burns 速度/幅度、粒子风格（尘埃/bokeh）<br>· 视频 → 音量、播放速度、对齐（cover/contain）<br>· 着色器 → uniform 滑杆（复用 `uniforms.ts` 的 `parseUniforms`）<br>· 公共 → 混合星空叠加开关（hybrid） |
| 底栏·播放列表 | 轮播管理：成员列表、间隔（分钟）、顺序/随机、立即换一张 |

「应用」按钮 → 生效当前选中壁纸 + 属性。

### 跨窗口设置同步（数据流）

- settings.ts 仍是唯一真相，壁纸中心**不直接写**设置存储。
- 壁纸中心窗口：Tauri 事件 `wallpaper://apply`（payload = `{ patch: { wallpaperMode, customBg } }`）。
- desktop 窗口：新增监听（挂载在现有 Workshop 集成点旁），收到后走同一 `onPatchSettings` 浅合并路径（与 `ai04:wallpaper-apply` 同语义）。
- 壁纸中心内先行本地预览（Workshop 同策略）。
- `CustomBg` 增加字段：`livingIntensity: number`（0..1 粒子密度）、`livingDrift: number`（0..1 Ken Burns 幅度/速度）、`particleStyle: "dust" | "bokeh" | "mixed"`——带默认值，旧设置向后兼容。

### GPU 负载自律（配合卡死缓解）

- 壁纸中心窗口打开期间：desktop 的壁纸动画自动降档（粒子暂停、Ken Burns 暂停，静态帧渲染）——壁纸中心本身是最大的活动窗口，桌面被完全遮挡，动画无意义。窗口销毁后恢复。
- 实现：壁纸中心窗口在挂载/卸载时发 app-wide Tauri 事件 `wallpaper://center-open` / `wallpaper://center-closed`，desktop 侧监听并复用现有 `isDegradeActive` 性能门控。

## 设计二：任务栏智能让位

### Rust 端（新文件 `src-tauri/src/shell/taskbar_yield.rs`）

- 检测：`FindWindowW("Shell_TrayWnd")` + `IsWindowVisible` + `GetWindowRect`；每 1s 轮询（与 compat watcher 同线程模式）。自动隐藏任务栏的语义天然覆盖：隐藏时 `IsWindowVisible=false` 或矩形在屏幕外 → 不让位；悬停/前台切换浮现时矩形侵入屏幕 → 让位。
- 判定「任务栏正浮在 Variable 上」：`Shell_TrayWnd` 可见 且 其矩形与桌面窗口矩形相交 且 Variable 非前台。
- 状态变化时 emit `sys://taskbar-yield` `{ visible: bool, height: u32 }`（height = 任务栏侵入高度，供前端让位距离用）。
- 注册进 lib.rs 命令表（`taskbar_yield_check` 手动查询命令 + `spawn_taskbar_yield_watcher` 启动钩子）。

### 前端（`Taskbar.tsx`）

- 监听 `sys://taskbar-yield`：`visible=true` → 底栏 `translateY(-height)` 上移让位 + 阴影弱化；`visible=false` → 归位。
- CSS transition 300ms ease；`reduceMotion/safeMode` → 无动画直接跳变。
- 与兼容层联动（自动成立，无需额外代码）：Steam/WE 运行 → `alwaysOnTop=false` → 失去前台时 Windows 任务栏可见 → 让位生效。

## 设计三：Steam 兼容层扩展（compat.rs）

- `WE_PROCS` 数组扩展：`"steam.exe"`、`"steamwebhelper.exe"`（小写后缀匹配，覆盖任意安装路径）。
- 事件 `compat://wallpaper-engine` 更名 `compat://cef-apps`，语义扩为「CEF 系应用兼容（Wallpaper Engine / Steam）」；`CompatBanner.tsx` 同步改监听名与文案（中英文案均提 Steam）。
- 命中行为不变：`apply_compat_mode`（`alwaysOnTop=false` + 前端降级壁纸 GPU 负载）；恢复走 `compat_restore`。
- `steam_launch`（ecosystem.rs）：启动前**主动**调用 `apply_compat_mode`（不等 3s watcher），杜绝启动瞬间 z-order 抖动。
- watcher 现状「WE 消失后 compat 保持到手动恢复」——扩展后 Steam 进程频繁启停（游戏内 Steam overlay 等），改为：**所有 CEF 进程消失 → 自动 emit 清除状态，并延迟 30s 自动恢复置顶**（防抖：30s 内再现则取消恢复）。前端 CompatBanner 状态同步。

## 错误处理

| 场景 | 行为 |
|------|------|
| 着色器编译失败 | 回退静态预览图 + 诚实提示（SceneShaderWallpaper 现有策略，绝不黑屏） |
| 无 Steam / 扫描失败 | 空状态 + 「选择目录扫描」按钮（wp_engine_scan 支持手动 root） |
| 窗口创建失败 | console.error + toast，不崩溃 |
| .vwp 包损坏 | validateVwp 现有错误路径 |
| 跨窗口事件丢失 | 壁纸中心应用后 500ms 内收不到 desktop 的 `wallpaper://applied` 回执则重试一次，再失败 toast 提示手动检查 |

## 测试计划

- **vitest**：taskbarMenu 注册表含新项且默认可见；属性面板滑杆 clamp 逻辑；播放列表增删/间隔/顺序纯函数；CustomBg 新字段默认值与向后兼容解析。
- **cargo test**：compat 进程匹配覆盖 steam.exe/steamwebhelper.exe；taskbar_yield 判定纯函数（矩形相交/前台组合）；自动恢复 30s 防抖逻辑。
- **实机手测**：开 Steam → 任务栏让位 + 兼容横幅出现；关 Steam → 30s 后置顶恢复、底栏归位；壁纸中心应用静态图 → desktop living 生效。

## 明确不做（YAGNI）

- 不做 Steam 创意工坊在线订阅/下载（只扫描本地已有项目，零网络红线不变）。
- 不修改 Windows 系统设置（任务栏自动隐藏等只引导不强制——本次连引导也不做，智能让位已解决）。
- 不做 WE scene 完整运行时（继续 WebGL 着色器本地渲染路线）。
- 不新建壁纸中心以外的窗口。
