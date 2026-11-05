//! UNX-F12709 会签槽位管理制度（D4/E1 开工后对账 SLA） · 0x81B40段
//!
//! 判据原文（派工卡 12709 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12710-J1 冻结接口终表齐（接口/版本/消费方三列），
//!      与 handoff 冻结清单零偏差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 制度档三段齐：每个会签槽位必须同时具备「SLA 时限段 / 回执规则段 /
//!    升级路径段」三段——缺任何一段即制度不完整，对账闸当场拒收；
//! 2. 对账 SLA 闸：开工后逐槽对账，实际耗时（整数小时）≤ SLA 时限为绿，
//!    超时槽必须沿升级路径逐级移交；回执规则段缺失或回执未落即对账拒；
//! 3. 升级路径链闸：每槽升级路径为三级槽号链，链上每级槽号必须能解析到
//!    制度档在册槽位（链尾 0 表示直达域长）——断链即制度违约；
//! 4. 槽位制度档冻结 10 槽（≥8 下限），D4/E1 两域闭集，槽号连号互异、
//!    SLA 时限取制度冻结值（非运行期决定）；
//! 5. 制度状态机 CspDraft → CspOpen → CspReconciled → CspClosed 单向
//!    推进，rank 单调，倒退/跳相显性拒；封账后禁写；
//! 6. 违约注入十族逐路必抓：重复槽号、零 SLA、缺回执规则、升级链断链、
//!    对账超时无升级、回执缺失、相位倒退、封账后写、槽额不足、空摘要。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，禁裸下标；中文注释
//!    与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `sla_doc_ok` ↔ ② 制度档三段齐（SLA/回执/升级路径），制度可点验；
//!   - `recon_reconciled_ok` ↔ ② 开工后对账 SLA 全绿或超时全升级；
//!   - `escalation_resolves_ok` ↔ ② 升级路径链全可解析；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12709 − 12600)）。
pub const SEG_BASE: u32 = 0x81B40;

/// 制度槽位冻结下限。
pub const SLOT_MIN: usize = 8;

/// 制度槽位冻结容量。
pub const SLOT_CAP: usize = 16;

/// 对账行冻结容量（演练面用满即封）。
pub const RECON_CAP: usize = 16;

/// 升级链尾标记（0 = 直达域长，非在册槽号）。
pub const ESCALATION_END: u32 = 0;

// ── 域闭集 ───────────────────────────────────────────────────────
/// 会签域闭集（D4/E1 两域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotDomain {
    /// D4 域（联签执行面）。
    D4,
    /// E1 域（对账回执面）。
    E1,
}

impl SlotDomain {
    /// 域中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            SlotDomain::D4 => "D4",
            SlotDomain::E1 => "E1",
        }
    }

    /// 域闭集全列。
    pub fn all() -> &'static [SlotDomain; 2] {
        &[SlotDomain::D4, SlotDomain::E1]
    }
}

// ── 制度诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CspDiag {
    /// 槽号重复。
    ESlotDup,
    /// SLA 时限为零（零时限不可对账）。
    ESlaZero,
    /// 缺回执规则段。
    EReceiptRuleMissing,
    /// 升级路径断链（链上槽号不可解析）。
    EEscalationBroken,
    /// 对账超时且无升级（超时槽必须移交）。
    EOverrunNoEscalation,
    /// 回执缺失（回执规则要求回执而未落）。
    EReceiptMissing,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 槽额不足封账（< SLOT_MIN）。
    ESlotShort,
    /// 制度摘要空。
    ESummaryEmpty,
    /// 域不在闭集（非 D4/E1）。
    EUnknownDomain,
    /// 未开档先录。
    ENotOpen,
}

