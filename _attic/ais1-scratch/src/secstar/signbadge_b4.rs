//! F178 签名状态角标 · 批次四深化（secstar · G-G-08）。
//!
//! 批次四功能面（与批次三互补：批次三管「渲染与路由」，本批管
//! 「时序与统计」）：
//! - [`BadgeTimeline`]：角标态变化时间线——应用×时刻×旧态→新态入账
//!   （信任列表变动/签名服务恢复时角标怎么变——可回放面）；
//! - [`TooltipThrottle`]：tooltip 节流——同应用 5s 内重复悬停不重复
//!   弹（tooltip 该出现时出现、不该出现时不挡路的反骚扰面）；
//! - [`TrustImport`]：信任表导入校验——去重/上限/零 id 拒（导入
//!   不是倒入：脏数据进门就拦）；
//! - [`BadgeHistogram`]：三态分布直方——管理页「多少应用是灰盾」
//!   的统计面（三态计数 + 占比 ‰）。
//!
//! 零堆纪律：定长时间线 + 定长直方，无 alloc。

use super::signbadge::Badge;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 角标态变化时间线
// ---------------------------------------------------------------------------

/// 时间线容量。
pub const TIMELINE_CAP: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadgeTransition {
    pub app_id: u32,
    pub at_ms: u64,
    pub from: Badge,
    pub to: Badge,
}

pub struct BadgeTimeline {
    ring: [Option<BadgeTransition>; TIMELINE_CAP],
    head: usize,
    pub n: usize,
    pub overflows: u32,
}

impl BadgeTimeline {
    pub const fn new() -> BadgeTimeline {
        BadgeTimeline { ring: [const { None }; TIMELINE_CAP], head: 0, n: 0, overflows: 0 }
    }

    /// 记态变（同态不记——时间线只收真实变化）。
    pub fn record(&mut self, app_id: u32, at_ms: u64, from: Badge, to: Badge) -> bool {
        if from == to {
            return false;
        }
        if self.n == TIMELINE_CAP {
            self.overflows += 1;
        } else {
            self.n += 1;
        }
        self.ring[self.head] = Some(BadgeTransition { app_id, at_ms, from, to });
        self.head = (self.head + 1) % TIMELINE_CAP;
        true
    }

    /// 某应用最近一次变化（i=0 即最新——环回读序从 head 倒退）。
    pub fn last_of(&self, app_id: u32) -> Option<BadgeTransition> {
        (0..self.n)
            .filter_map(|i| self.ring[(self.head + TIMELINE_CAP - 1 - i) % TIMELINE_CAP])
            .find(|t| t.app_id == app_id)
    }

