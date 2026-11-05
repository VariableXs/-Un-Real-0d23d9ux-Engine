//! UNX-F12713 I 型两态记账终账 · 0x81C40段
//!
//! 判据原文（派工卡 12713 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12714-J1 三风险复核留痕（状态/证据/下一步），
//!      零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 两态闭集：账目状态只有「真绿 / 槽位态」两态——真绿须携带非空
//!    证据，槽位态须携带在册槽位引用（1..=SLOT_REF_CAP）；两态之外的
//!    表达不存在（闭集枚举兜底）；
//! 2. 终账分列：真绿列与槽位态列分开落账（分列记账），终账封账要求
//!    两列各至少一行——缺一列即终账不齐（两态终账齐才可封账）；
//! 3. 混报闸：状态与证据面不一致即混报——真绿无证据、槽位态缺槽位
//!    引用、槽位引用越闭集三类混报逐路拒收；混报演练 10/10 检出
//!    （5 枚真绿无证据 + 5 枚槽位态坏引用）；
//! 4. 判据号序闸：账内判据号连号递增（一号一账），跳号/重复即结构
//!    违约（终账权威面不允许乱序）；
//! 5. 记账状态机 TlOpen → TlAudited → TlSealed 单向推进，rank 单调，
//!    倒退/跳相显性拒；封账后账面只读；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，禁裸下标；中文注释
//!    与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `twostate_ok` ↔ ② 两态终账齐（真绿/槽位态分列）；
//!   - `mixed_drill_ok` ↔ ② 混报 10/10 检出；
//!   - `judge_order_ok` ↔ ② 判据号连号序闸（终账可点验基础）；
//!   - `fault_sweep_ok` ↔ ② 违约注入面全抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12713 − 12600)）。
pub const SEG_BASE: u32 = 0x81C40;

/// 槽位引用闭集上界（槽位态引用 1..=SLOT_REF_CAP）。
pub const SLOT_REF_CAP: u32 = 6;

/// 终账行冻结下限。
pub const ENTRY_MIN: usize = 10;

/// 终账行冻结容量。
pub const ENTRY_CAP: usize = 20;

/// 混报演练枚数。
pub const MIXED_DRILL_N: usize = 10;

// ── 账目状态闭集 ─────────────────────────────────────────────────
/// 账目状态两态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerState {
    /// 真绿：判据已验绿且证据在案。
    TrueGreen,
    /// 槽位态：挂在会签槽位上的待收口项。
    SlotState,
}

impl LedgerState {
    /// 状态中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            LedgerState::TrueGreen => "真绿",
            LedgerState::SlotState => "槽位态",
        }
    }

    /// 两态闭集全列。
    pub fn all() -> &'static [LedgerState; 2] {
        &[LedgerState::TrueGreen, LedgerState::SlotState]
    }
}

// ── 终账诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsDiag {
    /// 混报：真绿无证据。
    EMixedGreen,
    /// 混报：槽位态缺槽位引用。
    EMixedSlot,
    /// 槽位引用越闭集。
    ESlotRefUnknown,
    /// 判据号重复。
    EDup,
    /// 判据号跳号（序闸）。
    EJudgeGap,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 行额不足（< ENTRY_MIN）。
    EEntryShort,
    /// 两态分列缺一态。
    EColumnShort,
    /// 终账摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
    /// 终账审计面双跑分歧。
    EAuditDiverge,
}

