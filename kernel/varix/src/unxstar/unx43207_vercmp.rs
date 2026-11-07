//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F43207 版本比较器（语义化版本比较 ×100 对照零漂移） · 0x9B180段
//! ───────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F43207-J1 ×100 对照含边界 40 组零差；
//!      判据锚定 任务书专题一；
//!   ③ 完成时 API/软件查回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 版本闭集：maj/min/patch 三段 u16 + 预发布段号（0=正式，
//!     1..=255 预发布序号，序号小者先）；
//!   - ×100 对照：60 常规对 + 40 边界对（全零/越沿/预发布/同值）
//!     冻结表逐对求值与期望相等，两遍求值相等（零漂移）；
//!   - ×10 注入：解析越段/越窗/短串逐类必抓；
//!   - 双跑一致 + 段位守卫 + 闭集回查；no_std + alloc；零 panic。
//! ═════════════════════════════════════════════════════(0x9B180)══════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x9B180，AI-VX12-1106 批1 12 槽第 7 槽）。
pub const SEG_BASE: u32 = 0x9B180;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerCmpCode {
    /// 版本段越窗
    SegOverflow,
    /// 版本串过短
    TooShort,
    /// 版本串过长
    TooLong,
    /// 版本段数不足三段
    SegShort,
    /// 预发布段越窗
    PreOverflow,
    /// 段值非数字
    NonDigit,
    /// 账满
    LedgerFull,
    /// 未知兜底
    Unknown,
}

impl VerCmpCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                VerCmpCode::SegOverflow => 0x01,
                VerCmpCode::TooShort => 0x02,
                VerCmpCode::TooLong => 0x03,
                VerCmpCode::SegShort => 0x04,
                VerCmpCode::PreOverflow => 0x05,
                VerCmpCode::NonDigit => 0x06,
                VerCmpCode::LedgerFull => 0x07,
                VerCmpCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            VerCmpCode::SegOverflow => "版本段越窗：每段 ≤ 999",
            VerCmpCode::TooShort => "版本串过短：≤ 16 字节",
            VerCmpCode::TooLong => "版本串过长：≤ 16 字节",
            VerCmpCode::SegShort => "版本段数不足：须三段",
            VerCmpCode::PreOverflow => "预发布段越窗：≤ 255",
            VerCmpCode::NonDigit => "段值非数字：段内字节须 0-9",
            VerCmpCode::LedgerFull => "账满：先清账",
            VerCmpCode::Unknown => "未知诊断码：不在 0x9B180 段闭集内",
        }
    }

    pub const ALL: [VerCmpCode; 8] = [
        VerCmpCode::SegOverflow,
        VerCmpCode::TooShort,
        VerCmpCode::TooLong,
        VerCmpCode::SegShort,
        VerCmpCode::PreOverflow,
        VerCmpCode::NonDigit,
        VerCmpCode::LedgerFull,
        VerCmpCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in VerCmpCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    VerCmpCode::Unknown.detail()
}

// ══ [CHUNK-2-END]

// ── 版本与解析 ──────────────────────────────────────────────────
/// 语义化版本（三段 + 预发布序号；0=正式，序号小者先）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemVer {
    pub maj: u16,
    pub min: u16,
    pub pat: u16,
    /// 预发布序号：0=正式，1..=255 预发布。
    pub pre: u8,
}

/// 段值上限。
pub const SEG_CAP: u16 = 999;
/// 版本串长度窗。
pub const STR_MIN: usize = 5;
pub const STR_MAX: usize = 16;

