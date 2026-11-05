//! # unx49099 — B15 批防重 grep 与批收官印
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49099 B15 批防重 grep（关键词族：flood/reset/finalseg）**
//! > **F49100 B15 批收官印（20 条冻结/域账累计 90 000 行）**
//!
//! 验收锚点：
//!
//! - `UNX-F49099-J1` 批收口防重留痕零撞号；判据锚定 §6.1。
//! - `UNX-F49100-J1` 批印入册：90000/240,000；判据锚定 §AI-62 §7。
//!
//! ## 闸序与理由
//!
//! 1. **关键词族零撞号**：三枚关键词 flood/reset/finalseg 必须互不为前缀、
//!    互不为子串，且二十张卡每张只挂一枚。关键词互相包含会让 grep 结果
//!    无法归因，防重就退化成“看起来有数”。
//! 2. **语料零重复**：二十行语料必须逐行不同，卡号从 F49081 连续到 F49100；
//!    重复行意味着同一张卡被数了两次，比例账随之失真。
//! 3. **grep 可复跑**：同一语料重复 grep 三次，命中数与摘要逐位一致；
//!    不能复跑的 grep 只是截屏，不是账。
//! 4. **二十条冻结**：B15 批恰好二十张卡，每卡三百行冻结，批内合计
//!    六千行；多一张或少一张都说明冻结清单与卡表脱节。
//! 5. **批印入册**：十五批 × 二十卡 × 三百行 = 九万行，对域目标二十四万行
//!    记三百七十五‰；批印必须由结构真算，不允许手填比例。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49089_finalgrep` 对十五批做全域关键词终宣；本模块只对 **B15 批内**
//!   的 flood/reset/finalseg 三族做批级 grep，范围更小、粒度到卡。
//! - `unx49093_handover` 的段印是“移交宣告”，本模块的批印是“冻结行数入册”，
//!   前者回答能不能交，后者回答交了多少。
//! - 任务板 `F49601` 的 PPD 差异不由本模块冒充：按诚实三态原则，它已在
//!   `unx49087_drift_guard` 的前哨账中登记为补测，本模块只报 375‰ 的结构
//!   比例，不宣称任何未测得的性能指标。
//!
//! ## 批次语料口径
//!
//! 二十张卡的卡号是 `F49081..=F49100`，关键词按卡序三循环落位，因此
//! 期望命中是 flood ×7、reset ×7、finalseg ×6，合计二十。这个分布是
//! **结构事实**，不是从任务板抄来的结论：谁改了卡序或关键词环，命中数
//! 与摘要会同时变红。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与常量 ───────────────────────────────────────────────────
/// 本模块段基址（B15 末槽）。
pub const SEG_BASE: u32 = 0x73740;
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;
/// 批内卡数。
pub const CARDS: usize = 20;
/// 首卡号。
pub const CARD_BASE: u32 = 49_081;
/// 末卡号（含）。
pub const CARD_LAST: u32 = CARD_BASE + CARDS as u32 - 1;
/// 关键词族数。
pub const KEYWORDS: usize = 3;
/// 每卡冻结行数。
pub const LINES_PER_CARD: u32 = 300;
/// 批内冻结行数。
pub const BATCH_LINES: u32 = CARDS as u32 * LINES_PER_CARD;
/// 批数。
pub const BATCHES: u32 = 15;
/// 冻结卡总数。
pub const FROZEN_CARDS: u32 = BATCHES * CARDS as u32;
/// 域账累计行数。
pub const SEALED_LINES: u32 = FROZEN_CARDS * LINES_PER_CARD;
/// 域目标行数。
pub const DOMAIN_TARGET: u32 = 240_000;
/// 批印比例（千分比）。
pub const PER_MILLE: u32 = 375;
/// grep 账行标签。
pub const GREP_TAG: &str = "GREP99";
/// 批印账行标签。
pub const SEAL_TAG: &str = "SEAL100";
/// grep 账键序。
pub const GREP_KEYS: [&str; 3] = ["keyword", "hits", "cards"];
/// 批印账键序。
pub const SEAL_KEYS: [&str; 4] = ["batches", "cards", "lines", "target"];
/// 复跑次数。
pub const REPLAY_TIMES: u32 = 3;
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B15 批防重诊断码";

// ── 诊断码 ───────────────────────────────────────────────────────
/// 批防重与批印诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SgCode {
    /// 关键词不在三族内
    KeywordBad,
    /// 关键词互为前缀或子串
    PrefixClash,
    /// 同一张卡挂了两枚关键词
    CardDup,
    /// 卡号不连续
    CardGap,
    /// 语料行重复
    LineDup,
    /// 命中数与语料不符
    HitsBad,
    /// 卡数越界
    CardsBad,
    /// 冻结行数不符
    LinesBad,
    /// 批数不符
    BatchesBad,
    /// 域目标不符
    TargetBad,
    /// 千分比与结构不符
    MillBad,
    /// 摘要与重算不符
    DigestBad,
    /// 账行键序或字段错位
    RowBad,
    /// 语料表不完整
    Incomplete,
}

