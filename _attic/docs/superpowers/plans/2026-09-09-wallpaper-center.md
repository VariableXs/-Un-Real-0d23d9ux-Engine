# 壁纸中心 + 任务栏智能让位 + Steam 兼容层扩展 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Variable 内建本地壁纸中心（独立 Tauri 窗口，无需打开 Steam），静态图片可加载自定义动态活化效果；Steam 打开时 Windows 任务栏与 Variable 底栏不再重叠（智能让位）；Steam 运行时不再卡死（兼容层扩展）。

**Architecture:** 三条独立子线复用既有基建：① 壁纸中心 = 新 MPA 窗口 `wallpaper-center.html`（复用 appWindows.ts 多窗口模式 + Workshop 组件嵌入）；跨窗口设置同步走 Tauri 事件 `wallpaper://apply` → desktop 侧 bridge → `onPatchSettings`。② 任务栏让位 = Rust 轮询 `Shell_TrayWnd` → 事件 `sys://taskbar-yield` → Taskbar 上移。③ 卡死缓解 = compat.rs 进程表扩展 steam.exe/steamwebhelper.exe + 30s 防抖自动恢复 + `steam_launch` 主动退让。

**Tech Stack:** Tauri v2（Rust windows crate 0.58，`Win32_UI_WindowsAndMessaging` 已启用）、React 18 + zustand、vitest、cargo test。

**规格：** `docs/superpowers/specs/2026-09-09-wallpaper-center-design.md`

**诚实裁剪（相对规格的偏差，均为有意识的边界）：**
- 视频壁纸属性面板只保留 播放开关 + 通用调节（模糊/亮度/饱和度/暗角，均为既有 CustomBg 字段）；音量/播放速度/对齐裁剪——壁纸视频当前恒静音播放（产品红线：壁纸不发声），速度/对齐无既有字段，核心诉求（静态图活化属性）完整交付。
- 本地图片目录用新增 Rust 命令 `wp_list_images` 枚举（目录含子图片平铺），不做递归监控。
- Workshop standalone 入口此前**不可达**（无任何代码派发 `ai04:open-feature {feature:"wallpaper-studio"}`，`ai04:wallpaper-apply` 无消费者）——本计划第 8 任务首次接通它（桌面右键菜单 + bridge 消费应用事件），兑现规格"Workshop 独立入口保留"。

---

### Task 1: Rust compat.rs 扩展（Steam 进程 + 事件更名 + 30s 自动恢复 + steam_launch 主动兼容）

**Files:**
- Modify: `src-tauri/src/shell/compat.rs`
- Modify: `src-tauri/src/shell/ecosystem.rs:331-341`（steam_launch）

- [ ] **Step 1.1: 扩展进程表与事件名（含失败测试先行）**

在 `compat.rs` 测试模块追加（先写测试）：

```rust
    #[test]
    fn cef_procs_cover_steam() {
        // Steam 主进程 + webhelper 与 WE 同表监控（小写后缀匹配）
        let procs = ["steamwebhelper.exe", "steam.exe"];
        for p in procs {
            assert!(
                WE_PROCS.iter().any(|pat| p.ends_with(pat) || pat == &p),
                "steam proc {p} should be monitored"
            );
        }
    }
```

- [ ] **Step 1.2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml cef_procs_cover_steam`
Expected: FAIL（WE_PROCS 尚无 steam 条目）

- [ ] **Step 1.3: 修改 WE_PROCS 与文档注释**

`compat.rs` 中（L36-44 区域）：

```rust
/// CEF 系应用进程名（小写后缀匹配，覆盖 32/64 位）。
/// 实机反馈：Steam 与 Wallpaper Engine 同为 Chromium 系——GPU 竞争与
/// z-order 抖动同源（libcef 0x80000003），故同表监控同策略让位。
const WE_PROCS: &[&str] = &[
    "wallpaper64.exe",
    "wallpaper32.exe",
    "wallpaperservice64.exe",
    "wallpaperservice32.exe",
    "wallpaperservice.exe",
    "wallpaper engine.exe",
    "steam.exe",
    "steamwebhelper.exe",
];
```

同时把模块头注释（L1-24）中"Wallpaper Engine 共存兼容层"改为"CEF 应用共存兼容层（Wallpaper Engine / Steam）"，并补一行：Steam 同为 CEF 应用，命中同策略（取消置顶 + 降级壁纸负载）。

- [ ] **Step 1.4: 事件更名 compat://wallpaper-engine → compat://cef-apps**

`compat.rs` 中三处 `app.emit("compat://wallpaper-engine", ...)` 全部改为 `app.emit("compat://cef-apps", ...)`（apply_compat_mode L153、compat_restore L166、spawn_compat_watcher L197）。

- [ ] **Step 1.5: recommendation_text 更新（提 Steam）**

```rust
fn recommendation_text(running: bool, compat: bool) -> String {
    if !running {
        String::new()
    } else if compat {
        "已自动进入 CEF 应用兼容模式（Wallpaper Engine / Steam）：已取消独占置顶并降低壁纸 GPU 负载。\nSteam 等退出约 30 秒后自动恢复置顶。\nAuto compat mode for CEF apps (Wallpaper Engine / Steam): always-on-top disabled & wallpaper GPU load reduced.".into()
    } else {
        "检测到 Wallpaper Engine / Steam 正在运行，与 Variable 的全屏独占 + 双 Chromium GPU 进程存在已知冲突（libcef 0x80000003）。\n建议：点“一键兼容”让 Variable 取消独占置顶并降级壁纸渲染；CEF 应用全部退出约 30 秒后自动恢复。\nWallpaper Engine / Steam detected — known conflict with Variable fullscreen + dual Chromium GPUs (libcef 0x80000003).".into()
    }
}
```

- [ ] **Step 1.6: watcher 30s 防抖自动恢复**

替换 `spawn_compat_watcher`（L182-203）整体为：

```rust
pub fn spawn_compat_watcher(app: AppHandle) {
    std::thread::Builder::new()
        .name("compat-watcher".into())
        .spawn(move || {
            let mut last_running = is_wallpaper_engine_running();
            // 进程全部消失的时刻（None = 尚在运行或从未让位）
            let mut gone_since: Option<std::time::Instant> = None;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let running = is_wallpaper_engine_running();
                if running != last_running {
                    last_running = running;
                    gone_since = None;
                    if running {
                        eprintln!("[compat] CEF app appeared at runtime -> compat on");
                        apply_compat_mode(&app);
                    } else {
                        eprintln!("[compat] CEF apps gone -> emitting cleared status (auto-restore in 30s)");
                        let _ = app.emit("compat://cef-apps", current_status());
                    }
                } else if !running && is_compat_active() {
                    // 30s 防抖：进程再次出现则取消（上面分支已清 gone_since）
                    if gone_since.is_none() {
                        gone_since = Some(std::time::Instant::now());
                    } else if gone_since.unwrap().elapsed().as_secs() >= 30 {
                        eprintln!("[compat] CEF apps gone for 30s -> auto restore always-on-top");
                        if let Err(e) = compat_restore(app.clone()) {
                            eprintln!("[compat] auto restore failed: {e:?}");
                        }
                        gone_since = None;
                    }
                } else {
                    gone_since = None;
                }
            }
        })
        .ok();
}
```

- [ ] **Step 1.7: steam_launch 启动前主动进入兼容态**

`ecosystem.rs` L331-341 改为：

```rust
/// steam:// 协议直通（rungameid / store 前台由 Steam 自管）。
/// 启动前主动进入 CEF 兼容态：不等 3s watcher，杜绝启动瞬间 z-order 抖动。
#[tauri::command]
pub fn steam_launch(app: tauri::AppHandle, app_id: String) -> CmdResult<()> {
    if app_id.is_empty() || !app_id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::validation("Steam AppID 必须是数字 / Steam AppID must be numeric"));
    }
    crate::shell::compat::apply_compat_mode(&app);
    let url = format!("steam://rungameid/{app_id}");
    // URI associations belong to Windows Shell; cmd/start is intentionally
    // not used because it breaks quoting and profile environment semantics.
    crate::shell::compat::shell_execute_path(Path::new(&url), Some("open"), None, None, None)?;
    Ok(())
}
```

（`AppHandle` 由 Tauri 注入，前端 `ipc.steamLaunch(appId)` 调用不变。）

- [ ] **Step 1.8: 跑测试 + 检查事件名引用**

Run: `cargo test --manifest-path src-tauri/Cargo.toml compat`
Expected: PASS（含新 cef_procs_cover_steam 与既有 4 项）

Run（前端引用核对，应只剩 CompatBanner 一处，Task 3 改）: `rg -n "compat://wallpaper-engine" src/ src-tauri/src/`
Expected: 仅 `src/system/compat/CompatBanner.tsx:32`

- [ ] **Step 1.9: cargo check 全量**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 0 error

- [ ] **Step 1.10: Commit**

```bash
git add src-tauri/src/shell/compat.rs src-tauri/src/shell/ecosystem.rs
git commit -m "feat(compat): 兼容层扩展到 Steam——进程同表监控+30s防抖自动恢复+启动前主动让位"
```

---

### Task 2: Rust taskbar_yield.rs + wp_list_images + 注册

**Files:**
- Create: `src-tauri/src/shell/taskbar_yield.rs`
- Modify: `src-tauri/src/shell/wallpaper.rs`（追加 wp_list_images）
- Modify: `src-tauri/src/shell/mod.rs`（追加 `pub mod taskbar_yield;`）
- Modify: `src-tauri/src/lib.rs`（setup 挂 watcher + 命令注册）

- [ ] **Step 2.1: taskbar_yield.rs（纯函数测试先行已含在文件内）**

创建 `src-tauri/src/shell/taskbar_yield.rs`：

```rust
//! L3 shell — taskbar_yield.rs：Windows 任务栏智能让位检测。
//!
//! 背景：Variable 桌面 fullscreen + alwaysOnTop 时，Steam 等 CEF 应用获得前台
//! 会把 Windows 任务栏（Shell_TrayWnd）顶到 Variable 全屏窗口上——与 Variable
//! 自身底栏重叠。本模块 1s 轮询任务栏可见性/矩形，与桌面窗口求交，
//! 状态变化推事件 `sys://taskbar-yield` { visible, height }，前端底栏上移让位。
//!
//! 自动隐藏任务栏语义天然覆盖：隐藏时不可见/矩形在屏外 → 不让位；
//! 悬停/前台切换浮现时矩形侵入屏幕 → 让位。
//! 仅 Windows 有真实行为；其余平台恒 visible=false。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::CmdResult;

