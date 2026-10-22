//! VE-F3428 · 换肤性能预算分解 —— 四段预算 + 分段实测 + 超支降档 + 口径统一。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3428`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四段预算**：一次换肤的耗时拆成四段，各给预算：
//!   - **diff**：算新旧令牌差异；
//!   - **求值**：把令牌求值成渲染属性；
//!   - **重绘**：受影响组件重新绘制；
//!   - **动画**：过渡帧的合成。
//!   四段预算之和必须等于总预算——不等就是 [`BudgetCode::SumMismatch`]，
//!   不许「留点余量」：余量会变成没人负责的黑洞；
//! - **分段实测**：每段跑完立刻记账（[`BudgetMeter::measure`]）。记账必须
//!   在段内完成，不许事后估算——事后估算出来的数字永远比真实值好看；
//! - **超支段自动降档**：哪段超支就降哪段的表现，不牵连其它段：
//!   diff 超 → 分帧；求值超 → 走增量缓存；重绘超 → 局部优先；
//!   动画超 → 降档到透明度渐变/瞬切。降档映射见 [`downgrade_for`]；
//! - **口径统一**：四段必须用同一时钟、同一单位（毫秒），且与 AD06 计量
//!   同构。混用 tick 与 ms 会让「超支」变成随机事件——所以
//!   [`Caliber`] 只接受毫秒，其它一律 [`BudgetCode::CaliberMismatch`]。
//!
//! # 为什么预算要拆段而不是只看总时长
//!
//! 总时长 60ms 可能来自「diff 5 + 求值 10 + 重绘 40 + 动画 5」，也可能
//! 来自「diff 30 + 求值 20 + 重绘 5 + 动画 5」。前者该优化重绘，后者
//! 该优化 diff——只看总分永远不知道该动哪里，最后只能全链一起优化，
//! 代价最大、收益最小。拆段是**让优化有靶子**的最小代价。
//!
//! # 复杂度
//!
//! - `measure`：O(1)；
//! - `report`：O(4)；
//! - `downgrade_for`：O(1)。
//!
//! # 零 panic 面
//!
//! 固定四段用数组 + `get()`，无索引 panic。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5467 起）
// ---------------------------------------------------------------------------

/// 预算诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetCode {
    /// 四段预算之和与总预算不等。
    SumMismatch = 5467,
    /// 某段超支。
    SegmentOver = 5468,
    /// 计量口径不符（不是毫秒）。
    CaliberMismatch = 5469,
    /// 某段未实测（收口前必须四段都有数）。
    SegmentUnmeasured = 5470,
    /// 总预算为 0。
    ZeroBudget = 5471,
    /// 同一段重复记账（后一次覆盖前一次会掩盖真实耗时）。
    DuplicateMeasurement = 5472,
    /// 降档已无可用手段。
    NoDowngrade = 5473,
}

impl BudgetCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BudgetCode::SumMismatch => "四段预算之和不等于总预算",
            BudgetCode::SegmentOver => "分段超支",
            BudgetCode::CaliberMismatch => "计量口径不是毫秒",
            BudgetCode::SegmentUnmeasured => "分段未实测",
            BudgetCode::ZeroBudget => "总预算为 0",
            BudgetCode::DuplicateMeasurement => "分段重复记账",
            BudgetCode::NoDowngrade => "无可用降档手段",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、四段与口径
// ---------------------------------------------------------------------------

/// 四个耗时段（封闭全集）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Segment {
    /// 差异计算。
    Diff,
    /// 令牌求值。
    Eval,
    /// 组件重绘。
    Repaint,
    /// 过渡动画。
    Animate,
}

impl Segment {
    /// 全部四段（顺序即报告顺序）。
    pub fn all() -> [Segment; 4] {
        [Segment::Diff, Segment::Eval, Segment::Repaint, Segment::Animate]
    }

    /// 下标（0..=3）。
    pub fn index(self) -> usize {
        match self {
            Segment::Diff => 0,
            Segment::Eval => 1,
            Segment::Repaint => 2,
            Segment::Animate => 3,
        }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Segment::Diff => "差异计算",
            Segment::Eval => "令牌求值",
            Segment::Repaint => "组件重绘",
            Segment::Animate => "过渡动画",
        }
    }
}

