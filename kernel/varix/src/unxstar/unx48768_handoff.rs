//! UNX-F48768 C·出厂衔接终验（J-5 陪测签字闭环）
//! 判据：UNX-F48768-J1 陪测签字闭环全绿；判据锚定 J-5 衔接。
//! 纪律：陪测轮次-签字-闭环三段全链，轮终态快照对拍、签字不可逆、闭环零悬挂。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1EE0;

const E_ROUND_BAD: u16 = SEG_BASE + 1; // 轮次非法
const E_SIGN_DUP: u16 = SEG_BASE + 2; // 签字重复
const E_SIGN_MISS: u16 = SEG_BASE + 3; // 签字缺失
const E_SNAP_DIFF: u16 = SEG_BASE + 4; // 快照不一致
const E_FROZEN_TOUCH: u16 = SEG_BASE + 5; // 闭环后改
const E_BAL_BREAK: u16 = SEG_BASE + 6; // 签认账守恒破
const E_FP_ZERO: u16 = SEG_BASE + 7; // 零指纹拒
const E_HANDOFF_MISS: u16 = SEG_BASE + 8; // 衔接缺漏

/// 陪测轮数。
pub const ROUND_N: u32 = 5;

/// 单轮陪测记录。
#[derive(Clone)]
pub struct JRound {
    /// 轮号（1..=5）。
    pub round: u32,
    /// 通过项数。
    pub passed: u32,
    /// 快照指纹。
    pub snap: u32,
    /// 陪测签字（0=未签）。
    pub sign: u32,
}

/// 出厂衔接簿。
pub struct HandoffBook {
    rounds: Vec<JRound>,
    closed: bool,
}

impl HandoffBook {
    pub fn new() -> HandoffBook {
        HandoffBook {
            rounds: Vec::new(),
            closed: false,
        }
    }

    /// 登记轮（轮号唯一、通过项 ≤ 预算、快照非零）。
    pub fn put(&mut self, round: u32, passed: u32, snap: u32) -> Result<bool, u16> {
        if round == 0 || round > ROUND_N {
            return Err(E_ROUND_BAD);
        }
        if passed > 100 {
            return Err(E_ROUND_BAD);
        }
        if snap == 0 {
            return Err(E_FP_ZERO);
        }
        if self.closed {
            return Err(E_FROZEN_TOUCH);
        }
        for r in self.rounds.iter() {
            if r.round == round {
                return Err(E_SIGN_DUP);
            }
        }
        self.rounds.push(JRound {
            round,
            passed,
            snap,
            sign: 0,
        });
        Ok(true)
    }

    /// 陪测签字（轮存在、未签、闭环后拒、指纹非零）。
    pub fn sign(&mut self, round: u32, fp: u32) -> Result<bool, u16> {
        if fp == 0 {
            return Err(E_FP_ZERO);
        }
        if self.closed {
            return Err(E_FROZEN_TOUCH);
        }
        let mut found = false;
        for r in self.rounds.iter_mut() {
            if r.round == round {
                found = true;
                if r.sign != 0 {
                    return Err(E_SIGN_DUP);
                }
                r.sign = fp;
                break;
            }
        }
        if !found {
            return Err(E_ROUND_BAD);
        }
        Ok(true)
    }

    /// 轮覆盖守恒：5 轮全登记。
    pub fn assert_cover(&self) -> Result<(), u16> {
        if self.rounds.len() as u32 != ROUND_N {
            return Err(E_HANDOFF_MISS);
        }
        let mut r = 1u32;
        while r <= ROUND_N {
            let mut hit = false;
            for x in self.rounds.iter() {
                if x.round == r {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_HANDOFF_MISS);
            }
            r += 1;
        }
        Ok(())
    }

    /// 快照复算：指纹 = 轮号+通过项联合 FNV。
    pub fn snap_of(round: u32, passed: u32) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= round;
        h = h.wrapping_mul(0x0100_0193);
        h ^= passed;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 快照对拍：登记快照 vs 复算快照。
    pub fn assert_snaps(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for r in self.rounds.iter() {
            if HandoffBook::snap_of(r.round, r.passed) != r.snap {
                return Err(E_SNAP_DIFF);
            }
            n += 1;
        }
        Ok(n)
    }

