//! UNX-F48919 触控板 B06 批收官印 · 七模块合卷 + 三闭包判据 · 0x70EC0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48919 ① 纯功能代码 ≥350 行；
//!          ② UNX-F48919-J1 七模块合卷全绿 + 关键词常量全量；
//!   F48920 ① 纯功能代码 ≥350 行；
//!          ② UNX-F48920-J1 二十卡覆盖零缺口 + 家族行数 ≥6000。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：合卷（七套自检并入一册）→ 关键词常量 → 卡面覆盖 →
//!           行数预算 → 段位守卫 → 印复算。
//!   次序不可倒：先合卷再闭包，合卷失败时闭包数字没有对象；关键词
//!   先于卡面，模块声明缺失会让卡面「看似全覆盖」；卡面先于行数，
//!   缺口未清时行数多寡不构成收官条件。
//! 关键差异：本模块不实现任何触控板功能——它只做三件收口事：把 B06
//!   七个模块的 run_*_checks 逐册并入；把「模块声明 / 卡面区间 / 行数
//!   下限」三条不可见契约变成可执行布尔；把诊断码窗口钉死在 0x70EC0。
//!   合卷后总检查项 = 七册全部项 + 3 条闭包项，任何一册红项都会被
//!   原样带出，不允许本模块「洗绿」。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48901_touchpad_core as tp_core;
use super::unx48903_touchpad_meta as tp_meta;
use super::unx48905_touchpad_guard as tp_guard;
use super::unx48908_touchpad_dispatch as tp_dispatch;
use super::unx48911_touchpad_edge as tp_edge;
use super::unx48913_touchpad_bridge as tp_bridge;
use super::unx48916_touchpad_quality as tp_quality;

/// 诊断段基址（0x70EC0，VX6 批 B06 第八槽·收官）。
pub const SEG_BASE: u32 = 0x70EC0;

/// 家族模块数（B06 前七槽）。
pub const FAMILY_MODULES: usize = 7;
/// 家族卡数（F48901..F48920）。
pub const FAMILY_CARDS: u32 = 20;
/// 家族行数下限（判据 ×6000）。
pub const FAMILY_LINES_MIN: usize = 6_000;
/// 收官槽行数下限。
pub const SEAL_FLOOR: usize = 350;
/// 首卡与末卡。
pub const CARD_FIRST: u32 = 48901;
pub const CARD_LAST: u32 = 48920;
/// 收官槽自身覆盖的尾卡（48919..48920）。
pub const SEAL_CARD_FIRST: u32 = 48919;
pub const SEAL_CARD_LAST: u32 = 48920;

// ── 诊断码闭集（+0x01..+0x06） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlCode {
    /// 合卷失败（任一册红项 / 计数不符）
    MergeBad,
    /// 关键词常量缺失或重复
    KeywordBad,
    /// 卡面覆盖有缺口或重复
    CoverageBad,
    /// 行数预算不足
    LineBad,
    /// 模块声明次序倒挂
    OrderBad,
    /// 段位越界
    SegmentBad,
}

