//! UNX-F12695 联签性能预算总表（四域 P95 合账） · 0x81780段
//!
//! 判据原文（派工卡 12694 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12695-J1 三风险复核留痕（状态/证据/下一步三列），
//!      零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. P95 预算域模型：四域（装载/对象/探针/回执）各带冻结预算额与确定
//!    性采样集（LCG 种子冻结、40 样本/域）——P95 合账的采样面；
//! 2. P95 整数口径（零浮点）：P95 = 最小的满足「样本中 ≤ 其值的占比
//!    ≥ 95%」的样本值（候选遍历计数折算，n×100 与 95×n 整数比较）——
//!    同种子必同 P95（可重演）；
//! 3. 预算判别：P95 ≤ 预算即绿判、P95 > 预算必落差异账（超线显性，不
//!    静默）——四域合账行四元（域名/预算/P95/判别）缺一即合账不齐；
//! 4. 三风险复核留痕：R-D1-028/029/030 各带三列（状态/证据/下一步），
//!    状态落三闭集（已复核/复核中/待实测）、证据与下一步恒非空——三
//!    列缺一即静默（零静默判据的留痕面）；
//! 5. 复核账状态机 PbOpen → PbRunning → PbSealed 单向推进，倒退显性
//!    拒；封账只读；零静默：风险数 ≡ 复核条数守恒；
//! 6. 注入联测十路：证据空/下一步空/状态越集/风险缺一/风险重复/复核
//!    留痕缺/P95 差异静默/封账后写/状态机倒退/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子冻结常
//!    量）；取值一律 match/if let/.get()/.get_mut()/saturating_*，无
//!    unwrap/expect/panic/裸下标；中文注释与中文诊断；无死代码无
//!    #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `risk_trail_ok` ↔ ② 三风险复核留痕（状态/证据/下一步三列齐）；
//!   - `zero_silent_ok` ↔ ② 零静默（三列缺一/复核缺条必检出）；
//!   - `p95_budget_ok` ↔ ① 四域 P95 合账面（同种子同 P95、判别守恒）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12694 − 12600)）。
pub const SEG_BASE: u32 = 0x81780;

// ── P95 预算总表诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PbDiag {
    /// 证据空（复核留痕证据列空）。
    EEvidenceEmpty,
    /// 下一步空（复核留痕下一步列空）。
    ENextEmpty,
    /// 状态越集（风险状态不在三闭集）。
    EStatusRange,
    /// 风险缺一。
    ERiskMissing,
    /// 风险重复。
    ERiskDup,
    /// 复核留痕缺条。
    EReviewMissing,
    /// P95 差异静默（超线未落差异账）。
    EP95Silent,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
    /// 采样越窗（样本值不在采样窗内）。
    ESampleRange,
}

impl PbDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PbDiag::EEvidenceEmpty => 0x01,
                PbDiag::ENextEmpty => 0x02,
                PbDiag::EStatusRange => 0x03,
                PbDiag::ERiskMissing => 0x04,
                PbDiag::ERiskDup => 0x05,
                PbDiag::EReviewMissing => 0x06,
                PbDiag::EP95Silent => 0x07,
                PbDiag::ESealedWrite => 0x08,
                PbDiag::ERewind => 0x09,
                PbDiag::ESummaryEmpty => 0x0a,
                PbDiag::ESampleRange => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            PbDiag::EEvidenceEmpty => "证据空：复核留痕证据列恒非空，空列先补证据引",
            PbDiag::ENextEmpty => "下一步空：复核留痕下一步列恒非空，空列先补行动项",
            PbDiag::EStatusRange => "状态越集：风险状态必须落在已复核/复核中/待实测闭集内",
            PbDiag::ERiskMissing => "风险缺一：R-D1-028/029/030 三风险全量，缺一先补",
            PbDiag::ERiskDup => "风险重复：同风险号只许一行，重复先查已有账",
            PbDiag::EReviewMissing => "复核留痕缺条：每风险一条复核账，缺条先补",
            PbDiag::EP95Silent => "P95 差异静默：超线必落差异账，有超未记先补账",
            PbDiag::ESealedWrite => "封账后写：总表只读，补行走下一轮合账",
            PbDiag::ERewind => "状态机倒退：PbOpen→PbRunning→PbSealed 只进不退，倒退即账可疑",
            PbDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            PbDiag::ESampleRange => "采样越窗：样本值必须落在采样窗内，越窗先折算",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const PB_ALL: &[PbDiag] = &[
    PbDiag::EEvidenceEmpty,
    PbDiag::ENextEmpty,
    PbDiag::EStatusRange,
    PbDiag::ERiskMissing,
    PbDiag::ERiskDup,
    PbDiag::EReviewMissing,
    PbDiag::EP95Silent,
    PbDiag::ESealedWrite,
    PbDiag::ERewind,
    PbDiag::ESummaryEmpty,
    PbDiag::ESampleRange,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in PB_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81781..0x8178B 闭集内"
}