/// 计量口径。内核里只允许毫秒：tick 与 ms 混用会让超支判定随帧率漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Caliber {
    /// 毫秒（唯一合法口径）。
    Millis,
    /// 微秒（拒绝）。
    Micros,
    /// 帧 tick（拒绝）。
    Ticks,
}

impl Caliber {
    /// 是否合法。
    pub fn accepted(self) -> bool {
        matches!(self, Caliber::Millis)
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Caliber::Millis => "毫秒",
            Caliber::Micros => "微秒",
            Caliber::Ticks => "帧 tick",
        }
    }
}

/// 降档手段。与 F3423 的策略对齐，但本模块只给「该降什么」的结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mitigation {
    /// diff 分帧。
    FrameSplit,
    /// 求值走增量缓存。
    IncrementalCache,
    /// 重绘只做局部。
    LocalRepaintOnly,
    /// 动画降透明渐变。
    AlphaFade,
    /// 动画降瞬切。
    InstantCut,
}

impl Mitigation {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Mitigation::FrameSplit => "差异分帧",
            Mitigation::IncrementalCache => "增量求值",
            Mitigation::LocalRepaintOnly => "仅局部重绘",
            Mitigation::AlphaFade => "动画降透明度渐变",
            Mitigation::InstantCut => "动画降瞬切",
        }
    }
}

/// 某段超支时该做什么。返回两级：先降一档，仍超再降一档。
pub fn downgrade_for(segment: Segment, level: u8) -> Option<Mitigation> {
    match (segment, level) {
        (Segment::Diff, 0) => Some(Mitigation::FrameSplit),
        (Segment::Eval, 0) => Some(Mitigation::IncrementalCache),
        (Segment::Repaint, 0) => Some(Mitigation::LocalRepaintOnly),
        (Segment::Animate, 0) => Some(Mitigation::AlphaFade),
        (Segment::Animate, 1) => Some(Mitigation::InstantCut),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 三、预算与实测
// ---------------------------------------------------------------------------

/// 预算计划。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetPlan {
    /// 四段预算（毫秒）。
    pub segments: [u32; 4],
    /// 总预算（毫秒）。
    pub total_ms: u32,
}

impl BudgetPlan {
    /// 按比例分解总预算：diff 15%、求值 35%、重绘 35%、动画 15%。
    /// 比例来自实测画像（求值与重绘是大头），改动要附实测数据。
    pub fn proportional(total_ms: u32) -> BudgetPlan {
        let diff = total_ms * 15 / 100;
        let eval = total_ms * 35 / 100;
        let repaint = total_ms * 35 / 100;
        // 最后一段吃掉整数除法的余数，保证四段之和严格等于总预算。
        let animate = total_ms
            .saturating_sub(diff)
            .saturating_sub(eval)
            .saturating_sub(repaint);
        BudgetPlan {
            segments: [diff, eval, repaint, animate],
            total_ms,
        }
    }

    /// 校验：总和相等、总预算非零。
    pub fn validate(&self) -> Result<(), BudgetCode> {
        if self.total_ms == 0 {
            return Err(BudgetCode::ZeroBudget);
        }
        let mut sum = 0u32;
        for v in self.segments.iter() {
            sum = sum.saturating_add(*v);
        }
        if sum != self.total_ms {
            return Err(BudgetCode::SumMismatch);
        }
        Ok(())
    }

    /// 某段预算。
    pub fn budget_of(&self, segment: Segment) -> u32 {
        self.segments
            .get(segment.index())
            .copied()
            .unwrap_or(0)
    }
}

/// 分段实测器。
#[derive(Clone, Debug)]
pub struct BudgetMeter {
    plan: BudgetPlan,
    /// 每段实测（`None` = 未测）。
    measured: [Option<u32>; 4],
    caliber: Caliber,
    /// 每段已降档次数。
    downgrades: [u8; 4],
}

impl BudgetMeter {
    /// 新建。口径默认毫秒。
    pub fn new(plan: BudgetPlan) -> Result<BudgetMeter, BudgetCode> {
        plan.validate()?;
        Ok(BudgetMeter {
            plan,
            measured: [None; 4],
            caliber: Caliber::Millis,
            downgrades: [0; 4],
        })
    }

    /// 设置口径。只收毫秒。
    pub fn set_caliber(&mut self, caliber: Caliber) -> Result<(), BudgetCode> {
        if !caliber.accepted() {
            return Err(BudgetCode::CaliberMismatch);
        }
        self.caliber = caliber;
        Ok(())
    }

    /// 记录一段实测。各段只许记一次——重复记账等于让实现自己挑一个
    /// 好看的数交上来。
    pub fn measure(&mut self, segment: Segment, ms: u32) -> Result<(), BudgetCode> {
        if !self.caliber.accepted() {
            return Err(BudgetCode::CaliberMismatch);
        }
        let slot = match self.measured.get_mut(segment.index()) {
            Some(s) => s,
            None => return Err(BudgetCode::SegmentUnmeasured),
        };
        if slot.is_some() {
            return Err(BudgetCode::DuplicateMeasurement);
        }
        *slot = Some(ms);
        Ok(())
    }

    /// 某段实测值。
    pub fn measured_of(&self, segment: Segment) -> Option<u32> {
        self.measured.get(segment.index()).copied().flatten()
    }

    /// 某段是否超支。
    pub fn is_over(&self, segment: Segment) -> bool {
        match self.measured_of(segment) {
            Some(v) => v > self.plan.budget_of(segment),
            None => false,
        }
    }

    /// 生成报告。
    pub fn report(&self) -> BudgetReport {
        let mut rows: Vec<SegmentRow> = Vec::new();
        let mut total_measured = 0u32;
        let mut over: Vec<Segment> = Vec::new();
        for seg in Segment::all().iter() {
            let budget = self.plan.budget_of(*seg);
            let measured = self.measured_of(*seg);
            let over_flag = match measured {
                Some(v) => v > budget,
                None => false,
            };
            if over_flag {
                over.push(*seg);
            }
            if let Some(v) = measured {
                total_measured = total_measured.saturating_add(v);
            }
            rows.push(SegmentRow {
                segment: *seg,
                budget_ms: budget,
                measured_ms: measured,
                over: over_flag,
                downgrades: self.downgrades.get(seg.index()).copied().unwrap_or(0),
            });
        }
        BudgetReport {
            rows,
            total_budget_ms: self.plan.total_ms,
            total_measured_ms: total_measured,
            over,
            caliber: self.caliber,
        }
    }

    /// 对超支段取降档手段，并记一次降档。
    pub fn mitigate(&mut self, segment: Segment) -> Result<Mitigation, BudgetCode> {
        let level = self.downgrades.get(segment.index()).copied().unwrap_or(0);
        match downgrade_for(segment, level) {
            Some(m) => {
                if let Some(slot) = self.downgrades.get_mut(segment.index()) {
                    *slot = slot.saturating_add(1);
                }
                Ok(m)
            }
            None => Err(BudgetCode::NoDowngrade),
        }
    }
}

/// 一行分段实测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentRow {
    /// 段。
    pub segment: Segment,
    /// 预算。
    pub budget_ms: u32,
    /// 实测（`None` 未测）。
    pub measured_ms: Option<u32>,
    /// 是否超支。
    pub over: bool,
    /// 已降档次数。
    pub downgrades: u8,
}

