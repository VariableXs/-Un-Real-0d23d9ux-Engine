//! L3 shell — audioime.rs（F-5：音量合成器 + 输入法指示 + 媒体控制读取）
//! - mixer_list/mixer_set：IAudioSessionManager2 会话级音量/静音（合成器）
//!   （嵌入应用条目与 VWM 窗口联动高亮由前端按 pid 匹配 proc_list 家族标注）
//! - ime_status/ime_list/ime_switch：任务栏 IME 指示（只读轮询 + 语言列表；
//!   切换仅影响 Variable 内输入——跨进程全局切换不承诺，边界如实声明）
//! - media_status：GlobalSystemMediaTransportControls（进度条 + 曲目信息）；
//!   探测不到（无会话/无权限）返回 None，前端简化呈现（规格允许）

use serde::Serialize;

use crate::error::{AppError, CmdResult};

#[cfg(windows)]
fn e2s(e: windows::core::Error) -> String {
    e.to_string()
}

// ---------- 音量合成器（F-5.2） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixerSession {
    pub pid: u32,
    /// 进程名（小写）；匹配 proc_list.family 可标 Variable 家族。
    pub name: String,
    /// 0.0–1.0。
    pub volume: f32,
    pub muted: bool,
}

#[cfg(windows)]
fn session_process_name(pid: u32) -> String {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut name = String::new();
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == pid {
                    name = String::from_utf16_lossy(
                        &entry.szExeFile[..entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0)],
                    );
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
        name.to_lowercase()
    }
}

#[cfg(windows)]
fn with_sessions<T>(
    f: impl FnOnce(&windows::Win32::Media::Audio::IAudioSessionEnumerator) -> Result<T, String>,
) -> Result<T, String> {
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let need_uninit = hr.is_ok();
        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e2s)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).map_err(e2s)?;
            let mgr: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None).map_err(e2s)?;
            let sessions = mgr.GetSessionEnumerator().map_err(e2s)?;
            f(&sessions)
        })();
        if need_uninit {
            CoUninitialize();
        }
        result
    }
}

/// 合成器会话列表（渲染设备；每会话 pid/进程名/音量/静音）。
#[tauri::command]
pub fn mixer_list() -> CmdResult<Vec<MixerSession>> {
    #[cfg(windows)]
    {
        use windows::core::Interface;
        use windows::Win32::Media::Audio::{IAudioSessionControl2, ISimpleAudioVolume};
        with_sessions(|sessions| unsafe {
            let n = sessions.GetCount().map_err(e2s)?;
            let mut out = Vec::new();
            for i in 0..n {
                let Ok(ctrl) = sessions.GetSession(i) else { continue };
                let Ok(c2) = ctrl.cast::<IAudioSessionControl2>() else { continue };
                let Ok(sav) = ctrl.cast::<ISimpleAudioVolume>() else { continue };
                let pid = match c2.GetProcessId() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if pid == 0 {
                    continue;
                }
                let volume = sav.GetMasterVolume().unwrap_or(1.0);
                let muted = sav.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                out.push(MixerSession {
                    pid,
                    name: session_process_name(pid),
                    volume,
                    muted,
                });
            }
            // 去重（同进程多会话合并取第一个）
            let mut seen = std::collections::HashSet::new();
            out.retain(|s| seen.insert(s.pid));
            Ok(out)
        })
        .map_err(AppError::io)
    }
    #[cfg(not(windows))]
    Err(AppError::validation("仅支持 Windows"))
}

/// 设置指定 pid 会话的音量/静音（合成器滑杆/静音按钮）。
#[tauri::command]
pub fn mixer_set(pid: u32, volume: f32, muted: bool) -> CmdResult<()> {
    #[cfg(windows)]
    {
        use windows::core::Interface;
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::Media::Audio::{IAudioSessionControl2, ISimpleAudioVolume};
        let volume = volume.clamp(0.0, 1.0);
        with_sessions(|sessions| unsafe {
            let n = sessions.GetCount().map_err(e2s)?;
            for i in 0..n {
                let Ok(ctrl) = sessions.GetSession(i) else { continue };
                let Ok(c2) = ctrl.cast::<IAudioSessionControl2>() else { continue };
                if c2.GetProcessId().unwrap_or(0) != pid {
                    continue;
                }
                let Ok(sav) = ctrl.cast::<ISimpleAudioVolume>() else { continue };
                sav.SetMasterVolume(volume, std::ptr::null()).map_err(e2s)?;
                sav.SetMute(BOOL::from(muted), std::ptr::null()).map_err(e2s)?;
                return Ok(());
            }
            Err(format!("未找到 pid {pid} 的音频会话"))
        })
        .map_err(AppError::io)
    }
    #[cfg(not(windows))]
    Err(AppError::validation("仅支持 Windows"))
}

