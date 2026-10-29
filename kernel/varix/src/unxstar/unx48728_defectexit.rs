//! UNX-F48728 C·缺陷账全域终出口（40 批缺陷整理终版工单）
//! 判据：UNX-F48728-J1 工单字段齐零悬挂；判据锚定 双轨产线。
//! 纪律：登记-处置-终出口三段闭环，P0 零悬挂、字段齐断言、批覆盖守恒。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x16A0;

const E_DUP_TICK: u16 = SEG_BASE + 1; // 工单重复
const E_SEV_BAD: u16 = SEG_BASE + 2; // 严重度非法
const E_P0_HANG: u16 = SEG_BASE + 3; // P0 悬挂
const E_FIELD_MISS: u16 = SEG_BASE + 4; // 工单字段缺
const E_OPEN_HANG: u16 = SEG_BASE + 5; // Open 悬挂
const E_BATCH_MISS: u16 = SEG_BASE + 6; // 批号缺
const E_DISPO_BAD: u16 = SEG_BASE + 7; // 处置态非法
const E_EXPORT_BREAK: u16 = SEG_BASE + 8; // 出口守恒破

/// 批数。
pub const BATCH_N: u32 = 40;

/// 严重度。
#[derive(Clone, Copy, PartialEq)]
pub enum Sev {
    P0 = 0,
    P1 = 1,
    P2 = 2,
}

/// 处置态。
#[derive(Clone, Copy, PartialEq)]
pub enum Dispo {
    Open = 0,
    Fixed = 1,
    Waived = 2,
}

/// 缺陷工单。
#[derive(Clone)]
pub struct DefectTicket {
    /// 工单号（全域唯一）。
    pub tid: u32,
    /// 批号（1..=40）。
    pub batch: u32,
    /// 严重度。
    pub sev: u8,
    /// 处置态。
    pub dispo: u8,
    /// 处置结论（Fixed/Waived 必填）。
    pub verdict: String,
}

/// 缺陷账终出口簿。
pub struct DefectExit {
    tickets: Vec<DefectTicket>,
    seen: Vec<u32>,
    batch_cover: [bool; 41],
}

impl DefectExit {
    pub fn new() -> DefectExit {
        DefectExit {
            tickets: Vec::new(),
            seen: Vec::new(),
            batch_cover: [false; 41],
        }
    }

    /// 登记缺陷工单。
    pub fn put(&mut self, tid: u32, batch: u32, sev: u8) -> Result<bool, u16> {
        if batch == 0 || batch as usize >= self.batch_cover.len() {
            return Err(E_BATCH_MISS);
        }
        if sev > 2 {
            return Err(E_SEV_BAD);
        }
        for s in self.seen.iter() {
            if *s == tid {
                return Err(E_DUP_TICK);
            }
        }
        self.seen.push(tid);
        self.tickets.push(DefectTicket {
            tid,
            batch,
            sev,
            dispo: Dispo::Open as u8,
            verdict: String::new(),
        });
        self.batch_cover[batch as usize] = true;
        Ok(true)
    }

    /// 处置：Open→Fixed/Waived（结论必填）。
    pub fn dispose(&mut self, tid: u32, dispo: u8, verdict: &str) -> Result<bool, u16> {
        if dispo != Dispo::Fixed as u8 && dispo != Dispo::Waived as u8 {
            return Err(E_DISPO_BAD);
        }
        if verdict.is_empty() {
            return Err(E_FIELD_MISS);
        }
        let mut found = false;
        for t in self.tickets.iter_mut() {
            if t.tid == tid {
                found = true;
                if t.dispo != Dispo::Open as u8 {
                    return Err(E_DISPO_BAD);
                }
                t.dispo = dispo;
                t.verdict = verdict.to_string();
                break;
            }
        }
        if !found {
            return Err(E_BATCH_MISS);
        }
        Ok(true)
    }

    /// P0 零悬挂：所有 P0 必须 Fixed。
    pub fn assert_p0(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for t in self.tickets.iter() {
            if t.sev == Sev::P0 as u8 {
                if t.dispo != Dispo::Fixed as u8 {
                    return Err(E_P0_HANG);
                }
                n += 1;
            }
        }
        Ok(n)
    }

    /// 零悬挂：所有工单必须非 Open。
    pub fn assert_no_open(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for t in self.tickets.iter() {
            if t.dispo == Dispo::Open as u8 {
                return Err(E_OPEN_HANG);
            }
            n += 1;
        }
        Ok(n)
    }

    /// 工单字段齐：处置态工单结论必非空。
    pub fn assert_fields(&self) -> Result<(), u16> {
        for t in self.tickets.iter() {
            if t.dispo != Dispo::Open as u8 && t.verdict.is_empty() {
                return Err(E_FIELD_MISS);
            }
            if t.dispo == Dispo::Open as u8 && !t.verdict.is_empty() {
                return Err(E_FIELD_MISS);
            }
        }
        Ok(())
    }