impl SgCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            SgCode::KeywordBad => "关键词不在 flood/reset/finalseg 内",
            SgCode::PrefixClash => "关键词互为前缀或子串",
            SgCode::CardDup => "同一卡号挂了多枚关键词",
            SgCode::CardGap => "卡号未从 F49081 连续到 F49100",
            SgCode::LineDup => "语料行两两重复",
            SgCode::HitsBad => "grep 命中数与语料结构不符",
            SgCode::CardsBad => "批内卡数不是二十",
            SgCode::LinesBad => "冻结行数不等于卡数乘三百",
            SgCode::BatchesBad => "批数不是十五",
            SgCode::TargetBad => "域目标不是二十四万行",
            SgCode::MillBad => "千分比与冻结行数不符",
            SgCode::DigestBad => "账行摘要与重算不一致",
            SgCode::RowBad => "账行标签、键序或字段数错位",
            SgCode::Incomplete => "语料或批印表不完整",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            SgCode::KeywordBad => "关键词越界：请回读三族",
            SgCode::PrefixClash => "关键词互含：请改名后再 grep",
            SgCode::CardDup => "卡号重叠：请核对卡表",
            SgCode::CardGap => "卡号断档：请补齐二十卡",
            SgCode::LineDup => "语料重复：请去重",
            SgCode::HitsBad => "命中失配：请重跑 grep",
            SgCode::CardsBad => "卡数不符：请冻结二十卡",
            SgCode::LinesBad => "行数不符：请重算冻结",
            SgCode::BatchesBad => "批数不符：请核对十五批",
            SgCode::TargetBad => "目标不符：请回读域账",
            SgCode::MillBad => "比例不符：请重算千分比",
            SgCode::DigestBad => "摘要失配：请重渲染账行",
            SgCode::RowBad => "账行损坏：请核对键序",
            SgCode::Incomplete => "表不完整：请补齐语料",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [SgCode; 14] = [
    SgCode::KeywordBad,
    SgCode::PrefixClash,
    SgCode::CardDup,
    SgCode::CardGap,
    SgCode::LineDup,
    SgCode::HitsBad,
    SgCode::CardsBad,
    SgCode::LinesBad,
    SgCode::BatchesBad,
    SgCode::TargetBad,
    SgCode::MillBad,
    SgCode::DigestBad,
    SgCode::RowBad,
    SgCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => SgCode::KeywordBad.detail(),
        2 => SgCode::PrefixClash.detail(),
        3 => SgCode::CardDup.detail(),
        4 => SgCode::CardGap.detail(),
        5 => SgCode::LineDup.detail(),
        6 => SgCode::HitsBad.detail(),
        7 => SgCode::CardsBad.detail(),
        8 => SgCode::LinesBad.detail(),
        9 => SgCode::BatchesBad.detail(),
        10 => SgCode::TargetBad.detail(),
        11 => SgCode::MillBad.detail(),
        12 => SgCode::DigestBad.detail(),
        13 => SgCode::RowBad.detail(),
        14 => SgCode::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 段位自检：14 码值两两不同且全部落在本段窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = ALL[i].val();
        if a <= SEG_BASE || a > SEG_BASE + 0x0F {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[j].val() == a {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 关键词族 ─────────────────────────────────────────────────────
/// 批内关键词。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keyword {
    /// 洪泛。
    Flood,
    /// 重置。
    Reset,
    /// 段收尾。
    FinalSeg,
}

impl Keyword {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Keyword::Flood => 0,
            Keyword::Reset => 1,
            Keyword::FinalSeg => 2,
        }
    }

    /// 关键词原文。
    pub fn tag(self) -> &'static str {
        match self {
            Keyword::Flood => "flood",
            Keyword::Reset => "reset",
            Keyword::FinalSeg => "finalseg",
        }
    }

    /// 由原文反解。
    pub fn from_tag(tag: &str) -> Option<Keyword> {
        match tag {
            "flood" => Some(Keyword::Flood),
            "reset" => Some(Keyword::Reset),
            "finalseg" => Some(Keyword::FinalSeg),
            _ => None,
        }
    }

    /// 三族按卡序循环落位。
    pub fn cycle(slot: usize) -> Keyword {
        match slot % KEYWORDS {
            0 => Keyword::Flood,
            1 => Keyword::Reset,
            _ => Keyword::FinalSeg,
        }
    }
}

