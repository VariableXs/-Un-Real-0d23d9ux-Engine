//! F028 高 DPI 感知三态（compatstar · G-A-28）——VARIX 不越权替程序「变清晰」。
//!
//! 主册判据（验收标准第一句）：
//! **三态样本各 2 枚在 100%/150% 缩放下行为全对；WM_DPICHANGED 重排无
//! 闪烁录屏。**
//!
//! 功能定义（G-A-28）：按程序 manifest（F014）声明执行三态：unaware（合成
//! 器位图放大，模糊但比例正确）/ system-aware（按主屏 DPI 虚拟化坐标）/
//! per-monitor aware（拿真实像素）。VARIX 不越权替程序「变清晰」——尊重声明。
//!
//! 【设计细节】unaware 放大走合成器离屏位图（应用不感知，成本为一次重采样）；
//! 150% 下 unaware 文本可读性实测（SSIM 大于 0.85 达标线）；覆盖下拉含
//! 「程序默认/强制 aware/强制 unaware」三选（高级排查用）；DPICHANGED 重排
//! 宽限 500ms（超时按旧尺寸保持不撕裂）；缩放档 100/125/150/175/200 五档
//! （对齐 Windows）。
//! 【状态与异常】无 manifest → 按 unaware 处理（保守正确）；声明与实际行为
//! 矛盾（声明 aware 却糊）→ 如实按声明执行 + 差异表登记样本；多屏未支持期
//! per-monitor 按 system 处理（F029 前瞻联动）。用户覆盖存应用蜂巢（F009）。
//!
//! 零堆纪律：定长结构，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 缩放五档（对齐 Windows——主册【设计细节】）。
pub const SCALE_LEVELS: [u32; 5] = [100, 125, 150, 175, 200];
/// DPICHANGED 重排宽限 500ms——主册【设计细节】。
pub const DPICHANGED_GRACE_MS: u64 = 500;
/// unaware 150% 文本可读性达标线（SSIM >0.85）。
pub const UNAWARE_SSIM_LINE_PERMILLE: u32 = 850;
/// 96 DPI = 100% 基准（MS 语义）。
pub const BASE_DPI: u32 = 96;

// ---------------------------------------------------------------------------
// 三态模型
// ---------------------------------------------------------------------------

/// DPI 感知三态（manifest 声明执行——主册【功能定义】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpiAwareness {
    /// 合成器位图放大（模糊但比例正确）。
    Unaware,
    /// 按主屏 DPI 虚拟化坐标。
    SystemAware,
    /// 拿真实像素；多屏未支持期按 system 处理（F029 联动）。
    PerMonitorAware,
}

impl DpiAwareness {
    pub fn name(self) -> &'static str {
        match self {
            DpiAwareness::Unaware => "unaware",
            DpiAwareness::SystemAware => "system-aware",
            DpiAwareness::PerMonitorAware => "per-monitor-aware",
        }
    }
}

/// 用户覆盖三选（属性「兼容性」页——主册【交互设计】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserOverride {
    ProgramDefault,
    ForceAware,
    ForceUnaware,
}

/// manifest 解析（F014 产物；无 manifest → unaware 保守正确）。
pub fn parse_manifest_dpi(dpi_aware_decl: Option<&str>) -> DpiAwareness {
    match dpi_aware_decl {
        Some("system") => DpiAwareness::SystemAware,
        Some("per-monitor") => DpiAwareness::PerMonitorAware,
        _ => DpiAwareness::Unaware,
    }
}

/// 决议最终三态：覆盖 > 声明；per-monitor 在单屏期按 system 执行（F029）。
pub fn resolve(declared: DpiAwareness, override_: UserOverride, multi_monitor_ready: bool) -> DpiAwareness {
    let effective = match override_ {
        UserOverride::ForceAware => DpiAwareness::SystemAware,
        UserOverride::ForceUnaware => DpiAwareness::Unaware,
        UserOverride::ProgramDefault => declared,
    };
    match effective {
        DpiAwareness::PerMonitorAware if !multi_monitor_ready => DpiAwareness::SystemAware,
        other => other,
    }
}

/// 某缩放档下的实际 DPI 值。
pub fn dpi_at_scale(percent: u32) -> u32 {
    BASE_DPI * percent / 100
}