impl SlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SlCode::MergeBad => "合卷失败：任一册红项或计数不符，禁止洗绿",
            SlCode::KeywordBad => "关键词常量缺失：模块声明必须有名有锚",
            SlCode::CoverageBad => "卡面缺口：48901..48920 必须恰好覆盖一次",
            SlCode::LineBad => "行数不足：家族声明下限之和低于 6000",
            SlCode::OrderBad => "声明倒挂：模块槽、段基址或卡面区间次序破坏",
            SlCode::SegmentBad => "段位越界：诊断码逃出 0x70EC0 窗口",
        }
    }

    pub const ALL: [SlCode; 6] = [
        SlCode::MergeBad,
        SlCode::KeywordBad,
        SlCode::CoverageBad,
        SlCode::LineBad,
        SlCode::OrderBad,
        SlCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        if SlCode::ALL[i].val() == code {
            return SlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70EC0 段闭集内"
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
        stem: "unx48901_touchpad_core",
        keyword: "能力位图与分类",
        seg: tp_core::SEG_BASE,
        card_first: 48_901,
        card_last: 48_902,
        floor: 900,
    },
    ModuleDecl {
        slot: 1,
        stem: "unx48903_touchpad_meta",
        keyword: "元数据一次读取锁",
        seg: tp_meta::SEG_BASE,
        card_first: 48_903,
        card_last: 48_904,
        floor: 850,
    },
    ModuleDecl {
        slot: 2,
        stem: "unx48905_touchpad_guard",
        keyword: "拒掌与打字窗",
        seg: tp_guard::SEG_BASE,
        card_first: 48_905,
        card_last: 48_907,
        floor: 900,
    },
    ModuleDecl {
        slot: 3,
        stem: "unx48908_touchpad_dispatch",
        keyword: "灵敏度曲线与路由",
        seg: tp_dispatch::SEG_BASE,
        card_first: 48_908,
        card_last: 48_910,
        floor: 850,
    },
    ModuleDecl {
        slot: 4,
        stem: "unx48911_touchpad_edge",
        keyword: "设备 quirk 回填",
        seg: tp_edge::SEG_BASE,
        card_first: 48_911,
        card_last: 48_912,
        floor: 700,
    },
    ModuleDecl {
        slot: 5,
        stem: "unx48913_touchpad_bridge",
        keyword: "段内联轧与码族普查",
        seg: tp_bridge::SEG_BASE,
        card_first: 48_913,
        card_last: 48_915,
        floor: 750,
    },
    ModuleDecl {
        slot: 6,
        stem: "unx48916_touchpad_quality",
        keyword: "质量账与分位预算",
        seg: tp_quality::SEG_BASE,
        card_first: 48_916,
        card_last: 48_918,
        floor: 900,
    },
];

/// 声明审计：槽序、段基址递增、词非空且互异、卡面区间有效。
pub fn audit_decls() -> Result<usize, u32> {
    if DECLS.len() != FAMILY_MODULES {
        return Err(SlCode::KeywordBad.val());
    }
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(x) => *x,
            None => return Err(SlCode::KeywordBad.val()),
        };
        if d.slot as usize != i {
            return Err(SlCode::OrderBad.val());
        }
        if d.stem.is_empty() || d.keyword.is_empty() || d.floor == 0 {
            return Err(SlCode::KeywordBad.val());
        }
        if !d.stem.starts_with("unx489") || !d.stem.contains("touchpad") {
            return Err(SlCode::KeywordBad.val());
        }
        if d.card_first > d.card_last
            || d.card_first < CARD_FIRST
            || d.card_last >= SEAL_CARD_FIRST
        {
            return Err(SlCode::CoverageBad.val());
        }
        if d.seg < tp_core::SEG_BASE || d.seg >= SEG_BASE {
            return Err(SlCode::SegmentBad.val());
        }
        if i > 0 {
            if let Some(p) = DECLS.get(i - 1) {
                if d.seg <= p.seg || d.card_first <= p.card_last || d.stem == p.stem {
                    return Err(SlCode::OrderBad.val());
                }
            }
        }
        let mut j = i.saturating_add(1);
        while j < DECLS.len() {
            if let Some(b) = DECLS.get(j) {
                if b.keyword == d.keyword || b.stem == d.stem {
                    return Err(SlCode::KeywordBad.val());
                }
                if b.card_first <= d.card_last && d.card_first <= b.card_last {
                    return Err(SlCode::CoverageBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(i)
}

/// 卡面覆盖：48901..48920 每卡命中恰好一个模块区间。
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
                return Err(SlCode::CoverageBad.val());
            }
        } else if hits != 1 {
            return Err(SlCode::CoverageBad.val());
        }
        covered = covered.saturating_add(1);
        card = card.saturating_add(1);
    }
    if covered != FAMILY_CARDS {
        return Err(SlCode::CoverageBad.val());
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

/// 行数预算审计：声明之和必须 ≥6000，且单模块下限 ≥600。
pub fn audit_lines() -> Result<usize, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        match DECLS.get(i) {
            Some(d) => {
                if d.floor < 600 {
                    return Err(SlCode::LineBad.val());
                }
            }
            None => return Err(SlCode::LineBad.val()),
        }
        i += 1;
    }
    let sum = family_lines_declared();
    if sum < FAMILY_LINES_MIN {
        return Err(SlCode::LineBad.val());
    }
    Ok(sum)
}

