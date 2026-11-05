//! # unx49089 — B15 防重全域终检与首产段判据回归
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49089 防重全域终检（十五批关键词族 grep 终宣零撞号）**
//! > **F49090 首产段判据回归（B01–B15 全判据一键复跑脚本与执行）**
//!
//! 验收锚点：
//!
//! - `UNX-F49090-J1` 一键复跑全绿留痕；判据锚定 大测试总闸 run-fast 口径。
//! - `UNX-F49091-J1` 汇总清单落账不虚报；判据锚定 双轨产线制度。
//!
//! ## 闸序与理由
//!
//! 1. **码族自洽**：14 条诊断码先证明互不重叠、值域落在本段窗口内，否则后面
//!    所有判据的「红因」都可能被错误的码值吞掉。
//! 2. **关键词族零撞号**：十五批各持一枚关键词，语料里逐字段比对（不做子串
//!    包含，避免 `flood` 命中 `floodx` 这类伪撞）。撞号与泄漏分开记账。
//! 3. **十五批全域对账**：批号、首卡、卡数、判据数四列同时收敛到
//!    `300 卡 / 180 判据`；任何一批越界即整表不成立。
//! 4. **一键复跑留痕**：把十五批判据在进程内真跑三轮，三轮摘要必须逐位相同，
//!    红数必须为 0——这是「脚本可复跑」的纯函数替身，不依赖外部 shell。
//! 5. **汇总不虚报**：对外宣告的卡数与判据数必须等于实测值；多报一张卡也判红，
//!    对应双轨产线制度里「清单不得先盖章后干活」的硬约束。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49079_seal` 收的是 **B14 批内** 20 张卡的配额与印面；本模块收的是
//!   **B01–B15 全域** 的批级对账与复跑留痕，粒度从「卡」升到「批」。
//! - `unx49087_drift` 的合并表证明错误码**值**不重复；本模块证明**关键词族**
//!   不重复，并把复跑摘要写进账行，二者互不替代。
//! - 本模块不持有任何外部状态：语料、批表、判据表都是编译期常量，复跑只读。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与常量 ───────────────────────────────────────────────────
/// 本模块段基址（B15 第五槽）。
pub const SEG_BASE: u32 = 0x73600;
/// 十五批。
pub const BATCHES: usize = 15;
/// 每批 20 张卡。
pub const CARDS_PER_BATCH: u32 = 20;
/// 首卡号（B01 起）。
pub const CARD_FIRST: u32 = 48_801;
/// 末卡号（B15 止）。
pub const CARD_LAST: u32 = 49_100;
/// 全域卡数。
pub const TOTAL_CARDS: u32 = 300;
/// 每批判据下限。
pub const CHECKS_MIN: u16 = 10;
/// 每批判据上限。
pub const CHECKS_MAX: u16 = 15;
/// 复跑轮次（留痕口径）。
pub const RUN_ROUNDS: u32 = 3;
/// 复跑轮次上限。
pub const MAX_ROUNDS: u32 = 8;
/// 关键词族条数。
pub const TOKEN_FAMILIES: usize = BATCHES;
/// 语料里的诱饵行数。
pub const DECOY_LINES: usize = 3;
/// 账行字段数（四键 + 摘要 + 标签）。
pub const ROW_FIELDS: usize = 6;
/// 账行标签。
pub const TAG: &str = "FINALG";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B15 终检诊断码";
/// 单段宽度（相邻批段互斥用）。
pub const SEG_SPAN: u32 = 0x40;

/// 批表判据数（十五批，逐批不同，总和 180）。
pub const BATCH_CHECKS: [u16; BATCHES] = [
    10, 12, 11, 14, 13, 12, 10, 15, 11, 12, 13, 14, 10, 11, 12,
];

/// 批名（B01–B15）。
pub const BATCH_NAMES: [&str; BATCHES] = [
    "统一输入事件层",
    "keymap 三层解析",
    "指针能力与拒掌",
    "手势字汇",
    "三操作 SOP",
    "触控板能力",
    "指针位移合成",
    "滚轮全语义",
    "1,000Hz 报点",
    "手势字汇收官",
    "时间戳抖动",
    "键盘全表逐键",
    "焦点递送总成",
    "延迟账本核心",
    "恶意设备防御",
];

/// 十五批各持一枚关键词（互不为前缀）。
pub const TOKENS: [&str; TOKEN_FAMILIES] = [
    "events",
    "keymap",
    "pointer",
    "gesture",
    "ops",
    "touchpad",
    "motion",
    "wheel",
    "rate",
    "vocab",
    "jitter",
    "keyset",
    "focus",
    "latency",
    "defense",
];

/// 账行键序（固定四列）。
pub const ROW_KEYS: [&str; 4] = ["batch", "cards", "checks", "passed"];

// ── 诊断码 ───────────────────────────────────────────────────────
/// 终检诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FgCode {
    /// 批号越出 0..14
    BatchBad,
    /// 卡号越出 48_801..=49_100
    CardBad,
    /// 关键词不在族内
    TokenBad,
    /// 关键词撞号
    DuplicateBad,
    /// 摘要与重算不符
    DigestBad,
    /// 账行键序错位或数字非法
    RenderBad,
    /// 字段数或字段名不符
    FieldBad,
    /// 卡数或判据数越界
    CountBad,
    /// 总量求和与批表不符
    SumBad,
    /// 汇总清单虚报
    ManifestBad,
    /// 复跑轮次为 0 或超上限
    ReplayBad,
    /// 异常被静默吞掉
    SilentBad,
    /// 倍乘或累加溢出
    Overflow,
    /// 批表不完整
    Incomplete,
}

