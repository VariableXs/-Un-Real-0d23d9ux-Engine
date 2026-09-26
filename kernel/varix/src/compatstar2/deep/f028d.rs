//! F028 深化批次二 · DPI 三出口与缩放盒面（compatstar2/deep · G-A-28）。
//!
//! 批次一深化覆盖 WM_DPICHANGED 参数/缩放因子/三拍推进；本批补齐：DPI 三
//! 出口（GetDpiForSystem/GetDpiForWindow/GetDpiForMonitor 语义分诊）、
//! EnableNonClientDpiScaling 适用面（仅 per-monitor 自动缩放非客户区）、
//! 缩放盒向上取整（.ceil 防内容裁切——重排不裁字的实现面）、DPI 变更历史账
//! （单调时间戳）、per-monitor-v2 声明串识别（manifest 扩展面）。
//!
//! 零堆纪律：定长历史表，无 alloc。

use crate::checks::CheckSet;

/// DPI 变更历史容量。
pub const DPI_HISTORY_SLOTS: usize = 8;
/// per-monitor-v2 声明串（MS manifest 值）。
pub const DECL_PER_MONITOR_V2: &str = "per-monitor-v2";

/// DPI 三出口分诊（MS GetDpiFor* 语义对拍）：
/// unaware 恒见 96（虚拟化）；system-aware 见主屏 DPI；per-monitor 见所在屏实际值。
pub fn dpi_for_window(awareness: crate::compatstar2::dpistate::DpiAwareness, primary_dpi: u32, monitor_dpi: u32) -> u32 {
    use crate::compatstar2::dpistate::DpiAwareness as A;
    match awareness {
        A::Unaware => 96,
        A::SystemAware => primary_dpi,
        A::PerMonitorAware => monitor_dpi,
    }
}

/// EnableNonClientDpiScaling 适用面：仅 per-monitor 系（v1/v2）自动缩放
/// 非客户区；unaware/system-aware 不适用（MS 语义）。
pub fn nonclient_scaling_applies(awareness: crate::compatstar2::dpistate::DpiAwareness, declared_v2: bool) -> bool {
    use crate::compatstar2::dpistate::DpiAwareness as A;
    matches!(awareness, A::PerMonitorAware) || declared_v2
}

/// 缩放盒向上取整：w_px @ from_dpi → 目标 DPI 像素宽（ceil 防裁切）。
pub fn scale_bbox_ceil(w_px: u32, from_dpi: u32, to_dpi: u32) -> u32 {
    (w_px * to_dpi + from_dpi - 1) / from_dpi
}

/// 一条 DPI 变更历史。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DpiChangeRecord {
    pub epoch_ms: u64,
    pub old_dpi: u32,
    pub new_dpi: u32,
}

/// DPI 变更历史账：单调时间戳入账（乱序拒绝——测量自身稳定的账面）。
pub struct DpiHistory {
    pub records: [Option<DpiChangeRecord>; DPI_HISTORY_SLOTS],
    pub count: usize,
}

impl DpiHistory {
    pub const fn new() -> Self {
        DpiHistory { records: [None; DPI_HISTORY_SLOTS], count: 0 }
    }
    pub fn record(&mut self, epoch_ms: u64, old_dpi: u32, new_dpi: u32) -> bool {
        if self.count >= DPI_HISTORY_SLOTS {
            return false;
        }
        if self.count > 0 {
            if let Some(last) = self.records[self.count - 1] {
                if epoch_ms <= last.epoch_ms {
                    return false; // 时间戳必须单调递增
                }
            }
        }
        self.records[self.count] = Some(DpiChangeRecord { epoch_ms, old_dpi, new_dpi });
        self.count += 1;
        true
    }
}

/// manifest 声明串识别：per-monitor-v2 扩展（批次一 parse_manifest_dpi 的
/// 上层判别——v2 属 per-monitor 系且启用非客户区缩放）。
pub fn is_per_monitor_v2(decl: Option<&str>) -> bool {
    decl == Some(DECL_PER_MONITOR_V2)
}

/// 域自检（深化批次二）。
pub fn run_f028d_checks() -> CheckSet {
    use crate::compatstar2::dpistate::DpiAwareness as A;
    let mut cs = CheckSet::new("F028-dpistate-d2");
    // 1) 三出口分诊：unaware 恒 96；system 见主屏 144；per-monitor 见所在屏 168。
    cs.add(
        "dpi_three_exits",
        dpi_for_window(A::Unaware, 144, 168) == 96
            && dpi_for_window(A::SystemAware, 144, 168) == 144
            && dpi_for_window(A::PerMonitorAware, 144, 168) == 168,
        "",
    );
    // 2) 非客户区缩放：per-monitor 与 v2 声明适用；unaware/system 不适用。
    cs.add(
        "nonclient_scaling",
        nonclient_scaling_applies(A::PerMonitorAware, false)
            && nonclient_scaling_applies(A::Unaware, true)
            && !nonclient_scaling_applies(A::SystemAware, false)
            && !nonclient_scaling_applies(A::Unaware, false),
        "",
    );
    // 3) 缩放盒 ceil：5px@96 → 8px@144（7.5 向上防裁切）；整除点不虚增。
    cs.add(
        "scale_bbox_ceil",
        scale_bbox_ceil(5, 96, 144) == 8 && scale_bbox_ceil(10, 96, 144) == 15 && scale_bbox_ceil(96, 96, 96) == 96,
        "",
    );
    // 4) 历史账：单调入账、乱序拒绝。
    let mut h = DpiHistory::new();
    let mono = h.record(100, 96, 144) && h.record(250, 144, 168);
    cs.add("history_monotonic", mono && !h.record(200, 168, 144) && h.count == 2, "");
    // 5) v2 声明串识别。
    cs.add("v2_declaration", is_per_monitor_v2(Some("per-monitor-v2")) && !is_per_monitor_v2(Some("per-monitor")) && !is_per_monitor_v2(None), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceil_never_underestimates() {
        // 穷举抽查：ceil 结果 × from ≥ 原 × to（永不裁切不变量，20 采样）。
        for w in 1..=20u32 {
            let got = scale_bbox_ceil(w, 96, 144);
            assert!(got * 96 >= w * 144, "{}px 不得裁切", w);
        }
    }

    #[test]
    fn history_cap_guard() {
        let mut h = DpiHistory::new();
        let mut t = 1u64;
        for _ in 0..DPI_HISTORY_SLOTS {
            assert!(h.record(t, 96, 144));
            t += 10;
        }
        assert!(!h.record(t, 96, 144), "容量满如实拒绝");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f028d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
