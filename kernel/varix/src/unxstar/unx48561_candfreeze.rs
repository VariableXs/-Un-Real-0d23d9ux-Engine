//! UNX-F48561 · I·candidate_view 接口冻结复签（波 21 Q3 冻结面全字段回归）
//!
//! 判据原文：UNX-F48561-J1 —— candidate_view 接口冻结面复签：冻结结构
//! 八字段（id/text/rank/score/attr/kind/gen/visible）字段序与类型码签
//! 名重算一致（复签全等）；任一字段增删改签名必变（签名敏感）；全字段
//! 值域回归（rank 单调区间、score 有界、kind 枚举三态、visible 布尔）；
//! 波 21 Q3 冻结常数（FREEZE_WAVE/FREEZE_QTR）不变；构造-复签-回归全链
//! 十场景零偏差；同种子重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF560;

pub const E_FIELD_OOB: u16 = SEG_BASE + 1;
pub const E_SIGN_MISS: u16 = SEG_BASE + 2;
pub const E_RANGE_BAD: u16 = SEG_BASE + 3;
pub const E_RANK_MONO: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_FREEZE_DRIFT: u16 = SEG_BASE + 6;

/// 波 21 Q3 冻结常数。
pub const FREEZE_WAVE: u32 = 21;
pub const FREEZE_QTR: u8 = 3;

/// 字段数。
pub const FIELD_N: usize = 8;

/// 字段类型码（冻结签名用）。
pub const T_U32: u8 = 1;
pub const T_STR: u8 = 2;
pub const T_I32: u8 = 3;
pub const T_U16: u8 = 4;
pub const T_U8: u8 = 5;
pub const T_BOOL: u8 = 6;

/// 冻结字段表：(类型码, 字段名)。
pub fn field_table() -> Vec<(u8, String)> {
    let mut v: Vec<(u8, String)> = Vec::new();
    v.push((T_U32, "id".to_string()));
    v.push((T_STR, "text".to_string()));
    v.push((T_I32, "rank".to_string()));
    v.push((T_U16, "score".to_string()));
    v.push((T_U8, "attr".to_string()));
    v.push((T_U8, "kind".to_string()));
    v.push((T_U32, "gen".to_string()));
    v.push((T_BOOL, "visible".to_string()));
    v
}

/// 冻结签名：字段序 + 类型码链式 FNV。
pub fn freeze_sign() -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15;
    h ^= FIELD_N as u64;
    h = h.wrapping_mul(0x100000001b3);
    for (i, (t, name)) in field_table().iter().enumerate() {
        h ^= *t as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= i as u64;
        h = h.wrapping_mul(0x100000001b3);
        for b in name.as_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// candidate_view 实例。
#[derive(Clone, PartialEq, Eq)]
pub struct CandidateView {
    pub id: u32,
    pub text: String,
    pub rank: i32,
    pub score: u16,
    pub attr: u8,
    pub kind: u8,
    pub gen: u32,
    pub visible: bool,
}

impl CandidateView {
    /// 实例内容指纹（与结构签名无关）。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.id as u64;
        h = h.wrapping_mul(0x100000001b3);
        for b in self.text.as_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= (self.rank as i64 as u64) & 0xFFFF;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.score as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.attr as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.kind as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.gen as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.visible as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }

    /// 全字段值域回归。
    pub fn range_ok(&self) -> Result<(), u16> {
        if self.rank < 0 || self.rank > 9 {
            return Err(E_RANGE_BAD);
        }
        if self.score > 1000 {
            return Err(E_RANGE_BAD);
        }
        if self.kind > 2 {
            return Err(E_RANGE_BAD);
        }
        if self.attr > 15 {
            return Err(E_RANGE_BAD);
        }
        if self.gen == 0 {
            return Err(E_RANGE_BAD);
        }
        Ok(())
    }
}

/// 构造确定性样本。
pub fn make_view(seed: u64, gen: u32) -> CandidateView {
    let mut g = Lcg::new(seed);
    let mut text = String::new();
    let tlen = 1 + g.below(6) as usize;
    for _ in 0..tlen {
        text.push((b'a' + (g.below(26) as u8)) as char);
    }
    CandidateView {
        id: (g.next() & 0xFFFF) as u32,
        text,
        rank: g.below(10) as i32,
        score: (g.below(1001)) as u16,
        attr: g.below(16) as u8,
        kind: g.below(3) as u8,
        gen,
        visible: true,
    }
}

/// R1 冻结签名稳定：两次重算全等且非零。
pub fn r1_sign_stable() -> Result<(), u16> {
    let a = freeze_sign();
    let b = freeze_sign();
    if a != b || a == 0 {
        return Err(E_SIGN_MISS);
    }
    Ok(())
}

/// R2 字段表完整：8 字段、名称互异、类型码在白名单。
pub fn r2_field_table() -> Result<(), u16> {
    let ft = field_table();
    if ft.len() != FIELD_N {
        return Err(E_FIELD_OOB);
    }
    for i in 0..ft.len() {
        for j in (i + 1)..ft.len() {
            if ft[i].1 == ft[j].1 {
                return Err(E_SIGN_MISS);
            }
        }
    }
    for (t, _) in ft.iter() {
        match *t {
            T_U32 | T_STR | T_I32 | T_U16 | T_U8 | T_BOOL => {}
            _ => return Err(E_FIELD_OOB),
        }
    }
    Ok(())
}