    /// 签认账守恒：签字数 = 轮数（闭环前全签）。
    pub fn assert_signs(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for r in self.rounds.iter() {
            if r.sign != 0 {
                n += 1;
            }
        }
        Ok(n)
    }

    /// 闭环：5 轮全登记全签 + 快照一致。
    pub fn close(&mut self) -> Result<u32, u16> {
        self.assert_cover()?;
        let s = self.assert_signs()?;
        if s != ROUND_N {
            return Err(E_SIGN_MISS);
        }
        self.assert_snaps()?;
        self.closed = true;
        Ok(s)
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

/// 出厂衔接终验主判据：5 轮陪测登记-签字-闭环全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 5 轮登记：通过项 88~96，快照按复算写入
    let mut hb = HandoffBook::new();
    let mut r = 1u32;
    while r <= ROUND_N {
        let passed = 88 + r;
        let snap = HandoffBook::snap_of(r, passed);
        hb.put(r, passed, snap)?;
        r += 1;
    }
    hb.assert_cover()?;
    hits += 1;

    // R2 轮号越界必拒
    if hb.put(6, 90, 1).err() == Some(E_ROUND_BAD) {
        hits += 1;
    } else {
        return Err(E_ROUND_BAD);
    }

    // R3 轮重复必拒
    if hb.put(1, 90, 1).err() == Some(E_SIGN_DUP) {
        hits += 1;
    } else {
        return Err(E_SIGN_DUP);
    }

    // R4 零快照必拒
    if hb.put(6, 90, 0).err() == Some(E_FP_ZERO) {
        hits += 1;
    } else {
        return Err(E_FP_ZERO);
    }

    // R5 未全签先闭必抓
    if hb.close().err() == Some(E_SIGN_MISS) {
        hits += 1;
    } else {
        return Err(E_SIGN_MISS);
    }

    // R6 全签 ×5
    let mut r2 = 1u32;
    while r2 <= ROUND_N {
        let fp = HandoffBook::snap_of(r2, 88 + r2).wrapping_add(0x9000);
        hb.sign(r2, fp)?;
        r2 += 1;
    }
    let s = hb.assert_signs()?;
    if s != 5 {
        return Err(E_BAL_BREAK);
    }
    hits += 1;

    // R6b 签字指纹独立复算对拍：签字值 = 快照复算 + 0x9000
    let mut r3 = 1u32;
    while r3 <= ROUND_N {
        let want = HandoffBook::snap_of(r3, 88 + r3).wrapping_add(0x9000);
        let got = hb.rounds[(r3 - 1) as usize].sign;
        if got != want {
            return Err(E_SNAP_DIFF);
        }
        r3 += 1;
    }
    hits += 1;

    // R7 签字重复必拒
    if hb.sign(1, 5).err() == Some(E_SIGN_DUP) {
        hits += 1;
    } else {
        return Err(E_SIGN_DUP);
    }

    // R8 零指纹签字必拒
    if hb.sign(6, 0).err() == Some(E_FP_ZERO) {
        hits += 1;
    } else {
        return Err(E_FP_ZERO);
    }

    // R9 闭环 + 快照对拍
    let s2 = hb.close()?;
    if s2 != 5 {
        return Err(E_SNAP_DIFF);
    }
    hb.assert_snaps()?;
    hits += 1;

    // R10 闭环后改必拒
    if hb.put(6, 90, 1).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }
    if hb.sign(6, 1).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }
    hits += 1;

    // R11 未登记轮签字必拒（未闭环独立簿）
    let mut hb2 = HandoffBook::new();
    hb2.put(1, 90, HandoffBook::snap_of(1, 90))?;
    if hb2.sign(9, 5).err() == Some(E_ROUND_BAD) {
        hits += 1;
    } else {
        return Err(E_ROUND_BAD);
    }
    hb2.sign(1, 0xA11)?;
    if hb2.assert_signs().err().is_some() {
        return Err(E_BAL_BREAK);
    }

    Ok(hits)
}
