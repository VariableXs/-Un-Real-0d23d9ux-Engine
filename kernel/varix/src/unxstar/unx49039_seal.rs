//! UNX-F49039 B12 批防重 grep（关键词族：fullkeymap/led/autorep）· 0x71D80
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第十槽；任务板 J1 引用按原文保留）：
//!   UNX-F49039 B12 批防重 grep（关键词族：fullkeymap/led/autorep）:
//!          ① 纯功能代码 ≥200 行；
//!          ② 判据原文：UNX-F49040-J1 批印入册：72000/240,000；判据锚定 §AI-62 §7。
//!   UNX-F49040 B12 批收官印（20 条冻结/域账累计 72 000 行）:
//!          ① 纯功能代码 ≥420 行；
//!          ② 判据原文：UNX-F49041-J1 递送 ×10^2 零串窗；
//!             判据锚定 §AI-62 §2 专题六 · §AI-62 §8 联签对 AI-26。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（防重 grep）：词法 → 归属 → 入册 → 去重 → 复跑。
//! 词法先于归属：一个关键词先必须满足小写/下划线/长度词法，才谈得上属于哪个模块；
//! 归属先于入册：词法合法的词干必须与其声明关键词族的前缀一致，否则入册无意义；
//! 入册先于去重：先把 10 个模块 × 3 个关键词族全部入账，再整体检查文本零撞号；
//! 去重先于复跑：撞号清零后复跑摘要才有意义——带撞号的账复跑一万次也稳定地错。
//! 闸门次序（收官印）：条目 → 槽窗 → 冻结 → 域账 → 递送。
//! 条目先于槽窗：20 条编号必须落在 F49021..F49040，槽位才有可解释的宿主；
//! 槽窗先于冻结：半槽窗口必须从 0x71B40 起逐 0x20 铺满到 0x71DC0，不许留缝；
//! 冻结先于域账：20 条全部冻结后，行数账才是不可再改的定账；
//! 域账先于递送：72000/240,000 是递送批次的入册口径，账不定则递送无法核对。
//! 关键差异：F49039 只回答"关键词族有没有跨模块撞名"，不做语义检索；
//! F49040 只回答"20 条是否冻结、域账是否 72000、递送 100 批是否零串窗"，
//! 不复算逐键判据本体——收官印若重算前九槽，就把"冻结"变成了"再跑一遍"。
//! 两本账的摘要域彼此独立：grep 账折关键词字节，收官账折槽位与行数，互不覆盖。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71D80，B12 族第十槽）。
pub const SEG_BASE: u32 = 0x71D80;

/// B12 批卡数（20）。
pub const CARD_TOTAL: usize = 20;
/// B12 批首卡编号。
pub const CARD_FIRST: u32 = 49_021;
/// B12 批末卡编号。
pub const CARD_LAST: u32 = 49_040;
/// 域账累计已封行数（判据口径 72 000）。
pub const DOMAIN_SEALED: u32 = 72_000;
/// 域账总预算（240 000）。
pub const DOMAIN_BUDGET: u32 = 240_000;
/// 已封行数占比（基点，30.00%）。
pub const SEAL_RATIO_BP: u32 = 3_000;
/// 递送批次数（×10^2）。
pub const DELIVERIES: u32 = 100;
/// 关键词族个数（fullkeymap/led/autorep）。
pub const FAMILIES: usize = 3;
/// 入册模块数（B12 十槽）。
pub const OWNERS: usize = 10;
/// 每族关键词条数（每槽一条，十槽共十条）。
pub const PER_FAMILY: usize = 10;
/// 关键词总条数（30）。
pub const TOKEN_TOTAL: usize = 30;
/// 重复文本注入次数。
pub const DUP_INJECT: u32 = 10;
/// 词法非法注入次数。
pub const SYNTAX_INJECT: u32 = 10;
/// B12 半槽窗口下界。
pub const SLOT_LO: u32 = 0x71B40;
/// B12 半槽窗口上界（开区间）。
pub const SLOT_HI: u32 = 0x71DC0;
/// 半槽宽度（0x20）。
pub const SLOT_SPAN: u32 = 0x20;
/// 递送批编号基址（与诊断段分离，避免把邮戳当诊断码读）。
pub const DELIVERY_BASE: u64 = 0x071E_0000;
/// 递送批次内每条目的窗口宽度。
pub const ITEM_SPAN: u64 = 0x10;
/// 递送批次窗口宽度（20 × 0x10）。
pub const DELIVERY_SPAN: u64 = CARD_TOTAL as u64 * ITEM_SPAN;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// grep 账标签。
pub const GREP_TAG: &str = "GREP";
/// 收官账标签。
pub const SEAL_TAG: &str = "SEAL";
/// 递送账标签。
pub const DELIV_TAG: &str = "DELIV";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B12 批收官诊断码";
/// 关键词族名（下标即族号）。
pub const FAMILY_NAMES: [&str; FAMILIES] = ["fullkeymap", "led", "autorep"];

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 关键词词法非法
    TokenBad,
    /// 关键词族号越界或前缀不符
    FamBad,
    /// 关键词文本跨模块撞名
    DupBad,
    /// 模块归属编号越界
    OwnerBad,
    /// grep 账不守恒
    GrepBad,
    /// 卡编号/行数/引用不合格
    CardBad,
    /// 冻结条目被改写
    FreezeBad,
    /// 收官行数或摘要不合格
    SealBad,
    /// 域账预算不守恒
    BudgetBad,
    /// 槽窗留缝/越界
    WindowBad,
    /// 递送批次不合格
    DeliveryBad,
    /// 渲染字段不可解析
    RenderBad,
    /// 诊断码越出段位窗口
    SegmentBad,
    /// 计数溢出
    Overflow,
}

impl SealCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SealCode::TokenBad => "关键词词法非法",
            SealCode::FamBad => "关键词族号越界或前缀不符",
            SealCode::DupBad => "关键词文本跨模块撞名",
            SealCode::OwnerBad => "模块归属编号越界",
            SealCode::GrepBad => "grep 账不守恒",
            SealCode::CardBad => "卡编号/行数/引用不合格",
            SealCode::FreezeBad => "冻结条目被改写",
            SealCode::SealBad => "收官行数或摘要不合格",
            SealCode::BudgetBad => "域账预算不守恒",
            SealCode::WindowBad => "槽窗留缝/越界",
            SealCode::DeliveryBad => "递送批次不合格",
            SealCode::RenderBad => "渲染字段不可解析",
            SealCode::SegmentBad => "诊断码越出段位窗口",
            SealCode::Overflow => "计数溢出",
        }
    }

    pub const ALL: [SealCode; 14] = [
        SealCode::TokenBad,
        SealCode::FamBad,
        SealCode::DupBad,
        SealCode::OwnerBad,
        SealCode::GrepBad,
        SealCode::CardBad,
        SealCode::FreezeBad,
        SealCode::SealBad,
        SealCode::BudgetBad,
        SealCode::WindowBad,
        SealCode::DeliveryBad,
        SealCode::RenderBad,
        SealCode::SegmentBad,
        SealCode::Overflow,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 是否为本段已知码。
pub fn known_code(code: u32) -> bool {
    detail(code) != UNKNOWN
}

/// 段位守卫：14 条码必须连续落在 0x71D81..0x71D8E。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 关键词（F49039） ─────────────────────────────────────────────
/// 一条关键词声明：哪个模块（owner）、哪个族（family）、什么文本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    pub owner: u8,
    pub family: u8,
    pub text: &'static str,
}

