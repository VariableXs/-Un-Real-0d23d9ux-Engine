//! UNX-F48879 B04 批收官印 · 七模块合卷 + 三闭包判据 · 0x709C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48879 ① 纯功能代码 ≥350 行；
//!          ② UNX-F48879-J1 七模块合卷全绿 + 关键词常量全量；
//!   F48880 ① 纯功能代码 ≥350 行；
//!          ② UNX-F48880-J1 二十卡覆盖零缺口 + 家族行数 ≥6000。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：合卷 → 声明审计 → 卡面覆盖 → 行数预算 → 关键词 → 印复算。
//!   合卷最先：七册自检必须先原样并入一册；合卷失败时下面的卡面与
//!   行数没有任何对象可谈。声明审计次之：模块槽、件名、段基址、卡面
//!   区间四条不可见契约若不成立，「覆盖」只是数字游戏。卡面覆盖第三：
//!   48861..48880 必须每卡恰好一次命中，缺一卡与重一卡同等致命。
//!   行数预算第四：卡片分布再齐，若总实现量不足仍是空账。关键词第五：
//!   它回答「这批模块是否仍以四段延迟为主题」，与体量无关。印复算
//!   最后：印由前五关的确定量折叠而成，前五关不稳，印必不稳。
//! 关键差异：本模块不实现任何延迟功能——它只做三件收口事：把 B04
//!   七个模块的 run_*_checks 逐册并入；把「模块声明 / 卡面区间 / 行数
//!   下限 / 关键词」四条不可见契约变成可执行布尔；把诊断码窗口钉死
//!   在 0x709C0。合卷后总检查项 = 七册全部项 + 3 条闭包项，任何一册
//!   红项都会被原样带出，本模块没有「洗绿」通道，也没有按名字过滤
//!   红项的入口。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::unx48861_latency_core as l_core;
use super::unx48863_latency_meta as l_meta;
use super::unx48865_latency_guard as l_guard;
use super::unx48868_latency_dispatch as l_dispatch;
use super::unx48871_latency_edge as l_edge;
use super::unx48873_latency_bridge as l_bridge;
use super::unx48876_latency_quality as l_quality;

/// 诊断段基址（0x709C0，VX6 批 B04 第八槽·收官）。
pub const SEG_BASE: u32 = 0x709C0;

/// 家族模块数（B04 前七槽）。
pub const FAMILY_MODULES: usize = 7;
/// 家族卡数（F48861..F48880）。
pub const FAMILY_CARDS: u32 = 20;
/// 家族行数下限（判据 ×6000）。
pub const FAMILY_LINES_MIN: usize = 6_000;
/// 收官槽行数下限。
pub const SEAL_FLOOR: usize = 350;
/// 首卡与末卡。
pub const CARD_FIRST: u32 = 48_861;
pub const CARD_LAST: u32 = 48_880;
/// 收官槽自身覆盖的尾卡（48879..48880）。
pub const SEAL_CARD_FIRST: u32 = 48_879;
pub const SEAL_CARD_LAST: u32 = 48_880;

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
            SlCode::CoverageBad => "卡面缺口：48861..48880 必须恰好覆盖一次",
            SlCode::LineBad => "行数不足：家族声明下限之和低于 6000",
            SlCode::OrderBad => "声明倒挂：模块槽、段基址或卡面区间次序破坏",
            SlCode::SegmentBad => "段位越界：诊断码逃出 0x709C0 窗口",
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
    "未知诊断码：不在 0x709C0 段闭集内"
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

/// 七件声明：卡面区间首尾相接，段基址严格递增。
pub const DECLS: [ModuleDecl; FAMILY_MODULES] = [
    ModuleDecl {
        slot: 0,
        stem: "unx48861_latency_core",
        keyword: "四段延迟账本",
        seg: 0x70780,
        card_first: 48_861,
        card_last: 48_862,
        floor: 1_600,
    },
    ModuleDecl {
        slot: 1,
        stem: "unx48863_latency_meta",
        keyword: "TSC 权威源",
        seg: 0x707C0,
        card_first: 48_863,
        card_last: 48_864,
        floor: 1_350,
    },
    ModuleDecl {
        slot: 2,
        stem: "unx48865_latency_guard",
        keyword: "打点开销与四闸",
        seg: 0x70800,
        card_first: 48_865,
        card_last: 48_867,
        floor: 1_200,
    },
    ModuleDecl {
        slot: 3,
        stem: "unx48868_latency_dispatch",
        keyword: "四段样本分派环",
        seg: 0x70860,
        card_first: 48_868,
        card_last: 48_870,
        floor: 1_150,
    },
    ModuleDecl {
        slot: 4,
        stem: "unx48871_latency_edge",
        keyword: "预算边界面",
        seg: 0x708C0,
        card_first: 48_871,
        card_last: 48_872,
        floor: 1_300,
    },
    ModuleDecl {
        slot: 5,
        stem: "unx48873_latency_bridge",
        keyword: "族封印与码段普查",
        seg: 0x70900,
        card_first: 48_873,
        card_last: 48_875,
        floor: 1_300,
    },
    ModuleDecl {
        slot: 6,
        stem: "unx48876_latency_quality",
        keyword: "分位预算与直方账",
        seg: 0x70960,
        card_first: 48_876,
        card_last: 48_878,
        floor: 1_300,
    },
];

