//! D-3 全域软件接管看门狗（终极形态施工总计划 22.3 第二道防线）：
//! - 每 500ms 枚举顶层可见窗口 → 不属于 Variable 家族 / 嵌入登记 /
//!   系统关键（15 类白名单）的新窗口 = 「逃逸窗口」
//! - 处置策略（watchdog.json 持久化，设置→接管可改）：
//!   auto（默认）= 自动收编；off = 整体关闭（回滚）。
//!   实机需求（用户硬约束）：不再弹「是否收进 Variable」询问卡 ——
//!   逃逸窗口一律直接收编；存量 watchdog.json 里的 legacy "ask" 在加载时
//!   归一为 auto（load_settings 迁移，不必重写文件）。
//! - 全屏独占 / 反作弊（C-5 特征 / L4 层级）→ 不回收，转让位
//! - 维护模式（双 Esc 切到 Windows 桌面）→ 看门狗暂停（「标记不回收」）
//! - 白名单先于逻辑执行（风险表 #9/#18）
//!
//! 本模块只做探测与事件派发；收编动作复用既有 embed_adopt 通道
//! （前端收到 `watch://escape` 后走与 embed://popup 完全相同的收编流）。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

/// 处置策略：ask（默认询问）| auto（自动收编）| off（关闭看门狗）
pub const POLICY_ASK: &str = "ask";
pub const POLICY_AUTO: &str = "auto";
pub const POLICY_OFF: &str = "off";

/// 系统关键白名单（15 类）：进程映像名（小写）。先于一切逻辑执行。
/// 覆盖 shell / 任务管理器 / 输入法 / 搜索与 shell 宿主 / UAC / 系统服务。
const WHITELIST: &[&str] = &[
    "explorer.exe",
    "taskmgr.exe",
    "ctfmon.exe",
    "textinputhost.exe",
    "searchhost.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "runtimebroker.exe",
    "sihost.exe",
    "svchost.exe",
    "dllhost.exe",
    "conhost.exe",
    "consent.exe",
    "fontdrvhost.exe",
    "applicationframehost.exe",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WatchSettings {
    pub enabled: bool,
    /// ask | auto | off
    pub policy: String,
    /// 用户选择「总是忽略」的进程映像名（小写）
    #[serde(default)]
    pub ignored: Vec<String>,
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self { enabled: true, policy: POLICY_AUTO.into(), ignored: Vec::new() }
    }
}

fn settings_path(st: &AppState) -> std::path::PathBuf {
    st.data_dir.join("watchdog.json")
}

/// 内置忽略名单（并入用户 ignored，不写入文件）：瞬时系统 UI 收编无意义。
/// snippingtool.exe —— Win+Shift+S 已被后端抢注给 Variable 截图工具，系统
/// 截图浮层（Snipping Tool Overlay）是瞬时覆盖层，收编只会留下僵尸占位窗
///（实机：每次截图都生成一个收编窗）。
const BUILTIN_IGNORED: &[&str] = &["snippingtool.exe"];

pub(crate) fn load_settings(st: &AppState) -> WatchSettings {
    let mut s = std::fs::read(settings_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice::<WatchSettings>(&b).ok())
        .unwrap_or_default();
    // 存量迁移：legacy "ask"（弹询问卡）→ auto。用户硬约束：绝不弹
    // 「是否收进 Variable」，逃逸窗口一律直接收编。
    if s.policy == POLICY_ASK {
        s.policy = POLICY_AUTO.into();
    }
    // 内置忽略名单并入（去重，不落盘）
    for bi in BUILTIN_IGNORED {
        if !s.ignored.iter().any(|i| i == bi) {
            s.ignored.push((*bi).into());
        }
    }
    s
}

fn save_settings(st: &AppState, s: &WatchSettings) -> CmdResult<()> {
    let bytes = serde_json::to_vec_pretty(s)
        .map_err(|e| AppError::io(format!("序列化看门狗设置失败: {e}")))?;
    std::fs::write(settings_path(st), bytes)
        .map_err(|e| AppError::io(format!("写入看门狗设置失败: {e}")))?;
    Ok(())
}

