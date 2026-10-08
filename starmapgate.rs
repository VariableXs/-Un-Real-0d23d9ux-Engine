//! 星图发布流水线门禁（WP-305 · B-2102 缺料星卡零上架）。
//!
//! MD2 篇 21.2：判例框架出报告 → 报告合规检查（判例覆盖度、实测指标齐备
//! 性、偏差解释存在性）→ 人工确认（唯一人工环节）→ 目录合入（版本递增）。
//! 上架物料的完整性与一致性脚本化：星卡缺截图、指标超期（复评日期过期）、
//! 版本号与实际不符——**任何一项不合拒绝进入人工环节**。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 提交物料（模型面）
// ---------------------------------------------------------------------------

/// 提交物料面（判例报告 + 上架物料的完整性字段）。
#[derive(Clone, Copy)]
pub struct Submission {
    /// 判例总数（覆盖度的分母）。
    pub case_total: u16,
    /// 已执行判例数（覆盖度的分子）。
    pub case_done: u16,
    /// 实测指标齐备（带单位的指标全在——齐备性）。
    pub metrics_complete: bool,
    /// 偏差解释存在（有偏差必须有解释——诚实面）。
    pub deviation_explained: bool,
    /// 截图张数（缺截图的星卡是裸奔的星卡）。
    pub screenshots: u8,
    /// 复评到期日（版本日计）。
    pub review_due: u32,
    /// 当前版本日。
    pub today: u32,
    /// 声称版本号。
    pub ver_claimed: u32,
    /// 实际版本号。
    pub ver_actual: u32,
}

/// 物料完整性三查（脚本化预检——**任何一项不合拒绝进入人工环节**）：
/// 截图在册、复评未过期、版本号与实际一致。
pub fn material_ok(s: &Submission) -> bool {
    s.screenshots > 0 && s.review_due > s.today && s.ver_claimed == s.ver_actual
}

/// 报告合规检查三要素：判例覆盖度（全跑且至少一条）+ 实测指标齐备性 +
/// 偏差解释存在性。
pub fn compliance_ok(s: &Submission) -> bool {
    s.case_total > 0 && s.case_done == s.case_total && s.metrics_complete && s.deviation_explained
}

// ---------------------------------------------------------------------------
// 流水线四段（穷举）与门禁
// ---------------------------------------------------------------------------

/// 流水线门禁段（穷举——门禁只裁三站：入口"已收到"归反馈状态面管，
/// 门禁只管过检/合入/拒绝）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateStage {
    /// 已过检（物料+合规双过，待人工确认）。
    Screened,
    /// 已合入（人工确认过，版本递增）。
    Merged,
    /// 已拒绝（物料或合规失守——拒因在账）。
    Rejected,
}

/// 门禁总入口（**B-2102 达标线：缺料星卡零上架**）——物料三查、合规三要
/// 素、人工确认三道门依序过，任一失守停在当前段且版本不动；全过才合入
/// 并进一号（与 starmapdir::version_step 同源：一变一号）。
pub fn gate(s: &Submission, human_confirmed: bool, cur_ver: u32) -> (GateStage, u32) {
    if !material_ok(s) {
        return (GateStage::Rejected, cur_ver);
    }
    if !compliance_ok(s) {
        return (GateStage::Rejected, cur_ver);
    }
    if !human_confirmed {
        return (GateStage::Screened, cur_ver);
    }
    (GateStage::Merged, cur_ver + 1)
}

// ---------------------------------------------------------------------------
// CheckSet（B-2102 · 5 项）
// ---------------------------------------------------------------------------