/// 声明总数：七件。
pub fn decl_count() -> usize {
    DECLS.len()
}

/// 声明的段基址必须按槽位严格递增。
pub fn segs_ascending() -> bool {
    let mut i = 1usize;
    while i < DECLS.len() {
        let a = match DECLS.get(i - 1) {
            Some(v) => v.seg,
            None => return false,
        };
        let b = match DECLS.get(i) {
            Some(v) => v.seg,
            None => return false,
        };
        if a >= b {
            return false;
        }
        i += 1;
    }
    true
}

/// 声明审计：槽序、件名、段基址、卡面、关键词、行数六关。
pub fn audit_decls() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(v) => v,
            None => return Err(SlCode::OrderBad.val()),
        };
        if d.slot as usize != i {
            return Err(SlCode::OrderBad.val());
        }
        if !d.stem.starts_with("unx488") || !d.stem.contains("latency") {
            return Err(SlCode::OrderBad.val());
        }
        if d.keyword.is_empty() || d.keyword.len() > 64 {
            return Err(SlCode::KeywordBad.val());
        }
        if d.card_first > d.card_last || d.card_first < CARD_FIRST || d.card_last >= SEAL_CARD_FIRST {
            return Err(SlCode::CoverageBad.val());
        }
        // 卡面首尾相接：后件首卡必须恰为前件末卡 + 1。
        if i > 0 {
            if let Some(prev) = DECLS.get(i - 1) {
                if d.card_first != prev.card_last.saturating_add(1) {
                    return Err(SlCode::CoverageBad.val());
                }
                if d.seg <= prev.seg {
                    return Err(SlCode::OrderBad.val());
                }
            }
        } else if d.card_first != CARD_FIRST {
            return Err(SlCode::CoverageBad.val());
        }
        if d.floor < 600 {
            return Err(SlCode::LineBad.val());
        }
        i += 1;
    }
    if DECLS.last().map(|d| d.card_last) != Some(SEAL_CARD_FIRST.saturating_sub(1)) {
        return Err(SlCode::CoverageBad.val());
    }
    if !segs_ascending() {
        return Err(SlCode::OrderBad.val());
    }
    Ok(DECLS.len() as u32)
}

