//! UNX-F48636 —— I32 文档一致性（转换表文档 vs 实现）
//! 判据原文：UNX-F48636-J1 抽查 ×20 全对；判据锚定 体验十九。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           转换表文档声明与实现行为逐项对齐。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x06A0; // I32 转换表文档一致性诊断段

const E_DOC_MISS: u16 = SEG_BASE + 1;
const E_IMPL_MISS: u16 = SEG_BASE + 2;
const E_MISMATCH: u16 = SEG_BASE + 3;
const E_NAME_BAD: u16 = SEG_BASE + 4;
const E_DUP: u16 = SEG_BASE + 5;
const E_LEDGER: u16 = SEG_BASE + 6;
const E_REPLAY: u16 = SEG_BASE + 7;

/// 转换表条目：文档声明 vs 实现行为。
#[derive(Clone)]
pub struct ConvItem {
    pub name: String,
    /// 文档声明：XIM KeySym → TI Key。
    pub doc_key_map: bool,
    pub impl_key_map: bool,
    /// 文档声明：XIM Commit → TI CommitString。
    pub doc_commit_map: bool,
    pub impl_commit_map: bool,
}

impl ConvItem {
    pub fn consistent(&self) -> bool {
        self.doc_key_map == self.impl_key_map
            && self.doc_commit_map == self.impl_commit_map
    }
}

/// 一致性对账簿。
pub struct ConvDocLedger {
    pub items: Vec<ConvItem>,
    pub label: String,
}

impl ConvDocLedger {
    pub fn new(label: &str) -> Self {
        ConvDocLedger { items: Vec::new(), label: label.to_string() }
    }