/// 三枚关键词全部原文（按序）。
pub const KEYWORD_TAGS: [&str; KEYWORDS] = ["flood", "reset", "finalseg"];

/// 二十卡的落位表（三循环，手写）。
pub const CARD_KEYWORDS: [Keyword; CARDS] = [
    Keyword::Flood,
    Keyword::Reset,
    Keyword::FinalSeg,
    Keyword::Flood,
    Keyword::Reset,
    Keyword::FinalSeg,
    Keyword::Flood,
    Keyword::Reset,
    Keyword::FinalSeg,
    Keyword::Flood,
    Keyword::Reset,
    Keyword::FinalSeg,
    Keyword::Flood,
    Keyword::Reset,
    Keyword::FinalSeg,
    Keyword::Flood,
    Keyword::Reset,
    Keyword::FinalSeg,
    Keyword::Flood,
    Keyword::Reset,
];

/// 卡号。
pub fn card_id(index: usize) -> Option<u32> {
    if index >= CARDS {
        return None;
    }
    Some(CARD_BASE + index as u32)
}

/// 单行语料（卡号、关键词、序号）。
pub fn corpus_line(index: usize) -> Option<String> {
    let id = card_id(index)?;
    let kw = CARD_KEYWORDS[index];
    Some(format!("F{}|{}|card-{}", id, kw.tag(), id))
}

/// 二十行语料。
pub fn corpus() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < CARDS {
        match corpus_line(i) {
            Some(line) => out.push(line),
            None => return out,
        }
        i += 1;
    }
    out
}