/// 预算报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BudgetReport {
    /// 四行。
    pub rows: Vec<SegmentRow>,
    /// 总预算。
    pub total_budget_ms: u32,
    /// 总实测（已测段之和）。
    pub total_measured_ms: u32,
    /// 超支段。
    pub over: Vec<Segment>,
    /// 口径。
    pub caliber: Caliber,
}

impl BudgetReport {
    /// 四段是否都测过。
    pub fn complete(&self) -> bool {
        self.rows.iter().all(|r| r.measured_ms.is_some())
    }

    /// 未测段（收口前必须为空）。
    pub fn unmeasured(&self) -> Vec<Segment> {
        self.rows
            .iter()
            .filter(|r| r.measured_ms.is_none())
            .map(|r| r.segment)
            .collect()
    }

    /// 是否全部达标。
    pub fn all_within(&self) -> bool {
        self.over.is_empty()
    }

    /// 报告文本（读屏可达，一行一段）。
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "换肤预算：总预算 {}ms，实测 {}ms，口径 {}，超支 {} 段",
            self.total_budget_ms,
            self.total_measured_ms,
            self.caliber.label(),
            self.over.len()
        ));
        for r in self.rows.iter() {
            let measured = match r.measured_ms {
                Some(v) => format!("{}ms", v),
                None => "未测".to_string(),
            };
            out.push(format!(
                "{}：预算 {}ms，实测 {}{}{}",
                r.segment.label(),
                r.budget_ms,
                measured,
                if r.over { "，超支" } else { "" },
                if r.downgrades > 0 {
                    format!("，已降档 {} 次", r.downgrades)
                } else {
                    String::new()
                }
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn meter() -> BudgetMeter {
        BudgetMeter::new(BudgetPlan::proportional(100)).expect("计划")
    }

    #[test]
    fn proportional_plan_sums_exactly() {
        let p = BudgetPlan::proportional(100);
        assert_eq!(p.segments, [15, 35, 35, 15]);
        assert!(p.validate().is_ok());
        // 不能被 100 整除的预算也要严格配平。
        let q = BudgetPlan::proportional(97);
        let sum: u32 = q.segments.iter().sum();
        assert_eq!(sum, 97);
        assert!(q.validate().is_ok());
    }

    #[test]
    fn sum_mismatch_is_rejected() {
        let bad = BudgetPlan {
            segments: [10, 10, 10, 10],
            total_ms: 100,
        };
        assert_eq!(bad.validate().err(), Some(BudgetCode::SumMismatch));
        assert!(BudgetMeter::new(bad).is_err());
    }

    #[test]
    fn four_segments_must_all_be_measured() {
        let mut m = meter();
        m.measure(Segment::Diff, 10).expect("测");
        m.measure(Segment::Eval, 30).expect("测");
        let r = m.report();
        assert!(!r.complete());
        assert_eq!(r.unmeasured(), alloc::vec![Segment::Repaint, Segment::Animate]);
        m.measure(Segment::Repaint, 30).expect("测");
        m.measure(Segment::Animate, 10).expect("测");
        assert!(m.report().complete());
        assert!(m.report().all_within());
    }

    #[test]
    fn over_budget_segment_is_flagged_and_mitigated() {
        let mut m = meter();
        m.measure(Segment::Diff, 5).expect("测");
        m.measure(Segment::Eval, 20).expect("测");
        m.measure(Segment::Repaint, 80).expect("测");
        m.measure(Segment::Animate, 5).expect("测");
        let r = m.report();
        assert_eq!(r.over, alloc::vec![Segment::Repaint]);
        assert!(!r.all_within());
        assert_eq!(
            m.mitigate(Segment::Repaint).ok(),
            Some(Mitigation::LocalRepaintOnly)
        );
        // 重绘只有一档降档，再来就没有了。
        assert_eq!(
            m.mitigate(Segment::Repaint).err(),
            Some(BudgetCode::NoDowngrade)
        );
    }

    #[test]
    fn animate_has_two_downgrade_levels() {
        assert_eq!(
            downgrade_for(Segment::Animate, 0),
            Some(Mitigation::AlphaFade)
        );
        assert_eq!(
            downgrade_for(Segment::Animate, 1),
            Some(Mitigation::InstantCut)
        );
        assert_eq!(downgrade_for(Segment::Animate, 2), None);
    }

    #[test]
    fn duplicate_measurement_is_rejected() {
        let mut m = meter();
        m.measure(Segment::Diff, 3).expect("首次");
        assert_eq!(
            m.measure(Segment::Diff, 1).err(),
            Some(BudgetCode::DuplicateMeasurement)
        );
        assert_eq!(m.measured_of(Segment::Diff), Some(3));
    }

    #[test]
    fn non_millisecond_caliber_is_rejected() {
        let mut m = meter();
        assert_eq!(
            m.set_caliber(Caliber::Ticks).err(),
            Some(BudgetCode::CaliberMismatch)
        );
        assert!(m.set_caliber(Caliber::Millis).is_ok());
    }

    #[test]
    fn zero_budget_is_rejected() {
        let z = BudgetPlan {
            segments: [0, 0, 0, 0],
            total_ms: 0,
        };
        assert_eq!(z.validate().err(), Some(BudgetCode::ZeroBudget));
    }

    #[test]
    fn report_lines_are_screen_reader_ready() {
        let mut m = meter();
        m.measure(Segment::Diff, 10).expect("测");
        m.measure(Segment::Eval, 40).expect("测");
        let lines = m.report().lines();
        assert!(lines.first().map(|l| l.contains("口径 毫秒")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("令牌求值：预算 35ms，实测 40ms，超支")));
        assert!(lines.iter().any(|l| l.contains("组件重绘：预算 35ms，实测 未测")));
    }
}
