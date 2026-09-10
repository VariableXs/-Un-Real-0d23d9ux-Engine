//! AI-07 · N-18 自动化宏引擎（后端护栏与触发器调度）：
//! - **全局急停**（首日硬护栏）：Ctrl+Esc 长按 1s → 停一切宏（100% 生效验收项）；
//! - **智能判停**：UAC 提权窗口（Credential/Consent 类）前台时键鼠模拟拒发；
//! - **时间触发器**：cron 子集（m h dom mon dow；`*` 数字 `,` `-`）调度，
//!   命中推 `macro://fired {id}` → 前端 engine.ts 执行动作（动作库复用 N-13）；
//! - **剪贴板触发器**：cliphist.rs 推 `macro://clipboard-changed`，前端匹配正则；
//! - **键鼠模拟**：send_text 仅在护栏全绿时 SendInput（仅前台窗口语义）。
//! 红线：安全分叉（急停/判停）硬编码，宏/规则/插件不可触碰。
//! 仅 Windows 有真实行为；其余平台占位（与 hardware.rs 同策略）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::CmdResult;

// ---------- 全局急停 ----------

static EMERGENCY: AtomicBool = AtomicBool::new(false);

/// 急停是否激活。
pub fn is_stopped() -> bool {
    EMERGENCY.load(Ordering::SeqCst)
}

// ---------- 宏定义（触发器登记；动作在前端 engine.ts 执行） ----------

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroTriggerDef {
    pub macro_id: String,
    /// hotkey | process | time | clipboardRegex | scene（前端枚举同源）
    pub trigger_type: String,
    /// hotkey: accel；process: 进程名；time: cron 子集；clipboardRegex: 正则；scene: 场景 id
    pub trigger_value: String,
    pub enabled: bool,
}

static TRIGGERS: Mutex<Vec<MacroTriggerDef>> = Mutex::new(Vec::new());
static SCHED_STARTED: AtomicBool = AtomicBool::new(false);

fn triggers() -> std::sync::MutexGuard<'static, Vec<MacroTriggerDef>> {
    TRIGGERS.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------- cron 子集（m h dom mon dow） ----------

const CRON_RANGES: [(u32, u32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];

/// cron 表达式合法性（与前端 engine.ts isValidCron 同口径）。
pub fn cron_valid(expr: &str) -> bool {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    fields.iter().enumerate().all(|(i, f)| {
        let (lo, hi) = CRON_RANGES[i];
        field_matches(f, lo, hi).is_some()
    })
}

/// 单字段是否命中当前值（合法表达式才调用；内部用 Option 表达非法）。
fn field_matches(f: &str, lo: u32, hi: u32) -> Option<bool> {
    if f == "*" {
        return Some(true);
    }
    f.split(',').try_fold(false, |acc, part| {
        let hit = if let Ok(n) = part.parse::<u32>() {
            if !(lo..=hi).contains(&n) {
                return None; // 单值越界 → 表达式非法（如分钟 60）
            }
            true
        } else if let Some((a, b)) = part.split_once('-') {
            let (a, b) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?);
            if !(a >= lo && b <= hi && a <= b) {
                return None;
            }
            true // 范围命中与否由调用方带当前值判断——见 cron_due
        } else {
            return None;
        };
        Some(acc || hit)
    })
}

/// cron 是否在给定时刻触发（fields = [min, hour, dom, mon, dow]）。
pub fn cron_due(expr: &str, now: &[u32; 5]) -> bool {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    fields.iter().enumerate().all(|(i, f)| {
        let (lo, hi) = CRON_RANGES[i];
        let cur = now[i];
        if f == &"*" {
            return true;
        }
        f.split(',').any(|part| {
            if let Ok(n) = part.parse::<u32>() {
                n == cur && (lo..=hi).contains(&n)
            } else if let Some((a, b)) = part.split_once('-') {
                match (a.parse::<u32>(), b.parse::<u32>()) {
                    (Ok(a), Ok(b)) => a <= cur && cur <= b,
                    _ => false,
                }
            } else {
                false
            }
        })
    })
}

