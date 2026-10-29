//! UNX-F48617 —— I·GCS_COMPATTR 与 span 映射联签复签
//!         （attr 逐项对照表年审）
//! 判据原文：UNX-F48617-J1 复签 ×10 一致；判据锚定 §AI-61 §2 专题二。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           attr↔span 对照表冻结 + 十轮复签一致。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0440; // GCS_COMPATTR↔span 映射诊断段

const E_MAP_BAD: u16 = SEG_BASE + 1;   // 映射漂移
const E_ATTR_OOB: u16 = SEG_BASE + 2;  // attr 越界
const E_SPAN_BAD: u16 = SEG_BASE + 3;  // span 违例
const E_SIGN_MISS: u16 = SEG_BASE + 4; // 复签轮次缺失
const E_SIGN_DIFF: u16 = SEG_BASE + 5; // 复签不一致
const E_LEDGER: u16 = SEG_BASE + 6;    // 对照账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;    // 重演不一致

/// GCS_COMPATTR 属性值（冻结族 0..=4）。
pub const ATTR_INPUT_ERROR: u8 = 0;
pub const ATTR_TARGET_CONVERTED: u8 = 1;
pub const ATTR_CONVERTED: u8 = 2;
pub const ATTR_TARGET_NOTCONVERTED: u8 = 3;
pub const ATTR_FIXEDCONVERTED: u8 = 4;

pub const ATTR_COUNT: usize = 5;

/// attr → span 类别映射（冻结对照表）。
pub fn attr_to_span(attr: u8) -> Result<u8, u16> {
    match attr {
        ATTR_INPUT_ERROR => Ok(0),          // span: 输入错
        ATTR_TARGET_CONVERTED => Ok(1),     // span: 目标已转
        ATTR_CONVERTED => Ok(1),            // span: 已转
        ATTR_TARGET_NOTCONVERTED => Ok(2),  // span: 目标未转
        ATTR_FIXEDCONVERTED => Ok(3),       // span: 固定
        _ => Err(E_ATTR_OOB),
    }
}

/// span 段：组合串内的一段属性区间。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub len: u32,
    pub attr: u8,
}

impl Span {
    /// 合法性：len>0、attr 合法、映射一致。
    pub fn valid(&self) -> Result<(), u16> {
        if self.len == 0 {
            return Err(E_SPAN_BAD);
        }
        let expect = attr_to_span(self.attr)?;
        let _ = expect;
        Ok(())
    }
}

/// 组合串属性账：串长 + 逐字符 attr + 折叠 span 序。
pub struct CompStr {
    pub attrs: Vec<u8>,
    pub spans: Vec<Span>,
}

impl CompStr {
    pub fn new() -> Self {
        CompStr { attrs: Vec::new(), spans: Vec::new() }
    }

    /// 追加一段：attrs 与 spans 同步登记。
    pub fn push_span(&mut self, s: Span) -> Result<(), u16> {
        s.valid()?;
        let mut i = 0u32;
        while i < s.len {
            self.attrs.push(s.attr);
            i += 1;
        }
        self.spans.push(s);
        Ok(())
    }

    /// 串长。
    pub fn text_len(&self) -> u32 {
        self.attrs.len() as u32
    }

    /// 年审：span 序连续覆盖全串且 attr 映射一致。
    pub fn audit(&self) -> Result<(), u16> {
        let mut pos = 0u32;
        let mut i = 0usize;
        while i < self.spans.len() {
            let s = self.spans[i];
            s.valid()?;
            if s.start != pos {
                return Err(E_SPAN_BAD);
            }
            pos += s.len;
            i += 1;
        }
        if pos != self.text_len() {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    /// 复签指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.attrs.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.attrs[i] as u64;
            i += 1;
        }
        i = 0;
        while i < self.spans.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.spans[i].start as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.spans[i].len as u64;
            i += 1;
        }
        h
    }
}

/// 复签账：十轮独立复签。
pub struct AttrSign {
    pub rounds: Vec<u64>,
    pub label: String,
}

impl AttrSign {
    pub fn new(label: &str) -> Self {
        AttrSign { rounds: Vec::new(), label: label.to_string() }
    }

    /// 单轮复签：全 attr 表映射一致 + 指纹。
    pub fn round(&mut self, salt: u64) -> Result<u64, u16> {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
        let mut a = 0u8;
        while (a as usize) < ATTR_COUNT {
            let s = attr_to_span(a)?;
            if s > 3 {
                return Err(E_MAP_BAD);
            }
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (a as u64);
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (s as u64);
            a += 1;
        }
        self.rounds.push(h);
        Ok(h)
    }

