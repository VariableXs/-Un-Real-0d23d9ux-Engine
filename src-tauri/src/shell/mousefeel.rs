//! AI-06 V-61/V-69 — 鼠标手感面板后端（Windows SPI，合法用户级调用）。
//!
//! 红线（承化境「不越权」口径）：
//! - `mouse_params_write` 是唯一写入口：写前先把当前四参数完整备份到
//!   进程内 static，供 `mouse_params_rollback` 一键还原（写回须显式确认
//!   ——确认流在前端面板，后端只提供可回滚的写）；
//! - `pointer_speed_temp` / `pointer_speed_restore` 供 V-69 精确模式按住
//!   修饰键期间临时降速、松开即还原（不落盘、退出前恢复）；
//! - 全部走 SystemParametersInfoW（SPIF_SENDCHANGE），不做注册表绕路。
//!
//! 非 Windows 平台全部返回 not-supported（前端如实显示不可用）。

use serde::Serialize;

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct MouseParamsDto {
    pub speed: i32,
    pub double_click_ms: u32,
    pub wheel_lines: i32,
    pub swap_buttons: bool,
}

/// 写回前的系统原值备份（进程内；回滚 = 恢复这份原值）。
static BACKUP: std::sync::Mutex<Option<MouseParamsDto>> = std::sync::Mutex::new(None);
/// V-69 精确模式期间保存的原速度档（0 = 未处于降速态）。
static TEMP_SPEED_BACKUP: std::sync::Mutex<Option<i32>> = std::sync::Mutex::new(None);

/// windows-0.58 未导出的两个 SPI GET 常量（WinUser.h 原值）。
#[cfg(windows)]
const SPI_GETDOUBLECLICKTIME: u32 = 0x0020;
#[cfg(windows)]
const SPI_GETMOUSEBUTTONSWAP: u32 = 0x0021;

#[cfg(windows)]
fn spi_get_u32(action: u32) -> u32 {
    let mut v: u32 = 0;
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_ACTION(action),
            0,
            Some(&mut v as *mut u32 as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    v
}

#[cfg(windows)]
fn spi_set(action: u32, v: u32) -> bool {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_ACTION(action),
            0,
            Some(v as *mut u32 as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0x2), // SPIF_SENDCHANGE
        )
        .is_ok()
    }
}

#[cfg(windows)]
fn read_params() -> MouseParamsDto {
    use windows::Win32::UI::WindowsAndMessaging::{SPI_GETMOUSE, SPI_GETMOUSESPEED, SPI_GETWHEELSCROLLLINES};
    let mut mouse = [0i32; 3]; // [thresh1, thresh2, speed] — 速度档在第 3 位
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            SPI_GETMOUSE,
            0,
            Some(mouse.as_mut_ptr() as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    let _ = SPI_GETMOUSESPEED;
    MouseParamsDto {
        speed: mouse[2],
        double_click_ms: spi_get_u32(SPI_GETDOUBLECLICKTIME),
        wheel_lines: spi_get_u32(SPI_GETWHEELSCROLLLINES.0) as i32,
        swap_buttons: spi_get_u32(SPI_GETMOUSEBUTTONSWAP) != 0,
    }
}

#[cfg(windows)]
fn spi_set_speed(v: i32) -> bool {
    spi_set(
        windows::Win32::UI::WindowsAndMessaging::SPI_SETMOUSESPEED.0,
        v.clamp(1, 20) as u32,
    )
}

/// 读取当前系统鼠标四参数（只读；V-61 面板初始化与回滚预览用）。
#[tauri::command(async)]
pub fn mouse_params_get() -> Result<MouseParamsDto, String> {
    #[cfg(windows)]
    return Ok(read_params());
    #[cfg(not(windows))]
    Err("not-supported".into())
}

/// 写回系统（前端已走显式确认）。返回写前的原值供前端展示回滚点。
#[tauri::command(async)]
pub fn mouse_params_write(
    speed: i32,
    double_click_ms: i32,
    wheel_lines: i32,
    swap_buttons: bool,
) -> Result<MouseParamsDto, String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_SETDOUBLECLICKTIME, SPI_SETMOUSE, SPI_SETMOUSEBUTTONSWAP, SPI_SETWHEELSCROLLLINES,
        };
        let before = read_params();
        let mut mouse = [10i32, 10i32, speed.clamp(1, 20)];
        let mut ok = unsafe {
            windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                SPI_SETMOUSE,
                0,
                Some(mouse.as_mut_ptr() as *mut core::ffi::c_void),
                windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0x2),
            )
            .is_ok()
        };
        ok = spi_set(SPI_SETDOUBLECLICKTIME.0, double_click_ms.clamp(200, 900) as u32) && ok;
        ok = spi_set(SPI_SETWHEELSCROLLLINES.0, wheel_lines.clamp(1, 10) as u32) && ok;
        ok = spi_set(SPI_SETMOUSEBUTTONSWAP.0, if swap_buttons { 1 } else { 0 }) && ok;
        if !ok {
            // 部分失败 → 立即整体回滚到 before，不留半套状态
            let _ = restore_params(&before);
            return Err("spi-write-failed".into());
        }
        *BACKUP.lock().map_err(|e| e.to_string())? = Some(before);
        Ok(before)
    }
    #[cfg(not(windows))]
    {
        let _ = (speed, double_click_ms, wheel_lines, swap_buttons);
        Err("not-supported".into())
    }
}

