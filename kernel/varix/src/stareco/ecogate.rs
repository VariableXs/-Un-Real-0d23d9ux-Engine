//! F150 生态域总判据 · 完整设计（STAR I 主册 G-D-25）。
//!
//! **判据（主册）**：首轮 30 分钟线达成；逐季不回退（新流程重新
//! 计时）。
//!
//! **设计要点（主册）**：开放性总验收 = 外部人 30 分钟内走完「下载
//! 规范→打包应用→提交判例→查到状态」全流程计时，超时即修流程——
//! **开放不是口号是无障碍程度的实测值**。四步定义细化：规范页找到
//! ≤5min / 打包成功 ≤10min / 判例提交 ≤10min / 状态可查 ≤5min；
//! 志愿者画像记录（开发经验年限——可比性）；卡壳点分级（文档缺/
//! 工具错/流程绕三类）；志愿者缺席 → AI01 自测降级并标注（不可比
//! 口径说明）；流程大改 → 基线重置（标注断点）；结果进季报 F149
//! 第五指标（逐季应下降）。
//!
//! 本模块是总判据的**计时核**：四步限时表、全流程计时器（超时步
//! 定位）、卡壳点分级账、基线与断点、降级口径标注。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格（四步限时表）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// 规范页找到 ≤5min。
    FindSpec,
    /// 打包成功 ≤10min。
    PackApp,
    /// 判例提交 ≤10min。
    SubmitCase,
    /// 状态可查 ≤5min。
    CheckStatus,
}

impl Step {
    pub const ALL: [Step; 4] = [Step::FindSpec, Step::PackApp, Step::SubmitCase, Step::CheckStatus];

    pub fn limit_min(self) -> u32 {
        match self {
            Step::FindSpec => 5,
            Step::PackApp => 10,
            Step::SubmitCase => 10,
            Step::CheckStatus => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Step::FindSpec => "find-spec",
            Step::PackApp => "pack-app",
            Step::SubmitCase => "submit-case",
            Step::CheckStatus => "check-status",
        }
    }
}

/// 全流程预算（四步之和）。
pub const TOTAL_BUDGET_MIN: u32 = 30;

// ---------------------------------------------------------------------------
// 卡壳点分级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StuckKind {
    /// 文档缺。
    DocMissing,
    /// 工具错。
    ToolBug,
    /// 流程绕（步骤冗余/指引不清）。
    ProcessWinding,
}

// ---------------------------------------------------------------------------
// 一次外部人计时
// ---------------------------------------------------------------------------

pub struct Walkthrough {
    /// 每步实际耗时（分钟；None = 该步卡壳未完成）。
    minutes: [Option<u32>; 4],
    /// 卡壳点（步, 分级）。
    pub stuck: [Option<(Step, StuckKind)>; 4],
    stuck_count: usize,
    /// 志愿者是否外部真人（false = AI01 自测降级——不可比口径）。
    pub external_volunteer: bool,
    /// 志愿者开发经验年限（可比性画像）。
    pub volunteer_years: u8,
    /// 流程代次（流程大改 → 代次 +1，基线重置）。
    pub process_gen: u32,
}

impl Walkthrough {
    pub fn new(external_volunteer: bool, volunteer_years: u8, process_gen: u32) -> Walkthrough {
        Walkthrough {
            minutes: [None; 4],
            stuck: [None; 4],
            stuck_count: 0,
            external_volunteer,
            volunteer_years,
            process_gen,
        }
    }

    /// 记一步完成。
    pub fn complete(&mut self, step: Step, minutes: u32) -> Result<bool, &'static str> {
        if minutes == 0 || minutes > step.limit_min() {
            return Err("超时即修流程——该步必须记卡壳而不是硬填数");
        }
        self.minutes[step as usize] = Some(minutes);
        Ok(self.all_done())
    }

    /// 记一步卡壳（分级必选——卡壳不许无类而终）。
    pub fn stuck_at(&mut self, step: Step, kind: StuckKind) -> Result<(), &'static str> {
        if self.minutes[step as usize].is_some() {
            return Err("已完成步不再记卡壳");
        }
        if self.stuck_count >= 4 {
            return Err("卡壳账满");
        }
        self.stuck[self.stuck_count] = Some((step, kind));
        self.stuck_count += 1;
        Ok(())
    }

    pub fn all_done(&self) -> bool {
        Step::ALL.iter().all(|s| self.minutes[*s as usize].is_some())
    }

    /// 总耗时（未完成步按预算上限保守计——诚实高估不低报）。
    pub fn total_minutes(&self) -> u32 {
        Step::ALL
            .iter()
            .map(|s| self.minutes[*s as usize].unwrap_or_else(|| s.limit_min()))
            .sum()
    }

    /// 30 分钟线：全流程 ≤30 且四步全完成。
    pub fn within_30(&self) -> bool {
        self.all_done() && self.total_minutes() <= TOTAL_BUDGET_MIN
    }

    /// 卡壳账只读视图（归档与回流清单用）。
    pub fn stuck_view(&self) -> &[Option<(Step, StuckKind)>] {
        &self.stuck[..self.stuck_count]
    }

    /// 可比口径：外部志愿者 + 同流程代次。降级自测的数据不能跟外部
    /// 真人混算（判据「逐季不回退」的分母可比性）。
    pub fn comparable_with(&self, other: &Walkthrough) -> bool {
        self.external_volunteer && other.external_volunteer && self.process_gen == other.process_gen
    }
}

// ---------------------------------------------------------------------------
// 季度基线（逐季不回退 + 断点）
// ---------------------------------------------------------------------------

