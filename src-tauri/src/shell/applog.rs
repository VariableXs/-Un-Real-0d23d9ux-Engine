//! L3 shell — applog.rs（软件启动实时日志总线 + 系统截图热键抢注）：
//!
//! ## 启动/嵌入实时日志（实机需求：查"未捕获/打不开"要能看到每一步）
//! - 环形缓冲（最近 600 条）+ Tauri 事件 `sys://applog` 实时推送 +
//!   落盘 `<数据目录>/logs/applog-YYYYMMDD.log`（跨会话排障）。
//! - 埋点链路：tp_launch（启动通道选择/pid）→ embed_launch（等窗/超时/兜底）
//!   → attach_by_tier（层级判定证据）→ embed_adopt / Steam 看护 / D-3 看门狗。
//! - 前端：任务管理器「日志」页实时滚动查看（ipc.applog_recent 拉历史）。
//!
//! ## Win+Shift+S 抢注（实机需求：截图时不再弹出 Windows Snipping 浮层）
//! - RegisterHotKey 全局注册 Win+Shift+S：注册成功后 Windows 截图工具的
//!   同名热键注册失败 → 按键派发到 Variable（WM_HOTKEY → `sys://snapshot-hotkey`
//!   → 前端打开 Variable 截图工具），Snipping 浮层从此不再挡屏。
//! - snapshot_capture 截图前另会 WM_CLOSE 掉已开着的 Snipping 浮层残留。
//! - 低级键盘钩子方案已实测被系统静默忽略（见 kbdhook.rs 注释），不用。

use serde::Serialize;
use std::sync::Mutex;

static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();
static LOG_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 环形缓冲容量（内存友好；更早的进日志文件）。
const CAP: usize = 600;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// ms since epoch（前端本地格式化）
    pub ts: u64,
    /// 链路标签：launch / embed / adopt / steam / watchdog / capture / hotkey
    pub tag: String,
    pub msg: String,
}

static BUF: Mutex<Vec<LogEntry>> = Mutex::new(Vec::new());

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// lib.rs setup 首行调用：登记事件通道与日志目录。
pub fn init(app: tauri::AppHandle, data_dir: std::path::PathBuf) {
    let _ = APP.set(app);
    let _ = LOG_DIR.set(data_dir.join("logs"));
}

/// 记一条实时日志：eprintln + 环形缓冲 + 落盘 + 事件推送（四处失败都开放）。
pub fn log(tag: &str, msg: impl AsRef<str>) {
    let entry = LogEntry { ts: now_ms(), tag: tag.to_string(), msg: msg.as_ref().to_string() };
    eprintln!("[{tag}] {}", entry.msg);
    {
        let mut g = BUF.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() >= CAP {
            let drop = g.len() + 1 - CAP;
            g.drain(0..drop);
        }
        g.push(entry.clone());
    }
    // 落盘（按天分文件；失败静默——日志不能反过来影响启动链路）
    if let Some(dir) = LOG_DIR.get() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static DAY: AtomicU32 = AtomicU32::new(0);
        let days = (entry.ts / 86_400_000) as u32;
        let path = dir.join(format!("applog-{days}.log"));
        let day_changed = DAY.swap(days, Ordering::Relaxed) != days;
        if day_changed || !dir.exists() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            use std::io::Write;
            let secs = entry.ts / 1000;
            let ms = entry.ts % 1000;
            let _ = writeln!(f, "{secs:010}.{ms:03} [{tag}] {}", entry.msg);
        }
    }
    // 实时推送（APP 未登记时跳过——启动极早期只进缓冲）
    if let Some(app) = APP.get() {
        use tauri::Emitter;
        let _ = app.emit("sys://applog", &entry);
    }
}

/// 前端拉取缓冲内最近日志（任务管理器「日志」页打开时先补历史）。
#[tauri::command(async)]
pub fn applog_recent() -> Vec<LogEntry> {
    BUF.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// ---------- Win+Shift+S 抢注（Snipping 浮层不再弹出） ----------

/// 启动热键抢注线程。注册成功后 Windows Snipping Tool 无法再注册同名热键，
/// Win+Shift+S 一律落到 Variable：emit `sys://snapshot-hotkey` → 前端开截图工具。
/// 注册失败（被他占）→ 5s 后重试（截到 12 次为止，避免无限空转）。
pub fn spawn_snapshot_hotkey() {
    std::thread::Builder::new()
        .name("snapshot-hotkey".into())
        .spawn(|| {
            #[cfg(windows)]
            hotkey_loop();
        })
        .ok();
}

#[cfg(windows)]
fn hotkey_loop() {
    use tauri::Emitter;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
    };

    const HOTKEY_ID: i32 = 0xB064; // Variable 截图热键（自选，避开常见占用）
    let mut tries = 0u32;
    let registered = loop {
        let ok = unsafe {
            RegisterHotKey(HWND::default(), HOTKEY_ID, MOD_WIN | MOD_SHIFT | MOD_NOREPEAT, 0x53)
        }
        .is_ok();
        if ok {
            break true;
        }
        tries += 1;
        if tries == 1 {
            log("hotkey", "Win+Shift+S 注册失败（暂被占用），5s 后重试");
        }
        if tries >= 12 {
            log("hotkey", "Win+Shift+S 重试 12 次仍失败 → 放弃抢注（截图走工具页入口）");
            break false;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    };
    if !registered {
        return;
    }
    log("hotkey", "Win+Shift+S 已由 Variable 抢注：按键直达 Variable 截图，Windows Snipping 不再弹出");

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) }.as_bool() {
        if msg.message == WM_HOTKEY && msg.wParam.0 == HOTKEY_ID as usize {
            log("hotkey", "Win+Shift+S → 打开 Variable 截图工具");
            if let Some(app) = APP.get() {
                let _ = app.emit("sys://snapshot-hotkey", ());
            }
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // GetMessage 返回 0/−1 = 消息循环结束（进程退出时自然发生；热键随进程注销）
}