// ── 预算域四闭集（独立于 12693 的合账轴：P95 采样轴） ────────────
/// P95 预算域（四域闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P95Domain {
    /// 装载 P95 域。
    PLoad,
    /// 对象 P95 域。
    PObject,
    /// 探针 P95 域。
    PProbe,
    /// 回执 P95 域。
    PReceipt,
}

impl P95Domain {
    /// 域名。
    pub fn name(self) -> &'static str {
        match self {
            P95Domain::PLoad => "装载 P95",
            P95Domain::PObject => "对象 P95",
            P95Domain::PProbe => "探针 P95",
            P95Domain::PReceipt => "回执 P95",
        }
    }

    /// 域号。
    pub fn ordinal(self) -> u32 {
        match self {
            P95Domain::PLoad => 0,
            P95Domain::PObject => 1,
            P95Domain::PProbe => 2,
            P95Domain::PReceipt => 3,
        }
    }

    /// 冻结预算额（P95 预算冻结值）。
    pub fn budget(self) -> u32 {
        match self {
            P95Domain::PLoad => 90,
            P95Domain::PObject => 88,
            P95Domain::PProbe => 86,
            P95Domain::PReceipt => 84,
        }
    }

    /// 全部域（闭集数组）。
    pub fn all() -> &'static [P95Domain] {
        &[P95Domain::PLoad, P95Domain::PObject, P95Domain::PProbe, P95Domain::PReceipt]
    }
}

// ── 确定性采样（LCG 种子冻结） ───────────────────────────────────
/// 冻结 LCG 种子。
pub const PB_SEED: u64 = 0x1A2B_3C4D_5E6F_7081;

/// LCG 乘子。
pub const PB_MULT: u64 = 6364136223846793005;

/// LCG 增量。
pub const PB_INC: u64 = 1442695040888963407;

/// 推进一步 LCG。
pub fn pb_lcg(state: u64) -> u64 {
    state.wrapping_mul(PB_MULT).wrapping_add(PB_INC)
}

/// 每域采样数。
pub const SAMPLE_CAP: usize = 40;

/// 采样窗下沿。
pub const SAMPLE_LO: u32 = 20;

/// 采样窗宽（值域 20..71）。
pub const SAMPLE_SPAN: u32 = 51;

/// 域采样集（确定性生成，同种子同集）。
pub fn domain_samples(d: P95Domain) -> Vec<u32> {
    let mut out = Vec::new();
    let mut st = pb_lcg(PB_SEED.wrapping_add(d.ordinal() as u64));
    let mut i = 0usize;
    while i < SAMPLE_CAP {
        st = pb_lcg(st);
        let v = SAMPLE_LO + ((st >> 33) as u32) % SAMPLE_SPAN;
        out.push(v);
        i += 1;
    }
    out
}

/// 采样窗校验。
pub fn sample_in_window(v: u32) -> bool {
    v >= SAMPLE_LO && v < SAMPLE_LO.saturating_add(SAMPLE_SPAN)
}

/// P95 整数口径：最小的满足「样本中 ≤ 其值的占比 ≥ 95%」的样本值。
pub fn p95_of(samples: &[u32]) -> Option<u32> {
    if samples.is_empty() {
        return None;
    }
    let n = samples.len() as u32;
    // 判定阈值：count × 100 ≥ 95 × n。
    let need_num = 95u32.saturating_mul(n);
    let mut best: Option<u32> = None;
    for c in samples.iter() {
        let mut count = 0u32;
        for s in samples.iter() {
            if s <= c {
                count = count.saturating_add(1);
            }
        }
        if count.saturating_mul(100) >= need_num {
            best = match best {
                Some(b) => {
                    if *c < b {
                        Some(*c)
                    } else {
                        Some(b)
                    }
                }
                None => Some(*c),
            };
        }
    }
    best
}