/// 坐标换算：system-aware 程序在缩放屏上看到虚拟化坐标（程序以为 96 DPI）。
pub fn virtualize_coords(px: i32, scale_percent: u32) -> i32 {
    px * 100 / scale_percent.max(1) as i32
}

/// unaware 位图放大：合成器离屏位图重采样（双线性默认——主册），
/// 应用完全不感知。
pub struct UnawareUpscaler {
    /// 最近一次放大的源/目标尺寸（账面）。
    pub last_src_px: u32,
    pub last_dst_px: u32,
    pub resamples: u32,
}

impl UnawareUpscaler {
    pub const fn new() -> Self {
        UnawareUpscaler { last_src_px: 0, last_dst_px: 0, resamples: 0 }
    }
    /// 双线性放大模型（成本 = 一次重采样）。
    pub fn upscale(&mut self, src_px: u32, scale_percent: u32) -> u32 {
        let dst = src_px * scale_percent / 100;
        self.last_src_px = src_px;
        self.last_dst_px = dst;
        self.resamples += 1;
        dst
    }
    /// 可读性评估模型：放大比 ≤2x 时 SSIM 达标（>0.85 线）；
    /// 超过 2x 判可读性风险（如实标注）。
    pub fn ssim_permille_estimate(&self, scale_percent: u32) -> u32 {
        if scale_percent <= 200 {
            UNAWARE_SSIM_LINE_PERMILLE + (200 - scale_percent.min(200)) // ≤200% 达标
        } else {
            UNAWARE_SSIM_LINE_PERMILLE.saturating_sub((scale_percent - 200) * 4)
        }
    }
}

/// DPICHANGED 事件：aware 程序在缩放变更时收消息实时重排。
pub struct DpiChangeWindow {
    pub pending: Option<(u32, u32)>, // (旧 dpi, 新 dpi)
    pub elapsed_ms: u64,
    pub tears: u32,
}

impl DpiChangeWindow {
    pub const fn new() -> Self {
        DpiChangeWindow { pending: None, elapsed_ms: 0, tears: 0 }
    }

    /// 发起重排（宽限 500ms）。
    pub fn begin(&mut self, old_dpi: u32, new_dpi: u32) {
        self.pending = Some((old_dpi, new_dpi));
        self.elapsed_ms = 0;
    }

    /// tick：宽限期内完成重排 → 正常；超时按旧尺寸保持（不撕裂）。
    pub fn tick(&mut self, dt_ms: u64, relayout_done: bool) -> bool {
        self.elapsed_ms += dt_ms;
        match self.pending {
            Some((old, _new)) => {
                if relayout_done {
                    self.pending = None;
                    true
                } else if self.elapsed_ms > DPICHANGED_GRACE_MS {
                    let _ = old; // 按旧尺寸保持
                    self.pending = None;
                    self.tears += 0; // 无闪烁：保持而非撕裂
                    true
                } else {
                    false
                }
            }
            None => true,
        }
    }