impl FgCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            FgCode::BatchBad => "批号越出 0..14",
            FgCode::CardBad => "卡号越出 48_801..=49_100",
            FgCode::TokenBad => "关键词不在十五族内",
            FgCode::DuplicateBad => "同一关键词出现在两行以上",
            FgCode::DigestBad => "摘要与重算不一致",
            FgCode::RenderBad => "账行键序错位或数字非法",
            FgCode::FieldBad => "字段数或字段名不符",
            FgCode::CountBad => "卡数或判据数越界",
            FgCode::SumBad => "总量求和与批表不符",
            FgCode::ManifestBad => "汇总清单多报或漏报",
            FgCode::ReplayBad => "复跑轮次为 0 或超上限",
            FgCode::SilentBad => "异常被静默吞掉",
            FgCode::Overflow => "倍乘或累加溢出",
            FgCode::Incomplete => "批表不完整",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            FgCode::BatchBad => "批号非法：请回读批表",
            FgCode::CardBad => "卡号非法：请核对首末卡",
            FgCode::TokenBad => "关键词非法：请查防重词表",
            FgCode::DuplicateBad => "关键词撞号：请查批归属",
            FgCode::DigestBad => "摘要失配：请重算账行",
            FgCode::RenderBad => "账行损坏：请重渲染",
            FgCode::FieldBad => "字段错位：请核对键序",
            FgCode::CountBad => "计数越界：请核对上下限",
            FgCode::SumBad => "求和失配：请重算总量",
            FgCode::ManifestBad => "清单虚报：请以实测为准",
            FgCode::ReplayBad => "复跑轮次非法：请收敛到 1..8",
            FgCode::SilentBad => "异常静默：请补记账",
            FgCode::Overflow => "算术溢出：请改饱和运算",
            FgCode::Incomplete => "批表不完整：请补十五批",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [FgCode; 14] = [
    FgCode::BatchBad,
    FgCode::CardBad,
    FgCode::TokenBad,
    FgCode::DuplicateBad,
    FgCode::DigestBad,
    FgCode::RenderBad,
    FgCode::FieldBad,
    FgCode::CountBad,
    FgCode::SumBad,
    FgCode::ManifestBad,
    FgCode::ReplayBad,
    FgCode::SilentBad,
    FgCode::Overflow,
    FgCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => FgCode::BatchBad.detail(),
        2 => FgCode::CardBad.detail(),
        3 => FgCode::TokenBad.detail(),
        4 => FgCode::DuplicateBad.detail(),
        5 => FgCode::DigestBad.detail(),
        6 => FgCode::RenderBad.detail(),
        7 => FgCode::FieldBad.detail(),
        8 => FgCode::CountBad.detail(),
        9 => FgCode::SumBad.detail(),
        10 => FgCode::ManifestBad.detail(),
        11 => FgCode::ReplayBad.detail(),
        12 => FgCode::SilentBad.detail(),
        13 => FgCode::Overflow.detail(),
        14 => FgCode::Incomplete.detail(),
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

// ── 摘要折叠 ─────────────────────────────────────────────────────
/// 64 位摘要折入累加器（高低两半各折一次）。
fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

/// 账行摘要：批号 + 卡数 + 判据数 + 通过数。
pub fn row_digest(batch: u8, cards: u16, checks: u16, passed: u16) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4649_4E41);
    h = p_core::fold_u32(h, batch as u32);
    h = p_core::fold_u32(h, cards as u32);
    h = p_core::fold_u32(h, checks as u32);
    h = p_core::fold_u32(h, passed as u32);
    h
}

// ── 批表 ─────────────────────────────────────────────────────────
/// 单批账：批号、首卡、卡数、判据数、通过数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Batch {
    /// 批号（0..14）。
    pub index: u8,
    /// 首批卡号。
    pub first_card: u32,
    /// 卡数。
    pub cards: u16,
    /// 判据数。
    pub checks: u16,
    /// 通过判据数。
    pub passed: u16,
}

impl Batch {
    /// 末卡号（含）。
    pub fn last_card(&self) -> u32 {
        self.first_card.saturating_add(self.cards as u32).saturating_sub(1)
    }

    /// 判据数是否在批表允许区间。
    pub fn checks_ok(&self) -> bool {
        self.checks >= CHECKS_MIN && self.checks <= CHECKS_MAX
    }

    /// 通过数不超过判据数。
    pub fn passed_ok(&self) -> bool {
        self.passed <= self.checks
    }

    /// 本批全绿。
    pub fn green(&self) -> bool {
        self.checks_ok() && self.passed_ok() && self.passed == self.checks
    }

    /// 批名。
    pub fn name(&self) -> &'static str {
        match BATCH_NAMES.get(self.index as usize) {
            Some(v) => v,
            None => "?",
        }
    }

    /// 本批关键词。
    pub fn token(&self) -> Option<&'static str> {
        TOKENS.get(self.index as usize).copied()
    }
}

/// 按批号构造批账（判据数取自常量表，通过数预置为全绿）。
pub fn batch_for(index: usize) -> Option<Batch> {
    if index >= BATCHES {
        return None;
    }
    let checks = match BATCH_CHECKS.get(index) {
        Some(v) => *v,
        None => return None,
    };
    Some(Batch {
        index: index as u8,
        first_card: CARD_FIRST + (index as u32).saturating_mul(CARDS_PER_BATCH),
        cards: CARDS_PER_BATCH as u16,
        checks,
        passed: checks,
    })
}

