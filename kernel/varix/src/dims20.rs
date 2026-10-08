//! 20 维度证据化与每日流水（WP-401 · B-1205/1206 每维度有证据模板与实例×构建到报告全自动）。
//!
//! MD2 篇 12.4/12.5（执行化）：20 维度每维度定"证据形态"——验收不是打分
//! 是交证据，证据齐全才算该维度通过，"感觉没问题"不构成证据。每日流水：
//! 构建到报告全自动，红绿通知。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 20 维度证据化
// ---------------------------------------------------------------------------

/// 维度总数（MD1 附录 C.2 同构——20 维一字不减）。
pub const DIMENSIONS: u8 = 20;

/// 维度证据（模板与实例两件——模板在没实例是空头，实例没模板是野路子）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DimEvidence {
    /// 维度号（1..=20）。
    pub dim: u8,
    /// 证据模板在册（每维度定证据形态——先有形态才有证据）。
    pub template: bool,
    /// 证据实例在册（真跑出来的记录——"感觉没问题"不构成证据）。
    pub instance: bool,
}

/// 维度号合法判：1..=20（0 与 21+ 都是账外维度）。
pub fn dim_valid(dim: u8) -> bool {
    dim >= 1 && dim <= DIMENSIONS
}

/// 证据化判（**B-1205 达标线：每维度有证据模板与实例**）——恰好 20 条账、
/// 维度号 1..=20 无重复、每条模板与实例双全。
pub fn evidence_complete(evs: &[DimEvidence]) -> bool {
    if evs.len() != DIMENSIONS as usize {
        return false;
    }
    let mut seen = [false; 21]; // 下标 0 弃用，1..=20 对账
    let mut i = 0;
    while i < evs.len() {
        let e = &evs[i];
        if !dim_valid(e.dim) || seen[e.dim as usize] {
            return false;
        }
        seen[e.dim as usize] = true;
        if !e.template || !e.instance {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 每日流水（构建到报告全自动）
// ---------------------------------------------------------------------------

/// 流水段（穷举四段——构建/测试/报告/通知，段序即流水序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipelineStage {
    /// 构建（一条命令出镜像）。
    Build,
    /// 测试（三层全套：单元+集成+QEMU 冒烟）。
    Test,
    /// 报告（红绿结果结构化产出）。
    Report,
    /// 通知（红绿都通知——红了不知道是流水最大的失败）。
    Notify,
}

/// 每日流水状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DailyPipeline {
    /// 四段全部自动化（无人值守跑完——人工环节插在流水里就不是每日流水）。
    pub automated: [bool; 4],
    /// 流水红时通知必发（绿报喜红报忧——只报喜的流水是装饰品）。
    pub notify_on_fail: bool,
}

/// 流水完整判（**B-1206 达标线：构建到报告全自动，红绿通知**）——四段全
/// 自动且失败通知在册。
pub fn pipeline_ok(p: &DailyPipeline) -> bool {
    let mut all_auto = true;
    let mut i = 0;
    while i < 4 {
        if !p.automated[i] {
            all_auto = false;
        }
        i += 1;
    }
    all_auto && p.notify_on_fail
}

// ---------------------------------------------------------------------------
// CheckSet（B-1205/1206 · 5 项）
// ---------------------------------------------------------------------------

pub fn run_dims20_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1205/1206 二十维度证据化与每日流水");
    // 1. 维度号合法：1..=20，0 与 21 拒。
    set.add(
        "B-1205 维度号合法",
        dim_valid(1) && dim_valid(20) && !dim_valid(0) && !dim_valid(21),
        "维度账 1..=20——账外维度进不了对账（20 维一字不减）",
    );
    // 2. 证据化全判（B-1205 达标线）：恰好 20 条、无重复、模板+实例双全。
    let mut evs = [DimEvidence { dim: 0, template: true, instance: true }; 20];
    let mut i = 0;
    while i < 20 {
        evs[i].dim = (i + 1) as u8;
        i += 1;
    }
    let mut missing_instance = evs;
    missing_instance[19].instance = false; // 维度 20 缺实例
    let mut dup = evs;
    dup[19].dim = 1; // 与维度 1 重复
    set.add(
        "B-1205 二十维证据化",
        evidence_complete(&evs) && !evidence_complete(&missing_instance) && !evidence_complete(&dup),
        "每维度有证据模板与实例——'感觉没问题'不构成证据（B-1205 达标线）",
    );
    // 3. 数量刚性：19 条与 21 条都拒（不是"至少 20"是"恰好 20"）。
    let short = [DimEvidence { dim: 1, template: true, instance: true }; 19];
    let over = [DimEvidence { dim: 1, template: true, instance: true }; 21];
    set.add(
        "B-1205 恰好二十条",
        !evidence_complete(&short) && !evidence_complete(&over),
        "20 维对账是等式不是下限——少一条多一条都答不了'全维度交证'",
    );
    // 4. 每日流水四段全自动（B-1206 达标线半边）。
    let full = DailyPipeline { automated: [true; 4], notify_on_fail: true };
    let manual_step = DailyPipeline { automated: [true, true, false, true], notify_on_fail: true };
    set.add(
        "B-1206 四段全自动",
        pipeline_ok(&full) && !pipeline_ok(&manual_step),
        "构建/测试/报告/通知无人值守——人工环节插进流水就不是每日流水",
    );
    // 5. 红绿通知（B-1206 达标线另一半）：红了必报。
    let silent = DailyPipeline { automated: [true; 4], notify_on_fail: false };
    set.add(
        "B-1206 红绿通知",
        pipeline_ok(&full) && !pipeline_ok(&silent),
        "绿报喜红报忧——只报喜的流水是装饰品，红了不知道是流水最大的失败（B-1206 达标线）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe22 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe22_dim_boundaries() {
        // 边界：1 与 20 合法，0/21/255 非法。
        assert!(dim_valid(1));
        assert!(dim_valid(20));
        assert!(!dim_valid(0));
        assert!(!dim_valid(21));
        assert!(!dim_valid(255));
        assert_eq!(DIMENSIONS, 20);
    }

    #[test]
    fn fe22_evidence_count_rigid() {
        // 恰好 20：19/21/0 条全拒；20 条双全过。
        let one = [DimEvidence { dim: 1, template: true, instance: true }];
        let empty: [DimEvidence; 0] = [];
        assert!(!evidence_complete(&one));
        assert!(!evidence_complete(&empty));
        let mut full = [DimEvidence { dim: 0, template: true, instance: true }; 20];
        let mut i = 0;
        while i < 20 {
            full[i].dim = (i as u8) + 1;
            i += 1;
        }
        assert!(evidence_complete(&full));
    }

    #[test]
    fn fe22_evidence_template_or_instance_missing() {
        // 模板缺/实例缺逐项独立红——空头与野路子都进不了验收。
        let mut full = [DimEvidence { dim: 0, template: true, instance: true }; 20];
        let mut i = 0;
        while i < 20 {
            full[i].dim = (i as u8) + 1;
            i += 1;
        }
        let mut no_tpl = full;
        no_tpl[0].template = false;
        assert!(!evidence_complete(&no_tpl));
        let mut no_inst = full;
        no_inst[7].instance = false;
        assert!(!evidence_complete(&no_inst));
    }

    #[test]
    fn fe22_pipeline_stages() {
        // 流水段穷举语义：automated 数组下标即段序（Build/Test/Report/Notify）。
        let p = DailyPipeline { automated: [true; 4], notify_on_fail: true };
        assert_eq!(p.automated.len(), 4);
        assert!(pipeline_ok(&p));
        // 全自动但静默：拒。半自动且报忧：拒。
        assert!(!pipeline_ok(&DailyPipeline { automated: [true; 4], notify_on_fail: false }));
        assert!(!pipeline_ok(&DailyPipeline { automated: [true, false, true, true], notify_on_fail: true }));
    }
}