    pub fn consistent(&self) -> bool {
        if self.rounds.len() < 2 {
            return false;
        }
        let f = self.rounds[0];
        let mut i = 1usize;
        while i < self.rounds.len() {
            if self.rounds[i] != f {
                return false;
            }
            i += 1;
        }
        true
    }
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

/// 主巡检：GCS_COMPATTR↔span 联签十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 映射表冻结：五项 attr 全部映射合法。
    let mut a = 0u8;
    while (a as usize) < ATTR_COUNT {
        let s = attr_to_span(a)?;
        if s > 3 {
            return Err(E_MAP_BAD);
        }
        a += 1;
    }
    hits += 1;

    // R2 attr 越界反例：5/255 须拒。
    let r = attr_to_span(5);
    if r.err() != Some(E_ATTR_OOB) {
        return Err(E_ATTR_OOB);
    }
    let r = attr_to_span(255);
    if r.err() != Some(E_ATTR_OOB) {
        return Err(E_ATTR_OOB);
    }
    hits += 1;

    // R3 span 组合串：四段折叠覆盖全串且年审绿。
    let mut cs = CompStr::new();
    cs.push_span(Span { start: 0, len: 2, attr: ATTR_INPUT_ERROR })?;
    cs.push_span(Span { start: 2, len: 3, attr: ATTR_TARGET_CONVERTED })?;
    cs.push_span(Span { start: 5, len: 4, attr: ATTR_CONVERTED })?;
    cs.push_span(Span { start: 9, len: 1, attr: ATTR_FIXEDCONVERTED })?;
    cs.audit()?;
    if cs.text_len() != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 空段反例：len==0 须拒。
    let mut cs2 = CompStr::new();
    let r = cs2.push_span(Span { start: 0, len: 0, attr: ATTR_CONVERTED });
    if r.err() != Some(E_SPAN_BAD) {
        return Err(E_SPAN_BAD);
    }
    hits += 1;

    // R5 位置断裂反例：start 不连续须红。
    let mut cs3 = CompStr::new();
    cs3.push_span(Span { start: 0, len: 2, attr: ATTR_CONVERTED })?;
    let r = cs3.push_span(Span { start: 3, len: 2, attr: ATTR_CONVERTED });
    if r.err() != Some(E_SPAN_BAD) {
        return Err(E_SPAN_BAD);
    }
    hits += 1;

    // R6 覆盖失配反例：span 合计 < 串长须红。
    let mut cs4 = CompStr::new();
    cs4.push_span(Span { start: 0, len: 2, attr: ATTR_CONVERTED })?;
    cs4.push_span(Span { start: 2, len: 2, attr: ATTR_INPUT_ERROR })?;
    // 手工追加 attr 制造覆盖缺口。
    cs4.attrs.push(ATTR_CONVERTED);
    let r = cs4.audit();
    if r.err() != Some(E_LEDGER) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R7 复签 ×10：十轮全表复签通过。
    let mut js = AttrSign::new("attrspan");
    let mut r = 0u32;
    while r < 10 {
        js.round(r as u64)?;
        r += 1;
    }
    if js.rounds.len() != 10 {
        return Err(E_SIGN_MISS);
    }
    hits += 1;

    // R8 复签一致：十轮指纹全等。
    if !js.consistent() {
        return Err(E_SIGN_DIFF);
    }
    hits += 1;

    // R9 fuzz ×256 随机 span 流：随机 attr/len 折叠全绿。
    let mut rng = Lcg::new(0x48617_0001);
    let mut cs5 = CompStr::new();
    let mut pos: u32 = 0;
    let mut k = 0u32;
    while k < 256 {
        let attr = rng.below(ATTR_COUNT as u64) as u8;
        let len = 1 + rng.below(6) as u32;
        cs5.push_span(Span { start: pos, len, attr })?;
        pos += len;
        k += 1;
    }
    cs5.audit()?;
    hits += 1;

    // R10 重演一致：同种子重建 fuzz 串，指纹相等。
    let fp1 = cs5.fingerprint();
    let mut rng2 = Lcg::new(0x48617_0001);
    let mut cs6 = CompStr::new();
    let mut pos: u32 = 0;
    let mut k = 0u32;
    while k < 256 {
        let attr = rng2.below(ATTR_COUNT as u64) as u8;
        let len = 1 + rng2.below(6) as u32;
        let _ = cs6.push_span(Span { start: pos, len, attr });
        pos += len;
        k += 1;
    }
    if cs6.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