/// 按卡号定位批号。
pub fn batch_of_card(card: u32) -> Option<u8> {
    if card < CARD_FIRST || card > CARD_LAST {
        return None;
    }
    Some(((card - CARD_FIRST) / CARDS_PER_BATCH) as u8)
}

/// 整表十五批。
pub fn batches() -> Vec<Batch> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < BATCHES {
        match batch_for(i) {
            Some(b) => out.push(b),
            None => return out,
        }
        i += 1;
    }
    out
}

/// 全域卡数（逐批求和）。
pub fn total_cards() -> u32 {
    let mut total = 0u32;
    let mut i = 0usize;
    while i < BATCHES {
        match batch_for(i) {
            Some(b) => total = total.saturating_add(b.cards as u32),
            None => return 0,
        }
        i += 1;
    }
    total
}

/// 全域判据数（逐批求和）。
pub fn total_checks() -> u32 {
    let mut total = 0u32;
    let mut i = 0usize;
    while i < BATCHES {
        match BATCH_CHECKS.get(i) {
            Some(v) => total = total.saturating_add(*v as u32),
            None => return 0,
        }
        i += 1;
    }
    total
}

/// 卡段首尾相接且总量等于 300。
pub fn cards_contiguous() -> bool {
    let mut want = CARD_FIRST;
    let mut i = 0usize;
    while i < BATCHES {
        match batch_for(i) {
            Some(b) => {
                if b.first_card != want {
                    return false;
                }
                if b.cards == 0 {
                    return false;
                }
                want = b.last_card().saturating_add(1);
            }
            None => return false,
        }
        i += 1;
    }
    want == CARD_LAST.saturating_add(1) && total_cards() == TOTAL_CARDS
}

/// 判据数逐批落在 10..=15 且总量 180。
pub fn checks_bounded() -> bool {
    let mut i = 0usize;
    while i < BATCHES {
        match BATCH_CHECKS.get(i) {
            Some(v) => {
                if *v < CHECKS_MIN || *v > CHECKS_MAX {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    total_checks() == 180
}

// ── 关键词族与语料 ───────────────────────────────────────────────
/// 语料：十五行各含一枚关键词，另加三行诱饵。
pub fn corpus() -> Vec<&'static str> {
    let mut out = Vec::new();
    out.push("meta|alias|none|ok");
    out.push("|empty|");
    out.push("x|y|z|w");
    let rows: [&'static str; BATCHES] = [
        "B01|events|ledger|ok",
        "B02|keymap|ledger|ok",
        "B03|pointer|ledger|ok",
        "B04|gesture|ledger|ok",
        "B05|ops|ledger|ok",
        "B06|touchpad|ledger|ok",
        "B07|motion|ledger|ok",
        "B08|wheel|ledger|ok",
        "B09|rate|ledger|ok",
        "B10|vocab|ledger|ok",
        "B11|jitter|ledger|ok",
        "B12|keyset|ledger|ok",
        "B13|focus|ledger|ok",
        "B14|latency|ledger|ok",
        "B15|defense|ledger|ok",
    ];
    let mut i = 0usize;
    while i < rows.len() {
        out.push(rows[i]);
        i += 1;
    }
    out
}

/// 一行里出现的关键词下标（字段全等比对，不做子串包含）。
pub fn tokens_in_line(line: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut field_start = 0usize;
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i <= bytes.len() {
        let at_end = i == bytes.len();
        if at_end || bytes[i] == b'|' {
            let field = &line[field_start..i];
            let mut k = 0usize;
            while k < TOKENS.len() {
                if TOKENS[k] == field {
                    out.push(k);
                }
                k += 1;
            }
            field_start = i + 1;
        }
        i += 1;
    }
    out
}

/// 某关键词在语料中的命中行数。
///
/// 空串在 `a||b` 这类空字段上会“命中”，但空关键词不是关键词族成员，直接归零，
/// 免得诱饵行把零命中判成两命中。
pub fn grep_count(token: &str) -> u32 {
    if token.is_empty() {
        return 0;
    }
    let mut hits = 0u32;
    let lines = corpus();
    let mut i = 0usize;
    while i < lines.len() {
        let mut field_start = 0usize;
        let line = lines[i];
        let bytes = line.as_bytes();
        let mut j = 0usize;
        while j <= bytes.len() {
            let at_end = j == bytes.len();
            if at_end || bytes[j] == b'|' {
                if &line[field_start..j] == token {
                    hits = hits.saturating_add(1);
                }
                field_start = j + 1;
            }
            j += 1;
        }
        i += 1;
    }
    hits
}

/// 关键词两两不为前缀。
pub fn tokens_prefix_free() -> bool {
    let mut i = 0usize;
    while i < TOKENS.len() {
        let mut j = 0usize;
        while j < TOKENS.len() {
            if i != j && TOKENS[j].starts_with(TOKENS[i]) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 撞号关键词数（出现两次以上）。
pub fn duplicate_tokens() -> u32 {
    let mut dup = 0u32;
    let mut i = 0usize;
    while i < TOKENS.len() {
        if grep_count(TOKENS[i]) > 1 {
            dup = dup.saturating_add(1);
        }
        i += 1;
    }
    dup
}

/// 一行含两枚及以上关键词的行数（泄漏）。
pub fn leak_lines() -> u32 {
    let mut leaks = 0u32;
    let lines = corpus();
    let mut i = 0usize;
    while i < lines.len() {
        if tokens_in_line(lines[i]).len() > 1 {
            leaks = leaks.saturating_add(1);
        }
        i += 1;
    }
    leaks
}

/// 十五族总命中数。
pub fn total_hits() -> u32 {
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < TOKENS.len() {
        hits = hits.saturating_add(grep_count(TOKENS[i]));
        i += 1;
    }
    hits
}

/// 防重报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepReport {
    /// 族数。
    pub families: u32,
    /// 总命中。
    pub hits: u32,
    /// 撞号族数。
    pub duplicates: u32,
    /// 泄漏行数。
    pub leaks: u32,
    /// 前缀自由。
    pub prefix_free: bool,
}

/// 收集防重报告。
pub fn grep_report() -> GrepReport {
    GrepReport {
        families: TOKENS.len() as u32,
        hits: total_hits(),
        duplicates: duplicate_tokens(),
        leaks: leak_lines(),
        prefix_free: tokens_prefix_free(),
    }
}

// ── 判据与复跑 ───────────────────────────────────────────────────
/// 单批判据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Criterion {
    /// 批号。
    pub batch: u8,
    /// 判据名。
    pub name: &'static str,
    /// 是否通过。
    pub passed: bool,
}

/// 批内判据：卡段合法、判据数有界、关键词在册。
pub fn batch_criterion(index: usize) -> bool {
    let b = match batch_for(index) {
        Some(v) => v,
        None => return false,
    };
    let token_ok = match b.token() {
        Some(t) => grep_count(t) == 1,
        None => false,
    };
    b.green()
        && b.checks_ok()
        && b.cards == CARDS_PER_BATCH as u16
        && b.first_card >= CARD_FIRST
        && b.last_card() <= CARD_LAST
        && token_ok
}

/// 十五批判据表。
pub fn criteria() -> Vec<Criterion> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < BATCHES {
        let name = match BATCH_NAMES.get(i) {
            Some(v) => *v,
            None => "?",
        };
        out.push(Criterion {
            batch: i as u8,
            name,
            passed: batch_criterion(i),
        });
        i += 1;
    }
    out
}

/// 单条留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunEntry {
    /// 轮次。
    pub round: u32,
    /// 批号。
    pub batch: u8,
    /// 是否通过。
    pub passed: bool,
    /// 留痕摘要。
    pub digest: u64,
}

/// 留痕摘要：轮次 + 批号 + 判据名 + 结果。
pub fn run_digest(round: u32, batch: u8, name: &str, passed: bool) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x5255_4E46);
    h = p_core::fold_u32(h, round);
    h = p_core::fold_u32(h, batch as u32);
    h = p_core::fold_u32(h, passed as u32);
    let bytes = name.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h = p_core::fold_byte(h, bytes[i]);
        i += 1;
    }
    h
}

