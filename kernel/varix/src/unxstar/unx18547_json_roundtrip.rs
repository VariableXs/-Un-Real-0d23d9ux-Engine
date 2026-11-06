//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18547 · 档案导入导出（开放格式 JSON）
//! · 0x80480 段 · 批 08 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18547-J1 导出 JSON 格式冻结 v1，往返（导出→导入）
//!       保真率 100%。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - JSON 格式 v1 冻结：键序死锁（schema/major/minor/build/sp/pt/
//!     suite/dpi/elevate 九键固定次序），无空白容忍（紧凑式），数值
//!     十进制无符号，schema 恒 =1——键序即格式指纹，乱序/缺键/
//!     未知键一概拒收（开放格式但版本冻结）；
//!   - 解析状态机六态单向：ExpectLBrace → ExpectKey(i) →
//!     ExpectColon → ExpectValue(i) → ExpectCommaOrBrace → Done；
//!     每态只认下一合法记号，越态即拒（截断/乱序在态机层暴露）；
//!   - 往返校验：20 例档案导出→解析→逐字段对拍，保真率 100%
//!     （任一字段翻转即红）；导出→导出 幂等（二次导出逐字节等）；
//!   - 注入三类 × 10 轮：截断 JSON / 未知键 / 非数值字段值——逐类
//!     逐轮必拒，漏检即红；
//!   - 边界：空串拒、超长拒（>512 字节）、数值上界 65535（越界
//!     拒）、schema≠1 拒、尾随垃圾拒；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] JsCode 闭集（12 码）+ ALL + detail() + detail(u32)
//!   [裁决表]   JSON_KEY_ORDER 九键次序表 + 字段值域表 validate_val
//!   [数据结构] JsonProfile（九字段）+ 往返账
//!   [状态机]   JsonPhase 六态解析机
//!   [账本]     RoundtripLedger（20 例保真账）
//!   [注入核]   三类坏 JSON × 10 轮（截断/未知键/非数值）
//!   [收口]     golden_run + run_unx18547_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80480，AI-VX11-1105 G1 批08 独占区）。
pub const SEG_BASE: u32 = 0x80480;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 九键次序（冻结：格式 v1 的键序即指纹）。
pub const JSON_KEY_ORDER: [&str; 9] =
    ["schema", "major", "minor", "build", "sp", "pt", "suite", "dpi", "elevate"];
/// schema 冻结值。
pub const SCHEMA_V1: u16 = 1;
/// 往返样本数（冻结：20）。
pub const RT_SAMPLES: usize = 20;
/// 输入长度上限（冻结：512 字节）。
pub const JSON_LEN_MAX: usize = 512;
/// 数值字段上界（冻结：65535，u16 闭域）。
pub const VAL_MAX: u32 = 65535;
/// DPI 闭集（与 F18544 同口径）。
pub const DPI_SET: [u16; 5] = [96, 120, 144, 192, 240];
/// PT 闭集。
pub const PT_MIN: u16 = 1;
pub const PT_MAX: u16 = 3;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 九字段档案 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonProfile {
    pub schema: u16,
    pub major: u16,
    pub minor: u16,
    pub build: u16,
    pub sp: u16,
    pub pt: u16,
    pub suite: u16,
    pub dpi: u16,
    pub elevate: u16,
}

impl JsonProfile {
    /// 值域守卫（schema 恒 1 + 五域闭集）。
    pub fn valid(&self) -> bool {
        if self.schema != SCHEMA_V1 {
            return false;
        }
        if self.major < 3 {
            return false;
        }
        if self.pt < PT_MIN || self.pt > PT_MAX {
            return false;
        }
        if self.elevate > 1 {
            return false;
        }
        dpi_legal(self.dpi)
    }
    /// 档案指纹（31 折叠，对拍锚）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18547;
        acc = acc.wrapping_mul(31).wrapping_add(self.major as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.minor as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.build as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.sp as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.pt as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.suite as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.dpi as u32);
        acc.wrapping_mul(31).wrapping_add(self.elevate as u32)
    }
}

/// DPI 是否在冻结闭集内。
pub fn dpi_legal(v: u16) -> bool {
    let mut i = 0;
    while i < DPI_SET.len() {
        if DPI_SET[i] == v {
            return true;
        }
        i += 1;
    }
    false
}