pub struct EcoGateBaseline {
    /// 各季最佳成绩（分钟，按流程代次分桶——跨代不可比）。
    seasons: [Option<(u32, u32, bool)>; 8], // (gen, minutes, external)
    count: usize,
    /// 基线重置断点（代次 → 重置季）。
    pub reset_points: [Option<u32>; 4],
    pub reset_count: usize,
}

impl EcoGateBaseline {
    pub const fn new() -> EcoGateBaseline {
        EcoGateBaseline { seasons: [None; 8], count: 0, reset_points: [None; 4], reset_count: 0 }
    }

    /// 归档一季成绩（同代次内才与上季比较）。
    pub fn archive(&mut self, gen: u32, minutes: u32, external: bool) -> Result<bool, &'static str> {
        if self.count >= 8 {
            return Err("季账满（8 季滚动）");
        }
        // 同代次连续性：与前一条比较代次；换代必须先登记断点
        if self.count > 0 {
            let prev = self.seasons[self.count - 1].unwrap();
            if gen != prev.0 && self.reset_points[..self.reset_count].iter().all(|r| r.unwrap_or(0) != self.count as u32) {
                return Err("流程大改必须先标断点（基线重置）");
            }
        }
        let mut no_regression = true;
        if self.count > 0 {
            let prev = self.seasons[self.count - 1].unwrap();
            // 逐季不回退：仅在同代次 + 双方均为外部志愿者口径下比较
            if prev.0 == gen && prev.2 && external && minutes > prev.1 {
                no_regression = false;
            }
        }
        self.seasons[self.count] = Some((gen, minutes, external));
        self.count += 1;
        Ok(no_regression)
    }

    /// 标注基线重置断点（流程大改）。
    pub fn mark_reset(&mut self, season_index: u32) -> Result<(), &'static str> {
        if self.reset_count >= 4 {
            return Err("断点账满");
        }
        self.reset_points[self.reset_count] = Some(season_index);
        self.reset_count += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150_TAG: &str = "stareco-F150-ecogate";

pub fn run_ecogate_checks() -> CheckSet {
    let mut set = CheckSet::new(F150_TAG);

    // 四步限时表 + 总预算
    let limits: [u32; 4] = Step::ALL.iter().map(|s| s.limit_min()).collect::<alloc::vec::Vec<_>>().try_into().unwrap();
    set.add(
        "f150 four-step limits",
        limits == [5, 10, 10, 5] && TOTAL_BUDGET_MIN == 30,
        "spec 5 / pack 10 / submit 10 / status 5",
    );

    // 首轮 30 分钟线达成（外部志愿者走全流程）
    let mut w = Walkthrough::new(true, 5, 1);
    set.add("f150 partial not done", !w.all_done(), "walk in progress");
    for (i, s) in Step::ALL.iter().enumerate() {
        w.complete(*s, [3, 8, 9, 4][i]).expect("step");
    }
    set.add("f150 first round within 30", w.within_30() && w.total_minutes() == 24, "24 ≤ 30 ✓");

    // 超时步不许硬填——必须记卡壳
    set.add("f150 overtime refused", w.complete(Step::PackApp, 11).is_err(), "honest stuck, not fake timing");
    set.add("f150 zero minutes refused", Walkthrough::new(true, 1, 1).complete(Step::FindSpec, 0).is_err(), "no zero-time");

    // 卡壳账：分级必选、已完成步拒记
    let mut w2 = Walkthrough::new(true, 3, 1);
    w2.stuck_at(Step::PackApp, StuckKind::ToolBug).expect("stuck");
    set.add(
        "f150 stuck classified",
        w2.stuck[0] == Some((Step::PackApp, StuckKind::ToolBug)),
        "three-kind taxonomy",
    );
    w2.complete(Step::PackApp, 9).ok();
    set.add("f150 done step no stuck", w2.stuck_at(Step::PackApp, StuckKind::DocMissing).is_err(), "no double bookkeeping");

    // 逐季不回退 + 断点
    let mut base = EcoGateBaseline::new();
    assert!(base.archive(1, 29, true).is_ok());
    let no_reg = base.archive(1, 27, true);
    set.add("f150 improving season green", no_reg == Ok(true), "27 < 29");
    set.add(
        "f150 regression flagged",
        base.archive(1, 28, true) == Ok(false),
        "28 > 27 = 回退如实标红",
    );
    // 换代不标断点 → 拒
    set.add("f150 gen change needs reset", base.archive(2, 20, true).is_err(), "baseline reset first");
    base.mark_reset(3).ok();
    let after = base.archive(2, 20, true);
    set.add("f150 reset then new baseline", after == Ok(true), "断点后新基线（标注在册）");

    // 可比性：外部真人口径
    let human_a = Walkthrough::new(true, 4, 1);
    let human_b = Walkthrough::new(true, 6, 1);
    let self_test = Walkthrough::new(false, 9, 1);
    set.add(
        "f150 comparable requires external+same gen",
        human_a.comparable_with(&human_b) && !human_a.comparable_with(&self_test),
        "AI01 自测降级不可比",
    );
    set.add("f150 volunteer profile kept", human_b.volunteer_years == 6, "years recorded");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_walk_then_stuck() {
        let mut w = Walkthrough::new(true, 2, 1);
        w.complete(Step::FindSpec, 4).unwrap();
        w.stuck_at(Step::PackApp, StuckKind::ProcessWinding).unwrap();
        w.complete(Step::PackApp, 10).unwrap();
        w.complete(Step::SubmitCase, 10).unwrap();
        w.complete(Step::CheckStatus, 5).unwrap();
        assert!(w.all_done());
        assert_eq!(w.total_minutes(), 29);
        assert!(w.within_30());
    }
}
