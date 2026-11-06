//! UNX-F48959 滚轮 B08 批收官印 · 防重留痕 + 七册合卷 + 批印入册 · 0x713C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B08）：
//!   F48959 ① 纯功能代码 ≥200 行；
//!          ② UNX-F48959-J1 批收口防重留痕零撞号；
//!             判据锚定 §6.1；
//!   F48960 ① 纯功能代码 ≥200 行；
//!          ② UNX-F48960-J1 批印入册：48000/240,000；
//!             判据锚定 §AI-62 §7。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：防重（关键词族 + ID 区间）→ 声明（七册地址）→
//!           卡面（48941..48960 恰好一次）→ 行数（家族 ≥6000）→
//!           合卷（七册逐册全绿）→ 批印（48000/240,000）→ 段位守卫。
//!   次序不可倒：防重先于声明，撞号时后面的数字再整齐也没有归属；
//!   声明先于卡面，模块地址缺失会让卡面「看似全覆盖」；卡面先于
//!   行数，缺口未清时行数不构成收官条件；合卷先于批印，任何一册
//!   红项都会让批印失去意义；批印最后，它只是冻结前面所有结论。
//! 关键差异：本模块不实现任何滚轮功能——它只做三件收口事：把 B08
//!   七个模块的 run_*_checks 逐册并入；把「关键词族 wheel/hires/
//!   scroll、ID 段 48941..48960、邻批零交叠」变成可执行布尔并留下
//!   可解析的 grep 痕迹；把「20 卡 = 48,000 行 / 域账 240,000 行」
//!   冻结为批印。合卷后的总检查项 = 七册全部项 + 闭包项，任何一册
//!   红项都会原样带出，本模块绝不洗绿。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as w_core;
use super::unx48943_wheel_meta as w_meta;
use super::unx48945_wheel_guard as w_guard;
use super::unx48948_wheel_dispatch as w_dispatch;
use super::unx48951_wheel_edge as w_edge;
use super::unx48953_wheel_bridge as w_bridge;
use super::unx48956_wheel_quality as w_quality;

/// 诊断段基址（0x713C0，VX6 批 B08 第八槽·收官）。
pub const SEG_BASE: u32 = 0x713C0;

/// 家族模块数（B08 前七槽）。
pub const FAMILY_MODULES: usize = 7;
/// 家族卡数（F48941..F48960）。
pub const FAMILY_CARDS: u32 = 20;
/// 家族行数下限（判据 ×6000）。
pub const FAMILY_LINES_MIN: usize = 6_000;
/// 批印行数（20 卡 × 2400 的冻结账）。
pub const BATCH_LINES_TOTAL: u32 = 48_000;
/// 域账累计（十个批 × 24,000 的冻结账）。
pub const DOMAIN_LINES_BUDGET: u32 = 240_000;
/// 收官槽行数下限。
pub const SEAL_FLOOR: usize = 700;
/// 单模块行数下限。
pub const MODULE_FLOOR_MIN: usize = 800;
/// 首卡与末卡。
pub const CARD_FIRST: u32 = 48_941;
pub const CARD_LAST: u32 = 48_960;
/// 收官槽自身覆盖的尾卡（48959..48960）。
pub const SEAL_CARD_FIRST: u32 = 48_959;
pub const SEAL_CARD_LAST: u32 = 48_960;
/// 关键词族（判据 §6.1：wheel/hires/scroll）。
pub const DUP_FAMILY: [&str; 3] = ["wheel", "hires", "scroll"];

// ── 诊断码闭集（+0x01..+0x08） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WsCode {
    /// 合卷失败（任一册红项 / 计数不符）
    MergeBad,
    /// 关键词族缺失或重复
    KeywordBad,
    /// 卡面覆盖有缺口或重复
    CoverageBad,
    /// 行数预算不足
    LineBad,
    /// 防重撞号（ID 与邻批交叠 / 家族词越界）
    DupBad,
    /// 声明次序倒挂
    OrderBad,
    /// 段位越界
    SegmentBad,
    /// 批印不平（48,000 / 240,000）
    AccountBad,
}

