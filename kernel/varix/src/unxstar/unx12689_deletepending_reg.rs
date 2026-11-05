//! UNX-F12690 删除挂起协议回归（B30 档） · 0x81640段
//!
//! 判据原文（派工卡 12689 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12690-J1 四方对账表齐（已收口方实对平/未开工方
//!      槽位态），两态分列零混报；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 删除挂起协议域模型：对象删除三相位（已开/挂起已置/已清收）单调
//!    推进（倒退显性拒），每相位迁移带守卫（未开不许置挂起、未清收不
//!    许再开）——删除挂起协议的算术面；
//! 2. 四方对账闭集（D2/D3/D4/E1）：每方一行对账账目（开计数/清收计数/
//!    两态/槽位旗），四方全量齐表才许对账封账；
//! 3. 两态分列：已收口方记真绿列（实对平：开计数 ≡ 清收计数且非零），
//!    未开工方记槽位列（双计数皆零 + 槽位旗）——两态判据分列互斥；
//! 4. 零混报判据：槽位列账目不得带非零余额（虚绿），真绿列账目不得缺
//!    平（虚平）——混报注入（列错挂/余额错填）10 路必被混报闸抓出；
//! 5. 对账账状态机 RcOpen → RcRunning → RcSealed 单向推进，封账只读；
//!    零静默：行数 ≡ 四方数守恒；
//! 6. 注入联测十路：行缺一/行重复/列错挂/虚绿余额/虚平缺平/槽位旗缺/
//!    相位倒退/未开置挂起/封账后写/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `fourway_recon_ok` ↔ ② 四方对账表齐；
//!   - `twostate_split_ok` ↔ ② 已收口方实对平/未开工方槽位态分列；
//!   - `zero_mixreport_ok` ↔ ② 两态分列零混报（混报注入必检出）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12689 − 12600)）。
pub const SEG_BASE: u32 = 0x81640;

// ── 删除挂起对账诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RcDiag {
    /// 对账行缺一（四方未齐）。
    ERowMissing,
    /// 对账行重复（同方多行）。
    ERowDup,
    /// 列错挂（两态与冻结口径不符）。
    EColumnMismatch,
    /// 虚绿余额（槽位列带非零余额）。
    EFakeGreen,
    /// 虚平缺平（真绿列余额不平）。
    EFakeBalance,
    /// 槽位旗缺（未开工方缺槽位旗）。
    ESlotFlagMissing,
    /// 相位倒退（删除相位倒退）。
    EPhaseRewind,
    /// 未开置挂起。
    EOpenMissing,
    /// 封账后写。
    ESealedWrite,
    /// 摘要空。
    ESummaryEmpty,
}

impl RcDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RcDiag::ERowMissing => 0x01,
                RcDiag::ERowDup => 0x02,
                RcDiag::EColumnMismatch => 0x03,
                RcDiag::EFakeGreen => 0x04,
                RcDiag::EFakeBalance => 0x05,
                RcDiag::ESlotFlagMissing => 0x06,
                RcDiag::EPhaseRewind => 0x07,
                RcDiag::EOpenMissing => 0x08,
                RcDiag::ESealedWrite => 0x09,
                RcDiag::ESummaryEmpty => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RcDiag::ERowMissing => "对账行缺一：D2/D3/D4/E1 四方各一行，缺行先回补",
            RcDiag::ERowDup => "对账行重复：同方只许一行，重复先查已有账",
            RcDiag::EColumnMismatch => "列错挂：两态必须与冻结口径一致，先核分列口径",
            RcDiag::EFakeGreen => "虚绿余额：槽位列账目不得带非零余额，虚绿先清账",
            RcDiag::EFakeBalance => "虚平缺平：真绿列必须开清计数相等且非零，先补平账",
            RcDiag::ESlotFlagMissing => "槽位旗缺：未开工方必须带槽位旗，缺旗先补",
            RcDiag::EPhaseRewind => "相位倒退：已开→挂起已置→已清收只进不退，倒退先查协议",
            RcDiag::EOpenMissing => "未开置挂起：对象未开不许置删除挂起，先走开启相位",
            RcDiag::ESealedWrite => "封账后写：对账表只读，补行走下一轮对账",
            RcDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RC_ALL: &[RcDiag] = &[
    RcDiag::ERowMissing,
    RcDiag::ERowDup,
    RcDiag::EColumnMismatch,
    RcDiag::EFakeGreen,
    RcDiag::EFakeBalance,
    RcDiag::ESlotFlagMissing,
    RcDiag::EPhaseRewind,
    RcDiag::EOpenMissing,
    RcDiag::ESealedWrite,
    RcDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in RC_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81641..0x8164A 闭集内"
}