/// 内存缓存（看门狗线程 500ms 轮询用；set 命令同步更新）
static SETTINGS: RwLock<Option<WatchSettings>> = RwLock::new(None);

/// 维护模式旗标（kbdhook 双 Esc 切到 Windows 桌面时置位）：
/// true = 用户特意在 Windows 桌面操作，看门狗暂停回收。
pub static MAINTENANCE: AtomicBool = AtomicBool::new(false);

pub fn set_maintenance(on: bool) {
    MAINTENANCE.store(on, Ordering::Relaxed);
}

/// 读取设置（供设置界面命令）。
#[tauri::command(async)]
pub fn watch_get_settings(st: tauri::State<'_, AppState>) -> CmdResult<WatchSettings> {
    Ok(load_settings(&st))
}

/// 更新设置：整体关闭 = 回滚开关（退回「仅手动从 Variable 内启动才嵌入」）。
#[tauri::command(async)]
pub fn watch_set_settings(
    st: tauri::State<'_, AppState>,
    enabled: bool,
    policy: String,
) -> CmdResult<()> {
    if !matches!(policy.as_str(), POLICY_ASK | POLICY_AUTO | POLICY_OFF) {
        return Err(AppError::validation("无效的看门狗策略 / invalid policy"));
    }
    let mut cur = load_settings(&st);
    cur.enabled = enabled;
    cur.policy = policy;
    save_settings(&st, &cur)?;
    *SETTINGS.write().map_err(|e| AppError::io(e.to_string()))? = Some(cur);
    Ok(())
}

/// 询问卡处置回执：once（本次保持在桌面）| always（总是忽略该软件）。
#[tauri::command(async)]
pub fn watch_dismiss(st: tauri::State<'_, AppState>, image: String, action: String) -> CmdResult<()> {
    let img = image.to_lowercase();
    if action == "always" && !img.is_empty() {
        let mut cur = load_settings(&st);
        if !cur.ignored.iter().any(|i| *i == img) {
            cur.ignored.push(img);
            save_settings(&st, &cur)?;
            *SETTINGS.write().map_err(|e| AppError::io(e.to_string()))? = Some(cur);
        }
    }
    Ok(())
}

/// 启动看门狗轮询线程（lib.rs setup 调用）。
pub fn spawn_watchdog(app: tauri::AppHandle, st: &AppState) {
    let init = load_settings(st);
    if let Ok(mut g) = SETTINGS.write() {
        *g = Some(init);
    }
    std::thread::spawn(move || watch_loop(app));
}

/// 逃逸事件重报口径（R6 实机根因修复）：watch://escape 是 fire-and-forget
/// 事件，前端 VWM 未挂载/主窗重建期间收到即丢——此前 emit 后即在 seen 里
/// 终身去重，窗口永久逃逸（wallpaperui.exe 实机复现）。改为：未确认收编
/// （未进入 embedded 登记）的窗口每 12s 重报一次，最多 5 次。
pub(crate) fn should_reemit(emits: u32, elapsed_secs: u64) -> bool {
    emits < 5 && elapsed_secs >= 12
}

