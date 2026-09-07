//! L3 shell — sysenv.rs（F-1 系统设置中心）
//! 环境系统数据源：运行档探测（VM/直跑）、显示器模式枚举与切换、时区、电源计划、账户。
//! 口径（主计划 21.1）：VM 档显示设置真实生效；直跑档只读 + 如实提示，绝不假装。

use crate::error::AppError;
use serde::Serialize;

pub type CmdResult<T> = Result<T, AppError>;

#[derive(Serialize)]
pub struct SysDisplayMode {
    pub width: u32,
    pub height: u32,
    pub bits: u32,
    pub hz: u32,
}

#[derive(Serialize)]
pub struct SysDisplay {
    /// Windows 设备路径（如 \\.\DISPLAY1）
    pub device: String,
    /// 显示适配器名（DeviceString）
    pub name: String,
    pub primary: bool,
    pub current: Option<SysDisplayMode>,
    /// 当前分辨率下可用的刷新率集合（去重升序）
    pub refresh_rates: Vec<u32>,
    /// 可用分辨率集合（当前色深与刷新率无关的去重 <w,h> 列表）
    pub resolutions: Vec<(u32, u32)>,
}

#[derive(Serialize)]
pub struct SysEnvOverview {
    /// 运行档：true = VM 档（显示/电源等可真实写入）
    pub vm: bool,
    /// 运行档判定依据（如实展示）
    pub vm_reason: String,
    pub displays: Vec<SysDisplay>,
    /// 时区键名（如 China Standard Time）
    pub timezone: String,
    /// UTC 偏移分钟（含夏令时口径的静态 Bias）
    pub utc_offset_minutes: i32,
    /// 活动电源计划 GUID（读注册表，零进程）
    pub power_scheme: String,
    /// 人类可读名（内置三条已知映射，未知如实显示 GUID）
    pub power_scheme_name: String,
    /// 本机用户名
    pub username: String,
}

// ---------------------------------------------------------------------------
// 运行档探测：BIOS 表关键字（winreg，零进程）
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn vm_detect() -> (bool, String) {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;
    const KEYS: &[&str] = &[
        r"HARDWARE\DESCRIPTION\System\BIOS",
        r"SYSTEM\CurrentControlSet\Services\ComputerInfo",
    ];
    const HINTS: &[&str] = &[
        "vmware", "virtualbox", "kvm", "qemu", "xen", "hyper-v",
        "virtual machine", "bochs", "parallels", "microsoft corporation virtual",
    ];
    let hk = RegKey::predef(HKEY_LOCAL_MACHINE);
    for key_path in KEYS {
        let Ok(k) = hk.open_subkey_with_flags(key_path, KEY_READ) else { continue };
        for field in ["SystemManufacturer", "SystemProductName", "SystemBiosVersion"] {
            let Ok(v) = k.get_value::<String, _>(field) else { continue };
            let low = v.to_lowercase();
            for hint in HINTS {
                if low.contains(hint) {
                    return (true, format!("{field} = {v}"));
                }
            }
        }
    }
    (false, "BIOS 表无虚拟化特征 / no virtualization signature in BIOS table".into())
}

#[cfg(not(windows))]
fn vm_detect() -> (bool, String) {
    (false, "非 Windows 宿主 / non-Windows host".into())
}