// ---------- 输入法指示（F-5.3，只读轮询） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImeStatus {
    /// HKL 低字语言 ID（如 "0804" 简体中文 / "0409" 英语）。
    pub lang_id: String,
    /// 中文态（IME_CMODE_NATIVE）；null = 无法读取（如英文键盘）。
    pub chinese: Option<bool>,
}

#[cfg(windows)]
#[tauri::command]
pub fn ime_status() -> CmdResult<ImeStatus> {
    use windows::Win32::UI::Input::Ime::{ImmGetDefaultIMEWnd, IME_CMODE_NATIVE};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SendMessageW, WM_IME_CONTROL};
    unsafe {
        let hkl = GetKeyboardLayout(0);
        let lang_id = format!("{:04x}", hkl.0 as usize & 0xFFFF);
        // 默认 IME 窗口询问转换模式（IMC_GETCONVERSIONMODE = 0x01）——只读探测
        let hwnd = GetForegroundWindow();
        let ime_wnd = ImmGetDefaultIMEWnd(hwnd);
        let mut chinese = None;
        if !ime_wnd.0.is_null() {
            let mode = SendMessageW(
                ime_wnd,
                WM_IME_CONTROL,
                windows::Win32::Foundation::WPARAM(0x01),
                windows::Win32::Foundation::LPARAM(0),
            )
            .0;
            chinese = Some(mode & IME_CMODE_NATIVE.0 as isize != 0);
        }
        Ok(ImeStatus { lang_id, chinese })
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub fn ime_status() -> CmdResult<ImeStatus> {
    Ok(ImeStatus { lang_id: "unknown".into(), chinese: None })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImeLayout {
    pub lang_id: String,
    pub name: String,
}

/// 系统已装键盘布局列表（弹层展示；切换见 ime_switch 边界说明）。
#[cfg(windows)]
#[tauri::command]
pub fn ime_list() -> CmdResult<Vec<ImeLayout>> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayoutList, HKL};
    unsafe {
        let n = GetKeyboardLayoutList(None);
        if n <= 0 {
            return Ok(vec![]);
        }
        let mut buf = vec![HKL(std::ptr::null_mut()); n as usize];
        let got = GetKeyboardLayoutList(Some(&mut buf));
        Ok(buf[..got.max(0) as usize]
            .iter()
            .map(|h| ImeLayout {
                lang_id: format!("{:04x}", h.0 as usize & 0xFFFF),
                name: ime_name(h.0 as usize & 0xFFFF),
            })
            .collect())
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub fn ime_list() -> CmdResult<Vec<ImeLayout>> {
    Ok(vec![])
}

#[cfg(windows)]
fn ime_name(id: usize) -> String {
    match id {
        0x0804 => "中文（简体）".into(),
        0x0404 => "中文（繁體）".into(),
        0x0409 => "英语（美国）".into(),
        0x0407 => "德语".into(),
        0x0411 => "日语".into(),
        0x0412 => "韩语".into(),
        0x040c => "法语".into(),
        _ => format!("0x{id:04x}"),
    }
}

/// 切换布局：ActivateKeyboardLayout 仅作用于本进程（Variable 内输入即刻生效）；
/// 不承诺改变宿主全局布局（跨进程 Shell 挂钩越界）——弹层文案如实标注。
#[cfg(windows)]
#[tauri::command]
pub fn ime_switch(lang_id: String) -> CmdResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{ActivateKeyboardLayout, GetKeyboardLayoutList, HKL, KLF_SETFORPROCESS};
    let want = usize::from_str_radix(&lang_id, 16).map_err(|_| AppError::validation("语言 ID 非法"))?;
    unsafe {
        let n = GetKeyboardLayoutList(None);
        if n <= 0 {
            return Err(AppError::not_found("未找到可用键盘布局"));
        }
        let mut buf = vec![HKL(std::ptr::null_mut()); n as usize];
        let got = GetKeyboardLayoutList(Some(&mut buf));
        for h in &buf[..got.max(0) as usize] {
            if h.0 as usize & 0xFFFF == want {
                ActivateKeyboardLayout(*h, KLF_SETFORPROCESS);
                return Ok(());
            }
        }
    }
    Err(AppError::not_found(format!("布局 {lang_id} 不存在")))
}

#[cfg(not(windows))]
#[tauri::command]
pub fn ime_switch(_lang_id: String) -> CmdResult<()> {
    Err(AppError::validation("仅支持 Windows"))
}

// ---------- 媒体控制读取（F-5.4） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaStatus {
    pub title: String,
    pub artist: String,
    /// 已播放秒数。
    pub position_sec: f64,
    /// 曲目总长秒数（0 = 未知）。
    pub duration_sec: f64,
    /// playing / paused / other。
    pub status: String,
}

