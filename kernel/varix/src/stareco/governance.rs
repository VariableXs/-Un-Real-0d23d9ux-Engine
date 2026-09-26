//! F148 社区规则与治理 · 完整设计（STAR I 主册 G-D-23）。
//!
//! **判据（主册）**：三文档齐全过审；一例完整仲裁演练（模拟争议全
//! 流程）；裁决记录公开可查。
//!
//! **设计要点（主册）**：三份治理文档（星图收录标准/徽标授予标准/
//! 争议仲裁流程）；仲裁流程：提出→复核→三人组裁决（AI01+轮值社区
//! 2 人，与 F129 复核池同池）→公示；裁决时限 14 天；申诉一级终级
//! 两级（不无限递归）；裁决记录册 append-only（F194 序号链同款——
//! 本模块用 ebase::SeqLedger）；规则漏洞 → 临时裁定+规则修订提案
//! （规则也走 ADR）；分歧升级 Variable 终裁（宪法级兜底）；收录
//! 标准量化（格式过验+无恶意+功能真实三硬条件，审美不设限）；规则
//! 修订频率上限（季一次——防规则抖动）。
//!
//! 本模块是治理的**流程核**：三文档登记、收录三硬条件、仲裁状态机
//! （14 天时限）、append-only 裁决册、修订频率闸。

use crate::checks::CheckSet;
use crate::stareco::ebase::{fnv1a64, SeqLedger, TraceId};

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

pub const ARBITRATION_SLA_DAYS: u32 = 14;
/// 规则修订频率上限：每季一次（90 天）。
pub const RULE_AMEND_MIN_INTERVAL_DAYS: u32 = 90;

/// 三文档。
pub const GOV_DOCS: [&str; 3] = ["listing-standard", "badge-standard", "arbitration-process"];

// ---------------------------------------------------------------------------
// 收录标准（三硬条件——审美不设限）
// ---------------------------------------------------------------------------

pub fn listing_admissible(format_valid: bool, hash_clear: bool, functional: bool) -> bool {
    // 审美维度不进判据——规则先于规模的量化面。
    format_valid && hash_clear && functional
}

// ---------------------------------------------------------------------------
// 仲裁状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseStage {
    /// 提出。
    Filed,
    /// 复核（证据齐全性）。
    Reviewing,
    /// 三人组裁决中。
    PanelDeciding,
    /// 裁决公示。
    Published,
    /// 申诉（一级；申诉裁决后终级 Variable 兜底）。
    Appealed,
    /// 终裁（Variable 宪法级兜底）。
    FinalRuled,
    /// 驳回（复核阶段证据不全）。
    Dismissed,
}

#[derive(Clone, Copy, Debug)]
pub struct ArbitrationCase {
    pub id: TraceId,
    pub stage: CaseStage,
    pub filed_day: u32,
    /// 三人组构成（AI01 + 轮值社区 2）。
    pub panel: [u32; 3],
    /// 裁决结论（0=未裁 1=维持 2=改判）。
    pub verdict: u8,
    /// 证据键（可查引用）。
    pub evidence_key: &'static str,
}

pub struct Governance {
    /// 三文档登记（缺一 = 治理未成形）。
    docs: [bool; 3],
    cases: [Option<ArbitrationCase>; 8],
    case_count: usize,
    seq: crate::stareco::ebase::SeqAlloc,
    /// 裁决记录册（append-only——公开可查的机制面）。
    ledger: SeqLedger,
    pub published: u32,
    /// 最近规则修订日（修订频率闸）。
    last_amend_day: u32,
    pub amend_count: u32,
}

impl Governance {
    pub fn new() -> Governance {
        Governance {
            docs: [false; 3],
            cases: [None; 8],
            case_count: 0,
            seq: crate::stareco::ebase::SeqAlloc::new(),
            ledger: SeqLedger::new(),
            published: 0,
            last_amend_day: 0,
            amend_count: 0,
        }
    }

