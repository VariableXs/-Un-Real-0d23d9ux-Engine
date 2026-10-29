//! UNX-F48652 —— I33 fuzz（随机配置基线注入零崩）
//! 判据原文：UNX-F48652-J1 fuzz ×10^4 零崩；判据锚定 体验十。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           配置基线状态机 ×10^4 注入零崩零漂。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x08A0; // I33 配置基线 fuzz 诊断段

const E_CRASH: u16 = SEG_BASE + 1;
const E_STATE_BAD: u16 = SEG_BASE + 2;
const E_CFG_BAD: u16 = SEG_BASE + 3;
const E_LOCK_BAD: u16 = SEG_BASE + 4;
const E_LEDGER: u16 = SEG_BASE + 5;
const E_REPLAY: u16 = SEG_BASE + 6;
const E_RESET_BAD: u16 = SEG_BASE + 7;

/// 配置键域。
pub const CFG_KEYS: usize = 8;

/// 配置操作。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CfgOp {
    Set = 0,
    Clear = 1,
    Lock = 2,
    Unlock = 3,
    ResetAll = 4,
}

pub const OP_COUNT: usize = 5;

/// 配置基线状态机。
pub struct CfgMachine {
    pub values: [u32; CFG_KEYS],
    pub locked: [bool; CFG_KEYS],
    pub resets: u32,
    pub fed: u32,
}

impl CfgMachine {
    pub fn new() -> Self {
        CfgMachine {
            values: [0; CFG_KEYS],
            locked: [false; CFG_KEYS],
            resets: 0,
            fed: 0,
        }
    }

    /// 注入一次操作：非法组合返回错误码，绝不崩。
    pub fn inject(&mut self, op: CfgOp, key: usize, val: u32) -> Result<(), u16> {
        if val > 0xffff {
            return Err(E_CFG_BAD);
        }
        match op {
            CfgOp::Set => {
                if key >= CFG_KEYS {
                    return Err(E_CFG_BAD);
                }
                if self.locked[key] {
                    return Err(E_LOCK_BAD);
                }
                self.values[key] = val;
            }
            CfgOp::Clear => {
                if key >= CFG_KEYS {
                    return Err(E_CFG_BAD);
                }
                if self.locked[key] {
                    return Err(E_LOCK_BAD);
                }
                self.values[key] = 0;
            }
            CfgOp::Lock => {
                if key >= CFG_KEYS {
                    return Err(E_CFG_BAD);
                }
                self.locked[key] = true;
            }
            CfgOp::Unlock => {
                if key >= CFG_KEYS {
                    return Err(E_CFG_BAD);
                }
                self.locked[key] = false;
            }
            CfgOp::ResetAll => {
                // 锁定键不受全量复位影响。
                let mut k = 0usize;
                while k < CFG_KEYS {
                    if !self.locked[k] {
                        self.values[k] = 0;
                    }
                    k += 1;
                }
                self.resets += 1;
            }
        }
        self.fed += 1;
        self.selfcheck()
    }

    pub fn selfcheck(&self) -> Result<(), u16> {
        let mut k = 0usize;
        while k < CFG_KEYS {
            if self.values[k] > 0xffff {
                return Err(E_CRASH);
            }
            k += 1;
        }
        if self.resets > 0 && self.resets > self.fed {
            return Err(E_RESET_BAD);
        }
        Ok(())
    }

    pub fn audit(&self, expect_fed: u32) -> Result<(), u16> {
        if self.fed != expect_fed {
            return Err(E_LEDGER);
        }
        self.selfcheck()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut k = 0usize;
        while k < CFG_KEYS {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.values[k] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.locked[k] as u64;
            k += 1;
        }
        h ^= self.resets as u64;
        h
    }
}

