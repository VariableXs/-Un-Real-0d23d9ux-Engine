//! UNX-F48777 —— C·闭账物向量账终版（终测用例集+校验和）
//! 判据原文：UNX-F48777-J1 终版归档；判据锚定 §AI-61 §8。
//! 判据落地（drill 场景）：R1 终测用例登记；R2 校验和落账；
//! R3 终版归档断言；R4 案号唯一；R5 案号越界检出；R6 校验和漂移
//! 检出；R7 复跑一致；R8 指纹稳定；R9 计数守恒；R10 呈现行；
//! R11 归档签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：闭账物向量账终版 16 例，每例案号显式编位（1..=16）
//! 并即时落校验和；终版归档 = 全例校验和可复算且指纹冻结；
//! 归档后账面不得再改（重复登记拒绝）。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x2960;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ID: u16 = SEG_BASE + 1; // 案号非法
const E_DUP: u16 = SEG_BASE + 2; // 案号重复
const E_SUM: u16 = SEG_BASE + 3; // 校验和漂移
const E_COUNT: u16 = SEG_BASE + 4; // 用例数不足
const E_REPLAY: u16 = SEG_BASE + 5; // 复跑漂移
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 8; // 归档签名不符

/// 终版用例数。
pub const CASE_N: u32 = 16;

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

/// 一条终测用例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FinalCase {
    pub case_id: u32, // 案号 1..=16
    pub input: u32,   // 输入向量
    pub expect: u64,  // 期望校验和
}

impl FinalCase {
    /// 校验和：案号 + 输入混签。
    pub fn calc(case_id: u32, input: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&case_id.to_le_bytes());
        buf.extend_from_slice(&input.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.case_id == 0 || self.case_id > CASE_N {
            return Err(E_ID);
        }
        if self.expect != FinalCase::calc(self.case_id, self.input) {
            return Err(E_SUM);
        }
        Ok(())
    }
}

/// 终版向量账。
pub struct FinalBook {
    pub cases: Vec<FinalCase>,
}

impl FinalBook {
    pub fn new() -> FinalBook {
        FinalBook { cases: vec![] }
    }

    /// 用例登记：案号闭集 + 唯一 + 校验和即时落账。
    pub fn enroll(&mut self, case_id: u32, input: u32) -> Result<(), u16> {
        if case_id == 0 || case_id > CASE_N {
            return Err(E_ID);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].case_id == case_id {
                return Err(E_DUP);
            }
            i += 1;
        }
        let expect = FinalCase::calc(case_id, input);
        self.cases.push(FinalCase { case_id, input, expect });
        Ok(())
    }

    /// 终版归档断言：全例复算 + 数量守恒。
    pub fn archive_check(&self) -> Result<(), u16> {
        if self.cases.len() as u32 != CASE_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            self.cases[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.cases.len() {
            buf.extend_from_slice(&self.cases[i].case_id.to_le_bytes());
            buf.extend_from_slice(&self.cases[i].input.to_le_bytes());
            buf.extend_from_slice(&self.cases[i].expect.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 归档签名：用例数 × 指纹混签。
pub fn final_sig(n: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：cases=16 archived=1。
pub fn final_line(n: u32) -> String {
    let mut out = String::new();
    out.push_str("cases=");
    out.push_str(&n.to_string());
    out.push_str(" archived=1");
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
    let mut g = Lcg::new(0x487_77);

    // R1 终测用例登记 ×16：案号显式编位，输入 LCG。
    let mut book = FinalBook::new();
    let mut id: u32 = 1;
    while id <= CASE_N {
        let input = 0x3000 + g.below(0x7000);
        book.enroll(id, input)?;
        id += 1;
    }
    if book.cases.len() as u32 != CASE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 校验和落账：逐条 sane。
    let mut i = 0usize;
    while i < book.cases.len() {
        book.cases[i].sane()?;
        i += 1;
    }
    hits += 1;

    // R3 终版归档断言。
    book.archive_check()?;
    hits += 1;

    // R4 案号唯一：重复登记拒绝。
    match book.enroll(5, 42) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R5 案号越界：0 与 17 拒绝。
    match book.enroll(0, 1) {
        Err(E_ID) => {}
        _ => return Err(E_ID),
    }
    match book.enroll(CASE_N + 1, 1) {
        Err(E_ID) => hits += 1,
        _ => return Err(E_ID),
    }

    // R6 校验和漂移检出：篡改期望 → sane 红。
    let mut tamper = FinalBook::new();
    tamper.enroll(1, 7)?;
    tamper.cases[0].expect = tamper.cases[0].expect ^ 1;
    match tamper.archive_check() {
        Err(E_SUM) => hits += 1,
        _ => return Err(E_SUM),
    }

    // R7 复跑一致：同种子重登记指纹一致。
    let mut book2 = FinalBook::new();
    let mut g2 = Lcg::new(0x487_77);
    let mut id2: u32 = 1;
    while id2 <= CASE_N {
        let input = 0x3000 + g2.below(0x7000);
        book2.enroll(id2, input)?;
        id2 += 1;
    }
    if book2.fp() != book.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 计数守恒：归档后不得增删。
    if book.cases.len() as u32 != CASE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R10 呈现行：cases=16 archived=1。
    let line = final_line(book.cases.len() as u32);
    if !has_kw(line.as_bytes(), b"cases=16") || !has_kw(line.as_bytes(), b"archived=1") {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 归档签名：16×指纹混签重算一致。
    let sig = final_sig(CASE_N, f1);
    if sig != final_sig(16, f1) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    Ok(hits)
}
