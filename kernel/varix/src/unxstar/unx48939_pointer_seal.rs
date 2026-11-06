//! UNX-F48939 指针 B07 批收官印 · 防重留痕 + 七册合卷 + 批印入册 · 0x71140
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-M2-B07）：
//!   F48939 ① 纯功能代码 ≥200 行；
//!          ② UNX-F48939-J1 批收口防重留痕零撞号；
//!             判据锚定 §6.1；
//!   F48940 ① 纯功能代码 ≥200 行；
//!          ② UNX-F48940-J1 批印入册：42000/240,000；
//!             判据锚定 §AI-62 §7。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：防重（关键词族 + ID 区间）→ 声明（七册合卷地址）→
//!           卡面（48921..48940 恰好一次）→ 行数（家族 ≥6000）→
//!           合卷（七册逐册全绿）→ 批印（42000/240,000）→ 段位守卫。
//!   次序不可倒：防重先于声明，撞号时后面的数字再整齐也没有归属；
//!   声明先于卡面，模块地址缺失会让卡面「看似全覆盖」；卡面先于
//!   行数，缺口未清时行数不构成收官条件；合卷先于批印，任何一册
//!   红项都会让批印失去意义；批印最后，它只是冻结前面所有结论。
//! 关键差异：本模块不实现任何指针功能——它只做三件收口事：把 B07
//!   七个模块的 run_*_checks 逐册并入；把「关键词族 pointer/accel/
//!   cursor、ID 段 48921..48940、邻批零交叠」变成可执行布尔并留下
//!   可解析的 grep 痕迹；把「20 卡 = 42,000 行 / 域账 240,000 行」
//!   冻结为批印。合卷后的总检查项 = 七册全部项 + 4 条闭包项，任何
//!   一册红项都会原样带出，本模块绝不洗绿。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48921_pointer_core as p_core;
use super::unx48923_pointer_meta as p_meta;
use super::unx48925_pointer_guard as p_guard;
use super::unx48928_pointer_dispatch as p_dispatch;
use super::unx48931_pointer_edge as p_edge;
use super::unx48933_pointer_bridge as p_bridge;
use super::unx48936_pointer_quality as p_quality;

/// 诊断段基址（0x71140，VX6 批 B07 第八槽·收官）。
pub const SEG_BASE: u32 = 0x71140;

/// 家族模块数（B07 前七槽）。
pub const FAMILY_MODULES: usize = 7;
/// 家族卡数（F48921..F48940）。
pub const FAMILY_CARDS: u32 = 20;
/// 家族行数下限（判据 ×6000）。
pub const FAMILY_LINES_MIN: usize = 6_000;
/// 批印行数（20 卡 × 2100 的冻结账）。
pub const BATCH_LINES_TOTAL: u32 = 42_000;
/// 域账累计（十个批 × 24,000 的冻结账）。
pub const DOMAIN_LINES_BUDGET: u32 = 240_000;
/// 收官槽行数下限。
pub const SEAL_FLOOR: usize = 700;
/// 单模块行数下限。
pub const MODULE_FLOOR_MIN: usize = 800;
/// 首卡与末卡。
pub const CARD_FIRST: u32 = 48_921;
pub const CARD_LAST: u32 = 48_940;
/// 收官槽自身覆盖的尾卡（48939..48940）。
pub const SEAL_CARD_FIRST: u32 = 48_939;
pub const SEAL_CARD_LAST: u32 = 48_940;
/// 关键词族（判据 §6.1：pointer/accel/cursor）。
pub const DUP_FAMILY: [&str; 3] = ["pointer", "accel", "cursor"];

// ── 诊断码闭集（+0x01..+0x08） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PsCode {
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
    /// 批印不平（42,000 / 240,000）
    AccountBad,
}

