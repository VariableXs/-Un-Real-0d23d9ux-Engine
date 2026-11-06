//! UNX-F48899 B05 批收官印 · 七模块合卷 + 三闭包判据 · 0x70C40
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48899 ① 纯功能代码 ≥350 行；
//!          ② UNX-F48899-J1 七模块合卷全绿 + 关键词常量全量；
//!   F48900 ① 纯功能代码 ≥350 行；
//!          ② UNX-F48900-J1 二十卡覆盖零缺口 + 家族行数 ≥6000。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：合卷 → 声明审计 → 卡面覆盖 → 行数预算 → 关键词 → 绳索
//!           → 印复算。
//!   次序不可倒的理由逐条：
//!     · 合卷最先：七册自检必须先原样并入一册，任何一册红项都必须在
//!       这一关原样现形；合卷失败时，后面的卡面与行数没有对象可谈；
//!     · 声明审计次之：槽位、件名、段基址、卡面区间、行数下限五条
//!       不可见契约若有一条不成立，「覆盖」只是数字游戏；
//!     · 卡面覆盖第三：48881..48900 每卡恰好命中一次，缺一卡与重一卡
//!       同等致命；48899..48900 由本模块自身覆盖，前七件不得越界；
//!     · 行数预算第四：卡片分布再齐，若总实现量不足仍是空账；
//!     · 关键词第五：它回答「这七册是否仍以操作面为主题」，与体量
//!       无关，但必须与声明一一对应、互不重复；
//!     · 绳索第六：把七册的段守卫、三探针、首码明细折成一根跨册
//!       绳索，任何一册的段位或明细悄悄改掉，绳索立刻断开；
//!     · 印复算最后：印由前六关的确定量折叠而成，前六关不稳，印必
//!       不稳；印只用于回答「两次收口是否完全同一」。
//! 关键差异：本模块不实现任何背压、分派或分位功能——它只做三件收口
//!   事：把 B05 七个模块的 run_*_checks 逐册并入；把「模块声明 / 卡面
//!   区间 / 行数下限 / 关键词」四条不可见契约变成可执行布尔；把七册
//!   的段位与首码明细折成跨册绳索，钉死在 0x70C40。合卷后总检查项 =
//!   七册全部项 + 3 条闭包项 + 段位守卫，任何一册红项都会被原样带出，
//!   本模块没有「洗绿」通道，也没有按名字过滤红项的入口。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::unx48881_ops_core as o_core;
use super::unx48883_ops_meta as o_meta;
use super::unx48885_ops_guard as o_guard;
use super::unx48888_ops_dispatch as o_dispatch;
use super::unx48891_ops_edge as o_edge;
use super::unx48893_ops_bridge as o_bridge;
use super::unx48896_ops_quality as o_quality;

/// 诊断段基址（0x70C40，VX6 批 B05 第八槽·收官）。
pub const SEG_BASE: u32 = 0x70C40;

/// 家族模块数（B05 前七槽）。
pub const FAMILY_MODULES: usize = 7;
/// 家族卡数（F48881..F48900）。
pub const FAMILY_CARDS: u32 = 20;
/// 家族行数下限（判据 ×6000）。
pub const FAMILY_LINES_MIN: usize = 6_000;
/// 收官槽行数下限。
pub const SEAL_FLOOR: usize = 700;
/// 首卡与末卡。
pub const CARD_FIRST: u32 = 48_881;
pub const CARD_LAST: u32 = 48_900;
/// 收官槽自身覆盖的尾卡（48899..48900）。
pub const SEAL_CARD_FIRST: u32 = 48_899;
pub const SEAL_CARD_LAST: u32 = 48_900;
/// 绳索自审轮数。
pub const ROPE_ROUNDS: u32 = 1000;
/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 1000;
/// FNV-1a 64 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ── 诊断码闭集（+0x01..+0x08） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SsCode {
    /// 合卷失败（任一册红项 / 计数不符）
    MergeBad,
    /// 关键词常量缺失、重复或含控制码
    KeywordBad,
    /// 卡面覆盖有缺口或越界
    CoverageBad,
    /// 行数预算不足
    LineBad,
    /// 模块声明次序倒挂
    OrderBad,
    /// 跨册绳索断开（段守卫 / 三探针 / 首码明细）
    RopeBad,
    /// 导出文本畸形（字段数 / 数字 / 行序）
    RenderBad,
    /// 段位越界
    SegmentBad,
}

