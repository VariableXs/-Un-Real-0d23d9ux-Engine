//! UNX-F48440 —— E22 批收官印（20 条冻结/累计 132,000 行）
//! 判据原文：UNX-F48440-J1 E22 批收官印为 20 条 (卡号, 行数) 冻结清单：
//! ① 条数恰 20；② 行数累计恰 132,000；③ 卡号连续升序无空洞；
//! ④ 串联 FNV 印与重算一致；⑤ 抽样缺卡注入必须被检出并指认缺号。
//! 判据落地（drill 场景）：R1 条数冻结；R2 行数累计冻结；
//! R3 卡号连续升序；R4 行数配额 sane；R5 印稳定可重算；
//! R6 缺卡注入检出（首/中/尾三点抽样）；R7 重复注入检出；
//! R8 拆印必异（去一条印值即变）；R9 批窗越界拒收；R10 印指纹非零。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDB40;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_COUNT: u16 = SEG_BASE + 1; // 条数不等于 20
const E_SUM: u16 = SEG_BASE + 2; // 行数累计不等于 132,000
const E_GAP: u16 = SEG_BASE + 3; // 卡号空洞/乱序
const E_ROWS: u16 = SEG_BASE + 4; // 行数越 sane 配额
const E_SEAL: u16 = SEG_BASE + 5; // 印不可重算/漂移
const E_INJECT: u16 = SEG_BASE + 6; // 缺卡注入未检出
const E_DUP: u16 = SEG_BASE + 7; // 重复注入未检出
const E_TAMPER: u16 = SEG_BASE + 8; // 拆印未变值
const E_WINDOW: u16 = SEG_BASE + 9; // 卡号越出批窗
const E_FP: u16 = SEG_BASE + 10; // 印指纹为零或漂移

/// 冻结条数。
pub const FROZEN_N: usize = 20;
/// 累计行数冻结值。
pub const FROZEN_SUM: u32 = 132_000;
/// 批窗：E22 段 20 卡号区间。
pub const BATCH_LO: u32 = 48413;
pub const BATCH_HI: u32 = 48432;
/// 单卡行数 sane 配额。
pub const ROWS_LO: u32 = 2_000;
pub const ROWS_HI: u32 = 20_000;

/// 20 条 (卡号, 行数) 冻结清单（行数合计 = 132,000）。
pub const FROZEN: [(u32, u32); 20] = [
    (48413, 6200),
    (48414, 7000),
    (48415, 5800),
    (48416, 7400),
    (48417, 6600),
    (48418, 6600),
    (48419, 6100),
    (48420, 7100),
    (48421, 6400),
    (48422, 6800),
    (48423, 6300),
    (48424, 6900),
    (48425, 5900),
    (48426, 7300),
    (48427, 6500),
    (48428, 6700),
    (48429, 6200),
    (48430, 7000),
    (48431, 6600),
    (48432, 6600),
];

/// FNV-1a 64 位（与本域其余收官印同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 链式 FNV：以给定初值继续递推。
pub fn fnv1a64_from(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    hh
}

/// 确定性 LCG（批内统一系数）。
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

/// 串联收官印：卡号/行数逐条链式压入 FNV。
pub fn seal_of(list: &[(u32, u32)]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < list.len() {
        h = fnv1a64_from(h, &list[i].0.to_le_bytes());
        h = fnv1a64_from(h, &list[i].1.to_le_bytes());
        i += 1;
    }
    h
}

/// 累计行数。
pub fn sum_rows(list: &[(u32, u32)]) -> u32 {
    let mut s = 0u32;
    let mut i = 0usize;
    while i < list.len() {
        s = s.wrapping_add(list[i].1);
        i += 1;
    }
    s
}

