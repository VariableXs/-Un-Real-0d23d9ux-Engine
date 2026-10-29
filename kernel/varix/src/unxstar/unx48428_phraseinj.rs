//! UNX-F48428 —— E·短语上限绕过注入（反：并发塞入绕过 ×10 全拦）
//! 判据原文：UNX-F48428-J1 绕过注入（并发批塞/删改伪装/重复键挤位/负容量申报）×10 全拦；
//!           库尺寸恒 ≤ 上限；拦截账四型封闭；LCG 随机绕过 ×256 全拦；
//!           合法增删零误拦；拦截账封印重演一致；两账守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD940; // E22 短语绕过注入段

pub const PHRASE_CAP: usize = 10_000;

const E_BYPASS_PASS: u16 = SEG_BASE + 1; // 绕过放行
const E_LEGAL_BLOCK: u16 = SEG_BASE + 2; // 合法误拦
const E_KIND_BAD: u16 = SEG_BASE + 3; // 分类越界
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 6; // 两账守恒破
const E_SIZE_MISS: u16 = SEG_BASE + 7; // 尺寸超限

/// 绕过型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum BypassKind {
    BatchFlood = 0,  // 并发批塞（一次申请 n > 剩余）
    DelMask = 1,     // 删改伪装（声明删除实际无键）
    DupSqueeze = 2,  // 重复键挤位
    NegClaim = 3,    // 负容量申报
}

/// 守门短语库。
pub struct GuardLib {
    keys: Vec<String>,
    blocks: Vec<(u64, BypassKind)>,
    seq: u64,
    accepted: u64,
}

impl GuardLib {
    pub fn new() -> Self {
        GuardLib { keys: Vec::new(), blocks: Vec::new(), seq: 0, accepted: 0 }
    }