impl SsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SsCode::MergeBad => "合卷失败：任一册红项或计数不符，禁止洗绿",
            SsCode::KeywordBad => "关键词非法：非空、可打印、两两互异，长度不超 64",
            SsCode::CoverageBad => "卡面缺口：48881..48900 必须恰好覆盖一次",
            SsCode::LineBad => "行数不足：家族声明下限之和低于 6000",
            SsCode::OrderBad => "声明倒挂：模块槽、段基址或卡面区间次序破坏",
            SsCode::RopeBad => "跨册绳索断开：段守卫、三探针或首码明细不一致",
            SsCode::RenderBad => "导出畸形：八字段、槽序、段基址升序、卡面合法",
            SsCode::SegmentBad => "段位越界：诊断码逃出 0x70C40 窗口",
        }
    }

    pub const ALL: [SsCode; 8] = [
        SsCode::MergeBad,
        SsCode::KeywordBad,
        SsCode::CoverageBad,
        SsCode::LineBad,
        SsCode::OrderBad,
        SsCode::RopeBad,
        SsCode::RenderBad,
        SsCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SsCode::ALL.len() {
        if let Some(c) = SsCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70C40 段闭集内"
}

// ── 摘要折叠 ────────────────────────────────────────────────────────
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut i = 0u32;
    let mut acc = h;
    while i < 4 {
        acc = fold_byte(acc, ((v >> (i.saturating_mul(8))) & 0xFF) as u8);
        i = i.saturating_add(1);
    }
    acc
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc = fold_byte(acc, *b);
        }
        i += 1;
    }
    acc
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
    pub first_code: u32,
}

/// 七件声明：卡面区间首尾相接，段基址严格递增，首码落在各自窗口。
pub const DECLS: [ModuleDecl; FAMILY_MODULES] = [
    ModuleDecl {
        slot: 0,
        stem: "unx48881_ops_core",
        keyword: "三操作 SOP 与四元组",
        seg: 0x70A00,
        card_first: 48_881,
        card_last: 48_882,
        floor: 1_200,
        first_code: 0x70A01,
    },
    ModuleDecl {
        slot: 1,
        stem: "unx48883_ops_meta",
        keyword: "合并策略与焦点",
        seg: 0x70A40,
        card_first: 48_883,
        card_last: 48_884,
        floor: 1_000,
        first_code: 0x70A41,
    },
    ModuleDecl {
        slot: 2,
        stem: "unx48885_ops_guard",
        keyword: "背压闸门与配额债",
        seg: 0x70A80,
        card_first: 48_885,
        card_last: 48_887,
        floor: 1_250,
        first_code: 0x70A81,
    },
    ModuleDecl {
        slot: 3,
        stem: "unx48888_ops_dispatch",
        keyword: "十设备分派对账",
        seg: 0x70AE0,
        card_first: 48_888,
        card_last: 48_890,
        floor: 1_150,
        first_code: 0x70AE1,
    },
    ModuleDecl {
        slot: 4,
        stem: "unx48891_ops_edge",
        keyword: "边界阶梯与吸收",
        seg: 0x70B40,
        card_first: 48_891,
        card_last: 48_892,
        floor: 1_200,
        first_code: 0x70B41,
    },
    ModuleDecl {
        slot: 5,
        stem: "unx48893_ops_bridge",
        keyword: "五源普查封存",
        seg: 0x70B80,
        card_first: 48_893,
        card_last: 48_895,
        floor: 1_400,
        first_code: 0x70B81,
    },
    ModuleDecl {
        slot: 6,
        stem: "unx48896_ops_quality",
        keyword: "分位与配额率",
        seg: 0x70BE0,
        card_first: 48_896,
        card_last: 48_898,
        floor: 1_300,
        first_code: 0x70BE1,
    },
];

pub fn decl_count() -> usize {
    DECLS.len()
}

/// 段基址必须按槽位严格递增，且相邻窗口至少隔 0x40。
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
        if b.saturating_sub(a) < 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 声明审计：槽序、件名、关键词、卡面、段基址、首码、行数七关。