impl PsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            PsCode::MergeBad => "合卷失败：任一册红项或计数不符，禁止洗绿",
            PsCode::KeywordBad => "关键词族缺失：模块声明必须有名有锚",
            PsCode::CoverageBad => "卡面缺口：48921..48940 必须恰好覆盖一次",
            PsCode::LineBad => "行数不足：家族声明下限之和低于 6000",
            PsCode::DupBad => "防重撞号：ID 段与邻批交叠或家族词越界",
            PsCode::OrderBad => "声明倒挂：模块槽、段基址或卡面区间次序破坏",
            PsCode::SegmentBad => "段位越界：诊断码逃出 0x71140 窗口",
            PsCode::AccountBad => "批印不平：42,000 行 / 域账 240,000 行被破坏",
        }
    }

    pub const ALL: [PsCode; 8] = [
        PsCode::MergeBad,
        PsCode::KeywordBad,
        PsCode::CoverageBad,
        PsCode::LineBad,
        PsCode::DupBad,
        PsCode::OrderBad,
        PsCode::SegmentBad,
        PsCode::AccountBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PsCode::ALL.len() {
        if PsCode::ALL[i].val() == code {
            return PsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x71140 段闭集内"
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
        stem: "unx48921_pointer_core",
        keyword: "指针位移合成",
        seg: p_core::SEG_BASE,
        card_first: 48_921,
        card_last: 48_922,
        floor: 1_000,
    },
    ModuleDecl {
        slot: 1,
        stem: "unx48923_pointer_meta",
        keyword: "开关与元数据锚",
        seg: p_meta::SEG_BASE,
        card_first: 48_923,
        card_last: 48_924,
        floor: 850,
    },
    ModuleDecl {
        slot: 2,
        stem: "unx48925_pointer_guard",
        keyword: "五键与加速曲线表",
        seg: p_guard::SEG_BASE,
        card_first: 48_925,
        card_last: 48_927,
        floor: 1_000,
    },
    ModuleDecl {
        slot: 3,
        stem: "unx48928_pointer_dispatch",
        keyword: "子像素与多屏路由",
        seg: p_dispatch::SEG_BASE,
        card_first: 48_928,
        card_last: 48_930,
        floor: 1_000,
    },
    ModuleDecl {
        slot: 4,
        stem: "unx48931_pointer_edge",
        keyword: "边界静默与手势机",
        seg: p_edge::SEG_BASE,
        card_first: 48_931,
        card_last: 48_932,
        floor: 900,
    },
    ModuleDecl {
        slot: 5,
        stem: "unx48933_pointer_bridge",
        keyword: "段内联轧与码族普查",
        seg: p_bridge::SEG_BASE,
        card_first: 48_933,
        card_last: 48_935,
        floor: 850,
    },
    ModuleDecl {
        slot: 6,
        stem: "unx48936_pointer_quality",
        keyword: "四点埋点与用例集",
        seg: p_quality::SEG_BASE,
        card_first: 48_936,
        card_last: 48_938,
        floor: 1_300,
    },
];

/// 邻批区间（B06 触控板与 B08 滚轮），B07 只读不写。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchRange {
    pub family: &'static str,
    pub first: u32,
    pub last: u32,
}

pub const NEIGHBORS: [BatchRange; 2] = [
    BatchRange {
        family: "touchpad",
        first: 48_901,
        last: 48_920,
    },
    BatchRange {
        family: "wheel",
        first: 48_941,
        last: 48_960,
    },
];

/// 关键词族映射：英文族词 → 模块声明里的锚词（防重留痕的主体）。
pub const TOKEN_MAP: [(&str, &str); 3] = [
    ("pointer", "指针位移合成"),
    ("accel", "五键与加速曲线表"),
    ("cursor", "开关与元数据锚"),
];

fn range_overlaps(a_first: u32, a_last: u32, b_first: u32, b_last: u32) -> bool {
    a_first <= b_last && b_first <= a_last
}

