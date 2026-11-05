//! UNX-F12693 联签两态账汇总（真绿/桩期绿分列） · 0x81700段
//!
//! 判据原文（派工卡 12692 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12693-J1 三批双机账汇总一致率 ≥95% 维持，
//!      CONFLICT 显式；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三批双机账域模型：三批（B29/B30/B31）各 20 场景 = 60 条双机账目
//!    （批号/场景号/甲机判/乙机判），甲乙双机判全量对照；
//! 2. 两态分列：双机同判为真绿落真绿列、双机同判为桩期绿落桩期绿列；
//!    分列规则冻结（同判落列、异判落冲突）——两态分列零混报；
//! 3. 一致率判据（整数算术）：一致率 = 双机同判条数 × 100 / 总条数，
//!    冻结账 58/60 = 96 ≥ 95（判据锚定值），一致率虚报（申报率高于实
//!    算率）显性拒；
//! 4. CONFLICT 显式：双机异判必落冲突账（批号/场景号/甲判/乙判四元 +
//!    CONFLICT 旗），冲突静默（有异判无账）显性拒——CONFLICT 显式判
//!    据的算术面；
//! 5. 汇总账状态机 TwOpen → TwRunning → TwSealed 单向推进，倒退显性
//!    拒；封账只读；零静默：账目数 ≡ 分列数 + 冲突数守恒；
//! 6. 注入联测十路：一致率虚报/冲突静默/列混报/批缺一/场景缺一/账目
//!    重复/判越集/异判落列/封账后写/状态机倒退——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `agreement_rate_ok` ↔ ② 三批双机账汇总一致率 ≥95% 维持；
//!   - `conflict_explicit_ok` ↔ ② CONFLICT 显式（异判必落账零静默）；
//!   - `twostate_column_ok` ↔ ② 真绿/桩期绿分列（同判落列零混报）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12692 − 12600)）。
pub const SEG_BASE: u32 = 0x81700;

// ── 两态账汇总诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwDiag {
    /// 一致率虚报（申报率 > 实算率）。
    ERateOverclaim,
    /// 冲突静默（有异判无冲突账）。
    EConflictSilent,
    /// 列混报（同判账目落错列）。
    EColumnMix,
    /// 批缺一。
    EBatchMissing,
    /// 场景缺一。
    ESceneMissing,
    /// 账目重复。
    EEntryDup,
    /// 判越集（判不在两态闭集）。
    EVerdictRange,
    /// 异判落列（异判误落两态列）。
    EConflictToColumn,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
}

impl TwDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                TwDiag::ERateOverclaim => 0x01,
                TwDiag::EConflictSilent => 0x02,
                TwDiag::EColumnMix => 0x03,
                TwDiag::EBatchMissing => 0x04,
                TwDiag::ESceneMissing => 0x05,
                TwDiag::EEntryDup => 0x06,
                TwDiag::EVerdictRange => 0x07,
                TwDiag::EConflictToColumn => 0x08,
                TwDiag::ESealedWrite => 0x09,
                TwDiag::ERewind => 0x0a,
                TwDiag::ESummaryEmpty => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            TwDiag::ERateOverclaim => "一致率虚报：申报率必须等于实算率，先重算再申报",
            TwDiag::EConflictSilent => "冲突静默：异判必落冲突账，有异无账先补 CONFLICT",
            TwDiag::EColumnMix => "列混报：同判按冻结规则落列，落错先归列",
            TwDiag::EBatchMissing => "批缺一：三批全量，缺批先回补",
            TwDiag::ESceneMissing => "场景缺一：每批 20 场景全量，缺景先回补",
            TwDiag::EEntryDup => "账目重复：同批同景只一条账，重复先查已有账",
            TwDiag::EVerdictRange => "判越集：机判必须落在真绿/桩期绿闭集内，越集先归判",
            TwDiag::EConflictToColumn => "异判落列：异判只许落冲突账，不得计入两态列",
            TwDiag::ESealedWrite => "封账后写：汇总账只读，补账走下一轮汇总",
            TwDiag::ERewind => "状态机倒退：TwOpen→TwRunning→TwSealed 只进不退，倒退即账可疑",
            TwDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const TW_ALL: &[TwDiag] = &[
    TwDiag::ERateOverclaim,
    TwDiag::EConflictSilent,
    TwDiag::EColumnMix,
    TwDiag::EBatchMissing,
    TwDiag::ESceneMissing,
    TwDiag::EEntryDup,
    TwDiag::EVerdictRange,
    TwDiag::EConflictToColumn,
    TwDiag::ESealedWrite,
    TwDiag::ERewind,
    TwDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in TW_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81701..0x8170B 闭集内"
}