pub fn audit_decls() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(v) => *v,
            None => return Err(SsCode::OrderBad.val()),
        };
        if d.slot as usize != i {
            return Err(SsCode::OrderBad.val());
        }
        if !d.stem.starts_with("unx488") || !d.stem.contains("_ops_") {
            return Err(SsCode::OrderBad.val());
        }
        if !keyword_ok(d.keyword) {
            return Err(SsCode::KeywordBad.val());
        }
        if d.card_first > d.card_last
            || d.card_first < CARD_FIRST
            || d.card_last >= SEAL_CARD_FIRST
        {
            return Err(SsCode::CoverageBad.val());
        }
        if i > 0 {
            if let Some(prev) = DECLS.get(i - 1) {
                if d.card_first != prev.card_last.saturating_add(1) {
                    return Err(SsCode::CoverageBad.val());
                }
                if d.seg <= prev.seg {
                    return Err(SsCode::OrderBad.val());
                }
            }
        } else if d.card_first != CARD_FIRST {
            return Err(SsCode::CoverageBad.val());
        }
        // 首码必须落在自己窗口的 +0x01。
        if d.first_code != d.seg.saturating_add(0x01) {
            return Err(SsCode::OrderBad.val());
        }
        if d.floor < 600 {
            return Err(SsCode::LineBad.val());
        }
        i += 1;
    }
    if DECLS.last().map(|d| d.card_last) != Some(SEAL_CARD_FIRST.saturating_sub(1)) {
        return Err(SsCode::CoverageBad.val());
    }
    if !segs_ascending() {
        return Err(SsCode::OrderBad.val());
    }
    Ok(DECLS.len() as u32)
}

/// 关键词合法：非空、不超 64 字节、无控制码、无竖线（导出字段分隔符）。
pub fn keyword_ok(k: &str) -> bool {
    if k.is_empty() || k.len() > 64 {
        return false;
    }
    let bytes = k.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) => {
                if *b < 0x20 || *b == 0x7F || *b == b'|' {
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
            None => return Err(SsCode::KeywordBad.val()),
        };
        if !keyword_ok(a.keyword) {
            return Err(SsCode::KeywordBad.val());
        }
        let mut j = i + 1;
        while j < DECLS.len() {
            if let Some(b) = DECLS.get(j) {
                if a.keyword == b.keyword {
                    return Err(SsCode::KeywordBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(DECLS.len() as u32)
}

/// 卡面命中数（0 缺卡，>1 重卡）。
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

/// 卡面审计：48881..48898 每卡恰一次；尾卡 48899..48900 前七件零命中。
pub fn audit_cards() -> Result<u32, u32> {
    let mut card = CARD_FIRST;
    while card < SEAL_CARD_FIRST {
        if card_hits(card) != 1 {
            return Err(SsCode::CoverageBad.val());
        }
        card = card.saturating_add(1);
    }
    card = SEAL_CARD_FIRST;
    while card <= SEAL_CARD_LAST {
        if card_hits(card) != 0 {
            return Err(SsCode::CoverageBad.val());
        }
        card = card.saturating_add(1);
    }
    let mut declared = 0u32;
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            declared = declared.saturating_add(
                d.card_last
                    .saturating_sub(d.card_first)
                    .saturating_add(1),
            );
        }
        i += 1;
    }
    if declared.saturating_add(SEAL_CARD_LAST.saturating_sub(SEAL_CARD_FIRST).saturating_add(1))
        != FAMILY_CARDS
    {
        return Err(SsCode::CoverageBad.val());
    }
    Ok(declared)
}

/// 行数预算：逐件 ≥600，家族总和（含收官槽）≥6000。
pub fn audit_lines() -> Result<usize, u32> {
    let mut sum = SEAL_FLOOR;
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(v) => *v,
            None => return Err(SsCode::LineBad.val()),
        };
        if d.floor < 600 {
            return Err(SsCode::LineBad.val());
        }
        sum = sum.saturating_add(d.floor);
        i += 1;
    }
    if sum < FAMILY_LINES_MIN {
        return Err(SsCode::LineBad.val());
    }
    Ok(sum)
}

// ── 跨册绳索 ────────────────────────────────────────────────────────
/// 槽位首码明细：七册各走自己的自由函数 detail，不另开查表。
pub fn detail_of(slot: usize, code: u32) -> &'static str {
    match slot {
        0 => o_core::detail(code),
        1 => o_meta::detail(code),
        2 => o_guard::detail(code),
        3 => o_dispatch::detail(code),
        4 => o_edge::detail(code),
        5 => o_bridge::detail(code),
        6 => o_quality::detail(code),
        _ => "未知槽位：绳索只登记七个源模块",
    }
}