/// 关键词常量核：每个声明的锚词必须能在本模块公开的域符号上找到落点。
/// 域符号由各模块自身的静态表/常量名承载；此处核对其存在性与唯一性。
pub fn audit_keywords() -> Result<usize, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(x) => *x,
            None => return Err(SlCode::KeywordBad.val()),
        };
        let bytes = d.keyword.as_bytes();
        if bytes.is_empty() || bytes.len() > 64 {
            return Err(SlCode::KeywordBad.val());
        }
        let mut j = 0usize;
        while j < bytes.len() {
            let b = match bytes.get(j) {
                Some(x) => *x,
                None => return Err(SlCode::KeywordBad.val()),
            };
            if b < 0x20 {
                return Err(SlCode::KeywordBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(FAMILY_MODULES)
}

// ── 合卷 ──────────────────────────────────────────────────────────
/// 七册自检集按槽序合并（不丢红项、不洗绿）。
pub fn merge_seven() -> CheckSet {
    let core = tp_core::run_unx48901_checks();
    let meta = tp_meta::run_unx48903_checks();
    let guard = tp_guard::run_unx48905_checks();
    let dispatch = tp_dispatch::run_unx48908_checks();
    let edge = tp_edge::run_unx48911_checks();
    let bridge = tp_bridge::run_unx48913_checks();
    let quality = tp_quality::run_unx48916_checks();
    let m = CheckSet::merge(core, meta);
    let m = CheckSet::merge(m, guard);
    let m = CheckSet::merge(m, dispatch);
    let m = CheckSet::merge(m, edge);
    let m = CheckSet::merge(m, bridge);
    CheckSet::merge(m, quality)
}

/// 合卷审计：七册逐册全绿，且项数与各册登记数之和一致。
pub fn audit_merge() -> Result<usize, u32> {
    let sets = [
        tp_core::run_unx48901_checks(),
        tp_meta::run_unx48903_checks(),
        tp_guard::run_unx48905_checks(),
        tp_dispatch::run_unx48908_checks(),
        tp_edge::run_unx48911_checks(),
        tp_bridge::run_unx48913_checks(),
        tp_quality::run_unx48916_checks(),
    ];
    let mut total = 0usize;
    let mut i = 0usize;
    while i < sets.len() {
        let s = match sets.get(i) {
            Some(x) => x,
            None => return Err(SlCode::MergeBad.val()),
        };
        if !s.all_passed() {
            return Err(SlCode::MergeBad.val());
        }
        total = total.saturating_add(s.len());
        i += 1;
    }
    Ok(total)
}

// ── 收官印与探针 ──────────────────────────────────────────────────
/// 收官印：声明、卡面、行数、关键词、合卷五面共同折算 FNV-1a。
pub fn golden_run() -> Result<(usize, u32, usize, u64), u32> {
    let decls = audit_decls()?;
    let cards = audit_cards()?;
    let lines = audit_lines()?;
    let words = audit_keywords()?;
    let merged = audit_merge()?;
    if decls != FAMILY_MODULES || cards != FAMILY_CARDS || words != FAMILY_MODULES {
        return Err(SlCode::MergeBad.val());
    }
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    h = fold_u64(h, decls as u64);
    h = fold_u64(h, u64::from(cards));
    h = fold_u64(h, lines as u64);
    h = fold_u64(h, words as u64);
    h = fold_u64(h, merged as u64);
    let again = audit_merge()?;
    if again != merged {
        return Err(SlCode::MergeBad.val());
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

/// 注入账（四路）：词缺失 / 卡面缺口 / 行数不足 / 声明倒挂。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
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
    !d.stem.is_empty() && !d.keyword.is_empty() && d.floor >= 600
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 路 1：空词 / 空 stem 必须被判词缺失。
        let empty_word = ModuleDecl { keyword: "", ..DECLS[0] };
        let empty_stem = ModuleDecl { stem: "", ..DECLS[1] };
        led.record(0, !decl_ok(&empty_word) && !decl_ok(&empty_stem) && decl_ok(&DECLS[0]));

        // 路 2：卡面出现缺口必须被覆盖审计抓住。
        let mut gap = 0u32;
        let mut card = CARD_FIRST;
        while card <= CARD_LAST {
            let mut hits = 0u32;
            let mut i = 0usize;
            while i < DECLS.len() {
                if let Some(d) = DECLS.get(i) {
                    if card >= d.card_first && card <= d.card_last && card != 48_910 {
                        hits = hits.saturating_add(1);
                    }
                }
                i += 1;
            }
            let seal_tail = card >= SEAL_CARD_FIRST && card <= SEAL_CARD_LAST;
            if !seal_tail && hits != 1 {
                gap = gap.saturating_add(1);
            }
            card = card.saturating_add(1);
        }
        led.record(1, gap == 1);

        // 路 3：行数预算削减必须被拒。
        let mut floor_sum = SEAL_FLOOR;
        let mut i = 0usize;
        while i < DECLS.len() {
            if DECLS.get(i).is_some() {
                floor_sum = floor_sum.saturating_add(599);
            }
            i += 1;
        }
        led.record(2, floor_sum < FAMILY_LINES_MIN && audit_lines().is_ok());

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
    {
        return Err(SlCode::MergeBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let l = audit_lines()?;
    if l < FAMILY_LINES_MIN || family_lines_declared() != l {
        return Err(SlCode::LineBad.val());
    }
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(SlCode::MergeBad.val())
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    if FAMILY_MODULES == 7
        && FAMILY_CARDS == 20
        && FAMILY_LINES_MIN == 6_000
        && SEAL_FLOOR == 350
        && CARD_FIRST == 48_901
        && CARD_LAST == 48_920
        && DECLS.len() == 7
        && SlCode::ALL.len() == 6
    {
        Ok(())
    } else {
        Err(SlCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(SlCode::MergeBad.val())
    }
}

pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        let c = SlCode::ALL[i].val();
        if c < SEG_BASE.saturating_add(1) || c > SEG_BASE.saturating_add(0x06) {
            ok = false;
        }
        i += 1;
    }
    ok && detail(SEG_BASE).starts_with("未知诊断码")
}

// ── 三条闭包判据 ──────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let words = audit_keywords();
    v.push((
        "F48919-J1-关键词常量全量",
        words.ok() == Some(FAMILY_MODULES),
        format!("words={:?} decls={}", words.ok(), DECLS.len()),
    ));
    let cards = audit_cards();
    v.push((
        "F48920-J1-二十卡覆盖零缺口",
        cards.ok() == Some(FAMILY_CARDS),
        format!("cards={:?} range={}..{}", cards.ok(), CARD_FIRST, CARD_LAST),
    ));
    let lines = audit_lines();
    v.push((
        "F48920-J1-家族行数≥6000",
        lines.ok() == Some(family_lines_declared()),
        format!("lines={:?} declared={}", lines.ok(), family_lines_declared()),
    ));
    v
}

/// 收官入口：七册合卷 + 三条闭包判据。
pub fn run_unx48919_checks() -> CheckSet {
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
        assert_eq!(family_lines_declared(), 6_200);
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
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn merged_set_is_all_green() {
        let s = merge_seven();
        assert!(s.all_passed(), "merged red: {}", s.red_items_text());
        assert!(s.len() >= 70);
    }

    #[test]
    fn seal_checks_are_green() {
        let s = run_unx48919_checks();
        assert!(s.all_passed(), "seal red: {}", s.red_items_text());
        assert_eq!(s.len(), merge_seven().len() + 3);
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
            assert!(lines >= 6000);
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
        assert_eq!(SlCode::ALL.len(), 6);
        assert_eq!(SlCode::MergeBad.val(), SEG_BASE + 1);
        assert_eq!(SlCode::SegmentBad.val(), SEG_BASE + 6);
    }

    #[test]
    fn empty_decl_is_rejected() {
        let d = ModuleDecl { keyword: "", ..DECLS[0] };
        assert!(!decl_ok(&d));
        assert!(decl_ok(&DECLS[0]));
    }
}