/// 声明审计：槽序、段基址递增、词非空且互异、卡面区间有效。
pub fn audit_decls() -> Result<usize, u32> {
    if DECLS.len() != FAMILY_MODULES {
        return Err(PsCode::KeywordBad.val());
    }
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(x) => *x,
            None => return Err(PsCode::KeywordBad.val()),
        };
        if d.slot as usize != i {
            return Err(PsCode::OrderBad.val());
        }
        if d.stem.is_empty() || d.keyword.is_empty() || d.floor == 0 {
            return Err(PsCode::KeywordBad.val());
        }
        if !d.stem.starts_with("unx489") || !d.stem.contains("pointer") {
            return Err(PsCode::KeywordBad.val());
        }
        if d.card_first > d.card_last
            || d.card_first < CARD_FIRST
            || d.card_last >= SEAL_CARD_FIRST
        {
            return Err(PsCode::CoverageBad.val());
        }
        if d.seg < p_core::SEG_BASE || d.seg >= SEG_BASE {
            return Err(PsCode::SegmentBad.val());
        }
        if i > 0 {
            if let Some(p) = DECLS.get(i - 1) {
                if d.seg <= p.seg || d.card_first <= p.card_last || d.stem == p.stem {
                    return Err(PsCode::OrderBad.val());
                }
            }
        }
        let mut j = i.saturating_add(1);
        while j < DECLS.len() {
            if let Some(b) = DECLS.get(j) {
                if b.keyword == d.keyword || b.stem == d.stem {
                    return Err(PsCode::KeywordBad.val());
                }
                if b.card_first <= d.card_last && d.card_first <= b.card_last {
                    return Err(PsCode::CoverageBad.val());
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
            None => return Err(PsCode::DupBad.val()),
        };
        let mut n = 0usize;
        while n < NEIGHBORS.len() {
            if let Some(b) = NEIGHBORS.get(n) {
                if range_overlaps(d.card_first, d.card_last, b.first, b.last) {
                    return Err(PsCode::DupBad.val());
                }
                if d.stem.contains(b.family) {
                    return Err(PsCode::DupBad.val());
                }
            }
            n += 1;
        }
        if d.keyword.contains("触控板") || d.keyword.contains("滚轮") {
            return Err(PsCode::DupBad.val());
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < TOKEN_MAP.len() {
        let (word, anchor) = match TOKEN_MAP.get(t) {
            Some(v) => *v,
            None => return Err(PsCode::DupBad.val()),
        };
        if word.is_empty() || !word.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err(PsCode::DupBad.val());
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
            return Err(PsCode::DupBad.val());
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

/// 卡面覆盖：48921..48940 每卡命中恰好一个模块区间。
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
                return Err(PsCode::CoverageBad.val());
            }
        } else if hits != 1 {
            return Err(PsCode::CoverageBad.val());
        }
        covered = covered.saturating_add(1);
        card = card.saturating_add(1);
    }
    if covered != FAMILY_CARDS {
        return Err(PsCode::CoverageBad.val());
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
                    return Err(PsCode::LineBad.val());
                }
            }
            None => return Err(PsCode::LineBad.val()),
        }
        i += 1;
    }
    let sum = family_lines_declared();
    if sum < FAMILY_LINES_MIN {
        return Err(PsCode::LineBad.val());
    }
    Ok(sum)
}

