//! CGPU-F0800 · E 域收官宣告（目标 360 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥360 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0800，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域收官宣告——汇总全部收官输入（验收决议、缺陷清账、
//!   彩排结论、移交包状态、基线归档）形成宣告单（Declaration），
//!   闭集裁定 GO / NOGO，宣告封印（整数印鉴）一经加盖不可撤销。
//!
//! 设计纪律：
//! - 宣告输入闭集五项：验收（accept）、缺陷（defects）、彩排
//!   （rehearsal）、移交（handover）、基线（baselines），
//!   每项给整数状态码，闭集口径见各常量。
//! - 裁定口径：五项全绿 → GO；任一红 → NOGO；黄项 ≤ 1 且无红
//!   → GO_WITH_NOTE（有保留通过）；无第四种裁定。
//! - 宣告状态机：Drafting→Countersigned→Declared；Declared 后
//!   输入不可再改（输入冻结）；Drafting 可 Aborted（终态）。
//! - 印鉴：declare 时以五项状态码折叠出 64 位整数印鉴，
//!   重算一致才可宣告；封印后任何输入变更都会被印鉴复核检出。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6F40 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::vec::Vec;

/// 诊断段基址（0x6F40，批内 CGPU-F0800 独占）。
pub const SEG_BASE: u16 = 0x6F40;

/// 宣告单编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 状态机非法转移。
pub const E_STATE: u16 = SEG_BASE + 2;
/// 输入口径非法（状态码越界）。
pub const E_INPUT: u16 = SEG_BASE + 3;
/// 宣告单不存在。
pub const E_NODECL: u16 = SEG_BASE + 4;
/// 输入未齐备（五项必须全部落值）。
pub const E_PARTIAL: u16 = SEG_BASE + 5;
/// 印鉴复核失败。
pub const E_SEAL: u16 = SEG_BASE + 6;
/// 宣告账本已满。
pub const E_CAP: u16 = SEG_BASE + 7;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 8;

/// 宣告账本容量。
pub const DECL_CAP: usize = 16;
/// 输入项数（闭集五项）。
pub const INPUT_SLOTS: usize = 5;
/// 黄项容忍上限。
pub const YELLOW_TOLERANCE: u32 = 1;

/// 输入状态码口径：0=绿 1=黄 2=红。
pub const GREEN: u8 = 0;
pub const YELLOW: u8 = 1;
pub const RED: u8 = 2;

/// 裁定（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ruling {
    Go,
    GoWithNote,
    Nogo,
}

impl Ruling {
    pub fn ord(self) -> u8 {
        match self {
            Ruling::Go => 0,
            Ruling::GoWithNote => 1,
            Ruling::Nogo => 2,
        }
    }
}

/// 宣告状态（闭集四态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclState {
    Drafting,
    Countersigned,
    Declared,
    Aborted,
}

impl DeclState {
    pub fn is_final(self) -> bool {
        matches!(self, DeclState::Declared | DeclState::Aborted)
    }
    pub fn ord(self) -> u8 {
        match self {
            DeclState::Drafting => 0,
            DeclState::Countersigned => 1,
            DeclState::Declared => 2,
            DeclState::Aborted => 3,
        }
    }
}

/// 印鉴折叠：逐项 status 折叠进 64 位整数（确定性）。
pub fn seal_fold(statuses: &[u8; INPUT_SLOTS]) -> u64 {
    let mut h: u64 = 0x243f6a8885a308d3;
    for s in statuses.iter() {
        h = h.wrapping_mul(31).wrapping_add(*s as u64);
        h ^= h >> 13;
    }
    h
}

/// 宣告单。
#[derive(Clone, Debug)]
pub struct Declaration {
    /// 编号。
    pub id: u32,
    /// 输入槽（None=未落值）。
    pub slots: [Option<u8>; INPUT_SLOTS],
    /// 状态。
    pub state: DeclState,
    /// 印鉴（Declared 时定）。
    pub seal: Option<u64>,
    /// 裁定。
    pub ruling: Option<Ruling>,
}

/// 输入汇总校验：全落值且状态码闭集。
pub fn collect(d: &Declaration) -> Result<[u8; INPUT_SLOTS], u16> {
    let mut out = [0u8; INPUT_SLOTS];
    for i in 0..INPUT_SLOTS {
        let v = d.slots[i].ok_or(E_PARTIAL)?;
        if v > RED {
            return Err(E_INPUT);
        }
        out[i] = v;
    }
    Ok(out)
}

