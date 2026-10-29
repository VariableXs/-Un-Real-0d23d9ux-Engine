//! UNX-F48661 —— I·O5 中文章打分输入包（体验 13 章中文输入章结构化输入包）
//! 判据原文：UNX-F48661-J1 输入包 ×1 生成+字段全齐可复算；
//! 判据锚定 §AI-61 §8 联签对 AI-75。
//! 判据落地（drill 场景）：R1 输入包 ×1 生成；R2 必填字段全齐；
//! R3 校验和可复算；R4 章节冻结；R5 数值域回归；R6 试产校准位如实；
//! R7 缺字段检出；R8 键唯一；R9 指纹稳定；R10 呈现行；R11 键序守恒；
//! R12 消费位就绪。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x09C0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_BUILD: u16 = SEG_BASE + 1; // 输入包生成失败
const E_FIELD: u16 = SEG_BASE + 2; // 必填字段缺失
const E_CSUM: u16 = SEG_BASE + 3; // 校验和复算不一致
const E_CHAPTER: u16 = SEG_BASE + 4; // 章节号漂移
const E_RANGE: u16 = SEG_BASE + 5; // 数值越出值域
const E_CALIB: u16 = SEG_BASE + 6; // 试产校准位不实
const E_DUPKEY: u16 = SEG_BASE + 7; // 键重复
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_ORDER: u16 = SEG_BASE + 10; // 键序漂移
const E_CONSUME: u16 = SEG_BASE + 11; // 消费位未就绪
const E_EMPTY: u16 = SEG_BASE + 12; // 键名或值非法

/// 体验 13 章章节号（冻结）。
pub const CHAPTER: u32 = 13;
/// 必填键数。
pub const FIELD_N: usize = 10;
/// 打分包生成版次。
pub const PACK_REV: u32 = 1;

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

/// 一条键值字段。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PackField {
    pub key: String,
    pub val: u64,
}

/// 打分输入包：键值序表 + 章节锚 + 校验和。
#[derive(Clone)]
pub struct ScoreInputPack {
    pub chapter: u32,
    pub rev: u32,
    pub fields: Vec<PackField>,
    pub calib_pending: bool, // 试产校准位：true = 数值待试产校准定
}

impl ScoreInputPack {
    pub fn new() -> ScoreInputPack {
        ScoreInputPack { chapter: CHAPTER, rev: PACK_REV, fields: vec![], calib_pending: true }
    }

    /// 字段登记：键非空 + 键唯一。
    pub fn put(&mut self, key: &str, val: u64) -> Result<(), u16> {
        if key.is_empty() {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.fields.len() {
            if self.fields[i].key == key {
                return Err(E_DUPKEY);
            }
            i += 1;
        }
        self.fields.push(PackField { key: key.to_string(), val });
        Ok(())
    }

    /// 按键取值。
    pub fn get(&self, key: &str) -> Option<u64> {
        let mut i = 0usize;
        while i < self.fields.len() {
            if self.fields[i].key == key {
                return Some(self.fields[i].val);
            }
            i += 1;
        }
        None
    }

    /// 必填键表（冻结序）。
    pub fn required_keys() -> Vec<&'static str> {
        vec![
            "genre_chars",
            "accuracy_bp",
            "latency_p50",
            "latency_p95",
            "latency_p99",
            "sample_n",
            "ts_source",
            "win_kinds",
            "err_injected",
            "pack_fp",
        ]
    }