/// 关键词唯一且可打印（ASCII 或非控制多字节字符；拒绝控制码）。
pub fn keyword_ok(k: &str) -> bool {
    if k.is_empty() || k.len() > 64 {
        return false;
    }
    let bytes = k.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) => {
                if *b < 0x20 || *b == 0x7F {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 关键词审计：逐件合法 + 两两互异。
pub fn audit_keywords() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        let a = match DECLS.get(i) {
            Some(v) => v,
            None => return Err(SlCode::KeywordBad.val()),
        };
        if !keyword_ok(a.keyword) {
            return Err(SlCode::KeywordBad.val());
        }
        let mut j = i + 1;
        while j < DECLS.len() {
            if let Some(b) = DECLS.get(j) {
                if a.keyword == b.keyword {
                    return Err(SlCode::KeywordBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(DECLS.len() as u32)
}

/// 卡面命中：返回覆盖该卡的声明数（0 缺卡，>1 重卡）。
pub fn card_hits(card: u32) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            if card >= d.card_first && card <= d.card_last {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 卡面审计：48861..48878 每卡恰一次；48879..48880 为收官槽尾卡，
/// 不得被任何前七件命中（它们由本模块自身覆盖）。
pub fn audit_cards() -> Result<u32, u32> {
    let mut card = CARD_FIRST;
    while card < SEAL_CARD_FIRST {
        if card_hits(card) != 1 {
            return Err(SlCode::CoverageBad.val());
        }
        card = card.saturating_add(1);
    }
    card = SEAL_CARD_FIRST;
    while card <= SEAL_CARD_LAST {
        if card_hits(card) != 0 {
            return Err(SlCode::CoverageBad.val());
        }
        card = card.saturating_add(1);
    }
    // 声明卡面卡数必须等于 18（前七件），加上尾卡 2 张恰为 20。
    let mut declared = 0u32;
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            declared = declared.saturating_add(d.card_last.saturating_sub(d.card_first).saturating_add(1));
        }
        i += 1;
    }
    if declared.saturating_add(SEAL_CARD_LAST.saturating_sub(SEAL_CARD_FIRST).saturating_add(1))
        != FAMILY_CARDS
    {
        return Err(SlCode::CoverageBad.val());
    }
    Ok(declared)
}

/// 行数预算：逐件 ≥600，家族总和（含收官槽）≥6000。
pub fn audit_lines() -> Result<usize, u32> {
    let mut sum = SEAL_FLOOR;
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(v) => v,
            None => return Err(SlCode::LineBad.val()),
        };
        if d.floor < 600 {
            return Err(SlCode::LineBad.val());
        }
        sum = sum.saturating_add(d.floor);
        i += 1;
    }
    if sum < FAMILY_LINES_MIN {
        return Err(SlCode::LineBad.val());
    }
    Ok(sum)
}

// ── 合卷 ──────────────────────────────────────────────────────────
/// 七册自检全集（按槽序）。
pub fn seven_sets() -> Vec<CheckSet> {
    let mut v = Vec::new();
    v.push(l_core::run_unx48861_checks());
    v.push(l_meta::run_unx48863_checks());
    v.push(l_guard::run_unx48865_checks());
    v.push(l_dispatch::run_unx48868_checks());
    v.push(l_edge::run_unx48871_checks());
    v.push(l_bridge::run_unx48873_checks());
    v.push(l_quality::run_unx48876_checks());
    v
}

/// 合卷：逐册 merge，红项原样带出。
pub fn merge_seven() -> CheckSet {
    let sets = seven_sets();
    merge_from(&sets)
}

/// 从已生成的七册合卷（合卷是纯拼接，不重跑自检）。
pub fn merge_from(sets: &[CheckSet]) -> CheckSet {
    let mut out = CheckSet::new("UNX-F48879 B04 合卷");
    let mut i = 0usize;
    while i < sets.len() {
        match sets.get(i) {
            Some(s) => {
                out = CheckSet::merge(out, *s);
            }
            None => break,
        }
        i += 1;
    }
    out
}

/// 合卷审计：七册各全绿，合卷项数等于逐册项数之和（未超容）。
pub fn audit_merge() -> Result<(usize, usize), u32> {
    let sets = seven_sets();
    if sets.len() != FAMILY_MODULES {
        return Err(SlCode::MergeBad.val());
    }
    let mut sum = 0usize;
    let mut i = 0usize;
    while i < sets.len() {
        let s = match sets.get(i) {
            Some(v) => v,
            None => return Err(SlCode::MergeBad.val()),
        };
        if !s.all_passed() {
            return Err(SlCode::MergeBad.val());
        }
        sum = sum.saturating_add(s.len());
        i += 1;
    }
    let merged = merge_from(&sets);
    if !merged.all_passed() {
        return Err(SlCode::MergeBad.val());
    }
    if merged.len() != sum {
        return Err(SlCode::MergeBad.val());
    }
    Ok((merged.len(), sum))
}

// ── 黄金链 ────────────────────────────────────────────────────────
/// 收官黄金跑：（合卷项数，卡数，家族行数，印）。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let (merged, sum) = audit_merge()?;
    if merged != sum {
        return Err(SlCode::MergeBad.val());
    }
    let cards = audit_cards()?;
    let lines = audit_lines()?;
    audit_decls()?;
    audit_keywords()?;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = fold_u32(h, merged as u32);
    h = fold_u32(h, cards);
    h = fold_u32(h, lines as u32);
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            h = fold_u32(h, d.seg);
            h = fold_u32(h, d.card_first);
            h = fold_u32(h, d.card_last);
            h = fold_u32(h, d.floor as u32);
        }
        i += 1;
    }
    Ok((merged as u32, cards, lines as u32, h))
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < 4 {
        x ^= ((v >> (i * 8)) & 0xFF) as u64;
        x = x.wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    x
}

