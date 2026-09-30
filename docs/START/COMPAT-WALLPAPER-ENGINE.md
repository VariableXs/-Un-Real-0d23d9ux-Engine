# Wallpaper Engine 共存兼容说明 — libcef.dll 0x80000003 崩溃与卡死

> 适用于 Variable 桌面环境与 Wallpaper Engine 同时运行的场景。  
> 现象：Wallpaper Engine 弹框 `libcef.dll 0x80000003` → `.mdmp`，Variable 打不开/卡死/假死。

## 一、为什么会这样（根因）

**不是病毒、不是注入，是双 Chromium GPU 抢资源 + 桌面层抢合成器。**

| 侧 | 技术 | 细节 |
|---|---|---|
| **Wallpaper Engine** | CEF（Chromium Embedded Framework）`libcef.dll` | 它的壁纸 UI 进程就是一套独立 Chromium，带独立 `GPU 进程`。CEF 内部有大量 `DCHECK`，一旦检测到 GPU 沙箱不变量被破坏（显存不足、合成器状态不一致、超时）会主动 `int3` 触发 `0x80000003` 自杀，避免静默错渲染。 |
| **Variable** | Tauri WebView2（Edge Chromium）+ CosmicBackground Ray-Marching 极光 + 场景 shader 视频壁纸 | 同样是完整 Chromium，同样 60fps 抢 GPU。 |
| **桌面层** | Variable `fullscreen + alwaysOnTop=true`覆盖 `Progman/WorkerW` | Wallpaper Engine 的壁纸窗口就建在 `WorkerW` 上。Variable 每次 `Focused(true) → set_always_on_top(true)` 都会把自己强顶到最前，与 WorkerW 在 DWM 合成器里抖 z-order，WE 的 WorkerW 句柄短暂失效。 |
| **时序** | `embed.rs` 的 `EnumWindows` 轮询 | 启动第三方应用时遍历全部可见窗口并 `OpenProcess + QueryFullProcessImageNameW`；若此时 WE 的 CEF 正在初始化/崩溃重启窗口，该查询会放大崩溃时序窗口（非注入）。 |

结果就是：

1. **WE 侧报 0x80000003** —— 对话框里“was likely crashed by another application”并不是说 Variable 注入了它的 DLL，只是 WE 的崩溃归因启发式把“最近的外部全屏独占窗口”列为 suspected causer，Variable 最符合。
2. **Variable 侧卡死** —— 同一 GPU 竞争的另一面：WebView2 的 GPU 进程在 DWM 已被 WE 占满时申请 4K 纹理/帧缓冲会阻塞在 `SwapBuffers`，主线程假死；`alwaysOnTop` 的反复置顶又与 WE 壁纸窗口形成正反馈，越抢越卡。

## 二、已在代码里修了什么（本分支）

| 修复 | 文件 | 行为 |
|---|---|---|
| 新增 `shell/compat.rs` 兼容层 | `src-tauri/src/shell/compat.rs` | `Toolhelp32` 快照检测 `wallpaper64.exe / wallpaper32.exe / wallpaperservice64.exe / wallpaperservice32.exe`（<1ms，零注入、不碰目标内存） |
| 启动自动缓解 | `src-tauri/src/lib.rs` `setup` | 若检测到 WE 已在运行，自动 `set_always_on_top(false)` 并 `emit compat://wallpaper-engine` |
| 运行期 watcher | 同上 `spawn_compat_watcher` | 3s 轮询，WE 中途启动/退出时推事件，前端横幅实时变化 |
| 不再抢置顶 | `on_window_event Focused(true)` | `is_compat_active() == true` 时跳过 `set_always_on_top(true)`，彻底避免 z-order 抖动 |
| 前端横幅 | `src/system/compat/CompatBanner.tsx` + `DesktopShell.tsx` | 桌面顶部悬浮横幅：说明原因、一键兼容/恢复、关闭。`high` 红底 / `mitigated` 绿底；首帧 `ipc.compatCheck()` + 监听后端事件 |
| i18n | `src/i18n/dictionaries.ts` | 三语 `compatWallpaper*` 文案 |
| 命令面 | `src-tauri/src/lib.rs` `generate_handler` | `compat_check / compat_apply / compat_restore` |

> 设计原则：**绝不注入、不终止、不操作 Wallpaper Engine 任何进程**——只调整 Variable 自身。

## 三、用户侧立刻可做的 3 步（不改代码也能脱困）

### 若现在打不开 / 一直卡死

1. **彻底退出 Wallpaper Engine**：托盘右键 Wallpaper Engine → 退出；任务管理器确认 `wallpaper64.exe / wallpaperservice64.exe` 已消失。
2. **再启动 Variable**：此时 `alwaysOnTop` 会保持正常，GPU 负载独占，秒开。
3. **想要两者共存**：启动后在桌面看到红色横幅 → 点 **“一键兼容（取消置顶）”** → 再到 `设置 → 外观 → 桌面壁纸` 把 `着色器/混合/视频` 切为 **纯黑 / 图片**，把 `性能档` 切为 **eco/static**，重启 WE。两者即可共存（Variable 不再抢置顶，显存压力也降一半）。

### 若 WE 已经崩溃弹框

- 弹框里写的 `.../wallpaper_ui_*.mdmp` 是 WE 的转储，可直接删掉；需要报障就去弹框里的 `help.wallpaperengine.io/crash` 上传该 `.mdmp` 给 WE 官方。
- 弹框点 **确定** 后，按上面 1-2 步先让 Variable 单独跑起来，再决定是否以兼容模式共存。

### 已在兼容模式想恢复

- 关闭 Wallpaper Engine → 在 Variable 桌面横幅点 **“恢复独占置顶”**，或重启 Variable（无 WE 时自动恢复）。

## 四、高级：手动降级与开关

| 需求 | 做法 |
|---|---|
| 强制软件渲染（黑屏/花屏时） | 关闭 Variable，执行 `Variable.exe --force-raster`（写入 `force-raster.flag`，下次启动走纯 CPU 渲染） |
| 完全避让 Windows 任务栏 | 设置 → 外观 → “避让 Windows 任务栏”开启（`avoidTaskbar=true`）——更不容易与 WorkerW 冲突 |
| 看当前兼容状态 | 开发者工具 `await ipc.compatCheck()` 或看横幅颜色 |
| 不想看横幅 | 横幅右上角 X 关闭；下次状态翻转才会再弹 |

## 五、常见误判

- **“Variable 是病毒在注入 WE”** —— 不是。Variable 没有任何 `CreateRemoteThread / SetWindowsHookEx / DLL 注入` 代码，`embed.rs` 的 `SetParent` 仅对 Variable 自己拉起的第三方窗口生效，且 `apps.json` 里的 WE 若未登记绝不会被枚举到嵌入链路。
- **“0x80000003 是内存坏”** —— 该码本就是 CEF 的 `DCHECK` breakpoint，不是硬件坏块；重装 WE 不解决，错开 GPU 负载才解决。

## 六、给开发者的验证

```bash
# 前端类型
npm run typecheck   # 或 ./node_modules/.bin/tsc --noEmit
# 后端（需本机有 Rust）— 含新增 compat 单测 3 项
cargo test --workspace  # 期望 131+ 全绿
# IPC 审计
node tools/audit.cjs
```

历史教训已写入 `project_memory.md`：透明窗口会让 `<video>` 静默不渲染；`alwaysOnTop` 必须由兼容态门控，否则与 WorkerW 死锁。
