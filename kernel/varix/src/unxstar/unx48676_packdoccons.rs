//! UNX-F48676 —— I34 文档一致性（打分输入包文档 vs 账本）
//! 判据原文：UNX-F48676-J1 抽查 ×20 全对；判据锚定 体验十九。
//! 判据落地（drill 场景）：R1 登记 ×20；R2 逐条全对；R3 错位检出；
//! R4 锚唯一；R5 值域回归；R6 文档行渲染；R7 抽查可复跑；
//! R8 指纹稳定；R9 计数守恒；R10 呈现行。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG 抽查索引；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0BE0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_MISMATCH: u16 = SEG_BASE + 1; // 文档值与实现值失配
const E_ANCHOR: u16 = SEG_BASE + 2; // 锚为空或前缀非法
const E_DUPANCHOR: u16 = SEG_BASE + 3; // 锚重复
const E_RANGE: u16 = SEG_BASE + 4; // 实现值越出值域
const E_COUNT: u16 = SEG_BASE + 5; // 抽查数不足 20
const E_RENDER: u16 = SEG_BASE + 6; // 文档行渲染缺段
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_SANE: u16 = SEG_BASE + 8; // 计数不守恒
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_REPLAY: u16 = SEG_BASE + 10; // 抽查复跑不一致

/// 抽查条数。
pub const CHECK_N: usize = 20;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一条文档-实现对照项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocItem {
    pub anchor: String,
    pub doc_val: u64,
    pub impl_val: u64,
    pub lo: u64,
    pub hi: u64,
}

impl DocItem {
    pub fn new(anchor: &str, doc_val: u64, impl_val: u64, lo: u64, hi: u64) -> DocItem {
        DocItem { anchor: anchor.to_string(), doc_val, impl_val, lo, hi }
    }

    /// 锚合法：非空且以 UNX- 前缀开头。
    pub fn anchor_ok(&self) -> bool {
        let b = self.anchor.as_bytes();
        b.len() >= 4 && b[0] == b'U' && b[1] == b'N' && b[2] == b'X' && b[3] == b'-'
    }

    /// 一致性。
    pub fn consistent(&self) -> Result<(), u16> {
        if !self.anchor_ok() {
            return Err(E_ANCHOR);
        }
        if self.impl_val < self.lo || self.impl_val > self.hi {
            return Err(E_RANGE);
        }
        if self.doc_val != self.impl_val {
            return Err(E_MISMATCH);
        }
        Ok(())
    }

    /// 文档行渲染。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(self.anchor.as_str());
        out.push_str(": doc=");
        out.push_str(&self.doc_val.to_string());
        out.push_str(" impl=");
        out.push_str(&self.impl_val.to_string());
        out
    }
}

/// 对照账。
pub struct DocLedger {
    pub items: Vec<DocItem>,
}

impl DocLedger {
    pub fn new() -> DocLedger {
        DocLedger { items: vec![] }
    }

    /// 登记：锚唯一。
    pub fn register(&mut self, it: DocItem) -> Result<(), u16> {
        if !it.anchor_ok() {
            return Err(E_ANCHOR);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].anchor == it.anchor {
                return Err(E_DUPANCHOR);
            }
            i += 1;
        }
        self.items.push(it);
        Ok(())
    }

    /// 逐条全检。
    pub fn check_all(&self) -> Result<u32, u16> {
        let mut ok: u32 = 0;
        let mut i = 0usize;
        while i < self.items.len() {
            self.items[i].consistent()?;
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 锚唯一自检。
    pub fn anchors_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            let mut j = i + 1;
            while j < self.items.len() {
                if self.items[i].anchor == self.items[j].anchor {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 账面指纹。
pub fn ledger_fp(l: &DocLedger) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < l.items.len() {
        buf.extend_from_slice(&l.items[i].doc_val.to_le_bytes());
        buf.extend_from_slice(&l.items[i].impl_val.to_le_bytes());
        buf.extend_from_slice(l.items[i].anchor.as_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行。
pub fn ledger_line(l: &DocLedger, checked: u32) -> String {
    let f = ledger_fp(l) as u32;
    let mut out = String::new();
    out.push_str("items=");
    out.push_str(&l.items.len().to_string());
    out.push_str(" checked=");
    out.push_str(&checked.to_string());
    out.push_str(" fp32=");
    out.push_str(&f.to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 锚拼装：UNX-F48676-J<i+1>。
pub fn concat_anchor(i: u32) -> String {
    let mut out = String::new();
    out.push_str("UNX-F48676-J");
    out.push_str(&(i + 1).to_string());
    out
}

/// 构造 ×20 一致对照项。
pub fn build_consistent(lcgc: &mut Lcg) -> Vec<DocItem> {
    let mut v: Vec<DocItem> = vec![];
    let mut i: u32 = 0;
    while i < CHECK_N as u32 {
        let val = 200 + lcgc.below(8000);
        let anchor = concat_anchor(i);
        v.push(DocItem::new(&anchor, val as u64, val as u64, 0, 10_000));
        i += 1;
    }
    v
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 登记 ×20。
    let mut g = Lcg::new(0x486_76);
    let mut led = DocLedger::new();
    let items = build_consistent(&mut g);
    let mut i = 0usize;
    while i < items.len() {
        led.register(items[i].clone())?;
        i += 1;
    }
    if led.items.len() != CHECK_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 逐条全对。
    let ok = led.check_all()?;
    if ok != CHECK_N as u32 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 错位检出。
    let mut bad = led.items[9].clone();
    bad.impl_val = bad.impl_val.wrapping_add(2);
    match bad.consistent() {
        Err(E_MISMATCH) => hits += 1,
        _ => return Err(E_MISMATCH),
    }

    // R4 锚唯一。
    if !led.anchors_unique() {
        return Err(E_DUPANCHOR);
    }
    let dup = DocItem::new("UNX-F48676-J2", 1, 1, 0, 10);
    match led.register(dup) {
        Err(E_DUPANCHOR) => hits += 1,
        _ => return Err(E_DUPANCHOR),
    }

    // R5 值域回归。
    let oob = DocItem::new("UNX-F48676-Z1", 5, 99_999, 0, 10_000);
    match oob.consistent() {
        Err(E_RANGE) => hits += 1,
        _ => return Err(E_RANGE),
    }

    // R6 文档行渲染。
    let r0 = led.items[1].render();
    if !has_kw(r0.as_bytes(), b"UNX-F48676-J2") || !has_kw(r0.as_bytes(), b"doc=")
        || !has_kw(r0.as_bytes(), b"impl=")
    {
        return Err(E_RENDER);
    }
    hits += 1;

    // R7 抽查可复跑。
    let a = led.check_all()?;
    let b = led.check_all()?;
    if a != b {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = ledger_fp(&led);
    let f2 = ledger_fp(&led);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 计数守恒。
    if led.items.len() as u32 != a {
        return Err(E_SANE);
    }
    hits += 1;

    // R10 呈现行。
    let line = ledger_line(&led, a);
    if !has_kw(line.as_bytes(), b"items=20") || !has_kw(line.as_bytes(), b"checked=20") {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}