/// 词法检查：小写字母开头，[a-z0-9_] 组成，4..=40 字节，无空段、无双下划线、不以 _ 收尾。
pub fn token_syntax_ok(text: &str) -> bool {
    if text.len() < 4 || text.len() > 40 {
        return false;
    }
    let b0 = match text.as_bytes().first() {
        Some(v) => *v,
        None => return false,
    };
    if !(b'a'..=b'z').contains(&b0) {
        return false;
    }
    let mut i = 0usize;
    let mut prev_us = false;
    while i < text.len() {
        let b = match text.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return false,
        };
        let is_low = (b'a'..=b'z').contains(&b);
        let is_dig = (b'0'..=b'9').contains(&b);
        let is_us = b == b'_';
        if !is_low && !is_dig && !is_us {
            return false;
        }
        if is_us && prev_us {
            return false;
        }
        prev_us = is_us;
        i += 1;
    }
    !prev_us
}

/// 取关键词族前缀（第一个下划线之前）。
pub fn family_prefix(text: &str) -> Option<&str> {
    let mut i = 0usize;
    while i < text.len() {
        if text.as_bytes().get(i).copied() == Some(b'_') {
            if i == 0 {
                return None;
            }
            return text.get(0..i);
        }
        i += 1;
    }
    None
}

/// 族号是否在本槽声明范围内。
pub fn family_ok(family: u8) -> bool {
    (family as usize) < FAMILIES
}

/// 模块归属是否在本批十槽内。
pub fn owner_ok(owner: u8) -> bool {
    (owner as usize) < OWNERS
}

/// 完整声明检查：词法 → 归属 → 族号 → 前缀 → 后缀非空。
pub fn claim_ok(t: &Token) -> bool {
    if !owner_ok(t.owner) || !family_ok(t.family) {
        return false;
    }
    if !token_syntax_ok(t.text) {
        return false;
    }
    let prefix = match family_prefix(t.text) {
        Some(v) => v,
        None => return false,
    };
    let name = match FAMILY_NAMES.get(t.family as usize) {
        Some(v) => *v,
        None => return false,
    };
    if prefix != name {
        return false;
    }
    // 后缀必须存在且非空（prefix + '_' 之外至少一字节）。
    t.text.len() > prefix.len() + 1
}

/// 关键词入册本：文本唯一、声明合法、摘要随入册推进。
#[derive(Clone, Debug)]
pub struct ClaimBook {
    pub tokens: Vec<Token>,
    pub digest: u64,
}

impl ClaimBook {
    pub fn new() -> ClaimBook {
        ClaimBook {
            tokens: Vec::new(),
            digest: p_core::FNV_OFF,
        }
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// 文本是否已被占用。
    pub fn contains(&self, text: &str) -> bool {
        let mut i = 0usize;
        while i < self.tokens.len() {
            if let Some(t) = self.tokens.get(i) {
                if t.text == text {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 入册：先词法/归属，再文本去重；同一文本不论 owner 是否变化都拒绝。
    pub fn insert(&mut self, t: Token) -> Result<(), u32> {
        if !owner_ok(t.owner) {
            return Err(SealCode::OwnerBad.val());
        }
        if !family_ok(t.family) {
            return Err(SealCode::FamBad.val());
        }
        if !token_syntax_ok(t.text) {
            return Err(SealCode::TokenBad.val());
        }
        let prefix = match family_prefix(t.text) {
            Some(v) => v,
            None => return Err(SealCode::TokenBad.val()),
        };
        let name = match FAMILY_NAMES.get(t.family as usize) {
            Some(v) => *v,
            None => return Err(SealCode::FamBad.val()),
        };
        if prefix != name {
            return Err(SealCode::FamBad.val());
        }
        if self.contains(t.text) {
            return Err(SealCode::DupBad.val());
        }
        self.digest = p_core::fold_byte(self.digest, t.owner);
        self.digest = p_core::fold_byte(self.digest, t.family);
        let mut i = 0usize;
        while i < t.text.len() {
            if let Some(b) = t.text.as_bytes().get(i).copied() {
                self.digest = p_core::fold_byte(self.digest, b);
            }
            i += 1;
        }
        self.tokens.push(t);
        Ok(())
    }
}

/// B12 十槽 × 三关键词族的静态声明表（30 条，逐条手写，不生成）。
pub const CATALOG: [Token; TOKEN_TOTAL] = [
    Token { owner: 0, family: 0, text: "fullkeymap_scan_21" },
    Token { owner: 1, family: 0, text: "fullkeymap_dead_23" },
    Token { owner: 2, family: 0, text: "fullkeymap_led_25" },
    Token { owner: 3, family: 0, text: "fullkeymap_mod_27" },
    Token { owner: 4, family: 0, text: "fullkeymap_lat_29" },
    Token { owner: 5, family: 0, text: "fullkeymap_pool_31" },
    Token { owner: 6, family: 0, text: "fullkeymap_cross_33" },
    Token { owner: 7, family: 0, text: "fullkeymap_probe_35" },
    Token { owner: 8, family: 0, text: "fullkeymap_fuzz_37" },
    Token { owner: 9, family: 0, text: "fullkeymap_seal_39" },
    Token { owner: 0, family: 1, text: "led_census_21" },
    Token { owner: 1, family: 1, text: "led_fail_23" },
    Token { owner: 2, family: 1, text: "led_state_25" },
    Token { owner: 3, family: 1, text: "led_trans_27" },
    Token { owner: 4, family: 1, text: "led_lat_29" },
    Token { owner: 5, family: 1, text: "led_pool_31" },
    Token { owner: 6, family: 1, text: "led_cross_33" },
    Token { owner: 7, family: 1, text: "led_point_35" },
    Token { owner: 8, family: 1, text: "led_fuzz_37" },
    Token { owner: 9, family: 1, text: "led_seal_39" },
    Token { owner: 0, family: 2, text: "autorep_period_21" },
    Token { owner: 1, family: 2, text: "autorep_delay_23" },
    Token { owner: 2, family: 2, text: "autorep_hold_25" },
    Token { owner: 3, family: 2, text: "autorep_key_27" },
    Token { owner: 4, family: 2, text: "autorep_lat_29" },
    Token { owner: 5, family: 2, text: "autorep_pool_31" },
    Token { owner: 6, family: 2, text: "autorep_cross_33" },
    Token { owner: 7, family: 2, text: "autorep_probe_35" },
    Token { owner: 8, family: 2, text: "autorep_fuzz_37" },
    Token { owner: 9, family: 2, text: "autorep_seal_39" },
];

/// grep 账：条数、每族条数、每模块条数、撞号数、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepLedger {
    pub tokens: u32,
    pub families: [u32; FAMILIES],
    pub owners: [u32; OWNERS],
    pub duplicates: u32,
    pub digest: u64,
}

impl GrepLedger {
    /// 守恒：总数 = 每族之和 = 每模块之和，且撞号为零。
    pub fn conserved(&self) -> bool {
        let mut fsum = 0u32;
        let mut i = 0usize;
        while i < FAMILIES {
            fsum = match fsum.checked_add(self.families[i]) {
                Some(v) => v,
                None => return false,
            };
            i += 1;
        }
        let mut osum = 0u32;
        i = 0;
        while i < OWNERS {
            osum = match osum.checked_add(self.owners[i]) {
                Some(v) => v,
                None => return false,
            };
            i += 1;
        }
        fsum == self.tokens && osum == self.tokens && self.duplicates == 0
    }
}

/// 将整张声明表入册并定账；词法/归属/撞号分别落到对应码。
pub fn audit_catalog() -> Result<GrepLedger, u32> {
    let mut book = ClaimBook::new();
    let mut families = [0u32; FAMILIES];
    let mut owners = [0u32; OWNERS];
    let mut i = 0usize;
    while i < CATALOG.len() {
        let t = match CATALOG.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::Overflow.val()),
        };
        if !owner_ok(t.owner) {
            return Err(SealCode::OwnerBad.val());
        }
        if !family_ok(t.family) {
            return Err(SealCode::FamBad.val());
        }
        if !token_syntax_ok(t.text) {
            return Err(SealCode::TokenBad.val());
        }
        if !claim_ok(&t) {
            return Err(SealCode::FamBad.val());
        }
        if book.insert(t).is_err() {
            return Err(SealCode::DupBad.val());
        }
        if let Some(slot) = families.get_mut(t.family as usize) {
            *slot = match slot.checked_add(1) {
                Some(v) => v,
                None => return Err(SealCode::Overflow.val()),
            };
        }
        if let Some(slot) = owners.get_mut(t.owner as usize) {
            *slot = match slot.checked_add(1) {
                Some(v) => v,
                None => return Err(SealCode::Overflow.val()),
            };
        }
        i += 1;
    }
    if book.len() != TOKEN_TOTAL {
        return Err(SealCode::GrepBad.val());
    }
    Ok(GrepLedger {
        tokens: book.len() as u32,
        families,
        owners,
        duplicates: 0,
        digest: book.digest,
    })
}

/// 独立复核：按族直接重数（不复用 audit_catalog 的累加器）。
pub fn family_recount(family: u8) -> Option<u32> {
    if !family_ok(family) {
        return None;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CATALOG.len() {
        if let Some(t) = CATALOG.get(i) {
            if t.family == family {
                n = n.checked_add(1)?;
            }
        }
        i += 1;
    }
    Some(n)
}

/// 独立复核：某族覆盖的模块位图（十槽全覆盖应为 0x3FF）。
pub fn owner_mask(family: u8) -> Option<u16> {
    if !family_ok(family) {
        return None;
    }
    let mut mask: u16 = 0;
    let mut i = 0usize;
    while i < CATALOG.len() {
        if let Some(t) = CATALOG.get(i) {
            if t.family == family {
                if !owner_ok(t.owner) {
                    return None;
                }
                mask |= 1u16 << t.owner;
            }
        }
        i += 1;
    }
    Some(mask)
}

/// 直接扫表统计某族条数（grep 的独立旁证）。
pub fn grep_family(family: u8) -> u32 {
    match family_recount(family) {
        Some(v) => v,
        None => 0,
    }
}

/// 重复文本注入：① 前 10 条换 owner 再入册；② 10 条词法非法文本。
/// 两组都必须全拒，返回 (文本撞号拒绝数, 词法拒绝数)。
pub fn dup_inject() -> Result<(u32, u32), u32> {
    let mut book = ClaimBook::new();
    let mut i = 0usize;
    while i < CATALOG.len() {
        if let Some(t) = CATALOG.get(i) {
            book.insert(*t)?;
        }
        i += 1;
    }
    let mut text_rejected = 0u32;
    i = 0;
    while i < DUP_INJECT as usize {
        let src = match CATALOG.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::Overflow.val()),
        };
        let clone = Token {
            owner: (src.owner + 1) % OWNERS as u8,
            family: src.family,
            text: src.text,
        };
        if book.insert(clone).is_err() {
            text_rejected = text_rejected.saturating_add(1);
        }
        i += 1;
    }
    let bad_texts: [&str; SYNTAX_INJECT as usize] = [
        "Fullkeymap_scan",
        "led__state",
        "led_tail_",
        "led",
        "_led_state",
        "led two",
        "9led_state",
        "mouse_pad",
        "led_state!",
        "",
    ];
    let mut syntax_rejected = 0u32;
    i = 0;
    while i < bad_texts.len() {
        let text = match bad_texts.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::Overflow.val()),
        };
        let t = Token {
            owner: 0,
            family: 1,
            text,
        };
        if book.insert(t).is_err() {
            syntax_rejected = syntax_rejected.saturating_add(1);
        }
        i += 1;
    }
    Ok((text_rejected, syntax_rejected))
}