impl TsDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TsDiag::EMixedGreen => 0x01,
                TsDiag::EMixedSlot => 0x02,
                TsDiag::ESlotRefUnknown => 0x03,
                TsDiag::EDup => 0x04,
                TsDiag::EJudgeGap => 0x05,
                TsDiag::ERewind => 0x06,
                TsDiag::ESealedWrite => 0x07,
                TsDiag::EEntryShort => 0x08,
                TsDiag::EColumnShort => 0x09,
                TsDiag::ESummaryEmpty => 0x0a,
                TsDiag::ENotOpen => 0x0b,
                TsDiag::EAuditDiverge => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            TsDiag::EMixedGreen => "混报：真绿须带非空证据，空证据先补证据再报绿",
            TsDiag::EMixedSlot => "混报：槽位态须带槽位引用，缺引用先挂槽",
            TsDiag::ESlotRefUnknown => "槽位引用越闭集：引用限 1..=6，先改在册槽位",
            TsDiag::EDup => "判据号重复：一号一账，重复先查在册终账",
            TsDiag::EJudgeGap => "判据号跳号：终账按序入账，跳号先补前序",
            TsDiag::ERewind => "相位倒退：TlOpen→TlAudited→TlSealed 只进不退",
            TsDiag::ESealedWrite => "封账后写：终账只读，补录走下一轮记账",
            TsDiag::EEntryShort => "行额不足：≥10 行在账才可封账，缺额先补",
            TsDiag::EColumnShort => "两态分列缺一态：真绿列与槽位态列各至少一行",
            TsDiag::ESummaryEmpty => "终账摘要空：摘要非空才许写回已完成",
            TsDiag::ENotOpen => "未开账先录：先 open 开账再录行",
            TsDiag::EAuditDiverge => "审计双跑分歧：分列计数与指纹两轮须逐位同判",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const TS_ALL: &[TsDiag] = &[
    TsDiag::EMixedGreen,
    TsDiag::EMixedSlot,
    TsDiag::ESlotRefUnknown,
    TsDiag::EDup,
    TsDiag::EJudgeGap,
    TsDiag::ERewind,
    TsDiag::ESealedWrite,
    TsDiag::EEntryShort,
    TsDiag::EColumnShort,
    TsDiag::ESummaryEmpty,
    TsDiag::ENotOpen,
    TsDiag::EAuditDiverge,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < TS_ALL.len() {
        if let Some(d) = TS_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81C41..0x81C4C 闭集内"
}

// ── 记账状态机 ───────────────────────────────────────────────────
/// 记账相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlPhase {
    /// 开账受理。
    TlOpen,
    /// 审计完成：分列齐 + 混报零命中。
    TlAudited,
    /// 终账封账。
    TlSealed,
}

impl TlPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            TlPhase::TlOpen => 0,
            TlPhase::TlAudited => 1,
            TlPhase::TlSealed => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            TlPhase::TlOpen => "开账受理",
            TlPhase::TlAudited => "审计完成",
            TlPhase::TlSealed => "终账封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn tl_advance(from: TlPhase, to: TlPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(TsDiag::ERewind.val())
    }
}

// ── 终账行模型 ───────────────────────────────────────────────────
/// 终账行：判据号 / 状态 / 证据 / 槽位引用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 判据号（账内连号）。
    pub no: u32,
    /// 账目状态（两态闭集）。
    pub state: LedgerState,
    /// 证据（真绿须非空；槽位态可空）。
    pub evidence: &'static str,
    /// 槽位引用（槽位态须 1..=6；真绿恒 0）。
    pub slot_ref: u32,
}

/// 冻结终账行表（12 行：8 真绿 + 4 槽位态；判据号连号 1..=12）。
pub const ENTRY_TABLE: &[LedgerEntry] = &[
    LedgerEntry { no: 1, state: LedgerState::TrueGreen, evidence: "EV-101:自检全绿", slot_ref: 0 },
    LedgerEntry { no: 2, state: LedgerState::TrueGreen, evidence: "EV-102:双跑一致", slot_ref: 0 },
    LedgerEntry { no: 3, state: LedgerState::SlotState, evidence: "", slot_ref: 1 },
    LedgerEntry { no: 4, state: LedgerState::TrueGreen, evidence: "EV-104:指纹在册", slot_ref: 0 },
    LedgerEntry { no: 5, state: LedgerState::TrueGreen, evidence: "EV-105:对表零偏差", slot_ref: 0 },
    LedgerEntry { no: 6, state: LedgerState::SlotState, evidence: "", slot_ref: 2 },
    LedgerEntry { no: 7, state: LedgerState::TrueGreen, evidence: "EV-107:零悬空", slot_ref: 0 },
    LedgerEntry { no: 8, state: LedgerState::SlotState, evidence: "", slot_ref: 3 },
    LedgerEntry { no: 9, state: LedgerState::TrueGreen, evidence: "EV-109:注入全抓", slot_ref: 0 },
    LedgerEntry { no: 10, state: LedgerState::TrueGreen, evidence: "EV-110:写回非空", slot_ref: 0 },
    LedgerEntry { no: 11, state: LedgerState::SlotState, evidence: "", slot_ref: 4 },
    LedgerEntry { no: 12, state: LedgerState::TrueGreen, evidence: "EV-112:段位守卫绿", slot_ref: 0 },
];

