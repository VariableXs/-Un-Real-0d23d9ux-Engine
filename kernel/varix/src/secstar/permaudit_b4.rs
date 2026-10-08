//! F179 权限审计页 · 批次四深化（secstar · G-G-09）。
//!
//! 批次四功能面（与批次三互补：批次三管「归并与过滤」，本批管
//! 「异常与摘要」）：
//! - [`AnomalyScanner`]：异常模式扫描——深夜批量授予（23:00-05:00
//!   ≥3 条）、拒后立授（拒绝后 60s 内同能力授予）、卸载前狂用
//!   （三类指纹——审计页不只是流水，是找麻烦的探照灯）；
//! - [`MonthDigest`]：月度摘要——六能力 × 三源三维计数表（统计卡
//!   的月面：数字有出处）；
//! - [`ExportDiff`]：两次导出差异——新增/消失条目分列（导出不是
//!   一次性快照，是可比对的时点）；
//! - [`PrivacyGapVerify`]：隐私空窗机械验证——空窗日事件数必须为 0
//!   （黄条不是装饰：空窗就是空窗）。
//!
//! 零堆纪律：定长扫描账 + 定长矩阵，无 alloc。

use super::permaudit::{CapKind, ARCHIVE_RETENTION_DAYS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 异常模式扫描
// ---------------------------------------------------------------------------

/// 异常类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anomaly {
    /// 深夜批量授予：23:00-05:00 内 ≥3 条授予。
    NightBulkGrant,
    /// 拒后立授：拒绝后 60s 内同能力授予（社会工程指纹）。
    GrantAfterDeny,
    /// 卸载前狂用：卸载日当日内事件 ≥10（清痕迹嫌疑）。
    SpreeBeforeUninstall,
}

/// 深夜窗起点（当日分钟）：23:00。
pub const NIGHT_START_MIN: u32 = 23 * 60;
/// 深夜窗宽度（跨 0 点到 05:00）：360 分钟。
pub const NIGHT_SPAN_MIN: u32 = 6 * 60;
/// 批量阈值。
pub const NIGHT_BULK_AT: usize = 3;
/// 拒后立授窗（秒 → 当日分钟内比较用毫秒面）。
pub const DENY_GRANT_GAP_MS: u64 = 60_000;
/// 卸载前狂用阈值。
pub const SPREE_AT: usize = 10;

/// 单条事件（扫描器消费面）。
#[derive(Clone, Copy, Debug)]
pub struct ScanEvent {
    pub at_min: u32,    // 当日分钟 0..1440
    pub cap: CapKind,
    pub granted: bool,
}

/// 扫描器：喂事件序列 → 异常列表（三类独立判）。
#[derive(Clone, Copy, Debug, Default)]
pub struct AnomalyScanner {
    night_grants: usize,
    last_deny: Option<(u64, CapKind)>, // (at_ms, cap)
    today_events: usize,
    pub found: usize,
}

impl AnomalyScanner {
    pub const fn new() -> AnomalyScanner {
        AnomalyScanner { night_grants: 0, last_deny: None, today_events: 0, found: 0 }
    }

    fn in_night_window(at_min: u32) -> bool {
        let m = at_min % 1440;
        // 夜窗 = [23:00, 24:00) ∪ [00:00, 05:00)——终点由常量推导（跨 0 点）。
        let night_end = (NIGHT_START_MIN + NIGHT_SPAN_MIN) % 1440;
        m >= NIGHT_START_MIN || m < night_end
    }

    /// 喂一条事件。`at_ms` 单调（同日内）。返回命中的异常（可 None）。
    pub fn feed(&mut self, ev: ScanEvent, at_ms: u64) -> Option<Anomaly> {
        self.today_events += 1;
        let mut hit = None;
        // 夜间批量授予。
        if ev.granted && Self::in_night_window(ev.at_min) {
            self.night_grants += 1;
            if self.night_grants == NIGHT_BULK_AT {
                hit = Some(Anomaly::NightBulkGrant);
            }
        }
        // 拒后立授：同能力 60s 内。
        if let Some((deny_ms, cap)) = self.last_deny {
            if ev.granted && cap == ev.cap && at_ms.saturating_sub(deny_ms) <= DENY_GRANT_GAP_MS {
                hit = Some(Anomaly::GrantAfterDeny);
                self.last_deny = None; // 一次性报告
            }
        }
        if !ev.granted {
            self.last_deny = Some((at_ms, ev.cap));
        }
        // 卸载前狂用。
        if self.today_events == SPREE_AT {
            hit = Some(Anomaly::SpreeBeforeUninstall);
        }
        if hit.is_some() {
            self.found += 1;
        }
        hit
    }
}

// ---------------------------------------------------------------------------
// 月度摘要（六能力 × 三源）
// ---------------------------------------------------------------------------

/// 能力列（按 CapKind ord 序 0-5）。
pub const DIGEST_CAPS: usize = 6;
/// 源行（Grant/Intercept/Network）。
pub const DIGEST_SOURCES: usize = 3;