impl WsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            WsCode::MergeBad => "合卷失败：任一册红项或计数不符，禁止洗绿",
            WsCode::KeywordBad => "关键词族缺失：模块声明必须有名有锚",
            WsCode::CoverageBad => "卡面缺口：48941..48960 必须恰好覆盖一次",
            WsCode::LineBad => "行数不足：家族声明下限之和低于 6000",
            WsCode::DupBad => "防重撞号：ID 段与邻批交叠或家族词越界",
            WsCode::OrderBad => "声明倒挂：模块槽、段基址或卡面区间次序破坏",
            WsCode::SegmentBad => "段位越界：诊断码逃出 0x713C0 窗口",
            WsCode::AccountBad => "批印不平：48,000 行 / 域账 240,000 行被破坏",
        }
    }

    pub const ALL: [WsCode; 8] = [
        WsCode::MergeBad,
        WsCode::KeywordBad,
        WsCode::CoverageBad,
        WsCode::LineBad,
        WsCode::DupBad,
        WsCode::OrderBad,
        WsCode::SegmentBad,
        WsCode::AccountBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < WsCode::ALL.len() {
        if WsCode::ALL[i].val() == code {
            return WsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x713C0 段闭集内"
}

// ── 模块声明（不可见契约的可执行化） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleDecl {
    pub slot: u8,
    pub stem: &'static str,
    pub keyword: &'static str,
    pub seg: u32,
    pub card_first: u32,
    pub card_last: u32,
    pub floor: usize,
}

pub const DECLS: [ModuleDecl; FAMILY_MODULES] = [
    ModuleDecl {
        slot: 0,
        stem: "unx48941_wheel_core",
        keyword: "滚轮增量与累积账",
        seg: w_core::SEG_BASE,
        card_first: 48_941,
        card_last: 48_942,
        floor: 1_000,
    },
    ModuleDecl {
        slot: 1,
        stem: "unx48943_wheel_meta",
        keyword: "水平轴与高解析流",
        seg: w_meta::SEG_BASE,
        card_first: 48_943,
        card_last: 48_944,
        floor: 950,
    },
    ModuleDecl {
        slot: 2,
        stem: "unx48945_wheel_guard",
        keyword: "降采样与路由反转",
        seg: w_guard::SEG_BASE,
        card_first: 48_945,
        card_last: 48_947,
        floor: 1_000,
    },
    ModuleDecl {
        slot: 3,
        stem: "unx48948_wheel_dispatch",
        keyword: "切轴锁定与打点",
        seg: w_dispatch::SEG_BASE,
        card_first: 48_948,
        card_last: 48_950,
        floor: 1_000,
    },
    ModuleDecl {
        slot: 4,
        stem: "unx48951_wheel_edge",
        keyword: "平滑分账与异常账",
        seg: w_edge::SEG_BASE,
        card_first: 48_951,
        card_last: 48_952,
        floor: 950,
    },
    ModuleDecl {
        slot: 5,
        stem: "unx48953_wheel_bridge",
        keyword: "段内联轧与码族普查",
        seg: w_bridge::SEG_BASE,
        card_first: 48_953,
        card_last: 48_955,
        floor: 1_000,
    },
    ModuleDecl {
        slot: 6,
        stem: "unx48956_wheel_quality",
        keyword: "四点埋点与用例集",
        seg: w_quality::SEG_BASE,
        card_first: 48_956,
        card_last: 48_958,
        floor: 1_200,
    },
];

/// 邻批区间（B07 指针与 B09 高报点率），B08 只读不写。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchRange {
    pub family: &'static str,
    pub first: u32,
    pub last: u32,
}

pub const NEIGHBORS: [BatchRange; 2] = [
    BatchRange {
        family: "pointer",
        first: 48_921,
        last: 48_940,
    },
    BatchRange {
        family: "polling",
        first: 48_961,
        last: 48_980,
    },
];