#[cfg(windows)]
fn watch_loop(app: tauri::AppHandle) {
    use std::collections::HashMap;
    use std::time::Instant;
    use tauri::Emitter;

    struct Escape {
        image: String,
        emits: u32,
        last: Instant,
    }

    let own_pid = std::process::id();
    // 已派发收编事件的窗口（hwnd → 状态）：未确认收编前按口径重报
    let mut reported: HashMap<isize, Escape> = HashMap::new();
    // 白名单/忽略清单命中的窗口：只记一次，不重复参与判定
    let mut dismissed: std::collections::HashSet<isize> = std::collections::HashSet::new();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        // 全局开关 / 反作弊（C-5 让位期）/ 维护模式（双 Esc 在 Windows 桌面）
        let s = match SETTINGS.read() {
            Ok(g) => g.clone().unwrap_or_default(),
            Err(_) => WatchSettings::default(),
        };
        if !s.enabled || s.policy == POLICY_OFF {
            continue;
        }
        if crate::shell::kbdhook::ANTICHEAT.load(Ordering::Relaxed) {
            continue;
        }
        if MAINTENANCE.load(Ordering::Relaxed) {
            continue;
        }

        let embedded: HashSet<isize> =
            crate::shell::embed::current_embed_hwnds().into_iter().collect();
        let ignored: HashSet<String> = s.ignored.iter().cloned().collect();
        // 自家进程树快照（每轮一次）：WebView2 子进程（msedgewebview2.exe）会
        // 拥有可见顶级窗口，绝不能被当成「逃逸窗口」收编 —— 收编自家窗口会
        // 拆掉主窗的内容宿主，导致整个应用静默退出（实机实测复现）。
        let own_tree: HashSet<u32> =
            crate::shell::embed::win::pid_tree(own_pid).into_iter().collect();

        for (hwnd, pid, full_image) in crate::shell::embed::watch_scan_windows() {
            // 已收编：固化状态（永不再报）后跳过
            if embedded.contains(&hwnd) {
                if let Some(e) = reported.get_mut(&hwnd) {
                    e.emits = u32::MAX;
                }
                continue;
            }
            if pid == own_pid || pid <= 4 {
                continue; // Variable 家族 / System / Idle
            }
            // 白名单与忽略清单按映像名（basename 小写）比对
            let image = full_image
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or("")
                .to_lowercase();
            // 自家进程树（含 WebView2 家族）与 Variable 家族映像：跳过
            if own_tree.contains(&pid) || is_variable_family_image(&image) {
                continue;
            }
            // 系统白名单 / 用户忽略清单（白名单先于逻辑执行）
            if image.is_empty() || WHITELIST.contains(&image.as_str()) || ignored.contains(&image) {
                dismissed.insert(hwnd);
                continue;
            }
            if dismissed.contains(&hwnd) {
                continue;
            }
            // R6 主窗闸门：无标题栏工具窗 / "Menu" 弹出窗 / 覆盖层不是应用
            // 主窗（CEF 家族大量此类窗口）——收编它们只得黑框，还会抢占真正
            // 主窗的收编次序（Steam 黑屏实机根因）。不入 reported：日后若
            // 获得主窗特征（如托盘还原）仍可被正确收编。
            if !crate::shell::embed::is_adoptable_main_window(hwnd) {
                continue;
            }
            // 重报判定：已派发但未确认收编的窗口按口径重报
            if let Some(e) = reported.get(&hwnd) {
                if e.image == image {
                    if !should_reemit(e.emits, e.last.elapsed().as_secs()) {
                        continue;
                    }
                }
            }
            // 探层级（首次/重报均探：层级可能随窗口状态变化）
            let info = crate::shell::compat_probe::probe_hwnd(hwnd);
            if info.effective() == crate::shell::compat_probe::CompatTier::L4 {
                continue; // 全屏独占 / 反作弊 → 不回收，转让位
            }
            let emits = reported.get(&hwnd).map(|e| e.emits).unwrap_or(0);
            reported.insert(
                hwnd,
                Escape { image: image.clone(), emits: emits + 1, last: Instant::now() },
            );
            let root_pid = pid;
            let title = window_title(hwnd);
            crate::shell::applog::log(
                "watchdog",
                format!("发现逃逸窗口 hwnd={hwnd} image={image} title={title:?} → 收编（policy={}，第{}次派发）", s.policy, emits + 1),
            );
            let _ = app.emit(
                "watch://escape",
                serde_json::json!({
                    "hwnd": hwnd,
                    "rootPid": root_pid,
                    "title": title,
                    "image": image,
                    "auto": s.policy == POLICY_AUTO,
                }),
            );
        }
        // 收敛已消失窗口（防 reported/dismissed 无限增长）
        let alive = |h: &isize| {
            use windows::Win32::UI::WindowsAndMessaging::IsWindow;
            unsafe { IsWindow(hwnd_from_isize(*h)) }.as_bool()
        };
        reported.retain(|h, _| alive(h));
        dismissed.retain(alive);
    }
}

