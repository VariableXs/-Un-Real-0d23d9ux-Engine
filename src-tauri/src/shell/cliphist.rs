//! AI-07 · N-15 剪贴板历史中心（后端）：
//! - Win32 剪贴板序列号轮询（300ms 去抖），文本/文件路径/图片（DIB）三类型录制；
//! - 敏感应用（前台进程名命中用户名单）期间零记录；图片 >16MB 拒存并提示；
//! - 容量上限 500 条 / 200MB FIFO（钉选不淘汰）；
//! - 落盘 `data/clipboard/history.json`（DPAPI 加密；换机/换用户诚实清空）；
//! - 「关闭即焚」：面板关闭即清空本次会话（cliphist_burn，同时焚毁落盘）；
//! - 新条目推 `cliphist://changed` 事件（N-18 剪贴板正则触发器与 UI 共用）。
//! 仅 Windows 有真实行为；其余平台占位（与 hardware.rs 同策略）。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

/// 条目上限（规格 N-15：500 条 FIFO）。
const CAP_ENTRIES: usize = 500;
/// 总量上限（200MB；data 字段字节数近似）。
const CAP_BYTES: usize = 200 * 1024 * 1024;
/// 图片拒存阈值（16MB DIB）。
const IMAGE_MAX: usize = 16 * 1024 * 1024;
/// 轮询间隔（序列号变化检测；去抖 300ms）。
const POLL_MS: u64 = 300;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipEntry {
    pub id: String,
    /// text | file | image
    pub kind: String,
    /// text/file：文本；image：base64(DIB)
    pub data: String,
    pub preview: String,
    pub ts: u64,
    pub pinned: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipConfig {
    pub enabled: bool,
    /// 敏感应用进程名（小写，不含 .exe 也匹配）——期间暂停记录。
    pub sensitive_apps: Vec<String>,
    /// 关闭即焚：历史面板关闭即清空（含落盘）。
    pub burn_on_close: bool,
}

impl Default for ClipConfig {
    fn default() -> Self {
        ClipConfig { enabled: true, sensitive_apps: vec![], burn_on_close: false }
    }
}

static HISTORY: Mutex<Vec<ClipEntry>> = Mutex::new(Vec::new());
static CONFIG: Mutex<ClipConfig> = Mutex::new(ClipConfig { enabled: true, sensitive_apps: Vec::new(), burn_on_close: false });
static LAST_SEQ: AtomicU32 = AtomicU32::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn cfg() -> std::sync::MutexGuard<'static, ClipConfig> {
    CONFIG.lock().unwrap_or_else(|e| e.into_inner())
}

fn hist() -> std::sync::MutexGuard<'static, Vec<ClipEntry>> {
    HISTORY.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------- 落盘（DPAPI；复用 tools.rs 的加解密通道） ----------

fn store_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    let dir = data_dir.join("clipboard");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("history.json")
}

fn persist(data_dir: &std::path::Path) {
    let json = serde_json::to_string(&*hist()).unwrap_or_else(|_| "[]".into());
    let cipher = crate::shell::tools::dpapi_protect(json.as_bytes());
    if let Some(c) = cipher {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(c);
        let _ = crate::fsutil::atomic_write(store_path(data_dir), b64.as_bytes());
    }
    // 加密失败：不落盘（宁失勿裸——内存历史仍在，重启后清空）。
}

fn restore(data_dir: &std::path::Path) {
    let Ok(raw) = std::fs::read(store_path(data_dir)) else { return };
    use base64::Engine as _;
    let Ok(cipher) = base64::engine::general_purpose::STANDARD.decode(String::from_utf8_lossy(&raw).trim()) else {
        return;
    };
    let Some(plain) = crate::shell::tools::dpapi_unprotect(&cipher) else {
        return; // 换机/换用户：解不开 → 诚实清空
    };
    if let Ok(list) = serde_json::from_slice::<Vec<ClipEntry>>(&plain) {
        *hist() = list;
    }
}

// ---------- 容量与录入 ----------

fn push_entry(e: ClipEntry) {
    let mut h = hist();
    // 去重：同 data 已存在则提到最前（保留钉选状态）
    if let Some(pos) = h.iter().position(|x| x.data == e.data) {
        let existed = h.remove(pos);
        let merged = ClipEntry { pinned: existed.pinned, ..e };
        h.insert(0, merged);
    } else {
        h.insert(0, e);
    }
    // FIFO：先淘汰未钉选的最旧条目，再检查总量
    while h.len() > CAP_ENTRIES {
        match h.iter().rposition(|x| !x.pinned) {
            Some(pos) => {
                h.remove(pos);
            }
            None => break, // 全部钉选：不淘汰（用户显式行为优先）
        }
    }
    while h.iter().map(|x| x.data.len()).sum::<usize>() > CAP_BYTES && h.iter().any(|x| !x.pinned) {
        if let Some(pos) = h.iter().rposition(|x| !x.pinned) {
            h.remove(pos);
        }
    }
}