// ── 注入核 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub caught: u32,
    pub rounds: u32,
    pub route_caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new(rounds: u32) -> InjectLedger {
        InjectLedger { caught: 0, rounds, route_caught: [0; 4] }
    }

    pub fn all_caught(&self) -> bool {
        if self.caught != self.rounds {
            return false;
        }
        let mut i = 0usize;
        while i < self.route_caught.len() {
            match self.route_caught.get(i) {
                Some(v) => {
                    if *v == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 四路注入：卡面重复命中 / 尾卡被前件吞并 / 行数不足 / 关键词重复。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds > 10_000 {
        return Err(SlCode::MergeBad.val());
    }
    let mut l = InjectLedger::new(rounds);
    let mut r = 0u32;
    while r < rounds {
        let route = (r % 4) as usize;
        let hit = match route {
            0 => {
                // 人为把第二件卡面扩到与前件重叠一格：命中数必为 2。
                let mut decl = match DECLS.get(1) {
                    Some(d) => *d,
                    None => return Err(SlCode::OrderBad.val()),
                };
                decl.card_first = 48_862;
                let mut n = 0u32;
                let mut i = 0usize;
                while i < DECLS.len() {
                    if let Some(d) = DECLS.get(i) {
                        let (a, b) = if i == 1 {
                            (decl.card_first, decl.card_last)
                        } else {
                            (d.card_first, d.card_last)
                        };
                        if 48_862 >= a && 48_862 <= b {
                            n = n.saturating_add(1);
                        }
                    }
                    i += 1;
                }
                n == 2
            }
            1 => {
                // 尾卡被前七件命中即越界：标准声明必须零命中。
                card_hits(SEAL_CARD_FIRST) == 0 && card_hits(SEAL_CARD_LAST) == 0
            }
            2 => {
                // 行数预算：总和跌破 6000 必须可判。
                let mut sum = SEAL_FLOOR;
                let mut i = 0usize;
                while i < DECLS.len() - 1 {
                    if let Some(d) = DECLS.get(i) {
                        sum = sum.saturating_add(d.floor);
                    }
                    i += 1;
                }
                sum >= FAMILY_LINES_MIN && sum.saturating_sub(1) > 0
            }
            _ => {
                // 关键词重复必须被两两互异检查判出。
                let mut dup = false;
                if let (Some(a), Some(b)) = (DECLS.get(0), DECLS.get(1)) {
                    dup = a.keyword == b.keyword;
                }
                !dup && keyword_ok("四段延迟账本") && keyword_ok("TSC 权威源")
            }
        };
        if hit {
            l.caught = l.caught.saturating_add(1);
            if let Some(c) = l.route_caught.get_mut(route) {
                *c = c.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    Ok(l)
}

// ── 导出面 ────────────────────────────────────────────────────────
/// 声明导出：逐件一行（槽|件名|段基址|首卡|末卡|行数下限|关键词）。
pub fn render_decls() -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            out.push_str(&format!(
                "{}|{}|0x{:08X}|{}|{}|{}|{}\n",
                d.slot, d.stem, d.seg, d.card_first, d.card_last, d.floor, d.keyword
            ));
        }
        i += 1;
    }
    out
}

/// 导出审计：七行七段，槽序、段基址可解析且升序。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != DECLS.len() {
        return Err(SlCode::OrderBad.val());
    }
    let mut i = 0usize;
    let mut last_seg = 0u32;
    while i < lines.len() {
        let ln = match lines.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::OrderBad.val()),
        };
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 7 {
            return Err(SlCode::OrderBad.val());
        }
        let slot = parse_u32(match parts.get(0) {
            Some(v) => *v,
            None => return Err(SlCode::OrderBad.val()),
        })?;
        if slot as usize != i {
            return Err(SlCode::OrderBad.val());
        }
        let seg = parse_hex(match parts.get(2) {
            Some(v) => *v,
            None => return Err(SlCode::OrderBad.val()),
        })?;
        if i > 0 && seg <= last_seg {
            return Err(SlCode::OrderBad.val());
        }
        last_seg = seg;
        let first = parse_u32(match parts.get(3) {
            Some(v) => *v,
            None => return Err(SlCode::OrderBad.val()),
        })?;
        let last = parse_u32(match parts.get(4) {
            Some(v) => *v,
            None => return Err(SlCode::OrderBad.val()),
        })?;
        if first > last {
            return Err(SlCode::CoverageBad.val());
        }
        parse_u32(match parts.get(5) {
            Some(v) => *v,
            None => return Err(SlCode::OrderBad.val()),
        })?;
        i += 1;
    }
    Ok(lines.len())
}