/// 解析 "maj.min.pat" 或 "maj.min.pat-pN"（pN=预发布序号 1..=255）。
pub fn parse_ver(s: &[u8]) -> Result<SemVer, u32> {
    if s.len() < STR_MIN {
        return Err(VerCmpCode::TooShort.val());
    }
    if s.len() > STR_MAX {
        return Err(VerCmpCode::TooLong.val());
    }
    let mut segs: [[u8; 4]; 3] = [[0; 4]; 3];
    let mut seg_lens: [usize; 3] = [0; 3];
    let mut seg_i = 0usize;
    let mut pre: u8 = 0;
    let mut i = 0usize;
    let mut in_pre = false;
    while i < s.len() {
        let c = s[i];
        if c == b'.' {
            if in_pre || seg_i >= 2 {
                return Err(VerCmpCode::SegShort.val());
            }
            seg_i += 1;
        } else if c == b'-' {
            if in_pre || seg_i != 2 {
                return Err(VerCmpCode::SegShort.val());
            }
            in_pre = true;
        } else if c == b'p' {
            // 预发布标记：'p' 后 1-3 位数字（1..=255）。
            if !in_pre {
                return Err(VerCmpCode::NonDigit.val());
            }
            let mut v: u32 = 0;
            let mut n = 0usize;
            while i + 1 < s.len() && s[i + 1] >= b'0' && s[i + 1] <= b'9' && n < 3 {
                v = v * 10 + (s[i + 1] - b'0') as u32;
                i += 1;
                n += 1;
            }
            if n == 0 || v == 0 || v > 255 {
                return Err(VerCmpCode::PreOverflow.val());
            }
            pre = v as u8;
        } else if c >= b'0' && c <= b'9' {
            if in_pre {
                return Err(VerCmpCode::NonDigit.val());
            }
            if seg_lens[seg_i] >= 4 {
                return Err(VerCmpCode::SegOverflow.val());
            }
            segs[seg_i][seg_lens[seg_i]] = c;
            seg_lens[seg_i] += 1;
        } else {
            return Err(VerCmpCode::NonDigit.val());
        }
        i += 1;
    }
    if seg_i != 2 {
        return Err(VerCmpCode::SegShort.val());
    }
    let mut vals = [0u16; 3];
    let mut k = 0usize;
    while k < 3 {
        if seg_lens[k] == 0 {
            return Err(VerCmpCode::SegShort.val());
        }
        let mut v = 0u32;
        let mut d = 0usize;
        while d < seg_lens[k] {
            v = v * 10 + (segs[k][d] - b'0') as u32;
            d += 1;
        }
        if v > SEG_CAP as u32 {
            return Err(VerCmpCode::SegOverflow.val());
        }
        vals[k] = v as u16;
        k += 1;
    }
    Ok(SemVer { maj: vals[0], min: vals[1], pat: vals[2], pre })
}

/// 比较（三段字典序 + 预发布先序：-1 小 / 0 等 / 1 大）。
pub fn cmp_ver(a: &SemVer, b: &SemVer) -> i8 {
    if a.maj != b.maj {
        return if a.maj < b.maj { -1 } else { 1 };
    }
    if a.min != b.min {
        return if a.min < b.min { -1 } else { 1 };
    }
    if a.pat != b.pat {
        return if a.pat < b.pat { -1 } else { 1 };
    }
    if a.pre != b.pre {
        return if a.pre < b.pre { -1 } else { 1 };
    }
    0
}

// ══ [CHUNK-3-END]