/// 文本是否更像文件路径（盘符 / UNC）。
fn looks_like_path(s: &str) -> bool {
    let s = s.trim();
    if s.len() >= 600 || s.contains('\n') {
        return false;
    }
    let bytes = s.as_bytes();
    if bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
        return true;
    }
    s.starts_with("\\\\")
}

// ---------- Win32 剪贴板捕获（仅 Windows） ----------

#[cfg(windows)]
mod win {
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
        OpenClipboard,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    pub const CF_UNICODETEXT: u32 = 13;
    pub const CF_HDROP: u32 = 15;
    pub const CF_DIB: u32 = 8;

    /// 当前剪贴板序列号（0 = 不支持）。
    pub fn seq() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    /// 前台窗口进程名（小写；取不到 → 空串）。
    pub fn foreground_process() -> String {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return String::new();
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return String::new();
            }
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return String::new();
            };
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = windows::Win32::Foundation::CloseHandle(h);
            if !ok {
                return String::new();
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
        }
    }

    fn lock_copy<T>(h: &windows::Win32::Foundation::HANDLE, f: impl FnOnce(*const u8, usize) -> T) -> Option<T> {
        let g = windows::Win32::Foundation::HGLOBAL(h.0 as *mut _);
        unsafe {
            let ptr = GlobalLock(g);
            if ptr.is_null() {
                return None;
            }
            let size = GlobalSize(g);
            let out = f(ptr as *const u8, size);
            let _ = GlobalUnlock(g);
            Some(out)
        }
    }

    /// 读取剪贴板文本（CF_UNICODETEXT）。
    pub fn read_text() -> Option<String> {
        unsafe {
            if OpenClipboard(None).is_err() {
                return None;
            }
            let out = (|| {
                if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
                    return None;
                }
                let h = GetClipboardData(CF_UNICODETEXT).ok()?;
                lock_copy(&h, |ptr, _size| {
                    let base = ptr as *const u16;
                    let mut len = 0usize;
                    while *base.add(len) != 0 {
                        len += 1;
                        if len > 10_000_000 {
                            break; // 超长保护
                        }
                    }
                    String::from_utf16_lossy(std::slice::from_raw_parts(base, len))
                })
            })();
            let _ = CloseClipboard();
            out
        }
    }

    /// 读取文件路径列表（CF_HDROP；多路径换行连接）。
    pub fn read_files() -> Option<String> {
        unsafe {
            if OpenClipboard(None).is_err() {
                return None;
            }
            let out = (|| {
                if IsClipboardFormatAvailable(CF_HDROP).is_err() {
                    return None;
                }
                let h = GetClipboardData(CF_HDROP).ok()?;
                let drop = HDROP(h.0);
                let count = DragQueryFileW(drop, u32::MAX, None);
                if count == 0 {
                    return None;
                }
                let mut paths: Vec<String> = Vec::new();
                for i in 0..count.min(64) {
                    let mut buf = [0u16; 1024];
                    let n = DragQueryFileW(drop, i, Some(&mut buf));
                    if n > 0 {
                        paths.push(String::from_utf16_lossy(&buf[..n as usize]));
                    }
                }
                Some(paths.join("\n"))
            })();
            let _ = CloseClipboard();
            out
        }
    }

    /// 读取图片 DIB 字节（CF_DIB；>16MB 由调用方拒存）。
    pub fn read_dib() -> Option<Vec<u8>> {
        unsafe {
            if OpenClipboard(None).is_err() {
                return None;
            }
            let out = (|| {
                if IsClipboardFormatAvailable(CF_DIB).is_err() {
                    return None;
                }
                let h = GetClipboardData(CF_DIB).ok()?;
                lock_copy(&h, |ptr, size| std::slice::from_raw_parts(ptr, size).to_vec())
            })();
            let _ = CloseClipboard();
            out
        }
    }

    /// 写回文本（回贴语义的写入半步；粘贴 SendInput 由前端在显式操作时执行）。
    pub fn write_text(text: &str) -> bool {
        use windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
        use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
        use windows::Win32::Foundation::HANDLE;
        unsafe {
            if OpenClipboard(None).is_err() {
                return false;
            }
            let ok = (|| {
                let mut wide: Vec<u16> = text.encode_utf16().collect();
                wide.push(0);
                let bytes = wide.len() * 2;
                let h = GlobalAlloc(GMEM_MOVEABLE, bytes).ok()?;
                let ptr = GlobalLock(h);
                if ptr.is_null() {
                    return None;
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, bytes);
                let _ = GlobalUnlock(h);
                if EmptyClipboard().is_err() {
                    return None;
                }
                // SetClipboardData 接管句柄所有权
                SetClipboardData(CF_UNICODETEXT, HANDLE(h.0)).ok()?;
                Some(())
            })()
            .is_some();
            let _ = CloseClipboard();
            ok
        }
    }
}

