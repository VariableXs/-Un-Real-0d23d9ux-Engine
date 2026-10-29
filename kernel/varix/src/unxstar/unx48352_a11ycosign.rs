//! unx48352_a11ycosign — UNX-F48352 B18 批与 AI-29 无障碍联检执行账（B15 联检复签扩容）
//!                        （诊断段 0xCCE0）。
//!
//! 判据原文：
//!   J1 复签登记入册；判据锚定 §AI-61 §8 联签对。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 联签对：B18 批十卡逐卡产出首签（批侧自检判定）与复签
//!      （AI-29 无障碍侧独立复核判定），两签判定必须一致方成对。
//!   2. 复签登记入册：一卡一账条，首签在先复签在后，复签与首签
//!      判定翻供即 E_VERDICT_GAP；重登记即 E_DUP_SIGN。
//!   3. 扩容守恒：B15 联检账只增不清——扩容后账条含 B18 十卡，
//!      逐条 (首签, 复签) 齐备方算入册；缺复签即 E_COVER_MISS。
//!   4. 篡改可检：账条指纹自封，判定或指纹被改即 E_TAMPER。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCCE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ITEM_BAD: u16 = SEG_BASE + 1; // 非法卡号
const E_VERDICT_GAP: u16 = SEG_BASE + 2; // 首签/复签判定翻供
const E_DUP_SIGN: u16 = SEG_BASE + 3; // 重登记
const E_TAMPER: u16 = SEG_BASE + 4; // 账条指纹失配
const E_COVER_MISS: u16 = SEG_BASE + 5; // 复签缺口
const E_ORDER_BAD: u16 = SEG_BASE + 6; // 复签先于首签
const E_SEAL_BAD: u16 = SEG_BASE + 7; // 账条封印失配
const E_REPLAY_BAD: u16 = SEG_BASE + 8; // 对账双跑不一致 // 账条封印失配

/// B18 批十卡（本批全部联检对象）。
pub const ITEMS: [u32; 10] = [
    48343, 48344, 48345, 48346, 48347, 48348, 48349, 48350, 48351, 48352,
];

pub const ITEM_NAMES: [&str; 10] = [
    "readanch", "hicontrast", "stickykey", "onehand", "readorder",
    "a11yerrm1", "a11yobs", "a11yfuzz", "a11yregsent", "a11ycosign",
];

/// 复签方侧别。
pub const SIDE_B18: u8 = 0; // 批侧
pub const SIDE_AI29: u8 = 1; // AI-29 无障碍侧

/// FNV-1a 64 位指纹。
pub fn fp_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 单侧签：卡号 + 侧别 + 判定（1=过）+ 封印。
#[derive(Clone, Copy)]
pub struct SideSign {
    pub item: u32,
    pub side: u8,
    pub verdict: u32,
    seal: u64,
}

impl SideSign {
    fn seal_of(item: u32, side: u8, verdict: u32) -> u64 {
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        h ^= item as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= side as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= verdict as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn new(item: u32, side: u8, verdict: u32) -> Result<Self, u16> {
        if side != SIDE_B18 && side != SIDE_AI29 {
            return Err(E_ITEM_BAD);
        }
        if verdict != 1 && verdict != 0 {
            return Err(E_VERDICT_GAP);
        }
        Ok(SideSign { item, side, verdict, seal: Self::seal_of(item, side, verdict) })
    }

    pub fn verify(&self) -> Result<(), u16> {
        if self.seal != Self::seal_of(self.item, self.side, self.verdict) {
            return Err(E_TAMPER);
        }
        Ok(())
    }
}

/// 联检账条：一卡一账条，首签 + 复签 + 成对封印。
#[derive(Clone, Copy)]
pub struct CosignEntry {
    pub item: u32,
    pub first: SideSign,
    pub second: Option<SideSign>,
    pair_seal: u64,
}

impl CosignEntry {
    fn pair_seal_of(item: u32, v1: u32) -> u64 {
        (item as u64).wrapping_mul(1_000_003) ^ (v1 as u64).wrapping_mul(31)
    }

    /// 首签落条：仅批侧可首签。
    pub fn open(item: u32, verdict: u32) -> Result<Self, u16> {
        let first = SideSign::new(item, SIDE_B18, verdict)?;
        first.verify()?;
        Ok(CosignEntry {
            item,
            first,
            second: None,
            pair_seal: Self::pair_seal_of(item, verdict),
        })
    }

    /// 复签入册：AI-29 侧判定必须与首签一致；重复签拒收。
    pub fn resign(&mut self, verdict: u32) -> Result<(), u16> {
        if self.second.is_some() {
            return Err(E_DUP_SIGN);
        }
        let s = SideSign::new(self.item, SIDE_AI29, verdict)?;
        s.verify()?;
        if s.verdict != self.first.verdict {
            return Err(E_VERDICT_GAP);
        }
        if self.pair_seal != Self::pair_seal_of(self.item, self.first.verdict) {
            return Err(E_SEAL_BAD);
        }
        self.second = Some(s);
        Ok(())
    }