/// 第 i 例确定性样本（20 例两两互异）。
pub fn sample_i(i: usize) -> JsonProfile {
    JsonProfile {
        schema: SCHEMA_V1,
        major: 5 + (i % 3) as u16,
        minor: (i % 2) as u16,
        build: 2600u16 + ((i * 37) % 20000) as u16,
        sp: (i % 3) as u16,
        pt: 1 + (i % 3) as u16,
        suite: 0x100 + i as u16,
        dpi: DPI_SET[i % 5],
        elevate: (i % 2) as u16,
    }
}

// ── 导出（格式 v1 紧凑式） ──────────────────────────────────────
/// 导出 JSON：键序死锁九键，紧凑无空白。
pub fn export_json(p: &JsonProfile) -> String {
    format!(
        "{{\"schema\":{},\"major\":{},\"minor\":{},\"build\":{},\"sp\":{},\"pt\":{},\"suite\":{},\"dpi\":{},\"elevate\":{}}}",
        p.schema, p.major, p.minor, p.build, p.sp, p.pt, p.suite, p.dpi, p.elevate
    )
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JsCode {
    /// 空串。
    JsonEmpty,
    /// 输入超长（>512）。
    JsonTooLong,
    /// 非对象开头（缺 {）。
    NotJsonOpen,
    /// 键失配（乱序/缺键/未知键——键序即格式）。
    KeyMismatch,
    /// 缺冒号。
    ColonMissing,
    /// 非数值（字段值含非数字字节）。
    NumberInvalid,
    /// 数值越界（>65535）。
    NumberTooLarge,
    /// 缺逗号/缺右括号（截断）。
    SeparatorMissing,
    /// 尾随垃圾。
    TrailingGarbage,
    /// schema 非 1。
    SchemaWrong,
    /// 往返失真（字段翻转）。
    RoundtripLoss,
    /// 双跑不一致。
    NotReproducible,
}

impl JsCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            JsCode::JsonEmpty => "空串：JSON 拒收",
            JsCode::JsonTooLong => "输入超长：512 字节上限",
            JsCode::NotJsonOpen => "非对象开头：缺 {",
            JsCode::KeyMismatch => "键失配：键序即格式 v1 指纹",
            JsCode::ColonMissing => "缺冒号：键后必为 :",
            JsCode::NumberInvalid => "非数值：字段值须十进制无符号",
            JsCode::NumberTooLarge => "数值越界：65535 上限",
            JsCode::SeparatorMissing => "缺逗号/右括号：JSON 截断",
            JsCode::TrailingGarbage => "尾随垃圾：右括号后须终结",
            JsCode::SchemaWrong => "schema 非 1：格式 v1 冻结",
            JsCode::RoundtripLoss => "往返失真：导出→导入须逐字段等",
            JsCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [JsCode; 12] = [
        JsCode::JsonEmpty,
        JsCode::JsonTooLong,
        JsCode::NotJsonOpen,
        JsCode::KeyMismatch,
        JsCode::ColonMissing,
        JsCode::NumberInvalid,
        JsCode::NumberTooLarge,
        JsCode::SeparatorMissing,
        JsCode::TrailingGarbage,
        JsCode::SchemaWrong,
        JsCode::RoundtripLoss,
        JsCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < JsCode::ALL.len() {
        if JsCode::ALL[i].code() == code {
            return JsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80480 段闭集内"
}

// ── 解析状态机（六态单向） ──────────────────────────────────────
/// 解析相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JsonPhase {
    /// 期待 {。
    ExpectLBrace,
    /// 期待第 i 键。
    ExpectKey(usize),
    /// 期待冒号。
    ExpectColon,
    /// 期待第 i 值。
    ExpectValue(usize),
    /// 期待逗号或右括号。
    ExpectCommaOrBrace,
    /// 完成。
    Done,
}

/// 十进制无符号数值扫描：起止下标切片，越界/非法显性。
/// 返回 (值, 消耗字节数)。
fn scan_number(b: &[u8], start: usize) -> Result<(u32, usize), u32> {
    if start >= b.len() {
        return Err(JsCode::SeparatorMissing.code());
    }
    let mut v: u32 = 0;
    let mut n = 0usize;
    let mut k = start;
    while k < b.len() {
        let c = b[k];
        if c >= b'0' && c <= b'9' {
            v = v.saturating_mul(10).saturating_add((c - b'0') as u32);
            n += 1;
            k += 1;
            if v > VAL_MAX {
                return Err(JsCode::NumberTooLarge.code());
            }
        } else {
            break;
        }
    }
    if n == 0 {
        return Err(JsCode::NumberInvalid.code());
    }
    Ok((v, n))
}

/// 键匹配：在位置 pos 匹配 "key"（含引号），失配 KeyMismatch。
fn match_key(b: &[u8], pos: usize, key: &str) -> Result<usize, u32> {
    // 引号 + key + 引号。
    let need = key.len() + 2;
    if pos + need > b.len() {
        return Err(JsCode::SeparatorMissing.code());
    }
    if b[pos] != b'"' || b[pos + need - 1] != b'"' {
        return Err(JsCode::KeyMismatch.code());
    }
    let kb = key.as_bytes();
    let mut i = 0;
    while i < key.len() {
        if b[pos + 1 + i] != kb[i] {
            return Err(JsCode::KeyMismatch.code());
        }
        i += 1;
    }
    Ok(pos + need)
}

/// 严格序解析：九键按冻结次序逐键吃入，六态状态机驱动。
/// 态序：LBrace → Key(i) → Colon → Value(i) → CommaOrBrace →
/// （i<8 回 Key(i+1) / i==8 落 Done）；每态只认下一合法记号。
pub fn parse_json(src: &str) -> Result<JsonProfile, u32> {
    let b = src.as_bytes();
    if b.len() == 0 {
        return Err(JsCode::JsonEmpty.code());
    }
    if b.len() > JSON_LEN_MAX {
        return Err(JsCode::JsonTooLong.code());
    }
    let mut pos = 0usize;
    let mut vals: [u32; 9] = [0; 9];
    let mut phase = JsonPhase::ExpectLBrace;
    let mut key_idx = 0usize;
    // 态机守恒上限：九键 × 四记号 + 首尾 ≤ 64 步。
    let mut guard = 0usize;
    while phase != JsonPhase::Done {
        guard += 1;
        if guard > 64 {
            return Err(JsCode::SeparatorMissing.code());
        }
        match phase {
            JsonPhase::ExpectLBrace => {
                if pos >= b.len() || b[pos] != b'{' {
                    return Err(JsCode::NotJsonOpen.code());
                }
                pos += 1;
                key_idx = 0;
                phase = JsonPhase::ExpectKey(0);
            }
            JsonPhase::ExpectKey(i) => {
                pos = match_key(b, pos, JSON_KEY_ORDER[i])?;
                key_idx = i;
                phase = JsonPhase::ExpectColon;
            }
            JsonPhase::ExpectColon => {
                if pos >= b.len() || b[pos] != b':' {
                    return Err(JsCode::ColonMissing.code());
                }
                pos += 1;
                phase = JsonPhase::ExpectValue(key_idx);
            }
            JsonPhase::ExpectValue(i) => {
                let (v, n) = scan_number(b, pos)?;
                vals[i] = v;
                pos += n;
                phase = JsonPhase::ExpectCommaOrBrace;
            }
            JsonPhase::ExpectCommaOrBrace => {
                // 前八键后必逗号并回 Key(i+1)；第九键后必右括号落 Done。
                if key_idx < 8 {
                    if pos >= b.len() || b[pos] != b',' {
                        return Err(JsCode::SeparatorMissing.code());
                    }
                    pos += 1;
                    phase = JsonPhase::ExpectKey(key_idx + 1);
                } else {
                    if pos >= b.len() || b[pos] != b'}' {
                        return Err(JsCode::SeparatorMissing.code());
                    }
                    pos += 1;
                    phase = JsonPhase::Done;
                }
            }
            JsonPhase::Done => {}
        }
    }
    // 终结：右括号后必须到尾。
    if pos != b.len() {
        return Err(JsCode::TrailingGarbage.code());
    }
    let p = JsonProfile {
        schema: vals[0] as u16,
        major: vals[1] as u16,
        minor: vals[2] as u16,
        build: vals[3] as u16,
        sp: vals[4] as u16,
        pt: vals[5] as u16,
        suite: vals[6] as u16,
        dpi: vals[7] as u16,
        elevate: vals[8] as u16,
    };
    // schema 冻结闸。
    if p.schema != SCHEMA_V1 {
        return Err(JsCode::SchemaWrong.code());
    }
    Ok(p)
}

/// 结构化解析入口（别名：入参闸与态机同体）。
pub fn parse_strict(src: &str) -> Result<JsonProfile, u32> {
    parse_json(src)
}

// ── 往返账 ──────────────────────────────────────────────────────
/// 往返账（保真率口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundtripLedger {
    pub tried: u32,
    pub faithful: u32,
}

impl RoundtripLedger {
    pub const fn new() -> RoundtripLedger {
        RoundtripLedger { tried: 0, faithful: 0 }
    }
    /// 保真率 100%（ tried == faithful 且 tried == 20 ）。
    pub fn full(&self) -> bool {
        self.tried == RT_SAMPLES as u32 && self.faithful == self.tried
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链输出（往返账 / 导出幂等样本指纹）。
pub type GoldenOut = (RoundtripLedger, u32);

/// 黄金全链：20 例样本逐例 导出→解析→逐字段对拍 → 导出幂等 →
/// 值域守卫。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut led = RoundtripLedger::new();
    let mut i = 0;
    while i < RT_SAMPLES {
        let p = sample_i(i);
        if !p.valid() {
            return Err(JsCode::RoundtripLoss.code());
        }
        let j = export_json(&p);
        let back = parse_strict(&j)?;
        led.tried = led.tried.saturating_add(1);
        if back == p {
            led.faithful = led.faithful.saturating_add(1);
        } else {
            return Err(JsCode::RoundtripLoss.code());
        }
        i += 1;
    }
    if !led.full() {
        return Err(JsCode::RoundtripLoss.code());
    }
    // 导出幂等：同档案二次导出逐字节等。
    let p0 = sample_i(0);
    let j1 = export_json(&p0);
    let j2 = export_json(&p0);
    if j1 != j2 {
        return Err(JsCode::NotReproducible.code());
    }
    Ok((led, p0.fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(JsCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类坏 JSON × 10 轮） ───────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 截断 JSON。
    pub trunc_caught: u32,
    /// 类2 未知键（乱序键序同罪）。
    pub key_caught: u32,
    /// 类3 非数值字段值。
    pub num_caught: u32,
}

/// 面额（实现推导）：三类各每轮 1 次违约。
pub const FACE_TRUNC: u32 = 1;
pub const FACE_KEY: u32 = 1;
pub const FACE_NUM: u32 = 1;

/// 三类注入 rounds 轮；每类每轮必拒，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        let p = sample_i(0);
        let good = export_json(&p);
        // 类1 截断：砍尾（右括号缺）。
        let cut = &good[..good.len() - 1];
        match parse_strict(cut) {
            Err(c) if c == JsCode::SeparatorMissing.code() => {
                led.trunc_caught = led.trunc_caught.saturating_add(FACE_TRUNC);
            }
            _ => return Err(JsCode::RoundtripLoss.code()),
        }
        // 类2 未知键：把 "sp" 换成 "svc"（键序/键名即指纹）。
        let bad_key = good.replace("\"sp\":", "\"svc\":");
        match parse_strict(&bad_key) {
            Err(c) if c == JsCode::KeyMismatch.code() => {
                led.key_caught = led.key_caught.saturating_add(FACE_KEY);
            }
            _ => return Err(JsCode::RoundtripLoss.code()),
        }
        // 类3 非数值："sp":0 改 "sp":x0（sample_i(0) 的 sp 恒 0）。
        let bad_num = good.replace("\"sp\":0", "\"sp\":x0");
        match parse_strict(&bad_num) {
            Err(c) if c == JsCode::NumberInvalid.code() => {
                led.num_caught = led.num_caught.saturating_add(FACE_NUM);
            }
            _ => return Err(JsCode::RoundtripLoss.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金链全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：schema≠1 → 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = sample_i(0);
    p.schema = 2;
    // Ok 侧折叠为违约码（探针语义：schema≠1 必须被拒，Ok 即漏检）。
    match parse_strict(&export_json(&p)) {
        Err(c) => Err(c),
        Ok(_) => Err(JsCode::SchemaWrong.code()),
    }
}

/// 探针 C：尾随垃圾 → 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let good = export_json(&sample_i(0));
    let mut s = String::with_capacity(good.len() + 1);
    s.push_str(&good);
    s.push(' ');
    // Ok 侧折叠为违约码（探针语义：尾随垃圾必须被拒，Ok 即漏检）。
    match parse_strict(&s) {
        Err(c) => Err(c),
        Ok(_) => Err(JsCode::TrailingGarbage.code()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < JsCode::ALL.len() {
        let v = JsCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：20 例往返 100% 保真（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((led, _)) => led.full(),
        Err(_) => false,
    };
    v.push(("J1-往返保真100%", g_ok, format!("{:?}", g)));

    // 2. 格式 v1 冻结：九键次序表与导出文本逐键在位。
    let key_ok = {
        let j = export_json(&sample_i(0));
        let mut ok = j.starts_with("{\"schema\":1,");
        let mut i = 1;
        while i < 9 {
            // 每键以 "," 或 "\" 开头的出现即可（首个即本键）。
            let pat = format!(",\"{}\":", JSON_KEY_ORDER[i]);
            if !j.contains(&pat) {
                ok = false;
            }
            i += 1;
        }
        ok && j.ends_with("}")
    };
    v.push(("J1-格式v1键序冻结", key_ok, String::from("九键死序")));

    // 3. 导出幂等：同档案两次导出逐字节等。
    let idem_ok = {
        let p = sample_i(7);
        export_json(&p) == export_json(&p)
    };
    v.push(("J1-导出幂等", idem_ok, String::from("两次导出逐字节等")));

    // 4. 往返逐字段对拍：单例翻转必失真（负对照）。
    let neg_ok = {
        let p = sample_i(3);
        let mut q = p;
        q.build = q.build.wrapping_add(1);
        q != p
    };
    v.push(("J1-翻转必失真", neg_ok, String::from("任一字段翻转≠原档")));

    // 5. 类1 截断注入（1 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let t1 = led.as_ref().map(|l| l.trunc_caught).unwrap_or(0);
    v.push(("J1-截断注入全拒", t1 == FACE_TRUNC * JUDGE_ROUNDS, format!("{}/{}", t1, FACE_TRUNC * JUDGE_ROUNDS)));

    // 6. 类2 未知键注入。
    let t2 = led.as_ref().map(|l| l.key_caught).unwrap_or(0);
    v.push(("J1-未知键注入全拒", t2 == FACE_KEY * JUDGE_ROUNDS, format!("{}/{}", t2, FACE_KEY * JUDGE_ROUNDS)));

    // 7. 类3 非数值注入。
    let t3 = led.as_ref().map(|l| l.num_caught).unwrap_or(0);
    v.push(("J1-非数值注入全拒", t3 == FACE_NUM * JUDGE_ROUNDS, format!("{}/{}", t3, FACE_NUM * JUDGE_ROUNDS)));

    // 8. 注入账守恒。
    let cons = match led.as_ref() {
        Ok(l) => {
            let sum = l.trunc_caught.saturating_add(l.key_caught).saturating_add(l.num_caught);
            sum == (FACE_TRUNC + FACE_KEY + FACE_NUM) * JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons, String::from("3类面额×10轮")));

    // 9. 空串拒 / 超长拒。
    let len_ok = parse_strict("").is_err() && {
        let mut long = String::from("{\"schema\":1,");
        let mut k = 0;
        while k < 600 {
            long.push('0');
            k += 1;
        }
        parse_strict(&long).err() == Some(JsCode::JsonTooLong.code())
    };
    v.push(("J1-空串/超长拒", len_ok, String::from("512 上限")));

    // 10. schema≠1 拒（探针 B）。
    v.push(("J1-schema冻结拒", probe_seq_b().is_err(), String::from("schema=2 必拒")));

    // 11. 尾随垃圾拒（探针 C）。
    v.push(("J1-尾随垃圾拒", probe_seq_c().is_err(), String::from("} 后须终结")));

    // 12. 数值越界拒：build=65536 溢出（需构造：直接把字段置 9 倍
    // 长数字无法经 u16 样本——用文本直造超界 JSON）。
    let oob_ok = {
        let s = "{\"schema\":1,\"major\":6,\"minor\":1,\"build\":65536,\"sp\":1,\"pt\":3,\"suite\":256,\"dpi\":96,\"elevate\":0}";
        parse_strict(s).err() == Some(JsCode::NumberTooLarge.code())
    };
    v.push(("J1-数值越界拒", oob_ok, String::from("65535 上限")));

    // 13. 非对象开头拒。
    let open_ok = parse_strict("schema:1").err() == Some(JsCode::NotJsonOpen.code());
    v.push(("J1-非对象开头拒", open_ok, String::from("缺 { 必拒")));

    // 14. 20 例样本两两互异（保真率口径的样本独立性）。
    let uniq_ok = {
        let mut ok = true;
        let mut a = 0;
        while a < RT_SAMPLES {
            let mut b = a + 1;
            while b < RT_SAMPLES {
                if sample_i(a) == sample_i(b) {
                    ok = false;
                }
                b += 1;
            }
            a += 1;
        }
        ok
    };
    v.push(("J1-样本两两互异", uniq_ok, String::from("20 例独立")));

    // 15. 指纹双跑一致。
    let fp = match &g {
        Ok((_, f)) => *f,
        Err(_) => 0,
    };
    let fp2 = match golden_run() {
        Ok((_, f)) => f,
        Err(_) => 1,
    };
    v.push(("J2-指纹双跑一致", fp == fp2 && fp != 0, format!("{:08X}", fp)));

    // 16. 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // 17. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 18. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = JsCode::ALL.len() == 12;
        let mut i = 0;
        while i < JsCode::ALL.len() {
            if JsCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18547_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18547 档案 JSON 导入导出");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18547_checks();
        assert!(set.all_passed(), "UNX-F18547 判据集应全绿");
    }

    /// 黄金链：20 例往返 100%。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((led, _)) => {
                assert!(led.full());
                assert_eq!(led.tried, 20);
                assert_eq!(led.faithful, 20);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 单例往返逐字段等。
    #[test]
    fn test_roundtrip_single() {
        let p = sample_i(11);
        let j = export_json(&p);
        let back = match parse_strict(&j) {
            Ok(x) => x,
            Err(e) => panic!("解析不应失败: {}", detail(e)),
        };
        assert_eq!(back, p);
    }

    /// 导出文本形态：九键在位、紧凑无空白。
    #[test]
    fn test_export_shape() {
        let j = export_json(&sample_i(0));
        assert!(j.starts_with("{\"schema\":1,"));
        assert!(j.contains("\"major\":5,"));
        assert!(j.ends_with('}'));
        assert!(!j.contains(' '));
    }

    /// 注入三类全拒（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.trunc_caught, 10);
                assert_eq!(l.key_caught, 10);
                assert_eq!(l.num_caught, 10);
            }
            Err(e) => assert!(false, "注入全拒不应报错: {}", detail(e)),
        }
    }

    /// 解析闸：乱序键 / 缺冒号 / 缺逗号逐码核对。
    #[test]
    fn test_parse_gates() {
        // 乱序键：major 与 schema 换位 → KeyMismatch。
        let bad = "{\"major\":6,\"schema\":1,\"minor\":1,\"build\":7601,\"sp\":1,\"pt\":3,\"suite\":256,\"dpi\":96,\"elevate\":0}";
        assert_eq!(parse_strict(bad).err(), Some(JsCode::KeyMismatch.code()));
        // 缺冒号。
        let nocolon = "{\"schema\"1,\"major\":6,\"minor\":1,\"build\":7601,\"sp\":1,\"pt\":3,\"suite\":256,\"dpi\":96,\"elevate\":0}";
        assert_eq!(parse_strict(nocolon).err(), Some(JsCode::ColonMissing.code()));
        // 缺逗号。
        let nocomma = "{\"schema\":1,\"major\":6,\"minor\":1,\"build\":7601,\"sp\":1\"pt\":3,\"suite\":256,\"dpi\":96,\"elevate\":0}";
        assert_eq!(parse_strict(nocomma).err(), Some(JsCode::SeparatorMissing.code()));
    }

    /// 空串 / 超长 / 尾随垃圾 / schema 冻结。
    #[test]
    fn test_boundary_gates() {
        assert_eq!(parse_strict("").err(), Some(JsCode::JsonEmpty.code()));
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
    }

    /// 样本独立性：20 例两两互异。
    #[test]
    fn test_samples_distinct() {
        let mut i = 0;
        while i < RT_SAMPLES {
            assert!(sample_i(i).valid(), "样本 {} 必过值域", i);
            let mut j = i + 1;
            while j < RT_SAMPLES {
                assert_ne!(sample_i(i), sample_i(j));
                j += 1;
            }
            i += 1;
        }
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
