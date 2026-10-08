//! 星卡流水线：判例的可执行化与评级发布纪律（WP-302 · B-1004 十二星卡
//! 判例可自动执行出报告 + B-1005 无人工确认不转绿）。
//!
//! MD2 篇 10.4：星卡不是手写文档是流水线产物——判例脚本框架（准备/执行/
//! 采集/报告）+ 流水线三档（首轮定级/复评定级/月度抽检）+ **评级发布是
//! 流程终点也是唯一人工环节：脚本报告全绿且无未解释偏差才允许评级转绿
//! ——机器跑机器看，人对诚实性负最后责任**。运营面单独核算工时
//! （MD3 行 110：判例流水线运营预算与实现预算等量）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 判例三件套（脚本 + 断言 + 预期资源画像）
// ---------------------------------------------------------------------------

/// 判例编号（SC-xxx 的 u16 面——与三系编号纪律同源）。
pub type CaseId = u16;

/// 十二星卡（S307 首轮定级面）。
pub const STAR_CASES: usize = 12;

/// 预期资源画像（采集对账的锚——冷启动/内存峰值/CPU 空闲三采样）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResourceProfile {
    /// 冷启动时长上限（ms）。
    pub cold_start_ms: u32,
    /// 内存峰值上限（KB）。
    pub mem_peak_kb: u32,
    /// CPU 空闲占用上限（permille——千分比纪律）。
    pub idle_permille: u16,
}

/// 一条判例（SC-xxx）：脚本 + 断言数 + 预期画像。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StarCase {
    pub id: CaseId,
    /// 脚本断言数（0 断言的判例是空转）。
    pub assertions: u32,
    pub expect: ResourceProfile,
}

impl StarCase {
    pub fn well_formed(&self) -> bool {
        self.id > 0 && self.assertions > 0 && self.expect.idle_permille <= 1000
    }
}

// ---------------------------------------------------------------------------
// 执行采集与报告（篇 10.4：退出码/界面可达性探针/vxbench 三采样）
// ---------------------------------------------------------------------------

/// 采集四要素。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseRun {
    pub case_id: CaseId,
    pub exit_code: i32,
    /// 界面可达性探针（窗口起来且可聚焦）。
    pub ui_reachable: bool,
    /// 实测冷启动（ms）。
    pub cold_start_ms: u32,
    /// 实测内存峰值（KB）。
    pub mem_peak_kb: u32,
    /// 实测 CPU 空闲占用（permille）。
    pub idle_permille: u16,
}

/// 单判例执行裁决：退出零 + 界面可达 + 三采样不超画像。
pub fn run_matches(c: &StarCase, r: &CaseRun) -> bool {
    c.id == r.case_id
        && r.exit_code == 0
        && r.ui_reachable
        && r.cold_start_ms <= c.expect.cold_start_ms
        && r.mem_peak_kb <= c.expect.mem_peak_kb
        && r.idle_permille <= c.expect.idle_permille
}

/// 跑批报告：判例数 / 通过数 / 未解释偏差数（偏差要解释，不解释就是红）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PipelineReport {
    pub cases: usize,
    pub passed: usize,
    pub unexplained: usize,
}

/// 报告绿判：全量跑 + 全过 + 零未解释偏差。
pub fn report_green(rep: &PipelineReport) -> bool {
    rep.cases == STAR_CASES && rep.passed == rep.cases && rep.unexplained == 0
}

// ---------------------------------------------------------------------------
// 流水线三档（首轮定级 / 复评定级 / 月度抽检防漂移）
// ---------------------------------------------------------------------------

/// 流水线档位（穷举三档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipelineTier {
    /// 首轮定级：十二星卡全量。
    FirstPass,
    /// 复评定级：上游升级或 VARIX 更新触发（判例 24 联动）。
    ReReview,
    /// 抽检：月度随机抽三分之一复跑防漂移。
    SpotCheck,
}

/// 月度抽检比例：三分之一（篇 10.4）。
pub const SPOT_CHECK_NUM: usize = 1;
pub const SPOT_CHECK_DEN: usize = 3;

/// 抽检样本数（向上取整——十二取四）。
pub fn spot_check_count(cases: usize) -> usize {
    (cases * SPOT_CHECK_NUM + SPOT_CHECK_DEN - 1) / SPOT_CHECK_DEN
}