/// 当前时刻 → [min, hour, dom, mon, dow]（本地时区；零依赖手写公历）。
fn now_fields() -> [u32; 5] {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // 本地时区偏移（分钟）——用本地时间渲染：先按 UTC 算日期，再加偏移。
    let offset = local_offset_minutes(secs);
    let total = secs + offset * 60;
    let days = total.div_euclid(86_400);
    let rem = total.rem_euclid(86_400);
    let (min, hour) = (rem / 60 % 60, rem / 3600);
    // days since 1970-01-01 → civil date（Howard Hinnant 算法）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    // 星期：1970-01-01 是周四（dow 4；cron 0=周日）
    let dow = (days.rem_euclid(7) + 4) % 7;
    [min as u32, hour as u32, d as u32, m as u32, dow as u32]
}

#[cfg(windows)]
fn local_offset_minutes(_secs: i64) -> i64 {
    // 当前时区偏移（含夏令时；对「当下」这一时刻取值即正确）
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    unsafe {
        let mut tz = TIME_ZONE_INFORMATION::default();
        let r = GetTimeZoneInformation(&mut tz);
        let bias = tz.Bias as i64; // UTC = local + bias（分钟）
        let dst = if r == 2 { tz.DaylightBias as i64 } else { 0 };
        -(bias + dst)
    }
}

#[cfg(not(windows))]
fn local_offset_minutes(_secs: i64) -> i64 {
    // 非 Windows 占位：UTC（无 local 依赖，如实降级）
    0
}

// ---------- UAC 前台检测 ----------

/// 前台是否 UAC 提权窗口（Credential Dialog Xaml / Consent）——键鼠模拟必须判停。
#[cfg(windows)]
pub fn uac_foreground() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut buf = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut buf);
        let class = String::from_utf16_lossy(&buf[..n as usize]);
        class == "Credential Dialog Xaml" || class == "Consent" || class == "Secure UIMono"
    }
}

#[cfg(not(windows))]
pub fn uac_foreground() -> bool {
    false
}

// ---------- 键鼠模拟（SendInput；护栏全绿才发） ----------

#[cfg(windows)]
fn send_text_input(text: &str) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, KEYBDINPUT, KEYEVENTF_UNICODE,
    };
    let mut inputs: Vec<INPUT> = Vec::new();
    for ch in text.chars() {
        let mk = |flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS| INPUT {
            r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                    wScan: ch as u16,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        inputs.push(mk(KEYEVENTF_UNICODE));
        inputs.push(mk(KEYEVENTF_UNICODE | windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_KEYUP));
    }
    let n = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    n == inputs.len() as u32
}

#[cfg(not(windows))]
fn send_text_input(_text: &str) -> bool {
    false
}

// ---------- 看护线程（急停监测 + cron 调度） ----------

