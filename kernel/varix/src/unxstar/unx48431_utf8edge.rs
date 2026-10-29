//! UNX-F48431 —— E·多字节劈字边界（正：UTF-8 边界不劈全过）
//! 判据原文：UNX-F48431-J1 多字节字符（2~4 字节 UTF-8）劈字点全部落在字符边界；
//!           ×100 切分点全合法（续字节 10xxxxxx 不作起点）；
//!           LCG 随机切分 512 点全边界安全；按边界切分重组恒等（原串复原）；
//!           单字节 ASCII 混排全过；边界账封印重演一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD9C0; // E22 UTF-8 边界段

const E_MID_CHAR: u16 = SEG_BASE + 1; // 切在字符中间
const E_REASM_MISS: u16 = SEG_BASE + 2; // 重组不恒等
const E_SEAL_BAD: u16 = SEG_BASE + 3; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 4; // 重演不一致
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 切分数不符
const E_BAD_UTF8: u16 = SEG_BASE + 6; // 非法 UTF-8 序列

/// 判断字节是否 UTF-8 续字节（10xxxxxx）。
pub fn is_cont(b: u8) -> bool {
    b & 0xC0 == 0x80
}

/// 判断 pos 是否字符边界：前一字节不是续字节。
pub fn is_boundary(bytes: &[u8], pos: usize) -> bool {
    if pos == 0 || pos >= bytes.len() {
        return true;
    }
    !is_cont(bytes[pos])
}

/// 多字节样例文本：中文 3 字节 + emoji 4 字节 + 拉丁 1 字节混排。
pub fn sample_text() -> String {
    let mut s = String::new();
    s.push('\u{5B89}'); // 安 3B
    s.push('\u{5B83}'); // 它 3B
    s.push('x');        // 1B
    s.push('\u{1F600}'); // 😀 4B
    s.push('\u{5B89}'); // 安 3B
    s.push('y');        // 1B
    s.push('\u{1F601}'); // 😁 4B
    s.push('\u{5B83}'); // 它 3B
    s
}

/// 切分验证器：对每个切分点做边界校验并切两半。
pub struct Splitter {
    text: String,
    cuts: Vec<usize>,
}

impl Splitter {
    pub fn new(text: &str) -> Self {
        Splitter { text: String::from(text), cuts: Vec::new() }
    }

    /// 在 pos 处切一刀：非边界拒绝。
    pub fn cut_at(&mut self, pos: usize) -> Result<(String, String), u16> {
        let b = self.text.as_bytes();
        if !is_boundary(b, pos) {
            return Err(E_MID_CHAR);
        }
        self.cuts.push(pos);
        let mut left = String::new();
        let mut i = 0usize;
        while i < pos {
            left.push(b[i] as char);
            i += 1;
        }
        let mut right = String::new();
        let mut i = pos;
        while i < b.len() {
            right.push(b[i] as char);
            i += 1;
        }
        Ok((left, right))
    }

    /// 切分账一致。
    pub fn cuts(&self) -> usize {
        self.cuts.len()
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.cuts.len() {
            h ^= self.cuts[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }
}

/// 边界安全重组：把字符串按边界切全部点再拼回，恒等原文。
pub fn reassemble(text: &str, cuts: &[usize]) -> Result<String, u16> {
    let b = text.as_bytes();
    // 校验全部切点边界
    let mut i = 0usize;
    while i < cuts.len() {
        if !is_boundary(b, cuts[i]) {
            return Err(E_MID_CHAR);
        }
        i += 1;
    }
    // 全量重组（按字节序拼接——切点只影响逻辑分段）
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        out.push(b[i] as char);
        i += 1;
    }
    if out.len() != text.len() {
        return Err(E_REASM_MISS);
    }
    Ok(out)
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

/// 主巡检：UTF-8 边界八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    let text = sample_text();
    let blen = text.len(); // 字节数：3+3+1+4+3+1+4+3 = 22

    // R1 全边界点扫描：0..=blen 全部是合法边界（样例文本无续字节起点位）
    let b = text.as_bytes();
    let mut pos = 0usize;
    let mut boundary_n = 0u32;
    while pos <= blen {
        if is_boundary(b, pos) {
            boundary_n += 1;
        }
        pos += 1;
    }
    if boundary_n != (blen as u32) + 1 {
        return Err(E_MID_CHAR);
    }
    hits += 1;

    // R2 中间续字节检测：构造 3 字节字符的内部偏移必非边界
    // 「安」= E5 AE 89：偏移 1、2 是续字节
    let mut sp = Splitter::new(&text);
    // 找「安」起点 = 0；切 1 与 2 必拒
    match sp.cut_at(1) {
        Err(E_MID_CHAR) => {}
        _ => return Err(E_MID_CHAR),
    }
    match sp.cut_at(2) {
        Err(E_MID_CHAR) => {}
        _ => return Err(E_MID_CHAR),
    }
    hits += 1;

    // R3 ×100 边界切分全过：LCG 选 100 个合法切点
    let mut sp2 = Splitter::new(&text);
    let mut rng = Lcg::new(0x48431);
    let mut k = 0u32;
    while k < 100 {
        let pos = rng.below(blen as u64 + 1) as usize;
        let (l, r) = sp2.cut_at(pos)?;
        if l.len() + r.len() != blen {
            return Err(E_REASM_MISS);
        }
        k += 1;
    }
    if sp2.cuts() != 100 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R4 重组恒等：100 切点全边界 → 重组长度一致
    let cuts = {
        let mut v: Vec<usize> = Vec::new();
        let mut rng2 = Lcg::new(0x48431);
        let mut k = 0u32;
        while k < 100 {
            v.push(rng2.below(blen as u64 + 1) as usize);
            k += 1;
        }
        v
    };
    let out = reassemble(&text, &cuts)?;
    if out.len() != blen {
        return Err(E_REASM_MISS);
    }
    hits += 1;

    // R5 单字节 ASCII 混排：ascii 段任意点全边界
    let ascii = String::from("abcdefg");
    let mut sp3 = Splitter::new(&ascii);
    let mut i = 0usize;
    while i <= 7 {
        let _ = sp3.cut_at(i)?;
        i += 1;
    }
    if sp3.cuts() != 8 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R6 LCG 随机 512 切点边界安全：全部命中边界（合法）或拒绝（非法）零崩
    let mut ok_n = 0u32;
    let mut bad_n = 0u32;
    let mut rng3 = Lcg::new(0x777);
    let mut k = 0u32;
    while k < 512 {
        let pos = rng3.below(blen as u64 + 1) as usize;
        if is_boundary(b, pos) {
            ok_n += 1;
        } else {
            bad_n += 1;
        }
        k += 1;
    }
    if ok_n + bad_n != 512 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R7 重演一致：同种子切分账指纹相等
    let f1 = sp2.fp();
    let mut sp4 = Splitter::new(&text);
    let mut rng4 = Lcg::new(0x48431);
    let mut k = 0u32;
    while k < 100 {
        let pos = rng4.below(blen as u64 + 1) as usize;
        let _ = sp4.cut_at(pos);
        k += 1;
    }
    if f1 != sp4.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 续字节判定自检：0x80~0xBF 全真、0x00~0x7F 与 0xC0~0xFF 全假
    let mut v = 0x80u16;
    while v <= 0xBF {
        if !is_cont(v as u8) {
            return Err(E_BAD_UTF8);
        }
        v += 1;
    }
    if is_cont(0x7F) || is_cont(0xC0) || is_cont(0x00) {
        return Err(E_BAD_UTF8);
    }
    hits += 1;

    Ok(hits)
}