pub fn run_starmapgate_checks() -> CheckSet {
    let mut set = CheckSet::new("B-2102 星图发布流水线门禁");
    // 1. 物料完整性三查：缺截图/指标超期/版本不符——任何一项拒进人工。
    let good = Submission {
        case_total: 12,
        case_done: 12,
        metrics_complete: true,
        deviation_explained: true,
        screenshots: 4,
        review_due: 100,
        today: 90,
        ver_claimed: 5,
        ver_actual: 5,
    };
    let no_shot = Submission { screenshots: 0, ..good };
    let overdue = Submission { review_due: 90, today: 90, ..good };
    let ver_drift = Submission { ver_claimed: 6, ..good };
    set.add(
        "B-2102 物料完整性三查",
        material_ok(&good)
            && !material_ok(&no_shot)
            && !material_ok(&overdue)
            && !material_ok(&ver_drift),
        "缺截图、指标超期、版本不符——任何一项不合拒绝进入人工环节",
    );
    // 2. 合规检查三要素：覆盖度/齐备性/偏差解释。
    let no_cov = Submission { case_done: 11, ..good };
    let no_metric = Submission { metrics_complete: false, ..good };
    let no_dev = Submission { deviation_explained: false, ..good };
    let zero_total = Submission { case_total: 0, case_done: 0, ..good };
    set.add(
        "B-2102 合规三要素",
        compliance_ok(&good)
            && !compliance_ok(&no_cov)
            && !compliance_ok(&no_metric)
            && !compliance_ok(&no_dev)
            && !compliance_ok(&zero_total),
        "判例覆盖度+实测指标齐备性+偏差解释存在性——三要素缺一不合",
    );
    // 3. 人工确认是唯一人工环节：无确认不合入（停在 Screened）。
    let (st_no, ver_no) = gate(&good, false, 7);
    set.add(
        "B-2102 人工确认门",
        st_no == GateStage::Screened && ver_no == 7,
        "合规检查再绿，无人工确认也不合入——唯一人工环节是门不是章",
    );
    // 4. 合入版本递增：合入进一号，拒绝版本不动。
    let (st_ok, ver_ok) = gate(&good, true, 7);
    let (st_rej, ver_rej) = gate(&no_shot, true, 7);
    set.add(
        "B-2102 合入版本递增",
        st_ok == GateStage::Merged && ver_ok == 8 && st_rej == GateStage::Rejected && ver_rej == 7,
        "每次变更产生一个版本号（一变一号）——拒绝不占号",
    );
    // 5. 端到端零上架（B-2102 达标线）：物料/合规/人工三道门的合取。
    let (st_e2e, _) = gate(&no_metric, false, 7);
    set.add(
        "B-2102 缺料零上架",
        st_e2e == GateStage::Rejected,
        "物料与合规双失守直落拒绝——缺料星卡零上架（B-2102 达标线）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe15 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn base() -> Submission {
        Submission {
            case_total: 10,
            case_done: 10,
            metrics_complete: true,
            deviation_explained: true,
            screenshots: 3,
            review_due: 200,
            today: 100,
            ver_claimed: 4,
            ver_actual: 4,
        }
    }

    #[test]
    fn fe15_material_checks() {
        // 三查逐项独立拒：只坏一项也是拒。
        assert!(material_ok(&base()));
        assert!(!material_ok(&Submission { screenshots: 0, ..base() }));
        assert!(!material_ok(&Submission { review_due: 100, today: 100, ..base() }));
        assert!(!material_ok(&Submission { ver_claimed: 5, ..base() }));
        // 复评"当天到期"即超期（review_due > today 严格不等）。
        assert!(!material_ok(&Submission { review_due: 101, today: 101, ..base() }));
    }

    #[test]
    fn fe15_compliance_three() {
        // 三要素逐项独立拒；零判例的"全覆盖"是空集全覆盖——拒。
        assert!(compliance_ok(&base()));
        assert!(!compliance_ok(&Submission { case_done: 9, ..base() }));
        assert!(!compliance_ok(&Submission { metrics_complete: false, ..base() }));
        assert!(!compliance_ok(&Submission { deviation_explained: false, ..base() }));
        assert!(!compliance_ok(&Submission { case_total: 0, case_done: 0, ..base() }));
    }

    #[test]
    fn fe15_human_gate_and_versions() {
        // 无人工确认：停在 Screened，版本不动。
        let (s1, v1) = gate(&base(), false, 10);
        assert_eq!(s1, GateStage::Screened);
        assert_eq!(v1, 10);
        // 有人工确认：合入进一号。
        let (s2, v2) = gate(&base(), true, 10);
        assert_eq!(s2, GateStage::Merged);
        assert_eq!(v2, 11);
        // 物料坏：直落 Rejected，人工确认也救不回（门序不可跳）。
        let mut bad = base();
        bad.screenshots = 0;
        let (s3, v3) = gate(&bad, true, 10);
        assert_eq!(s3, GateStage::Rejected);
        assert_eq!(v3, 10);
    }

    #[test]
    fn fe15_e2e_zero_listing() {
        // 端到端：合规缺一项 + 无人工确认——依然拒绝，零上架。
        let mut s = base();
        s.deviation_explained = false;
        let (st, v) = gate(&s, false, 42);
        assert_eq!(st, GateStage::Rejected);
        assert_eq!(v, 42);
        // 全绿全过：恰好进一号。
        let (st2, v2) = gate(&base(), true, 42);
        assert_eq!(st2, GateStage::Merged);
        assert_eq!(v2, 43);
    }
}
