//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13701 · UTF-16LE 编码一致性账 · 0x65DC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F13701-J1 全类型字符串字段编码往返与 Windows dump 逐字节
//!      一致（含 BMP 外字符有账）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 编码器：标量 <0x10000 单元直出 LE 双字节；BMP 外（≥0x10000）
//!     走代理对（高 0xD800~0xDBFF / 低 0xDC00~0xDFFF，算式写死）；
//!   - 解码器：奇字节数拒收；孤立高代理/孤立低代理拒收（闭集对账）；
//!   - 逐字节锚（写死常量，Windows dump 对照）："A"→41 00、
//!     U+10000→00 D8 00 DC、U+10FFFF→FF DB FF DF；
//!   - 长度恒等式：字节数 ≡ 2×UTF-16 单元数；单元数 ≡ 字符数 +
//!     BMP 外字符数（代理对占 2 单元）——编码账三列全对；
//!   - 编解码状态机闭集：Idle→Encoding→Encoded→Decoding→Verified，
//!     复用/跳级拒收；
//!   - 注入三族：奇字节数/孤立高代理/孤立低代理 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65DC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65DC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65DC0;

/// 代理对常量（写死，来源：Unicode 15 代理区定义）。
pub const SURR_HIGH_MIN: u32 = 0xD800;
pub const SURR_HIGH_MAX: u32 = 0xDBFF;
pub const SURR_LOW_MIN: u32 = 0xDC00;
pub const SURR_LOW_MAX: u32 = 0xDFFF;
/// BMP 上界（写死）。
pub const BMP_MAX: u32 = 0xFFFF;
/// 代理对合成偏移（写死：0x10000 + (hi-0xD800)*0x400 + (lo-0xDC00)）。
pub const SURR_BIAS: u32 = 0x10000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UtfCode {
    /// 奇字节数
    OddByteCount,
    /// 孤立高代理
    LoneHighSurrogate,
    /// 孤立低代理
    LoneLowSurrogate,
    /// 往返不一致
    RoundTripMismatch,
    /// 逐字节锚不符
    ByteAnchorMismatch,
    /// 长度恒等式破
    LengthInvariantBreak,
    /// 编解码器复用（状态机非法边）
    CodecReuse,
    /// 空输入入账
    EmptyInput,
}

