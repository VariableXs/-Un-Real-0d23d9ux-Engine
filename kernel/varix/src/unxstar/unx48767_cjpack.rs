//! UNX-F48767 C·O5 中文章输入包终版（AI-75 消费终版）
//! 判据：UNX-F48767-J1 中文章包消费终版全绿；判据锚定 AI-75 消费。
//! 纪律：字根-候选-上屏三段闭环，字根表冻结、候选序稳定、消费端零改写。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1EA0;

const E_ROOT_BAD: u16 = SEG_BASE + 1; // 字根非法
const E_CAND_BAD: u16 = SEG_BASE + 2; // 候选非法
const E_SEQ_DIFF: u16 = SEG_BASE + 3; // 候选序漂移
const E_DUP_ROOT: u16 = SEG_BASE + 4; // 字根重复
const E_FROZEN_TOUCH: u16 = SEG_BASE + 5; // 冻结后改
const E_CAND_MISS: u16 = SEG_BASE + 6; // 候选缺漏
const E_BAL_BREAK: u16 = SEG_BASE + 7; // 消费账守恒破
const E_ROOT_MISS: u16 = SEG_BASE + 8; // 字根未登记

/// 中文字根。
#[derive(Clone)]
pub struct CjRoot {
    /// 字根码（1..=0xFFFF）。
    pub code: u32,
    /// 映射字符（Unicode 标量）。
    pub ch: char,
}

/// 输入包。
pub struct CjPack {
    roots: Vec<CjRoot>,
    frozen: bool,
    consumed: u64,
}

impl CjPack {
    pub fn new() -> CjPack {
        CjPack {
            roots: Vec::new(),
            frozen: false,
            consumed: 0,
        }
    }

    /// 登记字根（码域合法、唯一、字符非控制区）。
    pub fn put(&mut self, code: u32, ch: char) -> Result<bool, u16> {
        if code == 0 || code > 0xFFFF {
            return Err(E_ROOT_BAD);
        }
        if (ch as u32) < 0x4E00 {
            return Err(E_ROOT_BAD);
        }
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        for r in self.roots.iter() {
            if r.code == code {
                return Err(E_DUP_ROOT);
            }
        }
        self.roots.push(CjRoot { code, ch });
        Ok(true)
    }