/// 随机操作生成。
pub fn gen_op(g: &mut Lcg) -> (CfgOp, usize, u32) {
    let k = g.below(OP_COUNT as u64) as usize;
    let op = match k {
        0 => CfgOp::Set,
        1 => CfgOp::Clear,
        2 => CfgOp::Lock,
        3 => CfgOp::Unlock,
        _ => CfgOp::ResetAll,
    };
    (op, g.below(CFG_KEYS as u64 + 2) as usize, g.below(0x1_0100) as u32)
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

/// 主巡检：I33 配置基线 fuzz 十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 基线操作流：设/锁/清/解锁/复位全绿。
    let mut m = CfgMachine::new();
    m.inject(CfgOp::Set, 1, 5)?;
    m.inject(CfgOp::Lock, 1, 0)?;
    m.inject(CfgOp::Clear, 1, 0)?;
    m.inject(CfgOp::Unlock, 1, 0)?;
    m.inject(CfgOp::ResetAll, 0, 0)?;
    if m.values[1] != 0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R2 fuzz ×10^4 零崩：合法操作域内随机注入（越界键被拒不入账）。
    let mut g = Lcg::new(0x48652_0001);
    let mut m2 = CfgMachine::new();
    let mut fed = 0u32;
    let mut k = 0u32;
    while k < 10_000 {
        let (op, key, val) = gen_op(&mut g);
        if key < CFG_KEYS {
            // 锁定键的 Set/Clear 可能被拒（预期内）。
            let r = m2.inject(op, key, val);
            if r.is_ok() || r.err() == Some(E_LOCK_BAD) {
                fed += 1;
            } else if let Err(e) = r {
                return Err(e);
            }
        } else {
            let r = m2.inject(op, key, val);
            if r.err() != Some(E_CFG_BAD) {
                return Err(E_CFG_BAD);
            }
        }
        m2.selfcheck()?;
        k += 1;
    }
    m2.audit(fed)?;
    hits += 1;

    // R3 键域守卫：越界键 Set 须拒不入账。
    let mut m3 = CfgMachine::new();
    let r = m3.inject(CfgOp::Set, CFG_KEYS, 1);
    if r.err() != Some(E_CFG_BAD) {
        return Err(E_CFG_BAD);
    }
    if m3.fed != 0 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 值域守卫：超域值须拒。
    let r = m3.inject(CfgOp::Set, 0, 0x1_0100);
    if r.err() != Some(E_CFG_BAD) {
        return Err(E_CFG_BAD);
    }
    hits += 1;

    // R5 锁定守卫：锁定后 Set 须拒，解锁后可写。
    let mut m4 = CfgMachine::new();
    m4.inject(CfgOp::Lock, 2, 0)?;
    let r = m4.inject(CfgOp::Set, 2, 9);
    if r.err() != Some(E_LOCK_BAD) {
        return Err(E_LOCK_BAD);
    }
    m4.inject(CfgOp::Unlock, 2, 0)?;
    m4.inject(CfgOp::Set, 2, 9)?;
    if m4.values[2] != 9 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 锁定免疫复位：复位不清锁定键值。
    let mut m5 = CfgMachine::new();
    m5.inject(CfgOp::Set, 3, 42)?;
    m5.inject(CfgOp::Lock, 3, 0)?;
    m5.inject(CfgOp::ResetAll, 0, 0)?;
    if m5.values[3] != 42 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 复位计数入账：resets == 1 且 fed 守恒。
    if m5.resets != 1 || m5.fed != 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R8 重复锁定/解锁幂等：多锁一次仍锁定。
    let mut m6 = CfgMachine::new();
    m6.inject(CfgOp::Lock, 4, 0)?;
    m6.inject(CfgOp::Lock, 4, 0)?;
    let r = m6.inject(CfgOp::Set, 4, 1);
    if r.err() != Some(E_LOCK_BAD) {
        return Err(E_LOCK_BAD);
    }
    hits += 1;

    // R9 fuzz 二段 ×10^4：全解锁域零崩。
    let mut g2 = Lcg::new(0x48652_0002);
    let mut m7 = CfgMachine::new();
    let mut fed2 = 0u32;
    let mut k = 0u32;
    while k < 10_000 {
        let (op, key, val) = gen_op(&mut g2);
        if key < CFG_KEYS {
            let r = m7.inject(op, key, val);
            if r.is_ok() || r.err() == Some(E_LOCK_BAD) {
                fed2 += 1;
            } else if let Err(e) = r {
                return Err(e);
            }
        } else {
            let _ = m7.inject(op, key, val);
        }
        m7.selfcheck()?;
        k += 1;
    }
    m7.audit(fed2)?;
    hits += 1;

    // R10 重演一致：同种子重建两段 fuzz 指纹全等。
    let fp1 = (m2.fingerprint(), m7.fingerprint());
    let mut g3 = Lcg::new(0x48652_0001);
    let mut m8 = CfgMachine::new();
    let mut k = 0u32;
    while k < 10_000 {
        let (op, key, val) = gen_op(&mut g3);
        if key < CFG_KEYS {
            let _ = m8.inject(op, key, val);
        } else {
            let _ = m8.inject(op, key, val);
        }
        k += 1;
    }
    let mut g4 = Lcg::new(0x48652_0002);
    let mut m9 = CfgMachine::new();
    let mut k = 0u32;
    while k < 10_000 {
        let (op, key, val) = gen_op(&mut g4);
        if key < CFG_KEYS {
            let _ = m9.inject(op, key, val);
        } else {
            let _ = m9.inject(op, key, val);
        }
        k += 1;
    }
    if (m8.fingerprint(), m9.fingerprint()) != fp1 || fp1.0 == 0 || fp1.1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
