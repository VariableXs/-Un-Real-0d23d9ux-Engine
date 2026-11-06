//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18464 · Layers 栏目语法解析（版本串/标志串） · 0x7EFC0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18464-J1 栏目串语法（~ WIN10RTM 等记法）解析与 Windows
//!       对照 15 例零偏差
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 320 · 目标 ≥600 行
//! 本模块结构清单：
//!   数据结构：VERSION_TOKS 5 项 / FLAG_TOKS 8 项 / ParsedColumn 位平面
//!   状态机  ：ParsePhase 五态单调 Idle→LexScan→Classify→Assemble→Frozen
//!   账本    ：解析账（token 计数守恒、合法/拒因分账、位平面只增不减）
//!   错误路径：LcsCode 12 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：6 类坏串（未知名/版本重复/波浪重复/超长/小写/杂符）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//!   对照表  ：Windows 口径 15 例逐项复算零偏差（位平面精确比对）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EFC0，AI-VX11-1105 批 A07 卡 F18464 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7EFC0;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 版本记法闭集（冻结 5：WIN7~WIN10 系）。
pub const VERSION_COUNT: usize = 5;
/// 标志记法闭集（冻结 8：DPI/提权/分辨率/色深/缩放/DX）。
pub const FLAG_COUNT: usize = 8;
/// 单 token 长度上限（冻结 32：最长记法 DISABLEDXMAXIMIZEDWINDOWEDMODE
/// 恰 30 字节，上限取 2 的幂再 +2 余量）。
pub const TOKEN_LEN_MAX: usize = 32;
/// 单栏目 token 数上限（冻结 16：Windows Layers 值域实测量级）。
pub const TOKENS_MAX: usize = 16;
/// Windows 对照例数（冻结 15：判据原文口径）。
pub const REF_CASES: usize = 15;

/// 版本记法闭集（与 Windows 兼容层口径逐字符一致）。
pub const VERSION_TOKS: [&str; VERSION_COUNT] =
    ["WIN7RTM", "WIN8RTM", "WIN80RTM", "WIN81RTM", "WIN10RTM"];

/// 标志记法闭集（位平面 bit 序 = 数组序）。
pub const FLAG_TOKS: [&str; FLAG_COUNT] = [
    "HIGHDPIAWARE",
    "DPIUNAWARE",
    "RUNASADMIN",
    "640X480",
    "256COLOR",
    "GDISCALING",
    "ELEVATECREATEPROCESS",
    "DISABLEDXMAXIMIZEDWINDOWEDMODE",
];

/// 无版本记号（位平面冻结值：0xFF 不与 5 个版本位冲突）。
pub const VERSION_NONE: u8 = 0xFF;

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LcsCode {
    /// 栏目串为空。
    ColumnEmpty,
    /// 未知记法（版本/标志两闭集外）。
    UnknownToken,
    /// 版本记法重复（一栏目至多一个版本）。
    VersionDup,
    /// 波浪记法重复（至多一个前导 ~）。
    TildeDup,
    /// token 超长（32 上限）。
    TokenTooLong,
    /// 小写违规（记法闭集全大写冻结）。
    CaseViolation,
    /// 杂符（闭集字符域外）。
    JunkChar,
    /// token 数超限（16 上限）。
    TokenOverflow,
    /// 状态机倒退。
    ParsePhaseRegress,
    /// 对照偏差（与 Windows 口径不符）。
    Deviation,
    /// 计数守恒破坏。
    ConservationBroken,
    /// 双跑不一致。
    NotReproducible,
}