// ── ×100 对照（60 常规 + 40 边界，冻结表两遍求值） ──────────────
/// 对照项：(左串, 右串, 期望 -1/0/1)。
pub const CASES: [(&str, &str, i8); 100] = [
    // 60 常规对。
    ("1.0.0", "1.0.0", 0), ("1.0.0", "1.0.1", -1), ("1.0.1", "1.0.0", 1),
    ("1.0.0", "1.1.0", -1), ("1.1.0", "1.0.0", 1), ("1.0.0", "2.0.0", -1),
    ("2.0.0", "1.0.0", 1), ("0.9.9", "1.0.0", -1), ("1.0.0", "0.9.9", 1),
    ("0.0.1", "0.1.0", -1), ("0.1.0", "0.0.1", 1), ("10.0.0", "9.99.99", 1),
    ("1.2.3", "1.2.10", -1), ("1.2.10", "1.2.3", 1), ("1.10.0", "1.9.0", 1),
    ("1.9.0", "1.10.0", -1), ("2.3.4", "2.3.4", 0), ("999.999.999", "999.999.998", 1),
    ("999.999.998", "999.999.999", -1),
    ("0.0.0", "0.0.1", -1), ("100.0.0", "99.999.999", 1), ("1.0.0", "1.0.0", 0),
    ("5.5.5", "5.5.6", -1), ("5.5.6", "5.5.5", 1), ("5.5.5", "5.6.5", -1),
    ("5.6.5", "5.5.5", 1), ("5.5.5", "6.5.5", -1), ("6.5.5", "5.5.5", 1),
    ("1.0.0", "1.0.0", 0), ("1.1.1", "1.1.2", -1), ("2.2.2", "2.2.1", 1),
    ("3.3.3", "3.3.3", 0), ("4.4.4", "4.4.5", -1), ("5.5.5", "5.5.4", 1),
    ("6.6.6", "6.6.6", 0), ("7.7.7", "7.7.8", -1), ("8.8.8", "8.8.7", 1),
    ("9.9.9", "9.9.9", 0), ("1.2.3", "1.2.3", 0), ("1.2.4", "1.2.3", 1),
    ("1.3.0", "1.2.9", 1), ("1.2.9", "1.3.0", -1), ("0.1.0", "0.0.9", 1),
    ("0.0.9", "0.1.0", -1), ("2.0.0", "1.9.9", 1), ("1.9.9", "2.0.0", -1),
    ("3.0.1", "3.0.0", 1), ("3.0.0", "3.0.1", -1), ("4.1.2", "4.1.2", 0),
    ("4.1.2", "4.1.3", -1), ("4.2.0", "4.1.9", 1), ("4.1.9", "4.2.0", -1),
    ("5.0.0", "4.999.999", 1), ("4.999.999", "5.0.0", -1), ("1.0.0", "1.0.0", 0),
    ("12.34.56", "12.34.55", 1), ("12.34.55", "12.34.56", -1), ("7.8.9", "7.8.9", 0),
    ("7.8.10", "7.8.9", 1), ("7.8.9", "7.8.10", -1),
    // 40 边界对（全零/沿值/预发布先序/同值）。
    ("0.0.0", "0.0.0", 0), ("0.0.0", "0.0.1", -1), ("0.0.1", "0.0.0", 1),
    ("0.0.999", "0.1.0", -1), ("0.1.0", "0.0.999", 1), ("0.999.999", "1.0.0", -1),
    ("1.0.0", "0.999.999", 1), ("999.999.999", "999.999.999", 0),
    ("1.0.0-p1", "1.0.0", -1), ("1.0.0", "1.0.0-p1", 1), ("1.0.0-p1", "1.0.0-p2", -1),
    ("1.0.0-p2", "1.0.0-p1", 1), ("1.0.0-p1", "1.0.0-p1", 0),
    ("1.0.0-p9", "1.0.0-p10", -1), ("1.0.0-p10", "1.0.0-p9", 1),
    ("1.0.0-p255", "1.0.0", -1), ("1.0.0", "1.0.0-p255", 1),
    ("0.0.1-p1", "0.0.1-p1", 0), ("0.0.1-p1", "0.0.2", -1), ("0.0.2", "0.0.1-p1", 1),
    ("1.0.0-p1", "1.0.1-p1", -1), ("1.0.1-p1", "1.0.0-p1", 1),
    ("1.0.0-p1", "1.1.0-p255", -1), ("1.1.0-p255", "1.0.0-p1", 1),
    ("2.0.0-p1", "1.255.255-p255", 1), ("1.255.255-p255", "2.0.0-p1", -1),
    ("0.1.0-p1", "0.1.0-p1", 0), ("999.0.0-p1", "999.0.0-p2", -1),
    ("999.0.0-p2", "999.0.0-p1", 1), ("999.999.999-p255", "999.999.999-p255", 0),
    ("1.0.0-p5", "1.0.0-p50", -1), ("1.0.0-p50", "1.0.0-p5", 1),
    ("1.0.0-p100", "1.0.0-p2", 1), ("1.0.0-p2", "1.0.0-p100", -1),
    ("5.4.3-p1", "5.4.3", -1), ("5.4.3", "5.4.3-p1", 1),
    ("5.4.3-p1", "5.4.4-p1", -1), ("5.4.4-p1", "5.4.3-p1", 1),
    ("6.0.0-p3", "6.0.0-p3", 0), ("6.0.0-p3", "6.0.1", -1),
];