#[derive(Serialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarYieldStatus {
    /// Windows 任务栏正浮在桌面窗口上（前端应让位）
    pub visible: bool,
    /// 任务栏侵入桌面窗口底边的高度（物理 px，让位距离）
    pub height: u32,
}

/// 纯判定：两矩形相交返回纵向重叠高度，否则 0。
pub fn overlap_height(tray: (i32, i32, i32, i32), desk: (i32, i32, i32, i32)) -> u32 {
    let ix1 = tray.0.max(desk.0);
    let iy1 = tray.1.max(desk.1);
    let ix2 = tray.2.min(desk.2);
    let iy2 = tray.3.min(desk.3);
    if ix1 < ix2 && iy1 < iy2 {
        (iy2 - iy1) as u32
    } else {
        0
    }
}

#[cfg(windows)]
fn current_status(app: &AppHandle) -> TaskbarYieldStatus {
    use tauri::Manager;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowRect, IsWindowVisible, RECT,
    };

    let Some(win) = app.get_webview_window("desktop") else {
        return TaskbarYieldStatus::default();
    };
    // 前台仍是桌面窗口时不让位（fullscreen 独占下任务栏本就隐藏）
    if win.is_focused().unwrap_or(false) {
        return TaskbarYieldStatus::default();
    }
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return TaskbarYieldStatus::default();
    };
    let desk = (
        pos.x,
        pos.y,
        pos.x + size.width as i32,
        pos.y + size.height as i32,
    );

    unsafe {
        let mut cls: Vec<u16> = "Shell_TrayWnd".encode_utf16().collect();
        cls.push(0);
        let Ok(hwnd): Result<HWND, _> = FindWindowW(PCWSTR(cls.as_ptr()), None) else {
            return TaskbarYieldStatus::default();
        };
        if !IsWindowVisible(hwnd).as_bool() {
            return TaskbarYieldStatus::default();
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return TaskbarYieldStatus::default();
        }
        let h = overlap_height(
            (rect.left, rect.top, rect.right, rect.bottom),
            desk,
        );
        TaskbarYieldStatus {
            visible: h > 0,
            height: h,
        }
    }
}

#[cfg(not(windows))]
fn current_status(_app: &AppHandle) -> TaskbarYieldStatus {
    TaskbarYieldStatus::default()
}

/// 手动查询（诊断用；运行期状态由 watcher 推送）。
#[tauri::command]
pub fn taskbar_yield_check(app: AppHandle) -> CmdResult<TaskbarYieldStatus> {
    Ok(current_status(&app))
}