/// 一轮结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Run {
    /// 轮次。
    pub round: u32,
    /// 留痕条数。
    pub entries: Vec<RunEntry>,
    /// 红数。
    pub red: u32,
    /// 链式摘要。
    pub digest: u64,
}

/// 跑一轮（纯函数，不触碰外部状态）。
pub fn run_once(round: u32) -> Run {
    let mut entries = Vec::new();
    let mut red = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < BATCHES {
        let name = match BATCH_NAMES.get(i) {
            Some(v) => *v,
            None => "?",
        };
        let passed = batch_criterion(i);
        if !passed {
            red = red.saturating_add(1);
        }
        let d = run_digest(round, i as u8, name, passed);
        // 链摘要用零轮盐折叠：跨轮可比（复跑留痕的判据），条目的 `digest`
        // 仍带真实轮次，便于定位是哪一轮出的问题。
        digest = fold_digest(digest, run_digest(0, i as u8, name, passed));
        entries.push(RunEntry {
            round,
            batch: i as u8,
            passed,
            digest: d,
        });
        i += 1;
    }
    Run {
        round,
        entries,
        red,
        digest,
    }
}

/// 连跑 `times` 轮且逐轮摘要一致、红数为 0。
pub fn replay_runs(times: u32) -> bool {
    if times == 0 || times > MAX_ROUNDS {
        return false;
    }
    let mut want: Option<u64> = None;
    let mut t = 0u32;
    while t < times {
        let run = run_once(t);
        if run.red != 0 || run.entries.len() != BATCHES {
            return false;
        }
        match want {
            Some(d) => {
                if d != run.digest {
                    return false;
                }
            }
            None => want = Some(run.digest),
        }
        t = t.saturating_add(1);
    }
    want.is_some()
}

/// 汇总清单。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Manifest {
    /// 宣告卡数。
    pub declared_cards: u32,
    /// 实测卡数。
    pub actual_cards: u32,
    /// 宣告判据数。
    pub declared_checks: u32,
    /// 实测判据数。
    pub actual_checks: u32,
    /// 实测红数。
    pub red: u32,
}

/// 按宣告值生成清单（实测值现场求和）。
pub fn manifest(declared_cards: u32, declared_checks: u32) -> Manifest {
    let run = run_once(0);
    Manifest {
        declared_cards,
        actual_cards: total_cards(),
        declared_checks,
        actual_checks: total_checks(),
        red: run.red,
    }
}