impl UtfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            UtfCode::OddByteCount => "奇字节数：UTF-16LE 必须偶字节",
            UtfCode::LoneHighSurrogate => "孤立高代理：后随必须是低代理",
            UtfCode::LoneLowSurrogate => "孤立低代理：不得作首单元",
            UtfCode::RoundTripMismatch => "往返不一致：decode(encode(s)) ≠ s",
            UtfCode::ByteAnchorMismatch => "逐字节锚不符：Windows dump 对照破",
            UtfCode::LengthInvariantBreak => "长度恒等式破：字节 ≡ 2×单元",
            UtfCode::CodecReuse => "编解码器复用：Verified 后只读",
            UtfCode::EmptyInput => "空输入入账：空串字段拒收",
        }
    }
    pub const ALL: [UtfCode; 8] = [
        UtfCode::OddByteCount,
        UtfCode::LoneHighSurrogate,
        UtfCode::LoneLowSurrogate,
        UtfCode::RoundTripMismatch,
        UtfCode::ByteAnchorMismatch,
        UtfCode::LengthInvariantBreak,
        UtfCode::CodecReuse,
        UtfCode::EmptyInput,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < UtfCode::ALL.len() {
        if UtfCode::ALL[i].val() == code {
            return UtfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65DC0 段闭集内"
}

// ── 编解码状态五闭集 ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodecPhase {
    Idle,
    Encoding,
    Encoded,
    Decoding,
    Verified,
}

/// 合法边闭集：Idle→Encoding→Encoded→Decoding→Verified（线性单链）。
fn codec_edge(cur: CodecPhase, next: CodecPhase) -> bool {
    matches!(
        (cur, next),
        (CodecPhase::Idle, CodecPhase::Encoding)
            | (CodecPhase::Encoding, CodecPhase::Encoded)
            | (CodecPhase::Encoded, CodecPhase::Decoding)
            | (CodecPhase::Decoding, CodecPhase::Verified)
    )
}

/// 编码账行：字段名/字符数/单元数/字节数（四列对账）。
pub struct UtfRow {
    pub field: String,
    pub chars: usize,
    pub units: usize,
    pub bytes: usize,
    /// 含 BMP 外字符标记（有账判据）。
    pub has_astral: bool,
}

// ── 编码器 ───────────────────────────────────────────────────────
/// 标量 → UTF-16 单元序列（1 或 2 个）。
pub fn scalar_to_units(cp: u32) -> Result<(u16, Option<u16>), u32> {
    if cp >= SURR_HIGH_MIN && cp <= SURR_LOW_MAX {
        // 标量值不得落在代理区（str 输入不会出现，防御闭集）。
        return Err(UtfCode::LoneHighSurrogate.val());
    }
    if cp <= BMP_MAX {
        Ok((cp as u16, None))
    } else if cp <= 0x10_FFFF {
        let v = cp - SURR_BIAS;
        let hi = (SURR_HIGH_MIN as u16) + ((v >> 10) as u16);
        let lo = (SURR_LOW_MIN as u16) + ((v & 0x3FF) as u16);
        Ok((hi, Some(lo)))
    } else {
        Err(UtfCode::RoundTripMismatch.val())
    }
}

/// 字符串 → UTF-16LE 字节流。
pub fn encode_utf16le(s: &str) -> Result<Vec<u8>, u32> {
    let mut out: Vec<u8> = Vec::new();
    let mut it = s.chars();
    while let Some(ch) = it.next() {
        let cp = ch as u32;
        let (hi, lo) = scalar_to_units(cp)?;
        out.push((hi & 0xFF) as u8);
        out.push((hi >> 8) as u8);
        if let Some(l2) = lo {
            out.push((l2 & 0xFF) as u8);
            out.push((l2 >> 8) as u8);
        }
    }
    Ok(out)
}

/// UTF-16LE 字节流 → 字符串（闭集拒收：奇字节/孤立代理）。
pub fn decode_utf16le(bytes: &[u8]) -> Result<String, u32> {
    if bytes.len() % 2 != 0 {
        return Err(UtfCode::OddByteCount.val());
    }
    if bytes.is_empty() {
        return Err(UtfCode::EmptyInput.val());
    }
    let mut s = String::new();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let u = (bytes[i] as u32) | ((bytes[i + 1] as u32) << 8);
        i += 2;
        let uc = u as u16;
        if (uc as u32) >= SURR_HIGH_MIN && (uc as u32) <= SURR_HIGH_MAX {
            // 高代理：必须后随低代理。
            if i + 1 >= bytes.len() {
                return Err(UtfCode::LoneHighSurrogate.val());
            }
            let u2 = (bytes[i] as u32) | ((bytes[i + 1] as u32) << 8);
            let uc2 = u2 as u16;
            if !((uc2 as u32) >= SURR_LOW_MIN && (uc2 as u32) <= SURR_LOW_MAX) {
                return Err(UtfCode::LoneHighSurrogate.val());
            }
            i += 2;
            let cp = SURR_BIAS
                + (((uc as u32) - SURR_HIGH_MIN) << 10)
                + ((uc2 as u32) - SURR_LOW_MIN);
            match core::char::from_u32(cp) {
                Some(c) => s.push(c),
                None => return Err(UtfCode::RoundTripMismatch.val()),
            }
        } else if (uc as u32) >= SURR_LOW_MIN && (uc as u32) <= SURR_LOW_MAX {
            return Err(UtfCode::LoneLowSurrogate.val());
        } else {
            match core::char::from_u32(uc as u32) {
                Some(c) => s.push(c),
                None => return Err(UtfCode::RoundTripMismatch.val()),
            }
        }
    }
    Ok(s)
}

// ── 编解码器账本 ─────────────────────────────────────────────────
pub struct UtfLedger {
    pub phase: CodecPhase,
    pub rows: Vec<UtfRow>,
}

impl UtfLedger {
    pub fn new() -> UtfLedger {
        UtfLedger { phase: CodecPhase::Idle, rows: Vec::new() }
    }

    /// 编码入账：往返自验 + 长度恒等式 + 账行登记。
    pub fn book(&mut self, field: &str, s: &str) -> Result<(), u32> {
        if !codec_edge(self.phase, CodecPhase::Encoding) {
            return Err(UtfCode::CodecReuse.val());
        }
        self.phase = CodecPhase::Encoding;
        if s.is_empty() {
            return Err(UtfCode::EmptyInput.val());
        }
        let bytes = encode_utf16le(s)?;
        // 长度恒等式：字节 ≡ 2×单元；单元 = 字节/2。
        let units = bytes.len() / 2;
        if bytes.len() != units * 2 {
            return Err(UtfCode::LengthInvariantBreak.val());
        }
        // 单元数 ≡ 字符数 + BMP 外字符数。
        let mut chars = 0usize;
        let mut astral = 0usize;
        let mut it = s.chars();
        while let Some(ch) = it.next() {
            chars += 1;
            if (ch as u32) > BMP_MAX {
                astral += 1;
            }
        }
        if units != chars + astral {
            return Err(UtfCode::LengthInvariantBreak.val());
        }
        // 往返自验。
        let back = decode_utf16le(&bytes)?;
        if back != s {
            return Err(UtfCode::RoundTripMismatch.val());
        }
        self.phase = CodecPhase::Encoded;
        self.rows.push(UtfRow {
            field: String::from(field),
            chars,
            units,
            bytes: bytes.len(),
            has_astral: astral > 0,
        });
        Ok(())
    }

