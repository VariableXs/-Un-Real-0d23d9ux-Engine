//! L3 shell — wallpaper.rs（批次E-6 壁纸细节）：
//! - 多显示器独立壁纸：IDesktopWallpaper（Windows 原生，每个显示器一个 device path）
//! - 每日自动换：从本地缓存目录（Bing 壁纸缓存等用户自备图片）按日期确定性取一张，
//!   或随机换一张 —— 零网络，纯本地文件选择
//! - 仅 Windows 有真实行为；其余平台返回空/报错（与 hardware.rs 同策略）

use serde::Serialize;

use crate::error::{AppError, CmdResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpMonitor {
    /// IDesktopWallpaper 的 monitor device path（SetWallpaper 直接回传使用）
    pub id: String,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
fn with_wallpaper<T>(
    f: impl FnOnce(&windows::Win32::UI::Shell::IDesktopWallpaper) -> windows::core::Result<T>,
) -> CmdResult<T> {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper};

    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let need_uninit = hr.is_ok();
    let result = (|| -> CmdResult<T> {
        let wp: IDesktopWallpaper = unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| AppError::io(format!("IDesktopWallpaper 创建失败 / create failed: {e}")))?;
        f(&wp).map_err(|e| AppError::io(format!("壁纸操作失败 / wallpaper failed: {e}")))
    })();
    if need_uninit {
        unsafe { CoUninitialize() };
    }
    result
}

/// 枚举显示器（IDesktopWallpaper 视角，id 可直接用于 SetWallpaper）。
#[tauri::command]
#[cfg(windows)]
pub fn wp_monitors() -> CmdResult<Vec<WpMonitor>> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::IDesktopWallpaper;
    with_wallpaper(|wp: &IDesktopWallpaper| -> windows::core::Result<Vec<WpMonitor>> {
        unsafe {
            let n = wp.GetMonitorDevicePathCount()?;
            let mut out = Vec::new();
            for i in 0..n {
                let id = wp.GetMonitorDevicePathAt(i)?;
                let id_s = id.to_string()?;
                let r = wp.GetMonitorRECT(PCWSTR(id.as_ptr()))?;
                out.push(WpMonitor {
                    id: id_s,
                    primary: i == 0,
                    x: r.left,
                    y: r.top,
                    width: (r.right - r.left).max(0) as u32,
                    height: (r.bottom - r.top).max(0) as u32,
                });
            }
            Ok(out)
        }
    })
}

#[tauri::command]
#[cfg(not(windows))]
pub fn wp_monitors() -> CmdResult<Vec<WpMonitor>> {
    Ok(Vec::new())
}

/// 设置某显示器（monitor 为空 = 全部显示器）的桌面壁纸为 path 指向的图片。
#[tauri::command]
#[cfg(windows)]
pub fn wp_set_monitor(monitor: String, path: String) -> CmdResult<()> {
    use windows::core::{HSTRING, PCWSTR};
    with_wallpaper(|wp| {
        unsafe {
            let mon: Option<HSTRING> = if monitor.is_empty() { None } else { Some(HSTRING::from(monitor.as_str())) };
            let img = HSTRING::from(path.as_str());
            let mon_pcw = mon
                .as_ref()
                .map(|m| PCWSTR(m.as_ptr()))
                .unwrap_or(PCWSTR::null());
            wp.SetWallpaper(mon_pcw, PCWSTR(img.as_ptr()))
        }
    })
}

#[tauri::command]
#[cfg(not(windows))]
pub fn wp_set_monitor(_monitor: String, _path: String) -> CmdResult<()> {
    Err(AppError::validation("仅 Windows 支持 / Windows only"))
}

/// 从本地缓存目录挑一张壁纸（零网络）：
/// - mode="date"：按当天日期确定性取一张（每日自动换，同一天内稳定）
/// - mode="next"：随机取一张（右键"下一张壁纸"）
/// 目录无可用图片时返回 None（前端如实提示，不伪造）。
#[tauri::command]
pub fn wp_pick_daily(dir: String, mode: String) -> CmdResult<Option<String>> {
    let root = std::path::PathBuf::from(&dir);
    if !root.is_dir() {
        return Err(AppError::not_found(format!(
            "壁纸缓存目录不存在 / cache dir not found: {dir}"
        )));
    }
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .map_err(|e| AppError::io(format!("读取目录失败 / read_dir failed: {e}")))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|e| {
                        let e = e.to_string_lossy().to_lowercase();
                        e == "jpg" || e == "jpeg" || e == "png" || e == "webp" || e == "bmp"
                    })
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Ok(None);
    }
    let idx: usize = if mode == "next" {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize ^ d.as_secs() as usize)
            .unwrap_or(0);
        nanos % files.len()
    } else {
        // 每日：以 UTC 日期天数为种子，同一天稳定命中同一张
        let days = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86_400) as usize;
        days % files.len()
    };
    Ok(files
        .get(idx)
        .map(|p| p.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_daily_is_stable_per_day_and_next_is_random_path() {
        let base = std::env::temp_dir().join(format!("variable-wp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("a.jpg"), b"x").unwrap();
        std::fs::write(base.join("b.png"), b"y").unwrap();
        std::fs::write(base.join("note.txt"), b"skip").unwrap();

        let dir = base.to_string_lossy().to_string();
        // date 模式：同一天多次调用结果一致，且必是某张图片
        let a = wp_pick_daily(dir.clone(), "date".into()).unwrap();
        let b = wp_pick_daily(dir.clone(), "date".into()).unwrap();
        assert_eq!(a, b);
        let s = a.unwrap();
        assert!(s.ends_with("a.jpg") || s.ends_with("b.png"));
        // next 模式：返回的也是目录内图片
        let n = wp_pick_daily(dir, "next".into()).unwrap().unwrap();
        assert!(n.ends_with("a.jpg") || n.ends_with("b.png"));

        // 目录不存在 → 报错；空图片目录 → None
        assert!(wp_pick_daily(base.join("nope").to_string_lossy().to_string(), "date".into()).is_err());
        let empty = base.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(wp_pick_daily(empty.to_string_lossy().to_string(), "date".into()).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&base);
    }
}