/// grep 复跑：两遍定账摘要必须一致且非零。
pub fn grep_replay() -> Result<(GrepLedger, GrepLedger), u32> {
    let a = audit_catalog()?;
    let b = audit_catalog()?;
    Ok((a, b))
}

// ── 收官印（F49040） ─────────────────────────────────────────────
/// 一张卡的静态声明：编号、行数配额、对下一卡的引用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardSpec {
    pub card: u32,
    pub lines: u32,
    pub cite: &'static str,
}

/// 20 张卡的静态表；行数按 5 组 [4200,3600,3400,3200] 累计 72 000。
pub const CARDS: [CardSpec; CARD_TOTAL] = [
    CardSpec { card: 49_021, lines: 4_200, cite: "UNX-F49022-J1" },
    CardSpec { card: 49_022, lines: 3_600, cite: "UNX-F49023-J1" },
    CardSpec { card: 49_023, lines: 3_400, cite: "UNX-F49024-J1" },
    CardSpec { card: 49_024, lines: 3_200, cite: "UNX-F49025-J1" },
    CardSpec { card: 49_025, lines: 4_200, cite: "UNX-F49026-J1" },
    CardSpec { card: 49_026, lines: 3_600, cite: "UNX-F49027-J1" },
    CardSpec { card: 49_027, lines: 3_400, cite: "UNX-F49028-J1" },
    CardSpec { card: 49_028, lines: 3_200, cite: "UNX-F49029-J1" },
    CardSpec { card: 49_029, lines: 4_200, cite: "UNX-F49030-J1" },
    CardSpec { card: 49_030, lines: 3_600, cite: "UNX-F49031-J1" },
    CardSpec { card: 49_031, lines: 3_400, cite: "UNX-F49032-J1" },
    CardSpec { card: 49_032, lines: 3_200, cite: "UNX-F49033-J1" },
    CardSpec { card: 49_033, lines: 4_200, cite: "UNX-F49034-J1" },
    CardSpec { card: 49_034, lines: 3_600, cite: "UNX-F49035-J1" },
    CardSpec { card: 49_035, lines: 3_400, cite: "UNX-F49036-J1" },
    CardSpec { card: 49_036, lines: 3_200, cite: "UNX-F49037-J1" },
    CardSpec { card: 49_037, lines: 4_200, cite: "UNX-F49038-J1" },
    CardSpec { card: 49_038, lines: 3_600, cite: "UNX-F49039-J1" },
    CardSpec { card: 49_039, lines: 3_400, cite: "UNX-F49040-J1" },
    CardSpec { card: 49_040, lines: 3_200, cite: "UNX-F49041-J1" },
];

/// 空白卡印（数组初始化用）。
pub const EMPTY_CARD: CardStamp = CardStamp {
    card: 0,
    slot: 0,
    lines: 0,
    cite: "",
    digest: 0,
    frozen: false,
};

/// 冻结后的卡印：槽位、行数、引用、摘要、冻结位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardStamp {
    pub card: u32,
    pub slot: u32,
    pub lines: u32,
    pub cite: &'static str,
    pub digest: u64,
    pub frozen: bool,
}

/// 半槽槽位：偶数卡起点 +0x00，奇数卡 +0x20，全部落在 0x71B40..0x71DC0。
pub fn card_slot(i: usize) -> Option<u32> {
    if i >= CARD_TOTAL {
        return None;
    }
    let pair = (i / 2) as u32;
    let half = if i % 2 == 0 { 0u32 } else { SLOT_SPAN };
    let base = SLOT_LO.checked_add(pair.checked_mul(2 * SLOT_SPAN)?)?;
    let slot = base.checked_add(half)?;
    if slot < SLOT_LO || slot + SLOT_SPAN > SLOT_HI {
        return None;
    }
    Some(slot)
}

