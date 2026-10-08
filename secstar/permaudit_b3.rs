//! F179 权限审计页 · 批次三深化（secstar · G-G-09）。
//!
//! 批次三功能面（主册判据「时间线与三源账本对拍一致 / 收回即时 / 导出
//! 脱敏」纵深）：
//! - [`merge_timeline`]：三源归并——授权/执法/网络折算三源事件按时间序归并，
//!   同刻 tie-break 按源序（ord：授权 0 < 执法 1 < 网络 2）（对拍一致性的排序面）；
//! - [`AuditFilter`]：过滤查询——按能力/来源/时间窗三维过滤（审计页
//!   不是只给一条流水，是给可提问的时间线）；
//! - [`RetentionRing`]：保留期环——90 天外的条目归档不删（ARCHIVE_
//!   RETENTION_DAYS 的环账执行面）；
//! - [`grant_use_diff`]：授予-使用差异行——授了没用/用了没授两类异常
//!   分别出列（对拍不是摆设，是找矛盾）。
//!
//! 零堆纪律：定长事件表 + 定长环，无 alloc。

use super::permaudit::{CapKind, Source, ARCHIVE_RETENTION_DAYS, REVOKE_CONFIRM_STEPS, TIMELINE_DAYS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 三源归并
// ---------------------------------------------------------------------------

/// 归并事件（源 + 时间 + 能力）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergedEvent {
    pub source: Source,
    pub at_min: u32, // 当日分钟（0..1440）
    pub cap: CapKind,
}

/// 源序（同刻 tie-break：授权 0 < 执法 1 < 网络 2——ord 即 tie-break）。
fn source_rank(s: Source) -> u8 {
    s.ord()
}

/// 三源事件归并排序：时间升序、同刻按源序。原地插入排序（定长小表
/// 性能足够——P99 <1ms 的审计页渲染预算）。
pub fn merge_timeline(events: &mut [MergedEvent]) {
    for i in 1..events.len() {
        let mut j = i;
        while j > 0 && event_lt(events[j], events[j - 1]) {
            events.swap(j, j - 1);
            j -= 1;
        }
    }
}

fn event_lt(a: MergedEvent, b: MergedEvent) -> bool {
    a.at_min < b.at_min || (a.at_min == b.at_min && source_rank(a.source) < source_rank(b.source))
}

/// 三源各自有序 → 归并对拍：合并后序列的时间戳必须单调不降（一致性
/// 的机械判定——乱序即对拍红）。
pub fn timeline_monotone(events: &[MergedEvent]) -> bool {
    events.windows(2).all(|w| w[0].at_min <= w[1].at_min)
}

// ---------------------------------------------------------------------------
// 过滤查询
// ---------------------------------------------------------------------------

/// 三维过滤条件（None=不限）。
#[derive(Clone, Copy, Default)]
pub struct AuditFilter {
    pub cap: Option<CapKind>,
    pub source: Option<Source>,
    /// 时间窗 [from_min, to_min)。
    pub window: Option<(u32, u32)>,
}

impl AuditFilter {
    /// 单事件命中判定。
    pub fn matches(&self, e: &MergedEvent) -> bool {
        if let Some(c) = self.cap {
            if e.cap != c {
                return false;
            }
        }
        if let Some(s) = self.source {
            if e.source != s {
                return false;
            }
        }
        if let Some((from, to)) = self.window {
            if e.at_min < from || e.at_min >= to {
                return false;
            }
        }
        true
    }

    /// 过滤写入定长出槽，返回命中数（超容诚实截断计数）。
    pub fn apply(&self, events: &[MergedEvent], out: &mut [Option<MergedEvent>]) -> (usize, usize) {
        let mut hits = 0;
        let mut stored = 0;
        for e in events {
            if self.matches(e) {
                hits += 1;
                if stored < out.len() {
                    out[stored] = Some(*e);
                    stored += 1;
                }
            }
        }
        (stored, hits)
    }
}

// ---------------------------------------------------------------------------
// 保留期环
// ---------------------------------------------------------------------------

/// 保留环：事件带日号，90 天外标记归档（不删——审计不可灭失）。
pub struct RetentionRing {
    days: [Option<u32>; TIMELINE_DAYS],
    pub n: usize,
}

impl RetentionRing {
    pub const fn new() -> RetentionRing {
        RetentionRing { days: [const { None }; TIMELINE_DAYS], n: 0 }
    }