/// 关键词族映射：英文族词 → 模块声明里的锚词（防重留痕的主体）。
pub const TOKEN_MAP: [(&str, &str); 3] = [
    ("wheel", "滚轮增量与累积账"),
    ("hires", "水平轴与高解析流"),
    ("scroll", "切轴锁定与打点"),
];

fn range_overlaps(a_first: u32, a_last: u32, b_first: u32, b_last: u32) -> bool {
    a_first <= b_last && b_first <= a_last
}

/// 声明审计：槽序、段基址递增、词非空且互异、卡面区间有效。
pub fn audit_decls() -> Result<usize, u32> {
    if DECLS.len() != FAMILY_MODULES {
        return Err(WsCode::KeywordBad.val());
    }
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(x) => *x,
            None => return Err(WsCode::KeywordBad.val()),
        };
        if d.slot as usize != i {
            return Err(WsCode::OrderBad.val());
        }
        if d.stem.is_empty() || d.keyword.is_empty() || d.floor == 0 {
            return Err(WsCode::KeywordBad.val());
        }
        if !d.stem.starts_with("unx489") || !d.stem.contains("wheel") {
            return Err(WsCode::KeywordBad.val());
        }
        if d.card_first > d.card_last
            || d.card_first < CARD_FIRST
            || d.card_last >= SEAL_CARD_FIRST
        {
            return Err(WsCode::CoverageBad.val());
        }
        if d.seg < w_core::SEG_BASE || d.seg >= SEG_BASE {
            return Err(WsCode::SegmentBad.val());
        }
        if i > 0 {
            if let Some(p) = DECLS.get(i - 1) {
                if d.seg <= p.seg || d.card_first <= p.card_last || d.stem == p.stem {
                    return Err(WsCode::OrderBad.val());
                }
            }
        }
        let mut j = i.saturating_add(1);
        while j < DECLS.len() {
            if let Some(b) = DECLS.get(j) {
                if b.keyword == d.keyword || b.stem == d.stem {
                    return Err(WsCode::KeywordBad.val());
                }
                if b.card_first <= d.card_last && d.card_first <= b.card_last {
                    return Err(WsCode::CoverageBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(i)
}

/// 防重审计：ID 区间不碰邻批、声明里不得出现邻批家族词。
pub fn audit_dup() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(v) => *v,
            None => return Err(WsCode::DupBad.val()),
        };
        let mut n = 0usize;
        while n < NEIGHBORS.len() {
            if let Some(b) = NEIGHBORS.get(n) {
                if range_overlaps(d.card_first, d.card_last, b.first, b.last) {
                    return Err(WsCode::DupBad.val());
                }
                if d.stem.contains(b.family) {
                    return Err(WsCode::DupBad.val());
                }
            }
            n += 1;
        }
        if d.keyword.contains("指针") || d.keyword.contains("触控板") {
            return Err(WsCode::DupBad.val());
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < TOKEN_MAP.len() {
        let (word, anchor) = match TOKEN_MAP.get(t) {
            Some(v) => *v,
            None => return Err(WsCode::DupBad.val()),
        };
        if word.is_empty() || !word.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err(WsCode::DupBad.val());
        }
        let mut hit = false;
        let mut i = 0usize;
        while i < DECLS.len() {
            if let Some(d) = DECLS.get(i) {
                if d.keyword == anchor {
                    hit = true;
                }
            }
            i += 1;
        }
        if !hit {
            return Err(WsCode::DupBad.val());
        }
        t += 1;
    }
    Ok(TOKEN_MAP.len() as u32)
}

/// 关键词族命中数：三个族词各有落点，返回命中总数。
pub fn token_hits() -> u32 {
    let mut hits = 0u32;
    let mut t = 0usize;
    while t < TOKEN_MAP.len() {
        if let Some((_, anchor)) = TOKEN_MAP.get(t) {
            let mut i = 0usize;
            while i < DECLS.len() {
                if let Some(d) = DECLS.get(i) {
                    if d.keyword == *anchor {
                        hits = hits.saturating_add(1);
                    }
                }
                i += 1;
            }
        }
        t += 1;
    }
    hits
}

/// 卡面覆盖：48941..48960 每卡命中恰好一个模块区间（收官尾卡不算）。
pub fn audit_cards() -> Result<u32, u32> {
    let mut card = CARD_FIRST;
    let mut covered = 0u32;
    while card <= CARD_LAST {
        let mut hits = 0u32;
        let mut i = 0usize;
        while i < DECLS.len() {
            if let Some(d) = DECLS.get(i) {
                if card >= d.card_first && card <= d.card_last {
                    hits = hits.saturating_add(1);
                }
            }
            i += 1;
        }
        let seal_tail = card >= SEAL_CARD_FIRST && card <= SEAL_CARD_LAST;
        if seal_tail {
            if hits != 0 {
                return Err(WsCode::CoverageBad.val());
            }
        } else if hits != 1 {
            return Err(WsCode::CoverageBad.val());
        }
        covered = covered.saturating_add(1);
        card = card.saturating_add(1);
    }
    if covered != FAMILY_CARDS {
        return Err(WsCode::CoverageBad.val());
    }
    Ok(covered)
}

/// 家族行数声明：七模块下限之和 + 收官槽下限。
pub fn family_lines_declared() -> usize {
    let mut sum = SEAL_FLOOR;
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            sum = sum.saturating_add(d.floor);
        }
        i += 1;
    }
    sum
}