#[cfg(windows)]
fn restore_params(p: &MouseParamsDto) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_SETDOUBLECLICKTIME, SPI_SETMOUSE, SPI_SETMOUSEBUTTONSWAP, SPI_SETWHEELSCROLLLINES,
    };
    let mut mouse = [10i32, 10i32, p.speed.clamp(1, 20)];
    let mut ok = unsafe {
        windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            SPI_SETMOUSE,
            0,
            Some(mouse.as_mut_ptr() as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0x2),
        )
        .is_ok()
    };
    ok = spi_set(SPI_SETDOUBLECLICKTIME.0, p.double_click_ms) && ok;
    ok = spi_set(SPI_SETWHEELSCROLLLINES.0, p.wheel_lines.clamp(0, i32::MAX) as u32) && ok;
    ok = spi_set(SPI_SETMOUSEBUTTONSWAP.0, if p.swap_buttons { 1 } else { 0 }) && ok;
    ok
}

/// 一键回滚到最近一次写回前的系统原值；无备份返回 false。
#[tauri::command(async)]
pub fn mouse_params_rollback() -> Result<bool, String> {
    let mut guard = BACKUP.lock().map_err(|e| e.to_string())?;
    match guard.take() {
        Some(before) => {
            #[cfg(windows)]
            return Ok(restore_params(&before));
            #[cfg(not(windows))]
            {
                let _ = before;
                Err("not-supported".into())
            }
        }
        None => Ok(false),
    }
}

/// V-69 精确模式：按住修饰键期间把指针速度按比例降到原档的 ratio（0.2..0.6）。
#[tauri::command(async)]
pub fn pointer_speed_temp(ratio: f64) -> Result<(), String> {
    #[cfg(windows)]
    {
        let cur = read_params().speed;
        let mut guard = TEMP_SPEED_BACKUP.lock().map_err(|e| e.to_string())?;
        if guard.is_none() {
            *guard = Some(cur);
        }
        let target = ((cur as f64) * ratio.clamp(0.2, 0.6)).round().max(1.0) as i32;
        if !spi_set_speed(target) {
            *guard = None;
            return Err("spi-write-failed".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = ratio;
        Err("not-supported".into())
    }
}

/// V-69 精确模式：松开修饰键 / 运行时卸载时还原原速度档。
#[tauri::command(async)]
pub fn pointer_speed_restore() -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut guard = TEMP_SPEED_BACKUP.lock().map_err(|e| e.to_string())?;
        if let Some(v) = guard.take() {
            if !spi_set_speed(v) {
                return Err("spi-write-failed".into());
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    Err("not-supported".into())
}

/// 进程退出兜底：恢复精确模式降速（若有）。在 lib.rs 退出路径调用。
pub fn restore_on_exit() {
    #[cfg(windows)]
    {
        if let Ok(mut guard) = TEMP_SPEED_BACKUP.lock() {
            if let Some(v) = guard.take() {
                let _ = spi_set_speed(v);
            }
        }
    }
}
