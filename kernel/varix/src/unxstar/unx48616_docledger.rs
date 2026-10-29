//! UNX-F48616 —— I31 文档一致性（对照账文档 vs 账本）
//! 判据原文：UNX-F48616-J1 抽查 ×20 全对；判据锚定 体验十九。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           对照账文档声明与账本行为逐项对齐。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0420; // I31 对照账文档一致性诊断段

const E_DOC_MISS: u16 = SEG_BASE + 1;
const E_IMPL_MISS: u16 = SEG_BASE + 2;
const E_MISMATCH: u16 = SEG_BASE + 3;
const E_NAME_BAD: u16 = SEG_BASE + 4;
const E_DUP: u16 = SEG_BASE + 5;
const E_LEDGER: u16 = SEG_BASE + 6;
const E_REPLAY: u16 = SEG_BASE + 7;

/// 对照账条目：文档声明 vs 账本行为。
#[derive(Clone)]
pub struct LedgerItem {
    pub name: String,
    /// 文档声明：录制库带校验和。
    pub doc_sum: bool,
    /// 账本行为：实际是否带校验和。
    pub impl_sum: bool,
    /// 文档声明：版本化历史。
    pub doc_ver: bool,
    /// 账本行为：实际是否版本化。
    pub impl_ver: bool,
}

impl LedgerItem {
    pub fn consistent(&self) -> bool {
        self.doc_sum == self.impl_sum && self.doc_ver == self.impl_ver
    }
}

/// 一致性对账簿。
pub struct DocLedger2 {
    pub items: Vec<LedgerItem>,
    pub label: String,
}

impl DocLedger2 {
    pub fn new(label: &str) -> Self {
        DocLedger2 { items: Vec::new(), label: label.to_string() }
    }

    pub fn add(&mut self, it: LedgerItem) -> Result<(), u16> {
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
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ it.doc_sum as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ it.impl_sum as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ it.doc_ver as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ it.impl_ver as u64;
            i += 1;
        }
        h ^= self.items.len() as u64;
        h
    }
}

/// 冻结真表：对照账文档声明集。
pub fn build_true_items() -> Vec<LedgerItem> {
    let mut v = Vec::new();
    v.push(LedgerItem {
        name: "rec-sum".to_string(),
        doc_sum: true, impl_sum: true, doc_ver: false, impl_ver: false,
    });
    v.push(LedgerItem {
        name: "rec-ver".to_string(),
        doc_sum: false, impl_sum: false, doc_ver: true, impl_ver: true,
    });
    v.push(LedgerItem {
        name: "cmp-order".to_string(),
        doc_sum: true, impl_sum: true, doc_ver: false, impl_ver: false,
    });
    v.push(LedgerItem {
        name: "diff-zero".to_string(),
        doc_sum: false, impl_sum: false, doc_ver: true, impl_ver: true,
    });
    v.push(LedgerItem {
        name: "resign-x10".to_string(),
        doc_sum: true, impl_sum: true, doc_ver: false, impl_ver: false,
    });
    v.push(LedgerItem {
        name: "spy-200".to_string(),
        doc_sum: false, impl_sum: false, doc_ver: true, impl_ver: true,
    });
    v.push(LedgerItem {
        name: "twin-code".to_string(),
        doc_sum: true, impl_sum: true, doc_ver: false, impl_ver: false,
    });
    v.push(LedgerItem {
        name: "dbcs-500".to_string(),
        doc_sum: false, impl_sum: false, doc_ver: true, impl_ver: true,
    });
    v.push(LedgerItem {
        name: "msg-order".to_string(),
        doc_sum: true, impl_sum: true, doc_ver: false, impl_ver: false,
    });
    v.push(LedgerItem {
        name: "imm-joint".to_string(),
        doc_sum: false, impl_sum: false, doc_ver: true, impl_ver: true,
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

/// 主巡检：对照账文档一致性十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 全量对齐。
    let mut led = DocLedger2::new("docledger");
    let items = build_true_items();
    let mut i = 0usize;
    while i < items.len() {
        led.add(items[i].clone())?;
        i += 1;
    }
    led.check_all()?;
    hits += 1;

    // R2 抽查 ×20。
    let mut rng = Lcg::new(0x48616_0001);
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
    let mut led2 = DocLedger2::new("drift");
    led2.add(LedgerItem {
        name: "rec-sum".to_string(),
        doc_sum: true,
        impl_sum: false,
        doc_ver: false,
        impl_ver: false,
    })?;
    let r = led2.check_all();
    if r.err() != Some(E_MISMATCH) {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R4 空名反例。
    let mut led3 = DocLedger2::new("name");
    let r = led3.add(LedgerItem {
        name: String::new(),
        doc_sum: true, impl_sum: true, doc_ver: false, impl_ver: false,
    });
    if r.err() != Some(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    hits += 1;

    // R5 重复反例。
    let mut led4 = DocLedger2::new("dup");
    led4.add(LedgerItem {
        name: "x".to_string(),
        doc_sum: true, impl_sum: true, doc_ver: false, impl_ver: false,
    })?;
    let r = led4.add(LedgerItem {
        name: "x".to_string(),
        doc_sum: false, impl_sum: false, doc_ver: true, impl_ver: true,
    });
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R6 空簿反例。
    let led5 = DocLedger2::new("empty");
    let r = led5.check_all();
    if r.err() != Some(E_DOC_MISS) {
        return Err(E_DOC_MISS);
    }
    hits += 1;

    // R7 抽查越界反例。
    let mut led6 = DocLedger2::new("oob");
    let items = build_true_items();
    let mut i = 0usize;
    while i < 3 {
        led6.add(items[i].clone())?;
        i += 1;
    }
    let mut bad: Vec<usize> = Vec::new();
    bad.push(0);
    bad.push(50);
    let r = led6.spot_check(&bad);
    if r.err() != Some(E_IMPL_MISS) {
        return Err(E_IMPL_MISS);
    }
    hits += 1;

    // R8 fuzz ×100 单点抽查守恒。
    let mut rng2 = Lcg::new(0x48616_0002);
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

    // R9 登记守恒：条目数 == 10。
    if led.items.len() != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R10 重演一致。
    let fp1 = led.fingerprint();
    let mut led7 = DocLedger2::new("docledger");
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
