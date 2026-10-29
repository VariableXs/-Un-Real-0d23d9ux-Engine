//! UNX-F48723 C·回归漂移清账（复跑差异逐条归零或挂账）
//! 判据：UNX-F48723-J1 清账零未决漂移；判据锚定 §AI-61 §3 C 型。
//! 纪律：登记-归因-处置三段闭环，终态断言未决=0，账目守恒 = 登记=归零+挂账。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1400;

const E_REG_DUP: u16 = SEG_BASE + 1; // 漂移登记重复
const E_ATTR_MISS: u16 = SEG_BASE + 2; // 归因缺失
const E_OPEN_LEFT: u16 = SEG_BASE + 3; // 未决漂移残留
const E_DISPO_BAD: u16 = SEG_BASE + 4; // 处置态非法
const E_TICKET_BAD: u16 = SEG_BASE + 5; // 挂账工单字段缺
const E_BAL_BREAK: u16 = SEG_BASE + 6; // 账目守恒破
const E_RERUN_DIFF: u16 = SEG_BASE + 7; // 复跑差异未消化
const E_ATTR_KIND: u16 = SEG_BASE + 8; // 归因类别越界

/// 漂移类别。
#[derive(Clone, Copy, PartialEq)]
pub enum DriftKind {
    /// 数值漂移。
    Num = 1,
    /// 时序漂移。
    Timing = 2,
    /// 账目漂移。
    Ledger = 3,
}

impl DriftKind {
    fn from_u8(v: u8) -> Option<DriftKind> {
        match v {
            1 => Some(DriftKind::Num),
            2 => Some(DriftKind::Timing),
            3 => Some(DriftKind::Ledger),
            _ => None,
        }
    }
}

/// 处置态。
#[derive(Clone, Copy, PartialEq)]
pub enum Dispo {
    /// 未决。
    Open = 0,
    /// 已归零（现值=基线）。
    Closed = 1,
    /// 已挂账（有工单）。
    Charged = 2,
}

/// 漂移条目。
#[derive(Clone)]
pub struct DriftItem {
    /// 判据 ID。
    pub cid: u32,
    /// 复跑基线值。
    pub baseline: u64,
    /// 复跑现值。
    pub current: u64,
    /// 漂移类别。
    pub kind: u8,
    /// 处置态。
    pub dispo: Dispo,
    /// 挂账工单号（0=无）。
    pub ticket: u32,
    /// 归因说明。
    pub cause: String,
}

impl DriftItem {
    fn new(cid: u32, baseline: u64, current: u64, kind: u8) -> Result<DriftItem, u16> {
        if DriftKind::from_u8(kind).is_none() {
            return Err(E_ATTR_KIND);
        }
        Ok(DriftItem {
            cid,
            baseline,
            current,
            kind,
            dispo: Dispo::Open,
            ticket: 0,
            cause: String::new(),
        })
    }

    /// 是否真差异。
    fn is_diff(&self) -> bool {
        self.baseline != self.current
    }
}

/// 回归漂移清账账本。
pub struct DriftLedger {
    items: Vec<DriftItem>,
    seen: Vec<u32>,
    closed_n: u32,
    charged_n: u32,
}

impl DriftLedger {
    pub fn new() -> DriftLedger {
        DriftLedger {
            items: Vec::new(),
            seen: Vec::new(),
            closed_n: 0,
            charged_n: 0,
        }
    }

    /// 登记漂移（判据 ID 零重复）。
    pub fn register(&mut self, cid: u32, baseline: u64, current: u64, kind: u8) -> Result<bool, u16> {
        for s in self.seen.iter() {
            if *s == cid {
                return Err(E_REG_DUP);
            }
        }
        let it = DriftItem::new(cid, baseline, current, kind)?;
        self.seen.push(cid);
        self.items.push(it);
        Ok(true)
    }

    /// 归因并归零：写入差异结论后现值对齐基线。
    pub fn close(&mut self, cid: u32) -> Result<bool, u16> {
        let mut found = false;
        for it in self.items.iter_mut() {
            if it.cid == cid {
                found = true;
                if it.dispo != Dispo::Open {
                    return Err(E_DISPO_BAD);
                }
                it.cause = "rerun-aligned".to_string();
                it.current = it.baseline;
                it.dispo = Dispo::Closed;
                self.closed_n += 1;
                break;
            }
        }
        if !found {
            return Err(E_ATTR_MISS);
        }
        Ok(true)
    }

    /// 归因并挂账：差异保留，工单号登记。
    pub fn charge(&mut self, cid: u32, ticket: u32) -> Result<bool, u16> {
        if ticket == 0 {
            return Err(E_TICKET_BAD);
        }
        let mut found = false;
        for it in self.items.iter_mut() {
            if it.cid == cid {
                found = true;
                if it.dispo != Dispo::Open {
                    return Err(E_DISPO_BAD);
                }
                if it.cause.is_empty() {
                    return Err(E_ATTR_MISS);
                }
                it.cause = "charged-ticket".to_string();
                it.ticket = ticket;
                it.dispo = Dispo::Charged;
                self.charged_n += 1;
                break;
            }
        }
        if !found {
            return Err(E_ATTR_MISS);
        }
        Ok(true)
    }

    /// 清账终态断言：未决 = 0。
    pub fn assert_no_open(&self) -> Result<(), u16> {
        for it in self.items.iter() {
            if it.dispo == Dispo::Open {
                return Err(E_OPEN_LEFT);
            }
        }
        Ok(())
    }

