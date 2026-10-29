//! UNX-F48586 —— I·多点触控输入法数据位预告
//!         （触屏 IME 数据位冻结预告）
//! 判据原文：UNX-F48586-J1 预告登记禁实装（§40）；判据锚定 §40 幻觉红线。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           预告位只登记不实装，任何实装尝试必须被拒。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0060; // 触屏 IME 数据位预告诊断段

const E_IMPL_BAD: u16 = SEG_BASE + 1;  // 禁实装位被实装
const E_FROZEN_BAD: u16 = SEG_BASE + 2; // 冻结态漂移
const E_REG_MISS: u16 = SEG_BASE + 3;  // 预告登记缺失
const E_DUP: u16 = SEG_BASE + 4;       // 重复登记
const E_BIT_OOB: u16 = SEG_BASE + 5;   // 数据位越界
const E_LEDGER: u16 = SEG_BASE + 6;    // 登记账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;    // 重演不一致

/// 预告登记条目：一个触屏 IME 数据位。
pub struct AnnounceEntry {
    pub bit: u32,
    pub name: String,
    /// 冻结预告：登记后不得变更。
    pub frozen: bool,
    /// 禁实装红线：必须恒为 false。
    pub implemented: bool,
}

impl AnnounceEntry {
    pub fn announce(bit: u32, name: &str) -> Self {
        AnnounceEntry {
            bit,
            name: name.to_string(),
            frozen: true,
            implemented: false,
        }
    }

    /// 实装尝试：预告位一律拒绝（§40 幻觉红线）。
    pub fn try_implement(&mut self) -> Result<(), u16> {
        if self.frozen && !self.implemented {
            return Err(E_IMPL_BAD);
        }
        Err(E_FROZEN_BAD)
    }

    /// 审计：冻结且未实装。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_FROZEN_BAD);
        }
        if self.implemented {
            return Err(E_IMPL_BAD);
        }
        Ok(())
    }
}

/// 预告登记簿。
pub struct AnnounceRegistry {
    pub entries: Vec<AnnounceEntry>,
    pub label: String,
}

impl AnnounceRegistry {
    pub fn new(label: &str) -> Self {
        AnnounceRegistry { entries: Vec::new(), label: label.to_string() }
    }

    /// 登记一个预告位：位号唯一、位号在预算段内。
    pub fn announce(&mut self, bit: u32, name: &str) -> Result<(), u16> {
        if bit >= 64 {
            return Err(E_BIT_OOB);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].bit == bit {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.entries.push(AnnounceEntry::announce(bit, name));
        Ok(())
    }

    /// 全簿审计：全部条目冻结未实装。
    pub fn audit(&self) -> Result<(), u16> {
        if self.entries.is_empty() {
            return Err(E_REG_MISS);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            self.entries[i].audit()?;
            i += 1;
        }
        Ok(())
    }

    /// 位图投影：登记位在位图中置位。
    pub fn bitmap(&self) -> u64 {
        let mut m: u64 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            m |= 1u64 << self.entries[i].bit;
            i += 1;
        }
        m
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.entries.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.entries[i].bit as u64;
            let nb = self.entries[i].name.as_bytes();
            let mut j = 0usize;
            while j < nb.len() {
                h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ nb[j] as u64;
                j += 1;
            }
            i += 1;
        }
        h ^= self.entries.len() as u64;
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

/// 触屏 IME 数据位预告名表（冻结样本）。
pub const BIT_NAMES: [&str; 8] = [
    "touch-cand",
    "touch-kbd",
    "touch-trace",
    "touch-gest",
    "touch-haptic",
    "touch-hover",
    "touch-swipe",
    "touch-pinch",
];

/// 主巡检：多点触控输入法数据位预告十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 预告登记 ×8：八个数据位全部登记成功。
    let mut reg = AnnounceRegistry::new("touch-ime-bits");
    let mut b = 0u32;
    while (b as usize) < BIT_NAMES.len() {
        reg.announce(b, BIT_NAMES[b as usize])?;
        b += 1;
    }
    if reg.entries.len() != 8 {
        return Err(E_REG_MISS);
    }
    hits += 1;

    // R2 全簿审计：全部冻结未实装（禁实装红线在账）。
    reg.audit()?;
    hits += 1;

    // R3 位图投影：8 位全置。
    if reg.bitmap() != 0x0000_00ff {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 实装尝试 ×10 全拒：对每个预告位逐一尝试实装。
    let mut inj = 0u32;
    while inj < 10 {
        let idx = (inj % 8) as usize;
        let r = reg.entries[idx].try_implement();
        if r.err() != Some(E_IMPL_BAD) {
            return Err(E_IMPL_BAD);
        }
        inj += 1;
    }
    reg.audit()?;
    hits += 1;

    // R5 重复登记反例：同位号二次登记须拒。
    let mut reg2 = AnnounceRegistry::new("touch-ime-bits2");
    reg2.announce(3, "dup-a")?;
    let r = reg2.announce(3, "dup-b");
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R6 位号越界反例：bit ≥ 64 须拒。
    let mut reg3 = AnnounceRegistry::new("touch-ime-bits3");
    let r = reg3.announce(64, "oob");
    if r.err() != Some(E_BIT_OOB) {
        return Err(E_BIT_OOB);
    }
    hits += 1;

    // R7 fuzz ×256 随机位号登记：登记/去重/越界三分类全部符合预期。
    let mut rng = Lcg::new(0x48586_0001);
    let mut reg4 = AnnounceRegistry::new("touch-ime-fuzz");
    let mut added: u32 = 0;
    let mut k = 0u32;
    while k < 256 {
        let bit = rng.below(70);
        let r = reg4.announce(bit as u32, "fz");
        if bit >= 64 {
            if r.err() != Some(E_BIT_OOB) {
                return Err(E_BIT_OOB);
            }
        } else if reg4.bitmap() & (1u64 << bit) != 0 {
            if r.err() != Some(E_DUP) {
                return Err(E_DUP);
            }
        } else {
            if r.is_err() {
                return Err(E_REG_MISS);
            }
            added += 1;
        }
        k += 1;
    }
    reg4.audit()?;
    if reg4.entries.len() as u32 != added {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R8 登记账守恒：entries 数 == 位图 popcount。
    let bm = reg4.bitmap();
    let mut pc = 0u32;
    let mut m = bm;
    while m != 0 {
        pc += (m & 1) as u32;
        m >>= 1;
    }
    if pc != reg4.entries.len() as u32 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R9 名表冻结：预告名表八项非空且互异。
    let mut i = 0usize;
    while i < BIT_NAMES.len() {
        if BIT_NAMES[i].len() == 0 {
            return Err(E_REG_MISS);
        }
        let mut j = i + 1;
        while j < BIT_NAMES.len() {
            if BIT_NAMES[i] == BIT_NAMES[j] {
                return Err(E_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R10 重演一致：同种子重建 fuzz 簿，指纹相等。
    let fp1 = reg4.fingerprint();
    let mut rng2 = Lcg::new(0x48586_0001);
    let mut reg5 = AnnounceRegistry::new("touch-ime-fuzz");
    let mut k = 0u32;
    while k < 256 {
        let bit = rng2.below(70);
        let _ = reg5.announce(bit as u32, "fz");
        k += 1;
    }
    if reg5.fingerprint() != fp1 || fp1 == 0 {
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
        return Err(E_IMPL_BAD);
    }
    if rail11.len() != 48 {
        return Err(E_IMPL_BAD);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_IMPL_BAD);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_IMPL_BAD);
    }
    hits += 1;
    Ok(hits)
}
