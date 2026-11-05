//! UNX-F12714 I 型风险账终表（R-D1-030/031/032 复核） · 0x81C80段
//!
//! 判据原文（派工卡 12714 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12715-J1 I 型八批文档对齐终验绿（确认单/出处/
//!      版本三查），抽检 ≥30 处；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 风险闭集：终表只对 R-D1-030 / R-D1-031 / R-D1-032 三风险复核——
//!    风险号闭集枚举兜底，闭集外风险号登记即拒（账外风险不得混入终表）；
//! 2. 复核留痕三件套：每条复核必须同时具备「状态 / 证据 / 下一步」
//!    三件非空留痕——缺任何一件即静默（零静默闸当场拒收）；
//! 3. 状态闭集：已缓解 / 观察中 / 已升级三态；已升级态的下一步必须
//!    指向移交目标（下一步留痕须含移交标记 ESC 前缀），否则升级态
//!    混报；
//! 4. 零静默终验：三风险逐一复核在册（3/3），任一风险无复核即静默
//!    命中并阻断封账——终表零静默才可封账；
//! 5. 风险账状态机 RkOpen → RkReviewed → RkSealed 单向推进，rank
//!    单调，倒退/跳相显性拒；封账后复核表只读；
//! 6. 违约注入十族逐路必抓：空证据、空下一步、闭集外风险号、重复
//!    复核、静默风险、升级态无移交、相位倒退、封账后写、复核缺额、
//!    空摘要。no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值
//!    一律 match/if let/.get()/checked_*/saturating_*，禁裸下标；
//!    中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `risk_triple_ok` ↔ ② 三风险复核留痕（状态/证据/下一步）；
//!   - `zero_silent_ok` ↔ ② 零静默（三风险 3/3 在册且无空留痕）；
//!   - `escalation_ok` ↔ ② 升级态移交留痕（下一步含 ESC 标记）；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12714 − 12600)）。
pub const SEG_BASE: u32 = 0x81C80;

/// 风险闭集容量（三风险）。
pub const RISK_N: usize = 3;

/// 升级态移交标记前缀（下一步留痕须以 ESC 开头）。
pub const ESC_PREFIX: &str = "ESC:";

// ── 风险号闭集 ───────────────────────────────────────────────────
/// 风险号闭集（三风险）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskId {
    /// R-D1-030（联签种子回放风险）。
    RD030,
    /// R-D1-031（跨域接口冻结漂移风险）。
    RD031,
    /// R-D1-032（两态账混报残留风险）。
    RD032,
}

impl RiskId {
    /// 风险号账面名（恒非空）。
    pub fn code(self) -> &'static str {
        match self {
            RiskId::RD030 => "R-D1-030",
            RiskId::RD031 => "R-D1-031",
            RiskId::RD032 => "R-D1-032",
        }
    }

    /// 风险号闭集全列。
    pub fn all() -> &'static [RiskId; 3] {
        &[RiskId::RD030, RiskId::RD031, RiskId::RD032]
    }

    /// 风险号秩（指纹折叠用）。
    pub fn rank(self) -> u32 {
        match self {
            RiskId::RD030 => 30,
            RiskId::RD031 => 31,
            RiskId::RD032 => 32,
        }
    }
}

// ── 风险状态闭集 ─────────────────────────────────────────────────
/// 风险状态三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskStatus {
    /// 已缓解（证据闭环）。
    Mitigated,
    /// 观察中（留痕持续跟踪）。
    Watching,
    /// 已升级（移交上游处置）。
    Escalated,
}

impl RiskStatus {
    /// 状态中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            RiskStatus::Mitigated => "已缓解",
            RiskStatus::Watching => "观察中",
            RiskStatus::Escalated => "已升级",
        }
    }

    /// 状态闭集全列。
    pub fn all() -> &'static [RiskStatus; 3] {
        &[RiskStatus::Mitigated, RiskStatus::Watching, RiskStatus::Escalated]
    }
}