/// SMTC 全局会话读取（进度条 + 曲目信息）。探测不到返回 None（前端简化呈现）。
///
/// 防冻结纪律（2026-09-09 事故修复）：`RequestAsync().get()` 在 SMTC broker
/// 卡死时永不返回——任务栏每 2s 轮询本命令，曾在主线程上把整个环境冻成
/// WER AppHangB1（0xCFFFFFFF），导致环境无法启动。现改为 async 命令 +
/// 常驻单例工作线程 + 3s 超时：broker 卡死 → None 诚实降级（前端隐藏组件），
/// UI 永不阻塞；卡死请求的迟到结果由下轮 poll 排空丢弃。
#[tauri::command]
pub async fn media_status() -> CmdResult<Option<MediaStatus>> {
    #[cfg(windows)]
    {
        let r = tauri::async_runtime::spawn_blocking(smtc_read).await.unwrap_or(None);
        Ok(r)
    }
    #[cfg(not(windows))]
    Ok(None)
}

/// 单例 SMTC 工作线程：请求-应答。全程仅一条线程（防轮询线程泄漏）。
#[cfg(windows)]
fn smtc_read() -> Option<MediaStatus> {
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    static WORKER: OnceLock<(Sender<()>, Mutex<Receiver<Option<MediaStatus>>>)> = OnceLock::new();
    let (req_tx, res_rx) = WORKER.get_or_init(|| {
        let (req_tx, req_rx) = channel::<()>();
        let (res_tx, res_rx) = channel::<Option<MediaStatus>>();
        let _ = std::thread::Builder::new()
            .name("smtc-reader".into())
            .spawn(move || {
                // COM apartment 跟随工作线程（MTA；失败仍尽力执行——与原实现口径一致）
                let hr = unsafe {
                    windows::Win32::System::Com::CoInitializeEx(
                        None,
                        windows::Win32::System::Com::COINIT_MULTITHREADED,
                    )
                };
                let need_uninit = hr.is_ok();
                while req_rx.recv().is_ok() {
                    let _ = res_tx.send(smtc_read_blocking());
                }
                if need_uninit {
                    unsafe { windows::Win32::System::Com::CoUninitialize() };
                }
            });
        (req_tx, Mutex::new(res_rx))
    });

    // 排空上一轮超时遗留的迟到结果（陈旧数据不冒充新数据）
    if let Ok(rx) = res_rx.lock() {
        while rx.try_recv().is_ok() {}
    }
    // 发起请求；工作线程若仍卡在上一轮读取，send 仍成功（缓冲），
    // 本轮 recv_timeout 到点即返回 None。
    req_tx.send(()).ok()?;
    res_rx
        .lock()
        .ok()?
        .recv_timeout(Duration::from_secs(3))
        .unwrap_or(None)
}

/// 真正的 SMTC 阻塞读取（只在工作线程上执行）。
#[cfg(windows)]
fn smtc_read_blocking() -> Option<MediaStatus> {
    use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus};
    let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()?
        .get()
        .ok()?;
    let session = mgr.GetCurrentSession().ok()?;
    let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let timeline = session.GetTimelineProperties().ok()?;
    let playback = session.GetPlaybackInfo().ok()?;
    let status = match playback.PlaybackStatus() {
        Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing) => "playing",
        Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused) => "paused",
        _ => "other",
    };
    Some(MediaStatus {
        title: props.Title().ok()?.to_string(),
        artist: props.Artist().map(|a| a.to_string()).unwrap_or_default(),
        position_sec: timeline.Position().ok().map(|t| t.Duration as f64 / 10_000_000.0).unwrap_or(0.0),
        duration_sec: timeline.EndTime().ok().map(|t| t.Duration as f64 / 10_000_000.0).unwrap_or(0.0),
        status: status.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn ime_name_common_langs() {
        assert_eq!(ime_name(0x0804), "中文（简体）");
        assert_eq!(ime_name(0x0409), "英语（美国）");
        assert!(ime_name(0x9999).starts_with("0x"));
    }

    #[cfg(windows)]
    #[test]
    fn session_process_name_self() {
        // 自身进程名可读（非空、小写化）
        let pid = std::process::id();
        let name = session_process_name(pid);
        assert!(!name.is_empty());
        assert_eq!(name, name.to_lowercase());
    }
}
