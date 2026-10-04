//! 批次E-8（规格 45 边界）：通讯软件消息提醒（微信 / QQ / 钉钉 / 飞书 / Telegram / 企业微信）。
//!
//! 边界如实声明：通讯软件的聊天内容属于对方进程内存，Variable 不读取、也无法读取。
//! 这里只做两件事 ——
//! 1. 轮询进程快照，检测哪些受支持的通讯软件正在运行；
//! 2. 读取其**顶层窗口标题**（例如微信主窗口未读时标题为「微信(N)」），把
//!    标题里的未读计数变化作为"有新消息"的信号，推给 Variable 通知中心。
//! 消息正文永远不经过 Variable。轮询 3s，零网络。

use serde::Serialize;
use tauri::Emitter;

/// 通讯软件进程名（小写）→ 显示名。
const IM_APPS: &[(&str, &str)] = &[
    ("weixin.exe", "微信"),
    ("wechat.exe", "微信"),
    ("qq.exe", "QQ"),
    ("wxwork.exe", "企业微信"),
    ("dingtalk.exe", "钉钉"),
    ("dingding.exe", "钉钉"),
    ("feishu.exe", "飞书"),
    ("lark.exe", "飞书"),
    ("telegram.exe", "Telegram"),
];

/// 通知载荷（发到前端 `sys://im-msg`）。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImMsg {
    pub app: String,
    pub title: String,
}

pub fn spawn_im_watcher(app: tauri::AppHandle) {
    #[cfg(windows)]
    {
        std::thread::spawn(move || {
            let mut last: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let hits = scan_unread_titles();
                let mut alive: std::collections::HashSet<String> = std::collections::HashSet::new();
                for (app_name, title) in &hits {
                    alive.insert(app_name.clone());
                    if last.get(app_name).map(|t| t == title).unwrap_or(false) {
                        continue; // 同一标题已提醒过 → 不重复打扰
                    }
                    last.insert(app_name.clone(), title.clone());
                    let _ = app.emit(
                        "sys://im-msg",
                        ImMsg { app: app_name.clone(), title: title.clone() },
                    );
                }
                // 标题不再带未读标记的应用解除记忆（下次新未读会重新提醒）
                last.retain(|k, _| alive.contains(k));
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

/// 标题里的未读标记检测：「(3)」「（12）」「【3】」。
#[cfg(windows)]
fn has_unread_marker(title: &str) -> bool {
    let chars: Vec<char> = title.chars().collect();
    let openers = ['(', '（', '【'];
    let closers = [')', '）', '】'];
    for i in 0..chars.len() {
        if openers.contains(&chars[i]) {
            let mut j = i + 1;
            let mut digits = true;
            let mut any = false;
            while j < chars.len() && !closers.contains(&chars[j]) {
                if chars[j].is_ascii_digit() {
                    any = true;
                } else {
                    digits = false;
                }
                j += 1;
            }
            if digits && any && j < chars.len() && closers.contains(&chars[j]) {
                return true;
            }
        }
    }
    false
}

#[cfg(windows)]
struct ScanCtx {
    pids: std::collections::HashMap<u32, String>,
    self_pid: u32,
    hits: Vec<(String, String)>,
}

/// 返回当前标题带未读标记的通讯软件窗口 (显示名, 标题)。
#[cfg(windows)]
fn scan_unread_titles() -> Vec<(String, String)> {
    use windows::Win32::Foundation::{CloseHandle, LPARAM};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    // 进程快照：pid → exe 名（小写）
    let mut pids = std::collections::HashMap::new();
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if let Ok(snap) = snap {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if unsafe { Process32FirstW(snap, &mut entry) }.is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                pids.insert(entry.th32ProcessID, exe);
                if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
                    break;
                }
            }
        }
        let _ = unsafe { CloseHandle(snap) };
    }

    let mut ctx = ScanCtx { pids, self_pid: unsafe { GetCurrentProcessId() }, hits: Vec::new() };
    unsafe {
        let _ = EnumWindows(
            Some(enum_cb),
            LPARAM(&mut ctx as *mut ScanCtx as isize),
        );
    }
    ctx.hits
}

#[cfg(windows)]
use windows::Win32::Foundation::BOOL;

#[cfg(windows)]
unsafe extern "system" fn enum_cb(hwnd: windows::Win32::Foundation::HWND, lparam: windows::Win32::Foundation::LPARAM) -> BOOL {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};

    let ctx = &mut *(lparam.0 as *mut ScanCtx);
    if !IsWindowVisible(hwnd).as_bool() {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == ctx.self_pid {
        return true.into();
    }
    let Some(exe) = ctx.pids.get(&pid) else {
        return true.into();
    };
    let Some((_, display)) = IM_APPS.iter().find(|(k, _)| *k == exe) else {
        return true.into();
    };
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(hwnd, &mut buf);
    if n <= 0 {
        return true.into();
    }
    let title = String::from_utf16_lossy(&buf[..n as usize]);
    if has_unread_marker(&title) {
        ctx.hits.push((display.to_string(), title));
    }
    true.into()
}