/// 20 个半槽必须从 0x71B40 起无缝铺到 0x71DC0。
pub fn slot_scan() -> bool {
    let mut prev_end = SLOT_LO;
    let mut i = 0usize;
    while i < CARD_TOTAL {
        let slot = match card_slot(i) {
            Some(v) => v,
            None => return false,
        };
        if slot != prev_end {
            return false;
        }
        prev_end = match slot.checked_add(SLOT_SPAN) {
            Some(v) => v,
            None => return false,
        };
        i += 1;
    }
    prev_end == SLOT_HI
}

/// 卡的期望引用：本卡编号 + 1（B12 任务板的 J1 引用逐卡后移一位）。
pub fn expected_cite(card: u32) -> Option<String> {
    let next = card.checked_add(1)?;
    Some(format!("UNX-F{}-J1", next))
}

/// 引用必须与期望一致，且落在 F49022..F49041 的已知窗口。
pub fn cite_ok(i: usize) -> bool {
    let spec = match CARDS.get(i) {
        Some(v) => v,
        None => return false,
    };
    let want = match expected_cite(spec.card) {
        Some(v) => v,
        None => return false,
    };
    if spec.cite != want {
        return false;
    }
    spec.card >= CARD_FIRST && spec.card <= CARD_LAST
}

/// 卡印摘要：折卡号、槽位、行数、引用字节与冻结位。
pub fn card_digest(spec: &CardSpec, slot: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, spec.card);
    h = p_core::fold_u32(h, slot);
    h = p_core::fold_u32(h, spec.lines);
    let mut i = 0usize;
    while i < spec.cite.len() {
        if let Some(b) = spec.cite.as_bytes().get(i).copied() {
            h = p_core::fold_byte(h, b);
        }
        i += 1;
    }
    p_core::fold_u32(h, 1)
}

/// 收官印：20 张冻结卡 + 已封行数 + 总摘要。
#[derive(Clone, Copy, Debug)]
pub struct Seal {
    pub cards: [CardStamp; CARD_TOTAL],
    pub sealed_lines: u32,
    pub digest: u64,
}