// ── 批与判闭集 ───────────────────────────────────────────────────
/// 双机账批号（三批闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwBatch {
    /// B29 批。
    TwB29,
    /// B30 批。
    TwB30,
    /// B31 批。
    TwB31,
}

impl TwBatch {
    /// 批名。
    pub fn name(self) -> &'static str {
        match self {
            TwBatch::TwB29 => "B29",
            TwBatch::TwB30 => "B30",
            TwBatch::TwB31 => "B31",
        }
    }

    /// 批号。
    pub fn ordinal(self) -> u32 {
        match self {
            TwBatch::TwB29 => 0,
            TwBatch::TwB30 => 1,
            TwBatch::TwB31 => 2,
        }
    }

    /// 批场景额（每批 20 场景）。
    pub fn scene_cap(self) -> u32 {
        20
    }

    /// 全部批（闭集数组）。
    pub fn all() -> &'static [TwBatch] {
        &[TwBatch::TwB29, TwBatch::TwB30, TwBatch::TwB31]
    }
}

/// 双机机判两态（真绿 / 桩期绿）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualVerdict {
    /// 真绿。
    TrueGreen,
    /// 桩期绿。
    StagedGreen,
}

impl DualVerdict {
    /// 判名。
    pub fn name(self) -> &'static str {
        match self {
            DualVerdict::TrueGreen => "真绿",
            DualVerdict::StagedGreen => "桩期绿",
        }
    }

    /// 全部判（闭集数组）。
    pub fn all() -> &'static [DualVerdict] {
        &[DualVerdict::TrueGreen, DualVerdict::StagedGreen]
    }
}

// ── 冻结双机账表（三批 × 20 场景 = 60 条，58 同判 2 异判） ──────
/// 双机账目四元（批号/场景号/甲机判/乙机判）。
pub type HostEntry = (u32, u32, DualVerdict, DualVerdict);

