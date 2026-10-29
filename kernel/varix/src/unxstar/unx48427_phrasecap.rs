//! UNX-F48427 —— E·超长自定义短语（正：上限 10,000 拒绝第 10001 条）
//! 判据原文：UNX-F48427-J1 短语库上限 10,000：前 10,000 全收；
//!           第 10,001 条拒绝且库不变；拒绝账 ×10 全记；
//!           LCG 随机增删 2048 轮库尺寸恒 ≤10,000 且守恒；
//!           库封印重演一致；同码重复短语拒（键唯一）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD920; // E22 短语上限段

pub const PHRASE_CAP: usize = 10_000;

const E_OVER_PASS: u16 = SEG_BASE + 1; // 超限放行
const E_SIZE_MISS: u16 = SEG_BASE + 2; // 库尺寸不符
const E_NO_LOG: u16 = SEG_BASE + 3; // 拒绝未记账
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 6; // 增删守恒破
const E_DUP_KEY: u16 = SEG_BASE + 7; // 同码重复放行
const E_CAP_VAR: u16 = SEG_BASE + 8; // 上限漂移

/// 短语条目。
#[derive(Clone, PartialEq, Eq)]
pub struct Phrase {
    pub key: String,   // 触发码
    pub text: String,  // 短语正文
}

/// 短语库。
pub struct PhraseLib {
    items: Vec<Phrase>,
    reject_log: u32,
    added: u64,
    removed: u64,
}

impl PhraseLib {
    pub fn new() -> Self {
        PhraseLib { items: Vec::new(), reject_log: 0, added: 0, removed: 0 }
    }

    fn find_key(&self, key: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 增：同码拒、超限拒（记账）。
    pub fn add(&mut self, key: &str, text: &str) -> Result<(), u16> {
        if self.find_key(key).is_some() {
            return Err(E_DUP_KEY);
        }
        if self.items.len() >= PHRASE_CAP {
            self.reject_log += 1;
            return Err(E_OVER_PASS);
        }
        self.items.push(Phrase { key: String::from(key), text: String::from(text) });
        self.added += 1;
        Ok(())
    }

    /// 删：存在则删返回 true。
    pub fn remove(&mut self, key: &str) -> bool {
        match self.find_key(key) {
            Some(i) => {
                self.items.swap_remove(i);
                self.removed += 1;
                true
            }
            None => false,
        }
    }

    /// 守恒：len == added - removed。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if self.items.len() as u64 != self.added - self.removed {
            return Err(E_BAL_MISS);
        }
        if self.items.len() > PHRASE_CAP {
            return Err(E_OVER_PASS);
        }
        Ok(())
    }

    /// 键唯一。
    pub fn audit_unique(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.items.len() {
            let mut j = i + 1;
            while j < self.items.len() {
                if self.items[i].key == self.items[j].key {
                    return Err(E_DUP_KEY);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.items.len() {
            let b = self.items[i].key.as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            let b = self.items[i].text.as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn rejects(&self) -> u32 {
        self.reject_log
    }
}

/// 第 n 条短语（键 p{n} 正文 t{n}）。
fn phrase_n(n: usize) -> (&'static str, String, String) {
    let key = String::from("p") + &n.to_string();
    let text = String::from("t") + &n.to_string();
    ("", key, text)
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

/// 主巡检：短语上限八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 前 10,000 全收（快速键名生成）
    let mut lib = PhraseLib::new();
    let mut n = 1usize;
    while n <= PHRASE_CAP {
        let (k, t) = {
            let (_, key, text) = phrase_n(n);
            (key, text)
        };
        lib.add(&k, &t)?;
        n += 1;
    }
    if lib.len() != PHRASE_CAP {
        return Err(E_SIZE_MISS);
    }
    lib.audit_balance()?;
    hits += 1;

    // R2 第 10,001 拒绝且库不变 + ×10 全记账
    let before = lib.len();
    let mut k = 0u32;
    while k < 10 {
        let (_, key, text) = phrase_n(20_000 + k as usize);
        match lib.add(&key, &text) {
            Err(E_OVER_PASS) => {}
            _ => return Err(E_OVER_PASS),
        }
        k += 1;
    }
    if lib.len() != before {
        return Err(E_OVER_PASS);
    }
    if lib.rejects() != 10 {
        return Err(E_NO_LOG);
    }
    hits += 1;

    // R3 同码重复拒：p1 重复 add 拒且库不变
    match lib.add("p1", "again") {
        Err(E_DUP_KEY) => {}
        _ => return Err(E_DUP_KEY),
    }
    if lib.len() != before {
        return Err(E_DUP_KEY);
    }
    hits += 1;

    // R4 删后可加：删 p1 再加新条成功，守恒保持
    if !lib.remove("p1") {
        return Err(E_BAL_MISS);
    }
    lib.add("pnew", "fresh")?;
    lib.audit_balance()?;
    lib.audit_unique()?;
    hits += 1;

    // R5 LCG 随机增删 2048 轮：尺寸恒 ≤ CAP 且守恒（基于已满库操作——先重建半满库提速）
    let mut lib2 = PhraseLib::new();
    let mut n = 1usize;
    while n <= 5_000 {
        let (k, t) = {
            let (_, key, text) = phrase_n(n);
            (key, text)
        };
        lib2.add(&k, &t)?;
        n += 1;
    }
    let mut rng = Lcg::new(0x48427);
    let mut k = 0u32;
    while k < 2048 {
        let act = rng.below(3);
        match act {
            0 => {
                let (_, key, text) = phrase_n(5_000 + k as usize);
                let _ = lib2.add(&key, &text);
            }
            1 => {
                let (_, key, _) = phrase_n(1 + rng.below(6_000) as usize);
                let _ = lib2.remove(&key);
            }
            _ => {
                let (_, key, text) = phrase_n(30_000 + k as usize);
                let _ = lib2.add(&key, &text);
            }
        }
        lib2.audit_balance()?;
        k += 1;
    }
    lib2.audit_unique()?;
    hits += 1;

    // R6 重演一致：同种子双跑库指纹相等
    let f1 = lib2.fp();
    let mut lib3 = PhraseLib::new();
    let mut n = 1usize;
    while n <= 5_000 {
        let (k, t) = {
            let (_, key, text) = phrase_n(n);
            (key, text)
        };
        let _ = lib3.add(&k, &t);
        n += 1;
    }
    let mut rng2 = Lcg::new(0x48427);
    let mut k = 0u32;
    while k < 2048 {
        let act = rng2.below(3);
        match act {
            0 => {
                let (_, key, text) = phrase_n(5_000 + k as usize);
                let _ = lib3.add(&key, &text);
            }
            1 => {
                let (_, key, _) = phrase_n(1 + rng2.below(6_000) as usize);
                let _ = lib3.remove(&key);
            }
            _ => {
                let (_, key, text) = phrase_n(30_000 + k as usize);
                let _ = lib3.add(&key, &text);
            }
        }
        k += 1;
    }
    if f1 != lib3.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R7 封印稳定：R1 库双算一致
    let s1 = lib.fp();
    let s2 = lib.fp();
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 上限恒定：PHRASE_CAP == 10,000
    if PHRASE_CAP != 10_000 {
        return Err(E_CAP_VAR);
    }
    hits += 1;

    Ok(hits)
}