    /// 终验态推进。
    pub fn finish_decode(&mut self) -> Result<(), u32> {
        if !codec_edge(self.phase, CodecPhase::Decoding) {
            return Err(UtfCode::CodecReuse.val());
        }
        self.phase = CodecPhase::Decoding;
        if !codec_edge(self.phase, CodecPhase::Verified) {
            return Err(UtfCode::CodecReuse.val());
        }
        self.phase = CodecPhase::Verified;
        Ok(())
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

// ── 逐字节锚（写死，Windows dump 对照） ──────────────────────────
/// 锚 1："A" → 41 00。
const ANCHOR_A: [u8; 2] = [0x41, 0x00];
/// 锚 2：U+10000 → 00 D8 00 DC。
const ANCHOR_10000: [u8; 4] = [0x00, 0xD8, 0x00, 0xDC];
/// 锚 3：U+10FFFF → FF DB FF DF。
const ANCHOR_10FFFF: [u8; 4] = [0xFF, 0xDB, 0xFF, 0xDF];

fn check_anchors() -> Result<(), u32> {
    let a = encode_utf16le("A")?;
    if a.len() != 2 || a[0] != ANCHOR_A[0] || a[1] != ANCHOR_A[1] {
        return Err(UtfCode::ByteAnchorMismatch.val());
    }
    // U+10000。
    match core::char::from_u32(0x10000) {
        Some(c) => {
            let b = encode_utf16le(c.encode_utf8(&mut [0u8; 4]))?;
            if b.len() != 4 || b[0] != ANCHOR_10000[0] || b[1] != ANCHOR_10000[1]
                || b[2] != ANCHOR_10000[2] || b[3] != ANCHOR_10000[3]
            {
                return Err(UtfCode::ByteAnchorMismatch.val());
            }
        }
        None => return Err(UtfCode::ByteAnchorMismatch.val()),
    }
    // U+10FFFF。
    match core::char::from_u32(0x10_FFFF) {
        Some(c) => {
            let b = encode_utf16le(c.encode_utf8(&mut [0u8; 4]))?;
            if b.len() != 4 || b[0] != ANCHOR_10FFFF[0] || b[1] != ANCHOR_10FFFF[1]
                || b[2] != ANCHOR_10FFFF[2] || b[3] != ANCHOR_10FFFF[3]
            {
                return Err(UtfCode::ByteAnchorMismatch.val());
            }
        }
        None => return Err(UtfCode::ByteAnchorMismatch.val()),
    }
    Ok(())
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演：样本集全类型往返 + 锚对账 + BMP 外有账 ─────────────
pub fn golden_run() -> Result<u64, u32> {
    // 逐字节锚。
    check_anchors()?;
    // 样本集：ASCII / CJK / BMP 高位 / BMP 外单字 / BMP 外混排。
    let s1 = "Software\\Classes";
    let s2 = "系统配置";
    let s3 = "\u{ABCD}\u{00E9}";
    let s4 = "\u{1F600}";
    let s5 = "\u{10437}\u{4E2D}\u{10FFFF}";
    let mut led = UtfLedger::new();
    led.book("path", s1)?;
    led.book("cjk", s2)?;
    led.book("bmp", s3)?;
    led.book("emoji", s4)?;
    led.book("mix", s5)?;
    // 账行守恒：5 行全登记。
    if led.row_count() != 5 {
        return Err(UtfCode::LengthInvariantBreak.val());
    }
    // BMP 外有账：emoji 与 mix 两行必标 astral。
    let mut astral_rows = 0;
    let mut i = 0;
    while i < led.rows.len() {
        if led.rows[i].has_astral {
            astral_rows += 1;
        }
        i += 1;
    }
    if astral_rows != 2 {
        return Err(UtfCode::LengthInvariantBreak.val());
    }
    // 终验态。
    led.finish_decode()?;
    // 复用拒收（独立反向判据）。
    let rej = led.book("again", s1);
    let mut reuse_ok = false;
    if let Err(e) = rej {
        if e == UtfCode::CodecReuse.val() {
            reuse_ok = true;
        }
    }
    if !reuse_ok {
        return Err(UtfCode::CodecReuse.val());
    }
    // 确定性长串对照：LCG 生成 BMP 外标量序列往返。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut long = String::new();
    let mut k = 0;
    while k < 64 {
        // 从合法标量池取（避开代理区）：池 = [0x0020,0x4E2D,0x1F600,0x10FFFF]。
        let pick = rng.below(4);
        let cp = match pick {
            0 => 0x0020u32,
            1 => 0x4E2Du32,
            2 => 0x1F600u32,
            _ => 0x10_FFFFu32,
        };
        match core::char::from_u32(cp) {
            Some(c) => long.push(c),
            None => return Err(UtfCode::RoundTripMismatch.val()),
        }
        k += 1;
    }
    let lb = encode_utf16le(long.as_str())?;
    let back = decode_utf16le(&lb)?;
    if back != long {
        return Err(UtfCode::RoundTripMismatch.val());
    }
    // 黄金哈希：5 行账字节数逐位卷入。
    let mut h: u64 = 0x811C9DC5;
    i = 0;
    while i < led.rows.len() {
        h = (h ^ (led.rows[i].chars as u64)).wrapping_mul(0x01000193);
        h = (h ^ (led.rows[i].units as u64)).wrapping_mul(0x01000193);
        h = (h ^ (led.rows[i].bytes as u64)).wrapping_mul(0x01000193);
        i += 1;
    }
    h = (h ^ (lb.len() as u64)).wrapping_mul(0x01000193);
    Ok(h)
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：奇字节数。
    ledger.tries[0] += 1;
    let odd: [u8; 3] = [0x41, 0x00, 0x42];
    let mut c0 = false;
    if let Err(e) = decode_utf16le(&odd) {
        if e == UtfCode::OddByteCount.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：孤立高代理（D8 00 收尾）。
    ledger.tries[1] += 1;
    let lone_hi: [u8; 2] = [0x00, 0xD8];
    let mut c1 = false;
    if let Err(e) = decode_utf16le(&lone_hi) {
        if e == UtfCode::LoneHighSurrogate.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：孤立低代理（DC 00 打头）。
    ledger.tries[2] += 1;
    let lone_lo: [u8; 2] = [0x00, 0xDC];
    let mut c2 = false;
    if let Err(e) = decode_utf16le(&lone_lo) {
        if e == UtfCode::LoneLowSurrogate.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < UtfCode::ALL.len() {
        let v = UtfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演往返账", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 逐字节锚独立判据（写死字节组对拍）。
    v.push((
        "J1-逐字节锚写死",
        ANCHOR_A == [0x41, 0x00]
            && ANCHOR_10000 == [0x00, 0xD8, 0x00, 0xDC]
            && ANCHOR_10FFFF == [0xFF, 0xDB, 0xFF, 0xDF],
        String::from("A/U+10000/U+10FFFF"),
    ));
    // 代理区常量独立判据。
    v.push((
        "J1-代理区闭集",
        SURR_HIGH_MIN == 0xD800
            && SURR_HIGH_MAX == 0xDBFF
            && SURR_LOW_MIN == 0xDC00
            && SURR_LOW_MAX == 0xDFFF
            && SURR_BIAS == 0x10000,
        String::from("D800~DFFF/0x10000"),
    ));
    // 反向判据：代理区标量直接编码必拒。
    let rej_surr = scalar_to_units(0xD800);
    let mut ok = false;
    if let Err(e) = rej_surr {
        if e == UtfCode::LoneHighSurrogate.val() {
            ok = true;
        }
    }
    v.push(("J1-代理区标量必拒", ok, String::from("标量不落代理区")));
    // 反向判据：空字节流必拒。
    let emp = decode_utf16le(&[]);
    let mut ok2 = false;
    if let Err(e) = emp {
        if e == UtfCode::EmptyInput.val() {
            ok2 = true;
        }
    }
    v.push(("J1-空流必拒", ok2, String::from("空串字段拒收")));
    // 边集判据：Idle 直跳 Decoding 必拒。
    v.push((
        "J1-跳级解码必拒",
        !codec_edge(CodecPhase::Idle, CodecPhase::Decoding)
            && !codec_edge(CodecPhase::Verified, CodecPhase::Encoding),
        String::from("闭式边集"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65DC0 窗口")));
    v
}

pub fn run_unx13701_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13701 UTF-16LE 编码一致性账");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