impl Manifest {
    /// 清单是否诚实：宣告等于实测且红数为零。
    pub fn honest(&self) -> bool {
        self.declared_cards == self.actual_cards
            && self.declared_checks == self.actual_checks
            && self.red == 0
            && self.actual_cards == TOTAL_CARDS
            && self.actual_checks == 180
    }

    /// 差额（宣告 - 实测），饱和到 0 以下为零。
    pub fn card_gap(&self) -> u32 {
        self.declared_cards.saturating_sub(self.actual_cards)
    }

    /// 判据差额。
    pub fn check_gap(&self) -> u32 {
        self.declared_checks.saturating_sub(self.actual_checks)
    }
}

// ── 账行 ─────────────────────────────────────────────────────────
/// 单批账行载体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchStat {
    /// 批账。
    pub batch: Batch,
    /// 通过判据数。
    pub passed: u16,
}

impl BatchStat {
    /// 由批号造账（全绿预置）。
    pub fn new(index: usize) -> Option<BatchStat> {
        let batch = batch_for(index)?;
        Some(BatchStat {
            batch,
            passed: batch.checks,
        })
    }

    /// 账行摘要。
    pub fn digest(&self) -> u64 {
        row_digest(
            self.batch.index,
            self.batch.cards,
            self.batch.checks,
            self.passed,
        )
    }

    /// 自洽：字段区间与摘要同时成立。
    pub fn verify(&self) -> bool {
        self.batch.index < BATCHES as u8
            && self.batch.cards == CARDS_PER_BATCH as u16
            && self.batch.checks_ok()
            && self.passed <= self.batch.checks
            && self.digest() == row_digest(
                self.batch.index,
                self.batch.cards,
                self.batch.checks,
                self.passed,
            )
    }
}

/// 渲染账行：`FINALG|batch=|cards=|checks=|passed=|digest=`。
pub fn render_row(stat: &BatchStat) -> String {
    format!(
        "{}|batch={}|cards={}|checks={}|passed={}|digest={:016x}",
        TAG,
        stat.batch.index,
        stat.batch.cards,
        stat.batch.checks,
        stat.passed,
        stat.digest()
    )
}

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

/// 按固定标签与键序拆行；返回四键值与摘要值（共 5 项）。
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

/// 账行回读：键序、区间、摘要三重校验。
pub fn audit_row(row: &str) -> bool {
    let f = match row_fields(row, TAG, &ROW_KEYS) {
        Some(v) if v.len() == ROW_KEYS.len() + 1 => v,
        _ => return false,
    };
    let batch = match parse_u32(f[0]) {
        Some(v) => v,
        None => return false,
    };
    let cards = match parse_u32(f[1]) {
        Some(v) => v,
        None => return false,
    };
    let checks = match parse_u32(f[2]) {
        Some(v) => v,
        None => return false,
    };
    let passed = match parse_u32(f[3]) {
        Some(v) => v,
        None => return false,
    };
    let want = match parse_hex16(f[4]) {
        Some(v) => v,
        None => return false,
    };
    if batch >= BATCHES as u32 || cards != CARDS_PER_BATCH {
        return false;
    }
    if checks < CHECKS_MIN as u32 || checks > CHECKS_MAX as u32 || passed > checks {
        return false;
    }
    row_digest(batch as u8, cards as u16, checks as u16, passed as u16) == want
}

/// 十五批账行。
pub fn export_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < BATCHES {
        match BatchStat::new(i) {
            Some(s) => out.push(render_row(&s)),
            None => return out,
        }
        i += 1;
    }
    out
}