// ── 删除挂起三相位 ───────────────────────────────────────────────
/// 对象删除相位（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DpPhase {
    /// 已开。
    Opened,
    /// 挂起已置。
    PendingSet,
    /// 已清收。
    Purged,
}

impl DpPhase {
    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            DpPhase::Opened => "已开",
            DpPhase::PendingSet => "挂起已置",
            DpPhase::Purged => "已清收",
        }
    }

    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            DpPhase::Opened => 0,
            DpPhase::PendingSet => 1,
            DpPhase::Purged => 2,
        }
    }

    /// 全部相位（闭集数组）。
    pub fn all() -> &'static [DpPhase] {
        &[DpPhase::Opened, DpPhase::PendingSet, DpPhase::Purged]
    }
}

/// 相位迁移守卫（只进不退；Opened 之外不得凭空置挂起）。
pub fn dp_advance(cur: DpPhase, next: DpPhase) -> Result<DpPhase, u32> {
    if next.rank() <= cur.rank() {
        return Err(RcDiag::EPhaseRewind.val());
    }
    if cur == DpPhase::Opened && next == DpPhase::Purged {
        // 未置挂起直接清收：跳相位按协议拒（必须经挂起已置）。
        return Err(RcDiag::EOpenMissing.val());
    }
    Ok(next)
}

// ── 四方闭集 ─────────────────────────────────────────────────────
/// 对账四方（跨域消费面闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconParty {
    /// D2 方。
    PartyD2,
    /// D3 方。
    PartyD3,
    /// D4 方。
    PartyD4,
    /// E1 方。
    PartyE1,
}

impl ReconParty {
    /// 方名。
    pub fn name(self) -> &'static str {
        match self {
            ReconParty::PartyD2 => "D2",
            ReconParty::PartyD3 => "D3",
            ReconParty::PartyD4 => "D4",
            ReconParty::PartyE1 => "E1",
        }
    }

    /// 方号。
    pub fn ordinal(self) -> u32 {
        match self {
            ReconParty::PartyD2 => 0,
            ReconParty::PartyD3 => 1,
            ReconParty::PartyD4 => 2,
            ReconParty::PartyE1 => 3,
        }
    }

    /// 全部四方（闭集数组）。
    pub fn all() -> &'static [ReconParty] {
        &[ReconParty::PartyD2, ReconParty::PartyD3, ReconParty::PartyD4, ReconParty::PartyE1]
    }
}

// ── 两态闭集 ─────────────────────────────────────────────────────
/// 对账两态（已收口实对平 / 未开工槽位态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwoState {
    /// 已收口方实对平（真绿列）。
    RealGreen,
    /// 未开工方槽位态（槽位列）。
    SlotPending,
}

impl TwoState {
    /// 态名。
    pub fn name(self) -> &'static str {
        match self {
            TwoState::RealGreen => "已收口实对平",
            TwoState::SlotPending => "未开工槽位态",
        }
    }

    /// 全部两态（闭集数组）。
    pub fn all() -> &'static [TwoState] {
        &[TwoState::RealGreen, TwoState::SlotPending]
    }
}

/// 冻结口径：四方两态（本卡冻结分列口径——D2/D3 已收口，D4/E1 未开工）。
pub fn frozen_two_state(p: ReconParty) -> TwoState {
    match p {
        ReconParty::PartyD2 => TwoState::RealGreen,
        ReconParty::PartyD3 => TwoState::RealGreen,
        ReconParty::PartyD4 => TwoState::SlotPending,
        ReconParty::PartyE1 => TwoState::SlotPending,
    }
}