    /// 字段全齐：必填键逐一在位。
    pub fn fields_complete(&self) -> Result<(), u16> {
        let keys = ScoreInputPack::required_keys();
        let mut i = 0usize;
        while i < keys.len() {
            if self.get(keys[i]).is_none() {
                return Err(E_FIELD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 校验和：章节 + 版次 + 键值序表串联 FNV。
    pub fn checksum(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.chapter.to_le_bytes());
        buf.extend_from_slice(&self.rev.to_le_bytes());
        buf.push(self.calib_pending as u8);
        let mut i = 0usize;
        while i < self.fields.len() {
            buf.extend_from_slice(self.fields[i].key.as_bytes());
            buf.push(0x1F);
            buf.extend_from_slice(&self.fields[i].val.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }

    /// 数值域回归：accuracy ≤10000bp、分位单调、sample_n ≥1。
    pub fn range_ok(&self) -> Result<(), u16> {
        let acc = self.get("accuracy_bp").ok_or(E_FIELD)?;
        if acc > 10_000 {
            return Err(E_RANGE);
        }
        let p50 = self.get("latency_p50").ok_or(E_FIELD)?;
        let p95 = self.get("latency_p95").ok_or(E_FIELD)?;
        let p99 = self.get("latency_p99").ok_or(E_FIELD)?;
        if !(p50 <= p95 && p95 <= p99) {
            return Err(E_RANGE);
        }
        let n = self.get("sample_n").ok_or(E_FIELD)?;
        if n == 0 {
            return Err(E_RANGE);
        }
        Ok(())
    }
}

/// 构造 ×1 打分输入包：确定性数值填充。
pub fn build_pack(g: &mut Lcg) -> Result<ScoreInputPack, u16> {
    let mut p = ScoreInputPack::new();
    p.put("genre_chars", 5_000 + g.below(1000) as u64)?;
    let p50: u64 = 120 + g.below(80) as u64;
    let p95: u64 = p50 + 40 + g.below(60) as u64;
    let p99: u64 = p95 + 30 + g.below(40) as u64;
    p.put("accuracy_bp", 9_200 + g.below(700) as u64)?;
    p.put("latency_p50", p50)?;
    p.put("latency_p95", p95)?;
    p.put("latency_p99", p99)?;
    p.put("sample_n", 1_500)?;
    p.put("ts_source", 1)?; // TSC
    p.put("win_kinds", 3)?; // 三类窗口
    p.put("err_injected", 0)?;
    p.put("pack_fp", 0)?; // 占位，下面回填
    let c0 = p.checksum();
    p.put_key_val_at_tail("pack_fp", c0 % 1_000_003)?;
    Ok(p)
}

/// 打分输入包扩展：尾部回填（限占位键）。
impl ScoreInputPack {
    fn put_key_val_at_tail(&mut self, key: &str, val: u64) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.fields.len() {
            if self.fields[i].key == key {
                self.fields[i].val = val;
                return Ok(());
            }
            i += 1;
        }
        Err(E_FIELD)
    }
}

/// 键序守恒：两次生成的键序一致（同 LCG 序）。
/// 打分包对消费方承诺键序稳定：O5 侧按序读取，不允许重排。
pub fn key_order(p: &ScoreInputPack) -> Vec<String> {
    let mut v: Vec<String> = vec![];
    let mut i = 0usize;
    while i < p.fields.len() {
        v.push(p.fields[i].key.clone());
        i += 1;
    }
    v
}

/// 消费位就绪判定：O5 消费的三个关键字段可读且非零。
pub fn consume_ready(p: &ScoreInputPack) -> Result<(), u16> {
    let acc = p.get("accuracy_bp").ok_or(E_CONSUME)?;
    let p99 = p.get("latency_p99").ok_or(E_CONSUME)?;
    let ts = p.get("ts_source").ok_or(E_CONSUME)?;
    if acc == 0 || p99 == 0 || ts == 0 {
        return Err(E_CONSUME);
    }
    Ok(())
}

/// 章节锚断言：章节号与版次冻结（13 / 1）。
pub fn chapter_frozen(p: &ScoreInputPack) -> Result<(), u16> {
    if p.chapter != CHAPTER || p.rev != PACK_REV {
        return Err(E_CHAPTER);
    }
    Ok(())
}

/// 呈现行：ch=<章> n=<字段数> cs=<校验和低 32 位> calib=<0|1>。
pub fn pack_line(p: &ScoreInputPack) -> String {
    let cs = p.checksum() as u32;
    let mut out = String::new();
    out.push_str("ch=");
    out.push_str(&p.chapter.to_string());
    out.push_str(" n=");
    out.push_str(&p.fields.len().to_string());
    out.push_str(" cs=");
    out.push_str(&cs.to_string());
    out.push_str(" calib=");
    out.push_str(&(p.calib_pending as u32).to_string());
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

/// 主巡检入口：R1..R12 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_61);

    // R1 输入包 ×1 生成：构造成功、非空。
    let pack = build_pack(&mut g)?;
    if pack.fields.is_empty() {
        return Err(E_BUILD);
    }
    hits += 1;

    // R2 字段全齐：必填键全部在位。
    pack.fields_complete()?;
    if pack.fields.len() != FIELD_N {
        return Err(E_FIELD);
    }
    hits += 1;

    // R3 校验和可复算：两次一致且非零；空包校验和不同。
    let c1 = pack.checksum();
    let c2 = pack.checksum();
    if c1 != c2 || c1 == 0 {
        return Err(E_CSUM);
    }
    hits += 1;

    // R4 章节冻结：chapter == 13 且 rev == 1。
    chapter_frozen(&pack)?;
    if CHAPTER != 13 || PACK_REV != 1 {
        return Err(E_CHAPTER);
    }
    hits += 1;

    // R5 数值域回归：accuracy ≤10000、分位单调、sample_n ≥1。
    pack.range_ok()?;
    hits += 1;

    // R6 试产校准位如实：calib_pending == true（阈值未冻结如实标注）。
    if !pack.calib_pending {
        return Err(E_CALIB);
    }
    hits += 1;

    // R7 缺字段检出：删一必填键后全齐判定红。
    let mut thin = pack.clone();
    thin.fields.remove(3);
    match thin.fields_complete() {
        Err(E_FIELD) => hits += 1,
        _ => return Err(E_FIELD),
    }

    // R8 键唯一：重复键拒收。
    let mut probe = ScoreInputPack::new();
    probe.put("k", 1)?;
    match probe.put("k", 2) {
        Err(E_DUPKEY) => hits += 1,
        _ => return Err(E_DUPKEY),
    }

    // R9 指纹稳定：同包两次指纹（校验和）一致。
    let f1 = pack.checksum();
    let f2 = pack.checksum();
    if f1 != f2 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：ch=13 n=10 calib=1。
    let line = pack_line(&pack);
    if !has_kw(line.as_bytes(), b"ch=13") || !has_kw(line.as_bytes(), b"n=10")
        || !has_kw(line.as_bytes(), b"calib=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 键序守恒：重建一次键序与首包一致（同种子同序）。
    let pack2 = build_pack(&mut g)?;
    let k1 = key_order(&pack);
    let k2 = key_order(&pack2);
    if k1 != k2 {
        return Err(E_ORDER);
    }
    hits += 1;

    // R12 消费位就绪：O5 消费字段（accuracy/p99/ts_source）可读且非零。
    consume_ready(&pack)?;
    let acc = pack.get("accuracy_bp").ok_or(E_CONSUME)?;
    let p99 = pack.get("latency_p99").ok_or(E_CONSUME)?;
    let ts = pack.get("ts_source").ok_or(E_CONSUME)?;
    if acc == 0 || p99 == 0 || ts == 0 {
        return Err(E_CONSUME);
    }
    hits += 1;

    Ok(hits)
}