    /// 批覆盖守恒：40 批每批至少一单。
    pub fn assert_batch_cover(&self) -> Result<(), u16> {
        let mut b = 1u32;
        while b <= BATCH_N {
            if !self.batch_cover[b as usize] {
                return Err(E_BATCH_MISS);
            }
            b += 1;
        }
        Ok(())
    }

    /// 终出口导出：全部非 Open 工单快照（计数守恒）。
    pub fn export(&self) -> Result<Vec<DefectTicket>, u16> {
        self.assert_no_open()?;
        self.assert_fields()?;
        let mut out: Vec<DefectTicket> = Vec::new();
        for t in self.tickets.iter() {
            out.push(t.clone());
        }
        if out.len() != self.tickets.len() {
            return Err(E_EXPORT_BREAK);
        }
        Ok(out)
    }

    fn len(&self) -> usize {
        self.tickets.len()
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

/// 缺陷账全域终出口主判据：40 批工单处置齐零悬挂终出口。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 40 批 × 每批 3 单（P0/P1/P2 各一）
    let mut de = DefectExit::new();
    let mut tid = 0x9000u32;
    let mut b = 1u32;
    while b <= BATCH_N {
        de.put(tid, b, 0)?;
        de.put(tid + 1, b, 1)?;
        de.put(tid + 2, b, 2)?;
        tid += 3;
        b += 1;
    }
    if de.len() != 120 {
        return Err(E_DUP_TICK);
    }
    hits += 1;

    // R2 重复工单必拒
    if de.put(0x9000, 1, 1).err() == Some(E_DUP_TICK) {
        hits += 1;
    } else {
        return Err(E_DUP_TICK);
    }

    // R3 非法严重度必拒
    if de.put(0xA000, 1, 5).err() == Some(E_SEV_BAD) {
        hits += 1;
    } else {
        return Err(E_SEV_BAD);
    }

    // R4 批号越界必拒
    if de.put(0xA001, 41, 1).err() == Some(E_BATCH_MISS) {
        hits += 1;
    } else {
        return Err(E_BATCH_MISS);
    }

    // R5 未处置先断零悬挂必抓
    if de.assert_no_open().err() == Some(E_OPEN_HANG) {
        hits += 1;
    } else {
        return Err(E_OPEN_HANG);
    }

    // R6 P0 悬挂必抓（P0 是 Open 且未 Fixed）
    if de.assert_p0().err() == Some(E_P0_HANG) {
        hits += 1;
    } else {
        return Err(E_P0_HANG);
    }

    // R7 全量处置：P0→Fixed，P1/P2→Fixed 或 Waived（固定分配）
    let mut d = 0x9000u32;
    let mut b2 = 1u32;
    while b2 <= BATCH_N {
        de.dispose(d, 1, "p0-root-fixed")?;
        let p1_wave = b2 % 2 == 0;
        if p1_wave {
            de.dispose(d + 1, 2, "p1-waived-dup")?;
        } else {
            de.dispose(d + 1, 1, "p1-fixed")?;
        }
        let p2_wave = b2 % 3 == 0;
        if p2_wave {
            de.dispose(d + 2, 2, "p2-waived-noise")?;
        } else {
            de.dispose(d + 2, 1, "p2-fixed")?;
        }
        d += 3;
        b2 += 1;
    }
    de.assert_no_open()?;
    de.assert_fields()?;
    de.assert_batch_cover()?;
    hits += 1;

    // R8 P0 全 Fixed 断言
    let p0_n = de.assert_p0()?;
    if p0_n != BATCH_N {
        return Err(E_P0_HANG);
    }
    hits += 1;

    // R9 空结论必拒
    let mut de2 = DefectExit::new();
    de2.put(0xB000, 1, 1)?;
    if de2.dispose(0xB000, 1, "").err() == Some(E_FIELD_MISS) {
        hits += 1;
    } else {
        return Err(E_FIELD_MISS);
    }

    // R10 LCG 抽查 ×256：工单处置态与结论一致性
    let ex = de.export()?;
    if ex.len() != 120 {
        return Err(E_EXPORT_BREAK);
    }
    let mut rng = Lcg::new(0x48728);
    let mut k = 0u32;
    while k < 256 {
        let pick = rng.below(120) as usize;
        let t = &ex[pick];
        if t.dispo == Dispo::Open as u8 {
            return Err(E_OPEN_HANG);
        }
        if t.verdict.is_empty() {
            return Err(E_FIELD_MISS);
        }
        if t.sev == Sev::P0 as u8 && t.dispo != Dispo::Fixed as u8 {
            return Err(E_P0_HANG);
        }
        k += 1;
    }
    hits += 1;

    Ok(hits)
}