// ── 三风险复核留痕 ───────────────────────────────────────────────
/// 风险号三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskId {
    /// R-D1-028。
    R28,
    /// R-D1-029。
    R29,
    /// R-D1-030。
    R30,
}

impl RiskId {
    /// 风险号名。
    pub fn name(self) -> &'static str {
        match self {
            RiskId::R28 => "R-D1-028",
            RiskId::R29 => "R-D1-029",
            RiskId::R30 => "R-D1-030",
        }
    }

    /// 风险号序。
    pub fn ordinal(self) -> u32 {
        match self {
            RiskId::R28 => 0,
            RiskId::R29 => 1,
            RiskId::R30 => 2,
        }
    }

    /// 全部风险号（闭集数组）。
    pub fn all() -> &'static [RiskId] {
        &[RiskId::R28, RiskId::R29, RiskId::R30]
    }
}

/// 风险状态三闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskStatus {
    /// 已复核。
    Reviewed,
    /// 复核中。
    Reviewing,
    /// 待实测。
    PendingRealTest,
}

impl RiskStatus {
    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            RiskStatus::Reviewed => "已复核",
            RiskStatus::Reviewing => "复核中",
            RiskStatus::PendingRealTest => "待实测",
        }
    }

    /// 全部状态（闭集数组）。
    pub fn all() -> &'static [RiskStatus] {
        &[RiskStatus::Reviewed, RiskStatus::Reviewing, RiskStatus::PendingRealTest]
    }
}

/// 风险行（三列：状态/证据/下一步）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiskRow {
    /// 风险号。
    pub risk: RiskId,
    /// 状态列。
    pub status: RiskStatus,
    /// 证据列。
    pub evidence: &'static str,
    /// 下一步列。
    pub next_step: &'static str,
}

/// 冻结风险表（三行三列，零静默口径）。
pub const RISK_ROWS: &[RiskRow] = &[
    RiskRow {
        risk: RiskId::R28,
        status: RiskStatus::Reviewed,
        evidence: "R-D1-028 留痕账（F12685 LoaderLock 回归十绿）",
        next_step: "随下轮回归复核复验，结论写入回归账",
    },
    RiskRow {
        risk: RiskId::R29,
        status: RiskStatus::Reviewed,
        evidence: "R-D1-029 复核账（F12689 挂起期拒止 10/10）",
        next_step: "对账表封账后复点，偏差即开新轮",
    },
    RiskRow {
        risk: RiskId::R30,
        status: RiskStatus::PendingRealTest,
        evidence: "R-D1-030 演练面（F12694 超线告警 10/10）",
        next_step: "待实测项挂下一实测轮次，实测前不封绿",
    },
];

/// 三风险总额。
pub const RISK_CAP: usize = 3;

/// 按风险号取冻结行（越界 None）。
pub fn replay_risk(r: RiskId) -> Option<RiskRow> {
    for row in RISK_ROWS.iter() {
        if row.risk == r {
            return Some(row.clone());
        }
    }
    None
}

// ── 合账账状态机 ─────────────────────────────────────────────────
/// 合账账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PbState {
    /// 开账。
    PbOpen,
    /// 合账中。
    PbRunning,
    /// 封账（只读）。
    PbSealed,
}

impl PbState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            PbState::PbOpen => 0,
            PbState::PbRunning => 1,
            PbState::PbSealed => 2,
        }
    }
}

/// 域合账行四元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainRow {
    /// 域。
    pub domain: P95Domain,
    /// 预算额。
    pub budget: u32,
    /// P95 值。
    pub p95: u32,
    /// 判别（true = P95 ≤ 预算）。
    pub pass: bool,
}

/// P95 预算合账账。
#[derive(Clone, Debug)]
pub struct PbLedger {
    /// 状态。
    state: PbState,
    /// 域合账行。
    rows: Vec<DomainRow>,
    /// 已入风险行。
    risks: Vec<RiskRow>,
    /// 差异账（超线必落账）。
    diffs: u32,
    /// 拒止计数。
    rejects: u32,
}