/// ×100 两遍求值零漂移。
pub fn compare_hundred() -> (u32, String) {
    let mut ok = 0u32;
    let mut marks = String::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let (l, r, want) = CASES[i];
        let a = parse_ver(l.as_bytes());
        let b = parse_ver(r.as_bytes());
        let got = match (a, b) {
            (Ok(x), Ok(y)) => cmp_ver(&x, &y),
            (Err(e), _) => {
                marks.push_str(&format!("{}L{:X}!", i, e));
                i += 1;
                continue;
            }
            (_, Err(e)) => {
                marks.push_str(&format!("{}R{:X}!", i, e));
                i += 1;
                continue;
            }
        };
        if got == want {
            ok += 1;
        } else {
            marks.push_str(&format!("{}v{}!", i, got));
        }
        i += 1;
    }
    // 第二遍全等（零漂移）。
    let mut ok2 = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        let (l, r, want) = CASES[i];
        if let (Ok(x), Ok(y)) = (parse_ver(l.as_bytes()), parse_ver(r.as_bytes())) {
            if cmp_ver(&x, &y) == want {
                ok2 += 1;
            }
        }
        i += 1;
    }
    if ok2 != ok {
        marks.push_str("drift!");
    }
    (ok, marks)
}

// ══ [CHUNK-4-END]

// ── ×10 注入 ────────────────────────────────────────────────────
pub fn inject_ten() -> (u32, String) {
    let mut caught = 0u32;
    let mut marks = String::new();
    // 1 过短（4 字节）。
    if parse_ver(b"1.0.").err() == Some(VerCmpCode::TooShort.val()) {
        caught += 1;
    } else {
        marks.push_str("1!");
    }
    // 2 过长（17 字节）。
    if parse_ver(b"999.999.999999999.").err() == Some(VerCmpCode::TooLong.val()) {
        caught += 1;
    } else {
        marks.push_str("2!");
    }
    // 3 段值越上限（1000）。
    if parse_ver(b"1000.0.0").err() == Some(VerCmpCode::SegOverflow.val()) {
        caught += 1;
    } else {
        marks.push_str("3!");
    }
    // 4 段数不足（两段）。
    if parse_ver(b"1.0").err() == Some(VerCmpCode::TooShort.val()) {
        caught += 1;
    } else {
        marks.push_str("4!");
    }
    // 5 段空（"1..0"）。
    if parse_ver(b"1..0.0").err() == Some(VerCmpCode::SegShort.val()) {
        caught += 1;
    } else {
        marks.push_str("5!");
    }
    // 6 非数字字符。
    if parse_ver(b"1.a.0").err() == Some(VerCmpCode::NonDigit.val()) {
        caught += 1;
    } else {
        marks.push_str("6!");
    }
    // 7 预发布越窗（p256）。
    if parse_ver(b"1.0.0-p256").err() == Some(VerCmpCode::PreOverflow.val()) {
        caught += 1;
    } else {
        marks.push_str("7!");
    }
    // 8 预发布零号（p0）。
    if parse_ver(b"1.0.0-p0").err() == Some(VerCmpCode::PreOverflow.val()) {
        caught += 1;
    } else {
        marks.push_str("8!");
    }
    // 9 正文段带 p（未进预发布段用 p）。
    if parse_ver(b"1.p0.0").err() == Some(VerCmpCode::NonDigit.val()) {
        caught += 1;
    } else {
        marks.push_str("9!");
    }
    // 10 预发布段带 '.'（形态闭集外）。
    if parse_ver(b"1.0.0-p1.2").err() == Some(VerCmpCode::SegShort.val()) {
        caught += 1;
    } else {
        marks.push_str("10!");
    }
    (caught, marks)
}

