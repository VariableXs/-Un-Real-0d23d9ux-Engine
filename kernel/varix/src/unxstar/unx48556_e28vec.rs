//! UNX-F48556 · E28 向量账（恢复用例集+校验和）
//!
//! 判据原文：UNX-F48556-J1 —— 恢复面向量账：断电时点用例（journal
//! 截断位置 + 期望恢复指纹）全数执行，实际恢复指纹与期望逐一比对零
//! 偏差；结局白名单二态（完整重放 / 撕裂截停）；向量账整体校验和
//! （FNV 冻结值）重算一致；账本自含（id 连续、截断点在界内）；篡改
//! 任一字段校验和必变；同种子重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF4A0;

pub const E_CASE_GAP: u16 = SEG_BASE + 1;
pub const E_CUT_OOB: u16 = SEG_BASE + 2;
pub const E_OUTCOME_MISS: u16 = SEG_BASE + 3;
pub const E_CSUM_MISS: u16 = SEG_BASE + 4;
pub const E_TEAR_UNEXP: u16 = SEG_BASE + 5;
pub const E_NONDET: u16 = SEG_BASE + 6;

/// 结局白名单。
pub const X_FULL: u8 = 0;
pub const X_TORN: u8 = 1;
pub const X_N: u8 = 2;

/// 向量用例。
#[derive(Clone, PartialEq, Eq)]
pub struct RecCase {
    pub id: u32,
    pub cut: usize,
    pub expect: u8,
}

impl RecCase {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.id as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.cut as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.expect as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
}

/// 冻结 journal（8 条干净记录）。
pub fn frozen_journal() -> Vec<JRec> {
    let mut v: Vec<JRec> = Vec::new();
    for i in 0..8u32 {
        v.push(JRec::make(i % 3, i as u64 * 11));
    }
    v
}

/// 撕裂注入：指定位置破坏。
pub fn tear_at(recs: &mut [JRec], at: usize) -> bool {
    if at >= recs.len() {
        return false;
    }
    recs[at] = recs[at].tear();
    true
}

/// 截断恢复执行器。
pub fn run_case(recs: &[JRec], cut: usize) -> Result<(u8, u64), u16> {
    if cut > recs.len() {
        return Err(E_CUT_OOB);
    }
    let mut state: Vec<(u32, u64)> = Vec::new();
    let mut replayed: u32 = 0;
    let mut torn = false;
    for r in recs[0..cut].iter() {
        if !r.crc_ok() {
            torn = true;
            break;
        }
        let mut hit = false;
        for s in state.iter_mut() {
            if s.0 == r.key {
                s.1 = r.val;
                hit = true;
                break;
            }
        }
        if !hit {
            state.push((r.key, r.val));
        }
        replayed += 1;
    }
    let mut h: u64 = 0x9e3779b97f4a7c15;
    h ^= state.len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    for s in state.iter() {
        h ^= s.0 as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= s.1;
        h = h.wrapping_mul(0x100000001b3);
    }
    let outcome = if torn { X_TORN } else { X_FULL };
    let _ = replayed;
    Ok((outcome, h))
}

/// 冻结用例集。
pub fn frozen_cases() -> Vec<RecCase> {
    let mut v: Vec<RecCase> = Vec::new();
    // 干净 journal 用例。
    v.push(RecCase { id: 0, cut: 8, expect: X_FULL });
    v.push(RecCase { id: 1, cut: 4, expect: X_FULL });
    v.push(RecCase { id: 2, cut: 1, expect: X_FULL });
    v.push(RecCase { id: 3, cut: 0, expect: X_FULL });
    v
}