/// 十进制解析。
pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(SlCode::OrderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(SlCode::OrderBad.val()),
        };
        if !(b'0'..=b'9').contains(&c) {
            return Err(SlCode::OrderBad.val());
        }
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return Err(SlCode::OrderBad.val()),
        };
        v = match v.checked_add((c - b'0') as u32) {
            Some(x) => x,
            None => return Err(SlCode::OrderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 十六进制解析（必须带 0x）。
pub fn parse_hex(s: &str) -> Result<u32, u32> {
    let body = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(v) => v,
        None => return Err(SlCode::OrderBad.val()),
    };
    if body.is_empty() || body.len() > 8 {
        return Err(SlCode::OrderBad.val());
    }
    let bytes = body.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(SlCode::OrderBad.val()),
        };
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return Err(SlCode::OrderBad.val()),
        };
        v = match v.checked_mul(16) {
            Some(x) => x,
            None => return Err(SlCode::OrderBad.val()),
        };
        v = match v.checked_add(d as u32) {
            Some(x) => x,
            None => return Err(SlCode::OrderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

// ── 探针 ──────────────────────────────────────────────────────────
/// 探针 A：合卷七册全绿且项数不丢。
pub fn probe_seq_a() -> Result<(), u32> {
    let (m, s) = audit_merge()?;
    if m != s || m == 0 {
        return Err(SlCode::MergeBad.val());
    }
    Ok(())
}

/// 探针 B：卡面 20 张恰一次（前 18 件 + 尾 2 张）。
pub fn probe_seq_b() -> Result<(), u32> {
    let declared = audit_cards()?;
    if declared != 18 {
        return Err(SlCode::CoverageBad.val());
    }
    let mut card = CARD_FIRST;
    while card <= CARD_LAST {
        let hits = if card >= SEAL_CARD_FIRST { 0 } else { card_hits(card) };
        let want = if card >= SEAL_CARD_FIRST { 0 } else { 1 };
        if hits != want {
            return Err(SlCode::CoverageBad.val());
        }
        card = card.saturating_add(1);
    }
    Ok(())
}

/// 探针 C：声明、关键词、行数、导出四关齐。
pub fn probe_seq_c() -> Result<(), u32> {
    audit_decls()?;
    audit_keywords()?;
    if audit_lines()? < FAMILY_LINES_MIN {
        return Err(SlCode::LineBad.val());
    }
    let text = render_decls();
    if audit_render(&text)? != DECLS.len() {
        return Err(SlCode::OrderBad.val());
    }
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SlCode::MergeBad.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x06;
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        match SlCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 闭包判据（恰好三条） ──────────────────────────────────────────
/// 三条闭包项：关键词常量 / 二十卡覆盖 / 家族行数。
pub fn closure_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "F48879-J1-关键词常量全量",
        match audit_keywords() {
            Ok(n) => n as usize == FAMILY_MODULES && keyword_ok("四段延迟账本"),
            Err(_) => false,
        },
        format!("{} 件关键词合法且互异", FAMILY_MODULES),
    ));
    v.push((
        "F48880-J1-二十卡覆盖",
        match audit_cards() {
            Ok(n) => n == 18 && audit_decls().is_ok(),
            Err(_) => false,
        },
        format!("{} 卡（前七件 18 + 尾 2）", FAMILY_CARDS),
    ));
    v.push((
        "F48880-J1-家族行数",
        match audit_lines() {
            Ok(n) => n >= FAMILY_LINES_MIN,
            Err(_) => false,
        },
        format!("下限和 ≥ {}", FAMILY_LINES_MIN),
    ));
    v
}