/// 捕获一次剪贴板内容（敏感名单命中 → None(零记录)）。返回 (条目, 是否拒存提示)。
#[cfg(windows)]
fn capture() -> Option<(Option<ClipEntry>, bool)> {
    {
        let conf = cfg();
        if !conf.enabled {
            return Some((None, false));
        }
        let fg = win::foreground_process();
        let bare = fg.trim_end_matches(".exe").to_string();
        let sensitive = conf
            .sensitive_apps
            .iter()
            .any(|s| !s.is_empty() && (fg == s.to_lowercase() || bare == s.to_lowercase()));
        if sensitive {
            return Some((None, false)); // 敏感应用期间零记录
        }
    }

    if let Some(text) = win::read_text() {
        if !text.trim().is_empty() {
            let kind = if looks_like_path(&text) { "file" } else { "text" };
            return Some((
                Some(ClipEntry {
                    id: format!("c{}", now_ms()),
                    kind: kind.into(),
                    preview: text.chars().take(80).collect(),
                    data: text,
                    ts: now_ms(),
                    pinned: false,
                }),
                false,
            ));
        }
    }
    if let Some(files) = win::read_files() {
        if !files.is_empty() {
            return Some((
                Some(ClipEntry {
                    id: format!("c{}", now_ms()),
                    kind: "file".into(),
                    preview: files.replace('\n', " · "),
                    data: files,
                    ts: now_ms(),
                    pinned: false,
                }),
                false,
            ));
        }
    }
    if let Some(dib) = win::read_dib() {
        if dib.len() > IMAGE_MAX {
            return Some((None, true)); // 超大图拒存 + 提示
        }
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&dib);
        return Some((
            Some(ClipEntry {
                id: format!("c{}", now_ms()),
                kind: "image".into(),
                preview: format!("DIB · {} KB", dib.len() / 1024),
                data: b64,
                ts: now_ms(),
                pinned: false,
            }),
            false,
        ));
    }
    Some((None, false))
}

// ---------- 看护线程 ----------

pub fn spawn_cliphist_watcher(app: AppHandle, data_dir: std::path::PathBuf) {
    #[cfg(windows)]
    {
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        restore(&data_dir);
        LAST_SEQ.store(win::seq(), Ordering::SeqCst);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
            let cur = win::seq();
            if cur == 0 || cur == LAST_SEQ.load(Ordering::SeqCst) {
                continue;
            }
            LAST_SEQ.store(cur, Ordering::SeqCst);
            let (entry, rejected) = match capture() {
                Some(x) => x,
                None => continue,
            };
            // 「剪贴板变化」事件（N-18 剪贴板正则触发器依赖；仅文本类，图片不外发内容）
            let trigger_text = entry.as_ref().filter(|e| e.kind != "image").map(|e| e.data.clone());
            if let Some(e) = entry {
                push_entry(e);
                let _ = app.emit("cliphist://changed", ());
            }
            if rejected {
                let _ = app.emit("cliphist://rejected-image", ());
            }
            if let Some(t) = trigger_text {
                let _ = app.emit("macro://clipboard-changed", t);
            }
            persist(&data_dir);
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (app, data_dir);
    }
}

// ---------- 命令 ----------

/// 历史列表（最新在前）。
#[tauri::command]
pub fn cliphist_list() -> CmdResult<Vec<ClipEntry>> {
    Ok(hist().clone())
}

/// 钉选 / 取消钉选。
#[tauri::command]
pub fn cliphist_pin(id: String, pinned: bool, st: State<'_, AppState>) -> CmdResult<()> {
    {
        let mut h = hist();
        if let Some(e) = h.iter_mut().find(|x| x.id == id) {
            e.pinned = pinned;
        }
    }
    persist(&st.data_dir);
    Ok(())
}

/// 删除单条。
#[tauri::command]
pub fn cliphist_remove(id: String, st: State<'_, AppState>) -> CmdResult<()> {
    hist().retain(|x| x.id != id);
    persist(&st.data_dir);
    Ok(())
}

/// 清空（keepPinned=true 保留钉选）。
#[tauri::command]
pub fn cliphist_clear(keep_pinned: bool, st: State<'_, AppState>) -> CmdResult<()> {
    if keep_pinned {
        hist().retain(|x| x.pinned);
    } else {
        hist().clear();
    }
    persist(&st.data_dir);
    Ok(())
}

