//! UNX-F48429 —— E·超长 preedit span（正：span 边界越界防御路径）
//! 判据原文：UNX-F48429-J1 span (offset,len) 对文本逐段校验：合法 span 全收；
//!           越界 span（offset+len > 文本长）走防御路径（拒绝+告警）×10 全记；
//!           LCG 随机 span 512 全有界；合法 span 切片内容正确（子串恒等）；
//!           防御账封印重演一致；空 span（len=0）合法零告警。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD960; // E22 preedit span 段

const E_OVER_PASS: u16 = SEG_BASE + 1; // 越界 span 放行
const E_SLICE_MISS: u16 = SEG_BASE + 2; // 切片内容错
const E_NO_WARN: u16 = SEG_BASE + 3; // 越界未告警
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_COUNT_MISS: u16 = SEG_BASE + 6; // 防御账计数不符
const E_TEXT_MISS: u16 = SEG_BASE + 7; // 文本锚缺失

/// span 描述。
#[derive(Clone, Copy)]
pub struct Span {
    pub offset: usize,
    pub len: usize,
}

/// preedit 校验器。
pub struct SpanGuard {
    text: String,
    defended: u32,
    warned: bool,
}

impl SpanGuard {
    pub fn new(text: &str) -> Self {
        SpanGuard { text: String::from(text), defended: 0, warned: false }
    }

    pub fn text_len(&self) -> usize {
        self.text.len()
    }

    /// span 校验：合法返回子串；越界拒绝+告警+防御账。
    pub fn check(&mut self, sp: Span) -> Result<String, u16> {
        if sp.offset > self.text.len() || sp.len > self.text.len() - sp.offset {
            self.defended += 1;
            self.warned = true;
            return Err(E_OVER_PASS);
        }
        // 子串提取（逐字节，内容恒等）
        let b = self.text.as_bytes();
        let mut s = String::new();
        let mut i = sp.offset;
        while i < sp.offset + sp.len {
            s.push(b[i] as char);
            i += 1;
        }
        Ok(s)
    }

    /// 防御账一致：有防御必有告警。
    pub fn audit_warn_pair(&self) -> Result<(), u16> {
        if self.defended > 0 && !self.warned {
            return Err(E_NO_WARN);
        }
        if self.defended == 0 && self.warned {
            return Err(E_NO_WARN);
        }
        Ok(())
    }

    /// 文本锚存在：len > 0。
    pub fn audit_anchor(&self) -> Result<(), u16> {
        if self.text.is_empty() {
            return Err(E_TEXT_MISS);
        }
        Ok(())
    }

    pub fn defended(&self) -> u32 {
        self.defended
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let b = self.text.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.defended as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
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
        self.next() % n
    }
}

/// 主巡检：preedit span 八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // 锚文本：64 字节
    let mut anchor = String::new();
    let mut i = 0usize;
    while i < 64 {
        anchor.push((b'a' + (i % 26) as u8) as char);
        i += 1;
    }

    // R1 合法 span 全收：全文本 span 内容恒等
    let mut g = SpanGuard::new(&anchor);
    g.audit_anchor()?;
    let whole = g.check(Span { offset: 0, len: 64 })?;
    if whole.len() != 64 {
        return Err(E_SLICE_MISS);
    }
    hits += 1;

    // R2 中段切片内容正确：offset 10 len 5 = 'k'..='o'（i=10..14 → 'k','l','m','n','o'）
    let mid = g.check(Span { offset: 10, len: 5 })?;
    let expect: String = {
        let mut s = String::new();
        let mut i = 10usize;
        while i < 15 {
            s.push((b'a' + (i % 26) as u8) as char);
            i += 1;
        }
        s
    };
    if mid != expect {
        return Err(E_SLICE_MISS);
    }
    hits += 1;

    // R3 空 span 合法零告警：len=0 返回空串、防御账 0
    let empty = g.check(Span { offset: 32, len: 0 })?;
    if !empty.is_empty() || g.defended() != 0 {
        return Err(E_SLICE_MISS);
    }
    hits += 1;

    // R4 越界 span ×10 全拒全记：offset+len > 64 十种
    let mut k = 0u32;
    while k < 10 {
        let off = 60 + k as usize;
        match g.check(Span { offset: off, len: 8 }) {
            Err(E_OVER_PASS) => {}
            _ => return Err(E_OVER_PASS),
        }
        k += 1;
    }
    if g.defended() != 10 {
        return Err(E_COUNT_MISS);
    }
    g.audit_warn_pair()?;
    hits += 1;

    // R5 offset 越界（offset > len）：防御路径同款
    let mut g2 = SpanGuard::new(&anchor);
    match g2.check(Span { offset: 65, len: 1 }) {
        Err(E_OVER_PASS) => {}
        _ => return Err(E_OVER_PASS),
    }
    if g2.defended() != 1 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R6 LCG 随机 span 512 全有界：合法/越界混合，防御账 = 越界数
    let mut g3 = SpanGuard::new(&anchor);
    let mut rng = Lcg::new(0x48429);
    let mut over_seen = 0u32;
    let mut k = 0u32;
    while k < 512 {
        let off = rng.below(70) as usize;
        let len = rng.below(10) as usize;
        match g3.check(Span { offset: off, len }) {
            Ok(_) => {}
            Err(E_OVER_PASS) => over_seen += 1,
            Err(_) => return Err(E_OVER_PASS),
        }
        k += 1;
    }
    if g3.defended() != over_seen {
        return Err(E_COUNT_MISS);
    }
    g3.audit_warn_pair()?;
    hits += 1;

    // R7 重演一致：同种子双跑防御账指纹相等
    let f1 = g3.fp();
    let mut g4 = SpanGuard::new(&anchor);
    let mut rng2 = Lcg::new(0x48429);
    let mut k = 0u32;
    while k < 512 {
        let off = rng2.below(70) as usize;
        let len = rng2.below(10) as usize;
        let _ = g4.check(Span { offset: off, len });
        k += 1;
    }
    if f1 != g4.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 边界 span：offset=64 len=0 合法（贴尾空 span）、offset=0 len=65 拒
    let tail = g.check(Span { offset: 64, len: 0 })?;
    if !tail.is_empty() {
        return Err(E_SLICE_MISS);
    }
    match g.check(Span { offset: 0, len: 65 }) {
        Err(E_OVER_PASS) => {}
        _ => return Err(E_OVER_PASS),
    }
    hits += 1;

    Ok(hits)
}