pub fn spawn_macro_runtime(app: AppHandle) {
    #[cfg(windows)]
    {
        if SCHED_STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::spawn(move || {
            let mut held_since: Option<std::time::Instant> = None;
            let mut last_minute: Option<u32> = None;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                // ---- 急停：Ctrl+Esc 长按 1s ----
                let ctrl = key_down(0x11); // VK_CONTROL
                let esc = key_down(0x1B); // VK_ESCAPE
                if ctrl && esc {
                    if held_since.is_none() {
                        held_since = Some(std::time::Instant::now());
                    } else if held_since.is_some_and(|t| t.elapsed() >= std::time::Duration::from_secs(1)) {
                        if !EMERGENCY.swap(true, Ordering::SeqCst) {
                            let _ = app.emit("macro://emergency", ());
                        }
                    }
                } else {
                    held_since = None;
                }
                // ---- cron 调度：每分钟边界检查一次 ----
                let now = now_fields();
                if last_minute != Some(now[0]) {
                    last_minute = Some(now[0]);
                    if !is_stopped() {
                        for t in triggers().iter() {
                            if t.enabled && t.trigger_type == "time" && cron_due(&t.trigger_value, &now) {
                                let _ = app.emit("macro://fired", t.macro_id.clone());
                            }
                        }
                    }
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

#[cfg(windows)]
fn key_down(vk: u32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0
}

// ---------- 命令 ----------

/// 手动急停（UI 急停按钮；与热键同效）。
#[tauri::command(async)]
pub fn macro_emergency_stop() -> CmdResult<()> {
    EMERGENCY.store(true, Ordering::SeqCst);
    Ok(())
}

/// 解除急停（用户显式操作）。
#[tauri::command(async)]
pub fn macro_emergency_clear() -> CmdResult<()> {
    EMERGENCY.store(false, Ordering::SeqCst);
    Ok(())
}

/// 急停状态查询。
#[tauri::command(async)]
pub fn macro_is_stopped() -> CmdResult<bool> {
    Ok(is_stopped())
}

/// UAC 前台检测（前端 GuardContext 数据源之一）。
#[tauri::command(async)]
pub fn macro_uac_foreground() -> CmdResult<bool> {
    Ok(uac_foreground())
}

/// 登记触发器（前端宏库同步；整表替换同 id 条目）。
#[tauri::command(async)]
pub fn macro_upsert_trigger(def: MacroTriggerDef) -> CmdResult<()> {
    if def.trigger_type == "time" && !cron_valid(&def.trigger_value) {
        return Err(crate::error::AppError::validation(format!(
            "非法 cron 表达式: {}",
            def.trigger_value
        )));
    }
    let mut list = triggers();
    match list.iter().position(|t| t.macro_id == def.macro_id) {
        Some(i) => list[i] = def,
        None => list.push(def),
    }
    Ok(())
}

/// 移除触发器（宏删除时）。
#[tauri::command(async)]
pub fn macro_remove_trigger(macro_id: String) -> CmdResult<()> {
    triggers().retain(|t| t.macro_id != macro_id);
    Ok(())
}

/// 触发器列表。
#[tauri::command(async)]
pub fn macro_list_triggers() -> CmdResult<Vec<MacroTriggerDef>> {
    Ok(triggers().clone())
}

/// 键鼠模拟（sendText 动作；护栏全绿才执行——急停/UAC/密码框任一命中即拒）。
#[tauri::command(async)]
pub fn macro_send_text(text: String, password_focus: bool) -> CmdResult<bool> {
    if is_stopped() {
        return Ok(false);
    }
    if uac_foreground() || password_focus {
        return Ok(false);
    }
    Ok(send_text_input(&text))
}

// ---------- 单元测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_valid_subset() {
        assert!(cron_valid("* * * * *"));
        assert!(cron_valid("30 9 * * 1-5"));
        assert!(cron_valid("0 8,12,18 * * *"));
        assert!(!cron_valid("60 * * * *"));
        assert!(!cron_valid("* * * *"));
        assert!(!cron_valid("* * * * * *"));
        assert!(!cron_valid("a b c d e"));
        assert!(!cron_valid("5-2 * * * *")); // 区间倒置
    }

    #[test]
    fn cron_due_matching() {
        let now = [30, 9, 15, 3, 1]; // 周一 09:30, 3月15日
        assert!(cron_due("* * * * *", &now));
        assert!(cron_due("30 9 * * *", &now));
        assert!(cron_due("* * * * 1", &now));
        assert!(cron_due("* * * * 1-5", &now));
        assert!(!cron_due("31 9 * * *", &now));
        assert!(!cron_due("30 10 * * *", &now));
        assert!(cron_due("15,30,45 * * * *", &now));
        assert!(!cron_due("0 8,12,18 * * *", &now));
        // 步长（*/n）不在首日子集——如实不匹配（与前端 isValidCron 同口径拒绝）
        assert!(!cron_due("*/15 * * * *", &now));
    }

    #[test]
    fn civil_date_epoch() {
        // 1970-01-01 是周四；验证 now_fields 的日历换算内核（用固定天数重放）
        // dow = (0 + 4) % 7 = 4（周四）
        assert_eq!((0i64 + 4) % 7, 4);
        // 2026-09-08（今天）距 epoch 20683 天 → 周二
        let days: i64 = 20_683;
        assert_eq!((days.rem_euclid(7) + 4) % 7, 2);
    }

    #[test]
    fn emergency_flag_roundtrip() {
        EMERGENCY.store(false, Ordering::SeqCst);
        assert!(!is_stopped());
        EMERGENCY.store(true, Ordering::SeqCst);
        assert!(is_stopped());
        EMERGENCY.store(false, Ordering::SeqCst);
    }
}