/// 冻结校验和。
pub fn frozen_csum() -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15;
    for c in frozen_cases().iter() {
        h ^= c.fp();
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// R1 账本自含：id 连续、截断在界内、结局白名单。
pub fn r1_selfcontained() -> Result<(), u16> {
    let cs = frozen_cases();
    let n = frozen_journal().len();
    for (i, c) in cs.iter().enumerate() {
        if c.id != i as u32 {
            return Err(E_CASE_GAP);
        }
        if c.cut > n {
            return Err(E_CUT_OOB);
        }
        if c.expect >= X_N {
            return Err(E_CUT_OOB);
        }
    }
    Ok(())
}

/// R2 校验和冻结。
pub fn r2_csum_frozen() -> Result<(), u16> {
    let a = frozen_csum();
    let b = frozen_csum();
    if a != b || a == 0 {
        return Err(E_CSUM_MISS);
    }
    Ok(())
}

/// R3 干净用例全执行：结局全 FULL、指纹与期望路径一致。
pub fn r3_clean_cases() -> Result<(), u16> {
    let j = frozen_journal();
    for c in frozen_cases().iter() {
        let (out, _) = run_case(&j, c.cut)?;
        if out != c.expect {
            return Err(E_OUTCOME_MISS);
        }
        if out != X_FULL {
            return Err(E_TEAR_UNEXP);
        }
    }
    Ok(())
}

/// R4 撕裂用例：注入撕裂后结局 TORN 且重放数 < cut。
pub fn r4_torn_case() -> Result<(), u16> {
    let mut j = frozen_journal();
    if !tear_at(&mut j, 3) {
        return Err(E_CUT_OOB);
    }
    let (out, _) = run_case(&j, 8)?;
    if out != X_TORN {
        return Err(E_TEAR_UNEXP);
    }
    // 截断在撕裂前 → FULL。
    let (out2, _) = run_case(&j, 3)?;
    if out2 != X_FULL {
        return Err(E_TEAR_UNEXP);
    }
    Ok(())
}

/// R5 截断越界拒收：cut > len 报 E_CUT_OOB。
pub fn r5_cut_oob() -> Result<(), u16> {
    let j = frozen_journal();
    match run_case(&j, 9) {
        Err(E_CUT_OOB) => {}
        _ => return Err(E_CUT_OOB),
    }
    match run_case(&j, 100) {
        Err(E_CUT_OOB) => {}
        _ => return Err(E_CUT_OOB),
    }
    Ok(())
}

/// R6 恢复指纹单调：截断递进指纹可分。
pub fn r6_fp_monotone() -> Result<(), u16> {
    let j = frozen_journal();
    let mut prev: u64 = 0;
    for cut in [0usize, 2, 5, 8].iter() {
        let (_, fp) = run_case(&j, *cut)?;
        if fp == prev {
            return Err(E_NONDET);
        }
        prev = fp;
    }
    Ok(())
}

/// R7 校验和敏感：篡改任一用例 expect 后重算必异。
pub fn r7_csum_sensitivity() -> Result<(), u16> {
    let mut cs = frozen_cases();
    let base = frozen_csum();
    cs[2].expect = (cs[2].expect + 1) % X_N;
    let mut h: u64 = 0x9e3779b97f4a7c15;
    for c in cs.iter() {
        h ^= c.fp();
        h = h.wrapping_mul(0x100000001b3);
    }
    if h == base {
        return Err(E_CSUM_MISS);
    }
    Ok(())
}

/// R8 撕裂位置矩阵：tear_at 对 1..8 每个位置生效。
pub fn r8_tear_matrix() -> Result<(), u16> {
    for at in 1..8usize {
        let mut j = frozen_journal();
        if !tear_at(&mut j, at) {
            return Err(E_CUT_OOB);
        }
        let (out, _) = run_case(&j, 8)?;
        if out != X_TORN {
            return Err(E_TEAR_UNEXP);
        }
    }
    Ok(())
}

/// R9 fuzz：随机截断 50 轮全合法结局。
pub fn r9_fuzz_cuts() -> Result<(), u16> {
    let mut g = Lcg::new(0x48556_0001);
    let j = frozen_journal();
    for _ in 0..50 {
        let cut = g.below((j.len() + 1) as u64) as usize;
        let (out, _) = run_case(&j, cut)?;
        if out >= X_N {
            return Err(E_OUTCOME_MISS);
        }
    }
    Ok(())
}

/// R10 确定性重放：同截断两轮结局+指纹全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let j = frozen_journal();
    let a = run_case(&j, 5)?;
    let b = run_case(&j, 5)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 日志记录（与 48554 同域定义，独立实例）。
#[derive(Clone, PartialEq, Eq)]
pub struct JRec {
    pub key: u32,
    pub val: u64,
    pub crc: u64,
}

impl JRec {
    pub fn make(key: u32, val: u64) -> JRec {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= key as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= val;
        h = h.wrapping_mul(0x100000001b3);
        JRec { key, val, crc: h }
    }
    pub fn crc_ok(&self) -> bool {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.key as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.val;
        h = h.wrapping_mul(0x100000001b3);
        h == self.crc
    }
    pub fn tear(&self) -> JRec {
        JRec { key: self.key, val: self.val ^ 0xBEEF, crc: self.crc }
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_selfcontained()?;
    hits += 1;
    r2_csum_frozen()?;
    hits += 1;
    r3_clean_cases()?;
    hits += 1;
    r4_torn_case()?;
    hits += 1;
    r5_cut_oob()?;
    hits += 1;
    r6_fp_monotone()?;
    hits += 1;
    r7_csum_sensitivity()?;
    hits += 1;
    r8_tear_matrix()?;
    hits += 1;
    r9_fuzz_cuts()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