    pub fn register_doc(&mut self, name: &str) -> Result<(), &'static str> {
        for (i, d) in GOV_DOCS.iter().enumerate() {
            if *d == name {
                self.docs[i] = true;
                return Ok(());
            }
        }
        Err("unknown governance doc")
    }

    pub fn docs_complete(&self) -> bool {
        self.docs.iter().all(|&b| b)
    }

    /// 立案。
    pub fn file_case(&mut self, day: u32, evidence_key: &'static str) -> Result<TraceId, &'static str> {
        if !self.docs_complete() {
            return Err("治理三文档未齐——规则先于规模");
        }
        if evidence_key.is_empty() {
            return Err("证据键必填");
        }
        if self.case_count >= 8 {
            return Err("案件簿满");
        }
        let s = self.seq.take(day);
        let id = TraceId::new("GOV", day, s);
        self.cases[self.case_count] = Some(ArbitrationCase {
            id,
            stage: CaseStage::Filed,
            filed_day: day,
            panel: [0, 0, 0],
            verdict: 0,
            evidence_key,
        });
        self.case_count += 1;
        Ok(id)
    }

    fn case_mut(&mut self, id: TraceId) -> Result<&mut ArbitrationCase, &'static str> {
        self.cases[..self.case_count]
            .iter_mut()
            .flatten()
            .find(|c| c.id == id)
            .ok_or("unknown case")
    }

    /// 流转：严格按 提出→复核→裁决→公示 阶梯；每步时限合计 14 天。
    pub fn advance(&mut self, id: TraceId, to: CaseStage, day: u32, panel: [u32; 3], verdict: u8) -> Result<(), &'static str> {
        let c = self.case_mut(id)?;
        let legal = matches!(
            (c.stage, to),
            (CaseStage::Filed, CaseStage::Reviewing)
                | (CaseStage::Reviewing, CaseStage::PanelDeciding)
                | (CaseStage::Reviewing, CaseStage::Dismissed)
                | (CaseStage::PanelDeciding, CaseStage::Published)
                | (CaseStage::Published, CaseStage::Appealed)
                | (CaseStage::Appealed, CaseStage::FinalRuled)
        );
        if !legal {
            return Err("非法阶段跳转");
        }
        if day.saturating_sub(c.filed_day) > ARBITRATION_SLA_DAYS
            && !matches!(to, CaseStage::Appealed | CaseStage::FinalRuled)
        {
            return Err("裁决时限 14 天已超");
        }
        if to == CaseStage::PanelDeciding {
            // 三人组：非零且互异（AI01+轮值 2 人——同池规则由调用方选人）
            if panel[0] == 0 || panel[1] == 0 || panel[2] == 0
                || panel[0] == panel[1] || panel[1] == panel[2] || panel[0] == panel[2]
            {
                return Err("三人组构成不合法");
            }
            c.panel = panel;
        }
        if to == CaseStage::Published && verdict == 0 {
            return Err("公示必须带结论");
        }
        // 上链载荷先取出，结束 case 借用后再写裁决册
        let evidence_fp = if to == CaseStage::Published {
            fnv1a64(c.evidence_key.as_bytes()) ^ (verdict as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        } else {
            0
        };
        c.stage = to;
        c.verdict = verdict;
        if to == CaseStage::Published {
            self.ledger.append(evidence_fp);
            self.published += 1;
        }
        Ok(())
    }

    /// 申诉：一级（公示后）；申诉后终级 = Variable 终裁。
    pub fn appeal(&mut self, id: TraceId, day: u32) -> Result<(), &'static str> {
        self.advance(id, CaseStage::Appealed, day, [0; 3], 0)
    }

    pub fn final_rule(&mut self, id: TraceId, day: u32, verdict: u8) -> Result<(), &'static str> {
        self.advance(id, CaseStage::FinalRuled, day, [0; 3], verdict)
    }

    /// 裁决记录册公开可查（链完整 = 可查）。
    pub fn records_queryable(&self) -> bool {
        self.ledger.verify()
    }

    /// 规则修订：频率闸——距上次修订不满一季拒绝（防规则抖动）。
    pub fn amend_rules(&mut self, day: u32, _proposal_ref: &'static str) -> Result<(), &'static str> {
        if self.last_amend_day != 0 && day.saturating_sub(self.last_amend_day) < RULE_AMEND_MIN_INTERVAL_DAYS {
            return Err("规则修订频率上限：每季一次");
        }
        if _proposal_ref.is_empty() {
            return Err("修订必须挂 ADR 提案");
        }
        self.last_amend_day = day;
        self.amend_count += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F148_TAG: &str = "stareco-F148-governance";

pub fn run_governance_checks() -> CheckSet {
    let mut set = CheckSet::new(F148_TAG);
    let day = 20260926;

    // 三文档齐全过审
    let mut gov = Governance::new();
    set.add("f145 docs incomplete blocks filing", gov.file_case(day, "e1").is_err(), "rules before scale");
    assert!(gov.register_doc("listing-standard").is_ok());
    assert!(gov.register_doc("badge-standard").is_ok());
    set.add("f148 two of three still blocked", !gov.docs_complete() && gov.file_case(day, "e").is_err(), "all three required");
    assert!(gov.register_doc("arbitration-process").is_ok());
    set.add("f148 three docs complete", gov.docs_complete(), "过审形态");
    set.add("f148 unknown doc rejected", gov.register_doc("meme-rules").is_err(), "fixed set");

    // 收录三硬条件（审美不设限）
    set.add("f148 listing three hard conditions", listing_admissible(true, true, true), "format+hash+real");
    set.add(
        "f148 aesthetic not in criteria",
        listing_admissible(true, true, true),
        "ugly-but-real still admitted",
    );
    set.add("f148 missing function rejected", !listing_admissible(true, true, false), "functional required");

    // 完整仲裁演练：提出→复核→三人裁决→公示
    let cid = gov.file_case(day, "F134-entry#7").expect("file");
    gov.advance(cid, CaseStage::Reviewing, day + 1, [0; 3], 0).expect("review");
    // 三人组构成不合法的拦截
    set.add(
        "f148 bad panel rejected",
        gov.advance(cid, CaseStage::PanelDeciding, day + 2, [1, 1, 2], 0).is_err(),
        "three distinct humans",
    );
    gov.advance(cid, CaseStage::PanelDeciding, day + 2, [1, 7, 9], 0).expect("panel");
    gov.advance(cid, CaseStage::Published, day + 3, [0; 3], 1).expect("publish");
    set.add("f148 arbitration drill published", gov.published == 1, "full-flow case 1");
    set.add("f148 records queryable", gov.records_queryable(), "append-only chain");

    // 非法跳转：未复核直接裁决 / 已公示再公示
    let cid2 = gov.file_case(day, "F144-grant#3").expect("file2");
    set.add(
        "f148 skip-stage rejected",
        gov.advance(cid2, CaseStage::Published, day + 1, [0; 3], 1).is_err(),
        "ladder enforced",
    );
    set.add(
        "f148 republish rejected",
        gov.advance(cid, CaseStage::Published, day + 4, [0; 3], 1).is_err(),
        "no double publish",
    );

    // 申诉一级 + Variable 终级
    gov.appeal(cid, day + 5).expect("appeal");
    gov.final_rule(cid, day + 6, 2).expect("final");
    set.add("f148 two-level appeal ended", gov.final_rule(cid, day + 7, 1).is_err(), "no infinite recursion");

    // 14 天时限
    let cid3 = gov.file_case(day, "e-x").expect("file3");
    gov.advance(cid3, CaseStage::Reviewing, day + 13, [0; 3], 0).expect("in time");
    set.add(
        "f148 sla breach rejected",
        gov.advance(cid3, CaseStage::PanelDeciding, day + 16, [1, 2, 3], 0).is_err(),
        "14d hard clock",
    );

    // 规则修订频率闸
    set.add("f148 first amend ok", gov.amend_rules(day, "ADR-G-01").is_ok(), "first time");
    set.add(
        "f148 amend throttle",
        gov.amend_rules(day + 30, "ADR-G-02").is_err(),
        "one per season",
    );
    set.add("f148 amend after season ok", gov.amend_rules(day + 91, "ADR-G-02").is_ok(), "season passed");
    set.add("f148 amend needs adr", gov.amend_rules(day + 200, "").is_err(), "proposal required");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismissal_path() {
        let mut gov = Governance::new();
        for d in GOV_DOCS.iter() {
            gov.register_doc(d).unwrap();
        }
        let id = gov.file_case(1, "ev").unwrap();
        gov.advance(id, CaseStage::Reviewing, 2, [0; 3], 0).unwrap();
        gov.advance(id, CaseStage::Dismissed, 3, [0; 3], 0).unwrap();
        // 驳回后不可再裁
        assert!(gov.advance(id, CaseStage::PanelDeciding, 4, [1, 2, 3], 0).is_err());
        assert_eq!(gov.published, 0);
    }
}