/// 关闭即焚：面板关闭时调用（配置开启才生效）——清空内存 + 焚毁落盘。
#[tauri::command]
pub fn cliphist_burn(st: State<'_, AppState>) -> CmdResult<()> {
    if !cfg().burn_on_close {
        return Ok(());
    }
    hist().clear();
    let _ = std::fs::remove_file(store_path(&st.data_dir));
    Ok(())
}

/// 读配置。
#[tauri::command]
pub fn cliphist_config_get() -> CmdResult<ClipConfig> {
    Ok(cfg().clone())
}

/// 写配置（前端设置面板调用）。
#[tauri::command]
pub fn cliphist_config_set(config: ClipConfig) -> CmdResult<()> {
    *cfg() = config;
    Ok(())
}

/// 写回剪贴板（回贴；粘贴动作由前端在用户显式操作时执行）。
#[tauri::command]
pub fn cliphist_write_back(id: String) -> CmdResult<bool> {
    let entry = hist().iter().find(|x| x.id == id).cloned();
    let Some(e) = entry else {
        return Err(AppError::validation("条目不存在 / entry not found"));
    };
    if e.kind != "image" {
        #[cfg(windows)]
        return Ok(win::write_text(&e.data));
        #[cfg(not(windows))]
        return Ok(false);
    }
    // 图片写回：DIB base64 → SetClipboardData(CF_DIB)
    #[cfg(windows)]
    {
        use base64::Engine as _;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
        use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
        let Ok(dib) = base64::engine::general_purpose::STANDARD.decode(&e.data) else {
            return Ok(false);
        };
        unsafe {
            if OpenClipboard(None).is_err() {
                return Ok(false);
            }
            let ok = (|| {
                let h = GlobalAlloc(GMEM_MOVEABLE, dib.len()).ok()?;
                let ptr = GlobalLock(h);
                if ptr.is_null() {
                    return None;
                }
                std::ptr::copy_nonoverlapping(dib.as_ptr(), ptr as *mut u8, dib.len());
                let _ = GlobalUnlock(h);
                if EmptyClipboard().is_err() {
                    return None;
                }
                SetClipboardData(8 /* CF_DIB */, HANDLE(h.0)).ok()?;
                Some(())
            })()
            .is_some();
            let _ = CloseClipboard();
            return Ok(ok);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = e;
        Ok(false)
    }
}

// ---------- 单元测试（纯逻辑；Win32 路径由 e2e 覆盖） ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, data: &str, pinned: bool) -> ClipEntry {
        ClipEntry {
            id: id.into(),
            kind: "text".into(),
            data: data.into(),
            preview: data.into(),
            ts: 1,
            pinned,
        }
    }

    #[test]
    fn fifo_evicts_unpinned_oldest_keeps_pinned() {
        let mut h: Vec<ClipEntry> = Vec::new();
        for i in 0..8 {
            h.push(entry(&format!("e{i}"), &format!("d{i}"), i == 7));
        }
        // 模拟 push_entry 的淘汰语义（静态锁不可直接复用，此处等价重放）
        while h.len() > 5 {
            if let Some(pos) = h.iter().rposition(|x| !x.pinned) {
                h.remove(pos);
            } else {
                break;
            }
        }
        assert_eq!(h.len(), 5, "cap enforced");
        assert!(h.iter().any(|x| x.pinned), "pinned survives FIFO");
    }

    #[test]
    fn sensitive_match_case_insensitive() {
        let conf = ClipConfig {
            enabled: true,
            sensitive_apps: vec!["KeePass".into()],
            burn_on_close: false,
        };
        let hit = |proc: &str| {
            let fg = proc.to_lowercase();
            let bare = fg.trim_end_matches(".exe").to_string();
            conf.sensitive_apps
                .iter()
                .any(|s| fg == s.to_lowercase() || bare == s.to_lowercase())
        };
        assert!(hit("keepass.exe"));
        assert!(hit("KeePass.exe"));
        assert!(!hit("keepassxc.exe")); // 前缀不同名不算命中
    }

    #[test]
    fn config_roundtrip_serde() {
        let c = ClipConfig { enabled: false, sensitive_apps: vec!["bitwarden".into()], burn_on_close: true };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"burnOnClose\":true"));
        let back: ClipConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sensitive_apps, vec!["bitwarden".to_string()]);
    }

    #[test]
    fn path_detection() {
        assert!(looks_like_path("C:\\Program Files\\x"));
        assert!(looks_like_path("\\\\server\\share"));
        assert!(!looks_like_path("hello world"));
        assert!(!looks_like_path("123*7"));
    }
}