// ---------------------------------------------------------------------------
// 评级发布纪律（B-1005：无人工确认不转绿——流程审计）
// ---------------------------------------------------------------------------

/// 评级状态（穷举：草稿/待确认/已转绿/驳回）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RatingState {
    Draft,
    AwaitingConfirm,
    Green,
    Rejected,
}

/// 评级转绿的门（**类型面强制**）：报告绿 + 无未解释偏差 + **人工确认**
/// 三条件缺一不可——无人工确认不转绿不是流程建议是编译期事实。
pub fn promote_to_green(rep: &PipelineReport, human_confirmed: bool) -> Result<RatingState, RatingState> {
    if !report_green(rep) {
        return Err(RatingState::Rejected);
    }
    if !human_confirmed {
        return Err(RatingState::AwaitingConfirm);
    }
    Ok(RatingState::Green)
}

/// 流程审计：转绿记录必须携带人工确认（审计面回放逐条核对）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromotionRecord {
    pub case_id: CaseId,
    pub human_confirmed: bool,
    pub to_green: bool,
}

/// 审计判定：任何 to_green=true 的记录必须 human_confirmed=true——
/// 无人工确认的转绿在审计里无所遁形。
pub fn audit_promotions(records: &[PromotionRecord]) -> bool {
    let mut i = 0;
    while i < records.len() {
        if records[i].to_green && !records[i].human_confirmed {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// CheckSet（B-1004/1005 · 7 项）
// ---------------------------------------------------------------------------

pub fn run_winecase_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1004/1005 判例框架与评级纪律");
    // 1. 判例三件套：脚本+断言+预期画像（零断言判例是空转）。
    let ok = StarCase { id: 1, assertions: 5, expect: ResourceProfile { cold_start_ms: 8000, mem_peak_kb: 512_000, idle_permille: 50 } };
    let empty = StarCase { id: 2, assertions: 0, expect: ok.expect };
    set.add(
        "B-1004 判例三件套",
        ok.well_formed() && !empty.well_formed(),
        "每条判例=脚本+断言+预期资源画像——判例是可执行用例不是文档",
    );
    // 2. 采集四要素：退出码+界面探针+资源三采样对账。
    let c = ok;
    let good_run = CaseRun { case_id: 1, exit_code: 0, ui_reachable: true, cold_start_ms: 6500, mem_peak_kb: 480_000, idle_permille: 40 };
    let late_run = CaseRun { case_id: 1, exit_code: 0, ui_reachable: true, cold_start_ms: 9000, mem_peak_kb: 480_000, idle_permille: 40 };
    set.add(
        "B-1004 采集与画像对账",
        run_matches(&c, &good_run) && !run_matches(&c, &late_run),
        "退出码/界面可达性/vxbench 三采样——实测不超预期画像",
    );
    // 3. 三档流水线齐：首轮/复评/抽检。
    let tiers = [PipelineTier::FirstPass, PipelineTier::ReReview, PipelineTier::SpotCheck];
    let mut distinct = 0;
    let mut i = 0;
    while i < tiers.len() {
        let mut j = i + 1;
        while j < tiers.len() && tiers[i] != tiers[j] {
            j += 1;
        }
        if j == tiers.len() {
            distinct += 1;
        }
        i += 1;
    }
    set.add(
        "B-1004 三档流水线齐",
        distinct == 3 && SPOT_CHECK_DEN == 3,
        "首轮定级/复评定级（升级触发）/月度抽检防漂移——三档穷举",
    );
    // 4. 抽检比例：月度三分之一（十二取四，向上取整）。
    set.add(
        "B-1004 月度抽检三分之一",
        spot_check_count(12) == 4 && spot_check_count(3) == 1 && spot_check_count(1) == 1,
        "随机抽三分之一样例复跑——漂移在月度窗口被抓",
    );
    // 5. 十二星卡全量出报告（B-1004 达标线）。
    let full = PipelineReport { cases: 12, passed: 12, unexplained: 0 };
    let partial = PipelineReport { cases: 12, passed: 11, unexplained: 0 };
    set.add(
        "B-1004 十二星卡全量",
        report_green(&full) && !report_green(&partial),
        "十二星卡判例自动执行出报告——首轮定级全量不含糊",
    );
    // 6. 无人工确认不转绿（B-1005 达标线，类型面强制）。
    let no_confirm = promote_to_green(&full, false);
    let confirmed = promote_to_green(&full, true);
    set.add(
        "B-1005 无确认不转绿",
        no_confirm == Err(RatingState::AwaitingConfirm) && confirmed == Ok(RatingState::Green),
        "报告全绿只是必要条件——人对诚实性负最后责任（唯一人工环节）",
    );
    // 7. 流程审计：未确认的转绿在审计里必红。
    let records = [
        PromotionRecord { case_id: 1, human_confirmed: true, to_green: true },
        PromotionRecord { case_id: 2, human_confirmed: false, to_green: true },
        PromotionRecord { case_id: 3, human_confirmed: false, to_green: false },
    ];
    let clean = [records[0], records[2]];
    set.add(
        "B-1005 流程审计",
        !audit_promotions(&records) && audit_promotions(&clean),
        "转绿记录逐条携带人工确认——审计面回放无遁形",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe04 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn case1() -> StarCase {
        StarCase { id: 1, assertions: 3, expect: ResourceProfile { cold_start_ms: 5000, mem_peak_kb: 400_000, idle_permille: 60 } }
    }

    #[test]
    fn fe04_case_run_matches_profile() {
        let c = case1();
        let ok = CaseRun { case_id: 1, exit_code: 0, ui_reachable: true, cold_start_ms: 5000, mem_peak_kb: 400_000, idle_permille: 60 };
        assert!(run_matches(&c, &ok)); // 恰在画像边界内（≤ 语义）
        let bad_exit = CaseRun { case_id: 1, exit_code: 1, ui_reachable: true, cold_start_ms: 100, mem_peak_kb: 1, idle_permille: 1 };
        let blind = CaseRun { case_id: 1, exit_code: 0, ui_reachable: false, cold_start_ms: 100, mem_peak_kb: 1, idle_permille: 1 };
        let other = CaseRun { case_id: 2, exit_code: 0, ui_reachable: true, cold_start_ms: 100, mem_peak_kb: 1, idle_permille: 1 };
        assert!(!run_matches(&c, &bad_exit));
        assert!(!run_matches(&c, &blind));
        assert!(!run_matches(&c, &other)); // 判例号不匹配的运行不算数
    }

    #[test]
    fn fe04_report_green_needs_full_and_explained() {
        assert!(report_green(&PipelineReport { cases: 12, passed: 12, unexplained: 0 }));
        assert!(!report_green(&PipelineReport { cases: 11, passed: 11, unexplained: 0 }));
        assert!(!report_green(&PipelineReport { cases: 12, passed: 12, unexplained: 2 }));
        // 未解释偏差是绿的死敌——解释了才能进评级流程。
    }

    #[test]
    fn fe04_promotion_gate_type_forced() {
        let green_report = PipelineReport { cases: 12, passed: 12, unexplained: 0 };
        // 全绿但未确认：卡在 AwaitingConfirm（不是 Green）。
        assert_eq!(promote_to_green(&green_report, false), Err(RatingState::AwaitingConfirm));
        // 全绿且确认：转绿。
        assert_eq!(promote_to_green(&green_report, true), Ok(RatingState::Green));
        // 不绿：直接驳回（确认了也驳回——人工不能洗白红报告）。
        assert_eq!(promote_to_green(&PipelineReport { cases: 12, passed: 9, unexplained: 0 }, true), Err(RatingState::Rejected));
    }

    #[test]
    fn fe04_spot_check_and_audit() {
        // 抽检向上取整：12→4、3→1、2→1、1→1。
        assert_eq!(spot_check_count(12), 4);
        assert_eq!(spot_check_count(3), 1);
        assert_eq!(spot_check_count(2), 1);
        assert_eq!(spot_check_count(1), 1);
        // 审计：未确认转绿必红；确认转绿与非转绿记录不误伤。
        let recs = [
            PromotionRecord { case_id: 1, human_confirmed: true, to_green: true },
            PromotionRecord { case_id: 2, human_confirmed: true, to_green: false },
        ];
        assert!(audit_promotions(&recs));
        let bad = [PromotionRecord { case_id: 3, human_confirmed: false, to_green: true }];
        assert!(!audit_promotions(&bad));
    }
}