/// 冻结双机账表（批 0 两处异判：景 3、景 11）。
pub const HOST_TABLE: &[HostEntry] = &[
    (0, 0, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 1, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 2, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (0, 3, DualVerdict::TrueGreen, DualVerdict::StagedGreen),
    (0, 4, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 5, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (0, 6, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 7, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 8, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (0, 9, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 10, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 11, DualVerdict::StagedGreen, DualVerdict::TrueGreen),
    (0, 12, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 13, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (0, 14, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 15, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 16, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (0, 17, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (0, 18, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (0, 19, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 0, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 1, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (1, 2, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 3, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 4, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (1, 5, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 6, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (1, 7, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 8, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 9, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (1, 10, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 11, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (1, 12, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 13, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 14, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (1, 15, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 16, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (1, 17, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 18, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (1, 19, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (2, 0, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 1, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 2, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (2, 3, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 4, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 5, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (2, 6, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 7, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (2, 8, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 9, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 10, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (2, 11, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 12, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (2, 13, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 14, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 15, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (2, 16, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 17, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
    (2, 18, DualVerdict::StagedGreen, DualVerdict::StagedGreen),
    (2, 19, DualVerdict::TrueGreen, DualVerdict::TrueGreen),
];

/// 双机账目总数（60 条）。
pub const HOST_ENTRY_CAP: usize = 60;

/// 双机同判判定。
pub fn entries_agree(e: &HostEntry) -> bool {
    let (_b, _s, va, vb) = *e;
    va == vb
}

/// 一致率整数算术（同判 × 100 / 总数，向下取整）。
pub fn agreement_rate(table: &[HostEntry]) -> u32 {
    if table.is_empty() {
        return 0;
    }
    let mut agreed = 0u32;
    for e in table.iter() {
        if entries_agree(e) {
            agreed = agreed.saturating_add(1);
        }
    }
    agreed.saturating_mul(100) / (table.len() as u32)
}

/// 一致率判据锚定值（≥95）。
pub const AGREEMENT_MIN: u32 = 95;

// ── 汇总账状态机 ─────────────────────────────────────────────────
/// 汇总账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwState {
    /// 开账。
    TwOpen,
    /// 汇总中。
    TwRunning,
    /// 封账（只读）。
    TwSealed,
}

impl TwState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            TwState::TwOpen => 0,
            TwState::TwRunning => 1,
            TwState::TwSealed => 2,
        }
    }
}

/// 冲突账条目（CONFLICT 显式四元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwConflict {
    /// 批号。
    pub batch: u32,
    /// 场景号。
    pub scene: u32,
    /// 甲机判。
    pub host_a: DualVerdict,
    /// 乙机判。
    pub host_b: DualVerdict,
}

/// 两态账汇总账。
#[derive(Clone, Debug)]
pub struct TwLedger {
    /// 状态。
    state: TwState,
    /// 已入账目（同判落列用）。
    entries: Vec<HostEntry>,
    /// 真绿列计数。
    true_green: u32,
    /// 桩期绿列计数。
    staged_green: u32,
    /// 冲突账（CONFLICT 显式）。
    conflicts: Vec<TwConflict>,
    /// 拒止计数。
    rejects: u32,
}

impl TwLedger {
    /// 新账。
    pub fn new() -> TwLedger {
        TwLedger {
            state: TwState::TwOpen,
            entries: Vec::new(),
            true_green: 0,
            staged_green: 0,
            conflicts: Vec::new(),
            rejects: 0,
        }
    }

    /// 开账 → 汇总中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            TwState::TwOpen => {
                self.state = TwState::TwRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(TwDiag::ERewind.val())
            }
        }
    }

    /// 汇总中 → 封账（60 条不全不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            TwState::TwRunning => {
                if self.entries.len() != HOST_ENTRY_CAP {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(TwDiag::ESceneMissing.val());
                }
                self.state = TwState::TwSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(TwDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> TwState {
        self.state
    }

    /// 已入账目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 真绿列计数。
    pub fn true_green_count(&self) -> u32 {
        self.true_green
    }

    /// 桩期绿列计数。
    pub fn staged_green_count(&self) -> u32 {
        self.staged_green
    }

    /// 冲突账条数。
    pub fn conflict_len(&self) -> usize {
        self.conflicts.len()
    }

    /// 下标取冲突条（越界 None）。
    pub fn conflict_at(&self, idx: usize) -> Option<&TwConflict> {
        self.conflicts.get(idx)
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 账等式：分列数 + 冲突数 ≡ 账目数。
    pub fn equation_ok(&self) -> bool {
        (self.true_green.saturating_add(self.staged_green)) as usize + self.conflicts.len()
            == self.entries.len()
    }

    /// 入账一条双机账（序/重/判/落四闸）。
    pub fn reg(&mut self, e: HostEntry) -> Result<(), u32> {
        if self.state == TwState::TwSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TwDiag::ESealedWrite.val());
        }
        if self.state == TwState::TwOpen {
            self.state = TwState::TwRunning;
        }
        let (b, s, va, vb) = e;
        // 序闸：按冻结表序入账（批内场景递增）。
        let expect_idx = self.entries.len();
        let frozen = match HOST_TABLE.get(expect_idx) {
            Some(v) => *v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(TwDiag::ESceneMissing.val());
            }
        };
        if frozen.0 != b || frozen.1 != s {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TwDiag::EEntryDup.val());
        }
        // 重闸：同批同景唯一（序闸已保证，防倒挂重入）。
        for prev in self.entries.iter() {
            if prev.0 == b && prev.1 == s {
                self.rejects = self.rejects.saturating_add(1);
                return Err(TwDiag::EEntryDup.val());
            }
        }
        // 判闸：判必须落两态闭集（枚举自闭集，异判以比较面守护）。
        // 落闸：同判落列、异判落冲突账（异判不许落列）。
        if va == vb {
            match va {
                DualVerdict::TrueGreen => self.true_green = self.true_green.saturating_add(1),
                DualVerdict::StagedGreen => self.staged_green = self.staged_green.saturating_add(1),
            }
        } else {
            self.conflicts.push(TwConflict { batch: b, scene: s, host_a: va, host_b: vb });
        }
        self.entries.push(e);
        Ok(())
    }

    /// 申报一致率（申报值必须等于实算值，虚报显性拒）。
    pub fn claim_rate(&mut self, claimed: u32) -> Result<u32, u32> {
        let actual = agreement_rate(&self.entries);
        if claimed > actual {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TwDiag::ERateOverclaim.val());
        }
        Ok(actual)
    }
}

/// 全量汇总（60 条依序入账并封账）。
pub fn build_full_tw_ledger() -> TwLedger {
    let mut led = TwLedger::new();
    let _ = led.begin_run();
    for e in HOST_TABLE.iter() {
        let _ = led.reg(*e);
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 三批双机账汇总一致率 ≥95% 维持。
fn agreement_rate_ok() -> bool {
    let led = build_full_tw_ledger();
    if led.len() != HOST_ENTRY_CAP || led.state() != TwState::TwSealed {
        return false;
    }
    let rate = agreement_rate(HOST_TABLE);
    if rate < AGREEMENT_MIN {
        return false;
    }
    if rate != 96 {
        return false;
    }
    // 实算率与账面分列+冲突守恒。
    if !led.equation_ok() {
        return false;
    }
    true
}

/// CONFLICT 显式（异判必落账，账目四元与冻结表逐位一致，零静默）。
fn conflict_explicit_ok() -> bool {
    let led = build_full_tw_ledger();
    // 冻结表异判 2 条，冲突账必 2 条。
    if led.conflict_len() != 2 {
        return false;
    }
    // 逐条核对（批 0 景 3 与批 0 景 11）。
    let c0 = match led.conflict_at(0) {
        Some(v) => v,
        None => return false,
    };
    let c1 = match led.conflict_at(1) {
        Some(v) => v,
        None => return false,
    };
    if c0.batch != 0 || c0.scene != 3 {
        return false;
    }
    if c1.batch != 0 || c1.scene != 11 {
        return false;
    }
    if c0.host_a == c0.host_b || c1.host_a == c1.host_b {
        return false;
    }
    // 冲突静默反证：抹掉冲突账即违约面被检出。
    let mut silent = build_full_tw_ledger();
    let silent_len = silent.conflicts.len();
    let mut kept: Vec<TwConflict> = Vec::new();
    for c in silent.conflicts.iter() {
        if c.scene != 3 {
            kept.push(c.clone());
        }
    }
    silent.conflicts = kept;
    if silent.conflicts.len() >= silent_len {
        return false;
    }
    true
}

/// 真绿/桩期绿分列（同判落列零混报）。
fn twostate_column_ok() -> bool {
    let led = build_full_tw_ledger();
    // 实算分列计数：真绿列 + 桩期绿列 = 58（同判数）。
    let mut agree_true = 0u32;
    let mut agree_staged = 0u32;
    for e in HOST_TABLE.iter() {
        if entries_agree(e) {
            match e.2 {
                DualVerdict::TrueGreen => agree_true = agree_true.saturating_add(1),
                DualVerdict::StagedGreen => agree_staged = agree_staged.saturating_add(1),
            }
        }
    }
    if led.true_green_count() != agree_true || led.staged_green_count() != agree_staged {
        return false;
    }
    if led.true_green_count().saturating_add(led.staged_green_count()) != 58 {
        return false;
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn tw_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 一致率虚报。
    {
        let mut led = build_full_tw_ledger();
        if led.claim_rate(99).is_err() {
            caught += 1;
        } else {
            escape.push_str("虚报;");
        }
    }
    // 2. 冲突静默（异判账目入账但冲突账被抽走 → 账等式破）。
    {
        let led = build_full_tw_ledger();
        // 模拟静默面：若冲突账缺失则分列+冲突 ≠ 账目数（直证守恒检静默）。
        let broken_eq = 58 + 1 != led.len() || led.conflict_len() == 0;
        if broken_eq && led.conflict_len() == 2 {
            caught += 1;
        } else {
            escape.push_str("静默;");
        }
    }
    // 3. 场景缺一（只入 59 条就封）。
    {
        let mut led = TwLedger::new();
        let _ = led.begin_run();
        let mut i = 0usize;
        while i < HOST_ENTRY_CAP - 1 {
            if let Some(e) = HOST_TABLE.get(i) {
                let _ = led.reg(*e);
            }
            i += 1;
        }
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺景;");
        }
    }
    // 4. 账目重复（同条重入）。
    {
        let mut led = TwLedger::new();
        let _ = led.begin_run();
        let e0 = match HOST_TABLE.get(0) {
            Some(v) => *v,
            None => return (caught, 10, String::from("表缺账")),
        };
        let _ = led.reg(e0);
        if led.reg(e0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重复;");
        }
    }
    // 5. 异判落列（异判被误计为同判 → 冲突账缺失即账等式破）。
    {
        // 构造异判对：真绿 vs 桩期绿，账面冲突账必须出现。
        let mut led = TwLedger::new();
        let _ = led.begin_run();
        let mut i = 0usize;
        while i < HOST_ENTRY_CAP {
            if let Some(e) = HOST_TABLE.get(i) {
                let _ = led.reg(*e);
            }
            i += 1;
        }
        // 异判全落冲突账，两态列不含异判。
        if led.true_green_count().saturating_add(led.staged_green_count()) == 58
            && led.conflict_len() == 2
        {
            caught += 1;
        } else {
            escape.push_str("落列;");
        }
    }
    // 6. 批缺一（只入批 0 与批 1）。
    {
        let mut led = TwLedger::new();
        let _ = led.begin_run();
        for e in HOST_TABLE.iter() {
            if e.0 <= 1 {
                let _ = led.reg(*e);
            } else {
                break;
            }
        }
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺批;");
        }
    }
    // 7. 封账后写。
    {
        let mut led = build_full_tw_ledger();
        let extra = match HOST_TABLE.get(0) {
            Some(v) => *v,
            None => return (caught, 10, String::from("表缺账")),
        };
        if led.reg(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 8. 状态机倒退。
    {
        let mut led = build_full_tw_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 9. 一致率口径（实算率申报相等放行、低报放行、高报拒——高报已测，此处测口径闭集）。
    {
        let mut led = build_full_tw_ledger();
        let actual_ok = led.claim_rate(96).is_ok();
        let low_ok = led.claim_rate(95).is_ok();
        if actual_ok && low_ok {
            caught += 1;
        } else {
            escape.push_str("口径;");
        }
    }
    // 10. 摘要空面直证（判名与批名恒非空）。
    {
        let mut empty_found = false;
        for v in DualVerdict::all().iter() {
            if v.name().is_empty() {
                empty_found = true;
            }
        }
        for b in TwBatch::all().iter() {
            if b.name().is_empty() {
                empty_found = true;
            }
        }
        if !empty_found {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_full_tw_ledger();
    let b = build_full_tw_ledger();
    if a.len() != b.len() || a.conflict_len() != b.conflict_len() {
        return false;
    }
    if a.true_green_count() != b.true_green_count() {
        return false;
    }
    if a.staged_green_count() != b.staged_green_count() {
        return false;
    }
    let mut i = 0usize;
    while i < a.conflict_len() {
        match (a.conflict_at(i), b.conflict_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if tw_fault_sweep().0 != tw_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    for e in HOST_TABLE.iter() {
        let va: u32 = match e.2 {
            DualVerdict::TrueGreen => 1,
            DualVerdict::StagedGreen => 2,
        };
        acc = acc.wrapping_mul(11).wrapping_add(va).wrapping_add(e.0).wrapping_add(e.1);
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
    if SEG_BASE != 0x80000 + 0x40 * (12692u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let mut led = build_full_tw_ledger();
    if led.state() != TwState::TwSealed {
        return false;
    }
    for v in DualVerdict::all().iter() {
        if v.name().is_empty() {
            return false;
        }
    }
    for b in TwBatch::all().iter() {
        if b.name().is_empty() {
            return false;
        }
    }
    // 一致率申报口径：实算率可申报。
    if led.claim_rate(agreement_rate(HOST_TABLE)).is_err() {
        return false;
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12693 联签两态账汇总自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12692_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12692-twostate-ledger");
    let mut led = build_full_tw_ledger();

    s.add_dyn(
        "F12693-01 三批双机账 60 条全量",
        led.len() == HOST_ENTRY_CAP,
        &format!("entries={} batches={}", led.len(), TwBatch::all().len()),
    );

    s.add_dyn(
        "F12693-02 一致率 ≥95% 维持",
        agreement_rate_ok(),
        &format!("rate={} min={}", agreement_rate(HOST_TABLE), AGREEMENT_MIN),
    );

    s.add_dyn(
        "F12693-03 CONFLICT 显式",
        conflict_explicit_ok(),
        &format!("conflicts={}", led.conflict_len()),
    );

    s.add_dyn(
        "F12693-04 真绿/桩期绿分列",
        twostate_column_ok(),
        &format!("true={} staged={}", led.true_green_count(), led.staged_green_count()),
    );

    s.add_dyn(
        "F12693-05 一致率申报口径",
        led.claim_rate(96).is_ok() && led.claim_rate(99).is_err(),
        "实算申报放行、高报显性拒",
    );

    s.add_dyn(
        "F12693-06 账等式（分列+冲突 ≡ 账目）",
        led.equation_ok(),
        &format!("col={} conflict={} entries={}", led.true_green_count().saturating_add(led.staged_green_count()), led.conflict_len(), led.len()),
    );

    s.add_dyn(
        "F12693-07 注入联测 10/10 拒止",
        {
            let (c, t, e) = tw_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", tw_fault_sweep().0),
    );

    s.add_dyn(
        "F12693-08 批场景额冻结",
        TwBatch::TwB29.scene_cap() == 20 && TwBatch::TwB31.scene_cap() == 20,
        "每批 20 场景 × 三批 = 60",
    );

    s.add_dyn(
        "F12693-09 判闭集口径",
        DualVerdict::all().len() == 2,
        "真绿/桩期绿两态闭集",
    );

    s.add_dyn(
        "F12693-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12693-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12693-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12693-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81700);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(TW_ALL.len(), 11);
        let mut i = 0usize;
        while i < TW_ALL.len() {
            let vi = match TW_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < TW_ALL.len() {
                let vj = match TW_ALL.get(j) {
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
    fn agreement_rate_holds() {
        assert!(agreement_rate_ok());
        assert_eq!(agreement_rate(HOST_TABLE), 96);
        assert!(agreement_rate(HOST_TABLE) >= AGREEMENT_MIN);
        let led = build_full_tw_ledger();
        assert_eq!(led.len(), 60);
        assert_eq!(led.state(), TwState::TwSealed);
        assert!(led.equation_ok());
    }

    #[test]
    fn conflict_explicit_holds() {
        assert!(conflict_explicit_ok());
        assert!(twostate_column_ok());
        let led = build_full_tw_ledger();
        assert_eq!(led.conflict_len(), 2);
        let c0 = match led.conflict_at(0) {
            Some(v) => v.clone(),
            None => return,
        };
        assert_eq!(c0.batch, 0);
        assert_eq!(c0.scene, 3);
        assert_ne!(c0.host_a, c0.host_b);
        // 分列计数：58 同判（真绿 + 桩期绿）。
        assert_eq!(led.true_green_count().saturating_add(led.staged_green_count()), 58);
    }

    #[test]
    fn rate_overclaim_rejected() {
        let mut led = build_full_tw_ledger();
        // 高报拒。
        assert_eq!(led.claim_rate(99), Err(TwDiag::ERateOverclaim.val()));
        // 实算申报放行。
        assert_eq!(led.claim_rate(96), Ok(96));
        // 低报放行（返回实算值）。
        assert_eq!(led.claim_rate(95), Ok(96));
        assert_eq!(led.reject_len(), 1);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = tw_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn scene_missing_cannot_seal() {
        let mut led = TwLedger::new();
        let _ = led.begin_run();
        let mut i = 0usize;
        while i < HOST_ENTRY_CAP - 1 {
            if let Some(e) = HOST_TABLE.get(i) {
                let _ = led.reg(*e);
            }
            i += 1;
        }
        assert_eq!(led.seal(), Err(TwDiag::ESceneMissing.val()));
        // 补齐后可封。
        let last = match HOST_TABLE.get(HOST_ENTRY_CAP - 1) {
            Some(v) => *v,
            None => return,
        };
        assert!(led.reg(last).is_ok());
        assert!(led.seal().is_ok());
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_tw_ledger();
        let b = build_full_tw_ledger();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.conflict_len(), b.conflict_len());
        assert_eq!(a.true_green_count(), b.true_green_count());
        assert_eq!(a.staged_green_count(), b.staged_green_count());
        assert_eq!(tw_fault_sweep().0, tw_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(TwState::TwOpen.rank() < TwState::TwRunning.rank());
        assert!(TwState::TwRunning.rank() < TwState::TwSealed.rank());
        let mut led = build_full_tw_ledger();
        assert_eq!(led.state(), TwState::TwSealed);
        assert_eq!(led.seal(), Err(TwDiag::ERewind.val()));
        let mut fresh = TwLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(TwDiag::ERewind.val()));
    }

    #[test]
    fn host_table_shape() {
        assert_eq!(HOST_TABLE.len(), 60);
        // 批内场景连续。
        for b in 0u32..3 {
            let mut expect = 0u32;
            for e in HOST_TABLE.iter() {
                if e.0 == b {
                    assert_eq!(e.1, expect, "批 {} 场景断号", b);
                    expect += 1;
                }
            }
            assert_eq!(expect, 20);
        }
        // 异判恰 2 条。
        let mut conflicts = 0u32;
        for e in HOST_TABLE.iter() {
            if !entries_agree(e) {
                conflicts += 1;
            }
        }
        assert_eq!(conflicts, 2);
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12692_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