impl Seal {
    /// 冻结条目只读：同值复写视为幂等，异值一律拒绝。
    pub fn reseal(&mut self, i: usize, next: CardStamp) -> Result<(), u32> {
        let cur = match self.cards.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::SealBad.val()),
        };
        if !cur.frozen {
            return Err(SealCode::FreezeBad.val());
        }
        // 冻结位只许保持为真：撤销冻结等同于改写。
        if !next.frozen {
            return Err(SealCode::FreezeBad.val());
        }
        if cur.card != next.card
            || cur.slot != next.slot
            || cur.lines != next.lines
            || cur.cite != next.cite
            || cur.digest != next.digest
        {
            return Err(SealCode::FreezeBad.val());
        }
        Ok(())
    }

    /// 冻结卡数。
    pub fn frozen_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CARD_TOTAL {
            if let Some(c) = self.cards.get(i) {
                if c.frozen {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 冻结卡编号唯一性（20 个编号两两不同、且落在声明窗口）。
    pub fn unique_cards(&self) -> bool {
        let mut i = 0usize;
        while i < CARD_TOTAL {
            let a = match self.cards.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a.card < CARD_FIRST || a.card > CARD_LAST {
                return false;
            }
            let mut j = i + 1;
            while j < CARD_TOTAL {
                if let Some(b) = self.cards.get(j) {
                    if a.card == b.card || a.slot == b.slot {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 生成收官印：条目 → 槽窗 → 引用 → 行数 → 冻结。
pub fn seal() -> Result<Seal, u32> {
    let mut cards = [EMPTY_CARD; CARD_TOTAL];
    let mut total = 0u32;
    let mut i = 0usize;
    while i < CARD_TOTAL {
        let spec = match CARDS.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::Overflow.val()),
        };
        let want_card = CARD_FIRST.checked_add(i as u32).ok_or(SealCode::Overflow.val())?;
        if spec.card != want_card {
            return Err(SealCode::CardBad.val());
        }
        if spec.lines == 0 {
            return Err(SealCode::CardBad.val());
        }
        if !cite_ok(i) {
            return Err(SealCode::CardBad.val());
        }
        let slot = match card_slot(i) {
            Some(v) => v,
            None => return Err(SealCode::WindowBad.val()),
        };
        total = total.checked_add(spec.lines).ok_or(SealCode::Overflow.val())?;
        let digest = card_digest(&spec, slot);
        if digest == 0 {
            return Err(SealCode::SealBad.val());
        }
        cards[i] = CardStamp {
            card: spec.card,
            slot,
            lines: spec.lines,
            cite: spec.cite,
            digest,
            frozen: true,
        };
        i += 1;
    }
    if total != DOMAIN_SEALED {
        return Err(SealCode::SealBad.val());
    }
    if !slot_scan() {
        return Err(SealCode::WindowBad.val());
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, total);
    i = 0;
    while i < CARD_TOTAL {
        if let Some(c) = cards.get(i) {
            h = p_core::fold_u32(h, c.card);
            h = p_core::fold_u32(h, c.slot);
            h = p_core::fold_u32(h, c.lines);
            h = p_core::fold_u32(h, (c.digest >> 32) as u32);
            h = p_core::fold_u32(h, c.digest as u32);
        }
        i += 1;
    }
    if h == 0 {
        return Err(SealCode::SealBad.val());
    }
    Ok(Seal {
        cards,
        sealed_lines: total,
        digest: h,
    })
}

/// 冻结复写注入：10 种异值改写全部拒绝，1 次同值复写幂等接受。
pub fn reseal_inject() -> Result<(u32, u32), u32> {
    let mut s = seal()?;
    let mut rejected = 0u32;
    let mut i = 0usize;
    while i < 10 {
        let cur = match s.cards.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::Overflow.val()),
        };
        let mutated = match i {
            0 => CardStamp { lines: cur.lines + 1, ..cur },
            1 => CardStamp { digest: cur.digest ^ 0x1, ..cur },
            2 => CardStamp { card: cur.card + 1, ..cur },
            3 => CardStamp { slot: cur.slot + SLOT_SPAN, ..cur },
            4 => CardStamp { cite: "UNX-F49041-J1", ..cur },
            5 => CardStamp { frozen: false, ..cur },
            6 => CardStamp { lines: cur.lines - 1, ..cur },
            7 => CardStamp { digest: 0, ..cur },
            8 => CardStamp { card: 0, ..cur },
            _ => CardStamp { slot: 0, ..cur },
        };
        if s.reseal(i, mutated).is_err() {
            rejected = rejected.saturating_add(1);
        }
        i += 1;
    }
    let cur = match s.cards.get(0) {
        Some(v) => *v,
        None => return Err(SealCode::Overflow.val()),
    };
    let mut accepted = 0u32;
    if s.reseal(0, cur).is_ok() {
        accepted = 1;
    }
    Ok((rejected, accepted))
}

/// 域账：已封 72000、预算 240000、余量、占比基点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Budget {
    pub sealed: u32,
    pub budget: u32,
    pub remaining: u32,
    pub ratio_bp: u32,
}

impl Budget {
    /// 守恒：已封 + 余量 = 预算；占比按四舍五入基点核。
    pub fn conserved(&self) -> bool {
        match self.sealed.checked_add(self.remaining) {
            Some(v) => v == self.budget,
            None => false,
        }
    }
}

/// 定域账：行数必须等于 72 000，预算 240 000，余量 168 000，占比 3000 基点。
pub fn budget_ledger() -> Result<Budget, u32> {
    let s = seal()?;
    if s.sealed_lines != DOMAIN_SEALED {
        return Err(SealCode::SealBad.val());
    }
    if s.sealed_lines > DOMAIN_BUDGET {
        return Err(SealCode::BudgetBad.val());
    }
    let remaining = DOMAIN_BUDGET - s.sealed_lines;
    let scaled = (s.sealed_lines as u64)
        .checked_mul(10_000)
        .ok_or(SealCode::Overflow.val())?;
    let ratio_bp = scaled / DOMAIN_BUDGET as u64;
    if ratio_bp != SEAL_RATIO_BP as u64 {
        return Err(SealCode::BudgetBad.val());
    }
    let b = Budget {
        sealed: s.sealed_lines,
        budget: DOMAIN_BUDGET,
        remaining,
        ratio_bp: ratio_bp as u32,
    };
    if !b.conserved() {
        return Err(SealCode::BudgetBad.val());
    }
    Ok(b)
}

/// 收官复跑：两次印章摘要必须一致。
pub fn seal_replay() -> Result<(Seal, Seal), u32> {
    let a = seal()?;
    let b = seal()?;
    Ok((a, b))
}

// ── 递送 ×10^2 零串窗 ────────────────────────────────────────────
/// 一批递送：批次号、窗口起点/宽度、条目数、首末邮戳、摘要、关闸位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivery {
    pub index: u32,
    pub base: u64,
    pub span: u64,
    pub items: u32,
    pub first: u64,
    pub last: u64,
    pub digest: u64,
    pub closed: bool,
}

/// 批次窗口起点（checked）。
pub fn delivery_base(index: u32) -> Option<u64> {
    if index >= DELIVERIES {
        return None;
    }
    DELIVERY_BASE.checked_add((index as u64).checked_mul(DELIVERY_SPAN)?)
}

/// 生成一批递送：20 条邮戳各占 0x10 窗口，末条引用下一批首条（只引用不写入）。
pub fn deliver(index: u32) -> Result<Delivery, u32> {
    if index >= DELIVERIES {
        return Err(SealCode::DeliveryBad.val());
    }
    let base = delivery_base(index).ok_or(SealCode::Overflow.val())?;
    let items = CARD_TOTAL as u32;
    let span = DELIVERY_SPAN;
    let first = base;
    let last = base
        .checked_add((items as u64 - 1).checked_mul(ITEM_SPAN).ok_or(SealCode::Overflow.val())?)
        .ok_or(SealCode::Overflow.val())?;
    if last.checked_add(ITEM_SPAN) != base.checked_add(span) {
        return Err(SealCode::DeliveryBad.val());
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, index);
    h = p_core::fold_u32(h, (base >> 32) as u32);
    h = p_core::fold_u32(h, base as u32);
    h = p_core::fold_u32(h, (last >> 32) as u32);
    h = p_core::fold_u32(h, last as u32);
    h = p_core::fold_u32(h, items);
    if h == 0 {
        return Err(SealCode::DeliveryBad.val());
    }
    Ok(Delivery {
        index,
        base,
        span,
        items,
        first,
        last,
        digest: h,
        closed: true,
    })
}

/// 单批审计：号 → 宽度 → 条目数 → 首末 → 关闸 → 与前批无缝不重叠。
pub fn audit_delivery(d: &Delivery, prev: Option<&Delivery>) -> Result<(), u32> {
    if d.index >= DELIVERIES {
        return Err(SealCode::DeliveryBad.val());
    }
    if d.span != DELIVERY_SPAN {
        return Err(SealCode::WindowBad.val());
    }
    if d.items != CARD_TOTAL as u32 {
        return Err(SealCode::DeliveryBad.val());
    }
    let want_base = delivery_base(d.index).ok_or(SealCode::Overflow.val())?;
    if d.base != want_base {
        return Err(SealCode::WindowBad.val());
    }
    if d.first != d.base {
        return Err(SealCode::WindowBad.val());
    }
    let want_last = d
        .base
        .checked_add((d.items as u64 - 1).checked_mul(ITEM_SPAN).ok_or(SealCode::Overflow.val())?)
        .ok_or(SealCode::Overflow.val())?;
    if d.last != want_last {
        return Err(SealCode::WindowBad.val());
    }
    if d.last.checked_add(ITEM_SPAN) != d.base.checked_add(d.span) {
        return Err(SealCode::WindowBad.val());
    }
    if d.digest == 0 {
        return Err(SealCode::DeliveryBad.val());
    }
    if !d.closed {
        return Err(SealCode::DeliveryBad.val());
    }
    if let Some(p) = prev {
        let prev_end = p.base.checked_add(p.span).ok_or(SealCode::Overflow.val())?;
        if d.base != prev_end {
            return Err(SealCode::WindowBad.val());
        }
        if p.last >= d.first {
            return Err(SealCode::WindowBad.val());
        }
    }
    Ok(())
}

/// 递送总量：100 批 × 20 条，逐批审计，返回账本与总摘要。
pub fn deliver_all() -> Result<(Vec<Delivery>, u64), u32> {
    let mut out: Vec<Delivery> = Vec::new();
    let mut h = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < DELIVERIES {
        let d = deliver(i)?;
        let prev = if i == 0 { None } else { out.get((i - 1) as usize) };
        audit_delivery(&d, prev)?;
        h = p_core::fold_u32(h, d.index);
        h = p_core::fold_u32(h, (d.base >> 32) as u32);
        h = p_core::fold_u32(h, d.base as u32);
        h = p_core::fold_u32(h, (d.digest >> 32) as u32);
        h = p_core::fold_u32(h, d.digest as u32);
        out.push(d);
        i += 1;
    }
    if out.len() != DELIVERIES as usize {
        return Err(SealCode::GrepBad.val());
    }
    if h == 0 {
        return Err(SealCode::SealBad.val());
    }
    Ok((out, h))
}

/// 全批扫描：返回（批数、邮戳数、串窗数、末窗口终点）。
pub fn window_scan() -> Result<(u32, u32, u32, u64), u32> {
    let (rows, _) = deliver_all()?;
    let mut deliveries = 0u32;
    let mut stamps = 0u32;
    let mut crossings = 0u32;
    let mut i = 0usize;
    while i < rows.len() {
        let d = match rows.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::Overflow.val()),
        };
        deliveries = deliveries.saturating_add(1);
        stamps = stamps.saturating_add(d.items);
        if i > 0 {
            if let Some(p) = rows.get(i - 1) {
                if d.base < p.base.saturating_add(p.span) {
                    crossings = crossings.saturating_add(1);
                }
                if p.last >= d.first {
                    crossings = crossings.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    let last = match rows.last() {
        Some(v) => v.base.saturating_add(v.span),
        None => return Err(SealCode::DeliveryBad.val()),
    };
    Ok((deliveries, stamps, crossings, last))
}

/// 十种病理递送：每一种都必须被 audit_delivery 拒绝。
pub fn delivery_faults() -> Result<(u32, u32), u32> {
    let good = deliver(1)?;
    let prev = deliver(0)?;
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < 10 {
        let bad = match i {
            0 => Delivery { span: good.span + 1, ..good },
            1 => Delivery { items: good.items - 1, ..good },
            2 => Delivery { base: good.base + 1, ..good },
            3 => Delivery { first: good.first + 1, ..good },
            4 => Delivery { last: good.last + ITEM_SPAN, ..good },
            5 => Delivery { digest: 0, ..good },
            6 => Delivery { closed: false, ..good },
            7 => Delivery { index: DELIVERIES, ..good },
            8 => {
                let shifted = match delivery_base(0) {
                    Some(v) => v,
                    None => return Err(SealCode::Overflow.val()),
                };
                Delivery {
                    index: 1,
                    base: shifted,
                    first: shifted,
                    last: shifted + (CARD_TOTAL as u64 - 1) * ITEM_SPAN,
                    ..good
                }
            }
            _ => Delivery { base: good.base - 1, ..good },
        };
        let p = if i == 7 { None } else { Some(&prev) };
        if audit_delivery(&bad, p).is_err() {
            rejected = rejected.saturating_add(1);
        } else {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

/// 递送复跑：两次全批摘要必须一致。
pub fn delivery_replay() -> Result<(u64, u64), u32> {
    let (_, a) = deliver_all()?;
    let (_, b) = deliver_all()?;
    Ok((a, b))
}

// ── 渲染 ─────────────────────────────────────────────────────────
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|a={}|b={}|c={}|d={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.get(0).and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

/// grep 账渲染：条数、族数、模块数、撞号数。
pub fn grep_row(l: &GrepLedger) -> String {
    render_row(
        GREP_TAG,
        l.tokens,
        FAMILIES as u32,
        OWNERS as u32,
        l.duplicates,
        l.digest,
    )
}

/// 收官印渲染：卡数、冻结数、已封行数、预算。
pub fn seal_row(s: &Seal) -> String {
    render_row(
        SEAL_TAG,
        CARD_TOTAL as u32,
        s.frozen_count(),
        s.sealed_lines,
        DOMAIN_BUDGET,
        s.digest,
    )
}

/// 递送账渲染：批数、邮戳数、串窗数、窗口终点高位。
pub fn deliv_row(deliveries: u32, stamps: u32, crossings: u32, end: u64) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, deliveries);
    h = p_core::fold_u32(h, stamps);
    h = p_core::fold_u32(h, crossings);
    h = p_core::fold_u32(h, (end >> 32) as u32);
    h = p_core::fold_u32(h, end as u32);
    render_row(
        DELIV_TAG,
        deliveries,
        stamps,
        crossings,
        (end >> 32) as u32,
        h,
    )
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49039 ①：关键词族扫描（30 条、词法合法、每族 10 条、摘要非零）。
    let grep = audit_catalog();
    let scan_ok = match &grep {
        Ok(l) => {
            l.tokens == TOKEN_TOTAL as u32
                && l.families.iter().all(|v| *v == PER_FAMILY as u32)
                && l.owners.iter().all(|v| *v == FAMILIES as u32)
                && l.digest != 0
                && l.conserved()
        }
        Err(_) => false,
    };
    out.push((
        "F49039/关键词族扫描",
        scan_ok,
        format!(
            "tokens={:?} families={:?} digest={:016X}",
            grep.as_ref().map(|l| l.tokens),
            grep.as_ref().map(|l| l.families),
            grep.as_ref().map(|l| l.digest).unwrap_or(0)
        ),
    ));

    // F49039 ②：防重零撞（重复文本 10 拒、词法非法 10 拒、账内撞号为零）。
    let dup = dup_inject();
    let dup_ok = match &dup {
        Ok((t, s)) => *t == DUP_INJECT && *s == SYNTAX_INJECT,
        Err(_) => false,
    };
    let zero_dup = match &grep {
        Ok(l) => l.duplicates == 0,
        Err(_) => false,
    };
    out.push((
        "F49039/防重零撞",
        dup_ok && zero_dup,
        format!(
            "text_rejected={:?} syntax_rejected={:?} ledger_dup={:?}",
            dup.as_ref().map(|(t, _)| *t),
            dup.as_ref().map(|(_, s)| *s),
            grep.as_ref().map(|l| l.duplicates)
        ),
    ));

    // F49039 ③：grep 复跑（两遍摘要一致且非零）。
    let grep_rep = grep_replay();
    let rep_ok = match &grep_rep {
        Ok((a, b)) => a.digest == b.digest && a.digest != 0 && a.conserved() && b.conserved(),
        Err(_) => false,
    };
    out.push((
        "F49039/grep复跑",
        rep_ok,
        format!(
            "digest={:016X} stable={}",
            match &grep_rep {
                Ok((a, _)) => a.digest,
                Err(_) => 0,
            },
            rep_ok
        ),
    ));

    // F49039 ④：家族覆盖（每族独立重数一致、十槽位图全满 0x3FF）。
    let mut cover_ok = true;
    let mut counts = [0u32; FAMILIES];
    let mut masks = [0u16; FAMILIES];
    let mut f = 0usize;
    while f < FAMILIES {
        let c = match family_recount(f as u8) {
            Some(v) => v,
            None => {
                cover_ok = false;
                0
            }
        };
        let m = match owner_mask(f as u8) {
            Some(v) => v,
            None => {
                cover_ok = false;
                0
            }
        };
        counts[f] = c;
        masks[f] = m;
        if m != 0x3FF || c != PER_FAMILY as u32 || grep_family(f as u8) != c {
            cover_ok = false;
        }
        f += 1;
    }
    out.push((
        "F49039/家族覆盖",
        cover_ok,
        format!("counts={:?} masks={:?}", counts, masks),
    ));

    // F49040 ①：20 条冻结（条目窗口、槽窗无缝合、编号唯一、冻结位全开）。
    let sealed = seal();
    let freeze_ok = match &sealed {
        Ok(s) => {
            s.frozen_count() == CARD_TOTAL as u32
                && s.unique_cards()
                && slot_scan()
                && s.sealed_lines == DOMAIN_SEALED
        }
        Err(_) => false,
    };
    let reseal_rep = reseal_inject();
    let reseal_ok = match &reseal_rep {
        Ok((r, a)) => *r == 10 && *a == 1,
        Err(_) => false,
    };
    out.push((
        "F49040/20条冻结",
        freeze_ok && reseal_ok,
        format!(
            "frozen={:?} reseal_rejected={:?} reseal_accepted={:?} slots={}",
            sealed.as_ref().map(|s| s.frozen_count()),
            reseal_rep.as_ref().map(|(r, _)| *r),
            reseal_rep.as_ref().map(|(_, a)| *a),
            slot_scan()
        ),
    ));

    // F49040 ②：域账 72000/240000（余量 168000、占比 3000 基点、守恒）。
    let budget = budget_ledger();
    let budget_ok = match &budget {
        Ok(b) => {
            b.sealed == DOMAIN_SEALED
                && b.budget == DOMAIN_BUDGET
                && b.remaining == DOMAIN_BUDGET - DOMAIN_SEALED
                && b.ratio_bp == SEAL_RATIO_BP
                && b.conserved()
        }
        Err(_) => false,
    };
    out.push((
        "F49040/域账72000",
        budget_ok,
        format!(
            "sealed={:?} budget={:?} remaining={:?} ratio_bp={:?}",
            budget.as_ref().map(|b| b.sealed),
            budget.as_ref().map(|b| b.budget),
            budget.as_ref().map(|b| b.remaining),
            budget.as_ref().map(|b| b.ratio_bp)
        ),
    ));

    // F49040 ③：递送 ×10^2 零串窗（100 批、2000 邮戳、串窗 0、窗口无缝到终点）。
    let win = window_scan();
    let faults = delivery_faults();
    let win_ok = match &win {
        Ok((d, s, c, end)) => {
            *d == DELIVERIES
                && *s == DELIVERIES * CARD_TOTAL as u32
                && *c == 0
                && *end == DELIVERY_BASE + DELIVERIES as u64 * DELIVERY_SPAN
        }
        Err(_) => false,
    };
    let fault_ok = match &faults {
        Ok((r, a)) => *r == 10 && *a == 0,
        Err(_) => false,
    };
    out.push((
        "F49040/递送百批零串窗",
        win_ok && fault_ok,
        format!(
            "deliveries={:?} stamps={:?} crossings={:?} faults={:?}",
            win.as_ref().map(|(d, _, _, _)| *d),
            win.as_ref().map(|(_, s, _, _)| *s),
            win.as_ref().map(|(_, _, c, _)| *c),
            faults.as_ref().map(|(r, a)| (*r, *a))
        ),
    ));

    // F49040 ④：收官复跑与渲染（印/递送复跑零漂移、六字段回读、段位守卫）。
    let seal_rep = seal_replay();
    let deliv_rep = delivery_replay();
    let replay_ok = match (&seal_rep, &deliv_rep) {
        (Ok((a, b)), Ok((x, y))) => a.digest == b.digest && a.digest != 0 && x == y && *x != 0,
        _ => false,
    };
    let sample = render_row(SEAL_TAG, CARD_TOTAL as u32, CARD_TOTAL as u32, DOMAIN_SEALED, DOMAIN_BUDGET, 0x0123_4567_89AB_CDEF);
    let sample_ok = match row_fields(&sample) {
        Some((t, v, d)) => {
            t == SEAL_TAG
                && v[0] == CARD_TOTAL as u32
                && v[1] == CARD_TOTAL as u32
                && v[2] == DOMAIN_SEALED
                && v[3] == DOMAIN_BUDGET
                && d == 0x0123_4567_89AB_CDEF
        }
        None => false,
    };
    let grep_row_ok = match (&grep, &sealed) {
        (Ok(g), Ok(_)) => match row_fields(&grep_row(g)) {
            Some((t, v, d)) => {
                t == GREP_TAG
                    && v[0] == TOKEN_TOTAL as u32
                    && v[1] == FAMILIES as u32
                    && v[2] == OWNERS as u32
                    && v[3] == 0
                    && d == g.digest
            }
            None => false,
        },
        _ => false,
    };
    let seal_row_ok = match (&sealed, &budget) {
        (Ok(s), Ok(b)) => match row_fields(&seal_row(s)) {
            Some((t, v, d)) => {
                t == SEAL_TAG
                    && v[0] == CARD_TOTAL as u32
                    && v[1] == CARD_TOTAL as u32
                    && v[2] == b.sealed
                    && v[3] == b.budget
                    && d == s.digest
            }
            None => false,
        },
        _ => false,
    };
    let deliv_row_ok = match &win {
        Ok((d, s, c, end)) => match row_fields(&deliv_row(*d, *s, *c, *end)) {
            Some((t, v, h)) => {
                t == DELIV_TAG
                    && v[0] == DELIVERIES
                    && v[1] == DELIVERIES * CARD_TOTAL as u32
                    && v[2] == 0
                    && h != 0
            }
            None => false,
        },
        Err(_) => false,
    };
    let render_ok = sample_ok
        && audit_render(&sample)
        && grep_row_ok
        && seal_row_ok
        && deliv_row_ok
        && segment_guard();
    out.push((
        "F49040/收官复跑与渲染",
        replay_ok && render_ok,
        format!(
            "replay={} render={} seg={}",
            replay_ok,
            render_ok,
            segment_guard()
        ),
    ));

    out
}

pub fn run_unx49039_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49040 B12 批防重 grep 与收官印");
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
    fn error_codes_are_closed_and_segmented() {
        assert_eq!(SealCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < SealCode::ALL.len() {
            let c = SealCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(c.val()));
            i += 1;
        }
        assert!(!known_code(SEG_BASE));
        assert!(!known_code(SEG_BASE + 0x0F));
    }

    #[test]
    fn detail_lookup_roundtrip() {
        let mut i = 0usize;
        while i < SealCode::ALL.len() {
            let c = SealCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            assert_ne!(detail(c.val()), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(0), UNKNOWN);
    }

    #[test]
    fn token_syntax_accepts_and_rejects() {
        assert!(token_syntax_ok("led_state_25"));
        assert!(token_syntax_ok("fullkeymap_scan_21"));
        assert!(token_syntax_ok("autorep_hold_25"));
        assert!(!token_syntax_ok("Led_state"));
        assert!(!token_syntax_ok("led__state"));
        assert!(!token_syntax_ok("led_"));
        assert!(!token_syntax_ok("_led"));
        assert!(!token_syntax_ok("led"));
        assert!(!token_syntax_ok("led state"));
        assert!(!token_syntax_ok("9led_state"));
        assert!(!token_syntax_ok("led_state!"));
        assert!(!token_syntax_ok(""));
        let long = "led_000000000000000000000000000000000000000";
        assert!(long.len() > 40);
        assert!(!token_syntax_ok(long));
    }

    #[test]
    fn family_prefix_is_enforced() {
        assert_eq!(family_prefix("led_state_25"), Some("led"));
        assert_eq!(family_prefix("fullkeymap_scan_21"), Some("fullkeymap"));
        assert_eq!(family_prefix("autorep_hold_25"), Some("autorep"));
        assert_eq!(family_prefix("led"), None);
        assert_eq!(family_prefix("_led"), None);
        let wrong = Token { owner: 0, family: 1, text: "autorep_hold_25" };
        assert!(!claim_ok(&wrong));
        let bad_owner = Token { owner: 10, family: 1, text: "led_state_25" };
        assert!(!claim_ok(&bad_owner));
        let bad_family = Token { owner: 0, family: 3, text: "led_state_25" };
        assert!(!claim_ok(&bad_family));
    }

    #[test]
    fn catalog_is_thirty_tokens() {
        assert_eq!(CATALOG.len(), TOKEN_TOTAL);
        assert_eq!(TOKEN_TOTAL, FAMILIES * PER_FAMILY);
        let mut i = 0usize;
        while i < CATALOG.len() {
            assert!(claim_ok(&CATALOG[i]), "catalog entry {} invalid", i);
            i += 1;
        }
    }

    #[test]
    fn catalog_has_no_duplicates() {
        let mut i = 0usize;
        while i < CATALOG.len() {
            let mut j = i + 1;
            while j < CATALOG.len() {
                assert_ne!(CATALOG[i].text, CATALOG[j].text);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn catalog_families_cover_all_owners() {
        let mut f = 0usize;
        while f < FAMILIES {
            assert_eq!(family_recount(f as u8), Some(PER_FAMILY as u32));
            assert_eq!(owner_mask(f as u8), Some(0x3FF));
            assert_eq!(grep_family(f as u8), PER_FAMILY as u32);
            f += 1;
        }
        assert_eq!(family_recount(FAMILIES as u8), None);
        assert_eq!(owner_mask(FAMILIES as u8), None);
    }

    #[test]
    fn claim_book_rejects_duplicate_text() {
        let mut book = ClaimBook::new();
        book.insert(CATALOG[0]).unwrap();
        let clone = Token { owner: 5, family: 0, text: CATALOG[0].text };
        assert_eq!(book.insert(clone), Err(SealCode::DupBad.val()));
        assert_eq!(book.len(), 1);
    }

    #[test]
    fn claim_book_rejects_bad_syntax() {
        let mut book = ClaimBook::new();
        let bad = Token { owner: 0, family: 0, text: "Fullkeymap_x" };
        assert_eq!(book.insert(bad), Err(SealCode::TokenBad.val()));
        let wrong_family = Token { owner: 0, family: 2, text: "led_state_25" };
        assert_eq!(book.insert(wrong_family), Err(SealCode::FamBad.val()));
        let bad_owner = Token { owner: 10, family: 1, text: "led_state_25" };
        assert_eq!(book.insert(bad_owner), Err(SealCode::OwnerBad.val()));
        assert!(book.is_empty());
    }

    #[test]
    fn grep_ledger_conserves() {
        let l = audit_catalog().unwrap();
        assert_eq!(l.tokens, 30);
        assert_eq!(l.families, [10, 10, 10]);
        assert!(l.owners.iter().all(|v| *v == 3));
        assert_eq!(l.duplicates, 0);
        assert_ne!(l.digest, 0);
        assert!(l.conserved());
    }

    #[test]
    fn dup_injection_rejects_all() {
        let (text, syntax) = dup_inject().unwrap();
        assert_eq!(text, DUP_INJECT);
        assert_eq!(syntax, SYNTAX_INJECT);
    }

    #[test]
    fn grep_replay_is_stable() {
        let (a, b) = grep_replay().unwrap();
        assert_eq!(a.digest, b.digest);
        assert_ne!(a.digest, 0);
    }

    #[test]
    fn card_specs_are_twenty_and_ordered() {
        assert_eq!(CARDS.len(), CARD_TOTAL);
        let mut i = 0usize;
        let mut total = 0u32;
        while i < CARDS.len() {
            assert_eq!(CARDS[i].card, CARD_FIRST + i as u32);
            assert!(CARDS[i].lines > 0);
            total += CARDS[i].lines;
            i += 1;
        }
        assert_eq!(total, DOMAIN_SEALED);
        assert_eq!(CARDS[CARD_TOTAL - 1].card, CARD_LAST);
    }

    #[test]
    fn card_slots_tile_the_window() {
        assert!(slot_scan());
        assert_eq!(card_slot(0), Some(SLOT_LO));
        assert_eq!(card_slot(CARD_TOTAL - 1), Some(SLOT_HI - SLOT_SPAN));
        assert_eq!(card_slot(CARD_TOTAL), None);
        let mut i = 0usize;
        while i + 1 < CARD_TOTAL {
            assert_eq!(card_slot(i).unwrap() + SLOT_SPAN, card_slot(i + 1).unwrap());
            i += 1;
        }
    }

    #[test]
    fn cites_follow_board_off_by_one() {
        let mut i = 0usize;
        while i < CARD_TOTAL {
            assert!(cite_ok(i));
            let want = format!("UNX-F{}-J1", CARDS[i].card + 1);
            assert_eq!(CARDS[i].cite, want);
            i += 1;
        }
        assert_eq!(CARDS[CARD_TOTAL - 1].cite, "UNX-F49041-J1");
        assert_eq!(expected_cite(CARD_LAST), Some(String::from("UNX-F49041-J1")));
    }

    #[test]
    fn seal_freezes_all_cards() {
        let s = seal().unwrap();
        assert_eq!(s.frozen_count(), CARD_TOTAL as u32);
        assert!(s.unique_cards());
        assert_eq!(s.sealed_lines, DOMAIN_SEALED);
        assert_ne!(s.digest, 0);
        let mut i = 0usize;
        while i < CARD_TOTAL {
            assert!(s.cards[i].frozen);
            assert_ne!(s.cards[i].digest, 0);
            assert_eq!(s.cards[i].slot, card_slot(i).unwrap());
            i += 1;
        }
    }

    #[test]
    fn reseal_attempts_are_rejected() {
        let mut s = seal().unwrap();
        let mut i = 0usize;
        while i < 10 {
            let cur = s.cards[i];
            let mutated = match i {
                0 => CardStamp { lines: cur.lines + 1, ..cur },
                1 => CardStamp { digest: cur.digest ^ 0x1, ..cur },
                2 => CardStamp { card: cur.card + 1, ..cur },
                3 => CardStamp { slot: cur.slot + SLOT_SPAN, ..cur },
                4 => CardStamp { cite: "UNX-F49041-J1", ..cur },
                5 => CardStamp { frozen: false, ..cur },
                6 => CardStamp { lines: cur.lines - 1, ..cur },
                7 => CardStamp { digest: 0, ..cur },
                8 => CardStamp { card: 0, ..cur },
                _ => CardStamp { slot: 0, ..cur },
            };
            assert_eq!(
                s.reseal(i, mutated),
                Err(SealCode::FreezeBad.val()),
                "index {} cur={:?} next={:?}",
                i,
                cur,
                mutated
            );
            i += 1;
        }
        let (rejected, accepted) = reseal_inject().unwrap();
        assert_eq!(rejected, 10, "rejected={} accepted={}", rejected, accepted);
        assert_eq!(accepted, 1);
    }

    #[test]
    fn budget_is_seventy_two_thousand() {
        let b = budget_ledger().unwrap();
        assert_eq!(b.sealed, 72_000);
        assert_eq!(b.budget, 240_000);
        assert_eq!(b.remaining, 168_000);
        assert!(b.conserved());
        assert_eq!(b.sealed * 10_000 / b.budget, 3_000);
    }

    #[test]
    fn budget_ratio_is_thirty_percent() {
        let b = budget_ledger().unwrap();
        assert_eq!(b.ratio_bp, 3_000);
        assert_eq!(b.ratio_bp, SEAL_RATIO_BP);
    }

    #[test]
    fn deliveries_are_hundred_and_twenty_each() {
        let (rows, digest) = deliver_all().unwrap();
        assert_eq!(rows.len(), DELIVERIES as usize);
        assert_ne!(digest, 0);
        let mut stamps = 0u32;
        let mut i = 0usize;
        while i < rows.len() {
            assert_eq!(rows[i].index, i as u32);
            assert_eq!(rows[i].items, CARD_TOTAL as u32);
            assert!(rows[i].closed);
            stamps += rows[i].items;
            assert_eq!(audit_delivery(&rows[i], if i == 0 { None } else { Some(&rows[i - 1]) }), Ok(()));
            i += 1;
        }
        assert_eq!(stamps, 2_000);
    }

    #[test]
    fn delivery_windows_do_not_cross() {
        let (deliveries, stamps, crossings, end) = window_scan().unwrap();
        assert_eq!(deliveries, 100);
        assert_eq!(stamps, 2_000);
        assert_eq!(crossings, 0);
        assert_eq!(end, DELIVERY_BASE + DELIVERIES as u64 * DELIVERY_SPAN);
        let mut i = 1u32;
        while i < DELIVERIES {
            let a = deliver(i - 1).unwrap();
            let b = deliver(i).unwrap();
            assert!(a.base + a.span <= b.base);
            assert!(a.last < b.first);
            i += 1;
        }
    }

    #[test]
    fn delivery_faults_are_detected() {
        let (rejected, accepted) = delivery_faults().unwrap();
        assert_eq!(rejected, 10);
        assert_eq!(accepted, 0);
    }

    #[test]
    fn delivery_replay_is_stable() {
        let (a, b) = delivery_replay().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
        let (x, y) = seal_replay().unwrap();
        assert_eq!(x.digest, y.digest);
        assert_ne!(x.digest, 0);
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row(SEAL_TAG, 20, 20, 72_000, 240_000, 0x0123_4567_89AB_CDEF);
        assert!(audit_render(&row));
        let (tag, vals, digest) = row_fields(&row).unwrap();
        assert_eq!(tag, SEAL_TAG);
        assert_eq!(vals, [20, 20, 72_000, 240_000]);
        assert_eq!(digest, 0x0123_4567_89AB_CDEF);
        assert!(!audit_render("SEAL|a=1"));
        assert!(!audit_render(""));
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49039_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
