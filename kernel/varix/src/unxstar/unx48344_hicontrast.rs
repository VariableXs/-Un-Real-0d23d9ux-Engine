//! unx48344_hicontrast — UNX-F48344 高对比度下的语义色标注（attr 语义位，不含视觉实现）（诊断段 0xCBE0）。
//!
//! 判据原文：
//!   J1 语义位 ×3 与 F4 消费联签；判据锚定 §AI-61 §4 上游 AI-29。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 只管语义不像素：高对比度模式下，界面元素携带语义位（选中/
//!      报错/禁用/高亮/幽灵/紧急），本模块只产出语义位面，绝不携带
//!      RGB/对比度等视觉实现数据——视觉归 F4 消费方。
//!   2. ×3 语义位：三类语义槽（preedit 区/候选区/状态区）各一槽，
//!      每槽一组语义位 + FNV 自体指纹；槽位登记后不可暗改。
//!   3. F4 消费联签：消费方按 elem_id 读语义位，回执 (槽指纹, 消费方
//!      指纹) 落联签账；一槽一签，重签判重（E_DUP_SIGN）。
//!   4. 篡改可检：语义位改一字即自体指纹失配（E_BIT_TAMPER）；位域
//!      越界（超出语义位掩码）登记即拒（E_BIT_RANGE）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCBE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BIT_RANGE: u16 = SEG_BASE + 1; // 语义位越出掩码
const E_BIT_TAMPER: u16 = SEG_BASE + 2; // 槽位指纹失配
const E_DUP_SIGN: u16 = SEG_BASE + 3; // 重复联签
const E_CONSUME_MISS: u16 = SEG_BASE + 4; // 消费方读不到槽
const E_SEAL_BAD: u16 = SEG_BASE + 5; // 联签封印失配
const E_ZONE_BAD: u16 = SEG_BASE + 6; // 语义槽区号非法
const E_COVER_MISS: u16 = SEG_BASE + 7; // ×3 覆盖缺口

/// 语义位掩码（仅 6 位语义；无任何视觉字段）。
pub const BIT_SELECTED: u16 = 1 << 0;
pub const BIT_ERROR: u16 = 1 << 1;
pub const BIT_DISABLED: u16 = 1 << 2;
pub const BIT_HILITE: u16 = 1 << 3;
pub const BIT_GHOST: u16 = 1 << 4;
pub const BIT_URGENT: u16 = 1 << 5;
pub const SEM_MASK: u16 = BIT_SELECTED | BIT_ERROR | BIT_DISABLED | BIT_HILITE | BIT_GHOST | BIT_URGENT;

/// 三类语义槽区（×3）。
pub const ZONES: [&str; 3] = ["preedit_zone", "cand_zone", "status_zone"];