/// 卡号连续升序判定（冻结口径：BATCH_LO 起 +1 递增）。
pub fn contiguous(list: &[(u32, u32)]) -> bool {
    let mut i = 0usize;
    while i < list.len() {
        if list[i].0 != BATCH_LO + i as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 缺卡指认：对升序清单找出第一个空洞位（应出现而未出现的卡号）。
pub fn detect_missing(list: &[(u32, u32)]) -> Option<u32> {
    let mut i = 0usize;
    while i < list.len() {
        let expect = BATCH_LO + i as u32;
        if list[i].0 != expect {
            return Some(expect);
        }
        i += 1;
    }
    // 无空洞但比冻结数短：缺的是末位。
    if list.len() < FROZEN_N {
        return Some(BATCH_LO + list.len() as u32);
    }
    None
}

/// 重复指认：相邻两键相同即重复。
pub fn detect_dup(list: &[(u32, u32)]) -> Option<u32> {
    let mut i = 1usize;
    while i < list.len() {
        if list[i].0 == list[i - 1].0 {
            return Some(list[i].0);
        }
        i += 1;
    }
    None
}

/// 去掉指定卡号后的清单拷贝。
pub fn without(list: &[(u32, u32)], card: u32) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = vec![];
    let mut i = 0usize;
    while i < list.len() {
        if list[i].0 != card {
            out.push((list[i].0, list[i].1));
        }
        i += 1;
    }
    out
}

/// 印行（呈现面）：seal=<hex> n=<条数> sum=<行数>。
pub fn seal_line(h: u64, n: usize, sum: u32) -> String {
    let mut s = String::new();
    s.push_str("seal=");
    s.push_str(&h.to_string());
    s.push_str(" n=");
    s.push_str(&(n as u32).to_string());
    s.push_str(" sum=");
    s.push_str(&sum.to_string());
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 条数冻结：恰 20 条。
    if FROZEN.len() != FROZEN_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 行数累计冻结：恰 132,000。
    if sum_rows(&FROZEN) != FROZEN_SUM {
        return Err(E_SUM);
    }
    hits += 1;

    // R3 卡号连续升序：BATCH_LO 起逐位 +1，无空洞无乱序。
    if !contiguous(&FROZEN) {
        return Err(E_GAP);
    }
    hits += 1;

    // R4 行数配额 sane：每条落在 [ROWS_LO, ROWS_HI]。
    let mut i = 0usize;
    while i < FROZEN.len() {
        if FROZEN[i].1 < ROWS_LO || FROZEN[i].1 > ROWS_HI {
            return Err(E_ROWS);
        }
        i += 1;
    }
    hits += 1;

    // R5 印稳定可重算：两次串联印一致，且与逐条重算一致。
    let s1 = seal_of(&FROZEN);
    let s2 = seal_of(&FROZEN);
    if s1 != s2 {
        return Err(E_SEAL);
    }
    let mut re: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < FROZEN.len() {
        re = fnv1a64_from(re, &FROZEN[i].0.to_le_bytes());
        re = fnv1a64_from(re, &FROZEN[i].1.to_le_bytes());
        i += 1;
    }
    if re != s1 {
        return Err(E_SEAL);
    }
    hits += 1;

    // R6 缺卡注入检出：首/中/尾三点抽样，指认缺号逐一正确。
    let mut lcg = Lcg::new(0x48440);
    let picks: [u32; 3] = [
        FROZEN[0].0,
        FROZEN[lcg.below(FROZEN_N as u32) as usize].0,
        FROZEN[FROZEN_N - 1].0,
    ];
    let mut p = 0usize;
    while p < picks.len() {
        let short = without(&FROZEN, picks[p]);
        if short.len() != FROZEN_N - 1 {
            return Err(E_INJECT);
        }
        match detect_missing(&short) {
            Some(m) => {
                if m != picks[p] {
                    return Err(E_INJECT);
                }
            }
            None => return Err(E_INJECT),
        }
        p += 1;
    }
    hits += 1;

    // R7 重复注入检出：相邻重复必须被指认，冻结清单自身零重复。
    let mut adj: Vec<(u32, u32)> = vec![];
    let mut i = 0usize;
    while i < FROZEN.len() {
        adj.push((FROZEN[i].0, FROZEN[i].1));
        if i == 9 {
            adj.push((FROZEN[i].0, FROZEN[i].1));
        }
        i += 1;
    }
    match detect_dup(&adj) {
        Some(d) => {
            if d != FROZEN[9].0 {
                return Err(E_DUP);
            }
        }
        None => return Err(E_DUP),
    }
    if detect_dup(&FROZEN).is_some() {
        return Err(E_DUP);
    }
    if adj.len() != FROZEN_N + 1 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R8 拆印必异：任去一条，印值必须改变（抽首/中/尾三点）。
    let mut p = 0usize;
    while p < picks.len() {
        let short = without(&FROZEN, picks[p]);
        if seal_of(&short) == s1 {
            return Err(E_TAMPER);
        }
        p += 1;
    }
    hits += 1;

    // R9 批窗越界拒收：批窗外卡号不得混入冻结窗。
    let mut m = 0usize;
    while m < FROZEN.len() {
        if FROZEN[m].0 < BATCH_LO || FROZEN[m].0 > BATCH_HI {
            return Err(E_WINDOW);
        }
        m += 1;
    }
    let outside = [(BATCH_LO - 1, 100u32), (BATCH_HI + 1, 100u32)];
    if contiguous(&outside) {
        return Err(E_WINDOW);
    }
    hits += 1;

    // R10 印指纹非零：收官印不得为零值，呈现行三段齐全。
    if s1 == 0 {
        return Err(E_FP);
    }
    let line = seal_line(s1, FROZEN_N, FROZEN_SUM);
    if !has_kw(line.as_bytes(), b"seal=") || !has_kw(line.as_bytes(), b"n=") || !has_kw(line.as_bytes(), b"sum=") {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
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