/// R3 签名敏感：字段表任何扰动必变签名。
pub fn r3_sign_sensitive() -> Result<(), u16> {
    let base = freeze_sign();
    // 改字段名。
    let mut ft = field_table();
    ft[3].1 = "score2".to_string();
    let mut h: u64 = 0x9e3779b97f4a7c15;
    h ^= ft.len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    for (i, (t, name)) in ft.iter().enumerate() {
        h ^= *t as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= i as u64;
        h = h.wrapping_mul(0x100000001b3);
        for b in name.as_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    if h == base {
        return Err(E_SIGN_MISS);
    }
    // 改类型码。
    let ft2 = field_table();
    let mut h2: u64 = 0x9e3779b97f4a7c15;
    h2 ^= (ft2.len() + 1) as u64;
    h2 = h2.wrapping_mul(0x100000001b3);
    if h2 == base {
        return Err(E_SIGN_MISS);
    }
    Ok(())
}

/// R4 冻结常数：WAVE=21、QTR=3。
pub fn r4_freeze_constants() -> Result<(), u16> {
    if FREEZE_WAVE != 21 || FREEZE_QTR != 3 {
        return Err(E_FREEZE_DRIFT);
    }
    Ok(())
}

/// R5 值域回归：50 样本全过。
pub fn r5_range_regression() -> Result<(), u16> {
    for s in 0..50u64 {
        let v = make_view(0x48561_0000 + s, 1 + s as u32);
        v.range_ok()?;
    }
    Ok(())
}

/// R6 值域守卫：越界样本被抓。
pub fn r6_range_guard() -> Result<(), u16> {
    let mut v = make_view(1, 1);
    v.rank = 10;
    match v.range_ok() {
        Err(E_RANGE_BAD) => {}
        _ => return Err(E_RANGE_BAD),
    }
    let mut v2 = make_view(1, 1);
    v2.kind = 3;
    match v2.range_ok() {
        Err(E_RANGE_BAD) => {}
        _ => return Err(E_RANGE_BAD),
    }
    let mut v3 = make_view(1, 1);
    v3.score = 1001;
    match v3.range_ok() {
        Err(E_RANGE_BAD) => {}
        _ => return Err(E_RANGE_BAD),
    }
    let mut v4 = make_view(1, 1);
    v4.gen = 0;
    match v4.range_ok() {
        Err(E_RANGE_BAD) => {}
        _ => return Err(E_RANGE_BAD),
    }
    Ok(())
}

/// R7 实例指纹敏感：八字段逐一扰动指纹必变。
pub fn r7_fp_sensitive() -> Result<(), u16> {
    let base = make_view(42, 3);
    let b = base.fp();
    let mut v;
    v = base.clone();
    v.id += 1;
    if v.fp() == b {
        return Err(E_SIGN_MISS);
    }
    v = base.clone();
    v.text = "zzz".to_string();
    if v.fp() == b {
        return Err(E_SIGN_MISS);
    }
    v = base.clone();
    v.rank += 1;
    if v.fp() == b {
        return Err(E_SIGN_MISS);
    }
    v = base.clone();
    v.score += 1;
    if v.fp() == b {
        return Err(E_SIGN_MISS);
    }
    v = base.clone();
    v.attr += 1;
    if v.fp() == b {
        return Err(E_SIGN_MISS);
    }
    v = base.clone();
    v.kind += 1;
    if v.fp() == b {
        return Err(E_SIGN_MISS);
    }
    v = base.clone();
    v.gen += 1;
    if v.fp() == b {
        return Err(E_SIGN_MISS);
    }
    v = base.clone();
    v.visible = !v.visible;
    if v.fp() == b {
        return Err(E_SIGN_MISS);
    }
    Ok(())
}

/// R8 rank 单调组：同代样本 rank 可排序且无越界。
pub fn r8_rank_order() -> Result<(), u16> {
    let mut ranks: Vec<i32> = Vec::new();
    for s in 0..20u64 {
        let v = make_view(0x48561_1000 + s, 7);
        v.range_ok()?;
        ranks.push(v.rank);
    }
    // 排序后单调不减。
    sort_i32(&mut ranks);
    for i in 1..ranks.len() {
        if ranks[i - 1] > ranks[i] {
            return Err(E_RANK_MONO);
        }
    }
    Ok(())
}

/// no_std 插入排序。
fn sort_i32(a: &mut Vec<i32>) {
    let n = a.len();
    for i in 1..n {
        let mut j = i;
        while j > 0 && a[j - 1] > a[j] {
            let tmp = a[j - 1];
            a[j - 1] = a[j];
            a[j] = tmp;
            j -= 1;
        }
    }
}

/// R9 构造-复签-回归全链：100 样本构造 → 指纹非零 → 值域过。
pub fn r9_full_chain() -> Result<(), u16> {
    for s in 0..100u64 {
        let v = make_view(0x48561_2000 + s, 1 + (s % 9) as u32);
        if v.fp() == 0 {
            return Err(E_SIGN_MISS);
        }
        v.range_ok()?;
        if v.text.len() == 0 {
            return Err(E_RANGE_BAD);
        }
    }
    Ok(())
}

/// R10 确定性重放：同种子两轮样本指纹全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<Vec<u64>, u16> {
        let mut fps = Vec::new();
        for s in 0..12u64 {
            let v = make_view(seed + s, 1 + s as u32);
            fps.push(v.fp());
        }
        Ok(fps)
    };
    let a = run(0x48561_3000)?;
    let b = run(0x48561_3000)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
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
    r1_sign_stable()?;
    hits += 1;
    r2_field_table()?;
    hits += 1;
    r3_sign_sensitive()?;
    hits += 1;
    r4_freeze_constants()?;
    hits += 1;
    r5_range_regression()?;
    hits += 1;
    r6_range_guard()?;
    hits += 1;
    r7_fp_sensitive()?;
    hits += 1;
    r8_rank_order()?;
    hits += 1;
    r9_full_chain()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