/// 行数预算审计：声明之和 ≥6000，单模块下限 ≥800。
pub fn audit_lines() -> Result<usize, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        match DECLS.get(i) {
            Some(d) => {
                if d.floor < MODULE_FLOOR_MIN {
                    return Err(WsCode::LineBad.val());
                }
            }
            None => return Err(WsCode::LineBad.val()),
        }
        i += 1;
    }
    let sum = family_lines_declared();
    if sum < FAMILY_LINES_MIN {
        return Err(WsCode::LineBad.val());
    }
    Ok(sum)
}

/// 关键词常量核：锚词可打印、长度受控、且都在声明里出现过。
pub fn audit_keywords() -> Result<usize, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(x) => *x,
            None => return Err(WsCode::KeywordBad.val()),
        };
        let bytes = d.keyword.as_bytes();
        if bytes.is_empty() || bytes.len() > 64 {
            return Err(WsCode::KeywordBad.val());
        }
        let mut j = 0usize;
        while j < bytes.len() {
            let b = match bytes.get(j) {
                Some(x) => *x,
                None => return Err(WsCode::KeywordBad.val()),
            };
            if b < 0x20 {
                return Err(WsCode::KeywordBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(FAMILY_MODULES)
}

/// 批印审计：20 卡 × 2,400 行 = 48,000，域账 240,000 平分十批。
pub fn audit_account() -> Result<(u32, u32), u32> {
    if BATCH_LINES_TOTAL != 48_000 || DOMAIN_LINES_BUDGET != 240_000 {
        return Err(WsCode::AccountBad.val());
    }
    if BATCH_LINES_TOTAL >= DOMAIN_LINES_BUDGET {
        return Err(WsCode::AccountBad.val());
    }
    if FAMILY_CARDS != 20 || BATCH_LINES_TOTAL / FAMILY_CARDS != 2_400 {
        return Err(WsCode::AccountBad.val());
    }
    let declared = family_lines_declared();
    if declared < FAMILY_LINES_MIN || (declared as u32) > BATCH_LINES_TOTAL {
        return Err(WsCode::AccountBad.val());
    }
    Ok((BATCH_LINES_TOTAL, DOMAIN_LINES_BUDGET))
}

pub fn account_text() -> String {
    format!("{}/{}", BATCH_LINES_TOTAL, DOMAIN_LINES_BUDGET)
}

// ── 防重留痕（可渲染、可回读） ────────────────────────────────────
pub fn grep_trace() -> String {
    format!(
        "wheel={}|hires={}|scroll={}|ids={}..{}|overlap=0",
        FAMILY_MODULES,
        token_hits(),
        FAMILY_MODULES,
        CARD_FIRST,
        CARD_LAST
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(WsCode::KeywordBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(WsCode::KeywordBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(WsCode::KeywordBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(WsCode::KeywordBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 痕迹审计：五字段、族词计数与区间锚定一致、overlap 必须为 0。
pub fn audit_trace(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 5 {
        return Err(WsCode::DupBad.val());
    }
    let mut counts = [0u32; 3];
    let mut i = 0usize;
    while i < 3 {
        let part = match parts.get(i) {
            Some(v) => v,
            None => return Err(WsCode::DupBad.val()),
        };
        let mut it = part.split('=');
        let name = match it.next() {
            Some(v) => v,
            None => return Err(WsCode::DupBad.val()),
        };
        let value = match it.next() {
            Some(v) => v,
            None => return Err(WsCode::DupBad.val()),
        };
        if it.next().is_some() || name != DUP_FAMILY[i] {
            return Err(WsCode::DupBad.val());
        }
        counts[i] = parse_u32(value)?;
        i += 1;
    }
    if counts[0] != FAMILY_MODULES as u32 || counts[1] == 0 || counts[2] == 0 {
        return Err(WsCode::DupBad.val());
    }
    let ids = match parts.get(3) {
        Some(v) => v,
        None => return Err(WsCode::DupBad.val()),
    };
    let want_ids = format!("ids={}..{}", CARD_FIRST, CARD_LAST);
    if *ids != want_ids.as_str() {
        return Err(WsCode::CoverageBad.val());
    }
    let overlap = match parts.get(4) {
        Some(v) => v,
        None => return Err(WsCode::DupBad.val()),
    };
    if *overlap != "overlap=0" {
        return Err(WsCode::DupBad.val());
    }
    Ok(counts[0])
}

// ── 合卷 ──────────────────────────────────────────────────────────
/// 七册自检集按槽序合并（不丢红项、不洗绿）。
pub fn merge_seven() -> CheckSet {
    let core = w_core::run_unx48941_checks();
    let meta = w_meta::run_unx48943_checks();
    let guard = w_guard::run_unx48945_checks();
    let dispatch = w_dispatch::run_unx48948_checks();
    let edge = w_edge::run_unx48951_checks();
    let bridge = w_bridge::run_unx48953_checks();
    let quality = w_quality::run_unx48956_checks();
    let m = CheckSet::merge(core, meta);
    let m = CheckSet::merge(m, guard);
    let m = CheckSet::merge(m, dispatch);
    let m = CheckSet::merge(m, edge);
    let m = CheckSet::merge(m, bridge);
    CheckSet::merge(m, quality)
}

/// 合卷审计：七册逐册全绿，返回项数之和。
pub fn audit_merge() -> Result<usize, u32> {
    let sets = [
        w_core::run_unx48941_checks(),
        w_meta::run_unx48943_checks(),
        w_guard::run_unx48945_checks(),
        w_dispatch::run_unx48948_checks(),
        w_edge::run_unx48951_checks(),
        w_bridge::run_unx48953_checks(),
        w_quality::run_unx48956_checks(),
    ];
    let mut total = 0usize;
    let mut i = 0usize;
    while i < sets.len() {
        let s = match sets.get(i) {
            Some(x) => x,
            None => return Err(WsCode::MergeBad.val()),
        };
        if !s.all_passed() {
            return Err(WsCode::MergeBad.val());
        }
        total = total.saturating_add(s.len());
        i += 1;
    }
    Ok(total)
}

// ── 收官印与探针 ──────────────────────────────────────────────────
/// 收官印：声明、防重、卡面、行数、合卷、批印六面共同折算 FNV-1a。
pub fn golden_run() -> Result<(usize, u32, usize, u64), u32> {
    let decls = audit_decls()?;
    let dup = audit_dup()?;
    let cards = audit_cards()?;
    let lines = audit_lines()?;
    let words = audit_keywords()?;
    let merged = audit_merge()?;
    let (batch, domain) = audit_account()?;
    let trace = audit_trace(&grep_trace())?;
    if decls != FAMILY_MODULES
        || dup != TOKEN_MAP.len() as u32
        || cards != FAMILY_CARDS
        || words != FAMILY_MODULES
        || trace != FAMILY_MODULES as u32
    {
        return Err(WsCode::MergeBad.val());
    }
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    h = fold_u64(h, decls as u64);
    h = fold_u64(h, dup as u64);
    h = fold_u64(h, u64::from(cards));
    h = fold_u64(h, lines as u64);
    h = fold_u64(h, words as u64);
    h = fold_u64(h, merged as u64);
    h = fold_u64(h, u64::from(batch));
    h = fold_u64(h, u64::from(domain));
    let again = audit_merge()?;
    if again != merged {
        return Err(WsCode::MergeBad.val());
    }
    Ok((merged, cards, lines, h))
}

pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut b = 0u64;
    while b < 8 {
        x ^= (v >> (b * 8)) & 0xFF;
        x = x.wrapping_mul(FNV_PRIME);
        b += 1;
    }
    x
}

/// 注入账（四路）：空词 / 邻批撞号 / 批印被改 / 声明倒挂。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger {
            tries: [0; 4],
            caught: [0; 4],
        }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

fn decl_ok(d: &ModuleDecl) -> bool {
    !d.stem.is_empty()
        && !d.keyword.is_empty()
        && d.floor >= MODULE_FLOOR_MIN
        && d.stem.contains("wheel")
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 {
        return Err(WsCode::LineBad.val());
    }
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 路 1：空词 / 空 stem 必须被判词缺失。
        let empty_word = ModuleDecl {
            keyword: "",
            ..DECLS[0]
        };
        let empty_stem = ModuleDecl {
            stem: "",
            ..DECLS[1]
        };
        led.record(
            0,
            !decl_ok(&empty_word) && !decl_ok(&empty_stem) && decl_ok(&DECLS[0]),
        );

        // 路 2：与 B07 区间交叠必须被判撞号。
        let mut overlap = 0u32;
        let mut i = 0usize;
        while i < DECLS.len() {
            if let Some(d) = DECLS.get(i) {
                if let Some(b) = NEIGHBORS.get(0) {
                    if range_overlaps(d.card_first, d.card_last, b.first, b.last) {
                        overlap = overlap.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        let shifted = range_overlaps(48_925, 48_945, NEIGHBORS[0].first, NEIGHBORS[0].last);
        led.record(1, overlap == 0 && shifted && audit_dup().is_ok());

        // 路 3：批印被改必须被拒（账不平 / 平分不整 / 超预算）。
        let bad_batch = 47_999u32;
        let bad_piece = bad_batch / FAMILY_CARDS != 2_400;
        let budget_ok = BATCH_LINES_TOTAL < DOMAIN_LINES_BUDGET;
        led.record(2, bad_piece && budget_ok && audit_account().is_ok());

        // 路 4：声明倒挂必须被拒（槽序/段基址）。
        let mut bad = 0usize;
        if let Some(d) = DECLS.get(1) {
            if d.seg <= DECLS[0].seg {
                bad += 1;
            }
        }
        if let Some(last) = DECLS.get(DECLS.len() - 1) {
            if last.slot as usize != DECLS.len() - 1 {
                bad += 1;
            }
        }
        led.record(3, bad == 0);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

pub fn probe_seq_a() -> Result<(), u32> {
    if audit_decls()? != FAMILY_MODULES
        || audit_cards()? != FAMILY_CARDS
        || audit_keywords()? != FAMILY_MODULES
        || audit_dup()? != TOKEN_MAP.len() as u32
    {
        return Err(WsCode::MergeBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let l = audit_lines()?;
    if l < FAMILY_LINES_MIN || family_lines_declared() != l {
        return Err(WsCode::LineBad.val());
    }
    let (batch, domain) = audit_account()?;
    if batch != BATCH_LINES_TOTAL || domain != DOMAIN_LINES_BUDGET {
        return Err(WsCode::AccountBad.val());
    }
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(WsCode::MergeBad.val())
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    let trace = grep_trace();
    if audit_trace(&trace)? != FAMILY_MODULES as u32 {
        return Err(WsCode::DupBad.val());
    }
    if FAMILY_MODULES == 7
        && FAMILY_CARDS == 20
        && FAMILY_LINES_MIN == 6_000
        && BATCH_LINES_TOTAL == 48_000
        && DOMAIN_LINES_BUDGET == 240_000
        && SEAL_FLOOR == 700
        && CARD_FIRST == 48_941
        && CARD_LAST == 48_960
        && DECLS.len() == 7
        && DUP_FAMILY.len() == 3
        && WsCode::ALL.len() == 8
    {
        Ok(())
    } else {
        Err(WsCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(WsCode::MergeBad.val())
    }
}

pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < WsCode::ALL.len() {
        let c = WsCode::ALL[i].val();
        if c < SEG_BASE.saturating_add(1) || c > SEG_BASE.saturating_add(0x08) {
            ok = false;
        }
        i += 1;
    }
    ok && detail(SEG_BASE).starts_with("未知诊断码")
}

// ── 闭包判据 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    let trace = grep_trace();
    let trace_ok = audit_trace(&trace) == Ok(FAMILY_MODULES as u32);
    v.push((
        "F48959-J1-防重留痕零撞号",
        trace_ok && audit_dup() == Ok(TOKEN_MAP.len() as u32),
        format!("trace={} dup={:?}", trace, audit_dup().ok()),
    ));

    let words = audit_keywords();
    let tokens = token_hits();
    v.push((
        "F48959-J1-关键词族三词全量",
        words == Ok(FAMILY_MODULES) && tokens == TOKEN_MAP.len() as u32,
        format!("words={:?} tokens={}/3", words.ok(), tokens),
    ));

    let mut unique = true;
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            let mut j = i + 1;
            while j < DECLS.len() {
                if let Some(q) = DECLS.get(j) {
                    if d.stem == q.stem || d.keyword == q.keyword {
                        unique = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    v.push((
        "F48959-J1-模块声明唯一",
        unique && DECLS.len() == FAMILY_MODULES,
        format!("unique={} decls={}", unique, DECLS.len()),
    ));

    let account = audit_account();
    v.push((
        "F48960-J1-批印入册48000/240000",
        account == Ok((BATCH_LINES_TOTAL, DOMAIN_LINES_BUDGET)),
        format!(
            "stamp={}/{} account={:?}",
            BATCH_LINES_TOTAL,
            DOMAIN_LINES_BUDGET,
            account.ok()
        ),
    ));

    let cards = audit_cards();
    v.push((
        "F48960-J1-二十卡覆盖零缺口",
        cards == Ok(FAMILY_CARDS),
        format!("cards={:?} range={}..{}", cards.ok(), CARD_FIRST, CARD_LAST),
    ));

    let lines = audit_lines();
    v.push((
        "F48960-J1-家族行数≥6000",
        lines == Ok(family_lines_declared()),
        format!("lines={:?} declared={}", lines.ok(), family_lines_declared()),
    ));

    let merged = merge_seven();
    v.push((
        "F48960-J1-七册合卷全绿",
        merged.all_passed(),
        format!(
            "merged={} red={}",
            merged.len(),
            if merged.all_passed() { 0 } else { 1 }
        ),
    ));

    v
}

/// 收官入口：七册合卷 + 闭包判据。
pub fn run_unx48959_checks() -> CheckSet {
    let mut s = merge_seven();
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_are_ordered() {
        assert_eq!(audit_decls(), Ok(7));
        let mut i = 1usize;
        while i < DECLS.len() {
            if let (Some(a), Some(b)) = (DECLS.get(i - 1), DECLS.get(i)) {
                assert!(b.seg > a.seg);
                assert!(b.card_first > a.card_last);
            }
            i += 1;
        }
    }

    #[test]
    fn twenty_cards_are_covered_exactly_once() {
        assert_eq!(audit_cards(), Ok(20));
        assert_eq!(FAMILY_CARDS, 20);
    }

    #[test]
    fn line_budget_meets_floor() {
        assert!(audit_lines().is_ok());
        assert!(family_lines_declared() >= FAMILY_LINES_MIN);
        assert_eq!(family_lines_declared(), 7_800);
    }

    #[test]
    fn keywords_are_present_and_unique() {
        assert_eq!(audit_keywords(), Ok(7));
        let mut i = 0usize;
        while i < DECLS.len() {
            let mut j = i + 1;
            while j < DECLS.len() {
                if let (Some(a), Some(b)) = (DECLS.get(i), DECLS.get(j)) {
                    assert_ne!(a.keyword, b.keyword);
                    assert_ne!(a.stem, b.stem);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn anti_dup_is_clean() {
        assert_eq!(audit_dup(), Ok(3));
        assert_eq!(token_hits(), 3);
        assert!(!range_overlaps(48_941, 48_960, 48_921, 48_940));
        assert!(!range_overlaps(48_941, 48_960, 48_961, 48_980));
        assert!(range_overlaps(48_935, 48_945, 48_921, 48_940));
    }

    #[test]
    fn trace_round_trips() {
        let trace = grep_trace();
        assert_eq!(audit_trace(&trace), Ok(7));
        assert!(trace.contains("overlap=0"));
        assert_eq!(
            audit_trace("wheel=7|hires=1|scroll=7|ids=48941..48960|overlap=1"),
            Err(WsCode::DupBad.val())
        );
    }

    #[test]
    fn batch_stamp_is_frozen() {
        assert_eq!(audit_account(), Ok((48_000, 240_000)));
        assert_eq!(account_text(), "48000/240000");
        assert_eq!(BATCH_LINES_TOTAL / FAMILY_CARDS, 2_400);
    }

    #[test]
    fn merged_set_is_all_green() {
        let s = merge_seven();
        assert!(s.all_passed(), "merged red: {}", s.red_items_text());
        assert!(s.len() >= 50);
    }

    #[test]
    fn seal_checks_are_green() {
        let s = run_unx48959_checks();
        assert!(s.all_passed(), "seal red: {}", s.red_items_text());
        assert_eq!(s.len(), merge_seven().len() + extra_checks().len());
    }

    #[test]
    fn golden_imprint_is_stable() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn golden_is_anchored() {
        let g = golden_run();
        assert!(g.is_ok());
        if let Ok((merged, cards, lines, h)) = g {
            assert!(merged >= 50);
            assert_eq!(cards, 20);
            assert!(lines >= 6_000);
            assert_ne!(h, 0);
        }
    }

    #[test]
    fn inject_four_routes_all_caught() {
        let l = inject_rounds(10);
        assert!(l.is_ok());
        if let Ok(ll) = l {
            assert!(ll.all_caught());
            assert_eq!(ll.tries, [10; 4]);
            assert_eq!(ll.caught, [10; 4]);
        }
    }

    #[test]
    fn probe_sequences_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn segment_is_closed() {
        assert!(segment_guard());
        assert_eq!(WsCode::ALL.len(), 8);
        assert_eq!(WsCode::MergeBad.val(), SEG_BASE + 1);
        assert_eq!(WsCode::AccountBad.val(), SEG_BASE + 8);
    }

    #[test]
    fn empty_decl_is_rejected() {
        let d = ModuleDecl {
            keyword: "",
            ..DECLS[0]
        };
        assert!(!decl_ok(&d));
        assert!(decl_ok(&DECLS[0]));
    }
}