    /// 冻结字根表（不可逆）。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.roots.is_empty() {
            return Err(E_ROOT_MISS);
        }
        self.frozen = true;
        Ok(self.roots.len() as u32)
    }

    /// 组字：字根码序列→候选字符串（未登记必拒、冻结后才可消费）。
    pub fn compose(&mut self, codes: &[u32]) -> Result<String, u16> {
        if !self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        if codes.is_empty() {
            return Err(E_CAND_BAD);
        }
        let mut out = String::new();
        for c in codes.iter() {
            let mut hit = false;
            for r in self.roots.iter() {
                if r.code == *c {
                    out.push(r.ch);
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_ROOT_MISS);
            }
        }
        self.consumed += 1;
        Ok(out)
    }

    /// 候选序稳定：同输入两次组字恒等。
    pub fn assert_stable(&mut self, codes: &[u32]) -> Result<(), u16> {
        let a = self.compose(codes)?;
        let b = self.compose(codes)?;
        if a != b {
            return Err(E_SEQ_DIFF);
        }
        Ok(())
    }

    /// 消费账守恒。
    pub fn assert_balance(&self, want: u64) -> Result<(), u16> {
        if self.consumed != want {
            return Err(E_BAL_BREAK);
        }
        Ok(())
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

/// 中文章输入包终版主判据：字根冻结-组字-候选序稳定-消费账全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 字根表登记 ×64（码 0x100..、字 U+4E00..）
    let mut pk = CjPack::new();
    let mut k = 0u32;
    while k < 64 {
        pk.put(0x100 + k, char::from_u32(0x4E00 + k).ok_or(E_ROOT_BAD)?)?;
        k += 1;
    }
    let n = pk.freeze()?;
    if n != 64 {
        return Err(E_ROOT_MISS);
    }
    hits += 1;

    // R2 码越界必拒
    if pk.put(0, '中').err() == Some(E_ROOT_BAD) {
        hits += 1;
    } else {
        return Err(E_ROOT_BAD);
    }

    // R3 重复字根必拒（冻结后登记路径）
    if pk.put(0x100, '文').err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    // R4 未冻结组字必拒
    let mut pk2 = CjPack::new();
    pk2.put(0x100, '和')?;
    if pk2.compose(&[0x100]).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }
    pk2.freeze()?;

    // R5 组字：单根 ×64 全消费
    let mut k2 = 0u32;
    while k2 < 64 {
        let s = pk2.compose(&[0x100 + k2])?;
        if s.chars().count() != 1 {
            return Err(E_CAND_BAD);
        }
        k2 += 1;
    }
    hits += 1;

    // R6 多根组字：双根/三根/四根
    let s2 = pk2.compose(&[0x100, 0x101])?;
    if s2.chars().count() != 2 {
        return Err(E_CAND_BAD);
    }
    let s3 = pk2.compose(&[0x100, 0x101, 0x102])?;
    if s3.chars().count() != 3 {
        return Err(E_CAND_BAD);
    }
    let s4 = pk2.compose(&[0x100, 0x101, 0x102, 0x103])?;
    if s4.chars().count() != 4 {
        return Err(E_CAND_BAD);
    }
    hits += 1;

    // R7 未登记字根必拒
    if pk2.compose(&[0x9999]).err() == Some(E_ROOT_MISS) {
        hits += 1;
    } else {
        return Err(E_ROOT_MISS);
    }

    // R8 空输入必拒
    if pk2.compose(&[]).err() == Some(E_CAND_BAD) {
        hits += 1;
    } else {
        return Err(E_CAND_BAD);
    }

    // R9 候选序稳定：×128 双跑恒等
    let mut rng = Lcg::new(0x48767);
    let mut k3 = 0u32;
    while k3 < 128 {
        let len = (rng.below(4) + 1) as u32;
        let mut codes: Vec<u32> = Vec::new();
        let mut j = 0u32;
        while j < len {
            codes.push(0x100 + rng.below(64) as u32);
            j += 1;
        }
        pk2.assert_stable(&codes)?;
        k3 += 1;
    }
    hits += 1;

    // R10 消费账守恒：consumed = 单根64 + 多根3 + 稳定128×2
    let want = 64 + 3 + 256;
    pk2.assert_balance(want)?;
    hits += 1;

    // R11 跨包隔离：独立包消费账互不串扰
    let mut pka = CjPack::new();
    let mut pkb = CjPack::new();
    pka.put(0x200, '甲')?;
    pkb.put(0x200, '乙')?;
    pka.freeze()?;
    pkb.freeze()?;
    let sa = pka.compose(&[0x200])?;
    let sb = pkb.compose(&[0x200])?;
    if sa == sb {
        return Err(E_SEQ_DIFF);
    }
    pka.assert_balance(1)?;
    pkb.assert_balance(1)?;
    hits += 1;

    // R12 字根域守恒：登记字符全部落在 CJK 统一表意区
    let mut pk3 = CjPack::new();
    let mut k4 = 0u32;
    while k4 < 32 {
        let ch = char::from_u32(0x4E00 + k4 * 7).ok_or(E_ROOT_BAD)?;
        pk3.put(0x300 + k4, ch)?;
        k4 += 1;
    }
    pk3.freeze()?;
    for r in pk3.roots.iter() {
        let u = r.ch as u32;
        if u < 0x4E00 {
            return Err(E_ROOT_BAD);
        }
    }
    hits += 1;

    // R13 控制区字符必拒
    if pk3.put(0x400, char::from_u32(0x41).ok_or(E_ROOT_BAD)?).err() == Some(E_ROOT_BAD) {
        hits += 1;
    } else {
        return Err(E_ROOT_BAD);
    }

    // R14 LCG 大流量组字 ×256：全可消费且账守恒
    let mut rng2 = Lcg::new(0x48767_0014);
    let mut k5 = 0u32;
    while k5 < 256 {
        let len = (rng2.below(4) + 1) as u32;
        let mut codes: Vec<u32> = Vec::new();
        let mut j = 0u32;
        while j < len {
            codes.push(0x300 + rng2.below(32) as u32);
            j += 1;
        }
        pk3.compose(&codes)?;
        k5 += 1;
    }
    pk3.assert_balance(256)?;
    hits += 1;

    // R15 字根表冻结断言与消费端零改写：冻结后登记路径全拒
    if pk3.put(0x500, '终').err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }
    if pk3.freeze().err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    Ok(hits)
}