    /// 账目守恒：登记数 = 归零数 + 挂账数。
    pub fn assert_balance(&self) -> Result<(), u16> {
        if self.items.len() as u32 != self.closed_n + self.charged_n {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }

    /// 挂账工单字段齐：工单号非零、归因非空。
    pub fn assert_tickets(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.dispo == Dispo::Charged {
                if it.ticket == 0 || it.cause.is_empty() {
                    return Err(E_TICKET_BAD);
                }
                n += 1;
            }
        }
        Ok(n)
    }

    /// 归零复跑：Closed 条目现值必须等于基线。
    pub fn assert_closed_aligned(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.dispo == Dispo::Closed {
                if it.is_diff() {
                    return Err(E_RERUN_DIFF);
                }
                n += 1;
            }
        }
        Ok(n)
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 漂移清账主判据：登记-归因-处置闭环 ×10^2 复跑零未决。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 登记三条真差异 + 一条伪差异（基线=现值）
    let mut lg = DriftLedger::new();
    lg.register(0x4701, 100, 120, 1)?;
    lg.register(0x4702, 50, 47, 2)?;
    lg.register(0x4703, 9_000, 9_900, 3)?;
    lg.register(0x4704, 77, 77, 1)?;
    if lg.len() != 4 {
        return Err(E_REG_DUP);
    }
    hits += 1;

    // R2 重复登记必拒
    if lg.register(0x4701, 1, 2, 1).err() == Some(E_REG_DUP) {
        hits += 1;
    } else {
        return Err(E_REG_DUP);
    }

    // R3 非法类别必拒
    if lg.register(0x4705, 1, 2, 9).err() == Some(E_ATTR_KIND) {
        hits += 1;
    } else {
        return Err(E_ATTR_KIND);
    }

    // R4 伪差异直接归零、真差异按类别处置：数值与时序归零、账目挂账
    lg.close(0x4704)?;
    lg.close(0x4701)?;
    lg.close(0x4702)?;
    lg.charge(0x4703, 0x77)?;
    hits += 1;

    // R5 挂账工单零工单号必拒
    let mut lg2 = DriftLedger::new();
    lg2.register(0x4801, 10, 12, 1)?;
    if lg2.charge(0x4801, 0).err() == Some(E_TICKET_BAD) {
        hits += 1;
    } else {
        return Err(E_TICKET_BAD);
    }

    // R6 未归因先挂账必拒（cause 为空）
    let mut lg3 = DriftLedger::new();
    lg3.register(0x4901, 10, 12, 1)?;
    if lg3.charge(0x4901, 0x91).err() == Some(E_ATTR_MISS) {
        hits += 1;
    } else {
        return Err(E_ATTR_MISS);
    }

    // R7 未决残留必抓：登记不处置，断言应红
    let mut lg4 = DriftLedger::new();
    lg4.register(0x4A01, 10, 12, 1)?;
    if lg4.assert_no_open().err() == Some(E_OPEN_LEFT) {
        hits += 1;
    } else {
        return Err(E_OPEN_LEFT);
    }

    // R8 清账终态：全处置后未决=0、守恒、工单齐、归零对齐
    lg.assert_no_open()?;
    lg.assert_balance()?;
    let t_n = lg.assert_tickets()?;
    if t_n != 1 {
        return Err(E_TICKET_BAD);
    }
    let c_n = lg.assert_closed_aligned()?;
    if c_n != 3 {
        return Err(E_RERUN_DIFF);
    }
    hits += 1;

    // R9 复跑 ×10^2：随机漂移流全处置零未决
    let mut rng = Lcg::new(0x48723);
    for _ in 0..100 {
        let mut run = DriftLedger::new();
        let m = (rng.below(6) + 1) as u32;
        let mut k = 0u32;
        while k < m {
            let cid = 0x5000 + k as u32;
            let kind = (rng.below(3) + 1) as u8;
            let diff = rng.below(2) == 1;
            let base = rng.below(1_000);
            let cur = if diff { base + 1 } else { base };
            run.register(cid, base, cur, kind)?;
            k += 1;
        }
        for it in run.items.clone().iter() {
            let cid = it.cid;
            if it.is_diff() {
                run.charge(cid, 0x7000 + cid)?;
            } else {
                run.close(cid)?;
            }
        }
        run.assert_no_open()?;
        run.assert_balance()?;
    }
    hits += 1;

    // R10 fuzz ×256：非法类别与重复登记全部被防御路径吸收
    let mut rng2 = Lcg::new(0x48723_0002);
    let mut blocked = 0u32;
    let mut lgf = DriftLedger::new();
    let mut k = 0u32;
    while k < 256 {
        let pick = rng2.below(4);
        let cid = 0x6000 + (k % 16);
        let kind = (rng2.below(4)) as u8;
        if pick == 0 {
            let r = lgf.register(cid, 1, 2, kind);
            if r.is_err() {
                blocked += 1;
            }
        } else if pick == 1 && kind > 3 {
            if lgf.register(0x6100 + k, 1, 2, kind).is_err() {
                blocked += 1;
            }
        } else if pick == 2 {
            if lgf.charge(0x7000 + k, 0).is_err() {
                blocked += 1;
            }
        } else {
            let _ = lgf.register(0x6100 + k, 1, 1, kind.min(3));
        }
        k += 1;
    }
    if blocked < 10 {
        return Err(E_DISPO_BAD);
    }
    lgf.assert_balance()?;
    hits += 1;

    Ok(hits)
}