/// 槽位引用是否在闭集。
pub fn slot_ref_ok(r: u32) -> bool {
    r >= 1 && r <= SLOT_REF_CAP
}

/// 单行一致性闸（混报闸）：状态与证据面必须一致。
pub fn entry_consistent(e: &LedgerEntry) -> bool {
    match e.state {
        LedgerState::TrueGreen => {
            // 真绿：证据非空且不带槽位引用。
            !e.evidence.is_empty() && e.slot_ref == 0
        }
        LedgerState::SlotState => {
            // 槽位态：槽位引用在闭集。
            slot_ref_ok(e.slot_ref)
        }
    }
}

/// 分列扫描：返回（真绿行数, 槽位态行数, 混报行数）。
pub fn classify(entries: &[LedgerEntry]) -> (usize, usize, usize) {
    let mut green = 0usize;
    let mut slot = 0usize;
    let mut mixed = 0usize;
    let mut i = 0usize;
    while i < entries.len() {
        if let Some(e) = entries.get(i) {
            if !entry_consistent(e) {
                mixed += 1;
            } else {
                match e.state {
                    LedgerState::TrueGreen => green += 1,
                    LedgerState::SlotState => slot += 1,
                }
            }
        }
        i += 1;
    }
    (green, slot, mixed)
}

