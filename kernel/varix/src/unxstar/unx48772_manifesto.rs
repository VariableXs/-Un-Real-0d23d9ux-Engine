//! UNX-F48772 C·域功宣告书（使命一句话兑现声明）
//! 判据：UNX-F48772-J1 宣告兑现全绿；判据锚定 域功条件。
//! 纪律：使命条款登记-兑现核验-宣告冻结三段闭环，条款全覆盖、兑现率 100%、冻结不可逆。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1FA0;

const E_CLAUSE_BAD: u16 = SEG_BASE + 1; // 条款非法
const E_CLAIM_DUP: u16 = SEG_BASE + 2; // 条款重复
const E_PROOF_MISS: u16 = SEG_BASE + 3; // 兑现证据缺
const E_DECLARE_DIFF: u16 = SEG_BASE + 4; // 宣告漂移
const E_FROZEN_TOUCH: u16 = SEG_BASE + 5; // 冻结后改
const E_BAL_BREAK: u16 = SEG_BASE + 6; // 兑现账守恒破
const E_CLAUSE_MISS: u16 = SEG_BASE + 7; // 条款缺漏
const E_TEXT_EMPTY: u16 = SEG_BASE + 8; // 空文本拒

/// 条款数。
pub const CLAUSE_N: u32 = 12;

/// 使命条款。
#[derive(Clone)]
pub struct Clause {
    /// 条款号（1..=12）。
    pub no: u32,
    /// 条款文本。
    pub text: String,
    /// 兑现证据锚（0=未兑现）。
    pub proof: u32,
    /// 冻结态。
    pub frozen: bool,
}

/// 域功宣告书。
pub struct Manifesto {
    clauses: Vec<Clause>,
    declared: bool,
}

impl Manifesto {
    pub fn new() -> Manifesto {
        Manifesto {
            clauses: Vec::new(),
            declared: false,
        }
    }

    /// 登记条款（号唯一、文本非空）。
    pub fn put(&mut self, no: u32, text: &str) -> Result<bool, u16> {
        if no == 0 || no > CLAUSE_N {
            return Err(E_CLAUSE_BAD);
        }
        if text.is_empty() {
            return Err(E_TEXT_EMPTY);
        }
        if self.declared {
            return Err(E_FROZEN_TOUCH);
        }
        for c in self.clauses.iter() {
            if c.no == no {
                return Err(E_CLAIM_DUP);
            }
        }
        self.clauses.push(Clause {
            no,
            text: text.to_string(),
            proof: 0,
            frozen: false,
        });
        Ok(true)
    }

    /// 兑现核验（证据锚非零、条款必须存在）。
    pub fn fulfill(&mut self, no: u32, proof: u32) -> Result<bool, u16> {
        if proof == 0 {
            return Err(E_PROOF_MISS);
        }
        if self.declared {
            return Err(E_FROZEN_TOUCH);
        }
        let mut found = false;
        for c in self.clauses.iter_mut() {
            if c.no == no {
                found = true;
                if c.proof != 0 {
                    return Err(E_CLAIM_DUP);
                }
                c.proof = proof;
                break;
            }
        }
        if !found {
            return Err(E_CLAUSE_MISS);
        }
        Ok(true)
    }

    /// 兑现率（万分比）。
    pub fn fulfill_bps(&self) -> u64 {
        if self.clauses.is_empty() {
            return 0;
        }
        let mut n = 0u32;
        for c in self.clauses.iter() {
            if c.proof != 0 {
                n += 1;
            }
        }
        (n as u64).wrapping_mul(10_000) / self.clauses.len() as u64
    }

    /// 条款覆盖守恒。
    pub fn assert_cover(&self) -> Result<(), u16> {
        if self.clauses.len() as u32 != CLAUSE_N {
            return Err(E_CLAUSE_MISS);
        }
        let mut c = 1u32;
        while c <= CLAUSE_N {
            let mut hit = false;
            for x in self.clauses.iter() {
                if x.no == c {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_CLAUSE_MISS);
            }
            c += 1;
        }
        Ok(())
    }