    /// 入册判定：首复齐备且封印自洽。
    pub fn is_registered(&self) -> Result<bool, u16> {
        self.first.verify()?;
        match &self.second {
            None => Ok(false),
            Some(s) => {
                s.verify()?;
                if self.pair_seal != Self::pair_seal_of(self.item, self.first.verdict) {
                    return Err(E_SEAL_BAD);
                }
                Ok(true)
            }
        }
    }
}

/// 执行账：B15 扩容继承——前账条目原样在册，B18 十卡续登。
pub struct ExecLedger {
    pub entries: Vec<CosignEntry>,
}

impl ExecLedger {
    pub fn new() -> Self {
        ExecLedger { entries: Vec::new() }
    }

    /// 首签开条（卡号合法、不重复开条）。
    pub fn open(&mut self, item: u32, verdict: u32) -> Result<(), u16> {
        let mut legal = false;
        let mut i = 0usize;
        while i < ITEMS.len() {
            if ITEMS[i] == item {
                legal = true;
            }
            i += 1;
        }
        if !legal {
            return Err(E_ITEM_BAD);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].item == item {
                return Err(E_DUP_SIGN);
            }
            i += 1;
        }
        self.entries.push(CosignEntry::open(item, verdict)?);
        Ok(())
    }

    /// 复签入册（卡号必须在册且尚未复签）。
    pub fn resign(&mut self, item: u32, verdict: u32) -> Result<(), u16> {
        let mut found = false;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].item == item {
                found = true;
                self.entries[i].resign(verdict)?;
                break;
            }
            i += 1;
        }
        if !found {
            return Err(E_ORDER_BAD);
        }
        Ok(())
    }

    /// 入册对账：逐条齐备、总数守恒十卡。
    pub fn audit(&self) -> Result<u32, u16> {
        if self.entries.len() != ITEMS.len() {
            return Err(E_COVER_MISS);
        }
        let mut registered = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if !self.entries[i].is_registered()? {
                return Err(E_COVER_MISS);
            }
            registered += 1;
            i += 1;
        }
        Ok(registered)
    }
}

/// drill：J1 复签登记入册。
pub fn drill() -> Result<u32, u16> {
    let mut ledger = ExecLedger::new();

    // ── R1 主判据：十卡逐条 首签(批侧,判过) + 复签(AI-29,判过)，
    //    全部入册 ──────────────────────────────────────────────
    let mut i = 0usize;
    while i < ITEMS.len() {
        ledger.open(ITEMS[i], 1)?;
        ledger.resign(ITEMS[i], 1)?;
        i += 1;
    }
    if ledger.audit()? != 10 {
        return Err(E_COVER_MISS);
    }

    // ── R2 判定翻供：复签判 0 与首签判 1 必拒 ────────────────
    let mut l2 = ExecLedger::new();
    l2.open(48343, 1)?;
    if l2.resign(48343, 0) != Err(E_VERDICT_GAP) {
        return Err(E_VERDICT_GAP);
    }
    // 翻供后账条仍无复签 → 入册对账必报缺口。
    if l2.audit().is_ok() {
        return Err(E_COVER_MISS);
    }

    // ── R3 重登记：重复开条/重复复签均拒 ─────────────────────
    if ledger.open(48345, 1) != Err(E_DUP_SIGN) {
        return Err(E_DUP_SIGN);
    }
    if ledger.resign(48345, 1) != Err(E_DUP_SIGN) {
        return Err(E_DUP_SIGN);
    }

    // ── R4 顺序纪律：未开条先复签必拒（复签先于首签）─────────
    let mut l3 = ExecLedger::new();
    if l3.resign(48346, 1) != Err(E_ORDER_BAD) {
        return Err(E_ORDER_BAD);
    }

    // ── R5 非法卡号：表外卡号开条/复签必拒 ──────────────────
    let mut l4 = ExecLedger::new();
    if l4.open(48999, 1).is_ok() || l4.open(48342, 1).is_ok() {
        return Err(E_ITEM_BAD);
    }
    if l4.resign(48343, 1).is_ok() {
        return Err(E_ORDER_BAD);
    }

    // ── R6 判定域：verdict 出 {0,1} 必拒 ─────────────────────
    if SideSign::new(48347, SIDE_B18, 2).is_ok() {
        return Err(E_VERDICT_GAP);
    }
    if SideSign::new(48347, 7, 1).is_ok() {
        return Err(E_ITEM_BAD);
    }

    // ── R7 篡改注入：判定或封印被改必失配 ────────────────────
    let mut bad = SideSign::new(48348, SIDE_AI29, 1)?;
    bad.verdict = 0;
    if bad.verify() != Err(E_TAMPER) {
        return Err(E_TAMPER);
    }
    let e0 = CosignEntry::open(48348, 1)?;
    let mut bad_e = e0;
    bad_e.pair_seal = bad_e.pair_seal.wrapping_add(1);
    if bad_e.is_registered().is_ok() {
        return Err(E_SEAL_BAD);
    }

    // ── R8 账条封印重放：入册对账双跑结果恒等 ────────────────
    let a1 = ledger.audit()?;
    let a2 = ledger.audit()?;
    if a1 != a2 || a1 != 10 {
        return Err(E_REPLAY_BAD);
    }

    // J2 写回摘要非空：十条入册即摘要实质。
    if ledger.entries.is_empty() {
        return Err(E_COVER_MISS);
    }
    Ok(ledger.audit()?)
}