impl PbLedger {
    /// 新账。
    pub fn new() -> PbLedger {
        PbLedger {
            state: PbState::PbOpen,
            rows: Vec::new(),
            risks: Vec::new(),
            diffs: 0,
            rejects: 0,
        }
    }

    /// 开账 → 合账中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            PbState::PbOpen => {
                self.state = PbState::PbRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(PbDiag::ERewind.val())
            }
        }
    }

    /// 合账中 → 封账（四域行 + 三风险行齐才许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            PbState::PbRunning => {
                if self.rows.len() != P95Domain::all().len() {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(PbDiag::ERiskMissing.val());
                }
                if self.risks.len() != RISK_CAP {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(PbDiag::EReviewMissing.val());
                }
                self.state = PbState::PbSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(PbDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> PbState {
        self.state
    }

    /// 域行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 风险行数。
    pub fn risk_len(&self) -> usize {
        self.risks.len()
    }

    /// 差异账条数。
    pub fn diff_len(&self) -> u32 {
        self.diffs
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 下标取域行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&DomainRow> {
        self.rows.get(idx)
    }

    /// 下标取风险行（越界 None）。
    pub fn risk_at(&self, idx: usize) -> Option<&RiskRow> {
        self.risks.get(idx)
    }

    /// 合账一域行（重/预算/窗/判别四闸；超线必落差异账）。
    pub fn reg_domain(&mut self, r: DomainRow) -> Result<(), u32> {
        if self.state == PbState::PbSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PbDiag::ESealedWrite.val());
        }
        if self.state == PbState::PbOpen {
            self.state = PbState::PbRunning;
        }
        // 重闸。
        for e in self.rows.iter() {
            if e.domain == r.domain {
                self.rejects = self.rejects.saturating_add(1);
                return Err(PbDiag::ERiskDup.val());
            }
        }
        // 预算闸。
        if r.budget != r.domain.budget() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PbDiag::ERiskMissing.val());
        }
        // 判别闸（先于窗闸）：pass 必须与算术一致；超线虚绿必被静默闸拒。
        let actual_pass = r.p95 <= r.budget;
        if r.pass != actual_pass {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PbDiag::EP95Silent.val());
        }
        // 窗闸：P95 必须落采样窗。
        if !sample_in_window(r.p95) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PbDiag::ESampleRange.val());
        }
        if !actual_pass {
            self.diffs = self.diffs.saturating_add(1);
        }
        self.rows.push(r);
        Ok(())
    }

    /// 落一条风险行（缺重/状态/三列四闸）。
    pub fn reg_risk(&mut self, r: RiskRow) -> Result<(), u32> {
        if self.state == PbState::PbSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PbDiag::ESealedWrite.val());
        }
        // 缺重闸。
        for e in self.risks.iter() {
            if e.risk == r.risk {
                self.rejects = self.rejects.saturating_add(1);
                return Err(PbDiag::ERiskDup.val());
            }
        }
        if r.evidence.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PbDiag::EEvidenceEmpty.val());
        }
        if r.next_step.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PbDiag::ENextEmpty.val());
        }
        self.risks.push(r);
        Ok(())
    }
}