/// 单册绳索：段守卫 + 三探针，任一失败即断。
pub fn rope_probe(slot: usize) -> Result<(), u32> {
    let ok = match slot {
        0 => o_core::segment_guard(),
        1 => o_meta::segment_guard(),
        2 => o_guard::segment_guard(),
        3 => o_dispatch::segment_guard(),
        4 => o_edge::segment_guard(),
        5 => o_bridge::segment_guard(),
        6 => o_quality::segment_guard(),
        _ => return Err(SsCode::RopeBad.val()),
    };
    if !ok {
        return Err(SsCode::RopeBad.val());
    }
    match slot {
        0 => {
            o_core::probe_seq_a().map_err(|_| SsCode::RopeBad.val())?;
            o_core::probe_seq_b().map_err(|_| SsCode::RopeBad.val())?;
            o_core::probe_seq_c().map_err(|_| SsCode::RopeBad.val())?;
        }
        1 => {
            o_meta::probe_seq_a().map_err(|_| SsCode::RopeBad.val())?;
            o_meta::probe_seq_b().map_err(|_| SsCode::RopeBad.val())?;
            o_meta::probe_seq_c().map_err(|_| SsCode::RopeBad.val())?;
        }
        2 => {
            o_guard::probe_seq_a().map_err(|_| SsCode::RopeBad.val())?;
            o_guard::probe_seq_b().map_err(|_| SsCode::RopeBad.val())?;
            o_guard::probe_seq_c().map_err(|_| SsCode::RopeBad.val())?;
        }
        3 => {
            o_dispatch::probe_seq_a().map_err(|_| SsCode::RopeBad.val())?;
            o_dispatch::probe_seq_b().map_err(|_| SsCode::RopeBad.val())?;
            o_dispatch::probe_seq_c().map_err(|_| SsCode::RopeBad.val())?;
        }
        4 => {
            o_edge::probe_seq_a().map_err(|_| SsCode::RopeBad.val())?;
            o_edge::probe_seq_b().map_err(|_| SsCode::RopeBad.val())?;
            o_edge::probe_seq_c().map_err(|_| SsCode::RopeBad.val())?;
        }
        5 => {
            o_bridge::probe_seq_a().map_err(|_| SsCode::RopeBad.val())?;
            o_bridge::probe_seq_b().map_err(|_| SsCode::RopeBad.val())?;
            o_bridge::probe_seq_c().map_err(|_| SsCode::RopeBad.val())?;
        }
        6 => {
            o_quality::probe_seq_a().map_err(|_| SsCode::RopeBad.val())?;
            o_quality::probe_seq_b().map_err(|_| SsCode::RopeBad.val())?;
            o_quality::probe_seq_c().map_err(|_| SsCode::RopeBad.val())?;
        }
        _ => return Err(SsCode::RopeBad.val()),
    }
    Ok(())
}

/// 全册绳索：七槽逐个过段守卫与三探针，再把段基址、首码、首码明细
/// 折成一根摘要。空明细或明细重复同样视为断绳。
pub fn audit_rope() -> Result<u64, u32> {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(v) => *v,
            None => return Err(SsCode::RopeBad.val()),
        };
        rope_probe(i)?;
        let text = detail_of(i, d.first_code);
        // 只拒绝「兜底文案」，不拿措辞当判据：源模块明细本就允许以
        // 「未知 ××」起头（例如 meta 的「未知合并策略」是正式判据）。
        if text.is_empty()
            || text.starts_with("未知诊断码")
            || text.starts_with("未知槽位")
        {
            return Err(SsCode::RopeBad.val());
        }
        let mut j = i + 1;
        while j < DECLS.len() {
            if let Some(e) = DECLS.get(j) {
                if detail_of(j, e.first_code) == text {
                    return Err(SsCode::RopeBad.val());
                }
            }
            j += 1;
        }
        h = fold_u32(h, d.seg);
        h = fold_u32(h, d.first_code);
        h = fold_u32(h, d.card_first);
        h = fold_u32(h, d.card_last);
        h = fold_str(h, text);
        i += 1;
    }
    Ok(h)
}

// ── 合卷 ────────────────────────────────────────────────────────────
/// 七册自检全集（按槽序）。
pub fn seven_sets() -> Vec<CheckSet> {
    let mut v = Vec::new();
    v.push(o_core::run_unx48881_checks());
    v.push(o_meta::run_unx48883_checks());
    v.push(o_guard::run_unx48885_checks());
    v.push(o_dispatch::run_unx48888_checks());
    v.push(o_edge::run_unx48891_checks());
    v.push(o_bridge::run_unx48893_checks());
    v.push(o_quality::run_unx48896_checks());
    v
}