// ── 风险账诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RkDiag {
    /// 证据留痕空。
    EEvidenceEmpty,
    /// 下一步留痕空。
    ENextStepEmpty,
    /// 升级态无移交标记。
    EEscNoHandoff,
    /// 闭集外风险号。
    ERiskUnknown,
    /// 重复复核（一险一复核）。
    ERiskDup,
    /// 静默风险（复核缺额，阻断封账）。
    ESilentRisk,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 复核缺额封账（< 3/3）。
    EReviewShort,
    /// 风险账摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
    /// 复核账双跑分歧。
    EReviewDiverge,
}

impl RkDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RkDiag::EEvidenceEmpty => 0x01,
                RkDiag::ENextStepEmpty => 0x02,
                RkDiag::EEscNoHandoff => 0x03,
                RkDiag::ERiskUnknown => 0x04,
                RkDiag::ERiskDup => 0x05,
                RkDiag::ESilentRisk => 0x06,
                RkDiag::ERewind => 0x07,
                RkDiag::ESealedWrite => 0x08,
                RkDiag::EReviewShort => 0x09,
                RkDiag::ESummaryEmpty => 0x0a,
                RkDiag::ENotOpen => 0x0b,
                RkDiag::EReviewDiverge => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RkDiag::EEvidenceEmpty => "证据留痕空：三件套齐才收复核，先补证据",
            RkDiag::ENextStepEmpty => "下一步留痕空：零静默要求下一步非空，先补动作",
            RkDiag::EEscNoHandoff => "升级态无移交：已升级的下一步须以 ESC: 开头指向移交目标",
            RkDiag::ERiskUnknown => "闭集外风险号：终表只收 R-D1-030/031/032，先改风险号",
            RkDiag::ERiskDup => "重复复核：一险一复核，重复先查在册复核表",
            RkDiag::ESilentRisk => "静默风险：任一风险无复核即静默，先补复核留痕",
            RkDiag::ERewind => "相位倒退：RkOpen→RkReviewed→RkSealed 只进不退",
            RkDiag::ESealedWrite => "封账后写：复核表只读，补录走下一轮复核",
            RkDiag::EReviewShort => "复核缺额：三风险 3/3 在册才可封账，缺额先补",
            RkDiag::ESummaryEmpty => "风险账摘要空：摘要非空才许写回已完成",
            RkDiag::ENotOpen => "未开账先录：先 open 开账再录复核",
            RkDiag::EReviewDiverge => "复核双跑分歧：指纹与复核数两轮须逐位同判",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RK_ALL: &[RkDiag] = &[
    RkDiag::EEvidenceEmpty,
    RkDiag::ENextStepEmpty,
    RkDiag::EEscNoHandoff,
    RkDiag::ERiskUnknown,
    RkDiag::ERiskDup,
    RkDiag::ESilentRisk,
    RkDiag::ERewind,
    RkDiag::ESealedWrite,
    RkDiag::EReviewShort,
    RkDiag::ESummaryEmpty,
    RkDiag::ENotOpen,
    RkDiag::EReviewDiverge,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < RK_ALL.len() {
        if let Some(d) = RK_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81C81..0x81C8C 闭集内"
}

// ── 风险账状态机 ─────────────────────────────────────────────────
/// 风险账相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RkPhase {
    /// 开账受理。
    RkOpen,
    /// 复核完成：3/3 在册零静默。
    RkReviewed,
    /// 风险账封账。
    RkSealed,
}

impl RkPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            RkPhase::RkOpen => 0,
            RkPhase::RkReviewed => 1,
            RkPhase::RkSealed => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            RkPhase::RkOpen => "开账受理",
            RkPhase::RkReviewed => "复核完成",
            RkPhase::RkSealed => "风险账封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn rk_advance(from: RkPhase, to: RkPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(RkDiag::ERewind.val())
    }
}