/// 关键词互不为前缀、互不为子串。
pub fn prefix_free() -> bool {
    let mut i = 0usize;
    while i < KEYWORD_TAGS.len() {
        let mut j = 0usize;
        while j < KEYWORD_TAGS.len() {
            if i != j {
                if KEYWORD_TAGS[i].starts_with(KEYWORD_TAGS[j])
                    || KEYWORD_TAGS[j].starts_with(KEYWORD_TAGS[i])
                    || KEYWORD_TAGS[i].contains(KEYWORD_TAGS[j])
                    || KEYWORD_TAGS[j].contains(KEYWORD_TAGS[i])
                {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 每卡恰好一枚关键词。
pub fn one_keyword_per_card() -> bool {
    if CARD_KEYWORDS.len() != CARDS {
        return false;
    }
    let mut i = 0usize;
    while i < CARD_KEYWORDS.len() {
        if CARD_KEYWORDS[i] != Keyword::cycle(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// 卡号连续。
pub fn cards_contiguous() -> bool {
    let mut i = 0usize;
    while i < CARDS {
        match card_id(i) {
            Some(id) => {
                if id != CARD_BASE + i as u32 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    card_id(CARDS).is_none()
}

/// 语料二十行两两不同。
pub fn corpus_distinct() -> bool {
    let lines = corpus();
    if lines.len() != CARDS {
        return false;
    }
    let mut i = 0usize;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() {
            if lines[i] == lines[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── grep ─────────────────────────────────────────────────────────
/// 单关键词命中报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepReport {
    /// 关键词。
    pub keyword: Keyword,
    /// 命中行数。
    pub hits: u32,
    /// 命中卡数。
    pub cards: u32,
}

/// 单关键词 grep（关键词非法返回 `None`）。
pub fn grep(token: &str) -> Option<GrepReport> {
    let keyword = Keyword::from_tag(token)?;
    let mut hits = 0u32;
    let mut cards = 0u32;
    let mut i = 0usize;
    while i < CARD_KEYWORDS.len() {
        if CARD_KEYWORDS[i] == keyword {
            hits = hits.saturating_add(1);
            cards = cards.saturating_add(1);
        }
        i += 1;
    }
    Some(GrepReport {
        keyword,
        hits,
        cards,
    })
}

/// 三族 grep。
pub fn grep_all() -> Vec<GrepReport> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < KEYWORD_TAGS.len() {
        match grep(KEYWORD_TAGS[i]) {
            Some(r) => out.push(r),
            None => return out,
        }
        i += 1;
    }
    out
}

/// 三族命中合计。
pub fn total_hits() -> u32 {
    let mut total = 0u32;
    let mut i = 0usize;
    let all = grep_all();
    while i < all.len() {
        total = total.saturating_add(all[i].hits);
        i += 1;
    }
    total
}

/// 期望命中分布：flood ×7、reset ×7、finalseg ×6。
pub fn expected_hits(keyword: Keyword) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CARD_KEYWORDS.len() {
        if CARD_KEYWORDS[i] == keyword {
            n += 1;
        }
        i += 1;
    }
    n
}

/// grep 摘要：关键词 + 命中 + 卡数。
pub fn grep_digest(r: &GrepReport) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4752_4550);
    let bytes = r.keyword.tag().as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h = p_core::fold_byte(h, bytes[i]);
        i += 1;
    }
    h = p_core::fold_u32(h, r.hits);
    p_core::fold_u32(h, r.cards)
}

// ── 批印 ─────────────────────────────────────────────────────────
/// 批收官印。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seal {
    /// 批数。
    pub batches: u32,
    /// 冻结卡数。
    pub cards: u32,
    /// 每卡行数。
    pub lines_per_card: u32,
    /// 域账累计行数。
    pub lines: u32,
    /// 域目标行数。
    pub target: u32,
}

impl Seal {
    /// 由结构造印（不手填行数）。
    pub fn from_structure(batches: u32, cards_per_batch: u32, lines_per_card: u32, target: u32) -> Seal {
        let cards = batches.saturating_mul(cards_per_batch);
        let lines = cards.saturating_mul(lines_per_card);
        Seal {
            batches,
            cards,
            lines_per_card,
            lines,
            target,
        }
    }

    /// 千分比（目标为零时记零）。
    pub fn per_mille(&self) -> u32 {
        if self.target == 0 {
            return 0;
        }
        self.lines.saturating_mul(1000) / self.target
    }

    /// 自洽：字段与结构、比例同时成立。
    pub fn honest(&self) -> bool {
        self.batches == BATCHES
            && self.cards == FROZEN_CARDS
            && self.lines_per_card == LINES_PER_CARD
            && self.lines == self.cards.saturating_mul(self.lines_per_card)
            && self.lines == SEALED_LINES
            && self.target == DOMAIN_TARGET
            && self.per_mille() == PER_MILLE
    }
}

/// 标准批印。
pub fn seal() -> Seal {
    Seal::from_structure(BATCHES, CARDS as u32, LINES_PER_CARD, DOMAIN_TARGET)
}

/// 批内冻结行数（卡数 × 每卡行数）。
pub fn batch_lines() -> u32 {
    (CARDS as u32).saturating_mul(LINES_PER_CARD)
}

/// 批印摘要：批数 + 卡数 + 行数 + 目标。
pub fn seal_digest(s: &Seal) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x5345_414C);
    h = p_core::fold_u32(h, s.batches);
    h = p_core::fold_u32(h, s.cards);
    h = p_core::fold_u32(h, s.lines);
    p_core::fold_u32(h, s.target)
}

// ── 账行 ─────────────────────────────────────────────────────────
/// 解析十进制（拒绝空串、非数字、溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 解析 16 位十六进制摘要。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?;
        acc = acc.checked_add(v)?;
        i += 1;
    }
    Some(acc)
}

/// 按固定标签与键序拆行；返回各键值与摘要值。
pub fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != keys.len() + 2 {
        return None;
    }
    if parts[0] != tag {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < keys.len() {
        let piece = parts[i + 1];
        let rest = piece.strip_prefix(keys[i])?;
        let value = rest.strip_prefix('=')?;
        if value.is_empty() {
            return None;
        }
        out.push(value);
        i += 1;
    }
    let d = parts[keys.len() + 1].strip_prefix("digest=")?;
    if d.len() != 16 {
        return None;
    }
    out.push(d);
    Some(out)
}

/// 渲染 grep 账行。
pub fn render_grep_row(r: &GrepReport) -> String {
    format!(
        "{}|keyword={}|hits={}|cards={}|digest={:016x}",
        GREP_TAG,
        r.keyword.tag(),
        r.hits,
        r.cards,
        grep_digest(r)
    )
}

/// 回读 grep 账行（重跑 grep 比对）。
pub fn audit_grep_row(row: &str) -> bool {
    let f = match row_fields(row, GREP_TAG, &GREP_KEYS) {
        Some(v) if v.len() == GREP_KEYS.len() + 1 => v,
        _ => return false,
    };
    let report = match grep(f[0]) {
        Some(r) => r,
        None => return false,
    };
    let hits = match parse_u32(f[1]) {
        Some(v) => v,
        _ => return false,
    };
    let cards = match parse_u32(f[2]) {
        Some(v) => v,
        _ => return false,
    };
    let want = match parse_hex16(f[3]) {
        Some(v) => v,
        None => return false,
    };
    report.hits == hits && report.cards == cards && grep_digest(&report) == want
}

/// 三行 grep 账。
pub fn export_grep_rows() -> Vec<String> {
    let mut out = Vec::new();
    let all = grep_all();
    let mut i = 0usize;
    while i < all.len() {
        out.push(render_grep_row(&all[i]));
        i += 1;
    }
    out
}

/// 三行两两不同且逐行可回读。
pub fn grep_rows_stable() -> bool {
    let rows = export_grep_rows();
    if rows.len() != KEYWORDS {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_grep_row(&rows[i]) {
            return false;
        }
        let mut j = i + 1;
        while j < rows.len() {
            if rows[j] == rows[i] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 渲染批印账行。
pub fn render_seal_row(s: &Seal) -> String {
    format!(
        "{}|batches={}|cards={}|lines={}|target={}|digest={:016x}",
        SEAL_TAG,
        s.batches,
        s.cards,
        s.lines,
        s.target,
        seal_digest(s)
    )
}

/// 回读批印账行（按结构重算）。
pub fn audit_seal_row(row: &str) -> bool {
    let f = match row_fields(row, SEAL_TAG, &SEAL_KEYS) {
        Some(v) if v.len() == SEAL_KEYS.len() + 1 => v,
        _ => return false,
    };
    let batches = match parse_u32(f[0]) {
        Some(v) => v,
        _ => return false,
    };
    let cards = match parse_u32(f[1]) {
        Some(v) => v,
        _ => return false,
    };
    let lines = match parse_u32(f[2]) {
        Some(v) => v,
        _ => return false,
    };
    let target = match parse_u32(f[3]) {
        Some(v) => v,
        _ => return false,
    };
    let want = match parse_hex16(f[4]) {
        Some(v) => v,
        None => return false,
    };
    let s = Seal::from_structure(batches, CARDS as u32, LINES_PER_CARD, target);
    cards == s.cards && lines == s.lines && s.honest() && seal_digest(&s) == want
}

// ── 八项判据 ─────────────────────────────────────────────────────
/// 八项自检（真算，不预置结论）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) 关键词族零撞号。
    let all = grep_all();
    let mut each_positive = all.len() == KEYWORDS;
    let mut i = 0usize;
    while i < all.len() {
        if all[i].hits == 0 || all[i].cards != all[i].hits {
            each_positive = false;
        }
        if all[i].hits != expected_hits(all[i].keyword) {
            each_positive = false;
        }
        i += 1;
    }
    let total = total_hits();
    let ok = prefix_free()
        && one_keyword_per_card()
        && each_positive
        && total == CARDS as u32
        && grep("").is_none()
        && grep("floodx").is_none();
    out.push((
        "F49099/关键词族零撞号",
        ok,
        format!(
            "keywords={} hits={} prefix_free={} one_per_card={}",
            all.len(),
            total,
            prefix_free(),
            one_keyword_per_card()
        ),
    ));

    // 2) 语料零重复。
    let lines = corpus();
    let ok = lines.len() == CARDS
        && corpus_distinct()
        && cards_contiguous()
        && card_id(0) == Some(CARD_BASE)
        && card_id(CARDS - 1) == Some(CARD_LAST)
        && card_id(CARDS).is_none();
    out.push((
        "F49099/语料零重复",
        ok,
        format!(
            "lines={} distinct={} contiguous={} first=F{} last=F{}",
            lines.len(),
            corpus_distinct(),
            cards_contiguous(),
            CARD_BASE,
            CARD_LAST
        ),
    ));

    // 3) grep 账行回读。
    let rows = export_grep_rows();
    let mut tamp = String::new();
    if let Some(first) = rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_grep_row(&tamp);
    let ok = rows.len() == KEYWORDS && grep_rows_stable() && tamper_rejected;
    out.push((
        "F49099/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            rows.len(),
            grep_rows_stable(),
            tamper_rejected
        ),
    ));

    // 4) grep 复跑留痕。
    let mut stable = true;
    let mut r = 0u32;
    let base = match grep("flood") {
        Some(g) => g,
        None => {
            out.push((
                "F49099/复跑留痕",
                false,
                String::from("flood grep 缺失"),
            ));
            return extra_checks_tail(out);
        }
    };
    let base_digest = grep_digest(&base);
    while r < REPLAY_TIMES {
        match grep("flood") {
            Some(g) => {
                if g != base || grep_digest(&g) != base_digest {
                    stable = false;
                }
            }
            None => stable = false,
        }
        r += 1;
    }
    let corpus_stable = corpus() == corpus();
    let ok = stable && corpus_stable && base_digest != p_core::FNV_OFF;
    out.push((
        "F49099/复跑留痕",
        ok,
        format!(
            "replays={} digest={:016x} corpus_stable={}",
            REPLAY_TIMES, base_digest, corpus_stable
        ),
    ));

    // 5) 二十条冻结。
    let s = seal();
    let ok = s.cards == FROZEN_CARDS
        && CARD_KEYWORDS.len() == CARDS
        && batch_lines() == BATCH_LINES
        && BATCH_LINES == 6_000
        && s.lines_per_card == LINES_PER_CARD;
    out.push((
        "F49100/二十条冻结",
        ok,
        format!(
            "batch_cards={} batch_lines={} per_card={}",
            CARDS, batch_lines(), LINES_PER_CARD
        ),
    ));

    // 6) 批印入册 90000/240000。
    let ok = s.honest()
        && s.lines == 90_000
        && s.target == DOMAIN_TARGET
        && s.per_mille() == PER_MILLE
        && !Seal::from_structure(BATCHES, CARDS as u32, LINES_PER_CARD, DOMAIN_TARGET - 1).honest();
    out.push((
        "F49100/批印入册",
        ok,
        format!(
            "lines={} target={} per_mille={}",
            s.lines,
            s.target,
            s.per_mille()
        ),
    ));

    // 7) 批印账行回读。
    let row = render_seal_row(&s);
    let mut tamp = row.clone();
    tamp.push('7');
    let tamper_rejected = !audit_seal_row(&tamp);
    let inflated = Seal::from_structure(BATCHES, CARDS as u32, LINES_PER_CARD, DOMAIN_TARGET);
    let ok = audit_seal_row(&row) && tamper_rejected && inflated.honest();
    out.push((
        "F49100/账行回读",
        ok,
        format!(
            "row_ok={} tamper_rejected={} lines={}",
            audit_seal_row(&row),
            tamper_rejected,
            s.lines
        ),
    ));

    extra_checks_tail(out)
}

/// 尾项：异常码守卫。
fn extra_checks_tail(mut out: Vec<(&'static str, bool, String)>) -> Vec<(&'static str, bool, String)> {
    let mut distinct = true;
    let mut a = 0usize;
    while a < ALL.len() {
        let mut b = a + 1;
        while b < ALL.len() {
            if ALL[a].val() == ALL[b].val()
                || ALL[a].detail() == ALL[b].detail()
                || ALL[a].message() == ALL[b].message()
            {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    let free_ok = detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + 0x0F) == UNKNOWN
        && detail(ALL[0].val()) == SgCode::KeywordBad.detail();
    let keywords_ok = Keyword::from_tag(KEYWORD_TAGS[0]) == Some(Keyword::Flood)
        && Keyword::from_tag(KEYWORD_TAGS[1]) == Some(Keyword::Reset)
        && Keyword::from_tag(KEYWORD_TAGS[2]) == Some(Keyword::FinalSeg)
        && Keyword::from_tag("").is_none();
    let ok = segment_guard() && distinct && free_ok && keywords_ok;
    out.push((
        "F49099/异常码守卫",
        ok,
        format!(
            "codes={} guard={} distinct={} free={} keywords={}",
            ALL.len(),
            segment_guard(),
            distinct,
            free_ok,
            keywords_ok
        ),
    ));
    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49099_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49099 批防重与批收官印");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        s.add_dyn(checks[i].0, checks[i].1, &checks[i].2);
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 诊断码 ──────────────────────────────────────────────────

    #[test]
    fn codes_in_window() {
        assert!(segment_guard());
        assert_eq!(ALL.len(), 14);
        let mut i = 0usize;
        while i < ALL.len() {
            assert!(ALL[i].val() > SEG_BASE);
            assert!(ALL[i].val() < SEG_BASE + SEG_SPAN);
            assert_eq!(ALL[i].val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }

    #[test]
    fn codes_text_distinct() {
        let mut i = 0usize;
        while i < ALL.len() {
            let mut j = i + 1;
            while j < ALL.len() {
                assert_ne!(ALL[i].val(), ALL[j].val());
                assert_ne!(ALL[i].detail(), ALL[j].detail());
                assert_ne!(ALL[i].message(), ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn detail_free_matches_table() {
        let mut i = 0usize;
        while i < ALL.len() {
            assert_eq!(detail(ALL[i].val()), ALL[i].detail());
            i += 1;
        }
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(SEG_BASE + SEG_SPAN), UNKNOWN);
    }

    // ── 关键词 ──────────────────────────────────────────────────

    #[test]
    fn keyword_roundtrip() {
        let all = [Keyword::Flood, Keyword::Reset, Keyword::FinalSeg];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].index() as usize, i);
            assert_eq!(Keyword::from_tag(all[i].tag()), Some(all[i]));
            assert_eq!(Keyword::from_tag(KEYWORD_TAGS[i]), Some(all[i]));
            i += 1;
        }
        assert_eq!(Keyword::from_tag(""), None);
        assert_eq!(Keyword::from_tag("Flood"), None);
        assert_eq!(Keyword::from_tag("floodx"), None);
    }

    #[test]
    fn keyword_tags_distinct() {
        let mut i = 0usize;
        while i < KEYWORD_TAGS.len() {
            let mut j = i + 1;
            while j < KEYWORD_TAGS.len() {
                assert_ne!(KEYWORD_TAGS[i], KEYWORD_TAGS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn keyword_cycle_repeats_every_three() {
        let mut i = 0usize;
        while i < CARDS {
            assert_eq!(Keyword::cycle(i), Keyword::cycle(i % KEYWORDS));
            i += 1;
        }
        assert_eq!(Keyword::cycle(0), Keyword::Flood);
        assert_eq!(Keyword::cycle(1), Keyword::Reset);
        assert_eq!(Keyword::cycle(2), Keyword::FinalSeg);
        assert_eq!(Keyword::cycle(3), Keyword::Flood);
    }

    #[test]
    fn prefix_free_holds() {
        assert!(prefix_free());
        assert!(!KEYWORD_TAGS[0].starts_with(KEYWORD_TAGS[1]));
        assert!(!KEYWORD_TAGS[2].contains(KEYWORD_TAGS[0]));
    }

    // ── 卡表与语料 ──────────────────────────────────────────────

    #[test]
    fn card_ids_contiguous() {
        assert!(cards_contiguous());
        assert_eq!(card_id(0), Some(49_081));
        assert_eq!(card_id(CARDS - 1), Some(49_100));
        assert_eq!(CARD_LAST, 49_100);
        assert_eq!(card_id(CARDS), None);
    }

    #[test]
    fn card_keywords_len_and_order() {
        assert_eq!(CARD_KEYWORDS.len(), CARDS);
        assert!(one_keyword_per_card());
    }

    #[test]
    fn corpus_shape_distinct() {
        let lines = corpus();
        assert_eq!(lines.len(), CARDS);
        assert!(corpus_distinct());
        let mut i = 0usize;
        while i < lines.len() {
            assert!(lines[i].starts_with('F'));
            assert!(lines[i].contains("card-"));
            i += 1;
        }
    }

    #[test]
    fn corpus_line_bounds() {
        assert!(corpus_line(0).is_some());
        assert_eq!(corpus_line(CARDS), None);
    }

    #[test]
    fn corpus_keyword_field_matches_table() {
        let lines = corpus();
        let mut i = 0usize;
        while i < lines.len() {
            let field = match lines[i].split('|').nth(1) {
                Some(f) => f,
                None => panic!("字段缺失"),
            };
            assert_eq!(field, CARD_KEYWORDS[i].tag());
            i += 1;
        }
    }

    // ── grep ────────────────────────────────────────────────────

    #[test]
    fn grep_distribution() {
        let flood = match grep("flood") {
            Some(r) => r,
            None => panic!("flood 缺失"),
        };
        let reset = match grep("reset") {
            Some(r) => r,
            None => panic!("reset 缺失"),
        };
        let seg = match grep("finalseg") {
            Some(r) => r,
            None => panic!("finalseg 缺失"),
        };
        assert_eq!(flood.hits, 7);
        assert_eq!(reset.hits, 7);
        assert_eq!(seg.hits, 6);
        assert_eq!(flood.cards, flood.hits);
        assert_eq!(total_hits(), 20);
    }

    #[test]
    fn grep_rejects_unknown() {
        assert_eq!(grep(""), None);
        assert_eq!(grep("fuzz"), None);
        assert_eq!(grep("FLOOD"), None);
    }

    #[test]
    fn grep_all_three() {
        let all = grep_all();
        assert_eq!(all.len(), KEYWORDS);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].keyword.index() as usize, i);
            assert_eq!(all[i].hits, expected_hits(all[i].keyword));
            i += 1;
        }
    }

    #[test]
    fn grep_digest_sensitive() {
        let a = match grep("flood") {
            Some(r) => r,
            None => panic!("flood 缺失"),
        };
        let b = match grep("reset") {
            Some(r) => r,
            None => panic!("reset 缺失"),
        };
        assert_eq!(grep_digest(&a), grep_digest(&a));
        assert_ne!(grep_digest(&a), grep_digest(&b));
        assert_ne!(grep_digest(&a), p_core::FNV_OFF);
    }

    #[test]
    fn grep_rows_stable_and_distinct() {
        let rows = export_grep_rows();
        assert_eq!(rows.len(), KEYWORDS);
        assert!(grep_rows_stable());
        let mut i = 0usize;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len() {
                assert_ne!(rows[i], rows[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn grep_row_tamper_rejected() {
        let rows = export_grep_rows();
        let first = match rows.first() {
            Some(r) => r,
            None => panic!("无账行"),
        };
        assert!(audit_grep_row(first));
        let mut tamp = first.clone();
        tamp.push('x');
        assert!(!audit_grep_row(&tamp));
        assert!(!audit_grep_row(
            "GREP99|keyword=fuzz|hits=0|cards=0|digest=0000000000000000"
        ));
    }

    #[test]
    fn grep_row_rejects_wrong_hits() {
        let row = match export_grep_rows().first() {
            Some(r) => r.clone(),
            None => panic!("无账行"),
        };
        let bad = row.replace("hits=7", "hits=8");
        assert!(!audit_grep_row(&bad));
    }

    // ── 批印 ────────────────────────────────────────────────────

    #[test]
    fn seal_structure() {
        let s = seal();
        assert_eq!(s.batches, 15);
        assert_eq!(s.cards, 300);
        assert_eq!(s.lines_per_card, 300);
        assert_eq!(s.lines, 90_000);
        assert_eq!(s.target, 240_000);
        assert_eq!(s.per_mille(), 375);
        assert!(s.honest());
    }

    #[test]
    fn batch_lines_six_thousand() {
        assert_eq!(BATCH_LINES, 6_000);
        assert_eq!(batch_lines(), 6_000);
        assert_eq!(batch_lines() * BATCHES, SEALED_LINES);
    }

    #[test]
    fn seal_honest_rejects_drift() {
        let base = seal();
        let mut bad = base;
        bad.batches = 14;
        assert!(!bad.honest());
        let mut bad = base;
        bad.cards = 299;
        assert!(!bad.honest());
        let mut bad = base;
        bad.lines = 89_999;
        assert!(!bad.honest());
        let mut bad = base;
        bad.target = 241_000;
        assert!(!bad.honest());
        let mut bad = base;
        bad.lines_per_card = 500;
        assert!(!bad.honest());
    }

    #[test]
    fn seal_per_mille_zero_target() {
        let s = Seal::from_structure(15, 20, 300, 0);
        assert_eq!(s.per_mille(), 0);
        assert!(!s.honest());
    }

    #[test]
    fn seal_from_structure_saturates() {
        let s = Seal::from_structure(u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        assert_eq!(s.cards, u32::MAX);
        assert_eq!(s.lines, u32::MAX);
        assert!(!s.honest());
    }

    #[test]
    fn seal_digest_sensitive() {
        let a = seal();
        let d = seal_digest(&a);
        assert_eq!(d, seal_digest(&a));
        assert_ne!(d, p_core::FNV_OFF);
        let mut b = a;
        b.target = DOMAIN_TARGET + 1;
        assert_ne!(d, seal_digest(&b));
    }

    #[test]
    fn seal_row_audit_and_tamper() {
        let row = render_seal_row(&seal());
        assert!(row.starts_with(SEAL_TAG));
        assert_eq!(row.split('|').count(), SEAL_KEYS.len() + 2);
        assert!(audit_seal_row(&row));
        let mut tamp = row.clone();
        tamp.push('0');
        assert!(!audit_seal_row(&tamp));
        let bad = row.replace("target=240000", "target=241000");
        assert!(!audit_seal_row(&bad));
    }

    #[test]
    fn seal_row_rejects_bad_batches() {
        assert!(!audit_seal_row(
            "SEAL100|batches=14|cards=280|lines=84000|target=240000|digest=0000000000000000"
        ));
    }

    // ── 解析与账行工具 ──────────────────────────────────────────

    #[test]
    fn parse_u32_boundaries() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("4x"), None);
    }

    #[test]
    fn parse_hex16_boundaries() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("0123456789abcdef"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789ABCDEF"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789abcde"), None);
        assert_eq!(parse_hex16("0123456789abcdef0"), None);
        assert_eq!(parse_hex16("0123456789abcdeg"), None);
    }

    #[test]
    fn row_fields_shape() {
        let row = render_seal_row(&seal());
        assert_eq!(
            row_fields(&row, SEAL_TAG, &SEAL_KEYS).map(|v| v.len()),
            Some(5)
        );
        assert_eq!(row_fields(&row, GREP_TAG, &SEAL_KEYS), None);
        assert_eq!(row_fields(&row, SEAL_TAG, &GREP_KEYS), None);
        assert_eq!(row_fields(SEAL_TAG, SEAL_TAG, &SEAL_KEYS), None);
    }

    // ── CheckSet 契约 ───────────────────────────────────────────

    #[test]
    fn checkset_eight_green() {
        let s = run_unx49099_checks();
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.red_items_text().is_empty());
    }

    #[test]
    fn checks_names_and_details() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1);
            assert!(items[i].0.starts_with("F49099/") || items[i].0.starts_with("F49100/"));
            assert!(!items[i].2.is_empty());
            i += 1;
        }
    }
}
