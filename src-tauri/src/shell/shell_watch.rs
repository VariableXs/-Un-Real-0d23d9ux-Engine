//! D-3 全域软件接管看门狗（终极形态施工总计划 22.3 第二道防线）：
//! - 每 500ms 枚举顶层可见窗口 → 不属于 Variable 家族 / 嵌入登记 /
//!   系统关键（15 类白名单）的新窗口 = 「逃逸窗口」
//! - 处置策略（watchdog.json 持久化，设置→接管可改）：
//!   ask（默认）= 前端弹询问卡；auto = 自动收编；off = 整体关闭（回滚）
//! - 全屏独占 / 反作弊（C-5 特征 / L4 层级）→ 不回收，转让位
//! - 维护模式（双 Esc 切到 Windows 桌面）→ 看门狗暂停（「标记不回收」）
//! - 白名单先于逻辑执行（风险表 #9/#18）；默认「询问」不自动回收
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
        Self { enabled: true, policy: POLICY_ASK.into(), ignored: Vec::new() }
    }
}

fn settings_path(st: &AppState) -> std::path::PathBuf {
    st.data_dir.join("watchdog.json")
}

pub(crate) fn load_settings(st: &AppState) -> WatchSettings {
    std::fs::read(settings_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
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
#[tauri::command]
pub fn watch_get_settings(st: tauri::State<'_, AppState>) -> CmdResult<WatchSettings> {
    Ok(load_settings(&st))
}

/// 更新设置：整体关闭 = 回滚开关（退回「仅手动从 Variable 内启动才嵌入」）。
#[tauri::command]
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
#[tauri::command]
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

#[cfg(windows)]
fn watch_loop(app: tauri::AppHandle) {
    use std::collections::HashMap;
    use tauri::Emitter;

    let own_pid = std::process::id();
    // 会话内已见窗口（hwnd → 进程映像）：逃逸窗口只报一次，不重复打扰
    let mut seen: HashMap<isize, String> = HashMap::new();

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

        for (hwnd, pid, full_image) in crate::shell::embed::watch_scan_windows() {
            if embedded.contains(&hwnd) {
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
            // 系统白名单 / 用户忽略清单（白名单先于逻辑执行）
            if image.is_empty()
                || WHITELIST.contains(&image.as_str())
                || ignored.contains(&image)
                || seen.get(&hwnd).map(|prev| *prev == image).unwrap_or(false)
            {
                seen.insert(hwnd, image);
                continue;
            }
            // 首次见到的窗口：探层级
            let info = crate::shell::compat_probe::probe_hwnd(hwnd);
            seen.insert(hwnd, image.clone());
            if info.effective() == crate::shell::compat_probe::CompatTier::L4 {
                continue; // 全屏独占 / 反作弊 → 不回收，转让位
            }
            let root_pid = pid;
            let title = window_title(hwnd);
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
        // 收敛已消失窗口（防 seen 无限增长）
        seen.retain(|h, _| {
            use windows::Win32::UI::WindowsAndMessaging::IsWindow;
            unsafe { IsWindow(hwnd_from_isize(*h)) }.as_bool()
        });
    }
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

    /// 策略枚举口径：ask 默认；未知策略在 watch_set_settings 层被拒绝
    /// （此处验证常量本身构成完整三态）。
    #[test]
    fn policy_enum_complete() {
        assert_eq!(POLICY_ASK, "ask");
        assert_eq!(POLICY_AUTO, "auto");
        assert_eq!(POLICY_OFF, "off");
        let d = WatchSettings::default();
        assert!(d.enabled);
        assert_eq!(d.policy, POLICY_ASK, "默认策略必须是「询问」，不自动回收");
    }
}