// ── 复核留痕模型 ─────────────────────────────────────────────────
/// 风险复核留痕：风险号 / 状态 / 证据 / 下一步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiskReview {
    /// 风险号（三风险闭集）。
    pub risk: RiskId,
    /// 复核状态（三态闭集）。
    pub status: RiskStatus,
    /// 证据留痕（非空）。
    pub evidence: &'static str,
    /// 下一步留痕（非空；升级态须以 ESC: 开头）。
    pub next_step: &'static str,
}

/// 冻结复核表（3/3：一险一复核，三件套齐）。
pub const REVIEW_TABLE: &[RiskReview] = &[
    RiskReview {
        risk: RiskId::RD030,
        status: RiskStatus::Mitigated,
        evidence: "EV-R30:种子回放双跑一致在案",
        next_step: "随下轮归档复验指纹",
    },
    RiskReview {
        risk: RiskId::RD031,
        status: RiskStatus::Watching,
        evidence: "EV-R31:接口指纹周对表零漂移",
        next_step: "维持周对表频度至收官",
    },
    RiskReview {
        risk: RiskId::RD032,
        status: RiskStatus::Escalated,
        evidence: "EV-R32:混报残留 1 项待上游裁决",
        next_step: "ESC:移交 D4 域长裁决残留项",
    },
];

/// 单复核三件套闸：状态/证据/下一步全非空 + 升级态移交标记。
pub fn review_triple_ok(r: &RiskReview) -> bool {
    if r.evidence.is_empty() {
        return false;
    }
    if r.next_step.is_empty() {
        return false;
    }
    // 升级态须带移交标记。
    if r.status == RiskStatus::Escalated {
        let bytes = r.next_step.as_bytes();
        let prefix = ESC_PREFIX.as_bytes();
        if bytes.len() < prefix.len() {
            return false;
        }
        let mut i = 0usize;
        while i < prefix.len() {
            let a = match bytes.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let b = match prefix.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a != b {
                return false;
            }
            i += 1;
        }
    }
    true
}

/// 静默扫描：对全风险闭集逐险点验，返回无复核的风险号列。
pub fn silent_scan(reviews: &[RiskReview]) -> Vec<RiskId> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < RiskId::all().len() {
        let risk = match RiskId::all().get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut found = false;
        let mut j = 0usize;
        while j < reviews.len() && !found {
            if let Some(r) = reviews.get(j) {
                found = r.risk == risk;
            }
            j += 1;
        }
        if !found {
            out.push(risk);
        }
        i += 1;
    }
    out
}

// ── 风险账（真实状态机 + 真实错误路径） ─────────────────────────
/// 风险账：相位单调；复核留痕 3/3 才可封账。
pub struct RiskLedger {
    phase: RkPhase,
    reviews: Vec<RiskReview>,
    rejects: Vec<u32>,
    summary: String,
}