    /// 兑现账守恒：全部条款必须已兑现。
    pub fn assert_all_fulfilled(&self) -> Result<u32, u16> {
        for c in self.clauses.iter() {
            if c.proof == 0 {
                return Err(E_PROOF_MISS);
            }
        }
        Ok(self.clauses.len() as u32)
    }

    /// 宣告冻结：覆盖率 100% + 全兑现。
    pub fn declare(&mut self) -> Result<u64, u16> {
        let n = self.assert_all_fulfilled()?;
        self.assert_cover()?;
        if n != CLAUSE_N {
            return Err(E_BAL_BREAK);
        }
        if self.fulfill_bps() != 10_000 {
            return Err(E_DECLARE_DIFF);
        }
        for c in self.clauses.iter_mut() {
            c.frozen = true;
        }
        self.declared = true;
        Ok(self.fulfill_bps())
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

/// 域功宣告书主判据：条款登记-兑现-宣告冻结全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 12 条款登记（LCG 文本编号）
    let mut mf = Manifesto::new();
    let mut c = 1u32;
    while c <= CLAUSE_N {
        let text = String::from("mission-clause-") + &c.to_string();
        mf.put(c, text.as_str())?;
        c += 1;
    }
    mf.assert_cover()?;
    hits += 1;

    // R2 条款越界必拒
    if mf.put(13, "over").err() == Some(E_CLAUSE_BAD) {
        hits += 1;
    } else {
        return Err(E_CLAUSE_BAD);
    }

    // R3 条款重复必拒
    if mf.put(1, "dup").err() == Some(E_CLAIM_DUP) {
        hits += 1;
    } else {
        return Err(E_CLAIM_DUP);
    }

    // R4 空文本必拒
    if mf.put(13, "").err() == Some(E_TEXT_EMPTY) {
        hits += 1;
    } else {
        return Err(E_TEXT_EMPTY);
    }

    // R5 未兑现先宣告必抓
    if mf.declare().err() == Some(E_PROOF_MISS) {
        hits += 1;
    } else {
        return Err(E_PROOF_MISS);
    }

    // R6 全兑现 ×12
    let mut c2 = 1u32;
    while c2 <= CLAUSE_N {
        mf.fulfill(c2, 0xF000 + c2)?;
        c2 += 1;
    }
    if mf.fulfill_bps() != 10_000 {
        return Err(E_DECLARE_DIFF);
    }
    hits += 1;

    // R7 重复兑现必拒
    if mf.fulfill(1, 9).err() == Some(E_CLAIM_DUP) {
        hits += 1;
    } else {
        return Err(E_CLAIM_DUP);
    }

    // R8 零证据必拒
    if mf.fulfill(13, 0).err() == Some(E_PROOF_MISS) {
        hits += 1;
    } else {
        return Err(E_PROOF_MISS);
    }

    // R9 宣告冻结
    let bps = mf.declare()?;
    if bps != 10_000 {
        return Err(E_DECLARE_DIFF);
    }
    hits += 1;

    // R10 冻结后改必拒
    if mf.put(13, "late").err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }
    if mf.fulfill(1, 9).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }
    hits += 1;

    // R11 条款缺漏必抓：11/12 条款
    let mut mf2 = Manifesto::new();
    let mut c3 = 1u32;
    while c3 <= CLAUSE_N - 1 {
        let t = String::from("clause-") + &c3.to_string();
        mf2.put(c3, t.as_str())?;
        mf2.fulfill(c3, 0xF100 + c3)?;
        c3 += 1;
    }
    if mf2.declare().err() == Some(E_CLAUSE_MISS) {
        hits += 1;
    } else {
        return Err(E_CLAUSE_MISS);
    }

    // R12 兑现率部分态：6/12 = 5000 万分比
    let mut mf3 = Manifesto::new();
    let mut c4 = 1u32;
    while c4 <= CLAUSE_N {
        let t = String::from("half-clause-") + &c4.to_string();
        mf3.put(c4, t.as_str())?;
        if c4 <= 6 {
            mf3.fulfill(c4, 0xF200 + c4)?;
        }
        c4 += 1;
    }
    let half = mf3.fulfill_bps();
    if half != 5_000 {
        return Err(E_DECLARE_DIFF);
    }
    hits += 1;

    Ok(hits)
}