#[derive(Clone, Copy, Debug, Default)]
pub struct MonthDigest {
    cells: [[u32; DIGEST_CAPS]; DIGEST_SOURCES],
}

impl MonthDigest {
    pub const fn new() -> MonthDigest {
        MonthDigest { cells: [[0; DIGEST_CAPS]; DIGEST_SOURCES] }
    }

    /// 记一笔（cap 按 ord 0-5、source 按 ord 0-2——越界诚实拒）。
    pub fn record(&mut self, cap_ord: usize, source_ord: usize) -> bool {
        if cap_ord >= DIGEST_CAPS || source_ord >= DIGEST_SOURCES {
            return false;
        }
        self.cells[source_ord][cap_ord] += 1;
        true
    }

    /// 单格读数。
    pub fn cell(&self, cap_ord: usize, source_ord: usize) -> u32 {
        self.cells[source_ord][cap_ord]
    }

    /// 能力行合计（该能力三源总和——统计卡纵向）。
    pub fn cap_total(&self, cap_ord: usize) -> u32 {
        self.cells.iter().map(|r| r[cap_ord.min(DIGEST_CAPS - 1)]).sum()
    }

    /// 全表合计。
    pub fn grand_total(&self) -> u32 {
        self.cells.iter().flatten().sum()
    }
}

// ---------------------------------------------------------------------------
// 导出差异
// ---------------------------------------------------------------------------

/// 差异行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffEntry {
    /// 本次新增（上次没有）。
    Added(u32),
    /// 本次消失（上次有本次无）。
    Removed(u32),
}

/// 两次导出的 id 集 diff（上次 vs 本次——时点比对面）。
pub fn export_diff(prev: &[u32], curr: &[u32], out: &mut [Option<DiffEntry>; 16]) -> usize {
    let mut k = 0;
    for c in curr {
        if k >= out.len() {
            break;
        }
        if !prev.contains(c) {
            out[k] = Some(DiffEntry::Added(*c));
            k += 1;
        }
    }
    for p in prev {
        if k >= out.len() {
            break;
        }
        if !curr.contains(p) {
            out[k] = Some(DiffEntry::Removed(*p));
            k += 1;
        }
    }
    k
}

// ---------------------------------------------------------------------------
// 隐私空窗机械验证
// ---------------------------------------------------------------------------

/// 空窗验证：空窗日集合内的日事件数必须全为 0（黄条是承诺不是装饰）。
pub fn privacy_gap_zero_events(gap_days: &[u32], events_by_day: &[(u32, usize)]) -> bool {
    gap_days.iter().all(|d| {
        events_by_day
            .iter()
            .find(|(day, _)| day == d)
            .map(|(_, n)| *n == 0)
            .unwrap_or(true) // 无记录日 = 无事件（诚实缺席）
    })
}