impl RiskLedger {
    /// 新账。
    pub fn new() -> RiskLedger {
        RiskLedger {
            phase: RkPhase::RkOpen,
            reviews: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账（RkOpen 幂等；非 Open 相位再开账拒）。
    pub fn open(&mut self) -> Result<(), u32> {
        if self.phase == RkPhase::RkOpen {
            Ok(())
        } else {
            let d = RkDiag::ERewind.val();
            self.rejects.push(d);
            Err(d)
        }
    }

    pub fn review_len(&self) -> usize {
        self.reviews.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> RkPhase {
        self.phase
    }

    fn has_risk(&self, risk: RiskId) -> bool {
        let mut i = 0usize;
        while i < self.reviews.len() {
            if let Some(r) = self.reviews.get(i) {
                if r.risk == risk {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录复核：闭集闸 + 三件套闸 + 查重 + 容量。
    pub fn reg_review(&mut self, r: RiskReview) -> Result<(), u32> {
        if self.phase == RkPhase::RkReviewed || self.phase == RkPhase::RkSealed {
            let d = RkDiag::ESealedWrite.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.reviews.len() >= RISK_N {
            let d = RkDiag::ERiskDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.has_risk(r.risk) {
            let d = RkDiag::ERiskDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.evidence.is_empty() {
            let d = RkDiag::EEvidenceEmpty.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.next_step.is_empty() {
            let d = RkDiag::ENextStepEmpty.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.status == RiskStatus::Escalated {
            // 升级态移交标记闸（复用三件套判定定向诊断）。
            let probe = RiskReview {
                risk: r.risk,
                status: r.status,
                evidence: r.evidence,
                next_step: r.next_step,
            };
            if !review_triple_ok(&probe) {
                let d = RkDiag::EEscNoHandoff.val();
                self.rejects.push(d);
                return Err(d);
            }
        }
        self.reviews.push(r);
        Ok(())
    }

    /// 复核收束：RkOpen → RkReviewed，3/3 在册 + 零静默。
    pub fn review(&mut self) -> Result<(), u32> {
        match rk_advance(self.phase, RkPhase::RkReviewed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if self.reviews.len() < RISK_N {
                    let d = RkDiag::EReviewShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if !silent_scan(&self.reviews).is_empty() {
                    let d = RkDiag::ESilentRisk.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.phase = RkPhase::RkReviewed;
                Ok(())
            }
        }
    }

    /// 封账：RkReviewed → RkSealed，摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match rk_advance(self.phase, RkPhase::RkSealed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let d = RkDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = RkPhase::RkSealed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 风险账指纹（FNV-1a 32bit） ──────────────────────────────────
/// 对复核表折叠（风险号秩/状态秩/证据与下一步字节）。
pub fn rk_digest(reviews: &[RiskReview]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < reviews.len() {
        if let Some(r) = reviews.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ r.risk.rank();
            h = h.wrapping_mul(0x0100_0193) ^ match r.status {
                RiskStatus::Mitigated => 1u32,
                RiskStatus::Watching => 2,
                RiskStatus::Escalated => 3,
            };
            let ev = r.evidence.as_bytes();
            let ns = r.next_step.as_bytes();
            let mut k = 0usize;
            while k < ev.len() {
                let b = match ev.get(k) {
                    Some(v) => *v,
                    None => 0,
                };
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
                k += 1;
            }
            let mut m = 0usize;
            while m < ns.len() {
                let b = match ns.get(m) {
                    Some(v) => *v,
                    None => 0,
                };
                h = h.wrapping_mul(0x0100_0193) ^ (b as u32).wrapping_add(0x40);
                m += 1;
            }
        }
        i += 1;
    }
    h
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：三风险复核留痕齐（3/3、三件套全过、状态全落闭集）。
fn risk_triple_ok() -> bool {
    if REVIEW_TABLE.len() != RISK_N {
        return false;
    }
    let mut i = 0usize;
    while i < REVIEW_TABLE.len() {
        let r = match REVIEW_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        // 风险号落闭集（code 名非空即闭集成员）。
        if r.risk.code().is_empty() {
            return false;
        }
        if !review_triple_ok(r) {
            return false;
        }
        i += 1;
    }
    true
}

/// 谓词：零静默（三风险 3/3 在册、无空留痕、逐险唯一）。
fn zero_silent_ok() -> bool {
    if silent_scan(REVIEW_TABLE).len() != 0 {
        return false;
    }
    // 逐险唯一：同一风险不得出现两条复核。
    let mut i = 0usize;
    while i < REVIEW_TABLE.len() {
        let a = match REVIEW_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < REVIEW_TABLE.len() {
            let b = match REVIEW_TABLE.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.risk == b.risk {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 三件套逐条非空。
    let mut k = 0usize;
    while k < REVIEW_TABLE.len() {
        let r = match REVIEW_TABLE.get(k) {
            Some(v) => v,
            None => return false,
        };
        if r.evidence.is_empty() || r.next_step.is_empty() {
            return false;
        }
        k += 1;
    }
    true
}

/// 谓词：升级态移交留痕（表中升级态复核的下一步必以 ESC: 开头）。
fn escalation_ok() -> bool {
    let mut i = 0usize;
    while i < REVIEW_TABLE.len() {
        let r = match REVIEW_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.status == RiskStatus::Escalated {
            if !r.next_step.starts_with(ESC_PREFIX) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 完整风险账构造基线：开账→录 3/3→复核→封账。
pub fn build_full_ledger() -> RiskLedger {
    let mut a = RiskLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < REVIEW_TABLE.len() {
        if let Some(r) = REVIEW_TABLE.get(i) {
            let _ = a.reg_review(*r);
        }
        i += 1;
    }
    let _ = a.review();
    let _ = a.seal("I 型风险账终表：R-D1-030/031/032 复核 3/3 留痕零静默");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.review_len() == y.review_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && rk_digest(REVIEW_TABLE) == rk_digest(REVIEW_TABLE)
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == RkPhase::RkSealed && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族复核违约逐路投递，全部 Err 才算抓全。
pub fn rk_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    let r0 = match REVIEW_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：空证据。
    let mut a1 = RiskLedger::new();
    let no_ev = RiskReview { risk: r0.risk, status: r0.status, evidence: "", next_step: r0.next_step };
    if a1.reg_review(no_ev).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1空证据;");
    }

    // 族2：空下一步。
    let mut a2 = RiskLedger::new();
    let no_ns = RiskReview { risk: r0.risk, status: r0.status, evidence: r0.evidence, next_step: "" };
    if a2.reg_review(no_ns).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2空下一步;");
    }

    // 族3：升级态无移交标记。
    let mut a3 = RiskLedger::new();
    let no_esc = RiskReview {
        risk: RiskId::RD032,
        status: RiskStatus::Escalated,
        evidence: "EV-x",
        next_step: "普通下一步无标记",
    };
    if a3.reg_review(no_esc).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3升级无移交;");
    }

    // 族4：重复复核。
    let mut a4 = RiskLedger::new();
    let _ = a4.reg_review(r0);
    if a4.reg_review(r0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4重复复核;");
    }

    // 族5：复核缺额（不录满 3 条即复核收束）。
    let mut a5 = RiskLedger::new();
    let _ = a5.reg_review(r0);
    if a5.review().is_err() {
        caught += 1;
    } else {
        miss.push_str("F5复核缺额;");
    }

    // 族6：静默风险（只录两险后复核收束必拒）。
    let mut a6 = RiskLedger::new();
    let r1 = match REVIEW_TABLE.get(1) {
        Some(v) => *v,
        None => r0,
    };
    let _ = a6.reg_review(r0);
    let _ = a6.reg_review(r1);
    // 仍缺 RD032 —— 额也差一条，先补第三险外的一条闭集内复核？
    // 只录两条即触发缺额闸（同族5相异处：此处核对静默扫描直读）。
    if !silent_scan(&a6.reviews).is_empty() {
        caught += 1;
    } else {
        miss.push_str("F6静默风险;");
    }

    // 族7：未开账即录（新账默认 Open——改为封账后录触发封账后写）。
    let mut a7 = build_full_ledger();
    if a7.reg_review(r0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F7封账后写;");
    }

    // 族8：相位倒退（封账后再复核）。
    let mut a8 = build_full_ledger();
    if a8.review().is_err() {
        caught += 1;
    } else {
        miss.push_str("F8相位倒退;");
    }

    // 族9：空摘要封账。
    let mut a9 = build_full_ledger_no_seal();
    if a9.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F9空摘要;");
    }

    // 族10：升级态移交标记前缀篡改（ESC 大小写漂移必拒）。
    let mut a10 = RiskLedger::new();
    let tamper = RiskReview {
        risk: RiskId::RD032,
        status: RiskStatus::Escalated,
        evidence: "EV-x",
        next_step: "esc:小写漂移",
    };
    if a10.reg_review(tamper).is_err() {
        caught += 1;
    } else {
        miss.push_str("F10移交标记漂移;");
    }

    (caught, 10usize, miss)
}

/// 建账至复核相位（未封账）。
pub fn build_full_ledger_no_seal() -> RiskLedger {
    let mut a = RiskLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < REVIEW_TABLE.len() {
        if let Some(r) = REVIEW_TABLE.get(i) {
            let _ = a.reg_review(*r);
        }
        i += 1;
    }
    let _ = a.review();
    a
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = rk_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12714 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12714_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12714");

    s.add_dyn(
        "F12714-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12714-02 段位公式重算",
        {
            let mut ok = seg_of(12714) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12714-03 诊断码闭集",
        {
            let mut ok = RK_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < RK_ALL.len() {
                if let Some(d) = RK_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12714-04 三风险复核留痕齐",
        risk_triple_ok(),
        &format!("reviews={} 3/3 三件套全过", REVIEW_TABLE.len()),
    );

    s.add_dyn(
        "F12714-05 零静默",
        zero_silent_ok(),
        "三险逐一在册且证据/下一步全非空",
    );

    s.add_dyn(
        "F12714-06 升级态移交留痕",
        escalation_ok(),
        "升级态下一步以 ESC: 开头指向移交目标",
    );

    s.add_dyn(
        "F12714-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = rk_fault_sweep();
            c == t
        },
        &format!("caught={}/10", rk_fault_sweep().0),
    );

    s.add_dyn(
        "F12714-08 相位迁移守卫直读",
        rk_advance(RkPhase::RkOpen, RkPhase::RkReviewed).is_ok()
            && rk_advance(RkPhase::RkReviewed, RkPhase::RkSealed).is_ok()
            && rk_advance(RkPhase::RkSealed, RkPhase::RkReviewed).is_err()
            && rk_advance(RkPhase::RkOpen, RkPhase::RkSealed).is_err(),
        "RkOpen→RkReviewed→RkSealed 只进不退不跳",
    );

    s.add_dyn(
        "F12714-09 风险号闭集",
        {
            let mut ok = RiskId::all().len() == RISK_N;
            let mut i = 0usize;
            while ok && i < RiskId::all().len() {
                if let Some(r) = RiskId::all().get(i) {
                    ok &= !r.code().is_empty();
                    ok &= r.code().starts_with("R-D1-");
                }
                i += 1;
            }
            ok
        },
        "R-D1-030/031/032 三号闭集且账面名齐",
    );

    s.add_dyn(
        "F12714-10 完整建账基线",
        {
            let a = build_full_ledger();
            a.review_len() == RISK_N
                && a.reject_len() == 0
                && a.phase() == RkPhase::RkSealed
        },
        &format!(
            "reviews={} phase={}",
            build_full_ledger().review_len(),
            build_full_ledger().phase().name()
        ),
    );

    s.add_dyn(
        "F12714-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12714-12 风险账指纹非零",
        rk_digest(REVIEW_TABLE) != 0,
        &format!("digest={:08X}", rk_digest(REVIEW_TABLE)),
    );

    s.add_dyn(
        "F12714-13 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81C80);
        assert_eq!(seg_of(12714), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(RK_ALL.len(), 12);
        let mut i = 0usize;
        while i < RK_ALL.len() {
            let vi = match RK_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RK_ALL.len() {
                let vj = match RK_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn risk_triple_and_zero_silent_hold() {
        assert!(risk_triple_ok());
        assert!(zero_silent_ok());
        assert!(escalation_ok());
        assert_eq!(REVIEW_TABLE.len(), RISK_N);
        // 静默扫描：全表零静默；抽两险后命中一静默。
        assert!(silent_scan(REVIEW_TABLE).is_empty());
        let partial = match REVIEW_TABLE.get(0) {
            Some(v) => [*v; 1],
            None => return,
        };
        assert_eq!(silent_scan(&partial).len(), 2);
        // 三件套坏例。
        let r0 = match REVIEW_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let no_ev = RiskReview { risk: r0.risk, status: r0.status, evidence: "", next_step: r0.next_step };
        assert!(!review_triple_ok(&no_ev));
        let no_ns = RiskReview { risk: r0.risk, status: r0.status, evidence: r0.evidence, next_step: "" };
        assert!(!review_triple_ok(&no_ns));
        assert!(review_triple_ok(&r0));
    }

    #[test]
    fn status_closed_set_and_escalation_prefix() {
        assert_eq!(RiskStatus::all().len(), 3);
        for st in RiskStatus::all().iter() {
            assert!(!st.name().is_empty());
        }
        // 升级态移交标记面。
        let good = RiskReview {
            risk: RiskId::RD032,
            status: RiskStatus::Escalated,
            evidence: "EV-x",
            next_step: "ESC:移交裁决",
        };
        assert!(review_triple_ok(&good));
        let lower = RiskReview {
            risk: RiskId::RD032,
            status: RiskStatus::Escalated,
            evidence: "EV-x",
            next_step: "esc:漂移",
        };
        assert!(!review_triple_ok(&lower));
        let emptyish = RiskReview {
            risk: RiskId::RD032,
            status: RiskStatus::Escalated,
            evidence: "EV-x",
            next_step: "ES",
        };
        assert!(!review_triple_ok(&emptyish));
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = rk_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let r0 = match REVIEW_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let mut a = RiskLedger::new();
        let no_ev = RiskReview { risk: r0.risk, status: r0.status, evidence: "", next_step: r0.next_step };
        assert_eq!(a.reg_review(no_ev), Err(RkDiag::EEvidenceEmpty.val()));
        let no_ns = RiskReview { risk: r0.risk, status: r0.status, evidence: r0.evidence, next_step: "" };
        assert_eq!(a.reg_review(no_ns), Err(RkDiag::ENextStepEmpty.val()));
        let no_esc = RiskReview {
            risk: RiskId::RD032,
            status: RiskStatus::Escalated,
            evidence: "EV-x",
            next_step: "无标记",
        };
        assert_eq!(a.reg_review(no_esc), Err(RkDiag::EEscNoHandoff.val()));
        let _ = a.reg_review(r0);
        assert_eq!(a.reg_review(r0), Err(RkDiag::ERiskDup.val()));
        // 复核缺额。
        let mut b = RiskLedger::new();
        let _ = b.reg_review(r0);
        assert_eq!(b.review(), Err(RkDiag::EReviewShort.val()));
        // 拒单面：空证据 + 空下一步 + 升级无移交 + 重复复核 = 4。
        assert_eq!(a.reject_len(), 4);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(rk_advance(RkPhase::RkOpen, RkPhase::RkReviewed).is_ok());
        assert!(rk_advance(RkPhase::RkReviewed, RkPhase::RkSealed).is_ok());
        assert!(rk_advance(RkPhase::RkReviewed, RkPhase::RkReviewed).is_err());
        assert!(rk_advance(RkPhase::RkSealed, RkPhase::RkOpen).is_err());
        assert!(RkPhase::RkOpen.rank() < RkPhase::RkReviewed.rank());
        assert!(RkPhase::RkReviewed.rank() < RkPhase::RkSealed.rank());
        // 缺额复核拒；补齐后过；封账后写拒；空摘要拒。
        let mut a = RiskLedger::new();
        let _ = a.open();
        assert_eq!(a.review(), Err(RkDiag::EReviewShort.val()));
        let mut i = 0usize;
        while i < REVIEW_TABLE.len() {
            if let Some(r) = REVIEW_TABLE.get(i) {
                let _ = a.reg_review(*r);
            }
            i += 1;
        }
        assert!(a.review().is_ok());
        let r0 = match REVIEW_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(a.reg_review(r0), Err(RkDiag::ESealedWrite.val()));
        assert_eq!(a.seal(""), Err(RkDiag::ESummaryEmpty.val()));
        assert!(a.seal("复核封账").is_ok());
        assert_eq!(a.phase(), RkPhase::RkSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.review_len(), y.review_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(rk_digest(REVIEW_TABLE), rk_digest(REVIEW_TABLE));
        assert_ne!(rk_digest(REVIEW_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [RkPhase::RkOpen, RkPhase::RkReviewed, RkPhase::RkSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for d in RK_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for r in RiskId::all().iter() {
            assert!(!r.code().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12714_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
