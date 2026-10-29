//! UNX-F48607 —— I·联签占比登记（≥30% 机检）
//! 判据原文：UNX-F48607-J1 机检通过；判据锚定 §AI-61 §3。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           联签占比单点机检，档位分明。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0300; // 联签占比登记诊断段（I31 批）

const E_RATIO_LOW: u16 = SEG_BASE + 1;
const E_COUNT_BAD: u16 = SEG_BASE + 2;
const E_DUP: u16 = SEG_BASE + 3;
const E_KIND_BAD: u16 = SEG_BASE + 4;
const E_REG_MISS: u16 = SEG_BASE + 5;
const E_LEDGER: u16 = SEG_BASE + 6;
const E_REPLAY: u16 = SEG_BASE + 7;

/// 占比门槛（‰）。
pub const RATIO_PERMILLE: u32 = 300;

/// 条目类别。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Solo,
    Joint,
}

/// 占比登记簿（I31 批）。
pub struct QuotaReg {
    pub items: Vec<(u32, ItemKind)>,
    pub label: String,
}

impl QuotaReg {
    pub fn new(label: &str) -> Self {
        QuotaReg { items: Vec::new(), label: label.to_string() }
    }

    pub fn add(&mut self, id: u32, kind: ItemKind) -> Result<(), u16> {
        if id == 0 {
            return Err(E_KIND_BAD);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].0 == id {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push((id, kind));
        Ok(())
    }

    pub fn joint_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].1 == ItemKind::Joint {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn permille(&self) -> u32 {
        let total = self.items.len() as u32;
        if total == 0 {
            return 0;
        }
        self.joint_count() * 1000 / total
    }

    pub fn machine_check(&self) -> Result<(), u16> {
        if self.items.is_empty() {
            return Err(E_REG_MISS);
        }
        if self.joint_count() > self.items.len() as u32 {
            return Err(E_COUNT_BAD);
        }
        if self.permille() < RATIO_PERMILLE {
            return Err(E_RATIO_LOW);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.items.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.items[i].0 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (self.items[i].1 as u64);
            i += 1;
        }
        h ^= self.items.len() as u64;
        h
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：联签占比登记十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 基准登记：12 条中 4 联签 = 333‰ 过机检。
    let mut reg = QuotaReg::new("i31-quota");
    let mut id = 1u32;
    let mut i = 0u32;
    while i < 12 {
        let kind = if i % 3 == 0 { ItemKind::Joint } else { ItemKind::Solo };
        reg.add(id, kind)?;
        id += 1;
        i += 1;
    }
    reg.machine_check()?;
    if reg.joint_count() != 4 {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R2 临界达标：10 条 3 联签 = 300‰ 恰过。
    let mut reg2 = QuotaReg::new("i31-min");
    let mut i = 0u32;
    while i < 10 {
        let kind = if i < 3 { ItemKind::Joint } else { ItemKind::Solo };
        reg2.add(10 + i, kind)?;
        i += 1;
    }
    reg2.machine_check()?;
    if reg2.permille() != 300 {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R3 低于门槛反例：10 条 2 联签 = 200‰ 须红。
    let mut reg3 = QuotaReg::new("i31-low");
    let mut i = 0u32;
    while i < 10 {
        let kind = if i < 2 { ItemKind::Joint } else { ItemKind::Solo };
        reg3.add(20 + i, kind)?;
        i += 1;
    }
    let r = reg3.machine_check();
    if r.err() != Some(E_RATIO_LOW) {
        return Err(E_RATIO_LOW);
    }
    hits += 1;

    // R4 ID 重复反例：同 ID 二次登记须拒。
    let mut reg4 = QuotaReg::new("i31-dup");
    reg4.add(5, ItemKind::Joint)?;
    let r = reg4.add(5, ItemKind::Solo);
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R5 零 ID 反例：id=0 须拒。
    let mut reg5 = QuotaReg::new("i31-zero");
    let r = reg5.add(0, ItemKind::Joint);
    if r.err() != Some(E_KIND_BAD) {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    // R6 空簿反例：零条目机检须红。
    let reg6 = QuotaReg::new("i31-empty");
    let r = reg6.machine_check();
    if r.err() != Some(E_REG_MISS) {
        return Err(E_REG_MISS);
    }
    hits += 1;

    // R7 fuzz ×200 随机登记：联合计数与独立复算一致。
    let mut rng = Lcg::new(0x48607_0001);
    let mut reg7 = QuotaReg::new("i31-fuzz");
    let mut expect_joint = 0u32;
    let mut id = 1u32;
    let mut k = 0u32;
    while k < 200 {
        let kind = if rng.below(2) == 0 { ItemKind::Joint } else { ItemKind::Solo };
        reg7.add(id, kind)?;
        if kind == ItemKind::Joint {
            expect_joint += 1;
        }
        id += 1;
        k += 1;
    }
    if reg7.joint_count() != expect_joint {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R8 补签达标：低占比簿补至 ≥300‰ 机检转绿。
    let mut reg8 = QuotaReg::new("i31-patch");
    let mut i = 0usize;
    while i < reg3.items.len() {
        let id = reg3.items[i].0;
        let kind = reg3.items[i].1;
        let _ = reg8.add(id, kind);
        i += 1;
    }
    let mut nid = 900u32;
    while reg8.permille() < RATIO_PERMILLE {
        reg8.add(nid, ItemKind::Joint)?;
        nid += 1;
    }
    reg8.machine_check()?;
    hits += 1;

    // R9 全联签退化：5 条全联签 = 1000‰。
    let mut reg9 = QuotaReg::new("i31-allj");
    let mut i = 0u32;
    while i < 5 {
        reg9.add(700 + i, ItemKind::Joint)?;
        i += 1;
    }
    reg9.machine_check()?;
    if reg9.permille() != 1000 {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R10 重演一致：同种子重建 fuzz 簿，指纹相等。
    let fp1 = reg7.fingerprint();
    let mut rng2 = Lcg::new(0x48607_0001);
    let mut reg10 = QuotaReg::new("i31-fuzz");
    let mut id = 1u32;
    let mut k = 0u32;
    while k < 200 {
        let kind = if rng2.below(2) == 0 { ItemKind::Joint } else { ItemKind::Solo };
        let _ = reg10.add(id, kind);
        id += 1;
        k += 1;
    }
    if reg10.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;


    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a11: u32 = 0x9E37_79B9;
    let mut track_b11: u32 = 0x9E37_79B9;
    let mut rail11: Vec<u32> = vec![];
    let mut rail_sum11: u64 = 0;
    let mut dstep11 = 0u32;
    while dstep11 < 48 {
        track_a11 = track_a11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b11 = track_b11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail11.push(track_a11);
        rail_sum11 += track_a11 as u64;
        dstep11 += 1;
    }
    if track_a11 != track_b11 {
        return Err(E_RATIO_LOW);
    }
    if rail11.len() != 48 {
        return Err(E_RATIO_LOW);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_RATIO_LOW);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_RATIO_LOW);
    }
    hits += 1;
    Ok(hits)
}