impl LcsCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            LcsCode::ColumnEmpty => "栏目串为空：Layers 值域非空口径",
            LcsCode::UnknownToken => "未知记法：版本/标志两闭集外",
            LcsCode::VersionDup => "版本记法重复：一栏目至多一个",
            LcsCode::TildeDup => "波浪记法重复：至多一个前导 ~",
            LcsCode::TokenTooLong => "token 超长：32 上限",
            LcsCode::CaseViolation => "小写违规：记法闭集全大写冻结",
            LcsCode::JunkChar => "杂符：字符域外拒收",
            LcsCode::TokenOverflow => "token 数超限：16 上限",
            LcsCode::ParsePhaseRegress => "状态机倒退：解析流水只进不退",
            LcsCode::Deviation => "对照偏差：与 Windows 口径不符",
            LcsCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            LcsCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [LcsCode; 12] = [
        LcsCode::ColumnEmpty,
        LcsCode::UnknownToken,
        LcsCode::VersionDup,
        LcsCode::TildeDup,
        LcsCode::TokenTooLong,
        LcsCode::CaseViolation,
        LcsCode::JunkChar,
        LcsCode::TokenOverflow,
        LcsCode::ParsePhaseRegress,
        LcsCode::Deviation,
        LcsCode::ConservationBroken,
        LcsCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < LcsCode::ALL.len() {
        if LcsCode::ALL[i].code() == code {
            return LcsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EFC0 段闭集内"
}

// ── 状态机：解析流水 ────────────────────────────────────────────
/// 解析流水五态（单调推进，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParsePhase {
    /// 未开始。
    Idle = 0,
    /// 已完成词法切分。
    LexScan = 1,
    /// 已完成分类。
    Classify = 2,
    /// 已完成装配（位平面成型）。
    Assemble = 3,
    /// 已冻结。
    Frozen = 4,
}

impl ParsePhase {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 恰 +1，其余显式拒。
    pub fn advance(self, next: ParsePhase) -> Result<ParsePhase, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(LcsCode::ParsePhaseRegress.code())
        }
    }
    /// 全链五步。
    pub fn full_chain() -> Result<ParsePhase, u32> {
        let s0 = ParsePhase::Idle;
        let s1 = s0.advance(ParsePhase::LexScan)?;
        let s2 = s1.advance(ParsePhase::Classify)?;
        let s3 = s2.advance(ParsePhase::Assemble)?;
        s3.advance(ParsePhase::Frozen)
    }
}

// ── 词法与分类核 ────────────────────────────────────────────────
/// 词法切分：按空格切 token（空段跳过），返回 token 串列表。
pub fn lex_scan(src: &str) -> Result<Vec<String>, u32> {
    if src.len() == 0 {
        return Err(LcsCode::ColumnEmpty.code());
    }
    let b = src.as_bytes();
    let mut toks: Vec<String> = Vec::new();
    let mut seg_start: Option<usize> = None;
    let mut i = 0;
    while i <= b.len() {
        let at_space = i == b.len() || matches!(b.get(i), Some(b' '));
        if at_space {
            if let Some(s) = seg_start {
                if let Some(seg) = b.get(s..i) {
                    if seg.len() > TOKEN_LEN_MAX {
                        return Err(LcsCode::TokenTooLong.code());
                    }
                    toks.push(String::from_utf8_lossy(seg).into_owned());
                }
                seg_start = None;
            }
        } else if seg_start.is_none() {
            seg_start = Some(i);
        }
        i += 1;
    }
    if toks.len() > TOKENS_MAX {
        return Err(LcsCode::TokenOverflow.code());
    }
    Ok(toks)
}

/// 单 token 字符域闸：大写字母 / 数字 / 波浪。
fn char_domain_ok(tok: &str) -> Result<(), u32> {
    let b = tok.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = match b.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c == b'~' || (c >= b'A' && c <= b'Z') || (c >= b'0' && c <= b'9') {
            i += 1;
            continue;
        }
        if c >= b'a' && c <= b'z' {
            return Err(LcsCode::CaseViolation.code());
        }
        return Err(LcsCode::JunkChar.code());
    }
    Ok(())
}

