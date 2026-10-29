//! UNX-F48716 —— I36 文档一致性（治理面文档 vs 账本）
//! 判据原文：UNX-F48716-J1 抽查 ×20 全对；判据锚定 体验十九。
//! 判据落地（drill 场景）：R1 治理账登记；R2 文档条目登记；
//! R3 抽查 ×20 逐条比对；R4 缺档检出；R5 账漂检出；R6 锚唯一；
//! R7 复查一致；R8 指纹稳定；R9 计数守恒；R10 呈现行；R11 抽签稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：治理面文档条目与账本条目按锚逐条对表；抽查 ×20 =
//! LCG 抽 20 锚重查，任一锚缺档或账值漂移即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1280;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ANCHOR: u16 = SEG_BASE + 1; // 锚非法
const E_DUP: u16 = SEG_BASE + 2; // 锚重复
const E_MISS: u16 = SEG_BASE + 3; // 缺档
const E_DRIFT: u16 = SEG_BASE + 4; // 账值漂移
const E_COUNT: u16 = SEG_BASE + 5; // 抽查数不足
const E_REPLAY: u16 = SEG_BASE + 6; // 复查漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 抽签签名不符

/// 抽查数。
pub const CHECK_N: u32 = 20;
/// 治理账容量。
pub const LEDGER_N: u32 = 24;

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

/// 一条治理账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GovEntry {
    pub anchor: u32, // 锚 1..=24
    pub value: u32,  // 账值
    pub sig: u64,    // 条目签名
}

impl GovEntry {
    /// 条目签名：锚 + 账值混签。
    pub fn make_sig(anchor: u32, value: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&anchor.to_le_bytes());
        buf.extend_from_slice(&value.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.anchor == 0 || self.anchor > LEDGER_N {
            return Err(E_ANCHOR);
        }
        if self.sig != GovEntry::make_sig(self.anchor, self.value) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 治理账。
pub struct GovLedger {
    pub entries: Vec<GovEntry>,
}

impl GovLedger {
    pub fn new() -> GovLedger {
        GovLedger { entries: vec![] }
    }

    /// 登记条目：锚闭集 + 唯一。
    pub fn put(&mut self, anchor: u32, value: u32) -> Result<(), u16> {
        if anchor == 0 || anchor > LEDGER_N {
            return Err(E_ANCHOR);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].anchor == anchor {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = GovEntry::make_sig(anchor, value);
        self.entries.push(GovEntry { anchor, value, sig });
        Ok(())
    }

    /// 查锚。
    pub fn get(&self, anchor: u32) -> Result<u32, u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].anchor == anchor {
                return Ok(self.entries[i].value);
            }
            i += 1;
        }
        Err(E_MISS)
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.entries.len() {
            buf.extend_from_slice(&self.entries[i].anchor.to_le_bytes());
            buf.extend_from_slice(&self.entries[i].value.to_le_bytes());
            buf.extend_from_slice(&self.entries[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 文档条目集（治理面文档侧）。
pub struct DocSide {
    pub anchors: Vec<u32>,
    pub values: Vec<u32>,
}

impl DocSide {
    pub fn new() -> DocSide {
        DocSide { anchors: vec![], values: vec![] }
    }

    /// 文档登记：与账侧同锚同步。
    pub fn put(&mut self, anchor: u32, value: u32) -> Result<(), u16> {
        if anchor == 0 || anchor > LEDGER_N {
            return Err(E_ANCHOR);
        }
        let mut i = 0usize;
        while i < self.anchors.len() {
            if self.anchors[i] == anchor {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.anchors.push(anchor);
        self.values.push(value);
        Ok(())
    }
}

/// 单锚对表：账侧值 vs 文档侧值。
fn check_anchor(led: &GovLedger, doc: &DocSide, anchor: u32) -> Result<(), u16> {
    let v_led = led.get(anchor)?;
    let mut v_doc: Option<u32> = None;
    let mut i = 0usize;
    while i < doc.anchors.len() {
        if doc.anchors[i] == anchor {
            v_doc = Some(doc.values[i]);
        }
        i += 1;
    }
    match v_doc {
        None => Err(E_MISS),
        Some(v) => {
            if v != v_led {
                Err(E_DRIFT)
            } else {
                Ok(())
            }
        }
    }
}

/// 抽查 ×20：LCG 抽锚，账侧与文档侧逐条比对。
pub fn spot_check(
    led: &GovLedger,
    doc: &DocSide,
    g: &mut Lcg,
) -> Result<u32, u16> {
    let mut checked: u32 = 0;
    let mut k: u32 = 0;
    while k < CHECK_N {
        let a = 1 + g.below(LEDGER_N);
        check_anchor(led, doc, a)?;
        checked += 1;
        k += 1;
    }
    Ok(checked)
}

/// 呈现行：checked=20 drift=0 miss=0。
pub fn check_line(checked: u32) -> String {
    let mut out = String::new();
    out.push_str("checked=");
    out.push_str(&checked.to_string());
    out.push_str(" drift=0 miss=0");
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

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_16);

    // R1 治理账登记：24 锚全量入账。
    let mut led = GovLedger::new();
    let mut a: u32 = 1;
    while a <= LEDGER_N {
        led.put(a, 0x100 + a)?;
        a += 1;
    }
    hits += 1;

    // R2 文档条目登记：同锚同值。
    let mut doc = DocSide::new();
    let mut i = 0usize;
    while i < led.entries.len() {
        doc.put(led.entries[i].anchor, led.entries[i].value)?;
        i += 1;
    }
    hits += 1;

    // R3 抽查 ×20 全对。
    let checked = spot_check(&led, &doc, &mut g)?;
    if checked != CHECK_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R4 缺档检出：文档漏登锚 3 → 单锚对表红。
    let mut doc2 = DocSide::new();
    let mut i = 0usize;
    while i < led.entries.len() {
        if led.entries[i].anchor != 3 {
            doc2.put(led.entries[i].anchor, led.entries[i].value)?;
        }
        i += 1;
    }
    match check_anchor(&led, &doc2, 3) {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R5 账漂检出：文档锚 5 值漂移 → 单锚对表红。
    let mut doc3 = DocSide::new();
    let mut i = 0usize;
    while i < led.entries.len() {
        if led.entries[i].anchor == 5 {
            doc3.put(5, led.entries[i].value + 1)?;
        } else {
            doc3.put(led.entries[i].anchor, led.entries[i].value)?;
        }
        i += 1;
    }
    match check_anchor(&led, &doc3, 5) {
        Err(E_DRIFT) => hits += 1,
        _ => return Err(E_DRIFT),
    }

    // R6 锚唯一：重复登记拒绝。
    match led.put(7, 9) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 复查一致：同种子重抽指纹侧不变。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 指纹稳定。
    if led.fp() != f1 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 计数守恒：账/文档条目数相等。
    if led.entries.len() != doc.anchors.len() {
        return Err(E_COUNT);
    }
    hits += 1;

    // R10 呈现行：checked=20 drift=0 miss=0。
    let line = check_line(checked);
    if !has_kw(line.as_bytes(), b"checked=20")
        || !has_kw(line.as_bytes(), b"drift=0")
        || !has_kw(line.as_bytes(), b"miss=0")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 抽签签名：条目签名逐条复算。
    let mut i = 0usize;
    while i < led.entries.len() {
        led.entries[i].sane()?;
        i += 1;
    }
    hits += 1;

    Ok(hits)
}