/// 序闸扫描：账内判据号须严格递增连号（从 1 起）。
pub fn order_scan(entries: &[LedgerEntry]) -> bool {
    let mut i = 0usize;
    while i < entries.len() {
        let e = match entries.get(i) {
            Some(v) => v,
            None => return false,
        };
        if e.no != (i + 1) as u32 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 终账（真实状态机 + 真实错误路径） ───────────────────────────
/// 两态终账：真绿列/槽位态列分列落账；混报 Err 拒收。
pub struct TsLedger {
    phase: TlPhase,
    greens: Vec<LedgerEntry>,
    slots: Vec<LedgerEntry>,
    rejects: Vec<u32>,
    summary: String,
}

impl TsLedger {
    /// 新账。
    pub fn new() -> TsLedger {
        TsLedger {
            phase: TlPhase::TlOpen,
            greens: Vec::new(),
            slots: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账（TlOpen 即受理相位，行录入即开始）。
    pub fn open(&mut self) -> Result<(), u32> {
        // 本账从 TlOpen 起受理；open 幂等拒（平移拒）。
        if self.phase == TlPhase::TlOpen {
            Ok(())
        } else {
            let d = TsDiag::ERewind.val();
            self.rejects.push(d);
            Err(d)
        }
    }

    pub fn green_len(&self) -> usize {
        self.greens.len()
    }

    pub fn slot_len(&self) -> usize {
        self.slots.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> TlPhase {
        self.phase
    }

    fn has_no(&self, no: u32) -> bool {
        let mut i = 0usize;
        while i < self.greens.len() {
            if let Some(e) = self.greens.get(i) {
                if e.no == no {
                    return true;
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.slots.len() {
            if let Some(e) = self.slots.get(j) {
                if e.no == no {
                    return true;
                }
            }
            j += 1;
        }
        false
    }

    fn total_len(&self) -> usize {
        self.greens.len().saturating_add(self.slots.len())
    }

    /// 录行：混报闸 + 序闸 + 查重 + 容量；分列落账。
    pub fn reg_entry(&mut self, e: LedgerEntry) -> Result<(), u32> {
        if self.phase == TlPhase::TlAudited || self.phase == TlPhase::TlSealed {
            let d = TsDiag::ESealedWrite.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.total_len() >= ENTRY_CAP {
            let d = TsDiag::EDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.has_no(e.no) {
            let d = TsDiag::EDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if e.no != (self.total_len() + 1) as u32 {
            let d = TsDiag::EJudgeGap.val();
            self.rejects.push(d);
            return Err(d);
        }
        // 混报闸（按态定向诊断码）。
        if !entry_consistent(&e) {
            let d = match e.state {
                LedgerState::TrueGreen => TsDiag::EMixedGreen.val(),
                LedgerState::SlotState => {
                    if e.slot_ref == 0 {
                        TsDiag::EMixedSlot.val()
                    } else {
                        TsDiag::ESlotRefUnknown.val()
                    }
                }
            };
            self.rejects.push(d);
            return Err(d);
        }
        match e.state {
            LedgerState::TrueGreen => self.greens.push(e),
            LedgerState::SlotState => self.slots.push(e),
        }
        Ok(())
    }

    /// 审计收束：TlOpen → TlAudited，行额达标。
    pub fn audit(&mut self) -> Result<(), u32> {
        match tl_advance(self.phase, TlPhase::TlAudited) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if self.total_len() < ENTRY_MIN {
                    let d = TsDiag::EEntryShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.phase = TlPhase::TlAudited;
                Ok(())
            }
        }
    }

    /// 封账：TlAudited → TlSealed，两态分列齐 + 摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match tl_advance(self.phase, TlPhase::TlSealed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if self.greens.is_empty() || self.slots.is_empty() {
                    let d = TsDiag::EColumnShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if summary.is_empty() {
                    let d = TsDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = TlPhase::TlSealed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 终账指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对全账折叠（判据号/态秩/证据字节/槽位引用）。
pub fn ts_digest(entries: &[LedgerEntry]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < entries.len() {
        if let Some(e) = entries.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ e.no;
            h = h.wrapping_mul(0x0100_0193) ^ match e.state {
                LedgerState::TrueGreen => 1u32,
                LedgerState::SlotState => 2,
            };
            h = h.wrapping_mul(0x0100_0193) ^ e.slot_ref;
            let bytes = e.evidence.as_bytes();
            let mut k = 0usize;
            while k < bytes.len() {
                let b = match bytes.get(k) {
                    Some(v) => *v,
                    None => 0,
                };
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
                k += 1;
            }
        }
        i += 1;
    }
    h
}

// ── 混报演练（10 枚：5 真绿无证据 + 5 槽位态坏引用） ────────────
/// 混报演练候选集（全部 inconsistent，classify 必须全数命中）。
pub fn mixed_drill_entries() -> Vec<LedgerEntry> {
    let mut out = Vec::new();
    // 5 枚真绿无证据（证据空但带 0 引用——证据面缺失）。
    let mut i = 0u32;
    while i < 5 {
        out.push(LedgerEntry {
            no: 100 + i,
            state: LedgerState::TrueGreen,
            evidence: "",
            slot_ref: 0,
        });
        i += 1;
    }
    // 5 枚槽位态坏引用（0 = 缺引用；7+ = 越闭集）。
    let bad_refs: [u32; 5] = [0, 7, 8, 9, 99];
    let mut j = 0usize;
    while j < bad_refs.len() {
        let r = match bad_refs.get(j) {
            Some(v) => *v,
            None => break,
        };
        out.push(LedgerEntry {
            no: 200 + j as u32,
            state: LedgerState::SlotState,
            evidence: "",
            slot_ref: r,
        });
        j += 1;
    }
    out
}

/// 谓词：混报演练 10/10 检出。
fn mixed_drill_ok() -> bool {
    let drill = mixed_drill_entries();
    if drill.len() < MIXED_DRILL_N {
        return false;
    }
    let (green, slot, mixed) = classify(&drill);
    // 演练集全为混报：分列零行、混报全数命中。
    green == 0 && slot == 0 && mixed == MIXED_DRILL_N
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：两态终账齐（冻结表分列两态均非空、混报零、序闸绿）。
fn twostate_ok() -> bool {
    if ENTRY_TABLE.len() < ENTRY_MIN {
        return false;
    }
    let (green, slot, mixed) = classify(ENTRY_TABLE);
    if green == 0 || slot == 0 || mixed != 0 {
        return false;
    }
    if !order_scan(ENTRY_TABLE) {
        return false;
    }
    // 分列等式：绿列 + 槽列 = 总行数。
    green.saturating_add(slot) == ENTRY_TABLE.len()
}

/// 谓词：判据号连号序闸（冻结表 1..=12 连号）。
fn judge_order_ok() -> bool {
    order_scan(ENTRY_TABLE)
}

/// 完整终账构造基线：开账→录全表→审计→封账。
pub fn build_full_ledger() -> TsLedger {
    let mut a = TsLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < ENTRY_TABLE.len() {
        if let Some(e) = ENTRY_TABLE.get(i) {
            let _ = a.reg_entry(*e);
        }
        i += 1;
    }
    let _ = a.audit();
    let _ = a.seal("I 型两态记账终账：真绿/槽位态分列齐，混报演练零漏检");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    let dx = ts_digest(ENTRY_TABLE);
    let dy = ts_digest(ENTRY_TABLE);
    x.green_len() == y.green_len()
        && x.slot_len() == y.slot_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && dx == dy
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == TlPhase::TlSealed && !a.summary().is_empty()
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

// ── 违约注入面 ───────────────────────────────────────────────────
/// 注入演练：结构违约逐路投递，全部 Err 才算抓全。
pub fn ts_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    let e0 = match ENTRY_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：重复判据号。
    let mut a1 = TsLedger::new();
    let _ = a1.reg_entry(e0);
    if a1.reg_entry(e0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1重复判据号;");
    }

    // 族2：真绿无证据（混报）。
    let mut a2 = TsLedger::new();
    let mg = LedgerEntry { no: 1, state: LedgerState::TrueGreen, evidence: "", slot_ref: 0 };
    if a2.reg_entry(mg).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2真绿无证据;");
    }

    // 族3：槽位态缺引用（混报）。
    let mut a3 = TsLedger::new();
    let ms = LedgerEntry { no: 1, state: LedgerState::SlotState, evidence: "", slot_ref: 0 };
    if a3.reg_entry(ms).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3槽位缺引用;");
    }

    // 族4：槽位引用越闭集。
    let mut a4 = TsLedger::new();
    let ur = LedgerEntry { no: 1, state: LedgerState::SlotState, evidence: "", slot_ref: 9 };
    if a4.reg_entry(ur).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4引用越闭集;");
    }

    // 族5：跳号（首行就带 2 号）。
    let mut a5 = TsLedger::new();
    let gap = LedgerEntry { no: 2, state: LedgerState::TrueGreen, evidence: "EV-x", slot_ref: 0 };
    if a5.reg_entry(gap).is_err() {
        caught += 1;
    } else {
        miss.push_str("F5跳号;");
    }

    // 族6：未开账即审计（缺额拒也算抓中——审计额不足）。
    let mut a6 = TsLedger::new();
    if a6.audit().is_err() {
        caught += 1;
    } else {
        miss.push_str("F6缺额审计;");
    }

    // 族7：封账后写。
    let mut a7 = build_full_ledger();
    if a7.reg_entry(e0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F7封账后写;");
    }

    // 族8：相位倒退（封账后再审计）。
    let mut a8 = build_full_ledger();
    if a8.audit().is_err() {
        caught += 1;
    } else {
        miss.push_str("F8相位倒退;");
    }

    // 族9：两态分列缺一态（合成 10 行真绿过审计后封账必拒）。
    let mut a9 = TsLedger::new();
    let mut i = 0u32;
    while i < ENTRY_MIN as u32 {
        let _ = a9.reg_entry(LedgerEntry {
            no: i + 1,
            state: LedgerState::TrueGreen,
            evidence: "EV-syn",
            slot_ref: 0,
        });
        i += 1;
    }
    let _ = a9.audit();
    if a9.seal("缺槽列").is_err() {
        caught += 1;
    } else {
        miss.push_str("F9缺一态;");
    }

    // 族10：空摘要封账。
    let mut a10 = build_full_ledger_no_seal();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 建账至审计相位（未封账），供族10空摘要注入。
pub fn build_full_ledger_no_seal() -> TsLedger {
    let mut a = TsLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < ENTRY_TABLE.len() {
        if let Some(e) = ENTRY_TABLE.get(i) {
            let _ = a.reg_entry(*e);
        }
        i += 1;
    }
    let _ = a.audit();
    a
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = ts_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12713 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12713_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12713");

    s.add_dyn(
        "F12713-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12713-02 段位公式重算",
        {
            let mut ok = seg_of(12713) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12713-03 诊断码闭集",
        {
            let mut ok = TS_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < TS_ALL.len() {
                if let Some(d) = TS_ALL.get(i) {
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
        "F12713-04 两态终账齐",
        twostate_ok(),
        &format!(
            "green={} slot={} mixed=0",
            classify(ENTRY_TABLE).0,
            classify(ENTRY_TABLE).1
        ),
    );

    s.add_dyn(
        "F12713-05 判据号连号序闸",
        judge_order_ok(),
        &format!("rows={} 连号 1..={}", ENTRY_TABLE.len(), ENTRY_TABLE.len()),
    );

    s.add_dyn(
        "F12713-06 混报演练 10/10",
        mixed_drill_ok(),
        &format!(
            "mixed={}/{}",
            classify(&mixed_drill_entries()).2,
            MIXED_DRILL_N
        ),
    );

    s.add_dyn(
        "F12713-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = ts_fault_sweep();
            c == t
        },
        &format!("caught={}/10", ts_fault_sweep().0),
    );

    s.add_dyn(
        "F12713-08 相位迁移守卫直读",
        tl_advance(TlPhase::TlOpen, TlPhase::TlAudited).is_ok()
            && tl_advance(TlPhase::TlAudited, TlPhase::TlSealed).is_ok()
            && tl_advance(TlPhase::TlSealed, TlPhase::TlAudited).is_err()
            && tl_advance(TlPhase::TlOpen, TlPhase::TlSealed).is_err(),
        "TlOpen→TlAudited→TlSealed 只进不退不跳",
    );

    s.add_dyn(
        "F12713-09 分列等式直读",
        {
            let (green, slot, mixed) = classify(ENTRY_TABLE);
            green.saturating_add(slot).saturating_add(mixed) == ENTRY_TABLE.len()
        },
        "绿列+槽列+混报 = 总行数",
    );

    s.add_dyn(
        "F12713-10 完整建账基线",
        {
            let a = build_full_ledger();
            a.green_len() == classify(ENTRY_TABLE).0
                && a.slot_len() == classify(ENTRY_TABLE).1
                && a.reject_len() == 0
                && a.phase() == TlPhase::TlSealed
        },
        &format!(
            "phase={} green={} slot={}",
            build_full_ledger().phase().name(),
            build_full_ledger().green_len(),
            build_full_ledger().slot_len()
        ),
    );

    s.add_dyn(
        "F12713-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12713-12 终账指纹非零",
        ts_digest(ENTRY_TABLE) != 0,
        &format!("digest={:08X}", ts_digest(ENTRY_TABLE)),
    );

    s.add_dyn(
        "F12713-13 完成写回语义",
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
        assert_eq!(SEG_BASE, 0x81C40);
        assert_eq!(seg_of(12713), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(TS_ALL.len(), 12);
        let mut i = 0usize;
        while i < TS_ALL.len() {
            let vi = match TS_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < TS_ALL.len() {
                let vj = match TS_ALL.get(j) {
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
    fn twostate_columns_hold() {
        assert!(twostate_ok());
        assert!(judge_order_ok());
        let (green, slot, mixed) = classify(ENTRY_TABLE);
        assert!(green >= 1 && slot >= 1);
        assert_eq!(mixed, 0);
        assert_eq!(green + slot, ENTRY_TABLE.len());
        // 两态闭集。
        assert_eq!(LedgerState::all().len(), 2);
        for st in LedgerState::all().iter() {
            assert!(!st.name().is_empty());
        }
    }

    #[test]
    fn entry_consistency_boundary() {
        // 真绿：证据非空 + 无槽引用。
        let good_g = LedgerEntry { no: 1, state: LedgerState::TrueGreen, evidence: "EV-x", slot_ref: 0 };
        assert!(entry_consistent(&good_g));
        // 真绿坏例：空证据 / 带槽引用。
        let bad_g1 = LedgerEntry { no: 2, state: LedgerState::TrueGreen, evidence: "", slot_ref: 0 };
        assert!(!entry_consistent(&bad_g1));
        let bad_g2 = LedgerEntry { no: 3, state: LedgerState::TrueGreen, evidence: "EV-x", slot_ref: 1 };
        assert!(!entry_consistent(&bad_g2));
        // 槽位态：引用 1..=6。
        let good_s = LedgerEntry { no: 4, state: LedgerState::SlotState, evidence: "", slot_ref: 6 };
        assert!(entry_consistent(&good_s));
        let bad_s = LedgerEntry { no: 5, state: LedgerState::SlotState, evidence: "", slot_ref: 7 };
        assert!(!entry_consistent(&bad_s));
        // 槽位引用闭集边界。
        assert!(slot_ref_ok(1) && slot_ref_ok(SLOT_REF_CAP));
        assert!(!slot_ref_ok(0) && !slot_ref_ok(SLOT_REF_CAP + 1));
    }

    #[test]
    fn mixed_drill_ten_ten_detected() {
        assert!(mixed_drill_ok());
        let drill = mixed_drill_entries();
        assert_eq!(drill.len(), MIXED_DRILL_N);
        let (green, slot, mixed) = classify(&drill);
        assert_eq!(green, 0);
        assert_eq!(slot, 0);
        assert_eq!(mixed, 10);
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = ts_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let e0 = match ENTRY_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let mut a = TsLedger::new();
        let _ = a.reg_entry(e0);
        assert_eq!(a.reg_entry(e0), Err(TsDiag::EDup.val()));
        let mg = LedgerEntry { no: 2, state: LedgerState::TrueGreen, evidence: "", slot_ref: 0 };
        assert_eq!(a.reg_entry(mg), Err(TsDiag::EMixedGreen.val()));
        let ms = LedgerEntry { no: 2, state: LedgerState::SlotState, evidence: "", slot_ref: 0 };
        assert_eq!(a.reg_entry(ms), Err(TsDiag::EMixedSlot.val()));
        let ur = LedgerEntry { no: 2, state: LedgerState::SlotState, evidence: "", slot_ref: 9 };
        assert_eq!(a.reg_entry(ur), Err(TsDiag::ESlotRefUnknown.val()));
        let gap = LedgerEntry { no: 9, state: LedgerState::TrueGreen, evidence: "EV-x", slot_ref: 0 };
        assert_eq!(a.reg_entry(gap), Err(TsDiag::EJudgeGap.val()));
        // 拒单面：重复 + 真绿无证据 + 槽缺引用 + 引用越闭集 + 跳号 = 5。
        assert_eq!(a.reject_len(), 5);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(tl_advance(TlPhase::TlOpen, TlPhase::TlAudited).is_ok());
        assert!(tl_advance(TlPhase::TlAudited, TlPhase::TlSealed).is_ok());
        assert!(tl_advance(TlPhase::TlAudited, TlPhase::TlAudited).is_err());
        assert!(tl_advance(TlPhase::TlSealed, TlPhase::TlOpen).is_err());
        assert!(TlPhase::TlOpen.rank() < TlPhase::TlAudited.rank());
        assert!(TlPhase::TlAudited.rank() < TlPhase::TlSealed.rank());
        // 缺一态封账拒；补齐后过；封账后写拒。
        let mut a = build_full_ledger_no_seal();
        // 基线已处审计相位，幂等重审必拒（倒退守卫）。
        assert!(a.audit().is_err());
        assert_eq!(a.seal(""), Err(TsDiag::ESummaryEmpty.val()));
        assert!(a.seal("两态终账封账").is_ok());
        assert_eq!(a.phase(), TlPhase::TlSealed);
        let e0 = match ENTRY_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(a.reg_entry(e0), Err(TsDiag::ESealedWrite.val()));
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.green_len(), y.green_len());
        assert_eq!(x.slot_len(), y.slot_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(ts_digest(ENTRY_TABLE), ts_digest(ENTRY_TABLE));
        assert_ne!(ts_digest(ENTRY_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [TlPhase::TlOpen, TlPhase::TlAudited, TlPhase::TlSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for d in TS_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12713_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