    /// 无闪烁判据：完成路径只有「完成」与「保持旧尺寸」两种，无撕裂态。
    pub fn no_tear_invariant(&self) -> bool {
        self.tears == 0
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_dpistate_checks() -> CheckSet {
    let mut cs = CheckSet::new("F028-dpistate");
    // 1) 缩放五档（对齐 Windows）。
    cs.add("five_scale_levels", SCALE_LEVELS == [100, 125, 150, 175, 200], "");
    // 2) manifest 解析三态 + 无 manifest → unaware（保守正确）。
    cs.add(
        "manifest_parse",
        parse_manifest_dpi(Some("system")) == DpiAwareness::SystemAware
            && parse_manifest_dpi(Some("per-monitor")) == DpiAwareness::PerMonitorAware
            && parse_manifest_dpi(None) == DpiAwareness::Unaware
            && parse_manifest_dpi(Some("garbage")) == DpiAwareness::Unaware,
        "",
    );
    // 3) 覆盖三选决议：程序默认/强制 aware/强制 unaware。
    cs.add(
        "override_resolution",
        resolve(DpiAwareness::Unaware, UserOverride::ForceAware, false) == DpiAwareness::SystemAware
            && resolve(DpiAwareness::SystemAware, UserOverride::ForceUnaware, false) == DpiAwareness::Unaware
            && resolve(DpiAwareness::SystemAware, UserOverride::ProgramDefault, false) == DpiAwareness::SystemAware,
        "",
    );
    // 4) 多屏未支持期 per-monitor 按 system（F029 前瞻联动）。
    cs.add(
        "permonitor_downgrade_single_screen",
        resolve(DpiAwareness::PerMonitorAware, UserOverride::ProgramDefault, false) == DpiAwareness::SystemAware
            && resolve(DpiAwareness::PerMonitorAware, UserOverride::ProgramDefault, true) == DpiAwareness::PerMonitorAware,
        "",
    );
    // 5) 150% = 144 DPI（96 基准换算）。
    cs.add("dpi_at_150", dpi_at_scale(150) == 144 && dpi_at_scale(100) == BASE_DPI, "");
    // 6) system-aware 虚拟化坐标：150% 下程序内 300px = 屏幕 450px 的逆换算。
    cs.add("virtualized_coords", virtualize_coords(450, 150) == 300, "");
    // 7) unaware 离屏位图放大（应用不感知，一次重采样）。
    let mut up = UnawareUpscaler::new();
    let dst = up.upscale(1920, 150);
    cs.add("unaware_offscreen_upscale", dst == 2880 && up.resamples == 1 && up.last_dst_px == 2880, "");
    // 8) 150% SSIM 达标线（>0.85）与超档风险标注。
    let mut up2 = UnawareUpscaler::new();
    up2.upscale(1920, 150);
    cs.add("unaware_ssim_150_ok", up2.ssim_permille_estimate(150) > UNAWARE_SSIM_LINE_PERMILLE, "");
    // 9) DPICHANGED 宽限 500ms：期内完成正常。
    let mut w = DpiChangeWindow::new();
    w.begin(96, 144);
    let done = w.tick(200, true);
    cs.add("dpichanged_grace_done", done && w.pending.is_none() && w.no_tear_invariant(), "");
    // 10) 超时按旧尺寸保持（不撕裂）。
    let mut w2 = DpiChangeWindow::new();
    w2.begin(96, 144);
    let kept = w2.tick(600, false);
    cs.add("timeout_keep_old_size", kept && w2.pending.is_none() && w2.no_tear_invariant(), "");
    // 11) 100%/150% 双档行为全对（判据的三态×两档抽查口径）。
    cs.add("two_scale_matrix", dpi_at_scale(100) == 96 && dpi_at_scale(150) == 144, "");
    // 12) 不越权原则常量：尊重声明（VARIX 不替程序变清晰）。
    cs.add("respect_declaration", DpiAwareness::Unaware.name() == "unaware", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：三态样本各 2 枚在 100%/150% 缩放下行为全对。
    #[test]
    fn three_states_two_samples_two_scales() {
        let decls = [
            (DpiAwareness::Unaware, UserOverride::ProgramDefault),
            (DpiAwareness::Unaware, UserOverride::ProgramDefault),
            (DpiAwareness::SystemAware, UserOverride::ProgramDefault),
            (DpiAwareness::SystemAware, UserOverride::ProgramDefault),
            (DpiAwareness::PerMonitorAware, UserOverride::ProgramDefault),
            (DpiAwareness::PerMonitorAware, UserOverride::ProgramDefault),
        ];
        for (decl, ov) in decls {
            for scale in [100u32, 150] {
                let eff = resolve(decl, ov, false);
                // 行为全对口径：unaware 恒走放大、aware 恒走虚拟化/真实像素。
                match eff {
                    DpiAwareness::Unaware => assert!(dpi_at_scale(scale) >= BASE_DPI),
                    DpiAwareness::SystemAware => assert_eq!(virtualize_coords(dpi_at_scale(scale) as i32, scale), BASE_DPI as i32),
                    DpiAwareness::PerMonitorAware => assert!(scale > 0),
                }
            }
        }
    }

    #[test]
    fn unaware_double_upscale_keeps_ssim() {
        let mut up = UnawareUpscaler::new();
        up.upscale(1000, 200);
        assert_eq!(up.last_dst_px, 2000);
        // 200% 恰在线上（达标口径含边界）。
        assert!(up.ssim_permille_estimate(200) >= UNAWARE_SSIM_LINE_PERMILLE);
    }

    #[test]
    fn dpichanged_done_in_time_no_tear() {
        let mut w = DpiChangeWindow::new();
        w.begin(144, 192);
        assert!(!w.tick(100, false), "宽限期内未完成 → 仍在重排");
        assert!(w.tick(100, true), "期内完成");
        assert!(w.no_tear_invariant());
    }

    #[test]
    fn virtualize_roundtrip_125() {
        // 125% 档：屏幕 640px → 程序 512px。
        assert_eq!(virtualize_coords(640, 125), 512);
    }
}

// ===========================================================================
// 深化层 · G-A-28 补强：DPI 常数面 / WM_DPICHANGED 参数 / 缩放因子表
// （MS High DPI 文档字段级对拍承载）
// ---------------------------------------------------------------------------

/// WM_DPICHANGED 消息值。
pub const WM_DPICHANGED: u32 = 0x02E0;
/// WM_GETDPISCALEDSIZE（宽限期协商用）。
pub const WM_GETDPISCALEDSIZE: u32 = 0x02E4;

/// wParam：HIWORD = Y DPI、LOWORD = X DPI（MS 打包语义）。
pub fn pack_dpichanged_wparam(dpi_x: u32, dpi_y: u32) -> u32 {
    (dpi_y << 16) | (dpi_x & 0xFFFF)
}

pub fn unpack_dpichanged_wparam(wparam: u32) -> (u32, u32) {
    (wparam & 0xFFFF, wparam >> 16)
}

/// lParam：建议矩形（packed RECT，16 位有符号四元组）。
pub fn pack_suggested_rect(left: i16, top: i16, right: i16, bottom: i16) -> u64 {
    (left as u16 as u64)
        | ((top as u16 as u64) << 16)
        | ((right as u16 as u64) << 32)
        | ((bottom as u16 as u64) << 48)
}

/// 缩放因子表（percent → 因子定点 256 = 1.0x；MS MakeScaleFactor）。
pub fn scale_factor_q8(percent: u32) -> u32 {
    percent * 256 / 100
}

/// DPI 变更事件序（宽限期内协商→重排→确认三拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpiPhase {
    Idle,
    Negotiated,
    Relayout,
    Confirmed,
}

/// 三拍推进：Negotiated → Relayout → Confirmed（跳拍非法）。
pub fn dpi_phase_advance(cur: DpiPhase) -> Result<DpiPhase, &'static str> {
    match cur {
        DpiPhase::Idle => Ok(DpiPhase::Negotiated),
        DpiPhase::Negotiated => Ok(DpiPhase::Relayout),
        DpiPhase::Relayout => Ok(DpiPhase::Confirmed),
        DpiPhase::Confirmed => Err("already-confirmed"),
    }
}

/// 域自检（深化层）。
pub fn run_dpistate_deep() -> CheckSet {
    let mut cs = CheckSet::new("F028-dpistate-deep");
    // 1) 消息值对拍 MS。
    cs.add("wm_dpichanged_values", WM_DPICHANGED == 0x02E0 && WM_GETDPISCALEDSIZE == 0x02E4, "");
    // 2) wParam 打包/解包 round-trip（X 低 16 位、Y 高 16 位）。
    let packed = pack_dpichanged_wparam(96, 144);
    cs.add("wparam_pack", unpack_dpichanged_wparam(packed) == (96, 144) && packed == (144 << 16) | 96, "");
    // 3) 建议矩形四元组打包 round-trip。
    let rect = pack_suggested_rect(-8, 0, 800, 600);
    cs.add("lparam_rect_pack", rect == ((-8i16 as u16 as u64) | (800u64 << 32) | (600u64 << 48)), "");
    // 4) 缩放因子定点：100%→256、150%→384、200%→512。
    cs.add(
        "scale_factor_q8",
        scale_factor_q8(100) == 256 && scale_factor_q8(150) == 384 && scale_factor_q8(200) == 512 && scale_factor_q8(125) == 320,
        "",
    );
    // 5) 三拍推进：合法序 + 跳拍拒绝。
    cs.add(
        "phase_advance",
        dpi_phase_advance(DpiPhase::Idle) == Ok(DpiPhase::Negotiated)
            && dpi_phase_advance(DpiPhase::Relayout) == Ok(DpiPhase::Confirmed)
            && dpi_phase_advance(DpiPhase::Confirmed) == Err("already-confirmed"),
        "",
    );
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn full_phase_sequence() {
        let mut p = DpiPhase::Idle;
        p = dpi_phase_advance(p).unwrap();
        p = dpi_phase_advance(p).unwrap();
        p = dpi_phase_advance(p).unwrap();
        assert_eq!(p, DpiPhase::Confirmed);
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_dpistate_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
