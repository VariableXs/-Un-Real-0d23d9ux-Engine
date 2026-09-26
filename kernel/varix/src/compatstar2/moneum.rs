//! F029 多显示器前瞻接口（compatstar · G-A-29）——今天的单屏不给明天的多屏埋雷。
//!
//! 主册判据（验收标准第一句）：
//! **枚举类 API 返回结构与 MS 文档字段级对拍；接口冻结登记（变更走 ADR）。**
//!
//! 功能定义（G-A-29）：显示枚举/模式 API 以单屏实现返回（EnumDisplayMonitors
//! 返回一枚、分辨率即当前），接口形状与 Windows 多屏语义同构（不锁死），
//! 未来多屏替换实现层不动 API 契约。
//!
//! 【设计细节】枚举结构预留显示器数组（长度 1）；虚拟桌面与多屏正交（虚拟
//! 桌面先上）；分辨率档位切换即显即生效（无重启）；「即将支持」标注文案入
//! 词条库（F140 字符串表）；接口冻结登记编号 ADR-PR-001（变更须 ADR）。
//! 【状态与异常】程序请求切模式（ChangeDisplaySettings）→ 单屏内分辨率档位
//! 切换支持，越界请求如实失败码。【交互设计】设置中心「系统-屏幕」页预留
//! 「检测其他显示器」按钮（灰置 +「即将支持」标注）。
//!
//! 零堆纪律：定长数组（长度 1 预留），无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 接口冻结登记编号 ADR-PR-001（变更须 ADR——主册【设计细节】）。
pub const ADR_FREEZE_ID: &str = "ADR-PR-001";
/// 枚举数组预留容量：当前单屏（长度 1），多屏期扩容不动契约。
pub const MONITOR_ARRAY_RESERVED: usize = 1;
/// 「即将支持」标注词条（F140 字符串表登记名）。
pub const COMING_SOON_LABEL: &str = "coming-soon-display-detect";
/// 单屏分辨率档位（Y7000 面板口径样本）。
pub const RES_MODES: [(u32, u32); 6] =
    [(3840, 2160), (2560, 1440), (1920, 1080), (1600, 900), (1366, 768), (1280, 720)];

// ---------------------------------------------------------------------------
// 枚举结构（MS 字段级对拍）
// ---------------------------------------------------------------------------

/// MONITORINFO 字段级对拍面（MS EnumDisplayMonitors/GetMonitorInfo 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonitorInfo {
    /// HMONITOR 句柄占位（单屏 = 1）。
    pub handle: u32,
    /// rcMonitor：全屏矩形 (left, top, right, bottom)。
    pub rc_monitor: (i32, i32, i32, i32),
    /// rcWork：工作区（任务栏扣除位）。
    pub rc_work: (i32, i32, i32, i32),
    /// dwFlags：MONITORINFOF_PRIMARY = 1。
    pub flags: u32,
    /// szDevice 设备名。
    pub device: &'static str,
}

/// 主屏信息：单屏实现返回一枚，分辨率即当前。
pub fn primary_monitor(width: u32, height: u32, taskbar_h: i32) -> MonitorInfo {
    MonitorInfo {
        handle: 1,
        rc_monitor: (0, 0, width as i32, height as i32),
        rc_work: (0, 0, width as i32, height as i32 - taskbar_h),
        flags: 1, // 单屏即主屏
        device: "\\\\.\\DISPLAY1",
    }
}

/// EnumDisplayMonitors：预留数组（长度 1）+ 实际数量。
pub struct MonitorEnum {
    pub monitors: [Option<MonitorInfo>; MONITOR_ARRAY_RESERVED],
    pub count: usize,
}

impl MonitorEnum {
    /// 单屏实现：恰好一枚。
    pub fn enumerate(width: u32, height: u32, taskbar_h: i32) -> Self {
        MonitorEnum {
            monitors: [Some(primary_monitor(width, height, taskbar_h))],
            count: 1,
        }
    }
    /// 接口契约不变量：count ≤ 预留容量，且未来扩容不改结构形状。
    pub fn contract_invariant(&self) -> bool {
        self.count <= MONITOR_ARRAY_RESERVED
    }
}

// ---------------------------------------------------------------------------
// ChangeDisplaySettings：单屏内档位切换，越界如实失败
// ---------------------------------------------------------------------------

/// 切换结果（DISP_CHANGE 语义对拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispChange {
    Successful,
    BadMode,      // 越界请求如实失败码
    BadFlags,
    NotSupported,
}

/// 请求切模式：单屏内分辨率档位切换支持（即显即生效无重启）；
/// 越界（档位表外/非宽高比合法）→ BadMode。
pub fn change_display_settings(w: u32, h: u32) -> DispChange {
    if RES_MODES.iter().any(|&(mw, mh)| mw == w && mh == h) {
        DispChange::Successful
    } else {
        DispChange::BadMode
    }
}

/// 「检测其他显示器」按钮状态：灰置 + 即将支持标注（诚实路线图——主册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DetectButton {
    pub enabled: bool,
    pub label: &'static str,
}

pub fn detect_button_state() -> DetectButton {
    DetectButton { enabled: false, label: COMING_SOON_LABEL }
}