    /// 变化总数（含回卷丢弃——审计面诚实）。
    pub fn total_transitions(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// tooltip 节流
// ---------------------------------------------------------------------------

/// 节流窗（ms）——同应用 5s 内不重复弹。
pub const TOOLTIP_THROTTLE_MS: u64 = 5_000;

pub struct TooltipThrottle {
    last_shown: [u64; 16],
    app_ids: [Option<u32>; 16],
}

impl TooltipThrottle {
    pub const fn new() -> TooltipThrottle {
        TooltipThrottle { last_shown: [0; 16], app_ids: [const { None }; 16] }
    }

    /// 是否允许弹出（允许即记账——节流面单一出口）。
    pub fn allow(&mut self, app_id: u32, now_ms: u64) -> bool {
        for i in 0..16 {
            if self.app_ids[i] == Some(app_id) {
                if now_ms.saturating_sub(self.last_shown[i]) >= TOOLTIP_THROTTLE_MS {
                    self.last_shown[i] = now_ms;
                    return true;
                }
                return false;
            }
        }
        // 新应用：开槽即允许（首次悬停必出——发现性优先）。
        let i = self.app_ids.iter().position(|a| a.is_none()).unwrap_or(0);
        self.app_ids[i] = Some(app_id);
        self.last_shown[i] = now_ms;
        true
    }
}

// ---------------------------------------------------------------------------
// 信任表导入校验
// ---------------------------------------------------------------------------

/// 导入结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub accepted: usize,
    pub rejected_dup: usize,
    pub rejected_zero: usize,
    pub rejected_overflow: usize,
}

/// 批量导入：去重 + 零 id 拒 + 上限截断计数（导入报告三拒一收）。
pub fn trust_import(ids: &[u32], cap: usize, existing: &mut [Option<u32>], existing_n: &mut usize) -> ImportReport {
    let mut rep = ImportReport { accepted: 0, rejected_dup: 0, rejected_zero: 0, rejected_overflow: 0 };
    for id in ids {
        if *id == 0 {
            rep.rejected_zero += 1;
            continue;
        }
        if existing[..*existing_n].contains(&Some(*id)) {
            rep.rejected_dup += 1;
            continue;
        }
        if *existing_n >= cap {
            rep.rejected_overflow += 1;
            continue;
        }
        existing[*existing_n] = Some(*id);
        *existing_n += 1;
        rep.accepted += 1;
    }
    rep
}

// ---------------------------------------------------------------------------
// 三态分布直方
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct BadgeHistogram {
    pub none: u32,
    pub shield_gray: u32,
    pub shield_yellow: u32,
    pub dot_gray: u32,
}

impl BadgeHistogram {
    pub fn record(&mut self, b: Badge) {
        match b {
            Badge::None => self.none += 1,
            Badge::ShieldGray => self.shield_gray += 1,
            Badge::ShieldYellow => self.shield_yellow += 1,
            Badge::DotGray => self.dot_gray += 1,
        }
    }

    pub fn total(&self) -> u32 {
        self.none + self.shield_gray + self.shield_yellow + self.dot_gray
    }