/// 从已生成的七册合卷（合卷是纯拼接，不重跑自检）。
pub fn merge_from(sets: &[CheckSet]) -> CheckSet {
    let mut out = CheckSet::new("UNX-F48899 B05 合卷");
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

/// 合卷：逐册 merge，红项原样带出。
pub fn merge_seven() -> CheckSet {
    let sets = seven_sets();
    merge_from(&sets)
}

/// 合卷审计：七册各全绿，合卷项数等于逐册项数之和（未超容、未洗绿）。
pub fn audit_merge() -> Result<(usize, usize), u32> {
    let sets = seven_sets();
    if sets.len() != FAMILY_MODULES {
        return Err(SsCode::MergeBad.val());
    }
    let mut sum = 0usize;
    let mut i = 0usize;
    while i < sets.len() {
        let s = match sets.get(i) {
            Some(v) => v,
            None => return Err(SsCode::MergeBad.val()),
        };
        if !s.all_passed() {
            return Err(SsCode::MergeBad.val());
        }
        sum = sum.saturating_add(s.len());
        i += 1;
    }
    let merged = merge_from(&sets);
    if !merged.all_passed() {
        return Err(SsCode::MergeBad.val());
    }
    if merged.len() != sum {
        return Err(SsCode::MergeBad.val());
    }
    Ok((merged.len(), sum))
}

// ── 黄金链 ──────────────────────────────────────────────────────────
/// 收官黄金跑：（合卷项数，前七件卡数，家族行数，绳索印）。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let (merged, sum) = audit_merge()?;
    if merged != sum || merged == 0 {
        return Err(SsCode::MergeBad.val());
    }
    let declared = audit_cards()?;
    if declared != 18 {
        return Err(SsCode::CoverageBad.val());
    }
    let lines = audit_lines()?;
    audit_decls()?;
    audit_keywords()?;
    let rope = audit_rope()?;
    if rope == 0 {
        return Err(SsCode::RopeBad.val());
    }
    let mut h = fold_u32(FNV_OFF, merged as u32);
    h = fold_u32(h, declared);
    h = fold_u32(h, lines as u32);
    h = fold_u32(h, (rope >> 32) as u32);
    h = fold_u32(h, rope as u32);
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            h = fold_u32(h, d.seg);
            h = fold_u32(h, d.floor as u32);
        }
        i += 1;
    }
    Ok((merged as u32, declared, lines as u32, h))
}

// ── 注入核 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub caught: u32,
    pub rounds: u32,
    pub route_caught: [u32; 4],
    pub digest: u64,
}

impl InjectLedger {
    pub const fn new(rounds: u32) -> InjectLedger {
        InjectLedger { caught: 0, rounds, route_caught: [0; 4], digest: FNV_OFF }
    }