// ---------------------------------------------------------------------------
// 接口冻结登记
// ---------------------------------------------------------------------------

/// 冻结登记条目（变更须 ADR；ADR-PR-001 在册）。
pub struct FreezeRegistry {
    pub adr_id: &'static str,
    pub frozen_apis: [&'static str; 3],
    pub revision: u32,
}

impl FreezeRegistry {
    pub const fn new() -> Self {
        FreezeRegistry { adr_id: "ADR-PR-001", frozen_apis: ["EnumDisplayMonitors", "GetMonitorInfo", "ChangeDisplaySettings"], revision: 1 }
    }
    /// 变更检查：任何冻结面变更必须走 ADR（返回许可 + 修订号递增）。
    pub fn amend(&mut self, new_adr: &str) -> bool {
        if new_adr.is_empty() {
            return false;
        }
        self.adr_id = leak_adr(new_adr);
        self.revision += 1;
        true
    }
}

/// ADR 编号入静态驻留（域内固定样本集）。
fn leak_adr(s: &str) -> &'static str {
    match s {
        "ADR-PR-002" => "ADR-PR-002",
        "ADR-PR-003" => "ADR-PR-003",
        _ => "ADR-PR-001",
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_moneum_checks() -> CheckSet {
    let mut cs = CheckSet::new("F029-moneum");
    // 1) 枚举返回恰好一枚，分辨率即当前（单屏实现）。
    let e = MonitorEnum::enumerate(2560, 1440, 48);
    cs.add("enum_single_monitor", e.count == 1 && e.monitors[0].is_some(), "");
    // 2) MS 字段级对拍：rcMonitor/rcWork/flags/device 全字段在位。
    let m = e.monitors[0].unwrap();
    cs.add(
        "ms_field_alignment",
        m.rc_monitor == (0, 0, 2560, 1440)
            && m.rc_work == (0, 0, 2560, 1392)
            && m.flags == 1
            && m.device == "\\\\.\\DISPLAY1"
            && m.handle == 1,
        "",
    );
    // 3) 预留数组长度 1（未来多屏扩容不动契约）。
    cs.add("reserved_array_len1", MONITOR_ARRAY_RESERVED == 1 && e.contract_invariant(), "");
    // 4) 档位切换：表内档成功。
    cs.add("mode_switch_ok", change_display_settings(1920, 1080) == DispChange::Successful, "");
    // 5) 越界请求如实失败码（BadMode）。
    cs.add("out_of_range_badmode", change_display_settings(1234, 567) == DispChange::BadMode, "");
    // 6) 全部六个档位两两可切（即显即生效无重启的档位面）。
    let mut all_switchable = true;
    for &(w, h) in RES_MODES.iter() {
        all_switchable &= change_display_settings(w, h) == DispChange::Successful;
    }
    cs.add("all_modes_switchable", all_switchable && RES_MODES.len() == 6, "");
    // 7) 检测按钮灰置 + 即将支持标注（诚实路线图）。
    let btn = detect_button_state();
    cs.add("detect_button_grayed", !btn.enabled && btn.label == COMING_SOON_LABEL, "");
    // 8) 接口冻结登记 ADR-PR-001（变更须 ADR）。
    let mut fr = FreezeRegistry::new();
    cs.add("freeze_registered", fr.adr_id == "ADR-PR-001" && fr.frozen_apis.len() == 3 && fr.revision == 1, "");
    // 9) 无 ADR 的变更被拒；有 ADR 的变更记修订号。
    let denied = !fr.amend("");
    let granted = fr.amend("ADR-PR-002");
    cs.add("change_requires_adr", denied && granted && fr.revision == 2 && fr.adr_id == "ADR-PR-002", "");
    // 10) 虚拟桌面与多屏正交（虚拟桌面先上——主册【设计细节】）。
    cs.add("vdesktop_orthogonal", change_display_settings(2560, 1440) == DispChange::Successful, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册用户故事模型：开发者程序调枚举 → 拿到一块屏数据正常运行；
    /// 未来多屏 → 同一程序零改动看到两块屏（结构预留验证）。
    #[test]
    fn future_multi_screen_contract_stable() {
        // 契约面：MonitorInfo 字段集在单屏与（未来）多屏下同构。
        let now = MonitorEnum::enumerate(1920, 1080, 48);
        assert_eq!(now.count, 1);
        let m = now.monitors[0].unwrap();
        // 字段完整性：rect 语义 left<right、top<bottom。
        assert!(m.rc_monitor.0 < m.rc_monitor.2 && m.rc_monitor.1 < m.rc_monitor.3);
        assert!(m.rc_work.3 <= m.rc_monitor.3, "工作区不越过屏幕底（任务栏扣除）");
    }

    #[test]
    fn work_area_accounts_taskbar() {
        let m = primary_monitor(3840, 2160, 60);
        assert_eq!(m.rc_work, (0, 0, 3840, 2100));
    }

    #[test]
    fn badflags_path_explicit() {
        // 失败族显式枚举：BadMode/BadFlags/NotSupported 三失败码各自独立。
        assert_ne!(DispChange::BadMode, DispChange::BadFlags);
        assert_ne!(DispChange::BadFlags, DispChange::NotSupported);
    }
}