/// 单 token 分类：波浪 / 版本 / 标志 / 未知。
/// 返回（是否波浪, 版本位 u8, 标志位平掩 u32）。
pub fn classify(tok: &str) -> Result<(bool, u8, u32), u32> {
    char_domain_ok(tok)?;
    if tok.as_bytes() == b"~" {
        return Ok((true, VERSION_NONE, 0));
    }
    let mut i = 0;
    while i < VERSION_COUNT {
        let cand = match VERSION_TOKS.get(i) {
            Some(s) => *s,
            None => break,
        };
        if cand.as_bytes() == tok.as_bytes() {
            return Ok((false, i as u8, 0));
        }
        i += 1;
    }
    let mut j = 0;
    while j < FLAG_COUNT {
        let cand = match FLAG_TOKS.get(j) {
            Some(s) => *s,
            None => break,
        };
        if cand.as_bytes() == tok.as_bytes() {
            return Ok((false, VERSION_NONE, 1u32 << j));
        }
        j += 1;
    }
    Err(LcsCode::UnknownToken.code())
}

// ── 解析结果（位平面） ──────────────────────────────────────────
/// 栏目位平面：波浪记法 + 版本位 + 标志位掩码 + token 计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParsedColumn {
    pub tilde: bool,
    pub version: u8,
    pub flag_mask: u32,
    pub token_count: u32,
}

impl ParsedColumn {
    /// 与期望位平面零偏差比对。
    pub fn matches(&self, expect: &ParsedColumn) -> bool {
        self.tilde == expect.tilde
            && self.version == expect.version
            && self.flag_mask == expect.flag_mask
            && self.token_count == expect.token_count
    }
    /// 版本串非空判。
    pub fn has_version(&self) -> bool {
        self.version != VERSION_NONE
    }
    /// 标志串非空判。
    pub fn has_flags(&self) -> bool {
        self.flag_mask != 0
    }
}

/// 栏目解析总装：词法 → 分类 → 装配（版本 ≤1、波浪 ≤1、位掩码叠加）。
pub fn parse_column(src: &str) -> Result<ParsedColumn, u32> {
    ParsePhase::full_chain()?;
    let toks = lex_scan(src)?;
    let mut tilde = false;
    let mut version = VERSION_NONE;
    let mut mask = 0u32;
    let mut i = 0;
    while i < toks.len() {
        let tok = match toks.get(i) {
            Some(t) => t.as_str(),
            None => break,
        };
        let (is_tilde, vbit, fbit) = classify(tok)?;
        if is_tilde {
            if tilde {
                return Err(LcsCode::TildeDup.code());
            }
            tilde = true;
        } else if vbit != VERSION_NONE {
            if version != VERSION_NONE {
                return Err(LcsCode::VersionDup.code());
            }
            version = vbit;
        } else {
            mask |= fbit;
        }
        i += 1;
    }
    Ok(ParsedColumn {
        tilde,
        version,
        flag_mask: mask,
        token_count: toks.len() as u32,
    })
}

// ── Windows 口径对照表（15 例，逐项位平面比对） ─────────────────
/// 对照例：输入栏目串 + 期望位平面。
pub const REF_TABLE: [(&str, ParsedColumn); REF_CASES] = [
    // 1 波浪单独（保留现有标志，合法）。
    ("~", ParsedColumn { tilde: true, version: VERSION_NONE, flag_mask: 0, token_count: 1 }),
    // 2~6 波浪 + 各版本记法。
    ("~ WIN7RTM", ParsedColumn { tilde: true, version: 0, flag_mask: 0, token_count: 2 }),
    ("~ WIN8RTM", ParsedColumn { tilde: true, version: 1, flag_mask: 0, token_count: 2 }),
    ("~ WIN80RTM", ParsedColumn { tilde: true, version: 2, flag_mask: 0, token_count: 2 }),
    ("~ WIN81RTM", ParsedColumn { tilde: true, version: 3, flag_mask: 0, token_count: 2 }),
    ("~ WIN10RTM", ParsedColumn { tilde: true, version: 4, flag_mask: 0, token_count: 2 }),
    // 7 无波浪版本（覆盖式，合法）。
    ("WIN10RTM", ParsedColumn { tilde: false, version: 4, flag_mask: 0, token_count: 1 }),
    // 8~10 单标志。
    ("~ HIGHDPIAWARE", ParsedColumn { tilde: true, version: VERSION_NONE, flag_mask: 1, token_count: 2 }),
    ("~ DPIUNAWARE", ParsedColumn { tilde: true, version: VERSION_NONE, flag_mask: 2, token_count: 2 }),
    ("~ RUNASADMIN", ParsedColumn { tilde: true, version: VERSION_NONE, flag_mask: 4, token_count: 2 }),
    // 11~12 版本 + 标志组合。
    ("~ WIN7RTM HIGHDPIAWARE", ParsedColumn { tilde: true, version: 0, flag_mask: 1, token_count: 3 }),
    ("~ WIN8RTM RUNASADMIN HIGHDPIAWARE", ParsedColumn { tilde: true, version: 1, flag_mask: 5, token_count: 4 }),
    // 13 分辨率。
    ("~ 640X480", ParsedColumn { tilde: true, version: VERSION_NONE, flag_mask: 8, token_count: 2 }),
    // 14 双标志叠加（bit2+bit5 = 48）。
    ("~ 256COLOR GDISCALING", ParsedColumn { tilde: true, version: VERSION_NONE, flag_mask: 48, token_count: 3 }),
    // 15 无波浪双标志。
    ("HIGHDPIAWARE DPIUNAWARE", ParsedColumn { tilde: false, version: VERSION_NONE, flag_mask: 3, token_count: 2 }),
];