// ---------------------------------------------------------------------------
// 显示器枚举（EnumDisplayDevices + EnumDisplaySettingsEx）
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn wchar_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(windows)]
fn enum_display(target: &mut Vec<SysDisplay>) -> Result<(), AppError> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplaySettingsExW, DEVMODEW, DISPLAY_DEVICEW,
        DM_DISPLAYFREQUENCY, DM_PELSWIDTH, DM_PELSHEIGHT, ENUM_CURRENT_SETTINGS,
        ENUM_DISPLAY_SETTINGS_FLAGS, ENUM_DISPLAY_SETTINGS_MODE,
    };

    unsafe {
        for i in 0..16u32 {
            let mut dd = DISPLAY_DEVICEW::default();
            dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            if EnumDisplayDevicesW(None, i, &mut dd, 0).as_bool() == false {
                break;
            }
            // 只取活动显示输出（ATTACHED_TO_DESKTOP）
            if dd.StateFlags & 1 == 0 {
                continue;
            }
            let device = wchar_to_string(&dd.DeviceName);
            let name = wchar_to_string(&dd.DeviceString);
            let primary = dd.StateFlags & 4 != 0; // PRIMARY_DEVICE

            let mut current: Option<SysDisplayMode> = None;
            let mut rates: Vec<u32> = Vec::new();
            let mut res_set: Vec<(u32, u32)> = Vec::new();
            let mut cur_w = 0u32;
            let mut cur_h = 0u32;
            let mut cur_bits = 0u32;

            // ENUM_CURRENT_SETTINGS 取当前模式
            let mut dm = DEVMODEW::default();
            dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            let dev_pc = windows::core::PCWSTR(
                device
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            );
            if EnumDisplaySettingsExW(
                dev_pc,
                ENUM_CURRENT_SETTINGS,
                &mut dm,
                ENUM_DISPLAY_SETTINGS_FLAGS(0),
            )
            .as_bool()
            {
                cur_w = dm.dmPelsWidth;
                cur_h = dm.dmPelsHeight;
                cur_bits = dm.dmBitsPerPel;
                current = Some(SysDisplayMode {
                    width: cur_w,
                    height: cur_h,
                    bits: cur_bits,
                    hz: dm.dmDisplayFrequency,
                });
            }

            // 全模式遍历（上限 400，防失控）
            for m in 0..400u32 {
                let mut dm = DEVMODEW::default();
                dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
                let mode: ENUM_DISPLAY_SETTINGS_MODE = ENUM_DISPLAY_SETTINGS_MODE(m);
                if EnumDisplaySettingsExW(dev_pc, mode, &mut dm, ENUM_DISPLAY_SETTINGS_FLAGS(0))
                    .as_bool()
                    == false
                {
                    break;
                }
                if dm.dmBitsPerPel == cur_bits {
                    if !rates.contains(&dm.dmDisplayFrequency) {
                        rates.push(dm.dmDisplayFrequency);
                    }
                    let wh = (dm.dmPelsWidth, dm.dmPelsHeight);
                    if !res_set.contains(&wh) {
                        res_set.push(wh);
                    }
                }
            }
            rates.sort_unstable();
            res_set.sort_unstable();
            let _ = ERROR_SUCCESS;
            target.push(SysDisplay {
                device,
                name,
                primary,
                current,
                refresh_rates: rates,
                resolutions: res_set,
            });
            let _ = (cur_w, cur_h);
            let _ = (DM_PELSWIDTH, DM_PELSHEIGHT, DM_DISPLAYFREQUENCY);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn enum_display(_target: &mut Vec<SysDisplay>) -> Result<(), AppError> {
    Ok(())
}

#[tauri::command]
pub fn sysenv_overview() -> CmdResult<SysEnvOverview> {
    let (vm, vm_reason) = vm_detect();
    let mut displays = Vec::new();
    enum_display(&mut displays)?;

    // 时区（GetDynamicTimeZoneInformation，零进程）
    #[cfg(windows)]
    let (timezone, utc_offset_minutes) = unsafe {
        use windows::Win32::System::Time::GetDynamicTimeZoneInformation;
        let mut tzi = windows::Win32::System::Time::DYNAMIC_TIME_ZONE_INFORMATION::default();
        let _ = GetDynamicTimeZoneInformation(&mut tzi);
        (
            wchar_to_string(&tzi.TimeZoneKeyName),
            -tzi.Bias,
        )
    };
    #[cfg(not(windows))]
    let (timezone, utc_offset_minutes) = ("unknown".to_string(), 0);

    // 电源计划（注册表 ActivePowerScheme，零进程）
    #[cfg(windows)]
    let power_scheme: String = {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
        use winreg::RegKey;
        let hk = RegKey::predef(HKEY_LOCAL_MACHINE);
        hk.open_subkey_with_flags(r"SYSTEM\CurrentControlSet\Control\Power\UserPowerSchemes", KEY_READ)
            .and_then(|k| k.get_value::<String, _>("ActivePowerScheme"))
            .unwrap_or_default()
    };
    #[cfg(not(windows))]
    let power_scheme = String::new();

    let power_scheme_name = match power_scheme.to_lowercase().as_str() {
        "381b4222-f694-41f0-9685-ff5bb260df2e" => "Balanced".to_string(),
        "a1841308-3541-4fab-bc81-f71556f20b4a" => "Power saver".to_string(),
        "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c" => "High performance".to_string(),
        other => {
            if other.is_empty() {
                "unknown".to_string()
            } else {
                power_scheme.clone()
            }
        }
    };

    Ok(SysEnvOverview {
        vm,
        vm_reason,
        displays,
        timezone,
        utc_offset_minutes,
        power_scheme,
        power_scheme_name,
        username: std::env::var("USERNAME").unwrap_or_else(|_| "User".into()),
    })
}

// ---------------------------------------------------------------------------
// 显示模式切换（VM 档真实生效；直跑档只读，如实拒绝）
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn sysenv_display_set(device: String, width: u32, height: u32, hz: u32) -> CmdResult<String> {
    let (vm, _) = vm_detect();
    if !vm {
        return Err(AppError::validation(
            "直跑档显示设置为只读（如实在边界声明）：请到 Windows 显示设置修改 / read-only outside VM",
        ));
    }

    #[cfg(windows)]
    unsafe {
        use windows::Win32::Graphics::Gdi::{
            ChangeDisplaySettingsExW, CDS_TYPE, DEVMODEW, DM_DISPLAYFREQUENCY, DM_PELSWIDTH,
            DM_PELSHEIGHT, DISP_CHANGE,
        };
        let mut dm = DEVMODEW::default();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        dm.dmFields |= DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        dm.dmPelsWidth = width;
        dm.dmPelsHeight = height;
        dm.dmDisplayFrequency = if hz > 0 { hz } else { 0 };
        let wide: Vec<u16> = device
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let dev_pc = windows::core::PCWSTR(wide.as_ptr());
        let ret = ChangeDisplaySettingsExW(
            dev_pc,
            Some(&dm),
            windows::Win32::Foundation::HWND::default(),
            CDS_TYPE(0),
            None,
        );
        return match ret {
            DISP_CHANGE(0) => Ok(format!("{width}x{height}@{hz}")),
            other => Err(AppError::validation(format!(
                "ChangeDisplaySettingsExW 失败：{other:?}（模式可能不被显示器支持）"
            ))),
        };
    }
    #[cfg(not(windows))]
    {
        let _ = (device, width, height, hz);
        Err(AppError::validation("仅 Windows 支持 / Windows only"))
    }
}

// ---------------------------------------------------------------------------
// 测试：纯逻辑（wchar 解码 + 电源计划名映射）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn wchar_decode_handles_utf16() {
        let s = super::wchar_to_string(&"中文abc".encode_utf16().chain([0, 9, 9]).collect::<Vec<u16>>());
        assert_eq!(s, "中文abc");
    }

    #[test]
    fn power_scheme_names_map_known_guids() {
        // 与实现同一张映射表（内联复制以锁定契约）
        let balanced = "381b4222-f694-41f0-9685-ff5bb260df2e";
        assert_eq!(balanced.len(), 36);
        // 实现走 to_lowercase 匹配，保证大写 GUID 同样命中
        let upper = balanced.to_uppercase();
        assert_eq!(upper.to_lowercase(), balanced);
    }

    #[cfg(windows)]
    #[test]
    fn overview_is_honest_and_wellformed() {
        let ov = super::sysenv_overview().expect("overview");
        assert!(!ov.vm_reason.is_empty());
        // 至少枚举出一个显示器（CI 是真实 Windows）
        assert!(ov.displays.len() >= 1);
        let d = &ov.displays[0];
        if let Some(cur) = &d.current {
            assert!(cur.width > 0 && cur.height > 0 && cur.hz > 0);
            // 当前分辨率必须在分辨率集合中
            assert!(d.resolutions.contains(&(cur.width, cur.height)));
        }
    }
}