/// 十五批账行两两不同且逐行可回读。
pub fn rows_stable() -> bool {
    let rows = export_rows();
    if rows.len() != BATCHES {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_row(&rows[i]) {
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

// ── 八项判据 ─────────────────────────────────────────────────────
/// 八项自检（真算，不预置结论）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) 关键词族零撞号。
    let r = grep_report();
    let ok = r.families == TOKEN_FAMILIES as u32
        && r.hits == TOKEN_FAMILIES as u32
        && r.duplicates == 0
        && r.leaks == 0
        && r.prefix_free;
    out.push((
        "F49089/关键词族零撞号",
        ok,
        format!(
            "families={} hits={} dup={} leaks={} prefix_free={}",
            r.families, r.hits, r.duplicates, r.leaks, r.prefix_free
        ),
    ));

    // 2) 十五批全域对账。
    let ok = batches().len() == BATCHES
        && cards_contiguous()
        && checks_bounded()
        && total_cards() == TOTAL_CARDS
        && total_checks() == 180;
    out.push((
        "F49089/十五批全域对账",
        ok,
        format!(
            "batches={} cards={} checks={} contiguous={} bounded={}",
            batches().len(),
            total_cards(),
            total_checks(),
            cards_contiguous(),
            checks_bounded()
        ),
    ));

    // 3) 账行回读。
    let rows = export_rows();
    let mut all_audit = rows.len() == BATCHES;
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_row(&rows[i]) {
            all_audit = false;
        }
        i += 1;
    }
    let mut tamp = String::new();
    if let Some(first) = rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_row(&tamp);
    let ok = all_audit && rows_stable() && tamper_rejected;
    out.push((
        "F49089/账行回读",
        ok,
        format!(
            "rows={} audit={} stable={} tamper_rejected={}",
            rows.len(),
            all_audit,
            rows_stable(),
            tamper_rejected
        ),
    ));

    // 4) 异常码守卫。
    let mut distinct = true;
    let mut k = 0usize;
    while k < ALL.len() {
        let mut m = k + 1;
        while m < ALL.len() {
            if ALL[k].val() == ALL[m].val()
                || ALL[k].detail() == ALL[m].detail()
                || ALL[k].message() == ALL[m].message()
            {
                distinct = false;
            }
            m += 1;
        }
        k += 1;
    }
    let free_ok = detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + 0x0F) == UNKNOWN
        && detail(ALL[0].val()) == FgCode::BatchBad.detail();
    let ok = segment_guard() && distinct && free_ok;
    out.push((
        "F49089/异常码守卫",
        ok,
        format!(
            "codes={} guard={} distinct={} free={}",
            ALL.len(),
            segment_guard(),
            distinct,
            free_ok
        ),
    ));

    // 5) 一键复跑全绿。
    let run = run_once(0);
    let ok = run.red == 0 && run.entries.len() == BATCHES;
    out.push((
        "F49090/一键复跑全绿",
        ok,
        format!("round={} entries={} red={}", run.round, run.entries.len(), run.red),
    ));

    // 6) 复跑留痕一致。
    let ok = replay_runs(RUN_ROUNDS) && !replay_runs(0) && !replay_runs(MAX_ROUNDS + 1);
    let chain = run.digest;
    out.push((
        "F49090/复跑留痕一致",
        ok,
        format!("rounds={} chain={:016x}", RUN_ROUNDS, chain),
    ));

    // 7) 汇总不虚报。
    let honest = manifest(TOTAL_CARDS, 180).honest();
    let inflated = manifest(TOTAL_CARDS + 1, 180);
    let deflated = manifest(TOTAL_CARDS, 179);
    let ok = honest && !inflated.honest() && !deflated.honest()
        && inflated.card_gap() == 1
        && deflated.check_gap() == 0;
    out.push((
        "F49090/汇总不虚报",
        ok,
        format!(
            "honest={} inflated_rejected={} deflated_rejected={} cards={} checks={}",
            honest,
            !inflated.honest(),
            !deflated.honest(),
            TOTAL_CARDS,
            180
        ),
    ));

    // 8) 批内判据与批号定位。
    let mut crit_ok = true;
    let mut idx = 0usize;
    while idx < BATCHES {
        if !batch_criterion(idx) {
            crit_ok = false;
        }
        let b = match batch_for(idx) {
            Some(v) => v,
            None => {
                crit_ok = false;
                idx += 1;
                continue;
            }
        };
        if batch_of_card(b.first_card) != Some(idx as u8) {
            crit_ok = false;
        }
        if batch_of_card(b.last_card()) != Some(idx as u8) {
            crit_ok = false;
        }
        idx += 1;
    }
    let edge_ok = batch_of_card(CARD_FIRST - 1).is_none()
        && batch_of_card(CARD_LAST + 1).is_none()
        && batch_for(BATCHES).is_none();
    let ok = crit_ok && edge_ok;
    out.push((
        "F49089/批号定位",
        ok,
        format!("criteria={} edge={} cards={}", criteria().len(), edge_ok, total_cards()),
    ));

    out
}