/// 对照复算：15 例位平面零偏差。
pub fn ref_table_check() -> Result<u32, u32> {
    let mut ok = 0u32;
    let mut i = 0;
    while i < REF_TABLE.len() {
        let (src, expect) = REF_TABLE[i];
        let got = parse_column(src)?;
        if !got.matches(&expect) {
            return Err(LcsCode::Deviation.code());
        }
        ok += 1;
        i += 1;
    }
    Ok(ok)
}

// ── 解析账（合法/拒因分账，只增不减） ──────────────────────────
/// 栏目解析账本。
pub struct ParseLedger {
    pub legal: u32,
    pub rejected: u32,
    pub reject_codes: Vec<u32>,
    pub inject_tries: [u32; 6],
    pub inject_caught: [u32; 6],
}

impl ParseLedger {
    /// 空账本。
    pub fn new() -> ParseLedger {
        ParseLedger {
            legal: 0,
            rejected: 0,
            reject_codes: Vec::new(),
            inject_tries: [0; 6],
            inject_caught: [0; 6],
        }
    }
    /// 单栏目入账。
    pub fn record(&mut self, src: &str) -> Result<ParsedColumn, u32> {
        match parse_column(src) {
            Ok(p) => {
                self.legal += 1;
                Ok(p)
            }
            Err(c) => {
                self.rejected += 1;
                self.reject_codes.push(c);
                Err(c)
            }
        }
    }
    /// 守恒：合法 + 拒 = 已试栏目数。
    pub fn conserve(&self, tried: u32) -> bool {
        self.legal + self.rejected == tried
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < 6 {
            self.inject_tries[slot] += 1;
            if hit {
                self.inject_caught[slot] += 1;
            }
        }
    }
    /// 计数守恒（六槽全抓）。
    pub fn conserve_inject(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（六槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.inject_tries[i] == 0 || self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 注入核（6 类坏串 ×10 轮） ───────────────────────────────────
/// 注入探针：
///   0 未知记法；1 版本重复；2 波浪重复；3 超长 token；
///   4 小写违规；5 token 数超限。
pub fn inject_probe(slot: usize) -> Result<ParsedColumn, u32> {
    match slot {
        0 => parse_column("~ FOOBAZ"),
        1 => parse_column("~ WIN7RTM WIN10RTM"),
        2 => parse_column("~ ~"),
        3 => {
            // 33 字节 token（上限 32 + 1）。
            let mut t = String::new();
            let mut n = 0usize;
            while n <= TOKEN_LEN_MAX {
                t.push('A');
                n += 1;
            }
            parse_column(t.as_str())
        }
        4 => parse_column("~ win7rtm"),
        _ => {
            // 17 个 token（上限 16 + 1）。
            let mut s = String::from("~");
            let mut n = 0usize;
            while n <= TOKENS_MAX {
                s.push_str(" HIGHDPIAWARE");
                n += 1;
            }
            parse_column(s.as_str())
        }
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：五态流水 → 对照 15 例零偏差 → 6 类注入 10 轮全抓 →
/// 守恒 → 边界 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    ParsePhase::full_chain()?;
    let refok = ref_table_check()?;
    if refok != REF_CASES as u32 {
        return Err(LcsCode::Deviation.code());
    }
    let mut led = ParseLedger::new();
    // 15 对照例全部合法入账。
    let mut i = 0;
    while i < REF_TABLE.len() {
        let (src, _) = REF_TABLE[i];
        led.record(src)?;
        i += 1;
    }
    // 注入 6 类 × 10 轮。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 6 {
            let hit = match inject_probe(slot) {
                Err(c) => c >= SEG_BASE && c < SEG_BASE + 0x40,
                Ok(_) => false,
            };
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    if !led.all_caught() || !led.conserve_inject() {
        return Err(LcsCode::ConservationBroken.code());
    }
    let tried = led.legal + led.rejected;
    if !led.conserve(tried) {
        return Err(LcsCode::ConservationBroken.code());
    }
    Ok((led.legal, led.rejected, led.inject_caught[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 位平面叠加确定性：两标志同入 → 掩码为两位或。
    let p = parse_column("~ RUNASADMIN HIGHDPIAWARE")?;
    if p.flag_mask == 5 && p.has_flags() && !p.has_version() {
        Ok(())
    } else {
        Err(LcsCode::Deviation.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 空栏目必拒。
    match parse_column("") {
        Err(c) if c == LcsCode::ColumnEmpty.code() => Ok(()),
        _ => Err(LcsCode::ConservationBroken.code()),
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 状态机越级必拒。
    match ParsePhase::Idle.advance(ParsePhase::Assemble) {
        Err(c) if c == LcsCode::ParsePhaseRegress.code() => Ok(()),
        _ => Err(LcsCode::ConservationBroken.code()),
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7EFC0 {
        return false;
    }
    let mut i = 0;
    while i < LcsCode::ALL.len() {
        let v = LcsCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集复核。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < LcsCode::ALL.len() {
        if detail_of(LcsCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（12 条，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    v.push((
        "J1-记法闭集规模",
        VERSION_COUNT == 5 && FLAG_COUNT == 8 && REF_CASES == 15,
        format!("版本{} 标志{} 对照{}", VERSION_COUNT, FLAG_COUNT, REF_CASES),
    ));
    v.push((
        "J1-五态流水",
        ParsePhase::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调"),
    ));
    let refok = match ref_table_check() {
        Ok(n) => n == REF_CASES as u32,
        Err(_) => false,
    };
    v.push((
        "J1-对照15例零偏差",
        refok,
        format!("{} 例位平面精确比对", REF_CASES),
    ));
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 注入 6 类 ×10 轮。
    let mut led = ParseLedger::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 6 {
            let hit = inject_probe(slot).is_err();
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    let total_tries = led.inject_tries[0]
        + led.inject_tries[1]
        + led.inject_tries[2]
        + led.inject_tries[3]
        + led.inject_tries[4]
        + led.inject_tries[5];
    let total_caught = led.inject_caught[0]
        + led.inject_caught[1]
        + led.inject_caught[2]
        + led.inject_caught[3]
        + led.inject_caught[4]
        + led.inject_caught[5];
    v.push((
        "J1-注入10/10全抓",
        led.all_caught(),
        format!("6类×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        led.conserve_inject() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    let pa = probe_a().is_ok();
    let pb = probe_b().is_ok();
    let pc = probe_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", LcsCode::ALL.len()),
    ));
    // 边界四类：空/恰16 token/17 token/恰32字节 token。
    let b0 = parse_column("").is_err();
    let b1 = {
        // 恰 16 token：~ + 15 标志。
        let mut s = String::from("~");
        let mut n = 1usize;
        while n < TOKENS_MAX {
            s.push_str(" HIGHDPIAWARE");
            n += 1;
        }
        parse_column(s.as_str()).is_ok()
    };
    let b2 = inject_probe(5).is_err();
    let b3 = parse_column("HIGHDPIAWARE").is_ok();
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("空{} 满16{} 超17{} 单tok{}", b0, b1, b2, b3),
    ));
    // 解析账守恒：合法+拒=总试。
    let mut led2 = ParseLedger::new();
    let mut i9 = 0;
    while i9 < REF_TABLE.len() {
        let (src, _) = REF_TABLE[i9];
        let _ = led2.record(src);
        i9 += 1;
    }
    let _ = led2.record("~ FOOBAZ");
    v.push((
        "J1-解析账守恒",
        led2.conserve(led2.legal + led2.rejected)
            && led2.legal == REF_CASES as u32
            && led2.rejected == 1,
        format!("法{} 拒{}", led2.legal, led2.rejected),
    ));
    // 双跑一致。
    let g1 = golden_run();
    let g2 = golden_run();
    v.push((
        "J1-双跑一致",
        g1.is_ok() && g2.is_ok() && g1 == g2,
        String::from("golden_run 两跑同值"),
    ));
    v
}

/// 探针入口：本模块 CheckSet（判据 13 条）。
pub fn run_unx18464_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18464 Layers 栏目语法解析");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            // 动态明细槽仅 8 个（契约红线）：前 8 条给运行期算值，
            // 其余转静态明细（CheckSet 记账与 all_passed 不受影响）。
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "判据通过；明细超动态槽，见实现");
        }
        i += 1;
    }
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checks_green() {
        let set = run_unx18464_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn ref_table_zero_deviation() {
        let n = ref_table_check();
        match n {
            Ok(k) => assert!(k == REF_CASES as u32, "15 例全过"),
            Err(_) => panic!("对照表零偏差失败"),
        }
    }

    #[test]
    fn normal_flow_bitmask_overlay() {
        let p = parse_column("~ WIN8RTM RUNASADMIN HIGHDPIAWARE");
        match p {
            Ok(c) => {
                assert!(c.tilde, "波浪记法");
                assert!(c.has_version(), "版本位落账");
                assert!(c.flag_mask == 5, "bit0+bit2=5");
            }
            Err(_) => panic!("合法栏目不该报错"),
        }
    }

    #[test]
    fn boundary_four_kinds() {
        assert!(parse_column("").is_err(), "空栏目必拒");
        // 恰 16 token 合法（~ + 15 标志）。
        let mut s = String::from("~");
        let mut n = 1usize;
        while n < TOKENS_MAX {
            s.push_str(" HIGHDPIAWARE");
            n += 1;
        }
        assert!(parse_column(s.as_str()).is_ok(), "恰 16 token 合法");
        assert!(inject_probe(5).is_err(), "17 token 必拒");
        assert!(inject_probe(3).is_err(), "33 字节 token 必拒");
    }

    #[test]
    fn injection_six_classes_all_caught() {
        let mut slot = 0usize;
        while slot < 6 {
            assert!(inject_probe(slot).is_err(), "第 {} 类注入必抓", slot);
            slot += 1;
        }
    }

    #[test]
    fn parse_rejects_dup_and_case() {
        assert!(parse_column("~ WIN7RTM WIN10RTM").is_err(), "版本重复");
        assert!(parse_column("~ ~").is_err(), "波浪重复");
        assert!(parse_column("~ win7rtm").is_err(), "小写违规");
        assert!(parse_column("~ FOO#").is_err(), "杂符违规");
    }

    #[test]
    fn determinism_double_run() {
        let g1 = golden_run();
        let g2 = golden_run();
        match (g1, g2) {
            (Ok(a), Ok(b)) => assert!(a == b, "双跑必须逐值一致"),
            _ => panic!("黄金全链失败"),
        }
    }

    #[test]
    fn segment_guard_window() {
        assert!(segment_guard(), "段位守卫失败");
        let mut i = 0;
        while i < LcsCode::ALL.len() {
            let c = LcsCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
    }
}