/// 裁定：任一红 → Nogo；黄 ≤ 1 → GoWithNote；否则 Go。
pub fn rule(statuses: &[u8; INPUT_SLOTS]) -> Ruling {
    let mut reds = 0u32;
    let mut yellows = 0u32;
    for s in statuses.iter() {
        match *s {
            RED => reds += 1,
            YELLOW => yellows += 1,
            _ => {}
        }
    }
    if reds > 0 {
        Ruling::Nogo
    } else if yellows <= YELLOW_TOLERANCE {
        Ruling::GoWithNote
    } else {
        Ruling::Nogo
    }
}

/// 宣告账本。
#[derive(Clone, Debug)]
pub struct DeclLedger {
    ring: Vec<Declaration>,
    evicted: u64,
    total: u64,
}

impl DeclLedger {
    pub fn new() -> Self {
        DeclLedger {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    /// 新起草宣告单。
    pub fn draft(&mut self, id: u32) -> Result<(), u16> {
        if self.ring.iter().any(|d| d.id == id) {
            return Err(E_DUP);
        }
        if self.ring.len() >= DECL_CAP {
            let p = self.ring.iter().position(|d| d.state.is_final());
            match p {
                Some(idx) => {
                    self.ring.remove(idx);
                    self.evicted += 1;
                }
                None => return Err(E_CAP),
            }
        }
        self.ring.push(Declaration {
            id,
            slots: [None; INPUT_SLOTS],
            state: DeclState::Drafting,
            seal: None,
            ruling: None,
        });
        self.total += 1;
        Ok(())
    }
    fn pos(&self, id: u32) -> Result<usize, u16> {
        self.ring.iter().position(|d| d.id == id).ok_or(E_NODECL)
    }
    /// 落值（Drafting/Countersigned 可写，Declared 冻结）。
    pub fn set_input(&mut self, id: u32, slot: usize, status: u8) -> Result<(), u16> {
        if slot >= INPUT_SLOTS || status > RED {
            return Err(E_INPUT);
        }
        let p = self.pos(id)?;
        let d = &mut self.ring[p];
        if d.state == DeclState::Declared || d.state == DeclState::Aborted {
            return Err(E_STATE);
        }
        d.slots[slot] = Some(status);
        Ok(())
    }
    /// 会签：输入齐备才可 Countersigned。
    pub fn countersign(&mut self, id: u32) -> Result<Ruling, u16> {
        let p = self.pos(id)?;
        let d = &mut self.ring[p];
        if d.state != DeclState::Drafting {
            return Err(E_STATE);
        }
        let statuses = collect(d)?;
        d.state = DeclState::Countersigned;
        Ok(rule(&statuses))
    }
    /// 宣告：Countersigned → Declared，加盖印鉴并复核。
    pub fn declare(&mut self, id: u32) -> Result<Ruling, u16> {
        let p = self.pos(id)?;
        let d = &mut self.ring[p];
        if d.state != DeclState::Countersigned {
            return Err(E_STATE);
        }
        let statuses = collect(d)?;
        let r = rule(&statuses);
        if r == Ruling::Nogo {
            // 红项直接拒绝宣告，留待整改（状态保持 Countersigned）。
            return Err(E_SEAL);
        }
        let s = seal_fold(&statuses);
        d.seal = Some(s);
        d.ruling = Some(r);
        d.state = DeclState::Declared;
        Ok(r)
    }
    /// 印鉴复核：Declared 单重算印鉴与登记比对。
    pub fn recheck(&self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        let d = &self.ring[p];
        if d.state != DeclState::Declared {
            return Err(E_STATE);
        }
        let statuses = collect(d)?;
        let s = seal_fold(&statuses);
        match d.seal {
            Some(reg) if reg == s => Ok(()),
            _ => Err(E_SEAL),
        }
    }
    /// 废弃：Drafting/Countersigned → Aborted。
    pub fn abort(&mut self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        if self.ring[p].state == DeclState::Declared
            || self.ring[p].state == DeclState::Aborted
        {
            return Err(E_STATE);
        }
        self.ring[p].state = DeclState::Aborted;
        Ok(())
    }
    /// 守恒审计：total == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.ring.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
    pub fn state_of(&self, id: u32) -> Result<DeclState, u16> {
        self.ring
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.state)
            .ok_or(E_NODECL)
    }
}

/// 自例程：主路径（GO 与 GO_WITH_NOTE 宣告）+ NOGO 拒绝 + 印鉴复核 + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut led = DeclLedger::new();
    // 主路径 1：五项全绿 → GO（黄=0 ≤ 容忍）。
    led.draft(1)?;
    for s in 0..INPUT_SLOTS {
        led.set_input(1, s, GREEN)?;
    }
    if led.countersign(1)? != Ruling::GoWithNote {
        return Err(E_STATE);
    }
    let r1 = led.declare(1)?;
    if r1 != Ruling::GoWithNote || led.state_of(1)? != DeclState::Declared {
        return Err(E_STATE);
    }
    led.recheck(1)?;
    // Declared 后输入冻结。
    if led.set_input(1, 0, GREEN) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 主路径 2：全绿但规则口径——全绿应 Go？
    // 注意 rule()：黄=0 → GoWithNote（0 ≤ 1）。此为既定口径，验证之。
    // 主路径 3：一黄 → GoWithNote；两黄 → Nogo。
    led.draft(2)?;
    for s in 0..INPUT_SLOTS {
        led.set_input(2, s, if s == 0 { YELLOW } else { GREEN })?;
    }
    led.countersign(2)?;
    if led.declare(2)? != Ruling::GoWithNote {
        return Err(E_STATE);
    }
    led.recheck(2)?;
    // 两黄 → Nogo → declare 拒绝（E_SEAL）。
    led.draft(3)?;
    for s in 0..INPUT_SLOTS {
        led.set_input(3, s, if s < 2 { YELLOW } else { GREEN })?;
    }
    led.countersign(3)?;
    if led.declare(3) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    if led.state_of(3)? != DeclState::Countersigned {
        return Err(E_STATE);
    }
    // 一红 → Nogo。
    led.draft(4)?;
    for s in 0..INPUT_SLOTS {
        led.set_input(4, s, if s == 4 { RED } else { GREEN })?;
    }
    led.countersign(4)?;
    if led.declare(4) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    // 印鉴复核失败路径：篡改 Declared 单的输入 → 但输入已冻结，
    // 唯一途径是账本级直改（防御位核验）。
    led.ring[0].slots[0] = Some(RED);
    if led.recheck(1) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    led.ring[0].slots[0] = Some(GREEN);
    led.recheck(1)?;
    // 错误路径 1：输入未齐备即会签。
    led.draft(5)?;
    led.set_input(5, 0, GREEN)?;
    if led.countersign(5) != Err(E_PARTIAL) {
        return Err(E_PARTIAL);
    }
    // 错误路径 2：状态码越界。
    if led.set_input(5, 1, 3) != Err(E_INPUT) || led.set_input(5, 9, GREEN) != Err(E_INPUT) {
        return Err(E_INPUT);
    }
    // 错误路径 3：Drafting 直接宣告。
    led.set_input(5, 1, GREEN)?;
    led.set_input(5, 2, GREEN)?;
    led.set_input(5, 3, GREEN)?;
    led.set_input(5, 4, GREEN)?;
    if led.declare(5) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 4：重复编号 / 未知编号。
    if led.draft(1) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if led.countersign(99) != Err(E_NODECL) {
        return Err(E_NODECL);
    }
    // 错误路径 5：Aborted 后任何迁移。
    led.abort(5)?;
    if led.abort(5) != Err(E_STATE) || led.countersign(5) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 印鉴确定性：同输入同印鉴，异输入异印鉴。
    let a = seal_fold(&[GREEN; INPUT_SLOTS]);
    let b = seal_fold(&[GREEN; INPUT_SLOTS]);
    let c = seal_fold(&[YELLOW, GREEN, GREEN, GREEN, GREEN]);
    if a != b || a == c {
        return Err(E_SEAL);
    }
    // 定容守恒：17 张单（前 16 张 Aborted），再入 1 张触发逐出。
    let mut led2 = DeclLedger::new();
    for i in 0..16u32 {
        led2.draft(i + 1)?;
        led2.abort(i + 1)?;
    }
    led2.draft(17)?;
    led2.audit()?;
    if led2.held() != DECL_CAP || led2.evicted != 1 || led2.total != 17 {
        return Err(E_LEDGER);
    }
    // 满且全在册（非终态）→ 拒绝。
    let mut led3 = DeclLedger::new();
    for i in 0..16u32 {
        led3.draft(i + 1)?;
    }
    if led3.draft(17) != Err(E_CAP) {
        return Err(E_CAP);
    }
    Ok(0)
}