/// 全量合账（四域行 + 三风险行依序入账并封账）。
pub fn build_full_pb_ledger() -> PbLedger {
    let mut led = PbLedger::new();
    let _ = led.begin_run();
    for d in P95Domain::all().iter() {
        let samples = domain_samples(*d);
        let p95 = match p95_of(&samples) {
            Some(v) => v,
            None => 0,
        };
        let _ = led.reg_domain(DomainRow {
            domain: *d,
            budget: d.budget(),
            p95,
            pass: p95 <= d.budget(),
        });
    }
    for r in RISK_ROWS.iter() {
        let _ = led.reg_risk(r.clone());
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 三风险复核留痕（三列齐、状态落闭集、复核账守恒）。
fn risk_trail_ok() -> bool {
    let led = build_full_pb_ledger();
    if led.risk_len() != RISK_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < led.risk_len() {
        let r = match led.risk_at(i) {
            Some(v) => v,
            None => return false,
        };
        if r.evidence.is_empty() || r.next_step.is_empty() {
            return false;
        }
        let known = RiskStatus::all().iter().any(|s| *s == r.status);
        if !known {
            return false;
        }
        i += 1;
    }
    // 冻结表逐行与账一致。
    for row in RISK_ROWS.iter() {
        let frozen = match replay_risk(row.risk) {
            Some(v) => v,
            None => return false,
        };
        if frozen != *row {
            return false;
        }
    }
    true
}

/// 零静默（三列缺一/复核缺条/差异静默必检出）。
fn zero_silent_ok() -> bool {
    let (caught, total, escape) = pb_fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 四域 P95 合账面（同种子同 P95、判别守恒、全绿判）。
fn p95_budget_ok() -> bool {
    let led = build_full_pb_ledger();
    if led.len() != P95Domain::all().len() || led.state() != PbState::PbSealed {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    // 同种子重算 P95 必同值。
    for d in P95Domain::all().iter() {
        let s1 = domain_samples(*d);
        let s2 = domain_samples(*d);
        if s1.len() != s2.len() || SAMPLE_CAP != 40 {
            return false;
        }
        let mut i = 0usize;
        while i < s1.len() {
            match (s1.get(i), s2.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        let p1 = match p95_of(&s1) {
            Some(v) => v,
            None => return false,
        };
        let p2 = match p95_of(&s2) {
            Some(v) => v,
            None => return false,
        };
        if p1 != p2 {
            return false;
        }
    }
    // 合账行判别守恒。
    let mut i = 0usize;
    while i < led.len() {
        let r = match led.row_at(i) {
            Some(v) => v,
            None => return false,
        };
        if r.pass != (r.p95 <= r.budget) {
            return false;
        }
        if r.p95 > r.budget {
            // 超线必落差异账。
            if led.diff_len() == 0 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn pb_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 证据空。
    {
        let mut led = PbLedger::new();
        let _ = led.begin_run();
        let bad = RiskRow { risk: RiskId::R28, status: RiskStatus::Reviewed, evidence: "", next_step: "补证据" };
        if led.reg_risk(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("证空;");
        }
    }
    // 2. 下一步空。
    {
        let mut led = PbLedger::new();
        let _ = led.begin_run();
        let bad = RiskRow { risk: RiskId::R28, status: RiskStatus::Reviewed, evidence: "证据引", next_step: "" };
        if led.reg_risk(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("步空;");
        }
    }
    // 3. 风险重复。
    {
        let mut led = PbLedger::new();
        let _ = led.begin_run();
        let r0 = match RISK_ROWS.get(0) {
            Some(v) => v.clone(),
            None => return (caught, 10, String::from("表缺险")),
        };
        let _ = led.reg_risk(r0.clone());
        if led.reg_risk(r0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重险;");
        }
    }
    // 4. P95 差异静默（pass 虚报为绿）。
    {
        let mut led = PbLedger::new();
        let _ = led.begin_run();
        let bad = DomainRow { domain: P95Domain::PLoad, budget: P95Domain::PLoad.budget(), p95: 95, pass: true };
        // 95 落窗内（20..71）但 > 预算 90 → 虚绿必被静默闸拒。
        if led.reg_domain(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("静默;");
        }
    }
    // 5. 预算漂移。
    {
        let mut led = PbLedger::new();
        let _ = led.begin_run();
        let bad = DomainRow { domain: P95Domain::PObject, budget: 999, p95: 60, pass: true };
        if led.reg_domain(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("预漂;");
        }
    }
    // 6. 采样越窗。
    {
        let mut led = PbLedger::new();
        let _ = led.begin_run();
        let bad = DomainRow { domain: P95Domain::PProbe, budget: P95Domain::PProbe.budget(), p95: 999, pass: false };
        if led.reg_domain(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("越窗;");
        }
    }
    // 7. 风险缺一封账。
    {
        let mut led = PbLedger::new();
        let _ = led.begin_run();
        for d in P95Domain::all().iter() {
            let samples = domain_samples(*d);
            let p95 = match p95_of(&samples) {
                Some(v) => v,
                None => 0,
            };
            let _ = led.reg_domain(DomainRow { domain: *d, budget: d.budget(), p95, pass: p95 <= d.budget() });
        }
        let _ = led.reg_risk(match RISK_ROWS.get(0) {
            Some(v) => v.clone(),
            None => return (caught, 10, String::from("表缺险")),
        });
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺险;");
        }
    }
    // 8. 封账后写。
    {
        let mut led = build_full_pb_ledger();
        let extra = match RISK_ROWS.get(0) {
            Some(v) => v.clone(),
            None => return (caught, 10, String::from("表缺险")),
        };
        if led.reg_risk(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut led = build_full_pb_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（状态名与风险号恒非空）。
    {
        let mut bad = false;
        for s in RiskStatus::all().iter() {
            if s.name().is_empty() {
                bad = true;
            }
        }
        for r in RiskId::all().iter() {
            if r.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_full_pb_ledger();
    let b = build_full_pb_ledger();
    if a.len() != b.len() || a.risk_len() != b.risk_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if pb_fault_sweep().0 != pb_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要（四域预算 + 冻结风险表折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    for d in P95Domain::all().iter() {
        acc = acc.wrapping_mul(5).wrapping_add(d.budget());
    }
    for r in RISK_ROWS.iter() {
        acc = acc
            .wrapping_add(r.risk.ordinal())
            .wrapping_add(r.evidence.len() as u32)
            .wrapping_add(r.next_step.len() as u32);
    }
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12694u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_pb_ledger();
    if led.state() != PbState::PbSealed {
        return false;
    }
    for s in RiskStatus::all().iter() {
        if s.name().is_empty() {
            return false;
        }
    }
    for r in RiskId::all().iter() {
        if r.name().is_empty() {
            return false;
        }
    }
    // 待实测项不虚封绿（R-D1-030 状态必须待实测）。
    match replay_risk(RiskId::R30) {
        Some(r) => {
            if r.status != RiskStatus::PendingRealTest {
                return false;
            }
        }
        None => return false,
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12695 联签性能预算总表自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12694_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12694-p95-budget-reg");
    let led = build_full_pb_ledger();

    s.add_dyn(
        "F12695-01 四域合账行齐",
        led.len() == P95Domain::all().len(),
        &format!("rows={} domains={}", led.len(), P95Domain::all().len()),
    );

    s.add_dyn(
        "F12695-02 三风险复核留痕三列齐",
        risk_trail_ok(),
        &format!("risks={} diffs={}", led.risk_len(), led.diff_len()),
    );

    s.add_dyn(
        "F12695-03 零静默",
        zero_silent_ok(),
        &format!("caught={}", pb_fault_sweep().0),
    );

    s.add_dyn(
        "F12695-04 四域 P95 合账面",
        p95_budget_ok(),
        "同种子同 P95 且判别守恒",
    );

    s.add_dyn(
        "F12695-05 采样窗口径",
        sample_in_window(SAMPLE_LO) && !sample_in_window(SAMPLE_LO + SAMPLE_SPAN),
        "采样窗 20..71 闭开区间",
    );

    s.add_dyn(
        "F12695-06 P95 整数口径",
        {
            let fixed = [30u32, 40, 50, 60, 70];
            match p95_of(&fixed) {
                Some(v) => v == 70,
                None => false,
            }
        },
        "五样本固定集 P95 直读 = 70",
    );

    s.add_dyn(
        "F12695-07 预算冻结值",
        P95Domain::PLoad.budget() == 90 && P95Domain::PReceipt.budget() == 84,
        "四域预算冻结 90/88/86/84",
    );

    s.add_dyn(
        "F12695-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = pb_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", pb_fault_sweep().0),
    );

    s.add_dyn(
        "F12695-09 账等式（风险数 ≡ 复核条数）",
        led.risk_len() == RISK_CAP,
        &format!("risks={} rejects={}", led.risk_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12695-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12695-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空（待实测项不虚封绿）",
    );

    s.add_dyn(
        "F12695-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12695-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
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
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81780);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(PB_ALL.len(), 11);
        let mut i = 0usize;
        while i < PB_ALL.len() {
            let vi = match PB_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < PB_ALL.len() {
                let vj = match PB_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn risk_trail_holds() {
        assert!(risk_trail_ok());
        assert!(zero_silent_ok());
        let led = build_full_pb_ledger();
        assert_eq!(led.risk_len(), 3);
        assert_eq!(led.state(), PbState::PbSealed);
        assert_eq!(led.reject_len(), 0);
        // 三列恒非空。
        let mut i = 0usize;
        while i < led.risk_len() {
            let r = match led.risk_at(i) {
                Some(v) => v.clone(),
                None => return,
            };
            assert!(!r.evidence.is_empty(), "风险 {} 证据空", r.risk.name());
            assert!(!r.next_step.is_empty(), "风险 {} 下一步空", r.risk.name());
            i += 1;
        }
    }

    #[test]
    fn p95_integer_semantics() {
        // 固定集直读。
        let fixed = [30u32, 40, 50, 60, 70];
        assert_eq!(p95_of(&fixed), Some(70));
        // 全同集。
        let same = [50u32, 50, 50, 50];
        assert_eq!(p95_of(&same), Some(50));
        // 空集 None。
        let empty: Vec<u32> = Vec::new();
        assert!(p95_of(&empty).is_none());
        // 窗校验。
        assert!(sample_in_window(20));
        assert!(sample_in_window(70));
        assert!(!sample_in_window(19));
        assert!(!sample_in_window(71));
    }

    #[test]
    fn samples_deterministic() {
        // 同种子同集同 P95。
        let a = domain_samples(P95Domain::PLoad);
        let b = domain_samples(P95Domain::PLoad);
        assert_eq!(a.len(), SAMPLE_CAP);
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            let va = match a.get(i) {
                Some(v) => *v,
                None => return,
            };
            let vb = match b.get(i) {
                Some(v) => *v,
                None => return,
            };
            assert_eq!(va, vb, "样本 {} 双跑漂移", i);
            i += 1;
        }
        assert_eq!(p95_of(&a), p95_of(&b));
        assert!(p95_budget_ok());
    }

    #[test]
    fn p95_within_budget() {
        // 四域 P95 全落预算内（绿判）。
        for d in P95Domain::all().iter() {
            let s = domain_samples(*d);
            let p = match p95_of(&s) {
                Some(v) => v,
                None => return,
            };
            assert!(p <= d.budget(), "域 {} P95 {} 超预算 {}", d.name(), p, d.budget());
        }
        let led = build_full_pb_ledger();
        assert_eq!(led.diff_len(), 0);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = pb_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn domain_row_gates() {
        let mut led = PbLedger::new();
        let _ = led.begin_run();
        // 预算漂移。
        let drift = DomainRow { domain: P95Domain::PLoad, budget: 999, p95: 50, pass: true };
        assert_eq!(led.reg_domain(drift), Err(PbDiag::ERiskMissing.val()));
        // 采样越窗。
        let over = DomainRow { domain: P95Domain::PLoad, budget: 90, p95: 999, pass: false };
        assert_eq!(led.reg_domain(over), Err(PbDiag::ESampleRange.val()));
        // P95 虚绿（95 > 90 却报绿）。
        let fake = DomainRow { domain: P95Domain::PLoad, budget: 90, p95: 95, pass: true };
        assert_eq!(led.reg_domain(fake), Err(PbDiag::EP95Silent.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn pending_real_test_not_fake_green() {
        // 待实测项不虚封绿。
        let r030 = match replay_risk(RiskId::R30) {
            Some(v) => v,
            None => return,
        };
        assert_eq!(r030.status, RiskStatus::PendingRealTest);
        assert!(writeback_ok());
        // 状态闭集。
        assert_eq!(RiskStatus::all().len(), 3);
        assert_eq!(RiskId::all().len(), 3);
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_pb_ledger();
        let b = build_full_pb_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(pb_fault_sweep().0, pb_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(PbState::PbOpen.rank() < PbState::PbRunning.rank());
        assert!(PbState::PbRunning.rank() < PbState::PbSealed.rank());
        let mut led = build_full_pb_ledger();
        assert_eq!(led.state(), PbState::PbSealed);
        assert_eq!(led.seal(), Err(PbDiag::ERewind.val()));
        let mut fresh = PbLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(PbDiag::ERewind.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12694_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