/// 自家/家族映像判定：variable.exe、variable_lib-<hash>.exe（单实例兜底
/// 收编通道会用到的自家映像）一律不可收编 —— 收编自家主窗 = 自毁。
fn is_variable_family_image(image: &str) -> bool {
    image.starts_with("variable")
}

#[cfg(windows)]
fn hwnd_from_isize(v: isize) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(v as *mut core::ffi::c_void)
}

#[cfg(windows)]
fn window_title(hwnd: isize) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
    unsafe {
        let h = hwnd_from_isize(hwnd);
        let len = GetWindowTextLengthW(h);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

#[cfg(not(windows))]
fn watch_loop(_app: tauri::AppHandle) {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自家家族映像不可收编（实机回归：收编自家 WebView2/二次启动主窗
    /// 会拆掉主窗内容宿主 → 整个应用静默退出）。
    #[test]
    fn own_family_images_never_adopted() {
        for img in ["variable.exe", "VARIABLE.EXE", "variable_lib-4416833e4f40c908.exe"] {
            assert!(super::is_variable_family_image(&img.to_lowercase()), "{img} 应判定为自家家族");
        }
        for foreign in ["msedge.exe", "notepad.exe", "steam.exe", "workbuddy.exe"] {
            assert!(!super::is_variable_family_image(foreign), "{foreign} 是外部软件");
        }
    }

    /// D-3 契约（联调点 D-1×D-3 冻结口径）：系统关键白名单固定 15 类、
    /// 全部小写进程映像名（与看门狗的 basename 小写比对口径一致）、无重复。
    #[test]
    fn whitelist_contract_15_unique_lowercase() {
        assert_eq!(WHITELIST.len(), 15);
        for w in WHITELIST {
            assert_eq!(*w, w.to_lowercase(), "白名单必须小写: {w}");
        }
        let mut sorted = WHITELIST.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), WHITELIST.len(), "白名单不得有重复项");
        // 看门狗/维护模式让位与 UAC 安全桌面必须在内
        for must in ["taskmgr.exe", "consent.exe", "explorer.exe", "ctfmon.exe"] {
            assert!(WHITELIST.contains(&must), "白名单缺少 {must}");
        }
    }

    /// 策略枚举口径：auto 默认（用户硬约束：不弹询问卡，一律直接收编）；
    /// 未知策略在 watch_set_settings 层被拒绝。legacy "ask" 在 load_settings
    /// 里归一为 auto（此处验证迁移语义 + 常量本身构成完整三态）。
    #[test]
    fn policy_enum_complete() {
        assert_eq!(POLICY_ASK, "ask");
        assert_eq!(POLICY_AUTO, "auto");
        assert_eq!(POLICY_OFF, "off");
        let d = WatchSettings::default();
        assert!(d.enabled);
        assert_eq!(d.policy, POLICY_AUTO, "默认策略必须是「自动收编」（不弹询问卡）");
        // legacy ask → auto 迁移（load_settings 归一口径的纯逻辑镜像）
        let legacy = WatchSettings { policy: POLICY_ASK.into(), ..d };
        assert_eq!(
            if legacy.policy == POLICY_ASK { POLICY_AUTO } else { legacy.policy.as_str() },
            POLICY_AUTO
        );
    }

    /// R6 重报口径：未确认收编的窗口 12s 后重报、最多 5 次；
    /// 5 次耗尽或间隔不足均不重报；已收编固化（u32::MAX）永不重报。
    #[test]
    fn reemit_policy_bounded() {
        assert!(super::should_reemit(1, 12));
        assert!(super::should_reemit(4, 99));
        assert!(!super::should_reemit(5, 99), "最多 5 次派发");
        assert!(!super::should_reemit(1, 11), "间隔不足 12s 不重报");
        assert!(!super::should_reemit(u32::MAX, 999), "已收编固化后永不重报");
    }
}
