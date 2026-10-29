//! UNX-F48638 —— I·text-input-v2/v3 差异对照账年审（两协议差异表复核）
//! 判据原文：UNX-F48638-J1 复核 ×10 零新增编造；判据锚定 §40 幻觉红线。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           v2/v3 差异表冻结 + 十轮复核零新增。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x06E0; // v2/v3 差异对照账诊断段

const E_FABRICATE: u16 = SEG_BASE + 1; // 新增编造
const E_DIFF_BAD: u16 = SEG_BASE + 2;  // 差异条目非法
const E_PROTO_BAD: u16 = SEG_BASE + 3; // 协议名非法
const E_DUP: u16 = SEG_BASE + 4;       // 条目重复
const E_LEDGER: u16 = SEG_BASE + 5;    // 对照账不守恒
const E_REPLAY: u16 = SEG_BASE + 6;    // 重演不一致
const E_SEAL_BAD: u16 = SEG_BASE + 7;  // 封账后写

/// 协议名（冻结）。
pub const PROTO_V2: &str = "text-input-v2";
pub const PROTO_V3: &str = "text-input-v3";

/// 差异条目：某能力在 v2/v3 中的行为差异。
pub struct DiffItem {
    pub cap: String,
    pub v2_behavior: u8, // 0=不支持 1=支持 2=默认开
    pub v3_behavior: u8,
    pub frozen: bool,
}

impl DiffItem {
    pub fn audit(&self) -> Result<(), u16> {
        if self.cap.len() == 0 {
            return Err(E_DIFF_BAD);
        }
        if self.v2_behavior > 2 || self.v3_behavior > 2 {
            return Err(E_DIFF_BAD);
        }
        if !self.frozen {
            return Err(E_SEAL_BAD);
        }
        Ok(())
    }
}

/// 差异对照账。
pub struct DiffLedger {
    pub items: Vec<DiffItem>,
    pub label: String,
    pub sealed: bool,
}

impl DiffLedger {
    pub fn new(label: &str) -> Self {
        DiffLedger { items: Vec::new(), label: label.to_string(), sealed: false }
    }

    pub fn add(&mut self, cap: &str, v2: u8, v3: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_FABRICATE);
        }
        if cap.len() == 0 {
            return Err(E_DIFF_BAD);
        }
        if v2 > 2 || v3 > 2 {
            return Err(E_DIFF_BAD);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].cap == cap {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(DiffItem {
            cap: cap.to_string(),
            v2_behavior: v2,
            v3_behavior: v3,
            frozen: true,
        });
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 年审复核：差异集与基线一致（零新增零删减）。
    pub fn review(&self, baseline: &Vec<String>) -> Result<u32, u16> {
        if self.items.len() != baseline.len() {
            return Err(E_FABRICATE);
        }
        let mut checked = 0u32;
        let mut i = 0usize;
        while i < baseline.len() {
            let mut found = false;
            let mut j = 0usize;
            while j < self.items.len() {
                if self.items[j].cap == baseline[i] {
                    found = true;
                }
                j += 1;
            }
            if !found {
                return Err(E_FABRICATE);
            }
            checked += 1;
            i += 1;
        }
        Ok(checked)
    }

    /// 全条目审计。
    pub fn audit(&self) -> Result<(), u16> {
        if self.items.is_empty() {
            return Err(E_LEDGER);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            self.items[i].audit()?;
            i += 1;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Vec<String> {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            v.push(self.items[i].cap.clone());
            i += 1;
        }
        v
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.items.len() as u64;
        let mut i = 0usize;
        while i < self.items.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (self.items[i].v2_behavior as u64);
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (self.items[i].v3_behavior as u64);
            let cb = self.items[i].cap.as_bytes();
            let mut j = 0usize;
            while j < cb.len() {
                h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ cb[j] as u64;
                j += 1;
            }
            i += 1;
        }
        h
    }
}

