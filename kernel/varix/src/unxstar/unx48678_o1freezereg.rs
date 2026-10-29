//! UNX-F48678 —— I·O1 对标账冻结登记（延迟账格式冻结 mini-ADR 留痕）
//! 判据原文：UNX-F48678-J1 登记在册；判据锚定 附则五。
//! 判据落地（drill 场景）：R1 ADR 在册；R2 格式版本冻结；R3 字段表
//! 冻结；R4 冻结指纹；R5 篡改检出；R6 重复登记拒；R7 查询；
//! R8 指纹稳定；R9 呈现行；R10 段位与登记一致。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：mini-ADR 登记冻结延迟账格式（版本 5、五字段表、
//! 段位锚）；登记在册 = 可查、可复算指纹、可断言段位一致；
//! 版本漂移、字段表漂移、重复登记均拒收。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0C20;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ID: u16 = SEG_BASE + 1; // ADR 编号非法
const E_VER: u16 = SEG_BASE + 2; // 格式版本漂移
const E_FIELD: u16 = SEG_BASE + 3; // 字段表漂移
const E_DUP: u16 = SEG_BASE + 4; // 重复登记
const E_NOTFOUND: u16 = SEG_BASE + 5; // 查无登记
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_TAMPER: u16 = SEG_BASE + 8; // 篡改未检出
const E_SEG: u16 = SEG_BASE + 9; // 段位与登记不一致

/// 冻结的延迟账格式版本。
pub const FMT_VER: u32 = 5;
/// mini-ADR 编号。
pub const ADR_ID: u32 = 8_678;
/// 冻结字段表。
pub const FMT_FIELDS: [&str; 5] = ["seg", "p50", "p95", "p99", "ts_source"];

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

/// 一条 mini-ADR 冻结登记。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AdrEntry {
    pub adr_id: u32,
    pub fmt_ver: u32,
    pub fields: Vec<String>,
    pub seg_base: u16, // 登记锚定的段位
}

impl AdrEntry {
    /// 登记指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.adr_id.to_le_bytes());
        buf.extend_from_slice(&self.fmt_ver.to_le_bytes());
        let mut i = 0usize;
        while i < self.fields.len() {
            buf.extend_from_slice(self.fields[i].as_bytes());
            i += 1;
        }
        buf.extend_from_slice(&self.seg_base.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 字段表与冻结表比对。
    pub fn fields_frozen(&self) -> Result<(), u16> {
        if self.fields.len() != FMT_FIELDS.len() {
            return Err(E_FIELD);
        }
        let mut i = 0usize;
        while i < FMT_FIELDS.len() {
            if self.fields[i] != FMT_FIELDS[i] {
                return Err(E_FIELD);
            }
            i += 1;
        }
        Ok(())
    }
}

/// ADR 登记账。
pub struct AdrLedger {
    pub entries: Vec<AdrEntry>,
}

impl AdrLedger {
    pub fn new() -> AdrLedger {
        AdrLedger { entries: vec![] }
    }

    /// 登记：编号 >0、版本冻结、字段表冻结、编号唯一。
    pub fn register(&mut self, e: AdrEntry) -> Result<(), u16> {
        if e.adr_id == 0 {
            return Err(E_ID);
        }
        if e.fmt_ver != FMT_VER {
            return Err(E_VER);
        }
        e.fields_frozen()?;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].adr_id == e.adr_id {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.entries.push(e);
        Ok(())
    }

    /// 查询。
    pub fn lookup(&self, adr_id: u32) -> Result<&AdrEntry, u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].adr_id == adr_id {
                return Ok(&self.entries[i]);
            }
            i += 1;
        }
        Err(E_NOTFOUND)
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.entries.len() {
            buf.extend_from_slice(&self.entries[i].fp().to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 构造本域 mini-ADR 登记。
pub fn build_entry() -> AdrEntry {
    let mut fields: Vec<String> = vec![];
    let mut i = 0usize;
    while i < FMT_FIELDS.len() {
        fields.push(FMT_FIELDS[i].to_string());
        i += 1;
    }
    AdrEntry { adr_id: ADR_ID, fmt_ver: FMT_VER, fields, seg_base: SEG_BASE }
}

/// 呈现行：adr=<编号> ver=<版本> n=<字段数> seg=<段位>。
pub fn adr_line(e: &AdrEntry) -> String {
    let mut out = String::new();
    out.push_str("adr=");
    out.push_str(&e.adr_id.to_string());
    out.push_str(" ver=");
    out.push_str(&e.fmt_ver.to_string());
    out.push_str(" n=");
    out.push_str(&e.fields.len().to_string());
    out.push_str(" seg=");
    out.push_str(&e.seg_base.to_string());
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

/// u16 十进制串（呈现断言辅助）。
pub fn concat_u16(v: u16) -> String {
    let mut out = String::new();
    out.push_str(&v.to_string());
    out
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_78);

    // R1 ADR 在册。
    let mut led = AdrLedger::new();
    led.register(build_entry())?;
    if led.entries.len() != 1 {
        return Err(E_ID);
    }
    hits += 1;

    // R2 格式版本冻结。
    let e = led.lookup(ADR_ID)?;
    if e.fmt_ver != FMT_VER || FMT_VER != 5 {
        return Err(E_VER);
    }
    hits += 1;

    // R3 字段表冻结：五字段逐一比对。
    e.fields_frozen()?;
    hits += 1;

    // R4 冻结指纹：同账两次一致。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R5 篡改检出：改版本或改字段表 → 登记拒收。
    let mut bad = build_entry();
    bad.fmt_ver = 6;
    match led.register(bad) {
        Err(E_VER) => {}
        _ => return Err(E_VER),
    }
    let mut bad2 = build_entry();
    bad2.fields[2] = "p99x".to_string();
    match led.register(bad2) {
        Err(E_FIELD) => hits += 1,
        _ => return Err(E_FIELD),
    }

    // R6 重复登记拒。
    match led.register(build_entry()) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 查询：在册可查、查无返回红。
    led.lookup(ADR_ID)?;
    match led.lookup(999) {
        Err(E_NOTFOUND) => hits += 1,
        _ => return Err(E_NOTFOUND),
    }

    // R8 指纹稳定（二次）。
    if led.fp() != f1 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：adr=8678 ver=5 n=5。
    let line = adr_line(led.lookup(ADR_ID)?);
    if !has_kw(line.as_bytes(), b"adr=8678") || !has_kw(line.as_bytes(), b"ver=5")
        || !has_kw(line.as_bytes(), b"n=5")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 段位与登记一致：登记 seg_base == 本卡 SEG_BASE。
    let e2 = led.lookup(ADR_ID)?;
    if e2.seg_base != SEG_BASE {
        return Err(E_SEG);
    }
    if !has_kw(line.as_bytes(), concat_u16(SEG_BASE).as_bytes()) {
        return Err(E_SEG);
    }
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