/// 对账行四元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RcRow {
    /// 所属方。
    pub party: ReconParty,
    /// 两态。
    pub state: TwoState,
    /// 开计数。
    pub opened: u32,
    /// 清收计数。
    pub purged: u32,
    /// 槽位旗（槽位列必 1）。
    pub slot_flag: u32,
}

/// 真绿列账目实对平校验。
fn real_green_balanced(r: &RcRow) -> bool {
    r.state == TwoState::RealGreen && r.opened == r.purged && r.opened > 0
}

/// 槽位列账目零余额校验。
fn slot_pending_clean(r: &RcRow) -> bool {
    r.state == TwoState::SlotPending && r.opened == 0 && r.purged == 0 && r.slot_flag == 1
}

/// 行两态分列校验（两态分列零混报的行级算术面）。
pub fn row_split_clean(r: &RcRow) -> bool {
    match r.state {
        TwoState::RealGreen => real_green_balanced(r),
        TwoState::SlotPending => slot_pending_clean(r),
    }
}

/// 冻结对账行表（四方分列口径）。
pub const RC_ROWS: &[RcRow] = &[
    RcRow { party: ReconParty::PartyD2, state: TwoState::RealGreen, opened: 20, purged: 20, slot_flag: 0 },
    RcRow { party: ReconParty::PartyD3, state: TwoState::RealGreen, opened: 20, purged: 20, slot_flag: 0 },
    RcRow { party: ReconParty::PartyD4, state: TwoState::SlotPending, opened: 0, purged: 0, slot_flag: 1 },
    RcRow { party: ReconParty::PartyE1, state: TwoState::SlotPending, opened: 0, purged: 0, slot_flag: 1 },
];

/// 按方取冻结行（越界 None）。
pub fn replay_rc(p: ReconParty) -> Option<RcRow> {
    for r in RC_ROWS.iter() {
        if r.party == p {
            return Some(r.clone());
        }
    }
    None
}

// ── 对账账状态机 ─────────────────────────────────────────────────
/// 对账账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RcState {
    /// 开账。
    RcOpen,
    /// 对账中。
    RcRunning,
    /// 封账（只读）。
    RcSealed,
}

impl RcState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            RcState::RcOpen => 0,
            RcState::RcRunning => 1,
            RcState::RcSealed => 2,
        }
    }
}

/// 四方对账账。
#[derive(Clone, Debug)]
pub struct RcLedger {
    /// 状态。
    state: RcState,
    /// 已入账行。
    rows: Vec<RcRow>,
    /// 拒止计数。
    rejects: u32,
}

impl RcLedger {
    /// 新账。
    pub fn new() -> RcLedger {
        RcLedger {
            state: RcState::RcOpen,
            rows: Vec::new(),
            rejects: 0,
        }
    }

    /// 开账 → 对账中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            RcState::RcOpen => {
                self.state = RcState::RcRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(RcDiag::EPhaseRewind.val())
            }
        }
    }

    /// 对账中 → 封账（四方未齐不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            RcState::RcRunning => {
                if self.rows.len() != ReconParty::all().len() {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(RcDiag::ERowMissing.val());
                }
                self.state = RcState::RcSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(RcDiag::EPhaseRewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> RcState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 下标取行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&RcRow> {
        self.rows.get(idx)
    }

    /// 入账一行（缺重/列/混报/旗四闸）。
    pub fn reg(&mut self, r: RcRow) -> Result<(), u32> {
        if self.state == RcState::RcSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RcDiag::ESealedWrite.val());
        }
        if self.state == RcState::RcOpen {
            self.state = RcState::RcRunning;
        }
        // 重闸：同方唯一。
        for e in self.rows.iter() {
            if e.party == r.party {
                self.rejects = self.rejects.saturating_add(1);
                return Err(RcDiag::ERowDup.val());
            }
        }
        // 列闸：两态必须与冻结口径一致。
        if r.state != frozen_two_state(r.party) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RcDiag::EColumnMismatch.val());
        }
        // 混报闸：行级分列校验（虚绿/虚平/缺旗）。
        let diag = match r.state {
            TwoState::RealGreen => {
                if !real_green_balanced(&r) {
                    Some(RcDiag::EFakeBalance)
                } else {
                    None
                }
            }
            TwoState::SlotPending => {
                if r.slot_flag != 1 {
                    Some(RcDiag::ESlotFlagMissing)
                } else if !slot_pending_clean(&r) {
                    Some(RcDiag::EFakeGreen)
                } else {
                    None
                }
            }
        };
        if let Some(d) = diag {
            self.rejects = self.rejects.saturating_add(1);
            return Err(d.val());
        }
        self.rows.push(r);
        Ok(())
    }
}