/// 1s 轮询 watcher：状态变化才推送（height 变化也推——让位距离跟随 DPI/位置）。
pub fn spawn_taskbar_yield_watcher(app: AppHandle) {
    std::thread::Builder::new()
        .name("taskbar-yield".into())
        .spawn(move || {
            let mut last = current_status(&app);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let cur = current_status(&app);
                if cur != last {
                    last = cur.clone();
                    let _ = app.emit("sys://taskbar-yield", &cur);
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_height_bottom_taskbar() {
        // 任务栏贴底：桌面 1920x1080，任务栏 (0,1040,1920,1080) → 侵入 40
        assert_eq!(overlap_height((0, 1040, 1920, 1080), (0, 0, 1920, 1080)), 40);
    }

    #[test]
    fn overlap_height_disjoint_is_zero() {
        assert_eq!(overlap_height((0, 1040, 1920, 1080), (0, 0, 1920, 1039)), 0);
        assert_eq!(overlap_height((-1920, 0, -100, 1080), (0, 0, 1920, 1080)), 0);
    }

    #[test]
    fn overlap_height_taskbar_above_desktop_is_zero() {
        // 顶置任务栏在桌面外之上 → 不相交
        assert_eq!(overlap_height((0, -48, 1920, 0), (0, 0, 1920, 1080)), 0);
    }
}
```

注意：`win.outer_position()`/`outer_size()`/`is_focused()` 是 Tauri v2 同步方法；若编译器提示需要 `.await`（版本差异），改为在 `app.run_on_main_thread` 闭包内取值。

- [ ] **Step 2.2: wp_list_images（wallpaper.rs 追加）**

`src-tauri/src/shell/wallpaper.rs` 末尾（tests 模块前）追加：

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpImageFile {
    pub name: String,
    pub path: String,
}

/// 枚举目录下的壁纸图片（平铺一层，不递归；按文件名排序）。
/// 壁纸中心「本地目录」库源；目录不存在报错，空目录返回空表（如实）。
#[tauri::command]
pub fn wp_list_images(dir: String) -> CmdResult<Vec<WpImageFile>> {
    use crate::error::AppError;
    let root = std::path::PathBuf::from(&dir);
    let mut out: Vec<WpImageFile> = std::fs::read_dir(&root)
        .map_err(|e| AppError::io(format!("读取目录失败 / read_dir failed: {e}")))?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let is_img = p
                .extension()
                .map(|x| {
                    let x = x.to_string_lossy().to_lowercase();
                    matches!(
                        x.as_str(),
                        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif"
                    )
                })
                .unwrap_or(false);
            if !p.is_file() || !is_img {
                return None;
            }
            Some(WpImageFile {
                name: e.file_name().to_string_lossy().into_owned(),
                path: p.to_string_lossy().into_owned(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}
```

- [ ] **Step 2.3: 模块与命令注册**

`src-tauri/src/shell/mod.rs` 按字母序插入：`pub mod taskbar_yield;`（放在 `pub mod sysinfo;` 之后）。

`src-tauri/src/lib.rs`：
- L102 `spawn_compat_watcher(...)` 之后插入一行：
  `shell::taskbar_yield::spawn_taskbar_yield_watcher(app.handle().clone());`
- 命令表（L375 `shell::ecosystem::steam_launch,` 附近）追加：
  `shell::taskbar_yield::taskbar_yield_check,`
  `shell::wallpaper::wp_list_images,`

- [ ] **Step 2.4: 测试 + check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml overlap`
Expected: PASS（3 项）

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 0 error

- [ ] **Step 2.5: Commit**

```bash
git add src-tauri/src/shell/taskbar_yield.rs src-tauri/src/shell/wallpaper.rs src-tauri/src/shell/mod.rs src-tauri/src/lib.rs
git commit -m "feat(shell): 任务栏智能让位检测（Shell_TrayWnd 轮询+sys://taskbar-yield）与本地图片枚举命令"
```

---

### Task 3: 前端 CompatBanner 事件名 + i18n 文案更新

**Files:**
- Modify: `src/system/compat/CompatBanner.tsx:32`
- Modify: `src/i18n/dictionaries.ts:264-269`（zh）、`:3063-3068`（zh-TW）、`:3280-3285`（en）

- [ ] **Step 3.1: CompatBanner 事件名**

`CompatBanner.tsx` L32：

```tsx
    const un = listen<CompatStatus>("compat://cef-apps", (e) => {
```

- [ ] **Step 3.2: i18n 三语更新**

zh（L264-265 替换）：

```ts
  compatWallpaperTitle: "CEF 应用兼容提醒（Wallpaper Engine / Steam）",
  compatWallpaperBody: "检测到 Wallpaper Engine 或 Steam 正在运行。为避免 libcef 0x80000003 崩溃与 GPU 卡死，已自动取消“独占置顶”并降低壁纸负载；相关应用退出约 30 秒后自动恢复。",
```

zh-TW（L3063-3064 替换）：

```ts
  compatWallpaperTitle: "CEF 應用相容提醒（Wallpaper Engine / Steam）",
  compatWallpaperBody: "偵測到 Wallpaper Engine 或 Steam 正在運行。為避免 libcef 0x80000003 崩潰與 GPU 卡死，已自動取消「獨佔置頂」並降低桌布負載；相關應用退出約 30 秒後自動恢復。",
```

en（L3280-3281 替换）：

```ts
  compatWallpaperTitle: "CEF app compatibility (Wallpaper Engine / Steam)",
  compatWallpaperBody: "Wallpaper Engine or Steam is running. To avoid libcef 0x80000003 crashes and GPU hangs, always-on-top has been disabled and wallpaper GPU load reduced; auto-restores ~30s after these apps exit.",
```

- [ ] **Step 3.3: 类型检查**

Run: `npx tsc --noEmit`
Expected: 0 error

- [ ] **Step 3.4: Commit**

```bash
git add src/system/compat/CompatBanner.tsx src/i18n/dictionaries.ts
git commit -m "fix(compat): 前端横幅跟随 cef-apps 事件名，文案覆盖 Steam"
```

---

### Task 4: CustomBg 新字段（活化参数）

**Files:**
- Modify: `src/lib/settings.ts:37-57`（CustomBg）、`:321`（customBg 默认值）
- Test: `src/lib/__tests__/settings.living.test.ts`（新建）

- [ ] **Step 4.1: 先写失败测试**

创建 `src/lib/__tests__/settings.living.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS } from "../settings";

describe("CustomBg 活化参数（壁纸中心属性面板落地字段）", () => {
  it("默认值：livingIntensity=0.8 / livingDrift=0.6 / particleStyle=mixed", () => {
    const cb = DEFAULT_SETTINGS.customBg;
    expect(cb.livingIntensity).toBe(0.8);
    expect(cb.livingDrift).toBe(0.6);
    expect(cb.particleStyle).toBe("mixed");
  });

  it("旧 customBg JSON（无新字段）合并后回落默认值，不缺键", () => {
    // parseSettings 的 customBg 合并是 {...默认, ...JSON.parse(raw)}——
    // 这里直接验证合并语义（与 settings.ts L421 同型）
    const legacy = JSON.stringify({ type: "image", color: "#0a0a0a" });
    const merged = { ...DEFAULT_SETTINGS.customBg, ...JSON.parse(legacy) };
    expect(merged.livingIntensity).toBe(0.8);
    expect(merged.color).toBe("#0a0a0a");
  });
});
```

（若 `DEFAULT_SETTINGS` 导出名不同——settings.ts 内默认对象可能叫别的名字，以实际导出为准调整 import；先 `rg -n "DEFAULT" src/lib/settings.ts` 确认。）

- [ ] **Step 4.2: 跑测试确认失败**

Run: `npx vitest run src/lib/__tests__/settings.living.test.ts`
Expected: FAIL（livingIntensity undefined）

- [ ] **Step 4.3: 实现**

`settings.ts` CustomBg 接口（L37-57）`shaderPath: string;` 之后追加：

```ts
  /** 壁纸中心·静态图活化属性：粒子密度 0..1（0=关）。 */
  livingIntensity: number;
  /** 壁纸中心·Ken Burns 漂移幅度 0..1（0≈静止，1=全幅慢漂）。 */
  livingDrift: number;
  /** 壁纸中心·粒子风格。 */
  particleStyle: "dust" | "bokeh" | "mixed";
```

L321 起的 customBg 默认值对象里 `shaderPath: ""` 之后追加：

```ts
    livingIntensity: 0.8,
    livingDrift: 0.6,
    particleStyle: "mixed",
```

- [ ] **Step 4.4: 跑测试通过**

Run: `npx vitest run src/lib/__tests__/settings.living.test.ts`
Expected: PASS（2 项）

- [ ] **Step 4.5: Commit**

```bash
git add src/lib/settings.ts src/lib/__tests__/settings.living.test.ts
git commit -m "feat(settings): CustomBg 增加静态图活化参数（粒子密度/漂移幅度/粒子风格）"
```

---

### Task 5: LivingWallpaper / WallpaperLayer 接入活化参数 + GPU 抑制

**Files:**
- Modify: `src/system/wallpaper/LivingWallpaper.tsx`
- Modify: `src/system/wallpaper/WallpaperLayer.tsx:61-70`

- [ ] **Step 5.1: LivingWallpaper props 扩展**

`LivingWallpaper.tsx` 组件签名（L17-22）改为：

```tsx
export function LivingWallpaper(props: {
  imagePath: string;
  reduceMotion?: boolean;
  safeMode?: boolean;
  perfMode?: string;
  /** 壁纸中心属性：粒子密度 0..1（0=关）；缺省 0.8 */
  livingIntensity?: number;
  /** 壁纸中心属性：Ken Burns 漂移幅度 0..1；缺省 0.6 */
  livingDrift?: number;
  /** 壁纸中心属性：粒子风格；缺省 mixed */
  particleStyle?: "dust" | "bokeh" | "mixed";
  /** GPU 自律：壁纸中心等大窗口打开时暂停动画（静态帧渲染） */
  suppress?: boolean;
}): React.ReactElement {
```

- [ ] **Step 5.2: 漂移幅度接入（drift 计算后、渲染处）**

L49 `return { dx, dy, dur: ... }` 保持；渲染处（L58-67）改为：

```tsx
  const amp = 0.2 + 0.8 * (props.livingDrift ?? 0.6);
  const dur = Math.max(30, Math.round(drift.dur / (0.4 + 0.6 * (props.livingDrift ?? 0.6))));

  return (
    <div className="wallpaper wallpaper-living" aria-hidden>
      <div
        className="living-media"
        style={{
          ["--kb-dx" as string]: String(drift.dx * amp),
          ["--kb-dy" as string]: String(drift.dy * amp),
          ["--kb-dur" as string]: `${dur}s`,
          animationPlayState: props.suppress ? "paused" : undefined,
        }}
      >
        <img src={toAssetUrl(imagePath)} alt="" draggable={false} />
      </div>
      <LivingParticles
        reduceMotion={props.reduceMotion}
        safeMode={props.safeMode}
        perfMode={props.perfMode}
        intensity={props.livingIntensity}
        particleStyle={props.particleStyle}
        suppress={props.suppress}
      />
    </div>
  );
```

- [ ] **Step 5.3: 粒子预算与风格接入**

`particleBudget`（L78-87）改为：

```tsx
/** 粒子分档：high=全量 / balanced=中量 / eco=轻量 / static·auto(static 解析)=关；
 *  intensity（0..1）为壁纸中心密度滑杆的整体缩放。 */
function particleBudget(
  perfMode: string | undefined,
  reduceMotion?: boolean,
  safeMode?: boolean,
  intensity?: number,
): number {
  if (reduceMotion || safeMode) return 0;
  const base = (() => {
    switch (perfMode) {
      case "high": return 110;
      case "balanced": return 72;
      case "eco": return 34;
      case "static": return 0;
      default: return 72; // auto 未定档前按 balanced 起步
    }
  })();
  return Math.round(base * Math.min(1.5, Math.max(0, intensity ?? 0.8)));
}
```

`LivingParticles` props（L99-103）与初始化（L105、L130 附近）改为：

```tsx
function LivingParticles(props: {
  reduceMotion?: boolean;
  safeMode?: boolean;
  perfMode?: string;
  intensity?: number;
  particleStyle?: "dust" | "bokeh" | "mixed";
  suppress?: boolean;
}): React.ReactElement | null {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const budget = props.suppress
    ? 0
    : particleBudget(props.perfMode, props.reduceMotion, props.safeMode, props.intensity);
  const enabled = budget > 0;
```

粒子生成循环（L129-142）中 `const bokeh = i % 9 === 0;` 改为：

```tsx
      const bokeh = props.particleStyle === "bokeh"
        ? true
        : props.particleStyle === "dust"
          ? false
          : i % 9 === 0;
```

useEffect 依赖数组（L226）改为 `[enabled, budget, props.particleStyle]`。

- [ ] **Step 5.4: WallpaperLayer 传参**

`WallpaperLayer.tsx` L21 签名追加 `suppress?: boolean`，L61-70 调用改为：

```tsx
  if (mode === "living") {
    return (
      <LivingWallpaper
        imagePath={s.customBg.imagePath}
        reduceMotion={s.reduceMotion}
        safeMode={s.safeMode}
        perfMode={s.perfMode}
        livingIntensity={s.customBg.livingIntensity}
        livingDrift={s.customBg.livingDrift}
        particleStyle={s.customBg.particleStyle}
        suppress={props.suppress}
      />
    );
  }
```

- [ ] **Step 5.5: 类型检查 + 既有测试回归**

Run: `npx tsc --noEmit`
Expected: 0 error

Run: `npx vitest run`
Expected: 全绿（既有 LivingWallpaper 相关测试若有 budget 断言，按新签名同步——先 `rg -n "particleBudget|LivingWallpaper" src/system/wallpaper/__tests__/` 核对）

- [ ] **Step 5.6: Commit**

```bash
git add src/system/wallpaper/LivingWallpaper.tsx src/system/wallpaper/WallpaperLayer.tsx
git commit -m "feat(living): 活化参数（密度/幅度/风格）接入渲染 + 壁纸中心打开时 GPU 抑制"
```

---

### Task 6: 壁纸中心窗口骨架 + desktop 桥

**Files:**
- Create: `wallpaper-center.html`
- Create: `src/entries/wallpaper-center/main.tsx`
- Create: `src/system/wallpaper/center/center.css`
- Create: `src/system/wallpaper/center/bridge.ts`
- Modify: `vite.config.ts:26-33`
- Modify: `src/entries/runtime.ts:4,21`
- Modify: `src/system/windows/appWindows.ts`（追加 openWallpaperCenter）
- Modify: `src/system/desktop/DesktopShell.tsx`（挂 bridge + suppress 下传）

- [ ] **Step 6.1: vite 入口 + EntryType**

`vite.config.ts` input 增加：`"wallpaper-center": html("wallpaper-center"),`

`src/entries/runtime.ts` L4 类型追加 `| "wallpaper-center"`；L21 三元改为：

```ts
    const label =
      entry === "desktop" || entry === "explorer" || entry === "wallpaper-center"
        ? entry
        : `app-${entry}`;
```

- [ ] **Step 6.2: wallpaper-center.html**

```html
<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Variable 壁纸中心</title>
    <style>
      html, body, #root { height: 100%; margin: 0; }
      body { background: #05070d; color: #dfe7f5; font-family: "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif; overflow: hidden; }
    </style>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/entries/wallpaper-center/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 6.3: 入口 main.tsx**

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setupEntryRuntime } from "../runtime";
import { WallpaperCenter } from "../../system/wallpaper/center/WallpaperCenter";
import { trackSelfGeom } from "../../system/windows/appWindows";
import "../../styles/global.css";
import "../../system/wallpaper/center/center.css";

// 壁纸中心：本地壁纸管理（WE 扫描/本地目录/活化属性/播放列表），零 Steam 依赖。
setupEntryRuntime("wallpaper-center");

// GPU 自律：打开即通知 desktop 抑制桌面壁纸动画；销毁时恢复
void emit("wallpaper://center-open");
void getCurrentWindow()
  .onDestroyed(() => {
    void emit("wallpaper://center-closed");
  })
  .catch(() => {});

void trackSelfGeom("wallpaper-center");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <WallpaperCenter />
  </React.StrictMode>,
);
```

- [ ] **Step 6.4: appWindows.openWallpaperCenter**

`appWindows.ts` `openSystemWindow` 之后追加：

```ts
/**
 * 壁纸中心（本地 Wallpaper UI）：独立窗口，无需 Steam。
 * 复用 M4 多窗口模式：几何记忆 + 已存在聚焦；关闭即销毁（释放 WebView GPU）。
 */
export async function openWallpaperCenter(): Promise<void> {
  const label = "wallpaper-center";
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    void existing.unminimize().catch(() => {});
    void existing.setFocus().catch(() => {});
    return;
  }
  const geom = loadGeom(label);
  new WebviewWindow(label, {
    url: "wallpaper-center.html",
    title: "Variable 壁纸中心",
    ...(geom ? { x: geom.x, y: geom.y, width: geom.width, height: geom.height } : { center: true, width: 1280, height: 800 }),
    minWidth: 960,
    minHeight: 640,
    resizable: true,
    decorations: false,
    // 与置顶桌面同层（topmost 组内按激活序），浮于覆盖桌面之上
    alwaysOnTop: true,
    dragDropEnabled: true,
  }).once("error", (e) => {
    console.error(`[Variable] open ${label} failed`, e);
  });
}
```

- [ ] **Step 6.5: desktop 桥 bridge.ts（应用事件 + GPU 抑制 store + 播放列表 runner）**

创建 `src/system/wallpaper/center/bridge.ts`：

```ts
/**
 * 壁纸中心 desktop 侧桥（无 UI）：
 * - 消费 Tauri 事件 `wallpaper://apply` { patch } → onPatchSettings（settings 唯一真相）；
 * - `wallpaper://center-open/closed` → centerOpenStore（DesktopShell 下传 WallpaperLayer
 *   做 GPU 抑制：壁纸中心是最上层活动窗口，桌面动画无意义）；
 * - PlaylistRunner：跨窗口共享 localStorage 播放列表（storage 事件跨窗口同步），
 *   间隔轮换 → patchSettings 应用。
 */

import { useStore } from "../../../lib/store";

export const centerOpenStore = { open: false };
useStore; // 保持与项目 store 风格一致（若项目有 zustand 则直接 create）

import { create } from "zustand";

interface CenterOpenState {
  open: boolean;
  set: (v: boolean) => void;
}

export const useCenterOpen = create<CenterOpenState>((set) => ({
  open: false,
  set: (v) => set({ open: v }),
}));
```

（注：先 `rg -n "from \"zustand\"" src/state/uiStore.ts` 确认项目 zustand 导入风格；`lib/store.ts` 若已封装 useStore 订阅原语则优先复用。上面保留两段是计划草稿，实施时合并为单一 zustand store，删除 `centerOpenStore` 字面量对象。）

桥主体（同文件续）：

```ts
export interface WpPatch {
  wallpaperMode?: string;
  customBg?: Record<string, unknown>;
}

/** 播放列表条目（localStorage: variable:wpcenter:playlist:v1） */
export interface PlaylistEntry {
  title: string;
  kind: "image" | "video" | "web" | "shader";
  path: string;
}

export const PLAYLIST_KEY = "variable:wpcenter:playlist:v1";
export const PLAYLIST_STATE_KEY = "variable:wpcenter:playlist-state:v1";

export interface PlaylistState {
  enabled: boolean;
  intervalMin: number;
  shuffle: boolean;
  cursor: number;
}

export const DEFAULT_PLAYLIST_STATE: PlaylistState = {
  enabled: false,
  intervalMin: 15,
  shuffle: true,
  cursor: 0,
};

export function loadPlaylist(): PlaylistEntry[] {
  try {
    const raw = JSON.parse(localStorage.getItem(PLAYLIST_KEY) ?? "[]");
    return Array.isArray(raw)
      ? raw.filter(
          (e): e is PlaylistEntry =>
            !!e && typeof e.path === "string" && typeof e.kind === "string",
        )
      : [];
  } catch {
    return [];
  }
}

export function loadPlaylistState(): PlaylistState {
  try {
    return {
      ...DEFAULT_PLAYLIST_STATE,
      ...JSON.parse(localStorage.getItem(PLAYLIST_STATE_KEY) ?? "{}"),
    };
  } catch {
    return DEFAULT_PLAYLIST_STATE;
  }
}

/** 纯函数：下一个索引（shuffle 用时间做种——轮换间隔分钟级，够散列）。 */
export function nextIndex(len: number, cursor: number, shuffle: boolean): number {
  if (len <= 0) return 0;
  if (!shuffle) return (cursor + 1) % len;
  if (len === 1) return 0;
  const seed = (Date.now() / 1000) | 0;
  let i = (seed * 2654435761) % len;
  if (i < 0) i += len;
  return i === cursor ? (i + 1) % len : i;
}

/** kind → 应用 patch（与各模式字段一一对应）。 */
export function entryPatch(e: PlaylistEntry): WpPatch {
  switch (e.kind) {
    case "video":
      return { wallpaperMode: "video", customBg: { videoPath: e.path, playVideo: true } };
    case "web":
      return { wallpaperMode: "web", customBg: { htmlPath: e.path } };
    case "shader":
      return { wallpaperMode: "shader", customBg: { shaderPath: e.path } };
    default:
      return { wallpaperMode: "living", customBg: { imagePath: e.path } };
  }
}

/** 挂在 DesktopShell；onPatch 由 props 传入。返回清理函数。 */
export function installWallpaperBridge(
  onPatch: (patch: Partial<import("../../../lib/settings").Settings>) => void,
): () => void {
  const disposers: Array<() => void> = [];

  void import("@tauri-apps/api/event").then(({ listen }) => {
    disposers.push(() => void listen(() => {}).catch(() => {})); // 占位不用
  });

  // Tauri 事件三件套
  void (async () => {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      const un1 = await listen<WpPatch>("wallpaper://apply", (e) => {
        if (e.payload && typeof e.payload === "object") onPatch(e.payload as never);
      });
      const un2 = await listen("wallpaper://center-open", () => useCenterOpen.getState().set(true));
      const un3 = await listen("wallpaper://center-closed", () => useCenterOpen.getState().set(false));
      disposers.push(() => void un1(), () => void un2(), () => void un3());
    } catch {
      /* 非 Tauri 环境（vitest）静默 */
    }
  })();

  // 播放列表 runner：storage 跨窗口同步 + 间隔轮换
  let timer = 0;
  const run = (): void => {
    window.clearInterval(timer);
    const st = loadPlaylistState();
    const list = loadPlaylist();
    if (!st.enabled || list.length === 0) return;
    timer = window.setInterval(() => {
      const s2 = loadPlaylistState();
      const l2 = loadPlaylist();
      if (!s2.enabled || l2.length === 0) {
        window.clearInterval(timer);
        return;
      }
      const i = nextIndex(l2.length, s2.cursor, s2.shuffle);
      const cur = l2[i];
      localStorage.setItem(
        PLAYLIST_STATE_KEY,
        JSON.stringify({ ...s2, cursor: i }),
      );
      onPatch(entryPatch(cur) as never);
    }, Math.max(1, st.intervalMin) * 60_000);
  };
  const onStorage = (e: StorageEvent): void => {
    if (e.key === PLAYLIST_KEY || e.key === PLAYLIST_STATE_KEY) run();
  };
  window.addEventListener("storage", onStorage);
  run();
  disposers.push(() => {
    window.clearInterval(timer);
    window.removeEventListener("storage", onStorage);
  });

  return () => disposers.forEach((d) => d());
}
```

（实施时删除 "占位不用" 那段死代码与 `centerOpenStore` 字面量——保留最终版干净实现。）

- [ ] **Step 6.6: DesktopShell 接线**

`DesktopShell.tsx`：import 区追加

```tsx
import { useEffect } from "react"; // 若已 import 则合并
import { installWallpaperBridge, useCenterOpen } from "../wallpaper/center/bridge";
```

组件体内（其他 useEffect 旁）：

```tsx
  // 壁纸中心桥：应用事件 → settings；中心开 → GPU 抑制
  useEffect(() => installWallpaperBridge(props.onPatchSettings), [props.onPatchSettings]);
  const wpCenterOpen = useCenterOpen((s) => s.open);
```

`WallpaperLayer` 渲染处（L591 附近）加 `suppress={wpCenterOpen}`。

- [ ] **Step 6.7: 类型检查**

Run: `npx tsc --noEmit`
Expected: 0 error（WallpaperCenter 尚未创建——本任务先建骨架组件空壳）

在 `src/system/wallpaper/center/WallpaperCenter.tsx` 建最小空壳（Task 7 填充）：

```tsx
export function WallpaperCenter(): React.ReactElement {
  return <div className="wp-center-root">Wallpaper Center</div>;
}
```

- [ ] **Step 6.8: Commit**

```bash
git add wallpaper-center.html src/entries/wallpaper-center src/system/wallpaper/center vite.config.ts src/entries/runtime.ts src/system/windows/appWindows.ts src/system/desktop/DesktopShell.tsx
git commit -m "feat(center): 壁纸中心窗口骨架——MPA入口/几何记忆/desktop桥(apply事件+GPU抑制+播放列表runner)"
```

---

### Task 7: 壁纸中心三栏 UI（库/预览属性/播放列表 + 创建嵌入 Workshop）

**Files:**
- Create: `src/system/wallpaper/center/useLibrary.ts`
- Modify: `src/system/wallpaper/center/WallpaperCenter.tsx`（填充实现）
- Modify: `src/system/wallpaper/center/center.css`（样式）
- Modify: `src/system/wallpaper/Workshop.tsx:44-46,362,565-580`（requestApply 双发 + embedded prop + footer 链接）
- Modify: `src/lib/ipc.ts:613` 附近（wpListImages 绑定）
- Modify: `src/i18n/dictionaries.ts`（三语词条）

- [ ] **Step 7.1: ipc 绑定**

`ipc.ts`（`wpEngineScan` 旁 L613-615）追加：

```ts
  wpListImages: (dir: string) => invoke<Shell.WpImageFile[]>("wp_list_images", { dir }),
```

`ipc.ts` Shell 命名空间类型区（WpEngineItem 定义旁）追加：

```ts
  interface WpImageFile {
    name: string;
    path: string;
  }
```

- [ ] **Step 7.2: useLibrary.ts（数据合并，纯函数可测）**

```ts
/**
 * 壁纸中心库源合并（纯数据层）：
 * ① 当前壁纸（settings.customBg）② WE 扫描（wp_engine_scan）
 * ③ 本地目录（wp_list_images）④ 手动添加图片（localStorage）。
 * dedupe 按 path；title 冲突不动（来源可并存）。
 */

import { ipc, type Shell } from "../../../lib/ipc";

export interface LibraryItem {
  id: string;
  title: string;
  kind: "image" | "video" | "shader" | "web" | "current";
  /** 应用入口路径（image→imagePath；video→videoPath；web→htmlPath；shader→shaderPath） */
  path: string;
  preview: string | null;
  source: string;
  supported: boolean;
}

const MANUAL_KEY = "variable:wpcenter:manual:v1";

export function loadManualImages(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(MANUAL_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function saveManualImages(list: string[]): void {
  try {
    localStorage.setItem(MANUAL_KEY, JSON.stringify(list));
  } catch {
    /* full */
  }
}

/** WE 扫描项 → 库项（kind 映射；unsupported 保留展示但不可应用）。 */
export function fromEngineItem(e: Shell.WpEngineItem): LibraryItem {
  const kind = e.kind === "video" ? "video"
    : e.kind === "web" ? "web"
    : e.kind === "scene" ? "shader"
    : "image";
  return {
    id: `we:${e.id}`,
    title: e.title || e.id,
    kind,
    path: e.file ?? e.dir,
    preview: e.preview ?? null,
    source: e.source,
    supported: e.supported && !!e.file,
  };
}

/** 本地图片文件 → 库项。 */
export function fromImageFile(p: string, name?: string): LibraryItem {
  const n = name ?? p.split(/[\\/]/).pop() ?? p;
  return {
    id: `img:${p}`,
    title: n,
    kind: "image",
    path: p,
    preview: p,
    source: "local",
    supported: true,
  };
}

/** 当前壁纸 → 库项（title 固定"当前壁纸"，selected 默认命中）。 */
export function currentItem(customBg: { imagePath: string; videoPath: string; htmlPath: string; shaderPath: string }): LibraryItem | null {
  if (customBg.shaderPath) {
    return { id: "cur", title: "当前壁纸", kind: "shader", path: customBg.shaderPath, preview: customBg.imagePath || null, source: "current", supported: true };
  }
  if (customBg.htmlPath) {
    return { id: "cur", title: "当前壁纸", kind: "web", path: customBg.htmlPath, preview: null, source: "current", supported: true };
  }
  if (customBg.videoPath) {
    return { id: "cur", title: "当前壁纸", kind: "video", path: customBg.videoPath, preview: null, source: "current", supported: true };
  }
  if (customBg.imagePath) {
    return { id: "cur", title: "当前壁纸", kind: "image", path: customBg.imagePath, preview: customBg.imagePath, source: "current", supported: true };
  }
  return null;
}

/** 纯合并 + 去重（path 相同保留先到；顺序：current → manual → we）。 */
export function mergeLibrary(items: LibraryItem[]): LibraryItem[] {
  const seen = new Set<string>();
  const out: LibraryItem[] = [];
  for (const it of items) {
    if (it.id === "cur" || seen.has(it.path)) continue;
    seen.add(it.path);
    out.push(it);
  }
  return out;
}

/** React hook：装配全部来源。 */
export function useLibrary(customBg: Parameters<typeof currentItem>[0]): {
  items: LibraryItem[];
  scanWe: () => Promise<void>;
  scanDir: (dir: string) => Promise<void>;
  addManual: (paths: string[]) => void;
  weError: string | null;
} {
  // 实现放 WallpaperCenter 内联亦可；保持 hook 返回三源合并：
  // weItems: Shell.WpEngineItem[]（wpEngineScan("") 自动探测）
  // dirItems: wpListImages(dir)（localStorage variable:wpcenter:dirs:v1 记目录）
  // manual: loadManualImages()
  // 组装：[currentItem(customBg), ...manual.map(fromImageFile), ...dir, ...we.map(fromEngineItem)] → mergeLibrary
  // 详见 WallpaperCenter.tsx 使用处（实现代码在 Task 7.3 主组件内，本文件只承载纯函数）。
  throw new Error("implemented inline in WallpaperCenter (pure fns live here)");
}
```

（实施注意：`useLibrary` 若不作为真 hook 使用就删掉这个导出，只保留纯函数 + `MANUAL_KEY` 存取——主组件内联装配。计划保留纯函数集，`useLibrary` 删除，避免死代码。）

- [ ] **Step 7.3: Workshop embedded + requestApply 双发**

`Workshop.tsx`：
1. `requestApply`（L44-46）改为（Tauri 可用时双发——桌面 overlay 与中心窗口都由 desktop 桥统一消费 Tauri 事件）：

```tsx
function requestApply(patch: { wallpaperMode: string; customBg?: Record<string, unknown> }): void {
  window.dispatchEvent(new CustomEvent("ai04:wallpaper-apply", { detail: { patch } }));
  void import("@tauri-apps/api/event")
    .then(({ emit }) => emit("wallpaper://apply", { patch }))
    .catch(() => {
      /* 纯浏览器环境（vitest）静默 */
    });
}
```

2. `export function Workshop(props: { embedded?: boolean })`（L362）；header 关闭按钮与 Escape（L567-569、L579）改为：

```tsx
      onKeyDown={(e) => {
        if (e.key === "Escape" && !props.embedded) dispatchClose("wallpaper-studio");
      }}
```

```tsx
        {!props.embedded && (
          <button className="wp-studio-close" onClick={() => dispatchClose("wallpaper-studio")}>✕</button>
        )}
```

3. footer（pack 标签页底部或 root 末尾）非 embedded 时加链接：

```tsx
        {!props.embedded && (
          <button className="wp-studio-open-center" onClick={() => void import("../windows/appWindows").then(({ openWallpaperCenter }) => openWallpaperCenter())}>
            {t("wpOpenCenter")}
          </button>
        )}
```

（放置位置：`wp-studio-header` 内 close 按钮之前最稳妥——放 header 右侧。）

- [ ] **Step 7.4: WallpaperCenter 主组件（完整实现）**

替换 Task 6 空壳（`src/system/wallpaper/center/WallpaperCenter.tsx`）：

```tsx
import { useEffect, useMemo, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../../i18n";
import { errMessage, ipc } from "../../../lib/ipc";
import { loadSettings, type CustomBg, type Settings } from "../../../lib/settings";
import { pushToast } from "../../../state/uiStore";
import { toAssetUrl } from "../../../features/background/CosmicBackground";
import { Workshop } from "../Workshop";
import {
  currentItem, fromEngineItem, fromImageFile, loadManualImages, mergeLibrary, saveManualImages,
  type LibraryItem,
} from "./useLibrary";
import {
  DEFAULT_PLAYLIST_STATE, PLAYLIST_KEY, PLAYLIST_STATE_KEY, entryPatch,
  loadPlaylist, loadPlaylistState, nextIndex, type PlaylistEntry, type PlaylistState,
} from "./bridge";

const DIRS_KEY = "variable:wpcenter:dirs:v1";

function loadDirs(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(DIRS_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function WallpaperCenter(): React.ReactElement {
  const { t } = useI18n();
  const [tab, setTab] = useState<"installed" | "create">("installed");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [weItems, setWeItems] = useState<Parameters<typeof fromEngineItem>[0][]>([]);
  const [dirItems, setDirItems] = useState<LibraryItem[]>([]);
  const [selected, setSelected] = useState<LibraryItem | null>(null);
  const [busy, setBusy] = useState(false);
  const [weError, setWeError] = useState<string | null>(null);
  // 属性面板草稿（应用时合并进 customBg）
  const [draft, setDraft] = useState<Partial<CustomBg>>({});
  const videoPrevRef = useRef<HTMLVideoElement | null>(null);

  // 载入 settings + WE 自动扫描
  useEffect(() => {
    let alive = true;
    void loadSettings().then((s) => {
      if (alive) {
        setSettings(s);
        const cur = currentItem(s.customBg);
        if (cur) setSelected(cur);
        setDraft({
          livingIntensity: s.customBg.livingIntensity,
          livingDrift: s.customBg.livingDrift,
          particleStyle: s.customBg.particleStyle,
        });
      }
    });
    void ipc.wpEngineScan("")
      .then((list) => { if (alive) setWeItems(list); })
      .catch((e) => { if (alive) setWeError(errMessage(e).message); });
    return () => { alive = false; };
  }, []);

  // 本地目录扫描
  useEffect(() => {
    let alive = true;
    void (async () => {
      const acc: LibraryItem[] = [];
      for (const d of loadDirs()) {
        try {
          const files = await ipc.wpListImages(d);
          acc.push(...files.map((f) => fromImageFile(f.path, f.name)));
        } catch { /* 目录失效：如实跳过 */ }
      }
      if (alive) setDirItems(acc);
    })();
    return () => { alive = false; };
  }, []);

  const items = useMemo(() => {
    const manual = loadManualImages().map((p) => fromImageFile(p));
    const we = weItems.map(fromEngineItem);
    const cur = settings ? currentItem(settings.customBg) : null;
    return cur ? mergeLibrary([cur, ...manual, ...dirItems, ...we]) : mergeLibrary([...manual, ...dirItems, ...we]);
  }, [settings, weItems, dirItems]);

  // ---- 应用：Tauri 事件 → desktop 桥 → onPatchSettings ----
  async function apply(item: LibraryItem): Promise<void> {
    const base: Record<string, unknown> = { ...settings?.customBg, ...draft };
    let patch: { wallpaperMode: string; customBg: Record<string, unknown> };
    switch (item.kind) {
      case "video":
        patch = { wallpaperMode: "video", customBg: { ...base, videoPath: item.path, playVideo: true } };
        break;
      case "web":
        patch = { wallpaperMode: "web", customBg: { ...base, htmlPath: item.path } };
        break;
      case "shader":
        patch = { wallpaperMode: "shader", customBg: { ...base, shaderPath: item.path, imagePath: item.preview ?? settings?.customBg.imagePath ?? "" } };
        break;
      default:
        patch = { wallpaperMode: "living", customBg: { ...base, imagePath: item.path } };
    }
    await emit("wallpaper://apply", { patch });
    pushToast("success", t("wpCenterTitle"), t("wpCenterApplied"));
  }

  async function scanWePick(): Promise<void> {
    setBusy(true);
    try {
      const dir = await open({ directory: true, multiple: false });
      if (typeof dir === "string") setWeItems(await ipc.wpEngineScan(dir));
    } catch (e) {
      setWeError(errMessage(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function addDir(): Promise<void> {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    const dirs = [...new Set([...loadDirs(), dir])];
    localStorage.setItem(DIRS_KEY, JSON.stringify(dirs));
    try {
      const files = await ipc.wpListImages(dir);
      setDirItems((prev) => {
        const known = new Set(prev.map((p) => p.path));
        return [...prev, ...files.map((f) => fromImageFile(f.path, f.name)).filter((x) => !known.has(x.path))];
      });
    } catch (e) {
      pushToast("error", t("wpCenterTitle"), errMessage(e).message);
    }
  }

  async function addManual(): Promise<void> {
    const sel = await open({
      multiple: true,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] }],
    });
    const list = Array.isArray(sel) ? sel : sel ? [sel] : [];
    if (list.length === 0) return;
    saveManualImages([...new Set([...loadManualImages(), ...list])]);
    setDirItems((prev) => {
      const known = new Set(prev.map((p) => p.path));
      return [...prev, ...list.map((p) => fromImageFile(p)).filter((x) => !known.has(x.path))];
    });
  }

  const sel = selected;
  const isImageKind = sel?.kind === "image" || sel?.kind === "current";

  return (
    <div className="wp-center-root">
      <header className="wp-center-header" data-tauri-drag-region>
        <h1 data-tauri-drag-region>{t("wpCenterTitle")}</h1>
        <nav className="wp-center-tabs">
          <button className={tab === "installed" ? "on" : ""} onClick={() => setTab("installed")}>{t("wpCenterInstalled")}</button>
          <button className={tab === "create" ? "on" : ""} onClick={() => setTab("create")}>{t("wpCenterCreate")}</button>
        </nav>
        <div className="wp-center-actions">
          <button disabled={busy} onClick={() => void scanWePick()}>{t("wpEngineScan")}</button>
          <button onClick={() => void addDir()}>{t("wpCenterPickDir")}</button>
          <button onClick={() => void addManual()}>{t("wpCenterAddImage")}</button>
          <button
            aria-label="close"
            className="wp-center-close"
            onClick={() => void getCurrentWindow().destroy()}
          >✕</button>
        </div>
      </header>

      {tab === "create" ? (
        <div className="wp-center-create">
          <Workshop embedded />
        </div>
      ) : (
        <div className="wp-center-main">
          {/* 左：库网格 */}
          <div className="wp-center-lib" role="listbox" aria-label={t("wpCenterInstalled")}>
            {weError && <p className="wp-center-error" role="alert">{weError}</p>}
            {items.length === 0 && !weError && <p className="wp-center-empty">{t("wpCenterEmpty")}</p>}
            {items.map((it) => (
              <button
                key={it.id}
                role="option"
                aria-selected={sel?.id === it.id}
                className={`wp-center-card ${sel?.id === it.id ? "on" : ""} ${it.supported ? "" : "off"}`}
                onClick={() => setSelected(it)}
                onDoubleClick={() => it.supported && void apply(it)}
              >
                <span className="wp-center-thumb">
                  {it.preview ? <img src={toAssetUrl(it.preview)} alt="" loading="lazy" /> : <span className="wp-center-thumb-none">—</span>}
                </span>
                <span className="wp-center-card-title" title={it.title}>{it.title}</span>
                <span className="wp-center-kind">{t(`wpCenterKind${it.kind.charAt(0).toUpperCase()}${it.kind.slice(1)}` as never)}</span>
              </button>
            ))}
          </div>

          {/* 右：预览 + 属性 */}
          <aside className="wp-center-side">
            <div className="wp-center-preview">
              {sel?.preview ? (
                <img src={toAssetUrl(sel.preview)} alt="" />
              ) : sel?.kind === "video" ? (
                <video ref={videoPrevRef} src={toAssetUrl(sel.path)} autoPlay loop muted playsInline />
              ) : (
                <span className="wp-center-empty">{t("wpCenterNoPreview")}</span>
              )}
            </div>
            {sel && (
              <div className="wp-center-props">
                <h3>{sel.title}</h3>
                {isImageKind && (
                  <>
                    <label className="wp-center-slider">
                      <span>{t("wpPropLivingIntensity")}</span>
                      <input
                        type="range" min={0} max={1.5} step={0.05}
                        value={draft.livingIntensity ?? 0.8}
                        onChange={(e) => setDraft((d) => ({ ...d, livingIntensity: Number(e.target.value) }))}
                      />
                      <code>{(draft.livingIntensity ?? 0.8).toFixed(2)}</code>
                    </label>
                    <label className="wp-center-slider">
                      <span>{t("wpPropLivingDrift")}</span>
                      <input
                        type="range" min={0} max={1} step={0.05}
                        value={draft.livingDrift ?? 0.6}
                        onChange={(e) => setDraft((d) => ({ ...d, livingDrift: Number(e.target.value) }))}
                      />
                      <code>{(draft.livingDrift ?? 0.6).toFixed(2)}</code>
                    </label>
                    <label className="wp-center-select">
                      <span>{t("wpPropParticleStyle")}</span>
                      <select
                        value={draft.particleStyle ?? "mixed"}
                        onChange={(e) => setDraft((d) => ({ ...d, particleStyle: e.target.value as CustomBg["particleStyle"] }))}
                      >
                        <option value="dust">{t("wpStyleDust")}</option>
                        <option value="bokeh">{t("wpStyleBokeh")}</option>
                        <option value="mixed">{t("wpStyleMixed")}</option>
                      </select>
                    </label>
                  </>
                )}
                {sel.kind === "video" && (
                  <p className="wp-center-note">{t("wpCenterVideoNote")}</p>
                )}
                <button
                  className="wp-center-apply"
                  disabled={!sel.supported}
                  onClick={() => void apply(sel)}
                >
                  {t("wpCenterApply")}
                </button>
                <button
                  className="wp-center-playlist-add"
                  disabled={!sel.supported || sel.kind === "current"}
                  onClick={() => {
                    const list = loadPlaylist();
                    const entry: PlaylistEntry = {
                      title: sel.title,
                      kind: sel.kind === "current" ? "image" : (sel.kind as PlaylistEntry["kind"]),
                      path: sel.path,
                    };
                    if (!list.some((x) => x.path === entry.path)) {
                      localStorage.setItem(PLAYLIST_KEY, JSON.stringify([...list, entry]));
                      pushToast("success", t("wpCenterPlaylist"), t("wpCenterPlaylistAdded"));
                    }
                  }}
                >
                  + {t("wpCenterPlaylist")}
                </button>
              </div>
            )}
          </aside>
        </div>
      )}

      {/* 底：播放列表（两 tab 共用） */}
      <PlaylistBar />
    </div>
  );
}

/** 底栏播放列表：成员/间隔/顺序/开关/立即换一张。 */
function PlaylistBar(): React.ReactElement {
  const { t } = useI18n();
  const [entries, setEntries] = useState<PlaylistEntry[]>(() => loadPlaylist());
  const [st, setSt] = useState<PlaylistState>(() => loadPlaylistState());

  // storage 事件跨窗口同步（desktop runner 写 cursor）
  useEffect(() => {
    const onS = (e: StorageEvent): void => {
      if (e.key === PLAYLIST_KEY) setEntries(loadPlaylist());
      if (e.key === PLAYLIST_STATE_KEY) setSt(loadPlaylistState());
    };
    window.addEventListener("storage", onS);
    return () => window.removeEventListener("storage", onS);
  }, []);

  const save = (list: PlaylistEntry[]): void => {
    setEntries(list);
    localStorage.setItem(PLAYLIST_KEY, JSON.stringify(list));
  };
  const saveState = (s: PlaylistState): void => {
    setSt(s);
    localStorage.setItem(PLAYLIST_STATE_KEY, JSON.stringify(s));
  };

  const next = async (): Promise<void> => {
    if (entries.length === 0) return;
    const i = nextIndex(entries.length, st.cursor, st.shuffle);
    saveState({ ...st, cursor: i });
    await emit("wallpaper://apply", { patch: entryPatch(entries[i]) });
  };

  return (
    <footer className="wp-center-playlist">
      <label className="wp-center-toggle">
        <input
          type="checkbox"
          checked={st.enabled}
          onChange={(e) => saveState({ ...st, enabled: e.target.checked })}
        />
        <span>{t("wpCenterPlaylist")}</span>
      </label>
      <span className="wp-center-pl-count">{entries.length}</span>
      <label className="wp-center-num">
        <span>{t("wpCenterInterval")}</span>
        <input
          type="number" min={1} max={720}
          value={st.intervalMin}
          onChange={(e) => saveState({ ...st, intervalMin: Math.max(1, Math.round(Number(e.target.value) || 1)) })}
        />
      </label>
      <label className="wp-center-toggle">
        <input type="checkbox" checked={st.shuffle} onChange={(e) => saveState({ ...st, shuffle: e.target.checked })} />
        <span>{t("wpCenterRandom")}</span>
      </label>
      <button onClick={() => void next()} disabled={entries.length === 0}>{t("wpCenterNext")}</button>
      <div className="wp-center-pl-items">
        {entries.map((e, i) => (
          <button
            key={`${e.path}:${i}`}
            className={i === st.cursor ? "on" : ""}
            title={e.title}
            onClick={() => saveState({ ...st, cursor: i })}
          >
            {e.title}
          </button>
        ))}
        {entries.length > 0 && (
          <button
            className="wp-center-pl-clear"
            title={t("wpCenterPlaylistClear")}
            onClick={() => save(entries.filter((_, i) => i !== st.cursor))}
          >✕</button>
        )}
      </div>
    </footer>
  );
}
```

（`DEFAULT_PLAYLIST_STATE` 若未使用则从 import 列表去掉，避免 TS 未用告警。）

- [ ] **Step 7.5: center.css（核心样式）**

```css
/* 壁纸中心（独立窗口）：三栏 WE 风格布局 */
.wp-center-root { display: flex; flex-direction: column; height: 100%; background: #0a0e18; }
.wp-center-header { display: flex; align-items: center; gap: 16px; padding: 8px 12px; background: #0d1220; border-bottom: 1px solid #1c2540; user-select: none; }
.wp-center-header h1 { font-size: 14px; margin: 0; letter-spacing: .08em; color: #cfe0ff; }
.wp-center-tabs { display: flex; gap: 4px; }
.wp-center-tabs button { padding: 6px 14px; border: none; border-radius: 6px; background: transparent; color: #8fa3c8; cursor: pointer; font-size: 13px; }
.wp-center-tabs button.on { background: #1b2a4d; color: #fff; }
.wp-center-actions { margin-left: auto; display: flex; gap: 6px; }
.wp-center-actions button { padding: 5px 10px; border-radius: 6px; border: 1px solid #27355c; background: #131a2e; color: #cfe0ff; cursor: pointer; font-size: 12px; }
.wp-center-actions button:disabled { opacity: .45; cursor: default; }
.wp-center-close:hover { background: #5c1f1f; border-color: #a33; }
.wp-center-main { flex: 1; display: grid; grid-template-columns: minmax(360px, 1fr) 320px; min-height: 0; }
.wp-center-lib { overflow-y: auto; display: grid; grid-template-columns: repeat(auto-fill, minmax(168px, 1fr)); gap: 10px; padding: 12px; align-content: start; }
.wp-center-card { display: flex; flex-direction: column; gap: 4px; padding: 6px; border-radius: 8px; border: 1px solid #1c2540; background: #0d1220; cursor: pointer; text-align: left; }
.wp-center-card.on { border-color: #3d6bd6; box-shadow: 0 0 0 1px #3d6bd6; }
.wp-center-card.off { opacity: .55; }
.wp-center-thumb { aspect-ratio: 16/9; border-radius: 6px; overflow: hidden; background: #070a12; display: flex; align-items: center; justify-content: center; }
.wp-center-thumb img, .wp-center-preview img, .wp-center-preview video { width: 100%; height: 100%; object-fit: cover; }
.wp-center-thumb-none { color: #3a4a6a; }
.wp-center-card-title { font-size: 12px; color: #dfe7f5; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.wp-center-kind { font-size: 10px; color: #7f97c4; text-transform: uppercase; letter-spacing: .08em; }
.wp-center-side { border-left: 1px solid #1c2540; display: flex; flex-direction: column; min-height: 0; }
.wp-center-preview { flex: 1; min-height: 0; background: #070a12; display: flex; align-items: center; justify-content: center; }
.wp-center-preview > img, .wp-center-preview > video { max-width: 100%; max-height: 100%; }
.wp-center-props { padding: 12px; display: flex; flex-direction: column; gap: 10px; overflow-y: auto; }
.wp-center-props h3 { margin: 0; font-size: 13px; color: #cfe0ff; }
.wp-center-slider { display: grid; grid-template-columns: 1fr 110px 38px; align-items: center; gap: 6px; font-size: 12px; color: #8fa3c8; }
.wp-center-slider input[type="range"] { width: 100%; }
.wp-center-select { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 6px; font-size: 12px; color: #8fa3c8; }
.wp-center-select select { background: #131a2e; color: #dfe7f5; border: 1px solid #27355c; border-radius: 6px; padding: 4px; }
.wp-center-apply { margin-top: 4px; padding: 8px; border-radius: 8px; border: none; background: #2f5fd0; color: #fff; font-weight: 600; cursor: pointer; }
.wp-center-apply:disabled { background: #27355c; cursor: default; }
.wp-center-playlist-add { padding: 6px; border-radius: 8px; border: 1px solid #27355c; background: transparent; color: #8fa3c8; cursor: pointer; }
.wp-center-playlist { display: flex; align-items: center; gap: 12px; padding: 6px 12px; background: #0d1220; border-top: 1px solid #1c2540; font-size: 12px; color: #8fa3c8; }
.wp-center-toggle { display: flex; align-items: center; gap: 4px; }
.wp-center-num input { width: 56px; background: #131a2e; color: #dfe7f5; border: 1px solid #27355c; border-radius: 6px; padding: 3px 6px; }
.wp-center-pl-items { display: flex; gap: 4px; overflow-x: auto; margin-left: auto; }
.wp-center-pl-items button { padding: 3px 8px; border-radius: 12px; border: 1px solid #27355c; background: transparent; color: #8fa3c8; cursor: pointer; white-space: nowrap; }
.wp-center-pl-items button.on { border-color: #3d6bd6; color: #fff; }
.wp-center-pl-clear { color: #b4281e !important; }
.wp-center-empty, .wp-center-error { grid-column: 1 / -1; color: #7f97c4; font-size: 12px; padding: 12px; }
.wp-center-error { color: #e07a6a; }
.wp-center-note { font-size: 11px; color: #7f97c4; margin: 0; }
.wp-center-create { flex: 1; overflow-y: auto; }
```

- [ ] **Step 7.6: i18n 词条（三语，插在各语区 compatHelp 行后）**

zh：

```ts
  wpCenterTitle: "壁纸中心",
  wpCenterInstalled: "已安装",
  wpCenterCreate: "创建",
  wpCenterApply: "应用壁纸",
  wpCenterApplied: "已应用到桌面",
  wpCenterPlaylist: "播放列表",
  wpCenterPlaylistAdded: "已加入播放列表",
  wpCenterPlaylistClear: "移除当前项",
  wpCenterInterval: "间隔（分）",
  wpCenterRandom: "随机",
  wpCenterNext: "立即换一张",
  wpCenterEmpty: "没有可用壁纸——扫描 Wallpaper Engine、添加本地目录或图片",
  wpCenterPickDir: "添加本地目录",
  wpCenterAddImage: "添加图片",
  wpCenterNoPreview: "选择左侧壁纸查看预览",
  wpCenterVideoNote: "视频壁纸：应用后在设置→外观可控制播放与通用调节（模糊/亮度等）。",
  wpCenterKindImage: "图片",
  wpCenterKindVideo: "视频",
  wpCenterKindShader: "着色器",
  wpCenterKindWeb: "网页",
  wpCenterKindCurrent: "当前",
  wpPropLivingIntensity: "活化粒子密度",
  wpPropLivingDrift: "漂移幅度",
  wpPropParticleStyle: "粒子风格",
  wpStyleDust: "尘埃",
  wpStyleBokeh: "光斑",
  wpStyleMixed: "混合",
  wpOpenCenter: "打开壁纸中心",
```

zh-TW：

```ts
  wpCenterTitle: "桌布中心",
  wpCenterInstalled: "已安裝",
  wpCenterCreate: "建立",
  wpCenterApply: "套用桌布",
  wpCenterApplied: "已套用到桌面",
  wpCenterPlaylist: "播放清單",
  wpCenterPlaylistAdded: "已加入播放清單",
  wpCenterPlaylistClear: "移除目前項",
  wpCenterInterval: "間隔（分）",
  wpCenterRandom: "隨機",
  wpCenterNext: "立即換一張",
  wpCenterEmpty: "沒有可用桌布——掃描 Wallpaper Engine、新增本機目錄或圖片",
  wpCenterPickDir: "新增本機目錄",
  wpCenterAddImage: "新增圖片",
  wpCenterNoPreview: "選擇左側桌布查看預覽",
  wpCenterVideoNote: "影片桌布：套用後可在設定→外觀控制播放與通用調節（模糊/亮度等）。",
  wpCenterKindImage: "圖片",
  wpCenterKindVideo: "影片",
  wpCenterKindShader: "著作器",
  wpCenterKindWeb: "網頁",
  wpCenterKindCurrent: "目前",
  wpPropLivingIntensity: "活化粒子密度",
  wpPropLivingDrift: "漂移幅度",
  wpPropParticleStyle: "粒子風格",
  wpStyleDust: "塵埃",
  wpStyleBokeh: "光斑",
  wpStyleMixed: "混合",
  wpOpenCenter: "開啟桌布中心",
```

en：

```ts
  wpCenterTitle: "Wallpaper Center",
  wpCenterInstalled: "Installed",
  wpCenterCreate: "Create",
  wpCenterApply: "Apply wallpaper",
  wpCenterApplied: "Applied to desktop",
  wpCenterPlaylist: "Playlist",
  wpCenterPlaylistAdded: "Added to playlist",
  wpCenterPlaylistClear: "Remove current",
  wpCenterInterval: "Interval (min)",
  wpCenterRandom: "Shuffle",
  wpCenterNext: "Next now",
  wpCenterEmpty: "No wallpapers — scan Wallpaper Engine, add a local folder or images",
  wpCenterPickDir: "Add local folder",
  wpCenterAddImage: "Add images",
  wpCenterNoPreview: "Select a wallpaper to preview",
  wpCenterVideoNote: "Video wallpaper: playback and generic adjustments (blur/brightness) live in Settings → Appearance after applying.",
  wpCenterKindImage: "Image",
  wpCenterKindVideo: "Video",
  wpCenterKindShader: "Shader",
  wpCenterKindWeb: "Web",
  wpCenterKindCurrent: "Current",
  wpPropLivingIntensity: "Living particle density",
  wpPropLivingDrift: "Drift amplitude",
  wpPropParticleStyle: "Particle style",
  wpStyleDust: "Dust",
  wpStyleBokeh: "Bokeh",
  wpStyleMixed: "Mixed",
  wpOpenCenter: "Open Wallpaper Center",
```

- [ ] **Step 7.7: 纯函数测试**

创建 `src/system/wallpaper/center/__tests__/center.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { fromEngineItem, fromImageFile, currentItem, mergeLibrary } from "../useLibrary";
import { entryPatch, nextIndex } from "../bridge";
import type { Shell } from "../../../../lib/ipc";

describe("useLibrary 纯函数", () => {
  it("fromEngineItem：kind 映射 video/web/scene→shader/其余 image", () => {
    const mk = (kind: string): Shell.WpEngineItem => ({
      id: "1", title: "T", kind, file: "x", dir: "d", preview: null, supported: true, source: "workshop",
    });
    expect(fromEngineItem(mk("video")).kind).toBe("video");
    expect(fromEngineItem(mk("web")).kind).toBe("web");
    expect(fromEngineItem(mk("scene")).kind).toBe("shader");
    expect(fromEngineItem(mk("image")).kind).toBe("image");
    expect(fromEngineItem(mk("application")).kind).toBe("image");
  });

  it("mergeLibrary：按 path 去重，current 恒保留在前", () => {
    const cur = currentItem({ imagePath: "a.jpg", videoPath: "", htmlPath: "", shaderPath: "" })!;
    const dup = fromImageFile("a.jpg");
    const other = fromImageFile("b.jpg");
    const out = mergeLibrary([cur, dup, other]);
    expect(out).toHaveLength(2);
    expect(out[0].id).toBe("cur");
  });

  it("currentItem：shaderPath 优先级最高；全空 → null", () => {
    const s = currentItem({ imagePath: "i", videoPath: "v", htmlPath: "h", shaderPath: "s" })!;
    expect(s.kind).toBe("shader");
    expect(currentItem({ imagePath: "", videoPath: "", htmlPath: "", shaderPath: "" })).toBeNull();
  });
});

describe("播放列表纯函数", () => {
  it("nextIndex：顺序推进 + 环绕；shuffle 不重复当前", () => {
    expect(nextIndex(3, 0, false)).toBe(1);
    expect(nextIndex(3, 2, false)).toBe(0);
    expect(nextIndex(1, 0, true)).toBe(0);
    const i = nextIndex(3, 1, true);
    expect(i).toBeGreaterThanOrEqual(0);
    expect(i).toBeLessThan(3);
    expect(i).not.toBe(1);
  });

  it("entryPatch：kind → 模式与字段", () => {
    expect(entryPatch({ title: "x", kind: "image", path: "a.jpg" })).toEqual({
      wallpaperMode: "living",
      customBg: { imagePath: "a.jpg" },
    });
    expect(entryPatch({ title: "x", kind: "web", path: "i.html" })).toEqual({
      wallpaperMode: "web",
      customBg: { htmlPath: "i.html" },
    });
  });
});
```

- [ ] **Step 7.8: 跑测试 + 类型检查**

Run: `npx vitest run src/system/wallpaper/center/__tests__/center.test.ts`
Expected: PASS

Run: `npx tsc --noEmit`
Expected: 0 error

- [ ] **Step 7.9: Commit**

```bash
git add src/system/wallpaper/center src/system/wallpaper/Workshop.tsx src/lib/ipc.ts src/i18n/dictionaries.ts
git commit -m "feat(center): 壁纸中心三栏UI——库合并/预览属性(活化滑杆)/播放列表/Workshop嵌入"
```

---

### Task 8: Taskbar 智能让位 + 三个入口 + Workshop 可达性

**Files:**
- Modify: `src/system/taskbar/Taskbar.tsx`（yield 监听 + 根元素样式 + 菜单项）
- Modify: `src/system/desktop/taskbarMenu.ts:19-24`（注册表新项）
- Modify: `src/system/desktop-icons/DesktopIcons.tsx:1570-1583`（右键菜单两项）
- Test: `src/system/desktop/__tests__/taskbarMenu.center.test.ts`（新建，若已有 taskbarMenu 测试文件则追加）

- [ ] **Step 8.1: taskbarMenu 注册表（测试先行）**

新建/追加测试 `src/system/desktop/__tests__/taskbarMenu.center.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { TASKBAR_MENU_REGISTRY, defaultOverride, effectiveMenuIds } from "../taskbarMenu";

describe("壁纸中心菜单项（M-15 注册表）", () => {
  it("注册表含 wallpaperCenter 且默认可见", () => {
    const e = TASKBAR_MENU_REGISTRY.find((r) => r.id === "wallpaperCenter");
    expect(e).toBeDefined();
    expect(e?.defaultVisible).toBe(true);
  });
  it("默认覆盖包含 wallpaperCenter（launcher 之后）", () => {
    const ids = defaultOverride().order;
    expect(ids).toContain("wallpaperCenter");
    expect(ids.indexOf("wallpaperCenter")).toBeGreaterThan(ids.indexOf("launcher"));
  });
  it("旧用户覆盖（无新 id）→ effectiveMenuIds 补齐渲染", () => {
    const legacy = { order: ["showDesktop", "launcher", "taskbarSettings"], hidden: ["sticky"] };
    // sanitize 语义：漏掉的注册表项补入 hidden —— 但用户手动 hidden 不渲染
    // wallpaperCenter 是新默认可见项：旧覆盖不含它 → 应落入 hidden（用户未表态）？
    // 决策：新默认可见项对旧用户直接可见（默认体验优先）——见实现 sanitize 补齐到 order。
    const ids = effectiveMenuIds(legacy);
    expect(ids).toContain("wallpaperCenter");
  });
});
```

**重要决策**：`sanitize()` 把未出现的新注册表项补进 `hidden`（现状 L48-51），这会让老用户永远看不到新菜单项。实现改为**补进 order 尾部**（默认可见）：

`taskbarMenu.ts` L48-51 改为：

```ts
  for (const e of TASKBAR_MENU_REGISTRY) {
    if (!uniqOrder.includes(e.id) && !uniqHidden.includes(e.id)) {
      // 新版本新增的注册表项：默认可见项进 order 尾部（老用户也能看到新功能）
      if (e.defaultVisible) uniqOrder.push(e.id);
      else uniqHidden.push(e.id);
    }
  }
```

注册表（L19-24）在 `launcher` 之后插入：

```ts
  { id: "wallpaperCenter", labelKey: "wpCenterTitle", defaultVisible: true },
```

- [ ] **Step 8.2: 跑测试**

Run: `npx vitest run src/system/desktop/__tests__/taskbarMenu.center.test.ts`
Expected: PASS（先实现 L48-51 改动 + 注册表项后再跑）

- [ ] **Step 8.3: Taskbar.tsx 三处改动**

① import 区追加：

```tsx
import { openWallpaperCenter } from "../windows/appWindows";
```

（`listen` 已由文件头 `@tauri-apps/api/event` 导入；若没有则补 `import { listen } from "@tauri-apps/api/event";`——先 rg 确认。）

② 组件体内（其他 useEffect 旁，L98 附近）追加 yield 监听：

```tsx
  // 任务栏智能让位：Windows 任务栏浮上桌面（Steam 等外部应用获前台）→ 上移让位
  const [yieldInfo, setYieldInfo] = useState<{ visible: boolean; height: number }>({ visible: false, height: 0 });
  useEffect(() => {
    let un: (() => void) | null = null;
    let disposed = false;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<{ visible: boolean; height: number }>("sys://taskbar-yield", (e) => {
          if (!disposed) setYieldInfo(e.payload);
        }),
      )
      .then((fn) => {
        if (disposed) fn();
        else un = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, []);
```

③ 根元素（L539-552）追加 style：

```tsx
    <div
      className="taskbar"
      data-testid="taskbar"
      data-pos={props.pos}
      data-ind={props.settings.runIndicator}
      data-media-breath={props.settings.mediaBreath ? "true" : "false"}
      data-yield={yieldInfo.visible ? "true" : "false"}
      style={{
        transform: yieldInfo.visible ? `translateY(-${Math.max(24, yieldInfo.height + 6)}px)` : undefined,
        transition: props.settings.reduceMotion ? "none" : "transform .3s ease",
      }}
      onContextMenu={/* 保持原样 */}
    >
```

④ blankMenuActions（L315-323）追加：

```tsx
      wallpaperCenter: () => void openWallpaperCenter(),
```

label 映射（L327-330 三元链）改为查表（避免嵌套地狱）：

```tsx
    const labelKeys: Record<string, string> = {
      showDesktop: "showDesktop",
      launcher: "launcherTitle",
      sticky: "tbQuickSticky",
      taskbarSettings: "taskbarSettings",
      wallpaperCenter: "wpCenterTitle",
    };
    const items = effectiveMenuIds(menuOverride).map((id): MenuItem => ({
      label: t(labelKeys[id] ?? id),
      onClick: blankMenuActions[id as keyof typeof blankMenuActions],
    }));
```

- [ ] **Step 8.4: 桌面右键菜单（DesktopIcons）**

`DesktopIcons.tsx` L1570-1583 的 `wallpaperSwitch` children 中、`{ separator: true }`（L1578）之前追加：

```tsx
          { label: t("wpOpenCenter"), onClick: () => void import("../windows/appWindows").then(({ openWallpaperCenter }) => openWallpaperCenter()) },
          { label: t("wpStudioTitle"), onClick: () => window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: "wallpaper-studio" } })) },
```

（`wpStudioTitle` 若词典不存在——先 `rg -n "wpStudioTitle" src/i18n/dictionaries.ts`；Workshop 的 labels.ts 是独立词条系统 `wpT()`，桌面菜单用的是 i18n `t()`——若缺 key 则在 Task 7 的三语区补 `wpStudioTitle: "壁纸工坊" / "桌布工坊" / "Wallpaper Studio"`。）

同时确认 Workshop 模块在 desktop 窗口被 import（否则 open-feature 无人监听）：`DesktopShell.tsx` import 区追加一次激活：

```tsx
// 壁纸工坊 overlay 激活（ai04:open-feature → Workshop 挂载；模块加载即监听）
void import("../wallpaper/Workshop");
```

（放在 DesktopShell 模块顶层（import 语句后），与 Workshop 自挂载协议一致。）

- [ ] **Step 8.5: 类型检查 + 菜单测试**

Run: `npx tsc --noEmit`
Expected: 0 error

Run: `npx vitest run src/system/desktop/__tests__ src/system/taskbar/__tests__`
Expected: PASS（含既有 taskbarMenu 测试——若断言默认三项需同步更新为四项）

- [ ] **Step 8.6: Commit**

```bash
git add src/system/taskbar/Taskbar.tsx src/system/desktop/taskbarMenu.ts src/system/desktop-icons/DesktopIcons.tsx src/system/desktop/DesktopShell.tsx src/system/desktop/__tests__
git commit -m "feat(taskbar): Windows任务栏智能让位 + 壁纸中心三入口 + Workshop首次接通"
```

---

### Task 9: 全量自检 + 提交推送

- [ ] **Step 9.1: 全量前端检查**

Run: `npx tsc --noEmit`
Expected: 0 error

Run: `npx vitest run`
Expected: 全绿（如有失败逐个修复——先看是否本计划改动引入）

- [ ] **Step 9.2: 全量 Rust 检查**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 0 error / 全绿

- [ ] **Step 9.3: 构建冒烟（dev 模式启动验证窗口可开）**

Run: `npm run build`
Expected: 成功产出 dist/（含 wallpaper-center.html bundle）

- [ ] **Step 9.4: 实机手测清单（人工）**

- 桌面右键/任务栏空区 → 壁纸中心窗口打开，几何记忆生效
- 中心扫描 WE 项目 → 网格出卡片 → 选中 → 属性滑杆 → 应用 → 桌面 living 生效（粒子密度/漂移变化可感知）
- 添加本地目录/图片 → 网格出现 → 双击应用
- 播放列表加入 2 项 → 间隔 1 分钟 → 开启 → 1 分钟后桌面自动换
- 打开 Steam（或启动 EcoTab 里的游戏）→ 横幅出现（CEF 兼容）→ Windows 任务栏可见时 Variable 底栏上移让位 → 关 Steam 30s 后置顶恢复、底栏归位
- 创建 tab 四引擎可用；Workshop 右键入口打开 overlay；Workshop 应用按钮生效（desktop 壁纸变化）

- [ ] **Step 9.5: Commit + Push**

```bash
git add -A
git commit -m "feat(wallpaper): 本地壁纸中心+任务栏智能让位+Steam兼容——不依赖Steam的完整壁纸体验"
git push origin main
```

---

## Self-Review 结论（计划自审）

1. **规格覆盖**：壁纸中心三栏✓（Task 6/7）；跨窗口应用✓（bridge）；CustomBg 新字段✓（Task 4）；GPU 自律✓（center-open/closed + suppress）；任务栏让位 Rust+前端✓（Task 2/8）；Steam 兼容（进程/主动退让/30s 恢复/事件更名/文案）✓（Task 1/3）；入口三处✓（Task 8）；Workshop 可达性修复✓（Task 8——规格"保留"的实际兑现）；本地图片目录✓（wp_list_images）；播放列表✓（PlaylistBar + runner）；测试计划✓（各任务内联 + Task 9 全量）。**视频音量/播放速度/对齐**：有意识裁剪，已在计划头部"诚实裁剪"声明理由（壁纸视频恒静音是既有产品行为）。
2. **占位符扫描**：Task 6 Step 6.5 的 bridge.ts 草稿含两段实施时要合并的说明（zustand store 与占位死代码）——已用注释明确标注"实施时删除"，属计划内指令而非 TBD；其余任务无占位符。
3. **类型一致性**：`LibraryItem`/`PlaylistEntry`/`PlaylistState`/`WpPatch`/`TaskbarYieldStatus` 在各任务间签名一致；`entryPatch` 在 bridge.ts 定义、中心与 runner 共用；`openWallpaperCenter` 在 Task 6 定义、Task 8 三处消费。