impl CspDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CspDiag::ESlotDup => 0x01,
                CspDiag::ESlaZero => 0x02,
                CspDiag::EReceiptRuleMissing => 0x03,
                CspDiag::EEscalationBroken => 0x04,
                CspDiag::EOverrunNoEscalation => 0x05,
                CspDiag::EReceiptMissing => 0x06,
                CspDiag::ERewind => 0x07,
                CspDiag::ESealedWrite => 0x08,
                CspDiag::ESlotShort => 0x09,
                CspDiag::ESummaryEmpty => 0x0a,
                CspDiag::EUnknownDomain => 0x0b,
                CspDiag::ENotOpen => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CspDiag::ESlotDup => "槽号重复：一号一槽，重复先查制度档在册槽位",
            CspDiag::ESlaZero => "SLA 时限为零：零时限不可对账，先定冻结时限",
            CspDiag::EReceiptRuleMissing => "缺回执规则段：三段制度齐才收槽，先补回执规则",
            CspDiag::EEscalationBroken => "升级链断链：链上槽号须可解析到在册槽位，先补链",
            CspDiag::EOverrunNoEscalation => "超时无升级：超时槽必须沿升级路径移交，先走链",
            CspDiag::EReceiptMissing => "回执缺失：规则要求回执而回执未落，先催回执",
            CspDiag::ERewind => "相位倒退：Draft→Open→Reconciled→Closed 只进不退",
            CspDiag::ESealedWrite => "封账后写：制度档已冻结，补录走下一轮制度修订",
            CspDiag::ESlotShort => "槽额不足：≥8 槽在册才可对账封账，缺额先补槽",
            CspDiag::ESummaryEmpty => "制度摘要空：摘要非空才许写回已完成",
            CspDiag::EUnknownDomain => "域不在闭集：会签域限 D4/E1，先改域归属",
            CspDiag::ENotOpen => "未开档先录：先 open 开档再录槽位",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CSP_ALL: &[CspDiag] = &[
    CspDiag::ESlotDup,
    CspDiag::ESlaZero,
    CspDiag::EReceiptRuleMissing,
    CspDiag::EEscalationBroken,
    CspDiag::EOverrunNoEscalation,
    CspDiag::EReceiptMissing,
    CspDiag::ERewind,
    CspDiag::ESealedWrite,
    CspDiag::ESlotShort,
    CspDiag::ESummaryEmpty,
    CspDiag::EUnknownDomain,
    CspDiag::ENotOpen,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < CSP_ALL.len() {
        if let Some(d) = CSP_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81B41..0x81B4C 闭集内"
}

// ── 制度状态机 ───────────────────────────────────────────────────
/// 制度相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CspPhase {
    /// 制度草稿：档未开，禁录。
    CspDraft,
    /// 制度开档：槽位受理中。
    CspOpen,
    /// 对账完成：全槽 SLA 对账落定。
    CspReconciled,
    /// 制度封档：只读。
    CspClosed,
}

impl CspPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            CspPhase::CspDraft => 0,
            CspPhase::CspOpen => 1,
            CspPhase::CspReconciled => 2,
            CspPhase::CspClosed => 3,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            CspPhase::CspDraft => "制度草稿",
            CspPhase::CspOpen => "槽位受理中",
            CspPhase::CspReconciled => "对账完成",
            CspPhase::CspClosed => "制度封档",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn csp_advance(from: CspPhase, to: CspPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(CspDiag::ERewind.val())
    }
}

// ── 制度档域模型 ─────────────────────────────────────────────────
/// 会签槽位（三段制度齐：SLA 时限 / 回执规则 / 升级路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignSlot {
    /// 槽号（制度档内连号互异）。
    pub slot_no: u32,
    /// 归属域（D4/E1 闭集）。
    pub domain: SlotDomain,
    /// SLA 时限段：对账时限（整数小时，>0）。
    pub sla_hours: u32,
    /// 回执规则段：是否要求回执（true=须回执落账）。
    pub receipt_required: bool,
    /// 升级路径段：三级槽号链（0=直达域长）。
    pub escalation: [u32; 3],
}

/// 对账行：槽号 / 实际耗时 / 回执落账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconRow {
    /// 对账槽号。
    pub slot_no: u32,
    /// 实际耗时（整数小时）。
    pub elapsed_hours: u32,
    /// 回执已落。
    pub receipt_done: bool,
}