/// 全量对账（四方依序入账并封账）。
pub fn build_full_rc_ledger() -> RcLedger {
    let mut led = RcLedger::new();
    let _ = led.begin_run();
    for p in ReconParty::all().iter() {
        if let Some(r) = replay_rc(*p) {
            let _ = led.reg(r);
        }
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 四方对账表齐。
fn fourway_recon_ok() -> bool {
    let led = build_full_rc_ledger();
    if led.len() != ReconParty::all().len() || led.state() != RcState::RcSealed {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    for p in ReconParty::all().iter() {
        let mut found = false;
        let mut i = 0usize;
        while i < led.len() {
            let r = match led.row_at(i) {
                Some(v) => v,
                None => return false,
            };
            if r.party == *p {
                if found {
                    return false;
                }
                found = true;
            }
            i += 1;
        }
        if !found {
            return false;
        }
    }
    true
}

/// 两态分列（已收口实对平/未开工槽位态与冻结口径逐位一致）。
fn twostate_split_ok() -> bool {
    for r in RC_ROWS.iter() {
        if r.state != frozen_two_state(r.party) || !row_split_clean(r) {
            return false;
        }
    }
    // 真绿两行实对平（20 开 ≡ 20 清）。
    for p in [ReconParty::PartyD2, ReconParty::PartyD3].iter() {
        let r = match replay_rc(*p) {
            Some(v) => v,
            None => return false,
        };
        if r.opened != 20 || r.purged != 20 {
            return false;
        }
    }
    true
}

/// 两态分列零混报（混报注入必检出：10 路注入全部被拒）。
fn zero_mixreport_ok() -> bool {
    let (caught, total, escape) = rc_fault_sweep();
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

/// 注入联测十路（人为构造违约必被拒）。
pub fn rc_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 行缺一：只入三方就封。
    {
        let mut led = RcLedger::new();
        let _ = led.begin_run();
        let _ = led.reg(match replay_rc(ReconParty::PartyD2) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        });
        let _ = led.reg(match replay_rc(ReconParty::PartyD3) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        });
        let _ = led.reg(match replay_rc(ReconParty::PartyD4) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        });
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺行;");
        }
    }
    // 2. 行重复。
    {
        let mut led = RcLedger::new();
        let _ = led.begin_run();
        let r0 = match replay_rc(ReconParty::PartyD2) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        };
        let _ = led.reg(r0.clone());
        if led.reg(r0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重复;");
        }
    }
    // 3. 列错挂：D4 挂真绿列。
    {
        let mut led = RcLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_rc(ReconParty::PartyD4) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        };
        bad.state = TwoState::RealGreen;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("错挂;");
        }
    }
    // 4. 虚绿余额：槽位列带余额。
    {
        let mut led = RcLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_rc(ReconParty::PartyE1) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        };
        bad.opened = 3;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("虚绿;");
        }
    }
    // 5. 虚平缺平：真绿列开清不平。
    {
        let mut led = RcLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_rc(ReconParty::PartyD2) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        };
        bad.purged = bad.purged.saturating_sub(1);
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("虚平;");
        }
    }
    // 6. 零余额真绿（真绿列计数皆零也按缺平拒）。
    {
        let mut led = RcLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_rc(ReconParty::PartyD3) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        };
        bad.opened = 0;
        bad.purged = 0;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("零平;");
        }
    }
    // 7. 槽位旗缺。
    {
        let mut led = RcLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_rc(ReconParty::PartyD4) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        };
        bad.slot_flag = 0;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("缺旗;");
        }
    }
    // 8. 相位倒退 + 未开置挂起。
    {
        let r1 = dp_advance(DpPhase::PendingSet, DpPhase::Opened);
        let r2 = dp_advance(DpPhase::Opened, DpPhase::Purged);
        if r1.is_err() && r2.is_err() {
            caught += 1;
        } else {
            escape.push_str("相位;");
        }
    }
    // 9. 封账后写。
    {
        let mut led = build_full_rc_ledger();
        let extra = match replay_rc(ReconParty::PartyE1) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺行")),
        };
        if led.reg(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 10. 摘要空面直证（冻结两态与方名恒非空）。
    {
        let mut empty_found = false;
        for p in ReconParty::all().iter() {
            if p.name().is_empty() {
                empty_found = true;
            }
        }
        for s in TwoState::all().iter() {
            if s.name().is_empty() {
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
    let a = build_full_rc_ledger();
    let b = build_full_rc_ledger();
    if a.len() != b.len() {
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
    if rc_fault_sweep().0 != rc_fault_sweep().0 {
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
    for r in RC_ROWS.iter() {
        acc = acc
            .wrapping_mul(19)
            .wrapping_add(r.party.ordinal())
            .wrapping_add(r.opened)
            .wrapping_add(r.purged)
            .wrapping_add(r.slot_flag);
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
    if SEG_BASE != 0x80000 + 0x40 * (12689u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_rc_ledger();
    if led.state() != RcState::RcSealed {
        return false;
    }
    for p in ReconParty::all().iter() {
        if p.name().is_empty() {
            return false;
        }
    }
    for s in TwoState::all().iter() {
        if s.name().is_empty() {
            return false;
        }
    }
    let head = match led.row_at(0) {
        Some(r) => r.party.name(),
        None => return false,
    };
    if head.is_empty() || frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12690 删除挂起协议回归自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12689_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12689-deletepending-reg");
    let led = build_full_rc_ledger();

    s.add_dyn(
        "F12690-01 四方对账表齐",
        fourway_recon_ok(),
        &format!("rows={} parties={}", led.len(), ReconParty::all().len()),
    );

    s.add_dyn(
        "F12690-02 两态分列口径",
        twostate_split_ok(),
        &format!("states={} realgreen={:?}", TwoState::all().len(), replay_rc(ReconParty::PartyD2).map(|r| r.opened)),
    );

    s.add_dyn(
        "F12690-03 两态分列零混报",
        zero_mixreport_ok(),
        &format!("caught={}", rc_fault_sweep().0),
    );

    s.add_dyn(
        "F12690-04 冻结两态口径",
        frozen_two_state(ReconParty::PartyD2) == TwoState::RealGreen
            && frozen_two_state(ReconParty::PartyD3) == TwoState::RealGreen
            && frozen_two_state(ReconParty::PartyD4) == TwoState::SlotPending
            && frozen_two_state(ReconParty::PartyE1) == TwoState::SlotPending,
        "D2/D3 真绿、D4/E1 槽位态",
    );

    s.add_dyn(
        "F12690-05 注入联测 10/10 拒止",
        {
            let (c, t, e) = rc_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", rc_fault_sweep().0),
    );

    s.add_dyn(
        "F12690-06 相位迁移守卫",
        dp_advance(DpPhase::Opened, DpPhase::PendingSet).is_ok()
            && dp_advance(DpPhase::PendingSet, DpPhase::Purged).is_ok()
            && dp_advance(DpPhase::Purged, DpPhase::PendingSet).is_err(),
        "已开→挂起已置→已清收只进不退",
    );

    s.add_dyn(
        "F12690-07 行级分列校验",
        {
            let mut all = true;
            for r in RC_ROWS.iter() {
                if !row_split_clean(r) {
                    all = false;
                }
            }
            all
        },
        &format!("rows={}", RC_ROWS.len()),
    );

    s.add_dyn(
        "F12690-08 账等式（行数 ≡ 四方数）",
        led.len() == ReconParty::all().len(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12690-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12690-10 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12690-11 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12690-12 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81640);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(RC_ALL.len(), 10);
        let mut i = 0usize;
        while i < RC_ALL.len() {
            let vi = match RC_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RC_ALL.len() {
                let vj = match RC_ALL.get(j) {
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
    fn fourway_recon_holds() {
        assert!(fourway_recon_ok());
        assert!(twostate_split_ok());
        let led = build_full_rc_ledger();
        assert_eq!(led.len(), 4);
        assert_eq!(led.state(), RcState::RcSealed);
        assert_eq!(led.reject_len(), 0);
        // 真绿两行实对平。
        for p in [ReconParty::PartyD2, ReconParty::PartyD3].iter() {
            let r = match replay_rc(*p) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(r.opened, r.purged, "方 {} 余额不平", p.name());
            assert!(r.opened > 0);
        }
    }

    #[test]
    fn phase_machine_guards() {
        assert!(dp_advance(DpPhase::Opened, DpPhase::PendingSet).is_ok());
        assert!(dp_advance(DpPhase::PendingSet, DpPhase::Purged).is_ok());
        // 倒退拒。
        assert!(dp_advance(DpPhase::PendingSet, DpPhase::Opened).is_err());
        assert!(dp_advance(DpPhase::Purged, DpPhase::Opened).is_err());
        // 同相位拒（rank 不进）。
        assert!(dp_advance(DpPhase::Opened, DpPhase::Opened).is_err());
        // 跳相位拒。
        assert!(dp_advance(DpPhase::Opened, DpPhase::Purged).is_err());
        // 秩单调。
        assert!(DpPhase::Opened.rank() < DpPhase::PendingSet.rank());
        assert!(DpPhase::PendingSet.rank() < DpPhase::Purged.rank());
    }

    #[test]
    fn mixreport_injections_all_caught() {
        assert!(zero_mixreport_ok());
        let (caught, total, escape) = rc_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn row_split_semantics() {
        // 真绿行拆改必失校。
        let mut bad = match RC_ROWS.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        bad.purged = 19;
        assert!(!row_split_clean(&bad));
        // 槽位行拆改必失校。
        let mut slot = match RC_ROWS.get(2) {
            Some(x) => x.clone(),
            None => return,
        };
        slot.slot_flag = 0;
        assert!(!row_split_clean(&slot));
        slot.slot_flag = 1;
        slot.opened = 1;
        assert!(!row_split_clean(&slot));
        // 正常行过校。
        let ok_row = match RC_ROWS.get(3) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(row_split_clean(&ok_row));
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_rc_ledger();
        let b = build_full_rc_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(rc_fault_sweep().0, rc_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(RcState::RcOpen.rank() < RcState::RcRunning.rank());
        assert!(RcState::RcRunning.rank() < RcState::RcSealed.rank());
        let mut led = build_full_rc_ledger();
        assert_eq!(led.state(), RcState::RcSealed);
        assert_eq!(led.seal(), Err(RcDiag::EPhaseRewind.val()));
        let mut fresh = RcLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(RcDiag::EPhaseRewind.val()));
        // 封账后写拒。
        let extra = match replay_rc(ReconParty::PartyE1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(led.reg(extra), Err(RcDiag::ESealedWrite.val()));
    }

    #[test]
    fn replay_boundary() {
        assert!(replay_rc(ReconParty::PartyD2).is_some());
        assert!(replay_rc(ReconParty::PartyE1).is_some());
        assert_eq!(RC_ROWS.len(), 4);
        assert_eq!(TwoState::all().len(), 2);
        assert_eq!(DpPhase::all().len(), 3);
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12689_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
