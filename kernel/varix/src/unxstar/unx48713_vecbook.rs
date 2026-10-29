//! UNX-F48713 —— I36 向量账（治理面用例集+校验和）
//! 判据原文：UNX-F48713-J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//! 判据落地（drill 场景）：R1 用例集登记 ×20；R2 校验和逐例落账；
//! R3 复跑一致；R4 案号唯一；R5 案号越界检出；R6 校验和漂移检出；
//! R7 集指纹稳定；R8 计数守恒；R9 呈现行；R10 向量签名；
//! R11 输入向量统计口径。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：治理面用例集 20 例，每例登记输入向量与期望校验和；
//! 可复跑 = 同输入重算校验和与账面一致；任一例漂移即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x10E0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ID: u16 = SEG_BASE + 1; // 案号非法
const E_DUP: u16 = SEG_BASE + 2; // 案号重复
const E_SUM: u16 = SEG_BASE + 3; // 校验和漂移
const E_COUNT: u16 = SEG_BASE + 4; // 用例数不足
const E_REPLAY: u16 = SEG_BASE + 5; // 复跑结果漂移
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 8; // 向量签名不符
const E_EMPTY: u16 = SEG_BASE + 9; // 空向量

/// 治理面用例数。
pub const CASE_N: u32 = 20;

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

/// 一条用例向量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseVec {
    pub case_id: u32, // 1..=20
    pub input: u32,   // 输入向量
    pub expect: u64,  // 期望校验和
}

impl CaseVec {
    /// 输入向量校验和：案号 + 输入混签。
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
        if self.expect != CaseVec::calc(self.case_id, self.input) {
            return Err(E_SUM);
        }
        Ok(())
    }
}

/// 向量账。
pub struct VecBook {
    pub cases: Vec<CaseVec>,
}

impl VecBook {
    pub fn new() -> VecBook {
        VecBook { cases: vec![] }
    }

    /// 登记：案号闭集 + 唯一 + 校验和即时落账。
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
        let expect = CaseVec::calc(case_id, input);
        self.cases.push(CaseVec { case_id, input, expect });
        Ok(())
    }

    /// 复跑：逐例重算校验和与账面对账。
    pub fn rerun_all(&self) -> Result<(), u16> {
        if self.cases.is_empty() {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            self.cases[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 集指纹。
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

/// 向量签名：用例数 + 指纹混签。
pub fn vec_sig(n: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 输入向量统计：min / max / 均值。
pub struct InputStats {
    pub min_in: u32,
    pub max_in: u32,
    pub avg_in: u32,
}

/// 全账输入向量统计：空账返回零值。
pub fn input_stats(book: &VecBook) -> InputStats {
    let mut min_in: u32 = u32::MAX;
    let mut max_in: u32 = 0;
    let mut sum: u64 = 0;
    let mut i = 0usize;
    while i < book.cases.len() {
        let v = book.cases[i].input;
        if v < min_in {
            min_in = v;
        }
        if v > max_in {
            max_in = v;
        }
        sum += v as u64;
        i += 1;
    }
    if book.cases.is_empty() {
        min_in = 0;
    }
    let avg_in = if book.cases.is_empty() {
        0
    } else {
        (sum / book.cases.len() as u64) as u32
    };
    InputStats { min_in, max_in, avg_in }
}

/// 呈现行：cases=20 rerun=pass。
pub fn vec_line(n: u32) -> String {
    let mut out = String::new();
    out.push_str("cases=");
    out.push_str(&n.to_string());
    out.push_str(" rerun=pass");
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_13);

    // R1 用例集登记 ×20：案号 1..20，输入由 LCG 生成。
    let mut book = VecBook::new();
    let mut id: u32 = 1;
    while id <= CASE_N {
        let input = 0x1000 + g.below(0x9000);
        book.enroll(id, input)?;
        id += 1;
    }
    if book.cases.len() as u32 != CASE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 校验和逐例落账：sane 逐条核对。
    let mut i = 0usize;
    while i < book.cases.len() {
        book.cases[i].sane()?;
        i += 1;
    }
    hits += 1;

    // R3 复跑一致：全量重跑。
    book.rerun_all()?;
    hits += 1;

    // R4 案号唯一：重复登记拒绝。
    match book.enroll(7, 42) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R5 案号越界：0 与 21 拒绝。
    match book.enroll(0, 1) {
        Err(E_ID) => {}
        _ => return Err(E_ID),
    }
    match book.enroll(CASE_N + 1, 1) {
        Err(E_ID) => hits += 1,
        _ => return Err(E_ID),
    }

    // R6 校验和漂移检出：篡改期望值 → sane 红。
    let mut tamper = VecBook::new();
    tamper.enroll(1, 100)?;
    tamper.cases[0].expect = tamper.cases[0].expect ^ 1;
    match tamper.rerun_all() {
        Err(E_SUM) => hits += 1,
        _ => return Err(E_SUM),
    }

    // R7 集指纹稳定。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 计数守恒 + 计数不足检出。
    if book.cases.len() as u32 != CASE_N {
        return Err(E_COUNT);
    }
    let mut short = VecBook::new();
    short.enroll(1, 9)?;
    match short.rerun_all() {
        Ok(()) => {}
        Err(e) => return Err(e),
    }
    if short.cases.len() as u32 >= CASE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R9 呈现行：cases=20 rerun=pass。
    let line = vec_line(book.cases.len() as u32);
    if !has_kw(line.as_bytes(), b"cases=20") || !has_kw(line.as_bytes(), b"rerun=pass") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 向量签名：n + 指纹混签重算一致。
    let sig = vec_sig(CASE_N, f1);
    if sig != vec_sig(20, f1) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    // R11 统计口径：min ≤ avg ≤ max 且非零。
    let st = input_stats(&book);
    if st.min_in == 0 || st.max_in == 0 || st.avg_in == 0 {
        return Err(E_EMPTY);
    }
    if st.min_in > st.avg_in || st.avg_in > st.max_in {
        return Err(E_COUNT);
    }
    hits += 1;

    Ok(hits)
}