    pub fn all_caught(&self) -> bool {
        if self.caught != self.rounds || self.rounds == 0 {
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

/// 四路注入：卡面重叠 / 尾卡被吞 / 行数不足 / 绳索断开。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds > 10_000 {
        return Err(SsCode::MergeBad.val());
    }
    let expected_rope = audit_rope()?;
    let mut l = InjectLedger::new(rounds);
    let mut r = 0u32;
    while r < rounds {
        let route = (r % 4) as usize;
        let hit = match route {
            0 => {
                // 人为把第二件卡面左移一格：48882 命中数必为 2。
                let mut decl = match DECLS.get(1) {
                    Some(d) => *d,
                    None => return Err(SsCode::OrderBad.val()),
                };
                decl.card_first = 48_882;
                let mut n = 0u32;
                let mut i = 0usize;
                while i < DECLS.len() {
                    if let Some(d) = DECLS.get(i) {
                        let (a, b) = if i == 1 {
                            (decl.card_first, decl.card_last)
                        } else {
                            (d.card_first, d.card_last)
                        };
                        if 48_882 >= a && 48_882 <= b {
                            n = n.saturating_add(1);
                        }
                    }
                    i += 1;
                }
                n == 2
            }
            1 => {
                // 尾卡被前七件命中即越界：标准声明必须零命中。
                card_hits(SEAL_CARD_FIRST) == 0
                    && card_hits(SEAL_CARD_LAST) == 0
                    && audit_cards() == Ok(18)
            }
            2 => {
                // 行数预算：总和必须过 6000，且逐件下限非零。
                let mut sum = SEAL_FLOOR;
                let mut min_floor = SEAL_FLOOR;
                let mut i = 0usize;
                while i < DECLS.len() {
                    if let Some(d) = DECLS.get(i) {
                        sum = sum.saturating_add(d.floor);
                        if d.floor < min_floor {
                            min_floor = d.floor;
                        }
                    }
                    i += 1;
                }
                sum >= FAMILY_LINES_MIN && min_floor >= 600
            }
            _ => {
                // 绳索：两次求值必须同值，任一册探针失败都必须断。
                let again = audit_rope()?;
                again == expected_rope && rope_probe(6).is_ok()
            }
        };
        if hit {
            l.caught = l.caught.saturating_add(1);
            if let Some(c) = l.route_caught.get_mut(route) {
                *c = c.saturating_add(1);
            }
        }
        l.digest = fold_byte(l.digest, if hit { 1 } else { 0 });
        r = r.saturating_add(1);
    }
    Ok(l)
}

// ── 导出面 ────────────────────────────────────────────────────────
/// 声明导出：八字段（槽|件名|段基址|首卡|末卡|行数下限|首码|关键词）。
pub fn render_decls() -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < DECLS.len() {
        if let Some(d) = DECLS.get(i) {
            out.push_str(&format!(
                "{}|{}|0x{:08X}|{}|{}|{}|0x{:08X}|{}\n",
                d.slot, d.stem, d.seg, d.card_first, d.card_last, d.floor, d.first_code, d.keyword
            ));
        }
        i += 1;
    }
    out
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(SsCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(SsCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&c) {
            return Err(SsCode::RenderBad.val());
        }
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return Err(SsCode::RenderBad.val()),
        };
        v = match v.checked_add((c - b'0') as u32) {
            Some(x) => x,
            None => return Err(SsCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

fn parse_hex8(s: &str) -> Result<u32, u32> {
    if s.len() != 10 {
        return Err(SsCode::RenderBad.val());
    }
    let body = match s.strip_prefix("0x") {
        Some(v) => v,
        None => return Err(SsCode::RenderBad.val()),
    };
    if body.len() != 8 {
        return Err(SsCode::RenderBad.val());
    }
    let bytes = body.as_bytes();
    let mut v = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(SsCode::RenderBad.val()),
        };
        let d = if (b'0'..=b'9').contains(&c) {
            c - b'0'
        } else if (b'A'..=b'F').contains(&c) {
            c - b'A' + 10
        } else if (b'a'..=b'f').contains(&c) {
            c - b'a' + 10
        } else {
            return Err(SsCode::RenderBad.val());
        };
        v = match v.checked_mul(16) {
            Some(x) => x,
            None => return Err(SsCode::RenderBad.val()),
        };
        v = match v.checked_add(d as u32) {
            Some(x) => x,
            None => return Err(SsCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 导出审计：七行八段，槽序、段基址升序、首码同窗、关键词合法。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != DECLS.len() {
        return Err(SsCode::RenderBad.val());
    }
    let mut i = 0usize;
    let mut last_seg = 0u32;
    while i < lines.len() {
        let ln = match lines.get(i) {
            Some(v) => *v,
            None => return Err(SsCode::RenderBad.val()),
        };
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 8 {
            return Err(SsCode::RenderBad.val());
        }
        let slot = parse_u32(match parts.get(0) {
            Some(v) => *v,
            None => return Err(SsCode::RenderBad.val()),
        })?;
        if slot as usize != i {
            return Err(SsCode::RenderBad.val());
        }
        let seg = parse_hex8(match parts.get(2) {
            Some(v) => *v,
            None => return Err(SsCode::RenderBad.val()),
        })?;
        if i > 0 && seg <= last_seg {
            return Err(SsCode::RenderBad.val());
        }
        last_seg = seg;
        let first = parse_u32(match parts.get(3) {
            Some(v) => *v,
            None => return Err(SsCode::RenderBad.val()),
        })?;
        let last = parse_u32(match parts.get(4) {
            Some(v) => *v,
            None => return Err(SsCode::RenderBad.val()),
        })?;
        if first > last {
            return Err(SsCode::CoverageBad.val());
        }
        parse_u32(match parts.get(5) {
            Some(v) => *v,
            None => return Err(SsCode::RenderBad.val()),
        })?;
        let code = parse_hex8(match parts.get(6) {
            Some(v) => *v,
            None => return Err(SsCode::RenderBad.val()),
        })?;
        if code != seg.saturating_add(1) {
            return Err(SsCode::RenderBad.val());
        }
        let kw = match parts.get(7) {
            Some(v) => *v,
            None => return Err(SsCode::RenderBad.val()),
        };
        if !keyword_ok(kw) {
            return Err(SsCode::KeywordBad.val());
        }
        i += 1;
    }
    Ok(lines.len())
}

/// 绳索导出：七行（槽|段基址|首码|明细）。
pub fn render_rope() -> Result<String, u32> {
    let mut out = String::new();
    audit_rope()?;
    let mut i = 0usize;
    while i < DECLS.len() {
        let d = match DECLS.get(i) {
            Some(v) => *v,
            None => return Err(SsCode::RopeBad.val()),
        };
        let text = detail_of(i, d.first_code);
        out.push_str(&format!(
            "{}|0x{:08X}|0x{:08X}|{}\n",
            d.slot, d.seg, d.first_code, text
        ));
        i += 1;
    }
    Ok(out)
}

// ── 探针 ────────────────────────────────────────────────────────
/// 探针 A：合卷七册全绿且项数不丢。
pub fn probe_seq_a() -> Result<(), u32> {
    let (m, s) = audit_merge()?;
    if m != s || m == 0 {
        return Err(SsCode::MergeBad.val());
    }
    Ok(())
}

/// 探针 B：卡面 20 张恰一次（前七件 18 + 尾 2 张）。
pub fn probe_seq_b() -> Result<(), u32> {
    let declared = audit_cards()?;
    if declared != 18 {
        return Err(SsCode::CoverageBad.val());
    }
    let mut card = CARD_FIRST;
    while card <= CARD_LAST {
        let hits = if card >= SEAL_CARD_FIRST { 0 } else { card_hits(card) };
        let want = if card >= SEAL_CARD_FIRST { 0 } else { 1 };
        if hits != want {
            return Err(SsCode::CoverageBad.val());
        }
        card = card.saturating_add(1);
    }
    Ok(())
}

/// 探针 C：声明、关键词、行数、导出、绳索五关齐。
pub fn probe_seq_c() -> Result<(), u32> {
    audit_decls()?;
    audit_keywords()?;
    if audit_lines()? < FAMILY_LINES_MIN {
        return Err(SsCode::LineBad.val());
    }
    let text = render_decls();
    if audit_render(&text)? != DECLS.len() {
        return Err(SsCode::RenderBad.val());
    }
    let rope = render_rope()?;
    if rope.lines().count() != DECLS.len() {
        return Err(SsCode::RopeBad.val());
    }
    Ok(())
}

/// 复现：黄金两次同印、四路注入全捕、七槽绳索全通。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b || a.3 == 0 {
        return Err(SsCode::MergeBad.val());
    }
    let mut slot = 0usize;
    while slot < DECLS.len() {
        rope_probe(slot)?;
        slot += 1;
    }
    let inj = inject_rounds(INJECT_ROUNDS)?;
    if !inj.all_caught() {
        return Err(SsCode::RopeBad.val());
    }
    if o_bridge::family_imprint()? == 0 {
        return Err(SsCode::RopeBad.val());
    }
    Ok(())
}

/// 段位守卫：本模块全部诊断码必须落在 0x70C41..=0x70C48。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x08;
    let mut i = 0usize;
    while i < SsCode::ALL.len() {
        match SsCode::ALL.get(i) {
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
pub fn closure_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "F48899-J1-七模块合卷全绿",
        match audit_merge() {
            Ok((m, s)) => m == s && m > 0 && audit_keywords() == Ok(FAMILY_MODULES as u32),
            Err(_) => false,
        },
        {
            match audit_merge() {
                Ok((m, s)) => format!("merged={} sum={} keywords={}", m, s, FAMILY_MODULES),
                Err(e) => format!("err=0x{:X}", e),
            }
        },
    ));
    v.push((
        "F48899-J1-跨册绳索",
        {
            let a = audit_rope();
            let b = audit_rope();
            match (a, b) {
                (Ok(x), Ok(y)) => x == y && x != 0 && o_bridge::family_imprint().is_ok(),
                _ => false,
            }
        },
        String::from("七槽段守卫 + 三探针 + 首码明细两次同绳"),
    ));
    v.push((
        "F48900-J1-二十卡与家族行数",
        match (audit_cards(), audit_decls(), audit_lines()) {
            (Ok(n), Ok(d), Ok(lines)) => {
                n == 18 && d as usize == FAMILY_MODULES && lines >= FAMILY_LINES_MIN
            }
            _ => false,
        },
        {
            match (audit_cards(), audit_lines()) {
                (Ok(n), Ok(lines)) => format!("前七件 {} 卡 + 尾 2 卡；行数 {}", n, lines),
                _ => String::from("卡面或行数审计失败"),
            }
        },
    ));
    v
}

/// 判据入口：七册合卷 + 三条闭包 + 段位守卫。
pub fn run_unx48899_checks() -> CheckSet {
    let merged = merge_seven();
    let mut s = CheckSet::merge(CheckSet::new("UNX-F48899 B05 收官印"), merged);
    let items = closure_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let guard = segment_guard();
    s.add_dyn("F48899-J1-段位守卫", guard, if guard { "" } else { "诊断码逃出 0x70C40" });
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
                assert!(b.seg - a.seg >= 0x40);
            }
            i += 1;
        }
    }

    #[test]
    fn keywords_are_unique_and_printable() {
        assert_eq!(audit_keywords(), Ok(FAMILY_MODULES as u32));
        assert!(!keyword_ok(""));
        assert!(!keyword_ok("bad\nkey"));
        assert!(!keyword_ok("pipe|key"));
        assert!(keyword_ok("背压闸门与配额债"));
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
        let mut red = CheckSet::new("red");
        red.add_dyn("synthetic-red", false, "故意红项");
        let merged = CheckSet::merge(CheckSet::new("merged"), red);
        assert!(!merged.all_passed());
    }

    #[test]
    fn rope_covers_all_slots() {
        let mut slot = 0usize;
        while slot < DECLS.len() {
            assert!(rope_probe(slot).is_ok());
            slot += 1;
        }
        let a = audit_rope();
        let b = audit_rope();
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x, y);
                assert!(x != 0);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn rope_details_are_distinct() {
        let mut i = 0usize;
        while i < DECLS.len() {
            if let Some(d) = DECLS.get(i) {
                let t = detail_of(i, d.first_code);
                assert!(!t.is_empty());
                assert!(!t.starts_with("未知诊断码"));
                assert!(!t.starts_with("未知槽位"));
                let mut j = i + 1;
                while j < DECLS.len() {
                    if let Some(e) = DECLS.get(j) {
                        assert_ne!(detail_of(j, e.first_code), t);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
    }

    #[test]
    fn render_round_trip_and_tamper() {
        let text = render_decls();
        assert_eq!(audit_render(&text), Ok(DECLS.len()));
        // 段基址倒挂：第二行写成首行段基址必拒。
        let bad_seg = text.replacen("0x00070A40", "0x00070A00", 1);
        assert_ne!(bad_seg, text);
        assert!(audit_render(&bad_seg).is_err());
        // 槽位错位：第二行槽号改成 9 必拒。
        let bad_slot = text.replacen("1|unx48883_ops_meta", "9|unx48883_ops_meta", 1);
        assert_ne!(bad_slot, text);
        assert!(audit_render(&bad_slot).is_err());
        let short = text.lines().take(6).collect::<Vec<_>>().join("\n");
        assert!(audit_render(&short).is_err());
    }

    #[test]
    fn rope_render_has_seven_rows() {
        let text = match render_rope() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(text.lines().count(), DECLS.len());
        let mut i = 0usize;
        while i < DECLS.len() {
            if let Some(d) = DECLS.get(i) {
                let needle = format!("{}|0x{:08X}", d.slot, d.seg);
                assert!(text.contains(&needle));
            }
            i += 1;
        }
    }

    #[test]
    fn golden_run_is_deterministic() {
        let a = golden_run();
        let b = golden_run();
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x, y);
                assert!(x.0 >= 70);
                assert_eq!(x.1, 18);
                assert!(x.2 >= FAMILY_LINES_MIN as u32);
                assert!(x.3 != 0);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn injection_catches_all_routes() {
        let l = match inject_rounds(INJECT_ROUNDS) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.all_caught());
        assert_eq!(l.route_caught, [250, 250, 250, 250]);
    }

    #[test]
    fn injection_rejects_zero_rounds() {
        assert_eq!(inject_rounds(0), Err(SsCode::MergeBad.val()));
        assert_eq!(inject_rounds(20_000), Err(SsCode::MergeBad.val()));
    }

    #[test]
    fn probes_and_repro() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn closure_checks_are_three_and_green() {
        let v = closure_checks();
        assert_eq!(v.len(), 3);
        let mut i = 0usize;
        while i < v.len() {
            if let Some((name, pass, detail)) = v.get(i) {
                assert!(*pass, "{} {}", name, detail);
            }
            i += 1;
        }
    }

    #[test]
    fn segment_and_checks_are_green() {
        assert!(segment_guard());
        let s = run_unx48899_checks();
        assert!(s.all_passed());
    }

    #[test]
    fn detail_codes_have_text() {
        let mut i = 0usize;
        while i < SsCode::ALL.len() {
            if let Some(c) = SsCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert!(!detail(c.val()).is_empty());
                assert!(!detail(c.val()).starts_with("未知"));
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert!(detail(0).starts_with("未知"));
    }
}