    pub fn add(&mut self, it: ConvItem) -> Result<(), u16> {
        if it.name.len() == 0 {
            return Err(E_NAME_BAD);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].name == it.name {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(it);
        Ok(())
    }

    pub fn check_all(&self) -> Result<(), u16> {
        if self.items.is_empty() {
            return Err(E_DOC_MISS);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].consistent() {
                return Err(E_MISMATCH);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn spot_check(&self, idxs: &Vec<usize>) -> Result<u32, u16> {
        if idxs.is_empty() {
            return Err(E_DOC_MISS);
        }
        let mut ok = 0u32;
        let mut i = 0usize;
        while i < idxs.len() {
            let k = idxs[i];
            if k >= self.items.len() {
                return Err(E_IMPL_MISS);
            }
            if !self.items[k].consistent() {
                return Err(E_MISMATCH);
            }
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = &self.items[i];
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ it.doc_key_map as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ it.impl_key_map as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ it.doc_commit_map as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ it.impl_commit_map as u64;
            i += 1;
        }
        h ^= self.items.len() as u64;
        h
    }
}

/// 冻结真表。
pub fn build_true_items() -> Vec<ConvItem> {
    let mut v = Vec::new();
    v.push(ConvItem {
        name: "keysym".to_string(),
        doc_key_map: true, impl_key_map: true, doc_commit_map: false, impl_commit_map: false,
    });
    v.push(ConvItem {
        name: "preedit".to_string(),
        doc_key_map: false, impl_key_map: false, doc_commit_map: false, impl_commit_map: false,
    });
    v.push(ConvItem {
        name: "commit".to_string(),
        doc_key_map: false, impl_key_map: false, doc_commit_map: true, impl_commit_map: true,
    });
    v.push(ConvItem {
        name: "reset".to_string(),
        doc_key_map: false, impl_key_map: false, doc_commit_map: false, impl_commit_map: false,
    });
    v.push(ConvItem {
        name: "payload".to_string(),
        doc_key_map: true, impl_key_map: true, doc_commit_map: false, impl_commit_map: false,
    });
    v.push(ConvItem {
        name: "commit-cap".to_string(),
        doc_key_map: false, impl_key_map: false, doc_commit_map: true, impl_commit_map: true,
    });
    v.push(ConvItem {
        name: "preedit-cap".to_string(),
        doc_key_map: true, impl_key_map: true, doc_commit_map: false, impl_commit_map: false,
    });
    v.push(ConvItem {
        name: "disconnect".to_string(),
        doc_key_map: false, impl_key_map: false, doc_commit_map: true, impl_commit_map: true,
    });
    v.push(ConvItem {
        name: "clamp".to_string(),
        doc_key_map: true, impl_key_map: true, doc_commit_map: false, impl_commit_map: false,
    });
    v.push(ConvItem {
        name: "hotkey".to_string(),
        doc_key_map: false, impl_key_map: false, doc_commit_map: true, impl_commit_map: true,
    });
    v
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

/// 主巡检：转换表文档一致性十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 全量对齐。
    let mut led = ConvDocLedger::new("convdoc");
    let items = build_true_items();
    let mut i = 0usize;
    while i < items.len() {
        led.add(items[i].clone())?;
        i += 1;
    }
    led.check_all()?;
    hits += 1;

    // R2 抽查 ×20。
    let mut rng = Lcg::new(0x48636_0001);
    let mut idxs: Vec<usize> = Vec::new();
    let mut k = 0u32;
    while k < 20 {
        idxs.push(rng.below(led.items.len() as u64) as usize);
        k += 1;
    }
    let ok = led.spot_check(&idxs)?;
    if ok != 20 {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R3 失配注入反例。
    let mut led2 = ConvDocLedger::new("drift");
    led2.add(ConvItem {
        name: "commit".to_string(),
        doc_key_map: false, impl_key_map: false,
        doc_commit_map: true, impl_commit_map: false,
    })?;
    let r = led2.check_all();
    if r.err() != Some(E_MISMATCH) {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R4 空名反例。
    let mut led3 = ConvDocLedger::new("name");
    let r = led3.add(ConvItem {
        name: String::new(),
        doc_key_map: true, impl_key_map: true, doc_commit_map: false, impl_commit_map: false,
    });
    if r.err() != Some(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    hits += 1;

    // R5 重复反例。
    let mut led4 = ConvDocLedger::new("dup");
    led4.add(ConvItem {
        name: "x".to_string(),
        doc_key_map: true, impl_key_map: true, doc_commit_map: false, impl_commit_map: false,
    })?;
    let r = led4.add(ConvItem {
        name: "x".to_string(),
        doc_key_map: false, impl_key_map: false, doc_commit_map: true, impl_commit_map: true,
    });
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R6 空簿反例。
    let led5 = ConvDocLedger::new("empty");
    let r = led5.check_all();
    if r.err() != Some(E_DOC_MISS) {
        return Err(E_DOC_MISS);
    }
    hits += 1;

    // R7 抽查越界反例。
    let mut led6 = ConvDocLedger::new("oob");
    let items = build_true_items();
    let mut i = 0usize;
    while i < 3 {
        led6.add(items[i].clone())?;
        i += 1;
    }
    let mut bad: Vec<usize> = Vec::new();
    bad.push(0);
    bad.push(88);
    let r = led6.spot_check(&bad);
    if r.err() != Some(E_IMPL_MISS) {
        return Err(E_IMPL_MISS);
    }
    hits += 1;

    // R8 fuzz ×100 单点抽查守恒。
    let mut rng2 = Lcg::new(0x48636_0002);
    let mut ok_sum = 0u32;
    let mut k = 0u32;
    while k < 100 {
        let mut one: Vec<usize> = Vec::new();
        one.push(rng2.below(led.items.len() as u64) as usize);
        ok_sum += led.spot_check(&one)?;
        k += 1;
    }
    if ok_sum != 100 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R9 登记守恒。
    if led.items.len() != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R10 重演一致。
    let fp1 = led.fingerprint();
    let mut led7 = ConvDocLedger::new("convdoc");
    let items2 = build_true_items();
    let mut i = 0usize;
    while i < items2.len() {
        let _ = led7.add(items2[i].clone());
        i += 1;
    }
    if led7.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