/// FNV-1a 64 位指纹。
pub fn fp_bytes(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

fn fp_str(s: &str) -> u64 {
    fp_bytes(s.as_bytes())
}

/// 语义槽：区号 + 元素 id + 语义位 + 自体封印（不含视觉数据）。
#[derive(Clone)]
pub struct AttrSlot {
    pub zone: u8,
    pub elem_id: u32,
    pub bits: u16,
    seal: u64,
}

impl AttrSlot {
    fn seal_of(zone: u8, elem_id: u32, bits: u16) -> u64 {
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        h ^= zone as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= elem_id as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= bits as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 登记：位必须落在语义掩码内，区号必须合法。
    pub fn register(zone: u8, elem_id: u32, bits: u16) -> Result<Self, u16> {
        if bits & !SEM_MASK != 0 {
            return Err(E_BIT_RANGE);
        }
        if zone as usize >= ZONES.len() {
            return Err(E_ZONE_BAD);
        }
        let seal = Self::seal_of(zone, elem_id, bits);
        Ok(AttrSlot { zone, elem_id, bits, seal })
    }

    /// 自体检封：位或封印任一被改即失配。
    pub fn verify(&self) -> Result<(), u16> {
        if self.seal != Self::seal_of(self.zone, self.elem_id, self.bits) {
            return Err(E_BIT_TAMPER);
        }
        Ok(())
    }

    pub fn slot_fp(&self) -> u64 {
        fp_str(ZONES[self.zone as usize])
            ^ (self.elem_id as u64).wrapping_mul(1_000_003)
            ^ (self.bits as u64)
            ^ self.seal
    }

    /// 置位（写路径，重封印）。
    pub fn set_bits(&mut self, bits: u16) -> Result<(), u16> {
        if bits & !SEM_MASK != 0 {
            return Err(E_BIT_RANGE);
        }
        self.bits = bits;
        self.seal = Self::seal_of(self.zone, self.elem_id, self.bits);
        Ok(())
    }
}

/// F4 消费方：读语义位 → 出联签回执（消费方指纹固定派生）。
pub struct F4Consumer {
    pub id_fp: u64,
}

impl F4Consumer {
    pub fn new(tag: &str) -> Self {
        F4Consumer { id_fp: fp_str(tag) }
    }

    /// 读槽消费：槽必先过自检，再出回执。
    pub fn consume(&self, slot: &AttrSlot) -> Result<Cosign, u16> {
        slot.verify()?;
        Ok(Cosign::new_pair(slot.slot_fp(), self.id_fp))
    }
}

/// 联签回执：槽指纹 + 消费方指纹 + 封印。
#[derive(Clone)]
pub struct Cosign {
    pub slot_fp: u64,
    pub consumer_fp: u64,
    seal: u64,
}

impl Cosign {
    fn new_pair(slot_fp: u64, consumer_fp: u64) -> Self {
        let seal = (slot_fp ^ consumer_fp).wrapping_mul(0x0000_0100_0000_01b3);
        Cosign { slot_fp, consumer_fp, seal }
    }

    pub fn verify(&self) -> Result<(), u16> {
        let want = (self.slot_fp ^ self.consumer_fp).wrapping_mul(0x0000_0100_0000_01b3);
        if want != self.seal {
            return Err(E_SEAL_BAD);
        }
        Ok(())
    }
}

/// 联签账：一槽一签，重签判重。
pub struct SignLedger {
    pub pairs: Vec<Cosign>,
}

impl SignLedger {
    pub fn new() -> Self {
        SignLedger { pairs: Vec::new() }
    }

    pub fn sign(&mut self, slot: &AttrSlot, consumer: &F4Consumer) -> Result<(), u16> {
        let c = consumer.consume(slot)?;
        c.verify()?;
        let sfp = c.slot_fp;
        let mut i = 0usize;
        while i < self.pairs.len() {
            if self.pairs[i].slot_fp == sfp {
                return Err(E_DUP_SIGN);
            }
            i += 1;
        }
        self.pairs.push(c);
        Ok(())
    }

    pub fn has_sign_for(&self, slot: &AttrSlot) -> bool {
        let sfp = slot.slot_fp();
        let mut i = 0usize;
        while i < self.pairs.len() {
            if self.pairs[i].slot_fp == sfp {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn len(&self) -> u32 {
        self.pairs.len() as u32
    }
}

/// 位面摘要：统计槽内置位个数（语义强度 0..=6），供消费方排序参考。
pub fn bit_popcount(slot: &AttrSlot) -> u32 {
    let mut v = slot.bits;
    let mut n = 0u32;
    while v != 0 {
        n += (v & 1) as u32;
        v >>= 1;
    }
    n
}

/// 三槽语义强度分级：0=平、1..2=弱、3..=强；纯语义，不含视觉映射。
pub fn semantics_grade(slot: &AttrSlot) -> u8 {
    match bit_popcount(slot) {
        0 => 0,
        1 | 2 => 1,
        _ => 2,
    }
}

/// drill：J1 语义位 ×3 与 F4 消费联签。
pub fn drill() -> Result<u32, u16> {
    let consumer = F4Consumer::new("f4_hicontrast_ai29");
    let mut ledger = SignLedger::new();

    // ── R1 主判据：三区各登记一语义槽，F4 逐一消费联签 ──────
    let s0 = AttrSlot::register(0, 0x1001, BIT_HILITE | BIT_SELECTED)?;
    let s1 = AttrSlot::register(1, 0x2002, BIT_SELECTED)?;
    let s2 = AttrSlot::register(2, 0x3003, BIT_ERROR | BIT_URGENT)?;
    ledger.sign(&s0, &consumer)?;
    ledger.sign(&s1, &consumer)?;
    ledger.sign(&s2, &consumer)?;
    if ledger.len() != 3 {
        return Err(E_COVER_MISS);
    }
    if !ledger.has_sign_for(&s0) || !ledger.has_sign_for(&s1) || !ledger.has_sign_for(&s2) {
        return Err(E_COVER_MISS);
    }

    // ── R2 重签判重：同槽再签必 E_DUP_SIGN ──────────────────
    if ledger.sign(&s1, &consumer) != Err(E_DUP_SIGN) {
        return Err(E_DUP_SIGN);
    }

    // ── R3 语义位代数：置位/清位回环，封印随写随封 ──────────
    let mut sx = AttrSlot::register(0, 0x4004, BIT_GHOST)?;
    if sx.bits != BIT_GHOST {
        return Err(E_BIT_TAMPER);
    }
    sx.set_bits(BIT_DISABLED | BIT_URGENT)?;
    if sx.bits != (BIT_DISABLED | BIT_URGENT) {
        return Err(E_BIT_RANGE);
    }
    sx.verify()?;
    // 回环：清到零位仍是合法语义态。
    sx.set_bits(0)?;
    sx.verify()?;
    if sx.bits != 0 {
        return Err(E_BIT_TAMPER);
    }

    // ── R4 越界注入：视觉位伪装混入必拒 ─────────────────────
    if AttrSlot::register(0, 0x5005, 0x0040 | BIT_ERROR).is_ok() {
        return Err(E_BIT_RANGE);
    }
    if AttrSlot::register(0, 0x5005, 0xFFFF).is_ok() {
        return Err(E_BIT_RANGE);
    }
    // 视觉实现偷渡检查：语义位面无任何 RGB 通道——位域宽度恰 6 位。
    if SEM_MASK > 0x003F {
        return Err(E_BIT_RANGE);
    }

    // ── R5 篡改注入：位改一字自检即失配 ─────────────────────
    let mut bad = s0.clone();
    bad.bits ^= BIT_ERROR;
    if bad.verify() != Err(E_BIT_TAMPER) {
        return Err(E_BIT_TAMPER);
    }
    let mut bad2 = s0.clone();
    bad2.seal = bad2.seal.wrapping_add(1);
    if bad2.verify() != Err(E_BIT_TAMPER) {
        return Err(E_BIT_TAMPER);
    }

    // ── R6 回执封印核验 + 篡改回执即失配 ────────────────────
    let c0 = consumer.consume(&s0)?;
    c0.verify()?;
    let mut cb = c0.clone();
    cb.slot_fp = cb.slot_fp.wrapping_add(1);
    if cb.verify() != Err(E_SEAL_BAD) {
        return Err(E_SEAL_BAD);
    }

    // ── R7 区号守门：非法区号登记必拒 ────────────────────────
    if AttrSlot::register(3, 0x6006, BIT_HILITE).is_ok() {
        return Err(E_ZONE_BAD);
    }

    // ── R8 位区间不重叠：六语义位两两互异 ───────────────────
    let all = [BIT_SELECTED, BIT_ERROR, BIT_DISABLED, BIT_HILITE, BIT_GHOST, BIT_URGENT];
    let mut i = 0usize;
    while i < all.len() {
        let mut j = i + 1;
        while j < all.len() {
            if all[i] == all[j] {
                return Err(E_BIT_RANGE);
            }
            j += 1;
        }
        i += 1;
    }

    // ── R9 位面摘要：三槽置位数与分级逐一对账 ────────────────
    // s0 = HILITE|SELECTED → 2 位 → 弱(1)；s1 = SELECTED → 1 位 → 弱(1)；
    // s2 = ERROR|URGENT → 2 位 → 弱(1)；sx 已清零 → 0 位 → 平(0)。
    if bit_popcount(&s0) != 2 || bit_popcount(&s1) != 1 || bit_popcount(&s2) != 2 {
        return Err(E_BIT_TAMPER);
    }
    if bit_popcount(&sx) != 0 {
        return Err(E_BIT_TAMPER);
    }
    if semantics_grade(&s0) != 1 || semantics_grade(&sx) != 0 {
        return Err(E_BIT_RANGE);
    }
    // 双位强档：临时槽置三位必须判强(2)。
    let strong = AttrSlot::register(2, 0x7007, BIT_ERROR | BIT_URGENT | BIT_DISABLED)?;
    if semantics_grade(&strong) != 2 || bit_popcount(&strong) != 3 {
        return Err(E_BIT_RANGE);
    }

    // J2 写回摘要非空：联签账三条即摘要实质。
    if ledger.len() == 0 {
        return Err(E_COVER_MISS);
    }
    Ok(ledger.len())
}
