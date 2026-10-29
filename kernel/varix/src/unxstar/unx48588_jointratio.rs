//! UNX-F48588 —— I·联签占比登记（≥30% 机检）
//! 判据原文：UNX-F48588-J1 机检通过；判据锚定 §AI-61 §3。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           占比单点机检：联签条目 / 总条目 ≥ 30‰（permille）。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x00A0; // 联签占比登记诊断段

const E_RATIO_LOW: u16 = SEG_BASE + 1;  // 占比低于门槛
const E_COUNT_BAD: u16 = SEG_BASE + 2;  // 计数失真
const E_ID_DUP: u16 = SEG_BASE + 3;     // 条目 ID 重复
const E_KIND_BAD: u16 = SEG_BASE + 4;   // 条目类别非法
const E_REG_MISS: u16 = SEG_BASE + 5;   // 登记缺失
const E_LEDGER: u16 = SEG_BASE + 6;     // 登记账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;     // 重演不一致

/// 占比门槛（千分之三百）。
pub const RATIO_PERMILLE: u32 = 300;

/// 条目类别。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ItemKind {
    Solo = 0,     // 单签
    Joint = 1,    // 联签
}

/// 登记条目。
pub struct RegItem {
    pub id: u32,
    pub kind: ItemKind,
    pub label: String,
}

/// 联签占比登记簿。
pub struct RatioReg {
    pub items: Vec<RegItem>,
    pub label: String,
}

impl RatioReg {
    pub fn new(label: &str) -> Self {
        RatioReg { items: Vec::new(), label: label.to_string() }
    }

    /// 登记一条：ID 唯一、类别合法。
    pub fn add(&mut self, id: u32, kind: ItemKind, label: &str) -> Result<(), u16> {
        if id == 0 {
            return Err(E_KIND_BAD);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id == id {
                return Err(E_ID_DUP);
            }
            i += 1;
        }
        self.items.push(RegItem { id, kind, label: label.to_string() });
        Ok(())
    }

    /// 联签条目数。
    pub fn joint_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].kind == ItemKind::Joint {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 占比（千分比）。
    pub fn permille(&self) -> u32 {
        let total = self.items.len() as u32;
        if total == 0 {
            return 0;
        }
        self.joint_count() * 1000 / total
    }

    /// 机检：占比 ≥ 门槛且账目守恒。
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
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.items[i].id as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (self.items[i].kind as u64);
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

    // R1 基准登记：10 条中 4 条联签 = 400‰ ≥ 300‰，机检过。
    let mut reg = RatioReg::new("i-batch");
    reg.add(1, ItemKind::Joint, "j1")?;
    reg.add(2, ItemKind::Solo, "s1")?;
    reg.add(3, ItemKind::Solo, "s2")?;
    reg.add(4, ItemKind::Joint, "j2")?;
    reg.add(5, ItemKind::Solo, "s3")?;
    reg.add(6, ItemKind::Solo, "s4")?;
    reg.add(7, ItemKind::Joint, "j3")?;
    reg.add(8, ItemKind::Solo, "s5")?;
    reg.add(9, ItemKind::Solo, "s6")?;
    reg.add(10, ItemKind::Joint, "j4")?;
    reg.machine_check()?;
    if reg.permille() != 400 {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R2 占比临界：3/10 = 300‰ 恰过门槛。
    let mut reg2 = RatioReg::new("i-batch-min");
    let mut id = 1u32;
    let mut i = 0u32;
    while i < 10 {
        let kind = if i < 3 { ItemKind::Joint } else { ItemKind::Solo };
        reg2.add(id, kind, "m")?;
        id += 1;
        i += 1;
    }
    reg2.machine_check()?;
    hits += 1;

    // R3 低于门槛反例：2/10 = 200‰ 须红。
    let mut reg3 = RatioReg::new("i-batch-low");
    let mut i = 0u32;
    while i < 10 {
        let kind = if i < 2 { ItemKind::Joint } else { ItemKind::Solo };
        reg3.add(100 + i, kind, "l")?;
        i += 1;
    }
    let r = reg3.machine_check();
    if r.err() != Some(E_RATIO_LOW) {
        return Err(E_RATIO_LOW);
    }
    hits += 1;

    // R4 ID 重复反例：同 ID 二次登记须拒。
    let mut reg4 = RatioReg::new("i-dup");
    reg4.add(7, ItemKind::Joint, "a")?;
    let r = reg4.add(7, ItemKind::Solo, "b");
    if r.err() != Some(E_ID_DUP) {
        return Err(E_ID_DUP);
    }
    hits += 1;

    // R5 空簿反例：零条目机检须返 E_REG_MISS。
    let reg5 = RatioReg::new("i-empty");
    let r = reg5.machine_check();
    if r.err() != Some(E_REG_MISS) {
        return Err(E_REG_MISS);
    }
    hits += 1;

    // R6 fuzz ×256 随机登记：联合计数与独立复算一致。
    let mut rng = Lcg::new(0x48588_0001);
    let mut reg6 = RatioReg::new("i-fuzz");
    let mut expect_joint = 0u32;
    let mut id = 1u32;
    let mut k = 0u32;
    while k < 256 {
        let kind = if rng.below(2) == 0 { ItemKind::Joint } else { ItemKind::Solo };
        reg6.add(id, kind, "f")?;
        if kind == ItemKind::Joint {
            expect_joint += 1;
        }
        id += 1;
        k += 1;
    }
    if reg6.joint_count() != expect_joint {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R7 占比核算一致性：permille 与手算一致。
    let pm = reg6.joint_count() * 1000 / reg6.items.len() as u32;
    if reg6.permille() != pm {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R8 补签达标演练：对低占比簿补联签至 ≥300‰ 机检转绿。
    let mut reg7 = reg3_clone(&reg3);
    let mut id = 500u32;
    while reg7.permille() < RATIO_PERMILLE {
        reg7.add(id, ItemKind::Joint, "fix")?;
        id += 1;
    }
    reg7.machine_check()?;
    hits += 1;

    // R9 全联签退化：全部条目联签 = 1000‰ 机检过。
    let mut reg8 = RatioReg::new("i-alljoint");
    let mut i = 0u32;
    while i < 5 {
        reg8.add(900 + i, ItemKind::Joint, "aj")?;
        i += 1;
    }
    reg8.machine_check()?;
    if reg8.permille() != 1000 {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R10 重演一致：同种子重建 fuzz 簿，指纹相等。
    let fp1 = reg6.fingerprint();
    let mut rng2 = Lcg::new(0x48588_0001);
    let mut reg9 = RatioReg::new("i-fuzz");
    let mut id = 1u32;
    let mut k = 0u32;
    while k < 256 {
        let kind = if rng2.below(2) == 0 { ItemKind::Joint } else { ItemKind::Solo };
        let _ = reg9.add(id, kind, "f");
        id += 1;
        k += 1;
    }
    if reg9.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}

/// 簿克隆辅助（条目复制）。
fn reg3_clone(src: &RatioReg) -> RatioReg {
    let mut dst = RatioReg::new("i-batch-low-fix");
    let mut i = 0usize;
    while i < src.items.len() {
        let it = &src.items[i];
        let kind = if it.kind == ItemKind::Joint { ItemKind::Joint } else { ItemKind::Solo };
        let _ = dst.add(it.id, kind, "l");
        i += 1;
    }
    dst
}