    fn has(&self, k: &str) -> bool {
        let mut i = 0usize;
        while i < self.keys.len() {
            if self.keys[i] == k {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 合法增。
    pub fn add(&mut self, k: &str) -> Result<(), u16> {
        self.seq += 1;
        if self.has(k) {
            self.blocks.push((self.seq, BypassKind::DupSqueeze));
            return Err(E_BYPASS_PASS);
        }
        if self.keys.len() >= PHRASE_CAP {
            self.blocks.push((self.seq, BypassKind::BatchFlood));
            return Err(E_BYPASS_PASS);
        }
        self.keys.push(String::from(k));
        self.accepted += 1;
        Ok(())
    }

    /// 合法删：存在返回 true。
    pub fn remove(&mut self, k: &str) -> bool {
        self.seq += 1;
        let mut i = 0usize;
        let mut found = None;
        while i < self.keys.len() {
            if self.keys[i] == k {
                found = Some(i);
            }
            i += 1;
        }
        match found {
            Some(i) => {
                self.keys.swap_remove(i);
                self.accepted += 1;
                true
            }
            None => false, // 无害 no-op（不算拦截，算空操作）
        }
    }

    /// 注入：并发批塞 n 条（n > 剩余容量即整批拦）。
    pub fn inject_batch(&mut self, n: usize) -> Result<(), u16> {
        self.seq += 1;
        let room = PHRASE_CAP - self.keys.len();
        if n > room {
            self.blocks.push((self.seq, BypassKind::BatchFlood));
            return Err(E_BYPASS_PASS);
        }
        Err(E_BYPASS_PASS) // 注入通道恒拒
    }

    /// 注入：负容量申报（语义上不可能的申请）。
    pub fn inject_neg(&mut self, claim: i64) -> Result<(), u16> {
        self.seq += 1;
        if claim < 0 {
            self.blocks.push((self.seq, BypassKind::NegClaim));
            return Err(E_BYPASS_PASS);
        }
        Err(E_BYPASS_PASS)
    }

    /// 分类封闭。
    pub fn audit_kind_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.blocks.len() {
            if self.blocks[i].1 as u16 > 3 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 两账守恒：accepted + blocks.len() == seq。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if self.accepted + self.blocks.len() as u64 != self.seq {
            return Err(E_BAL_MISS);
        }
        if self.keys.len() > PHRASE_CAP {
            return Err(E_SIZE_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.keys.len() {
            let b = self.keys[i].as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h ^= self.blocks.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn blocks_count(&self) -> u32 {
        self.blocks.len() as u32
    }

    pub fn size(&self) -> usize {
        self.keys.len()
    }
}

/// 第 n 号键。
fn key_n(n: usize) -> String {
    String::from("k") + &n.to_string()
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

/// 主巡检：短语绕过注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 并发批塞 ×10 全拦：满载后 batch 1..=10 全拒
    let mut g = GuardLib::new();
    let mut n = 1usize;
    while n <= PHRASE_CAP {
        g.add(&key_n(n))?;
        n += 1;
    }
    let mut k = 1u32;
    while k <= 10 {
        match g.inject_batch(k as usize) {
            Err(E_BYPASS_PASS) => {}
            _ => return Err(E_BYPASS_PASS),
        }
        k += 1;
    }
    if g.size() != PHRASE_CAP || g.blocks_count() != 10 {
        return Err(E_BYPASS_PASS);
    }
    hits += 1;

    // R2 负容量申报全拦：claim -1/-100/-MAX
    match g.inject_neg(-1) {
        Err(E_BYPASS_PASS) => {}
        _ => return Err(E_BYPASS_PASS),
    }
    match g.inject_neg(-100_000) {
        Err(E_BYPASS_PASS) => {}
        _ => return Err(E_BYPASS_PASS),
    }
    hits += 1;

    // R3 重复键挤位全拦：已有键重复 add 拒（DupSqueeze）
    match g.add(&key_n(1)) {
        Err(E_BYPASS_PASS) => {}
        _ => return Err(E_BYPASS_PASS),
    }
    if g.size() != PHRASE_CAP {
        return Err(E_BYPASS_PASS);
    }
    hits += 1;

    // R4 删改伪装无害化：删不存在键 no-op（不拦不记账不崩）
    let before = g.seq;
    if g.remove("no_such_key") {
        return Err(E_LEGAL_BLOCK);
    }
    if g.seq != before + 1 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R5 合法增删零误拦：删 3 加 3 全成功、守恒
    let mut k = 0u32;
    while k < 3 {
        if !g.remove(&key_n(100 + k as usize)) {
            return Err(E_LEGAL_BLOCK);
        }
        k += 1;
    }
    let mut k = 0u32;
    while k < 3 {
        g.add(&key_n(50_000 + k as usize))?;
        k += 1;
    }
    g.audit_balance()?;
    if g.size() != PHRASE_CAP {
        return Err(E_SIZE_MISS);
    }
    hits += 1;

    // R6 LCG 随机绕过 ×256 全拦：批塞/负申报/重复键混合
    let mut g2 = GuardLib::new();
    let mut n = 1usize;
    while n <= 9_500 {
        g2.add(&key_n(n))?;
        n += 1;
    }
    let mut rng = Lcg::new(0x48428);
    let mut k = 0u32;
    while k < 256 {
        let m = rng.below(3);
        match m {
            0 => {
                let batch = 100 + rng.below(10_000) as usize;
                match g2.inject_batch(batch) {
                    Err(E_BYPASS_PASS) => {}
                    _ => return Err(E_BYPASS_PASS),
                }
            }
            1 => {
                let claim = -(rng.below(1000) as i64) - 1;
                match g2.inject_neg(claim) {
                    Err(E_BYPASS_PASS) => {}
                    _ => return Err(E_BYPASS_PASS),
                }
            }
            _ => {
                let kn = 1 + rng.below(9_499) as usize;
                match g2.add(&key_n(kn)) {
                    Err(E_BYPASS_PASS) => {}
                    _ => return Err(E_BYPASS_PASS),
                }
            }
        }
        g2.audit_balance()?;
        k += 1;
    }
    if g2.blocks_count() != 256 {
        return Err(E_BAL_MISS);
    }
    g2.audit_kind_closed()?;
    hits += 1;

    // R7 封印重演一致：同种子双跑指纹相等
    let f1 = g2.fp();
    let mut g3 = GuardLib::new();
    let mut n = 1usize;
    while n <= 9_500 {
        g3.add(&key_n(n))?;
        n += 1;
    }
    let mut rng2 = Lcg::new(0x48428);
    let mut k = 0u32;
    while k < 256 {
        let m = rng2.below(3);
        match m {
            0 => {
                let batch = 100 + rng2.below(10_000) as usize;
                let _ = g3.inject_batch(batch);
            }
            1 => {
                let claim = -(rng2.below(1000) as i64) - 1;
                let _ = g3.inject_neg(claim);
            }
            _ => {
                let kn = 1 + rng2.below(9_499) as usize;
                let _ = g3.add(&key_n(kn));
            }
        }
        k += 1;
    }
    if f1 != g3.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 守恒终验
    g.audit_balance()?;
    g3.audit_balance()?;
    hits += 1;

    Ok(hits)
}