// ══ [CHUNK-5-END]

// ── 黄金正演 + 双跑 + 段位守卫 + 检查入口 ───────────────────────
/// 黄金正演：×100 零差 + 预发布先序代表对复验。
pub fn golden_run() -> Result<u32, u32> {
    let (c, _) = compare_hundred();
    if c != 100 {
        return Err(VerCmpCode::SegOverflow.val());
    }
    // 代表对复验：1.0.0-p1 < 1.0.0 < 1.0.1。
    let a = parse_ver(b"1.0.0-p1").map_err(|_| VerCmpCode::NonDigit.val())?;
    let b = parse_ver(b"1.0.0").map_err(|_| VerCmpCode::NonDigit.val())?;
    let c2 = parse_ver(b"1.0.1").map_err(|_| VerCmpCode::NonDigit.val())?;
    if cmp_ver(&a, &b) != -1 || cmp_ver(&b, &c2) != -1 || cmp_ver(&a, &c2) != -1 {
        return Err(VerCmpCode::SegOverflow.val());
    }
    Ok(100)
}

/// 双跑一致。
pub fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && a != 0
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x9B000..=0x9B400).contains(&SEG_BASE) {
        return false;
    }
    for c in VerCmpCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

/// 检查入口（卡 43207）。
pub fn run_unx43207_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F43207 版本比较器");
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    let (c100, mk100) = compare_hundred();
    v.push(("J1-对照×100零差", c100 == 100, format!("c {}/100 {}", c100, mk100)));
    let (ci, mki) = inject_ten();
    v.push(("J2-注入拒×10", ci == 10, format!("c {}/10 {}", ci, mki)));
    // J3：预发布先序三连（p1<p2<正式）。
    let p1 = parse_ver(b"2.0.0-p1").ok();
    let p2 = parse_ver(b"2.0.0-p2").ok();
    let rel = parse_ver(b"2.0.0").ok();
    let chain = match (p1, p2, rel) {
        (Some(a), Some(b), Some(c)) => cmp_ver(&a, &b) == -1 && cmp_ver(&b, &c) == -1,
        _ => false,
    };
    v.push(("J3-预发布先序", chain, String::from("p1<p2<正式")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x9B180 窗口")));
    let closed = {
        let mut ok = true;
        for c in VerCmpCode::ALL.iter() {
            if detail(c.val()) != c.detail() {
                ok = false;
            }
        }
        ok
    };
    v.push(("J6-闭集回查", closed, String::from("detail 全等")));
    for (n, p, d) in v {
        s.add_dyn(n, p, &d);
    }
    s
}

// ══ [CHUNK-6-END]

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_compare_hundred() {
        let (c, mk) = compare_hundred();
        assert_eq!(c, 100, "marks: {}", mk);
    }

    #[test]
    fn test_inject_ten() {
        let (c, mk) = inject_ten();
        assert_eq!(c, 10, "marks: {}", mk);
    }

    #[test]
    fn test_prerelease_order() {
        let a = parse_ver(b"1.0.0-p1").ok();
        let b = parse_ver(b"1.0.0").ok();
        assert!(a.is_some() && b.is_some());
        if let (Some(a), Some(b)) = (a, b) {
            assert_eq!(cmp_ver(&a, &b), -1);
        }
    }

    #[test]
    fn test_dual_run() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x9B180);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in VerCmpCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}

// ══ [END-43207]