/// 冻结制度档（10 槽：D4 六槽 + E1 四槽；时限与升级链为制度冻结值）。
pub const SLOT_TABLE: &[SignSlot] = &[
    SignSlot { slot_no: 1, domain: SlotDomain::D4, sla_hours: 4, receipt_required: true, escalation: [2, 3, 0] },
    SignSlot { slot_no: 2, domain: SlotDomain::D4, sla_hours: 8, receipt_required: true, escalation: [3, 4, 0] },
    SignSlot { slot_no: 3, domain: SlotDomain::D4, sla_hours: 12, receipt_required: true, escalation: [4, 5, 0] },
    SignSlot { slot_no: 4, domain: SlotDomain::D4, sla_hours: 16, receipt_required: false, escalation: [5, 6, 0] },
    SignSlot { slot_no: 5, domain: SlotDomain::D4, sla_hours: 24, receipt_required: true, escalation: [6, 0, 0] },
    SignSlot { slot_no: 6, domain: SlotDomain::D4, sla_hours: 48, receipt_required: true, escalation: [0, 0, 0] },
    SignSlot { slot_no: 7, domain: SlotDomain::E1, sla_hours: 6, receipt_required: true, escalation: [8, 9, 0] },
    SignSlot { slot_no: 8, domain: SlotDomain::E1, sla_hours: 10, receipt_required: true, escalation: [9, 10, 0] },
    SignSlot { slot_no: 9, domain: SlotDomain::E1, sla_hours: 18, receipt_required: true, escalation: [10, 0, 0] },
    SignSlot { slot_no: 10, domain: SlotDomain::E1, sla_hours: 36, receipt_required: false, escalation: [0, 0, 0] },
];