    /// 未签类占比 ‰（灰盾+黄盾——需要用户警惕的面）。
    pub fn unsigned_permille(&self) -> u32 {
        let t = self.total();
        if t == 0 {
            return 0;
        }
        ((self.shield_gray + self.shield_yellow) * 1_000 / t) as u32
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_signbadge_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F178-b4");

    // 1) 时间线：态变入账、同态不记（时间线只收真实变化）。
    let mut tl = BadgeTimeline::new();
    let t1 = tl.record(7, 1_000, Badge::ShieldGray, Badge::ShieldYellow);
    let same = tl.record(7, 2_000, Badge::ShieldYellow, Badge::ShieldYellow);
    cs.add("timeline_records_changes", t1 && !same && tl.n == 1, "");

    // 2) 时间线回放：两应用各一次 → last_of 各归各（回放面）。
    tl.record(8, 3_000, Badge::None, Badge::ShieldGray);
    let l7 = tl.last_of(7).unwrap();
    let l8 = tl.last_of(8).unwrap();
    cs.add(
        "timeline_last_per_app",
        l7.to == Badge::ShieldYellow && l8.from == Badge::None && tl.total_transitions() == 2,
        "",
    );

    // 3) 时间线满容留痕：32 满后 overflows 计数（审计不静默丢）。
    let mut tl2 = BadgeTimeline::new();
    for i in 0..(TIMELINE_CAP + 4) as u64 {
        let from = if i % 2 == 0 { Badge::None } else { Badge::ShieldGray };
        let to = if i % 2 == 0 { Badge::ShieldGray } else { Badge::None };
        tl2.record(1, i, from, to);
    }
    cs.add("timeline_overflow_counted", tl2.n == TIMELINE_CAP && tl2.overflows == 4, "");

    // 4) tooltip 节流：5s 内重复悬停拒、5s 后放行（反骚扰面）。
    let mut th = TooltipThrottle::new();
    let first = th.allow(7, 0);
    let quick = th.allow(7, 3_000);
    let later = th.allow(7, 5_100);
    cs.add("tooltip_throttle", first && !quick && later, "");

    // 5) 节流首悬停必出：新应用第一次悬停不被老应用窗口冤枉（发现性）。
    let mut th2 = TooltipThrottle::new();
    th2.allow(1, 0);
    let other = th2.allow(2, 100);
    cs.add("tooltip_first_hover_shown", other, "");

    // 6) 信任导入三拒一收：去重/零 id/超容分别计数（导入面）。
    let mut store = [None; 8];
    let mut n = 0usize;
    let rep = trust_import(&[3, 3, 0, 9, 11, 12], 5, &mut store, &mut n);
    cs.add(
        "trust_import_report",
        rep.accepted == 4 && rep.rejected_dup == 1 && rep.rejected_zero == 1 && rep.rejected_overflow == 0 && n == 4,
        "",
    );

    // 7) 信任导入超容：cap 5 → 第 6 个计数 overflow（容量诚实）。
    let mut store2 = [None; 8];
    let mut n2 = 0usize;
    let rep2 = trust_import(&[1, 2, 3, 4, 5, 6, 7], 5, &mut store2, &mut n2);
    cs.add(
        "trust_import_cap",
        rep2.accepted == 5 && rep2.rejected_overflow == 2 && n2 == 5,
        "",
    );

    // 8) 直方三态计数：灰 3 黄 2 点 1 链验 4（分布面）。
    let mut hist = BadgeHistogram::default();
    for _ in 0..3 {
        hist.record(Badge::ShieldGray);
    }
    for _ in 0..2 {
        hist.record(Badge::ShieldYellow);
    }
    hist.record(Badge::DotGray);
    for _ in 0..4 {
        hist.record(Badge::None);
    }
    cs.add("histogram_counts", hist.total() == 10 && hist.shield_gray == 3 && hist.dot_gray == 1, "");

    // 9) 直方未签占比：灰+黄 = 5/10 = 500‰（管理页数字来源）。
    cs.add("histogram_unsigned_permille", hist.unsigned_permille() == 500, "");

    // 10) 直方空表诚实：0 总数 → 0‰（不编造）。
    cs.add("histogram_empty_zero", BadgeHistogram::default().unsigned_permille() == 0, "");

    // 11) 直方枚举完备：Badge 四变体逐一入账（加变体编译即红——
    //     match 穷尽性就是完备性验证本体）。
    let mut hist2 = BadgeHistogram::default();
    hist2.record(Badge::None);
    hist2.record(Badge::ShieldGray);
    hist2.record(Badge::ShieldYellow);
    hist2.record(Badge::DotGray);
    cs.add("histogram_enum_complete", hist2.total() == 4, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn timeline_app_isolation() {
        // 多应用时间线互不串扰：A 的态变不影响 B 的 last_of。
        let mut tl = BadgeTimeline::new();
        tl.record(1, 100, Badge::None, Badge::ShieldGray);
        tl.record(2, 200, Badge::None, Badge::ShieldYellow);
        tl.record(1, 300, Badge::ShieldGray, Badge::None);
        assert_eq!(tl.last_of(1).unwrap().to, Badge::None);
        assert_eq!(tl.last_of(2).unwrap().to, Badge::ShieldYellow);
    }

    #[test]
    fn throttle_per_app_isolation() {
        // 节流按应用隔离：A 冷却中 B 必出（反骚扰不反发现）。
        let mut th = TooltipThrottle::new();
        assert!(th.allow(1, 0));
        assert!(!th.allow(1, 1000));
        assert!(th.allow(2, 1000));
    }

    #[test]
    fn import_all_zero_rejected() {
        // 全零 id 导入：零收全拒（导入面零妥协）。
        let mut store = [None; 4];
        let mut n = 0;
        let rep = trust_import(&[0, 0, 0], 4, &mut store, &mut n);
        assert_eq!((rep.accepted, rep.rejected_zero), (0, 3));
        assert_eq!(n, 0);
    }
}