/// 构建冻结差异表（v2/v3 已知差异如实登记）。
pub fn build_diff_ledger() -> Result<DiffLedger, u16> {
    let mut d = DiffLedger::new("ti-v2v3");
    d.add("enable-arg", 2, 1)?;       // v3 enable 带原因参数
    d.add("commit-on-disable", 1, 2)?; // v3 禁用时默认提交
    d.add("surrounding-text", 2, 2)?;  // 两版均默认开
    d.add("mailbox-caps", 1, 1)?;
    Ok(d)
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

/// 主巡检：v2/v3 差异对照年审十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 冻结差异表建立 + 封账 + 审计。
    let mut d = build_diff_ledger()?;
    d.seal();
    d.audit()?;
    let base = d.snapshot();
    if base.len() != 4 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 复核 ×10：十轮复核零新增零删减。
    let mut r = 0u32;
    while r < 10 {
        let checked = d.review(&base)?;
        if checked != 4 {
            return Err(E_FABRICATE);
        }
        r += 1;
    }
    hits += 1;

    // R3 协议名冻结：v2/v3 名非空且互异。
    if PROTO_V2.len() == 0 || PROTO_V3.len() == 0 || PROTO_V2 == PROTO_V3 {
        return Err(E_PROTO_BAD);
    }
    hits += 1;

    // R4 封账后新增反例：seal 后 add 须拒（编造红线）。
    let r = d.add("ghost-cap", 1, 1);
    if r.err() != Some(E_FABRICATE) {
        return Err(E_FABRICATE);
    }
    hits += 1;

    // R5 重复能力名反例。
    let mut d2 = DiffLedger::new("dup");
    d2.add("cap-a", 1, 0)?;
    let r = d2.add("cap-a", 0, 1);
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R6 非法行为值反例：behavior > 2 须拒。
    let mut d3 = DiffLedger::new("badval");
    let r = d3.add("cap-b", 3, 1);
    if r.err() != Some(E_DIFF_BAD) {
        return Err(E_DIFF_BAD);
    }
    let r = d3.add("cap-b", 1, 9);
    if r.err() != Some(E_DIFF_BAD) {
        return Err(E_DIFF_BAD);
    }
    hits += 1;

    // R7 空能力名反例。
    let mut d4 = DiffLedger::new("empty");
    let r = d4.add("", 1, 1);
    if r.err() != Some(E_DIFF_BAD) {
        return Err(E_DIFF_BAD);
    }
    hits += 1;

    // R8 编造检出反例：基线外条目使 review 红。
    let mut d5 = DiffLedger::new("inject");
    d5.add("real-cap", 1, 2)?;
    d5.seal();
    let base2 = d5.snapshot();
    let mut d6 = DiffLedger::new("inject2");
    d6.add("real-cap", 1, 2)?;
    let _ = d6.add("ghost", 1, 1);
    d6.seal();
    let r = d6.review(&base2);
    if r.err() != Some(E_FABRICATE) {
        return Err(E_FABRICATE);
    }
    hits += 1;

    // R9 fuzz ×64 随机条目账审计：行为值域封闭。
    let mut rng = Lcg::new(0x48638_0001);
    let mut d7 = DiffLedger::new("fuzz");
    let mut k = 0u32;
    while k < 64 {
        let cap = match k % 4 {
            0 => "c0",
            1 => "c1",
            2 => "c2",
            _ => "c3",
        };
        let v2 = rng.below(3) as u8;
        let v3 = rng.below(3) as u8;
        if d7.items.len() < 4 || !{
            let mut has = false;
            let mut i = 0usize;
            while i < d7.items.len() {
                if d7.items[i].cap == cap {
                    has = true;
                }
                i += 1;
            }
            !has
        } {
            let _ = d7.add(cap, v2, v3);
        }
        k += 1;
    }
    d7.audit()?;
    hits += 1;

    // R10 重演一致：同种子重建差异表指纹相等。
    let fp1 = d.fingerprint();
    let d8 = build_diff_ledger()?;
    if d8.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