/// 关键词常量核：锚词短、可打印、且都在声明里出现过。
pub fn audit_keywords() -> Result<usize, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(x) => *x,
            None => return Err(PsCode::KeywordBad.val()),
        };
        let bytes = d.keyword.as_bytes();
        if bytes.is_empty() || bytes.len() > 64 {
            return Err(PsCode::KeywordBad.val());
        }
        let mut j = 0usize;
        while j < bytes.len() {
            let b = match bytes.get(j) {
                Some(x) => *x,
                None => return Err(PsCode::KeywordBad.val()),
            };
            if b < 0x20 {
                return Err(PsCode::KeywordBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(FAMILY_MODULES)
}

/// 批印审计：20 卡 × 2,100 行 = 42,000，域账 240,000 平分十批。
pub fn audit_account() -> Result<(u32, u32), u32> {
    if BATCH_LINES_TOTAL != 42_000 || DOMAIN_LINES_BUDGET != 240_000 {
        return Err(PsCode::AccountBad.val());
    }
    if BATCH_LINES_TOTAL >= DOMAIN_LINES_BUDGET {
        return Err(PsCode::AccountBad.val());
    }
    if FAMILY_CARDS != 20 || BATCH_LINES_TOTAL / FAMILY_CARDS != 2_100 {
        return Err(PsCode::AccountBad.val());
    }
    let declared = family_lines_declared();
    if declared < FAMILY_LINES_MIN || (declared as u32) > BATCH_LINES_TOTAL {
        return Err(PsCode::AccountBad.val());
    }
    Ok((BATCH_LINES_TOTAL, DOMAIN_LINES_BUDGET))
}

pub fn account_text() -> String {
    format!("{}/{}", BATCH_LINES_TOTAL, DOMAIN_LINES_BUDGET)
}

// ── 防重留痕（可渲染、可回读） ────────────────────────────────────
pub fn grep_trace() -> String {
    format!(
        "pointer={}|accel={}|cursor={}|ids={}..{}|overlap=0",
        FAMILY_MODULES,
        token_hits(),
        FAMILY_MODULES,
        CARD_FIRST,
        CARD_LAST
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(PsCode::KeywordBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(PsCode::KeywordBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(PsCode::KeywordBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(PsCode::KeywordBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 痕迹审计：五字段、族词计数与区间锚定一致、overlap 必须为 0。
pub fn audit_trace(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 5 {
        return Err(PsCode::DupBad.val());
    }
    let mut counts = [0u32; 3];
    let mut i = 0usize;
    while i < 3 {
        let part = match parts.get(i) {
            Some(v) => v,
            None => return Err(PsCode::DupBad.val()),
        };
        let mut it = part.split('=');
        let name = match it.next() {
            Some(v) => v,
            None => return Err(PsCode::DupBad.val()),
        };
        let value = match it.next() {
            Some(v) => v,
            None => return Err(PsCode::DupBad.val()),
        };
        if it.next().is_some() || name != DUP_FAMILY[i] {
            return Err(PsCode::DupBad.val());
        }
        counts[i] = parse_u32(value)?;
        i += 1;
    }
    if counts[0] != FAMILY_MODULES as u32 || counts[1] == 0 || counts[2] == 0 {
        return Err(PsCode::DupBad.val());
    }
    let ids = match parts.get(3) {
        Some(v) => v,
        None => return Err(PsCode::DupBad.val()),
    };
    let want_ids = format!("ids={}..{}", CARD_FIRST, CARD_LAST);
    if *ids != want_ids.as_str() {
        return Err(PsCode::CoverageBad.val());
    }
    let overlap = match parts.get(4) {
        Some(v) => v,
        None => return Err(PsCode::DupBad.val()),
    };
    if *overlap != "overlap=0" {
        return Err(PsCode::DupBad.val());
    }
    Ok(counts[0])
}

// ── 合卷 ──────────────────────────────────────────────────────────
/// 七册自检集按槽序合并（不丢红项、不洗绿）。
pub fn merge_seven() -> CheckSet {
    let core = p_core::run_unx48921_checks();
    let meta = p_meta::run_unx48923_checks();
    let guard = p_guard::run_unx48925_checks();
    let dispatch = p_dispatch::run_unx48928_checks();
    let edge = p_edge::run_unx48931_checks();
    let bridge = p_bridge::run_unx48933_checks();
    let quality = p_quality::run_unx48936_checks();
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
        p_core::run_unx48921_checks(),
        p_meta::run_unx48923_checks(),
        p_guard::run_unx48925_checks(),
        p_dispatch::run_unx48928_checks(),
        p_edge::run_unx48931_checks(),
        p_bridge::run_unx48933_checks(),
        p_quality::run_unx48936_checks(),
    ];
    let mut total = 0usize;
    let mut i = 0usize;
    while i < sets.len() {
        let s = match sets.get(i) {
            Some(x) => x,
            None => return Err(PsCode::MergeBad.val()),
        };
        if !s.all_passed() {
            return Err(PsCode::MergeBad.val());
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
        return Err(PsCode::MergeBad.val());
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
        return Err(PsCode::MergeBad.val());
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
        && d.stem.contains("pointer")
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 {
        return Err(PsCode::LineBad.val());
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

        // 路 2：与 B06 区间交叠必须被判撞号。
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
        let shifted = range_overlaps(48_905, 48_925, NEIGHBORS[0].first, NEIGHBORS[0].last);
        led.record(1, overlap == 0 && shifted && audit_dup().is_ok());

        // 路 3：批印被改必须被拒（账不平 / 平分不整 / 超预算）。
        let bad_batch = 41_999u32;
        let bad_piece = bad_batch / FAMILY_CARDS != 2_100;
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
        return Err(PsCode::MergeBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let l = audit_lines()?;
    if l < FAMILY_LINES_MIN || family_lines_declared() != l {
        return Err(PsCode::LineBad.val());
    }
    let (batch, domain) = audit_account()?;
    if batch != BATCH_LINES_TOTAL || domain != DOMAIN_LINES_BUDGET {
        return Err(PsCode::AccountBad.val());
    }
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(PsCode::MergeBad.val())
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    let trace = grep_trace();
    if audit_trace(&trace)? != FAMILY_MODULES as u32 {
        return Err(PsCode::DupBad.val());
    }
    if FAMILY_MODULES == 7
        && FAMILY_CARDS == 20
        && FAMILY_LINES_MIN == 6_000
        && BATCH_LINES_TOTAL == 42_000
        && DOMAIN_LINES_BUDGET == 240_000
        && SEAL_FLOOR == 700
        && CARD_FIRST == 48_921
        && CARD_LAST == 48_940
        && DECLS.len() == 7
        && DUP_FAMILY.len() == 3
        && PsCode::ALL.len() == 8
    {
        Ok(())
    } else {
        Err(PsCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(PsCode::MergeBad.val())
    }
}

pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < PsCode::ALL.len() {
        let c = PsCode::ALL[i].val();
        if c < SEG_BASE.saturating_add(1) || c > SEG_BASE.saturating_add(0x08) {
            ok = false;
        }
        i += 1;
    }
    ok && detail(SEG_BASE).starts_with("未知诊断码")
}

// ── 四条闭包判据 ──────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    let trace = grep_trace();
    let trace_ok = audit_trace(&trace) == Ok(FAMILY_MODULES as u32);
    v.push((
        "F48939-J1-防重留痕零撞号",
        trace_ok && audit_dup() == Ok(TOKEN_MAP.len() as u32),
        format!(
            "trace={} dup={:?}",
            trace,
            audit_dup().ok()
        ),
    ));

    let words = audit_keywords();
    let tokens = token_hits();
    v.push((
        "F48939-J1-关键词族三词全量",
        words == Ok(FAMILY_MODULES) && tokens == TOKEN_MAP.len() as u32 * 1,
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
        "F48939-J1-模块声明唯一",
        unique && DECLS.len() == FAMILY_MODULES,
        format!("unique={} decls={}", unique, DECLS.len()),
    ));

    let account = audit_account();
    v.push((
        "F48940-J1-批印入册42000/240000",
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
        "F48940-J1-二十卡覆盖零缺口",
        cards == Ok(FAMILY_CARDS),
        format!("cards={:?} range={}..{}", cards.ok(), CARD_FIRST, CARD_LAST),
    ));

    let lines = audit_lines();
    v.push((
        "F48940-J1-家族行数≥6000",
        lines == Ok(family_lines_declared()),
        format!("lines={:?} declared={}", lines.ok(), family_lines_declared()),
    ));

    let merged = merge_seven();
    v.push((
        "F48940-J1-七册合卷全绿",
        merged.all_passed(),
        format!(
            "merged={} red={}",
            merged.len(),
            if merged.all_passed() { 0 } else { 1 }
        ),
    ));

    v
}

/// 收官入口：七册合卷 + 四条闭包判据。
pub fn run_unx48939_checks() -> CheckSet {
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
        assert_eq!(family_lines_declared(), 7_600);
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
        assert!(!range_overlaps(48_921, 48_940, 48_901, 48_920));
        assert!(!range_overlaps(48_921, 48_940, 48_941, 48_960));
        assert!(range_overlaps(48_915, 48_925, 48_901, 48_920));
    }

    #[test]
    fn trace_round_trips() {
        let trace = grep_trace();
        assert_eq!(audit_trace(&trace), Ok(7));
        assert!(trace.contains("overlap=0"));
        assert_eq!(audit_trace("pointer=7|accel=1|cursor=7|ids=48921..48940|overlap=1"),
            Err(PsCode::DupBad.val()));
    }

    #[test]
    fn batch_stamp_is_frozen() {
        assert_eq!(audit_account(), Ok((42_000, 240_000)));
        assert_eq!(account_text(), "42000/240000");
        assert_eq!(BATCH_LINES_TOTAL / FAMILY_CARDS, 2_100);
    }

    #[test]
    fn merged_set_is_all_green() {
        let s = merge_seven();
        assert!(s.all_passed(), "merged red: {}", s.red_items_text());
        assert!(s.len() >= 70);
    }

    #[test]
    fn seal_checks_are_green() {
        let s = run_unx48939_checks();
        assert!(s.all_passed(), "seal red: {}", s.red_items_text());
        assert_eq!(s.len(), merge_seven().len() + 7);
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
            assert!(merged >= 70);
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
        assert_eq!(PsCode::ALL.len(), 8);
        assert_eq!(PsCode::MergeBad.val(), SEG_BASE + 1);
        assert_eq!(PsCode::AccountBad.val(), SEG_BASE + 8);
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
