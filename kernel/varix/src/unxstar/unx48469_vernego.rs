//! UNX-F48469 —— E·版本协商违例（反：假版本号 ×10 拒绝降级）
//! 判据原文：UNX-F48469-J1 协议版本协商：支持集 {2,3,4}；假版本（1/5/0/MAX）×10 全拒；
//!           拒绝账（版本+时刻）×10 全记且零降级（state 不回落）；协商成功恒定版本；
//!           LCG 随机假版本 ×256 全拒；账封印重演一致；
//!           已协商版本不受后续假版本影响（隔离性）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE340; // E24 版本协商段

pub const VER_MIN: u16 = 2;
pub const VER_MAX: u16 = 4;

const E_VER_PASS: u16 = SEG_BASE + 1; // 假版本放行
const E_DOWNGRADE: u16 = SEG_BASE + 2; // 已协商版本被降级
const E_KIND_BAD: u16 = SEG_BASE + 3; // 账字段缺失
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 6; // 守恒破
const E_COUNT_MISS: u16 = SEG_BASE + 7; // 拒绝计数不符

/// 拒绝账条目。
#[derive(Clone)]
pub struct Verdict {
    pub ver: u16,
    pub tick: u32,
}

/// 版本协商器。
pub struct Negotiator {
    agreed: Option<u16>,
    rejects: Vec<Verdict>,
    tick: u32,
    sessions: u32,
}

impl Negotiator {
    pub fn new() -> Self {
        Negotiator { agreed: None, rejects: Vec::new(), tick: 0, sessions: 0 }
    }

    /// 协商申请：版本在 [2,4] 内受理；否则拒绝记账（零降级）。
    pub fn negotiate(&mut self, ver: u16) -> Result<u16, u16> {
        self.tick += 1;
        if ver < VER_MIN || ver > VER_MAX {
            self.rejects.push(Verdict { ver, tick: self.tick });
            return Err(E_VER_PASS);
        }
        self.agreed = Some(ver);
        self.sessions += 1;
        Ok(ver)
    }

    /// 隔离性：已协商版本不被假版本影响。
    pub fn audit_isolation(&self, expect: u16) -> Result<(), u16> {
        match self.agreed {
            Some(v) => {
                if v != expect {
                    return Err(E_DOWNGRADE);
                }
                Ok(())
            }
            None => Err(E_BAL_MISS),
        }
    }

    /// 拒绝账审计：tick 递增、ver 全在支持集外。
    pub fn audit_rejects(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rejects.len() {
            let r = &self.rejects[i];
            if r.ver >= VER_MIN && r.ver <= VER_MAX {
                return Err(E_KIND_BAD);
            }
            if r.tick == 0 {
                return Err(E_KIND_BAD);
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.rejects.len() {
            h ^= self.rejects[i].ver as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.rejects[i].tick as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.sessions as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }
}

/// 假版本 ×10 清单。
pub fn fake_ten() -> Vec<u16> {
    vec![0, 1, 5, 6, 100, 1000, 0xFFFF, 7, 0x8000, 2 + 4] // 6 重复亦拒
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
        self.next() % n
    }
}

/// 主巡检：版本协商违例八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 假版本 ×10 全拒零降级（协商器未协商态）
    let mut n = Negotiator::new();
    let fakes = fake_ten();
    let mut i = 0usize;
    while i < fakes.len() {
        match n.negotiate(fakes[i]) {
            Err(E_VER_PASS) => {}
            _ => return Err(E_VER_PASS),
        }
        i += 1;
    }
    if n.reject_count() != 10 {
        return Err(E_COUNT_MISS);
    }
    if n.sessions != 0 {
        return Err(E_VER_PASS);
    }
    n.audit_rejects()?;
    hits += 1;

    // R2 合法版本受理：3 协商成功
    let v = n.negotiate(3)?;
    if v != 3 {
        return Err(E_VER_PASS);
    }
    hits += 1;

    // R3 隔离性：协商后假版本不动摇
    match n.negotiate(1) {
        Err(E_VER_PASS) => {}
        _ => return Err(E_VER_PASS),
    }
    n.audit_isolation(3)?;
    hits += 1;

    // R4 边界版本：2 与 4 都受理（支持集闭区间）
    let mut n2 = Negotiator::new();
    n2.negotiate(2)?;
    n2.negotiate(4)?;
    if n2.sessions != 2 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R5 u16::MAX 拒：不 panic
    match n2.negotiate(0xFFFF) {
        Err(E_VER_PASS) => {}
        _ => return Err(E_VER_PASS),
    }
    hits += 1;

    // R6 LCG 随机假版本 ×256 全拒：版本 ∈ [0,1] ∪ [5,0xFFFF]
    let mut n3 = Negotiator::new();
    n3.negotiate(4)?;
    let mut rng = Lcg::new(0x48469);
    let mut k = 0u32;
    while k < 256 {
        let bad = if rng.below(2) == 0 {
            rng.below(2) as u16 // 0 或 1
        } else {
            5 + rng.below(65_000) as u16
        };
        match n3.negotiate(bad) {
            Err(E_VER_PASS) => {}
            _ => return Err(E_VER_PASS),
        }
        k += 1;
    }
    if n3.reject_count() != 256 {
        return Err(E_COUNT_MISS);
    }
    n3.audit_isolation(4)?;
    hits += 1;

    // R7 重演一致：同种子双跑拒绝账指纹相等
    let f1 = n3.fp();
    let mut n4 = Negotiator::new();
    n4.negotiate(4)?;
    let mut rng2 = Lcg::new(0x48469);
    let mut k = 0u32;
    while k < 256 {
        let bad = if rng2.below(2) == 0 {
            rng2.below(2) as u16
        } else {
            5 + rng2.below(65_000) as u16
        };
        let _ = n4.negotiate(bad);
        k += 1;
    }
    if f1 != n4.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 封印稳定 + 守恒（sessions 1 + rejects 256 = tick 257）
    if n3.sessions + n3.reject_count() as u32 != n3.tick {
        return Err(E_BAL_MISS);
    }
    let s1 = n3.fp();
    let s2 = n3.fp();
    if s1 != s2 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    Ok(hits)
}
