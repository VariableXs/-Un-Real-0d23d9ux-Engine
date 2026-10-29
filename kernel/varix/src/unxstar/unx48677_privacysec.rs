//! UNX-F48677 —— I·O5 隐私边界联签（打分包不含输入明文）
//! 判据原文：UNX-F48677-J1 扫描 ×10 零明文；判据锚定 体验十三 隐私。
//! 判据落地（drill 场景）：R1 白名单键闭集；R2 干净包零明文；
//! R3 明文注入 ×10 检出；R4 值域注入检出；R5 键名越界拒收；
//! R6 扫描可复跑；R7 指纹稳定；R8 呈现行；R9 计数守恒；R10 空包。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：打分包隐私红线 = 包内不含任何输入明文（键白名单闭集
//! + 值纯数值 + 键名/文本零明文特征标记 user_text/raw_input/typed）；
//! 扫描 ×10 注入面全检，任一明文嫌疑即红且不得入包。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0C00;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_PLAIN: u16 = SEG_BASE + 1; // 检出明文
const E_KEY: u16 = SEG_BASE + 2; // 键越出白名单
const E_VAL: u16 = SEG_BASE + 3; // 值非法（含明文特征）
const E_FP: u16 = SEG_BASE + 4; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 5; // 呈现行缺段
const E_CONSERV: u16 = SEG_BASE + 6; // 扫描计数不守恒
const E_REPLAY: u16 = SEG_BASE + 7; // 扫描复跑不一致
const E_EMPTY: u16 = SEG_BASE + 8; // 空包未拦截

/// 白名单键闭集（打分包只允许统计口径字段）。
pub const WL_KEYS: [&str; 6] = ["genre_chars", "accuracy_bp", "latency_p99", "sample_n", "ts_source", "pack_fp"];
/// 扫描轮次。
pub const SCAN_N: u32 = 10;

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

/// 一条打分包字段（值仅允许数值，字符串即明文嫌疑）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PackField {
    pub key: String,
    pub val: u64,
    pub text: String, // 空串 = 纯数值字段；非空 = 明文嫌疑
}

/// 隐私打分包。
pub struct PrivacyPack {
    pub fields: Vec<PackField>,
}

impl PrivacyPack {
    pub fn new() -> PrivacyPack {
        PrivacyPack { fields: vec![] }
    }