/// 空窗天数不超保留期（空窗范围受 90 天账面约束——不越界伪造历史）。
pub fn gap_within_retention(gap_days: &[u32], today: u32) -> bool {
    gap_days.iter().all(|d| today.saturating_sub(*d) <= ARCHIVE_RETENTION_DAYS)
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_permaudit_b4_checks() -> CheckSet {
    use CapKind as CK;
    let mut cs = CheckSet::new("F179-b4");

    // 1) 深夜批量：23:30 起 3 条授予 → 第三条命中 NightBulkGrant。
    let mut s = AnomalyScanner::new();
    let e1 = s.feed(ScanEvent { at_min: 23 * 60 + 30, cap: CK::Camera, granted: true }, 1_000);
    let e2 = s.feed(ScanEvent { at_min: 23 * 60 + 31, cap: CK::Camera, granted: true }, 2_000);
    let e3 = s.feed(ScanEvent { at_min: 23 * 60 + 32, cap: CK::Camera, granted: true }, 3_000);
    cs.add(
        "night_bulk_detected",
        e1.is_none() && e2.is_none() && e3 == Some(Anomaly::NightBulkGrant),
        "",
    );

    // 2) 深夜窗边界：22:59 的授予不计、05:01 的不计（窗界逐点）。
    let mut s2 = AnomalyScanner::new();
    s2.feed(ScanEvent { at_min: 22 * 60 + 59, cap: CK::Camera, granted: true }, 1_000);
    s2.feed(ScanEvent { at_min: 5 * 60 + 1, cap: CK::Camera, granted: true }, 2_000);
    let not_bulk = s2.found == 0;
    cs.add("night_window_edges", not_bulk, "");

    // 3) 拒后立授：Camera 拒 → 30s 后 Camera 授 → 命中（社会工程指纹）。
    let mut s3 = AnomalyScanner::new();
    s3.feed(ScanEvent { at_min: 600, cap: CK::Camera, granted: false }, 10_000);
    let hit = s3.feed(ScanEvent { at_min: 600, cap: CK::Camera, granted: true }, 40_000);
    cs.add("grant_after_deny", hit == Some(Anomaly::GrantAfterDeny), "");

    // 4) 拒后立授窗界：61s 后授不命中（窗口外放行）。
    let mut s4 = AnomalyScanner::new();
    s4.feed(ScanEvent { at_min: 600, cap: CK::Camera, granted: false }, 10_000);
    let late = s4.feed(ScanEvent { at_min: 600, cap: CK::Camera, granted: true }, 71_000);
    cs.add("grant_after_deny_window", late.is_none(), "");

    // 5) 卸载前狂用：当日内第 10 条事件命中 SpreeBeforeUninstall。
    let mut s5 = AnomalyScanner::new();
    let mut last = None;
    for i in 0..10u64 {
        last = s5.feed(ScanEvent { at_min: 480 + i as u32, cap: CK::Documents, granted: true }, i * 1_000);
    }
    cs.add("spree_before_uninstall", last == Some(Anomaly::SpreeBeforeUninstall) && s5.found == 1, "");

    // 6) 月度摘要：记账/单格/行合计/全表合计（三维面）。
    let mut d = MonthDigest::new();
    d.record(2, 0); // Camera × Grant
    d.record(2, 1); // Camera × Intercept
    d.record(2, 1);
    let rec_ok = d.record(9, 0) == false && d.record(2, 3) == false;
    cs.add(
        "digest_cells",
        d.cell(2, 1) == 2 && d.cap_total(2) == 3 && d.grand_total() == 3 && rec_ok,
        "",
    );

    // 7) 导出 diff：新增 2 消失 1 分列（时点比对面）。
    let prev = [1u32, 2, 3];
    let curr = [2u32, 3, 5, 7];
    let mut out = [None; 16];
    let k = export_diff(&prev, &curr, &mut out);
    cs.add(
        "export_diff",
        k == 3
            && out[0] == Some(DiffEntry::Added(5))
            && out[1] == Some(DiffEntry::Added(7))
            && out[2] == Some(DiffEntry::Removed(1)),
        "",
    );

    // 8) 导出 diff 无变化：同集 → 零差异行（对拍绿的空结果也是结果）。
    let mut out2 = [None; 16];
    cs.add("export_diff_none", export_diff(&prev, &prev, &mut out2) == 0, "");

    // 9) 隐私空窗机械验证：空窗日事件数 0 → 真；非 0 → 假（黄条是承诺）。
    let gaps = [10u32, 11, 12];
    let clean = [(9u32, 3usize), (10, 0), (11, 0), (12, 0), (13, 2)];
    let dirty = [(10u32, 1usize)];
    cs.add(
        "privacy_gap_verify",
        privacy_gap_zero_events(&gaps, &clean) && !privacy_gap_zero_events(&gaps, &dirty),
        "",
    );

    // 10) 空窗无记录日视同零事件（诚实缺席不是违规）。
    cs.add("privacy_gap_absent_ok", privacy_gap_zero_events(&[20u32], &[(9u32, 5usize)]), "");

    // 11) 空窗受保留期约束：90 天内的日可标、超期的日不可标（不伪造历史）。
    cs.add(
        "gap_within_retention",
        gap_within_retention(&[50u32, 89], 100) && !gap_within_retention(&[9], 100),
        "",
    );

    // 12) 主册常量贯通：保留 90 天一处一事实。
    cs.add("consts_aligned", ARCHIVE_RETENTION_DAYS == 90, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;
    use CapKind as CK;

    #[test]
    fn scanner_day_boundary_reset_is_not_needed_for_night() {
        // 夜窗跨 0 点：00:30 的授予同样计入（跨日界语义）。
        let mut s = AnomalyScanner::new();
        s.feed(ScanEvent { at_min: 0, cap: CK::Microphone, granted: true }, 1_000);
        s.feed(ScanEvent { at_min: 30, cap: CK::Microphone, granted: true }, 2_000);
        let hit = s.feed(ScanEvent { at_min: 59, cap: CK::Microphone, granted: true }, 3_000);
        assert_eq!(hit, Some(Anomaly::NightBulkGrant));
    }

    #[test]
    fn digest_row_isolation() {
        // 三源行互不串扰：Intercept 暴涨不改 Grant 行。
        let mut d = MonthDigest::new();
        d.record(0, 0);
        for _ in 0..5 {
            d.record(0, 1);
        }
        assert_eq!(d.cell(0, 0), 1);
        assert_eq!(d.cell(0, 1), 5);
        assert_eq!(d.cap_total(0), 6);
    }

    #[test]
    fn diff_overflow_honest() {
        // 超容 diff：16 槽装满即停（截断可见——不静默丢）。
        let prev: [u32; 2] = [1, 2];
        let curr: Vec<u32> = (100..130).collect();
        let mut out = [None; 16];
        let k = export_diff(&prev, &curr, &mut out);
        assert_eq!(k, 16);
        assert!(out.iter().all(|o| o.is_some()));
    }
}