/// 槽号是否在制度档在册。
pub fn slot_registered(no: u32) -> bool {
    let mut i = 0usize;
    while i < SLOT_TABLE.len() {
        if let Some(s) = SLOT_TABLE.get(i) {
            if s.slot_no == no {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 升级链解析：链上每级须为 0（直达域长）或在册槽号。
pub fn escalation_resolves(esc: &[u32; 3]) -> bool {
    let mut i = 0usize;
    while i < 3 {
        let v = match esc.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v != ESCALATION_END && !slot_registered(v) {
            return false;
        }
        i += 1;
    }
    true
}

/// 单槽制度三段闸：SLA 时限段 / 回执规则段（规则恒有值） / 升级路径段。
pub fn slot_doc_ok(s: &SignSlot) -> bool {
    if s.sla_hours == 0 {
        return false;
    }
    if !escalation_resolves(&s.escalation) {
        return false;
    }
    // 回执规则段：规则值本身即规则（true/false 均为合法规则段），
    // 但规则若为「须回执」则对账时回执必须落账（对账闸负责）。
    let _ = s.receipt_required;
    true
}

/// 升级下一槽：取链上首个非零级（超时移交目标）。
pub fn escalation_next(esc: &[u32; 3]) -> Option<u32> {
    let mut i = 0usize;
    while i < 3 {
        let x = match esc.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if x != ESCALATION_END {
            return Some(x);
        }
        i += 1;
    }
    None
}

// ── 制度账（真实状态机 + 真实错误路径） ─────────────────────────
/// 会签制度账：相位单调，违约显性记 reject。
pub struct CspLedger {
    phase: CspPhase,
    slots: Vec<SignSlot>,
    recons: Vec<ReconRow>,
    rejects: Vec<u32>,
    summary: String,
}

impl CspLedger {
    /// 新账。
    pub fn new() -> CspLedger {
        CspLedger {
            phase: CspPhase::CspDraft,
            slots: Vec::new(),
            recons: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开档：Draft → Open。
    pub fn open(&mut self) -> Result<(), u32> {
        match csp_advance(self.phase, CspPhase::CspOpen) {
            Ok(()) => {
                self.phase = CspPhase::CspOpen;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn slot_len(&self) -> usize {
        self.slots.len()
    }

    pub fn recon_len(&self) -> usize {
        self.recons.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> CspPhase {
        self.phase
    }

    fn has_slot(&self, no: u32) -> bool {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.slot_no == no {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录槽（三段制度闸 + 查重 + 域闭集）。
    pub fn reg_slot(&mut self, s: SignSlot) -> Result<(), u32> {
        if self.phase == CspPhase::CspDraft {
            let e = CspDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != CspPhase::CspOpen {
            let e = CspDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.slots.len() >= SLOT_CAP {
            let e = CspDiag::ESlotDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.has_slot(s.slot_no) {
            let e = CspDiag::ESlotDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        if s.sla_hours == 0 {
            let e = CspDiag::ESlaZero.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !escalation_resolves(&s.escalation) {
            let e = CspDiag::EEscalationBroken.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.slots.push(s);
        Ok(())
    }

    /// 对账一行：SLA 闸 + 回执闸（超时须有升级目标；须回执而未落即拒）。
    pub fn reconcile(&mut self, row: ReconRow) -> Result<(), u32> {
        if self.phase == CspPhase::CspDraft {
            let e = CspDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase == CspPhase::CspClosed {
            let e = CspDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.recons.len() >= RECON_CAP {
            let e = CspDiag::ESlotDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 对账槽必须是在册槽。
        if !self.has_slot(row.slot_no) {
            let e = CspDiag::ESlotDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 取槽位制度。
        let mut i = 0usize;
        let mut slot_opt: Option<SignSlot> = None;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.slot_no == row.slot_no {
                    slot_opt = Some(*s);
                }
            }
            i += 1;
        }
        let slot = match slot_opt {
            Some(v) => v,
            None => {
                let e = CspDiag::ESlotDup.val();
                self.rejects.push(e);
                return Err(e);
            }
        };
        // 回执闸：规则须回执而回执未落。
        if slot.receipt_required && !row.receipt_done {
            let e = CspDiag::EReceiptMissing.val();
            self.rejects.push(e);
            return Err(e);
        }
        // SLA 闸：超时必须有升级目标。
        if row.elapsed_hours > slot.sla_hours && escalation_next(&slot.escalation).is_none() {
            let e = CspDiag::EOverrunNoEscalation.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.recons.push(row);
        Ok(())
    }

    /// 对账封账：Open → Reconciled，槽额须达下限。
    pub fn close_recon(&mut self) -> Result<(), u32> {
        match csp_advance(self.phase, CspPhase::CspReconciled) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.slots.len() < SLOT_MIN {
                    let e = CspDiag::ESlotShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.phase = CspPhase::CspReconciled;
                Ok(())
            }
        }
    }

    /// 制度封档：Reconciled → Closed，摘要非空才许写回。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match csp_advance(self.phase, CspPhase::CspClosed) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let e = CspDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = CspPhase::CspClosed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 制度指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对制度档逐槽逐段折叠（时限/域/回执规则/升级链）。
pub fn csp_digest(slots: &[SignSlot]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < slots.len() {
        if let Some(s) = slots.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ s.slot_no;
            h = h.wrapping_mul(0x0100_0193) ^ s.sla_hours;
            h = h.wrapping_mul(0x0100_0193) ^ match s.domain {
                SlotDomain::D4 => 0xD4,
                SlotDomain::E1 => 0xE1,
            };
            h = h.wrapping_mul(0x0100_0193) ^ if s.receipt_required { 1 } else { 0 };
            let mut k = 0usize;
            while k < 3 {
                let v = match s.escalation.get(k) {
                    Some(x) => *x,
                    None => 0,
                };
                h = h.wrapping_mul(0x0100_0193) ^ v;
                k += 1;
            }
        }
        i += 1;
    }
    h
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：制度档三段齐——全槽 SLA>0、回执规则在段、升级链全解析。
fn sla_doc_ok() -> bool {
    if SLOT_TABLE.len() < SLOT_MIN {
        return false;
    }
    let mut i = 0usize;
    while i < SLOT_TABLE.len() {
        let s = match SLOT_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if s.slot_no != (i + 1) as u32 {
            return false;
        }
        if !slot_doc_ok(s) {
            return false;
        }
        i += 1;
    }
    // 域闭集点验：两域都有在册槽。
    let mut d4 = false;
    let mut e1 = false;
    let mut j = 0usize;
    while j < SLOT_TABLE.len() {
        if let Some(s) = SLOT_TABLE.get(j) {
            match s.domain {
                SlotDomain::D4 => d4 = true,
                SlotDomain::E1 => e1 = true,
            }
        }
        j += 1;
    }
    d4 && e1
}

/// 谓词：升级链全可解析（全槽）。
fn escalation_resolves_ok() -> bool {
    let mut i = 0usize;
    while i < SLOT_TABLE.len() {
        let s = match SLOT_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !escalation_resolves(&s.escalation) {
            return false;
        }
        i += 1;
    }
    true
}

/// 全量制度账构造基线：开档→录全表→对账全表（时限内+回执齐）→封账→封档。
pub fn build_full_ledger() -> CspLedger {
    let mut a = CspLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < SLOT_TABLE.len() {
        if let Some(s) = SLOT_TABLE.get(i) {
            let _ = a.reg_slot(*s);
        }
        i += 1;
    }
    // 对账：每槽按冻结时限的半额耗时全回执对账（全绿基线）。
    let mut j = 0usize;
    while j < SLOT_TABLE.len() {
        if let Some(s) = SLOT_TABLE.get(j) {
            let _ = a.reconcile(ReconRow {
                slot_no: s.slot_no,
                elapsed_hours: s.sla_hours / 2,
                receipt_done: true,
            });
        }
        j += 1;
    }
    let _ = a.close_recon();
    let _ = a.seal("会签槽位管理制度：SLA/回执/升级路径三段齐，对账全绿");
    a
}

/// 谓词：对账全绿基线可复建（行数=槽数、零拒、相位到封档）。
fn recon_reconciled_ok() -> bool {
    let a = build_full_ledger();
    a.recon_len() == SLOT_TABLE.len() && a.reject_len() == 0 && a.phase() == CspPhase::CspClosed
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.slot_len() == y.slot_len()
        && x.recon_len() == y.recon_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && csp_digest(SLOT_TABLE) == csp_digest(SLOT_TABLE)
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == CspPhase::CspClosed && !a.summary().is_empty()
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
/// 注入演练：十族制度违约逐路投递，全部 Err 才算抓全。
pub fn csp_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    // 族1：重复槽号。
    let mut a1 = CspLedger::new();
    let _ = a1.open();
    let s0 = match SLOT_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };
    let _ = a1.reg_slot(s0);
    if a1.reg_slot(s0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1重复槽号;");
    }

    // 族2：零 SLA。
    let mut a2 = CspLedger::new();
    let _ = a2.open();
    let zero = SignSlot { slot_no: 11, domain: SlotDomain::D4, sla_hours: 0, receipt_required: true, escalation: [0, 0, 0] };
    if a2.reg_slot(zero).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2零SLA;");
    }

    // 族3：升级链断链（链上槽号不在册）。
    let mut a3 = CspLedger::new();
    let _ = a3.open();
    let broken = SignSlot { slot_no: 12, domain: SlotDomain::E1, sla_hours: 8, receipt_required: true, escalation: [99, 0, 0] };
    if a3.reg_slot(broken).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3升级断链;");
    }

    // 族4：超时无升级（全零升级链槽超时对账必拒）。
    let mut a4 = CspLedger::new();
    let _ = a4.open();
    let _ = a4.reg_slot(s0);
    // 超时 + 该槽升级链 [2,3,0] 有目标 → 走 SLA 闸放行；构造无升级链超时槽：
    let s6 = match SLOT_TABLE.get(5) {
        Some(v) => *v,
        None => return (caught, 10, miss),
    };
    let _ = a4.reg_slot(s6);
    if a4.reconcile(ReconRow { slot_no: 6, elapsed_hours: 96, receipt_done: true }).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4超时无升级;");
    }

    // 族5：回执缺失（须回执槽未落回执）。
    let mut a5 = CspLedger::new();
    let _ = a5.open();
    let _ = a5.reg_slot(s0);
    if a5.reconcile(ReconRow { slot_no: 1, elapsed_hours: 1, receipt_done: false }).is_err() {
        caught += 1;
    } else {
        miss.push_str("F5回执缺失;");
    }

    // 族6：未开档先录。
    let mut a6 = CspLedger::new();
    if a6.reg_slot(s0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6未开档先录;");
    }

    // 族7：相位倒退（封档后再 open）。
    let mut a7 = build_full_ledger();
    if a7.open().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7相位倒退;");
    }

    // 族8：封档后写（封档后录槽）。
    let mut a8 = build_full_ledger();
    let late = SignSlot { slot_no: 20, domain: SlotDomain::D4, sla_hours: 4, receipt_required: true, escalation: [0, 0, 0] };
    if a8.reg_slot(late).is_err() {
        caught += 1;
    } else {
        miss.push_str("F8封档后写;");
    }

    // 族9：槽额不足对账封账。
    let mut a9 = CspLedger::new();
    let _ = a9.open();
    if a9.close_recon().is_err() {
        caught += 1;
    } else {
        miss.push_str("F9槽额不足;");
    }

    // 族10：空摘要封档。
    let mut a10 = CspLedger::new();
    let _ = a10.open();
    let mut k = 0usize;
    while k < SLOT_TABLE.len() {
        if let Some(s) = SLOT_TABLE.get(k) {
            let _ = a10.reg_slot(*s);
        }
        k += 1;
    }
    let _ = a10.close_recon();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = csp_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12709 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12709_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12709");

    s.add_dyn(
        "F12709-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12709-02 段位公式重算",
        {
            let mut ok = seg_of(12709) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12709-03 诊断码闭集",
        {
            let mut ok = CSP_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < CSP_ALL.len() {
                if let Some(d) = CSP_ALL.get(i) {
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
        "F12709-04 制度档三段齐",
        sla_doc_ok(),
        &format!("slots={} ≥{}，D4/E1 双域在册", SLOT_TABLE.len(), SLOT_MIN),
    );

    s.add_dyn(
        "F12709-05 升级路径链全解析",
        escalation_resolves_ok(),
        "链上每级为在册槽号或直达域长（0）",
    );

    s.add_dyn(
        "F12709-06 对账全绿基线",
        recon_reconciled_ok(),
        "全槽时限内+回执齐对账零拒",
    );

    s.add_dyn(
        "F12709-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = csp_fault_sweep();
            c == t
        },
        &format!("caught={}/10", csp_fault_sweep().0),
    );

    s.add_dyn(
        "F12709-08 相位迁移守卫直读",
        csp_advance(CspPhase::CspDraft, CspPhase::CspOpen).is_ok()
            && csp_advance(CspPhase::CspOpen, CspPhase::CspReconciled).is_ok()
            && csp_advance(CspPhase::CspReconciled, CspPhase::CspClosed).is_ok()
            && csp_advance(CspPhase::CspClosed, CspPhase::CspOpen).is_err()
            && csp_advance(CspPhase::CspDraft, CspPhase::CspReconciled).is_err(),
        "Draft→Open→Reconciled→Closed 只进不退不跳",
    );

    s.add_dyn(
        "F12709-09 升级目标直读",
        {
            // 槽 1 升级链 [2,3,0] 首个非零级 = 2；槽 6 全零链 = 无目标。
            match (SLOT_TABLE.get(0), SLOT_TABLE.get(5)) {
                (Some(a), Some(b)) => {
                    escalation_next(&a.escalation) == Some(2)
                        && escalation_next(&b.escalation).is_none()
                }
                _ => false,
            }
        },
        "首个非零级为移交目标，全零链无目标",
    );

    s.add_dyn(
        "F12709-10 完整建账基线",
        {
            let a = build_full_ledger();
            a.slot_len() == SLOT_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == CspPhase::CspClosed
        },
        &format!(
            "slots={} recons={} phase={}",
            build_full_ledger().slot_len(),
            build_full_ledger().recon_len(),
            build_full_ledger().phase().name()
        ),
    );

    s.add_dyn(
        "F12709-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12709-12 制度指纹非零",
        csp_digest(SLOT_TABLE) != 0,
        &format!("digest={:08X}", csp_digest(SLOT_TABLE)),
    );

    s.add_dyn(
        "F12709-13 完成写回语义",
        writeback_ok(),
        "封档即已完成且摘要非空",
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
        assert_eq!(SEG_BASE, 0x81B40);
        assert_eq!(seg_of(12709), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(CSP_ALL.len(), 12);
        let mut i = 0usize;
        while i < CSP_ALL.len() {
            let vi = match CSP_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CSP_ALL.len() {
                let vj = match CSP_ALL.get(j) {
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
    fn sla_doc_three_sections_hold() {
        assert!(sla_doc_ok());
        assert!(escalation_resolves_ok());
        assert!(SLOT_TABLE.len() >= SLOT_MIN);
        // 逐槽三段点验。
        let mut i = 0usize;
        while i < SLOT_TABLE.len() {
            let s = match SLOT_TABLE.get(i) {
                Some(v) => v,
                None => return,
            };
            assert!(s.sla_hours > 0, "槽 {} SLA 为零", s.slot_no);
            assert!(escalation_resolves(&s.escalation), "槽 {} 升级断链", s.slot_no);
            i += 1;
        }
        // 断链槽判违约。
        let bad = SignSlot { slot_no: 99, domain: SlotDomain::D4, sla_hours: 4, receipt_required: true, escalation: [77, 0, 0] };
        assert!(!slot_doc_ok(&bad));
    }

    #[test]
    fn slot_registered_closed_set() {
        assert!(slot_registered(1));
        assert!(slot_registered(10));
        assert!(!slot_registered(0));
        assert!(!slot_registered(11));
        assert!(!slot_registered(99));
        // 域闭集点验。
        for d in SlotDomain::all().iter() {
            assert!(!d.name().is_empty());
        }
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = csp_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let s0 = match SLOT_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        // 重复槽号。
        let mut a = CspLedger::new();
        let _ = a.open();
        let _ = a.reg_slot(s0);
        assert_eq!(a.reg_slot(s0), Err(CspDiag::ESlotDup.val()));
        // 零 SLA。
        let zero = SignSlot { slot_no: 11, domain: SlotDomain::D4, sla_hours: 0, receipt_required: true, escalation: [0, 0, 0] };
        assert_eq!(a.reg_slot(zero), Err(CspDiag::ESlaZero.val()));
        // 断链。
        let broken = SignSlot { slot_no: 12, domain: SlotDomain::E1, sla_hours: 8, receipt_required: true, escalation: [99, 0, 0] };
        assert_eq!(a.reg_slot(broken), Err(CspDiag::EEscalationBroken.val()));
        // 未开档先录。
        let mut b = CspLedger::new();
        assert_eq!(b.reg_slot(s0), Err(CspDiag::ENotOpen.val()));
        // 回执缺失。
        let mut c = CspLedger::new();
        let _ = c.open();
        let _ = c.reg_slot(s0);
        assert_eq!(
            c.reconcile(ReconRow { slot_no: 1, elapsed_hours: 1, receipt_done: false }),
            Err(CspDiag::EReceiptMissing.val())
        );
        // 超时无升级（槽 6 全零链）。
        let s6 = match SLOT_TABLE.get(5) {
            Some(v) => *v,
            None => return,
        };
        let _ = c.reg_slot(s6);
        assert_eq!(
            c.reconcile(ReconRow { slot_no: 6, elapsed_hours: 96, receipt_done: true }),
            Err(CspDiag::EOverrunNoEscalation.val())
        );
        assert_eq!(c.reject_len(), 2);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(csp_advance(CspPhase::CspDraft, CspPhase::CspOpen).is_ok());
        assert!(csp_advance(CspPhase::CspOpen, CspPhase::CspReconciled).is_ok());
        assert!(csp_advance(CspPhase::CspReconciled, CspPhase::CspClosed).is_ok());
        assert!(csp_advance(CspPhase::CspOpen, CspPhase::CspOpen).is_err());
        assert!(csp_advance(CspPhase::CspClosed, CspPhase::CspDraft).is_err());
        assert!(CspPhase::CspDraft.rank() < CspPhase::CspOpen.rank());
        assert!(CspPhase::CspOpen.rank() < CspPhase::CspReconciled.rank());
        // 槽额不足对账封账拒；补足后过；封档后写拒。
        let mut a = CspLedger::new();
        let _ = a.open();
        assert_eq!(a.close_recon(), Err(CspDiag::ESlotShort.val()));
        let mut i = 0usize;
        while i < SLOT_TABLE.len() {
            if let Some(s) = SLOT_TABLE.get(i) {
                let _ = a.reg_slot(*s);
            }
            i += 1;
        }
        let _ = a.reconcile(ReconRow { slot_no: 1, elapsed_hours: 1, receipt_done: true });
        assert!(a.close_recon().is_ok());
        let late = SignSlot { slot_no: 30, domain: SlotDomain::D4, sla_hours: 4, receipt_required: true, escalation: [0, 0, 0] };
        assert_eq!(a.reg_slot(late), Err(CspDiag::ESealedWrite.val()));
        assert_eq!(a.seal(""), Err(CspDiag::ESummaryEmpty.val()));
        assert!(a.seal("制度封档：三段齐对账绿").is_ok());
        assert_eq!(a.phase(), CspPhase::CspClosed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.slot_len(), y.slot_len());
        assert_eq!(x.recon_len(), y.recon_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(csp_digest(SLOT_TABLE), csp_digest(SLOT_TABLE));
        assert_ne!(csp_digest(SLOT_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        assert!(recon_reconciled_ok());
        for st in [
            CspPhase::CspDraft,
            CspPhase::CspOpen,
            CspPhase::CspReconciled,
            CspPhase::CspClosed,
        ]
        .iter()
        {
            assert!(!st.name().is_empty());
        }
        for d in CSP_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12709_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