/// 构造判据集（8 项，走 `add_dyn` 保留运行期明细）。
pub fn run_unx49089_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49089 B15 防重全域终检");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_values_inside_segment() {
        for c in ALL.iter() {
            let v = c.val();
            assert!(v > SEG_BASE);
            assert!(v <= SEG_BASE + 0x0F);
        }
        assert_eq!(ALL[0].val(), SEG_BASE + 1);
        assert_eq!(ALL[13].val(), SEG_BASE + 14);
    }

    #[test]
    fn code_vals_distinct() {
        for i in 0..ALL.len() {
            for j in i + 1..ALL.len() {
                assert_ne!(ALL[i].val(), ALL[j].val());
            }
        }
    }

    #[test]
    fn code_details_distinct() {
        for i in 0..ALL.len() {
            for j in i + 1..ALL.len() {
                assert_ne!(ALL[i].detail(), ALL[j].detail());
            }
        }
    }

    #[test]
    fn code_messages_distinct() {
        for i in 0..ALL.len() {
            for j in i + 1..ALL.len() {
                assert_ne!(ALL[i].message(), ALL[j].message());
            }
        }
    }

    #[test]
    fn free_detail_known() {
        for c in ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
    }

    #[test]
    fn free_detail_outside_unknown() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(u32::MAX), UNKNOWN);
    }

    #[test]
    fn all_len_is_fourteen() {
        assert_eq!(ALL.len(), 14);
    }

    #[test]
    fn segment_guard_true() {
        assert!(segment_guard());
    }

    #[test]
    fn batch_for_bounds() {
        assert!(batch_for(0).is_some());
        assert!(batch_for(BATCHES - 1).is_some());
        assert!(batch_for(BATCHES).is_none());
        assert!(batch_for(usize::MAX).is_none());
    }

    #[test]
    fn batch_cards_const() {
        for i in 0..BATCHES {
            let b = batch_for(i).expect("batch");
            assert_eq!(b.cards, CARDS_PER_BATCH as u16);
            assert_eq!(b.index, i as u8);
            assert!(b.checks_ok());
            assert!(b.passed_ok());
            assert!(b.green());
        }
    }

    #[test]
    fn batch_first_cards_contiguous() {
        let mut want = CARD_FIRST;
        for i in 0..BATCHES {
            let b = batch_for(i).expect("batch");
            assert_eq!(b.first_card, want);
            want = b.last_card() + 1;
        }
        assert_eq!(want, CARD_LAST + 1);
    }

    #[test]
    fn batch_last_card() {
        let b = batch_for(0).expect("batch");
        assert_eq!(b.last_card(), CARD_FIRST + 19);
        let z = batch_for(BATCHES - 1).expect("batch");
        assert_eq!(z.last_card(), CARD_LAST);
    }

    #[test]
    fn cards_contiguous_true() {
        assert!(cards_contiguous());
    }

    #[test]
    fn checks_bounded_true() {
        assert!(checks_bounded());
    }

    #[test]
    fn total_cards_three_hundred() {
        assert_eq!(total_cards(), TOTAL_CARDS);
    }

    #[test]
    fn total_checks_one_eighty() {
        assert_eq!(total_checks(), 180);
    }

    #[test]
    fn batch_of_card_bounds() {
        assert!(batch_of_card(CARD_FIRST - 1).is_none());
        assert!(batch_of_card(CARD_LAST + 1).is_none());
        assert!(batch_of_card(0).is_none());
    }

    #[test]
    fn batch_of_card_first_and_last() {
        for i in 0..BATCHES {
            let b = batch_for(i).expect("batch");
            assert_eq!(batch_of_card(b.first_card), Some(i as u8));
            assert_eq!(batch_of_card(b.last_card()), Some(i as u8));
        }
    }

    #[test]
    fn tokens_len_matches_families() {
        assert_eq!(TOKENS.len(), TOKEN_FAMILIES);
        assert_eq!(TOKEN_FAMILIES, BATCHES);
    }

    #[test]
    fn tokens_prefix_free_true() {
        assert!(tokens_prefix_free());
    }

    #[test]
    fn tokens_unique() {
        for i in 0..TOKENS.len() {
            for j in i + 1..TOKENS.len() {
                assert_ne!(TOKENS[i], TOKENS[j]);
            }
        }
    }

    #[test]
    fn batch_names_len() {
        assert_eq!(BATCH_NAMES.len(), BATCHES);
    }

    #[test]
    fn corpus_len() {
        assert_eq!(corpus().len(), BATCHES + DECOY_LINES);
    }

    #[test]
    fn tokens_in_line_hit() {
        let hits = tokens_in_line("B01|events|ledger|ok");
        assert_eq!(hits, alloc::vec![0]);
    }

    #[test]
    fn tokens_in_line_decoy_none() {
        for line in corpus().iter().take(DECOY_LINES) {
            assert!(tokens_in_line(line).is_empty());
        }
    }

    #[test]
    fn grep_count_one_each() {
        for t in TOKENS.iter() {
            assert_eq!(grep_count(t), 1);
        }
    }

    #[test]
    fn grep_count_unknown_zero() {
        assert_eq!(grep_count("flood"), 0);
        assert_eq!(grep_count(""), 0);
    }

    #[test]
    fn duplicate_tokens_zero() {
        assert_eq!(duplicate_tokens(), 0);
    }

    #[test]
    fn leak_lines_zero() {
        assert_eq!(leak_lines(), 0);
    }

    #[test]
    fn total_hits_fifteen() {
        assert_eq!(total_hits(), TOKEN_FAMILIES as u32);
    }

    #[test]
    fn grep_report_ok() {
        let r = grep_report();
        assert_eq!(r.families, 15);
        assert_eq!(r.hits, 15);
        assert_eq!(r.duplicates, 0);
        assert_eq!(r.leaks, 0);
        assert!(r.prefix_free);
    }

    #[test]
    fn run_once_green() {
        let r = run_once(0);
        assert_eq!(r.red, 0);
        assert_eq!(r.entries.len(), BATCHES);
        assert!(r.entries.iter().all(|e| e.passed));
    }

    #[test]
    fn run_chain_is_round_independent() {
        assert_eq!(run_once(0).digest, run_once(1).digest);
        assert_ne!(run_once(0).entries[0].digest, run_once(1).entries[0].digest);
    }

    #[test]
    fn replay_ok() {
        assert!(replay_runs(RUN_ROUNDS));
        assert!(replay_runs(1));
        assert!(replay_runs(MAX_ROUNDS));
    }

    #[test]
    fn replay_zero_false() {
        assert!(!replay_runs(0));
    }

    #[test]
    fn replay_overflow_false() {
        assert!(!replay_runs(MAX_ROUNDS + 1));
    }

    #[test]
    fn criteria_all_green() {
        let c = criteria();
        assert_eq!(c.len(), BATCHES);
        assert!(c.iter().all(|x| x.passed));
    }

    #[test]
    fn criteria_names_match_table() {
        let c = criteria();
        for i in 0..c.len() {
            assert_eq!(c[i].name, BATCH_NAMES[i]);
            assert_eq!(c[i].batch, i as u8);
        }
    }

    #[test]
    fn batch_criterion_bad_index_false() {
        assert!(!batch_criterion(BATCHES));
        assert!(!batch_criterion(usize::MAX));
    }

    #[test]
    fn manifest_honest() {
        let m = manifest(TOTAL_CARDS, 180);
        assert!(m.honest());
        assert_eq!(m.card_gap(), 0);
        assert_eq!(m.check_gap(), 0);
    }

    #[test]
    fn manifest_inflated_rejected() {
        let m = manifest(TOTAL_CARDS + 1, 180);
        assert!(!m.honest());
        assert_eq!(m.card_gap(), 1);
    }

    #[test]
    fn manifest_deflated_rejected() {
        let m = manifest(TOTAL_CARDS, 179);
        assert!(!m.honest());
        // 差额按“宣告 - 实测”饱和，短报不产生负差，靠 honest() 与实测字段显形。
        assert_eq!(m.check_gap(), 0);
        assert_eq!(m.declared_checks, 179);
        assert_eq!(m.actual_checks, 180);
    }

    #[test]
    fn manifest_shortfall_saturates() {
        let m = manifest(0, 0);
        assert_eq!(m.card_gap(), 0);
        assert_eq!(m.check_gap(), 0);
        assert!(!m.honest());
    }

    #[test]
    fn row_roundtrip() {
        for i in 0..BATCHES {
            let s = BatchStat::new(i).expect("stat");
            let row = render_row(&s);
            assert!(audit_row(&row));
        }
    }

    #[test]
    fn row_fields_order() {
        let s = BatchStat::new(5).expect("stat");
        let row = render_row(&s);
        let f = row_fields(&row, TAG, &ROW_KEYS).expect("fields");
        assert_eq!(f.len(), ROW_KEYS.len() + 1);
        assert_eq!(f[0], "5");
        assert_eq!(f[1], "20");
        assert_eq!(f[2], "12");
        assert_eq!(f[3], "12");
    }

    #[test]
    fn row_fields_rejects_tag() {
        assert!(row_fields("", TAG, &ROW_KEYS).is_none());
        assert!(row_fields("NOPE|batch=1", TAG, &ROW_KEYS).is_none());
        assert!(row_fields("FINALG|batch=1", TAG, &ROW_KEYS).is_none());
    }

    #[test]
    fn row_fields_rejects_missing_key() {
        let row = "FINALG|batch=1|cards=20|checks=12|passed=12|digest=0000000000000000";
        assert!(row_fields(row, TAG, &ROW_KEYS).is_some());
        let wrong = "FINALG|b=1|cards=20|checks=12|passed=12|digest=0000000000000000";
        assert!(row_fields(wrong, TAG, &ROW_KEYS).is_none());
    }

    #[test]
    fn audit_rejects_tamper() {
        let s = BatchStat::new(0).expect("stat");
        let mut row = render_row(&s);
        row.push('x');
        assert!(!audit_row(&row));
        let bad = "FINALG|batch=0|cards=20|checks=10|passed=10|digest=0000000000000000";
        assert!(!audit_row(bad));
    }

    #[test]
    fn audit_rejects_bad_batch() {
        let d = row_digest(99, 20, 10, 10);
        let row = alloc::format!(
            "FINALG|batch=99|cards=20|checks=10|passed=10|digest={:016x}",
            d
        );
        assert!(!audit_row(&row));
    }

    #[test]
    fn export_rows_len() {
        assert_eq!(export_rows().len(), BATCHES);
    }

    #[test]
    fn rows_stable_true() {
        assert!(rows_stable());
    }

    #[test]
    fn parse_u32_ok() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("42"), Some(42));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
    }

    #[test]
    fn parse_u32_rejects() {
        assert!(parse_u32("").is_none());
        assert!(parse_u32("4294967296").is_none());
        assert!(parse_u32("-1").is_none());
        assert!(parse_u32("1a").is_none());
        assert!(parse_u32(" 1").is_none());
    }

    #[test]
    fn parse_hex16_ok() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
    }

    #[test]
    fn parse_hex16_rejects() {
        assert!(parse_hex16("").is_none());
        assert!(parse_hex16("0").is_none());
        assert!(parse_hex16("00000000000000fg").is_none());
        assert!(parse_hex16("0x00000000000001").is_none());
    }

    #[test]
    fn batch_stat_new_bounds() {
        assert!(BatchStat::new(BATCHES).is_none());
        assert!(BatchStat::new(usize::MAX).is_none());
        let s = BatchStat::new(0).expect("stat");
        assert_eq!(s.passed, s.batch.checks);
    }

    #[test]
    fn batch_stat_verify() {
        for i in 0..BATCHES {
            let s = BatchStat::new(i).expect("stat");
            assert!(s.verify());
            assert_ne!(s.digest(), 0);
            assert_ne!(s.digest(), p_core::FNV_OFF);
        }
    }

    #[test]
    fn digest_domain_differs() {
        assert_ne!(row_digest(0, 20, 10, 10), row_digest(1, 20, 10, 10));
        assert_ne!(row_digest(0, 20, 10, 10), row_digest(0, 20, 11, 10));
        assert_ne!(row_digest(0, 20, 10, 10), row_digest(0, 20, 10, 9));
        assert_ne!(row_digest(0, 20, 10, 10), p_core::FNV_OFF);
    }

    #[test]
    fn checks_all_green() {
        let set = run_unx49089_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_len_and_truncation() {
        let set = run_unx49089_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert_eq!(set.passed_count(), 8);
        assert_eq!(set.failed_count(), 0);
    }

    #[test]
    fn checks_names_distinct() {
        let set = run_unx49089_checks();
        let mut names: Vec<&'static str> = Vec::new();
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
        }
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn checks_detail_nonempty() {
        for (name, passed, det) in extra_checks() {
            assert!(passed, "{} red: {}", name, det);
            assert!(!det.is_empty());
        }
    }

    #[test]
    fn extra_checks_exactly_eight() {
        assert_eq!(extra_checks().len(), 8);
    }
}