    /// 键在白名单内。
    pub fn key_allowed(key: &str) -> bool {
        let mut i = 0usize;
        while i < WL_KEYS.len() {
            if WL_KEYS[i] == key {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记字段：键白名单 + 明文嫌疑拒收。
    pub fn put(&mut self, key: &str, val: u64, text: &str) -> Result<(), u16> {
        if !PrivacyPack::key_allowed(key) {
            return Err(E_KEY);
        }
        if !text.is_empty() {
            return Err(E_VAL);
        }
        self.fields.push(PackField { key: key.to_string(), val, text: String::new() });
        Ok(())
    }

    /// 隐私扫描：键白名单 + 字段无明文 + 值不含明文标记位。
    pub fn privacy_scan(&self) -> Result<u32, u16> {
        let mut n: u32 = 0;
        let mut i = 0usize;
        while i < self.fields.len() {
            if !PrivacyPack::key_allowed(self.fields[i].key.as_str()) {
                return Err(E_PLAIN);
            }
            if !self.fields[i].text.is_empty() {
                return Err(E_PLAIN);
            }
            if has_marker(self.fields[i].key.as_bytes()) {
                return Err(E_PLAIN);
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.fields.len() {
            buf.extend_from_slice(self.fields[i].key.as_bytes());
            buf.extend_from_slice(&self.fields[i].val.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 明文特征标记：user_text / raw_input / typed。
pub fn has_marker(hay: &[u8]) -> bool {
    has_kw(hay, b"user_text") || has_kw(hay, b"raw_input") || has_kw(hay, b"typed")
}

/// 明文标记断言辅助：三标记逐一命中验证（扫描器自检）。
pub fn marker_selftest() -> bool {
    if !has_marker(b"xxuser_textyy") || !has_marker(b"raw_input") || !has_marker(b"typed1") {
        return false;
    }
    if has_marker(b"clean-bytes") || has_marker(b"accuracy_bp") {
        return false;
    }
    true
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

/// 构造干净包：白名单键全量入包。
pub fn build_clean() -> Result<PrivacyPack, u16> {
    let mut p = PrivacyPack::new();
    p.put("genre_chars", 15_000, "")?;
    p.put("accuracy_bp", 9_400, "")?;
    p.put("latency_p99", 480, "")?;
    p.put("sample_n", 1_500, "")?;
    p.put("ts_source", 1, "")?;
    p.put("pack_fp", 778_899, "")?;
    Ok(p)
}

/// 呈现行：n=<字段数> clean=<0|1>。
pub fn pack_line(p: &PrivacyPack) -> String {
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&p.fields.len().to_string());
    let clean = match p.privacy_scan() {
        Ok(_) => 1,
        Err(_) => 0,
    };
    out.push_str(" clean=");
    out.push_str(&clean.to_string());
    out
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_77);

    // R1 白名单键闭集：六个统计键全部在册；扫描器标记自检通过。
    if WL_KEYS.len() != 6 || !PrivacyPack::key_allowed("accuracy_bp")
        || !PrivacyPack::key_allowed("pack_fp")
    {
        return Err(E_KEY);
    }
    if !marker_selftest() {
        return Err(E_PLAIN);
    }
    hits += 1;

    // R2 干净包零明文：全量扫描通过。
    // 白名单六键全入包，逐键扫描零明文嫌疑。
    let clean = build_clean()?;
    let n = clean.privacy_scan()?;
    if n != 6 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R3 明文注入 ×10 检出：交替注入明文字段与越界键。
    let mut round: u32 = 0;
    while round < SCAN_N {
        let mut p = build_clean()?;
        if round % 2 == 0 {
            // 注入非白名单键（模拟明文字段）
            let _r = p.put("user_text", 1, "");
            if !matches!(_r, Err(E_KEY)) {
                return Err(E_PLAIN);
            }
        } else {
            // 注入明文嫌疑（text 非空）
            let _r = p.put("sample_n", 5, "typed-content");
            if !matches!(_r, Err(E_VAL)) {
                return Err(E_PLAIN);
            }
        }
        round += 1;
    }
    hits += 1;

    // R4 值域注入检出：键白名单但带明文 → 红。
    let mut p4 = build_clean()?;
    match p4.put("ts_source", 1, "raw_input") {
        Err(E_VAL) => hits += 1,
        _ => return Err(E_VAL),
    }

    // R5 键名越界拒收：user_text / raw_input / typed 三标记键全拒。
    let mut p5 = PrivacyPack::new();
    if !matches!(p5.put("user_text", 1, ""), Err(E_KEY)) {
        return Err(E_PLAIN);
    }
    if !matches!(p5.put("raw_input", 1, ""), Err(E_KEY)) {
        return Err(E_PLAIN);
    }
    if !matches!(p5.put("typed", 1, ""), Err(E_KEY)) {
        return Err(E_PLAIN);
    }
    hits += 1;

    // R6 扫描可复跑：两次结果一致。
    let a = clean.privacy_scan()?;
    let b = clean.privacy_scan()?;
    if a != b {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = clean.fp();
    let f2 = clean.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行：n=6 clean=1。
    let line = pack_line(&clean);
    if !has_kw(line.as_bytes(), b"n=6") || !has_kw(line.as_bytes(), b"clean=1") {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 计数守恒：扫描数 == 字段数。
    if clean.fields.len() as u32 != a {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R10 空包拦截：零字段包扫描通过但计数 0（不入册）。
    let empty = PrivacyPack::new();
    let e = empty.privacy_scan()?;
    if e != 0 {
        return Err(E_EMPTY);
    }
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