    /// 登记事件日号（重复日只记一次——日粒度账）。
    pub fn touch(&mut self, day: u32) -> bool {
        if self.n >= TIMELINE_DAYS || self.days[..self.n].contains(&Some(day)) {
            return false;
        }
        self.days[self.n] = Some(day);
        self.n += 1;
        true
    }

    /// 归档判定：day 距今超过保留期 → 归档（ARCHIVE_RETENTION_DAYS 对账）。
    pub fn archive_due(&self, day: u32, today: u32) -> bool {
        today.saturating_sub(day) > ARCHIVE_RETENTION_DAYS
    }

    /// 今日需归档的日号数（审计页「已归档 N 天」数字的来源）。
    pub fn archive_due_count(&self, today: u32) -> usize {
        (0..self.n).filter(|i| self.archive_due(self.days[*i].unwrap(), today)).count()
    }
}

// ---------------------------------------------------------------------------
// 授予-使用差异
// ---------------------------------------------------------------------------

/// 差异行类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// 授了没用（授予在案、窗口内零使用——权限收缩候选）。
    GrantedUnused,
    /// 用了没授（使用在案、授予缺席——越权信号，最高优先）。
    UsedWithoutGrant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: DiffKind,
    pub cap: CapKind,
}

/// 授予-使用对拍：位图比对（grant 位图 vs use 位图——同位不同态即差异）。
pub fn grant_use_diff(granted: u64, used: u64) -> [Option<DiffRow>; 6] {
    let mut out = [None; 6];
    let mut k = 0;
    for bit in 0..6u64 {
        let mask = 1u64 << bit;
        let cap = match bit {
            0 => CapKind::Documents,
            1 => CapKind::Network,
            2 => CapKind::Camera,
            3 => CapKind::Microphone,
            4 => CapKind::Location,
            _ => CapKind::DeviceRaw,
        };
        if k >= 6 {
            break;
        }
        if granted & mask != 0 && used & mask == 0 {
            out[k] = Some(DiffRow { kind: DiffKind::GrantedUnused, cap });
            k += 1;
        } else if granted & mask == 0 && used & mask != 0 {
            out[k] = Some(DiffRow { kind: DiffKind::UsedWithoutGrant, cap });
            k += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_permaudit_b3_checks() -> CheckSet {
    use CapKind as CK;
    let mut cs = CheckSet::new("F179-b3");

    // 1) 三源归并：乱序输入 → 时间升序 + 同刻内核先于用户（tie-break）。
    let mut evs = [
        MergedEvent { source: Source::Network, at_min: 100, cap: CK::Documents },
        MergedEvent { source: Source::Grant, at_min: 100, cap: CK::Documents },
        MergedEvent { source: Source::Intercept, at_min: 50, cap: CK::Network },
    ];
    merge_timeline(&mut evs);
    cs.add(
        "merge_sorted_tiebreak",
        evs[0].at_min == 50 && evs[1].source == Source::Grant && evs[2].source == Source::Network && timeline_monotone(&evs),
        "",
    );

    // 2) 归并单调性两面：有序真、构造乱序假（对拍的机械判定在岗）。
    let bad = [
        MergedEvent { source: Source::Grant, at_min: 200, cap: CK::Documents },
        MergedEvent { source: Source::Grant, at_min: 100, cap: CK::Documents },
    ];
    cs.add("monotone_two_ways", timeline_monotone(&evs) && !timeline_monotone(&bad), "");

    // 3) 三维过滤：能力维命中数对、窗口维半开区间语义（含头不含尾）。
    let f_cap = AuditFilter { cap: Some(CK::Documents), source: None, window: None };
    let f_win = AuditFilter { cap: None, source: None, window: Some((50, 100)) };
    let mut slot = [None; 8];
    let (stored_c, hits_c) = f_cap.apply(&evs, &mut slot);
    let (_, hits_w) = f_win.apply(&evs, &mut slot);
    cs.add(
        "filter_dims",
        stored_c == 2 && hits_c == 2 && hits_w == 1,
        "",
    );

    // 4) 过滤超容诚实：2 命中装 1 槽 → stored=1 hits=2（截断可见）。
    let mut one = [None; 1];
    let (stored_1, hits_1) = f_cap.apply(&evs, &mut one);
    cs.add("filter_overflow_honest", stored_1 == 1 && hits_1 == 2, "");

    // 5) 保留环：90 天内不归档、91 天归档（保留线逐日卡准）。
    let mut days = [const { None }; TIMELINE_DAYS];
    days[0] = Some(1);
    days[1] = Some(30);
    days[2] = Some(100);
    let ring = RetentionRing { days, n: 3 };
    cs.add(
        "retention_boundary",
        !ring.archive_due(100, 150) && ring.archive_due(1, 100) && ring.archive_due(100, 191),
        "",
    );

    // 6) 保留环计数：今日 100 → 恰 1 天需归档（页面数字来源直算）。
    cs.add("retention_count", ring.archive_due_count(100) == 1, "");

    // 7) 保留环重复日拒收（日粒度账不重复计）。
    let mut ring2 = RetentionRing::new();
    let t1 = ring2.touch(5);
    let t2 = ring2.touch(5);
    cs.add("retention_dedup", t1 && !t2 && ring2.n == 1, "");

    // 8) 授予-使用差异：授 3 用 1 → 2 行 GrantedUnused + 0 越权。
    let diffs = grant_use_diff(0b1011, 0b0010);
    let n_unused = diffs.iter().flatten().filter(|d| d.kind == DiffKind::GrantedUnused).count();
    let n_over = diffs.iter().flatten().filter(|d| d.kind == DiffKind::UsedWithoutGrant).count();
    cs.add("diff_granted_unused", n_unused == 2 && n_over == 0, "");

    // 9) 越权信号：用位 4（Location）未授 → 恰 1 行 UsedWithoutGrant。
    let diffs2 = grant_use_diff(0b0000, 0b010000);
    cs.add(
        "diff_overreach_flagged",
        diffs2[0] == Some(DiffRow { kind: DiffKind::UsedWithoutGrant, cap: CK::Location }),
        "",
    );

    // 10) 完全一致 → 零差异行（对拍绿的空结果也是结果）。
    cs.add("diff_none_when_aligned", grant_use_diff(0b111111, 0b111111).iter().all(|d| d.is_none()), "");

    // 11) 六能力全谱：位 0-5 映射 Camera..Devices 全六值可达（枚举面）。
    let all_unused = grant_use_diff(0b111111, 0);
    cs.add(
        "diff_six_caps_covered",
        all_unused[..6].iter().flatten().map(|d| d.cap).eq([CK::Documents, CK::Network, CK::Camera, CK::Microphone, CK::Location, CK::DeviceRaw]),
        "",
    );

    // 12) 主册常量贯通：保留 90 天 / 时间线 90 天 / 收回两步确认一处一事实。
    cs.add(
        "consts_aligned",
        ARCHIVE_RETENTION_DAYS == 90 && TIMELINE_DAYS == 90 && REVOKE_CONFIRM_STEPS == 2,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;
    use CapKind as CK;

    #[test]
    fn merge_sort_is_stable_for_same_source() {
        // 同源同刻保持原相对序（稳定排序——审计不洗时间戳顺序）。
        let mut evs = [
            MergedEvent { source: Source::Grant, at_min: 10, cap: CK::Camera },
            MergedEvent { source: Source::Grant, at_min: 10, cap: CK::Documents },
        ];
        merge_timeline(&mut evs);
        assert_eq!(evs[0].cap, CK::Camera);
        assert_eq!(evs[1].cap, CK::Documents);
    }

    #[test]
    fn filter_combined_dims() {
        // 三维叠加：能力 × 源 × 窗口同时命中才算（AND 语义）。
        let evs = [
            MergedEvent { source: Source::Grant, at_min: 100, cap: CK::Camera },
            MergedEvent { source: Source::Intercept, at_min: 100, cap: CK::Camera },
            MergedEvent { source: Source::Grant, at_min: 200, cap: CK::Camera },
        ];
        let f = AuditFilter { cap: Some(CK::Camera), source: Some(Source::Grant), window: Some((0, 150)) };
        let mut slot = [None; 8];
        let (stored, hits) = f.apply(&evs, &mut slot);
        assert_eq!((stored, hits), (1, 1));
    }

    #[test]
    fn diff_priority_overreach_first_scan() {
        // 授 0 用全 6 → 六行全为越权（UsedWithoutGrant 优先扫出）。
        let diffs = grant_use_diff(0, 0b111111);
        assert!(diffs[..6].iter().all(|d| matches!(d, Some(DiffRow { kind: DiffKind::UsedWithoutGrant, .. }))));
    }
}