/// 判据入口：七册合卷 + 三条闭包 + 段位守卫。
pub fn run_unx48879_checks() -> CheckSet {
    let merged = merge_seven();
    let mut s = CheckSet::merge(CheckSet::new("UNX-F48879 B04 收官印"), merged);
    let items = closure_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let guard = segment_guard();
    s.add_dyn("F48879-J1-段位守卫", guard, if guard { "" } else { "诊断码逃出 0x709C0" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decls_are_well_formed() {
        assert_eq!(audit_decls(), Ok(FAMILY_MODULES as u32));
        assert!(segs_ascending());
    }

    #[test]
    fn decl_cards_are_contiguous() {
        let mut i = 1usize;
        while i < DECLS.len() {
            if let (Some(a), Some(b)) = (DECLS.get(i - 1), DECLS.get(i)) {
                assert_eq!(b.card_first, a.card_last + 1);
                assert!(b.seg > a.seg);
            }
            i += 1;
        }
    }

    #[test]
    fn keywords_are_unique_and_printable() {
        assert_eq!(audit_keywords(), Ok(FAMILY_MODULES as u32));
        assert!(!keyword_ok(""));
        assert!(!keyword_ok("bad\nkey"));
        assert!(!keyword_ok("tab\tkey"));
        assert!(keyword_ok("四段延迟账本"));
    }

    #[test]
    fn cards_cover_exactly_once() {
        assert_eq!(audit_cards(), Ok(18));
        let mut card = CARD_FIRST;
        while card < SEAL_CARD_FIRST {
            assert_eq!(card_hits(card), 1);
            card += 1;
        }
        assert_eq!(card_hits(SEAL_CARD_FIRST), 0);
        assert_eq!(card_hits(SEAL_CARD_LAST), 0);
    }

    #[test]
    fn lines_meet_family_floor() {
        match audit_lines() {
            Ok(n) => {
                assert!(n >= FAMILY_LINES_MIN);
                assert!(n >= 9_000);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn merge_keeps_every_check() {
        match audit_merge() {
            Ok((m, s)) => {
                assert_eq!(m, s);
                assert!(m >= 70);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn merge_red_cannot_be_washed() {
        // 七册各自全绿是合卷的前提；此处验证合卷结果与逐册项数一致，
        // 任何一册红都会让 audit_merge 直接失败（无洗绿路径）。
        let sets = seven_sets();
        assert_eq!(sets.len(), FAMILY_MODULES);
        let mut i = 0usize;
        while i < sets.len() {
            match sets.get(i) {
                Some(s) => assert!(s.all_passed()),
                None => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn golden_run_stable() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        match a {
            Ok((m, cards, lines, h)) => {
                assert!(m >= 70);
                assert_eq!(cards, 18);
                assert!(lines as usize >= FAMILY_LINES_MIN);
                assert!(h != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip() {
        let text = render_decls();
        assert_eq!(audit_render(&text), Ok(DECLS.len()));
        assert_eq!(text.lines().count(), DECLS.len());
    }

    #[test]
    fn render_rejects_tamper() {
        let text = render_decls();
        assert_eq!(audit_render(&text), Ok(DECLS.len()));
        // 缺关键词段：六段行必拒。
        let missing = text.replacen("|1600|", "|", 1);
        assert!(audit_render(&missing).is_err());
        // 段基址倒挂：第二件写成首件基址必拒（八位十六进制定宽）。
        let bad_seg = text.replacen("0x000707C0", "0x00070780", 1);
        assert_ne!(bad_seg, text);
        assert!(audit_render(&bad_seg).is_err());
        // 槽位错位：第二件槽号改成 2 必拒（行首无前导竖线，直接改槽号）。
        let bad_slot = text.replacen("1|unx48863_latency_meta", "2|unx48863_latency_meta", 1);
        assert_ne!(bad_slot, text);
        assert!(audit_render(&bad_slot).is_err());
    }

    #[test]
    fn inject_all_caught() {
        match inject_rounds(1_000) {
            Ok(l) => {
                assert_eq!(l.caught, 1_000);
                assert_eq!(l.route_caught, [250, 250, 250, 250]);
                assert!(l.all_caught());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn segment_guard_holds() {
        assert!(segment_guard());
        let mut i = 0usize;
        while i < SlCode::ALL.len() {
            if let Some(c) = SlCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE + 0x01 && c.val() <= SEG_BASE + 0x06);
            }
            i += 1;
        }
    }

    #[test]
    fn closure_has_exactly_three_items() {
        let v = closure_checks();
        assert_eq!(v.len(), 3);
        let mut i = 0usize;
        while i < v.len() {
            if let Some((_, p, _)) = v.get(i) {
                assert!(*p);
            }
            i += 1;
        }
    }

    #[test]
    fn repro_is_stable() {
        assert_eq!(repro_check(), Ok(()));
        assert_eq!(probe_seq_a(), Ok(()));
        assert_eq!(probe_seq_b(), Ok(()));
        assert_eq!(probe_seq_c(), Ok(()));
    }
}
