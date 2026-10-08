//! AI-19 无障碍与本地化组 — 系统辅助功能桥（M-73）与系统高对比度跟随（M-74）。
//!
//! 红线（承 SUMMIT 域 U 口径）：
//! - 只读探针：SystemParametersInfo 读取粘滞键/筛选键/高对比度标志，
//!   讲述人（Narrator）以进程快照探测；零写入、零模拟；
//! - 屏幕阅读器本体是系统职责（不实现自家阅读器），环境只做
//!   「讲述人运行 → 动效自动降级」的联动信号；
//! - 探测失败一律如实返回 false / 错误，绝不编造状态。

use serde::Serialize;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct A11yProbe {
    /// 系统粘滞键已开启（SKF_STICKYKEYSON）。
    pub sticky_keys: bool,
    /// 系统筛选键已开启（FKF_FILTERKEYSON）。
    pub filter_keys: bool,
    /// 系统高对比度已开启（HCF_HIGHCONTRASTON）。
    pub high_contrast: bool,
    /// 讲述人（narrator.exe）正在运行。
    pub narrator_running: bool,
    /// 探测发生的时间（前端用于跟随延迟计量与日志）。
    pub probed_at: u64,
}

/// M-73/M-74：一次性读取系统辅助功能状态（只读探针）。
/// 前端按需轮询（hcFollow 开启时 ~500ms，其余场景低频或手动），
/// HC 切换的 300ms 跟随目标由轮询间隔 + 前端立即应用共同达成。
#[tauri::command(async)]
pub fn a11y_probe() -> Result<A11yProbe, String> {
    #[cfg(windows)]
    {
        let sticky_keys = unsafe { get_flag_sticky_keys() };
        let filter_keys = unsafe { get_flag_filter_keys() };
        let high_contrast = unsafe { get_flag_high_contrast() };
        let narrator_running = narrator_running();
        Ok(A11yProbe {
            sticky_keys,
            filter_keys,
            high_contrast,
            narrator_running,
            probed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        })
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

#[cfg(windows)]
unsafe fn get_flag_sticky_keys() -> bool {
    use windows::Win32::UI::Accessibility::STICKYKEYS;
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETSTICKYKEYS};
    let mut sk = STICKYKEYS {
        cbSize: std::mem::size_of::<STICKYKEYS>() as u32,
        dwFlags: windows::Win32::UI::Accessibility::STICKYKEYS_FLAGS(0),
    };
    // SKF_STICKYKEYSON = 0x00000001
    let ok = SystemParametersInfoW(
        SPI_GETSTICKYKEYS,
        std::mem::size_of::<STICKYKEYS>() as u32,
        Some(&mut sk as *mut STICKYKEYS as *mut core::ffi::c_void),
        windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    ok.is_ok() && (sk.dwFlags.0 & 0x1) != 0
}

#[cfg(windows)]
unsafe fn get_flag_filter_keys() -> bool {
    use windows::Win32::UI::Accessibility::FILTERKEYS;
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETFILTERKEYS};
    let mut fk = FILTERKEYS {
        cbSize: std::mem::size_of::<FILTERKEYS>() as u32,
        dwFlags: 0,
        iWaitMSec: 0,
        iDelayMSec: 0,
        iRepeatMSec: 0,
        iBounceMSec: 0,
    };
    // FKF_FILTERKEYSON = 0x00000001
    let ok = SystemParametersInfoW(
        SPI_GETFILTERKEYS,
        std::mem::size_of::<FILTERKEYS>() as u32,
        Some(&mut fk as *mut FILTERKEYS as *mut core::ffi::c_void),
        windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    ok.is_ok() && (fk.dwFlags & 0x1) != 0
}

#[cfg(windows)]
unsafe fn get_flag_high_contrast() -> bool {
    use windows::Win32::UI::Accessibility::HIGHCONTRASTW;
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST};
    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: windows::Win32::UI::Accessibility::HIGHCONTRASTW_FLAGS(0),
        lpszDefaultScheme: windows::core::PWSTR::null(),
    };
    // HCF_HIGHCONTRASTON = 0x00000001
    let ok = SystemParametersInfoW(
        SPI_GETHIGHCONTRAST,
        std::mem::size_of::<HIGHCONTRASTW>() as u32,
        Some(&mut hc as *mut HIGHCONTRASTW as *mut core::ffi::c_void),
        windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    ok.is_ok() && (hc.dwFlags.0 & 0x1) != 0
}

/// 讲述人进程探测：narrator.exe（大小写不敏感精确匹配）。
#[cfg(windows)]
fn narrator_running() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let Ok(snap) = snap else { return false };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = false;
    if unsafe { Process32FirstW(snap, &mut entry) }.is_ok() {
        loop {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
            if exe == "narrator.exe" {
                found = true;
                break;
            }
            if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snap) };
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_shape_is_camel_case() {
        // 序列化字段名契约（前端 ipc 依赖 camelCase）。
        let p = A11yProbe {
            sticky_keys: false,
            filter_keys: false,
            high_contrast: false,
            narrator_running: false,
            probed_at: 0,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"stickyKeys\""));
        assert!(json.contains("\"narratorRunning\""));
        assert!(json.contains("\"highContrast\""));
    }
}
