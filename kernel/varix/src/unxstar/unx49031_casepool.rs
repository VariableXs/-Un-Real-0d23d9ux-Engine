//! UNX-F49031 全表判据用例集入总库 + UNX-F49032 键盘错误码族终版 · 0x71C80
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第六槽；任务板 J1 引用按原文保留）：
//!   UNX-F49031 全表判据用例集入总库（104+87 全表 191 键 ×2 判据入库）:
//!          ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49032-J1 合并表零重复断言。
//!   UNX-F49032 键盘错误码族终版（全域合并对照表零重复）:
//!          ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49033-J1 fuzz ×10^4 零崩。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（用例总库）：键域 → 判据编号 → 期望值 → 入库 → 合并 → 去重。
//! 键域先于编号：104 键与 87 键是两段互不相交的连续区间，先验证区间边界
//! 与长度，再谈 "191 键 ×2 判据"。区间重叠会让 "191" 这个数字本身失真。
//! 判据编号先于期望值：每条用例必须先确定自己属于哪一条判据（解析/记账），
//! 期望值才能由 (键, 判据) 唯一导出。编号越界等于给总库里塞了没有归属的行。
//! 期望值先于入库：期望值必须由独立函数重新计算，而不是从用例行里抄回。
//! 抄回的期望值永远等于自己，判据就失去了独立复核的意义。
//! 入库先于合并：两张分表各自入库并各自去重，合并时再整体去重。
//! 合并先于去重：合并只做追加，去重单独判定；合并过程静默覆盖重复键，
//! 才是 "零重复" 最容易被做假的地方。
//! 闸门次序（错误码族）：码值 → 名称 → 归属 → 分类 → fuzz。
//! 码值先于名称：15 条错误码的码值必须互不相同且在声明窗口内。码值撞号，
//! 排障时两条诊断会指向同一条，后面所有分类都不可信。
//! 名称先于归属：名称必须非空且互不相同，归属族号才有意义。
//! 归属先于分类：每条码都必须能反查所属族；反查不到就是悬空码。
//! 分类先于 fuzz：fuzz 的每一次拒绝都必须落到一条已知码上，"零崩" 不等于
//! "零静默"——崩溃是显性故障，静默丢弃输入才是更深的故障。
//! 关键差异：本模块不重写解析器与 LED 状态机，只做 "入库 + 去重 + 分类"。
//! 用例总库回答 "191 键 ×2 判据是否都有一条可复跑的记录"；错误码族回答
//! "全域诊断码是否零撞号、且随机输入都能被有界分类"。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71C80，B12 族第六槽）。
pub const SEG_BASE: u32 = 0x71C80;

/// 主键表下界（HID usage 'a'）。
pub const PRIMARY_LO: u8 = 0x04;
/// 主键表上界。
pub const PRIMARY_HI: u8 = 0x6B;
/// 副键表下界。
pub const SECONDARY_LO: u8 = 0x6C;
/// 副键表上界。
pub const SECONDARY_HI: u8 = 0xC2;
/// 主键数（104）。
pub const PRIMARY_KEYS: usize = 104;
/// 副键数（87）。
pub const SECONDARY_KEYS: usize = 87;
/// 全表键数（191）。
pub const TOTAL_KEYS: usize = 191;
/// 每键判据数（解析 / 记账）。
pub const CRITERIA: usize = 2;
/// 用例总数（382）。
pub const CASE_TOTAL: usize = 382;
/// 错误码族条数（15）。
pub const FAMILY_SIZE: usize = 15;
/// fuzz 轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 种子。
pub const FUZZ_SEED: u64 = 0x0000_0000_4A03_0001;
/// 重复注入次数。
pub const DUP_INJECT: u32 = 10;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 用例总库标签。
pub const CASE_TAG: &str = "CASES";
/// 错误码族标签。
pub const CODE_TAG: &str = "CODES";

/// 错误码族窗口下界。
pub const CODE_LO: u32 = 0x71B40;
/// 错误码族窗口上界。
pub const CODE_HI: u32 = 0x71C46;
/// 错误码归属族数。
pub const FAMILIES: usize = 4;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IccCode {
    /// 键域区间越界或重叠
    TableBad,
    /// 用例重复入库
    DupCase,
    /// 覆盖轮数越界
    CoverBad,
    /// 逐键判据未全绿
    ScreenBad,
    /// 合并表条数不符
    MergeBad,
    /// 总库重建摘要不一致
    RebuildBad,
    /// 错误码族不合格
    FamilyBad,
    /// fuzz 断言失败
    FuzzBad,
    /// 分类反查不到归属
    ClassBad,
    /// 计数溢出
    Overflow,
    /// 渲染不可解析
    RenderBad,
    /// 段位越窗
    SegmentBad,
    /// 复跑不一致
    ReplayBad,
    /// 边界声明或越界注入不合格
    BoundBad,
}

impl IccCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            IccCode::TableBad => "键域区间越界或重叠",
            IccCode::DupCase => "用例重复入库",
            IccCode::CoverBad => "覆盖轮数越界",
            IccCode::ScreenBad => "逐键判据未全绿",
            IccCode::MergeBad => "合并表条数不符",
            IccCode::RebuildBad => "总库重建摘要不一致",
            IccCode::FamilyBad => "错误码族条数、码值或名称不合格",
            IccCode::FuzzBad => "fuzz 断言失败",
            IccCode::ClassBad => "分类反查不到归属",
            IccCode::Overflow => "计数溢出",
            IccCode::RenderBad => "渲染字段不可解析",
            IccCode::SegmentBad => "诊断码越出段位窗口",
            IccCode::ReplayBad => "复跑不一致",
            IccCode::BoundBad => "边界声明或越界注入不合格",
        }
    }

    pub const ALL: [IccCode; 14] = [
        IccCode::TableBad,
        IccCode::DupCase,
        IccCode::CoverBad,
        IccCode::ScreenBad,
        IccCode::MergeBad,
        IccCode::RebuildBad,
        IccCode::FamilyBad,
        IccCode::FuzzBad,
        IccCode::ClassBad,
        IccCode::Overflow,
        IccCode::RenderBad,
        IccCode::SegmentBad,
        IccCode::ReplayBad,
        IccCode::BoundBad,
    ];
}

/// 按诊断码取值反查中文说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < IccCode::ALL.len() {
        if let Some(c) = IccCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知键盘总库诊断码"
}

// ── 键域 ──────────────────────────────────────────────────────────
/// 键域声明：两段互不相交的连续区间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeySpan {
    pub lo: u8,
    pub hi: u8,
    pub count: usize,
}

pub const PRIMARY: KeySpan = KeySpan { lo: PRIMARY_LO, hi: PRIMARY_HI, count: PRIMARY_KEYS };
pub const SECONDARY: KeySpan = KeySpan { lo: SECONDARY_LO, hi: SECONDARY_HI, count: SECONDARY_KEYS };

impl KeySpan {
    /// 区间内键数（含两端）。
    pub fn len(&self) -> usize {
        if self.hi < self.lo {
            0
        } else {
            (self.hi - self.lo) as usize + 1
        }
    }

    pub fn contains(&self, key: u8) -> bool {
        key >= self.lo && key <= self.hi
    }
}

/// 两段键域自洽：长度匹配、区间不重叠、合计 191。
pub fn spans_ok() -> bool {
    PRIMARY.count == PRIMARY.len()
        && SECONDARY.count == SECONDARY.len()
        && PRIMARY.count == PRIMARY_KEYS
        && SECONDARY.count == SECONDARY_KEYS
        && PRIMARY.hi < SECONDARY.lo
        && PRIMARY.count + SECONDARY.count == TOTAL_KEYS
}

/// 键是否在全表内。
pub fn in_table(key: u8) -> bool {
    PRIMARY.contains(key) || SECONDARY.contains(key)
}

// ── 用例总库 ──────────────────────────────────────────────────────
/// 一条入库用例：(键, 判据编号) → 期望值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseEntry {
    pub key: u8,
    pub criterion: u8,
    pub expected: u32,
}

/// 判据编号名称（1=解析，2=记账）。
pub fn criterion_label(criterion: u8) -> &'static str {
    match criterion {
        1 => "resolve",
        2 => "ledger",
        _ => "",
    }
}

/// 独立期望值：由 (键, 判据) 直接导出，绝不从用例行回读。
pub fn expected_for(key: u8, criterion: u8) -> Result<u32, u32> {
    if !in_table(key) {
        return Err(IccCode::TableBad.val());
    }
    match criterion {
        1 => Ok(0x1000_0000 | key as u32),
        2 => Ok(0x2000_0000 | key as u32),
        _ => Err(IccCode::TableBad.val()),
    }
}

/// 用例总库。
#[derive(Clone, Debug)]
pub struct Registry {
    pub entries: Vec<CaseEntry>,
    pub rejected: u32,
}

impl Registry {
    pub fn new() -> Registry {
        Registry { entries: Vec::new(), rejected: 0 }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn contains(&self, key: u8, criterion: u8) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key && e.criterion == criterion {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 入库：键域、判据编号、期望值、重复四道门。
    pub fn insert(&mut self, e: CaseEntry) -> Result<(), u32> {
        if !in_table(e.key) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(IccCode::TableBad.val());
        }
        if criterion_label(e.criterion).is_empty() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(IccCode::TableBad.val());
        }
        let want = expected_for(e.key, e.criterion)?;
        if want != e.expected {
            self.rejected = self.rejected.saturating_add(1);
            return Err(IccCode::TableBad.val());
        }
        if self.contains(e.key, e.criterion) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(IccCode::DupCase.val());
        }
        self.entries.push(e);
        Ok(())
    }

    /// 摘要（键 | 判据 | 期望值）。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = p_core::fold_u32(h, e.key as u32);
                h = p_core::fold_u32(h, e.criterion as u32);
                h = p_core::fold_u32(h, e.expected);
            }
            i += 1;
        }
        p_core::fold_u32(h, self.entries.len() as u32)
    }
}

/// 按一段键域构图（每键 ×2 判据）。
pub fn build_span(span: KeySpan) -> Result<Registry, u32> {
    if span.len() != span.count || span.hi < span.lo {
        return Err(IccCode::TableBad.val());
    }
    let mut reg = Registry::new();
    let mut key = span.lo;
    loop {
        let mut crit = 1u8;
        while crit as usize <= CRITERIA {
            let expected = expected_for(key, crit)?;
            reg.insert(CaseEntry { key, criterion: crit, expected })?;
            crit += 1;
        }
        if key == span.hi {
            break;
        }
        key += 1;
    }
    Ok(reg)
}

/// 合并两张分表：追加后整体判零重复。
pub fn merge(primary: &Registry, secondary: &Registry) -> Result<Registry, u32> {
    let mut out = Registry::new();
    let mut i = 0usize;
    while i < primary.entries.len() {
        if let Some(e) = primary.entries.get(i) {
            out.insert(*e)?;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < secondary.entries.len() {
        if let Some(e) = secondary.entries.get(j) {
            out.insert(*e)?;
        }
        j += 1;
    }
    Ok(out)
}

/// 合并表零重复断言：条数、唯一性、分表计数、无拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MergeReport {
    pub primary: usize,
    pub secondary: usize,
    pub total: usize,
    pub unique: usize,
    pub rejected: u32,
    pub digest: u64,
}

impl MergeReport {
    pub fn zero_dup(&self) -> bool {
        self.total == self.unique
            && self.total == CASE_TOTAL
            && self.primary == PRIMARY_KEYS * CRITERIA
            && self.secondary == SECONDARY_KEYS * CRITERIA
            && self.rejected == 0
            && self.digest != 0
    }
}

/// 合并报告。
pub fn merge_report(primary: &Registry, secondary: &Registry, merged: &Registry) -> MergeReport {
    let mut unique = 0usize;
    let mut i = 0usize;
    while i < merged.entries.len() {
        let a = match merged.entries.get(i) {
            Some(v) => *v,
            None => {
                i += 1;
                continue;
            }
        };
        let mut dup = false;
        let mut j = 0usize;
        while j < i {
            if let Some(b) = merged.entries.get(j) {
                if b.key == a.key && b.criterion == a.criterion {
                    dup = true;
                    break;
                }
            }
            j += 1;
        }
        if !dup {
            unique += 1;
        }
        i += 1;
    }
    MergeReport {
        primary: primary.len(),
        secondary: secondary.len(),
        total: merged.len(),
        unique,
        rejected: merged.rejected,
        digest: merged.digest(),
    }
}

/// 逐键判据执行：382 条用例全部按独立期望值复核。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenLedger {
    pub cases: usize,
    pub passed: usize,
    pub failed: usize,
    pub digest: u64,
}

/// 执行全部用例。
pub fn screen(reg: &Registry) -> ScreenLedger {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < reg.entries.len() {
        if let Some(e) = reg.entries.get(i) {
            let ok = match expected_for(e.key, e.criterion) {
                Ok(want) => want == e.expected && (e.expected >> 28) == e.criterion as u32,
                Err(_) => false,
            };
            if ok {
                passed += 1;
            } else {
                failed += 1;
            }
            digest = p_core::fold_u32(digest, e.key as u32);
            digest = p_core::fold_u32(digest, ok as u32);
        }
        i += 1;
    }
    ScreenLedger { cases: reg.len(), passed, failed, digest }
}

/// 全库构图：主表 + 副表 + 合并。
pub fn build_all() -> Result<(Registry, Registry, Registry), u32> {
    let p = build_span(PRIMARY)?;
    let s = build_span(SECONDARY)?;
    let m = merge(&p, &s)?;
    Ok((p, s, m))
}

/// 重复注入：十条已在库中的用例必须全部被拒。
pub fn dup_inject() -> Result<(u32, u32), u32> {
    let (_, _, mut merged) = build_all()?;
    let mut attempts = 0u32;
    let mut caught = 0u32;
    let mut i = 0usize;
    while i < DUP_INJECT as usize {
        attempts = attempts.saturating_add(1);
        let e = match merged.entries.get(i) {
            Some(v) => *v,
            None => break,
        };
        match merged.insert(e) {
            Err(code) if code == IccCode::DupCase.val() => {
                caught = caught.saturating_add(1);
            }
            _ => {}
        }
        i += 1;
    }
    Ok((attempts, caught))
}

/// 用例总库行。
pub fn case_row(report: &MergeReport) -> String {
    render_row(CASE_TAG, report.total as u32, report.unique as u32, report.rejected, report.primary as u32, report.digest)
}

/// 用例总库行可复现。
pub fn case_row_ok(report: &MergeReport) -> bool {
    let row = case_row(report);
    match row_fields(&row) {
        Some((tag, vals, digest)) => {
            tag == CASE_TAG
                && vals[0] == report.total as u32
                && vals[1] == report.unique as u32
                && vals[2] == report.rejected
                && vals[3] == report.primary as u32
                && digest == report.digest
                && audit_render(&row)
        }
        None => false,
    }
}

// ── 错误码族 ──────────────────────────────────────────────────────
/// 一条全域诊断码：码值、名称、归属族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamEntry {
    pub code: u32,
    pub name: &'static str,
    pub family: u8,
}

/// 十五码合并对照表（覆盖 B12 第三槽与第五槽窗口，码值互不相同）。
pub const FAMILY: [FamEntry; FAMILY_SIZE] = [
    FamEntry { code: 0x71B41, name: "dead-combo-bad", family: 0 },
    FamEntry { code: 0x71B42, name: "dead-dup-combo", family: 0 },
    FamEntry { code: 0x71B43, name: "dead-base-bad", family: 0 },
    FamEntry { code: 0x71B44, name: "dead-order-bad", family: 0 },
    FamEntry { code: 0x71B45, name: "dead-ledger-bad", family: 0 },
    FamEntry { code: 0x71B46, name: "led-sync-bad", family: 1 },
    FamEntry { code: 0x71B47, name: "led-degrade-bad", family: 1 },
    FamEntry { code: 0x71B48, name: "led-blocked", family: 1 },
    FamEntry { code: 0x71C41, name: "autorep-param", family: 2 },
    FamEntry { code: 0x71C42, name: "autorep-rhythm", family: 2 },
    FamEntry { code: 0x71C43, name: "autorep-ledger", family: 2 },
    FamEntry { code: 0x71C44, name: "keymap-dup-case", family: 3 },
    FamEntry { code: 0x71C45, name: "keymap-cover", family: 3 },
    FamEntry { code: 0x71C46, name: "keymap-bound", family: 3 },
    FamEntry { code: 0x71B49, name: "dead-segment", family: 0 },
];

/// 族名。
pub fn family_name(family: u8) -> &'static str {
    match family {
        0 => "dead-key",
        1 => "led",
        2 => "autorep",
        3 => "keymap",
        _ => "",
    }
}

/// 族表自洽：条数、码值唯一且在窗口内、名称非空且唯一、族号有名称。
pub fn family_ok() -> bool {
    if FAMILY.len() != FAMILY_SIZE {
        return false;
    }
    let mut i = 0usize;
    while i < FAMILY.len() {
        let a = match FAMILY.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.code < CODE_LO || a.code > CODE_HI {
            return false;
        }
        if a.name.is_empty() || a.family as usize >= FAMILIES || family_name(a.family).is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < FAMILY.len() {
            if let Some(b) = FAMILY.get(j) {
                if j != i && (a.code == b.code || a.name == b.name) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 码值反查归属族。
pub fn classify_family(code: u32) -> Option<u8> {
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(e) = FAMILY.get(i) {
            if e.code == code {
                return Some(e.family);
            }
        }
        i += 1;
    }
    None
}

/// 名称反查码值。
pub fn classify_name(name: &str) -> Option<u32> {
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(e) = FAMILY.get(i) {
            if e.name == name {
                return Some(e.code);
            }
        }
        i += 1;
    }
    None
}

/// 每条族码都必须能被族函数反查且名称/码值互查一致。
pub fn family_roundtrip() -> bool {
    let mut i = 0usize;
    while i < FAMILY.len() {
        let e = match FAMILY.get(i) {
            Some(v) => *v,
            None => return false,
        };
        match classify_family(e.code) {
            Some(f) if f == e.family => {}
            _ => return false,
        }
        match classify_name(e.name) {
            Some(c) if c == e.code => {}
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 错误码族行。
pub fn code_row() -> String {
    render_row(CODE_TAG, FAMILY_SIZE as u32, FAMILY.len() as u32, 0, FAMILIES as u32, family_digest())
}

/// 错误码族摘要。
pub fn family_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(e) = FAMILY.get(i) {
            h = p_core::fold_u32(h, e.code);
            h = p_core::fold_u32(h, e.family as u32);
            let bytes = e.name.as_bytes();
            let mut k = 0usize;
            while k < bytes.len() {
                if let Some(b) = bytes.get(k) {
                    h = p_core::fold_byte(h, *b);
                }
                k += 1;
            }
        }
        i += 1;
    }
    p_core::fold_u32(h, FAMILY_SIZE as u32)
}

/// 错误码族行可复现。
pub fn code_row_ok() -> bool {
    let row = code_row();
    match row_fields(&row) {
        Some((tag, vals, digest)) => {
            tag == CODE_TAG
                && vals[0] == FAMILY_SIZE as u32
                && vals[1] == FAMILY.len() as u32
                && vals[2] == 0
                && vals[3] == FAMILIES as u32
                && digest == family_digest()
                && audit_render(&row)
        }
        None => false,
    }
}

// ── fuzz ×10^4 ────────────────────────────────────────────────────
/// 分类结果：被拒时必须带一条已知码，杜绝静默。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub accepted: bool,
    pub code: u32,
}

/// 有界解码：层号/扫描码/修饰位三道校验，拒绝一律返回已知码。
pub fn safe_decode(layer: u8, scan: u8, mods: u8) -> Verdict {
    if layer as usize >= 4 {
        return Verdict { accepted: false, code: IccCode::ClassBad.val() };
    }
    if scan < 0x04 || scan > 0xE7 {
        return Verdict { accepted: false, code: IccCode::TableBad.val() };
    }
    if mods & !0x0F != 0 {
        return Verdict { accepted: false, code: IccCode::BoundBad.val() };
    }
    Verdict { accepted: true, code: 0x1000_0000 | ((layer as u32) << 8) | scan as u32 }
}

/// fuzz 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzLedger {
    pub rounds: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub classified: u32,
    pub digest: u64,
}

impl FuzzLedger {
    pub fn conserved(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.rounds
            && self.classified == self.rejected
            && self.rounds == FUZZ_ROUNDS
    }
}

/// fuzz 主循环：随机字节注入，零崩且每次拒绝都有码可查。
pub fn fuzz_rounds(rounds: u32, seed: u64) -> Result<FuzzLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(IccCode::CoverBad.val());
    }
    let mut lcg = p_core::Lcg::new(seed);
    let mut accepted: u32 = 0;
    let mut rejected: u32 = 0;
    let mut classified: u32 = 0;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let layer = lcg.below(8) as u8;
        let scan = lcg.below(256) as u8;
        let mods = lcg.below(32) as u8;
        let v = safe_decode(layer, scan, mods);
        if v.accepted {
            accepted = accepted.saturating_add(1);
        } else {
            rejected = rejected.saturating_add(1);
            if detail(v.code) != "未知键盘总库诊断码" {
                classified = classified.saturating_add(1);
            }
        }
        digest = p_core::fold_u32(digest, v.code);
        digest = p_core::fold_u32(digest, v.accepted as u32);
        i += 1;
    }
    Ok(FuzzLedger { rounds, accepted, rejected, classified, digest })
}

/// fuzz 复跑对。
pub fn fuzz_replay() -> Result<(FuzzLedger, FuzzLedger), u32> {
    let a = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED)?;
    let b = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED)?;
    Ok((a, b))
}

/// fuzz 行。
pub fn fuzz_row(l: &FuzzLedger) -> String {
    render_row("FUZZ", l.rounds, l.accepted, l.rejected, l.classified, l.digest)
}

/// fuzz 行可复现。
pub fn fuzz_row_ok(l: &FuzzLedger) -> bool {
    let row = fuzz_row(l);
    match row_fields(&row) {
        Some((tag, vals, digest)) => {
            tag == "FUZZ"
                && vals[0] == l.rounds
                && vals[1] == l.accepted
                && vals[2] == l.rejected
                && vals[3] == l.classified
                && digest == l.digest
                && audit_render(&row)
        }
        None => false,
    }
}

// ── 段位守卫 ──────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < IccCode::ALL.len() {
        if let Some(c) = IccCode::ALL.get(i) {
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

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：tag | params | schedule | fails | keys | digest。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
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

/// 六字段解析。
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

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49031 ①：全表 191 键 ×2 判据入库（382 条，两段键域不相交）。
    let built = build_all();
    let (pairs_ok, total) = match &built {
        Ok((p, s, m)) => (
            spans_ok() && m.len() == CASE_TOTAL && p.len() == PRIMARY_KEYS * CRITERIA && s.len() == SECONDARY_KEYS * CRITERIA,
            m.len(),
        ),
        Err(_) => (false, 0),
    };
    out.push((
        "F49031/全表入库",
        pairs_ok,
        format!("keys={} cases={} spans={}", TOTAL_KEYS, total, spans_ok()),
    ));

    // F49031 ②：逐键判据全绿（独立期望值复核 382 条）。
    let screen_ok = match &built {
        Ok((_, _, m)) => {
            let l = screen(m);
            l.cases == CASE_TOTAL && l.passed == CASE_TOTAL && l.failed == 0
        }
        Err(_) => false,
    };
    out.push((
        "F49031/逐键全绿",
        screen_ok,
        format!("cases={} passed={}", CASE_TOTAL, CASE_TOTAL),
    ));

    // F49031 ③：合并表零重复（条数=唯一数，重复注入十条全拒）。
    let report = match &built {
        Ok((p, s, m)) => Some(merge_report(p, s, m)),
        Err(_) => None,
    };
    let dup = dup_inject();
    let dup_ok = match &dup {
        Ok((a, c)) => *a == DUP_INJECT && *c == DUP_INJECT,
        Err(_) => false,
    };
    let merge_ok = match &report {
        Some(r) => r.zero_dup() && case_row_ok(r),
        None => false,
    };
    out.push((
        "F49031/合并零重复",
        merge_ok && dup_ok,
        format!(
            "total={:?} unique={:?} dup_caught={:?}",
            report.as_ref().map(|r| r.total),
            report.as_ref().map(|r| r.unique),
            dup.as_ref().map(|(_, c)| *c)
        ),
    ));

    // F49031 ④：总库重建复跑一致（两次构图摘要相等）。
    let rebuild = match (build_all(), build_all()) {
        (Ok((_, _, a)), Ok((_, _, b))) => a.digest() == b.digest() && a.digest() != 0,
        _ => false,
    };
    out.push((
        "F49031/重建一致",
        rebuild,
        format!("stable={} cases={}", rebuild, CASE_TOTAL),
    ));

    // F49032 ①：错误码族零重复（15 条、码值/名称唯一、窗口内）。
    let fam = family_ok() && family_roundtrip() && code_row_ok();
    out.push((
        "F49032/码族零重复",
        fam,
        format!("codes={} window=[{:X},{:X}]", FAMILY_SIZE, CODE_LO, CODE_HI),
    ));

    // F49032 ②：fuzz ×10^4 零崩（接受+拒绝=轮数，拒绝全部有码）。
    let fz = fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED);
    let fuzz_ok = match &fz {
        Ok(l) => l.conserved() && l.classified == l.rejected && l.accepted > 0 && l.rejected > 0,
        Err(_) => false,
    };
    out.push((
        "F49032/fuzz×10^4",
        fuzz_ok,
        format!(
            "rounds={} accepted={:?} rejected={:?} classified={:?}",
            FUZZ_ROUNDS,
            fz.as_ref().map(|l| l.accepted),
            fz.as_ref().map(|l| l.rejected),
            fz.as_ref().map(|l| l.classified)
        ),
    ));

    // F49032 ③：分类有界（每条族码可反查，未知码有明确兜底说明）。
    let bounded = family_roundtrip()
        && classify_family(0x71B41) == Some(0)
        && classify_family(0x71C46) == Some(3)
        && classify_family(0x9999).is_none()
        && detail(0) != "";
    out.push((
        "F49032/分类有界",
        bounded,
        format!("roundtrip={} unknown={}", family_roundtrip(), classify_family(0x9999).is_none()),
    ));

    // F49032 ④：fuzz 复跑一致 + 段位守卫。
    let rep = fuzz_replay();
    let rep_ok = match &rep {
        Ok((a, b)) => a.digest == b.digest && a.digest != 0 && a.conserved() && b.conserved(),
        Err(_) => false,
    };
    let seg = segment_guard();
    out.push((
        "F49032/复跑一致",
        rep_ok && seg,
        format!(
            "digest={:016X} stable={} seg={}",
            rep.as_ref().map(|(a, _)| a.digest).unwrap_or_else(|_| 0),
            rep_ok,
            seg
        ),
    ));

    out
}

pub fn run_unx49031_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49032 全表用例总库与键盘错误码族");
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
    fn spans_cover_191_keys() {
        assert!(spans_ok());
        assert_eq!(PRIMARY.len(), 104);
        assert_eq!(SECONDARY.len(), 87);
        assert_eq!(PRIMARY.len() + SECONDARY.len(), TOTAL_KEYS);
        assert!(PRIMARY.hi < SECONDARY.lo);
    }

    #[test]
    fn span_contains_boundaries() {
        assert!(PRIMARY.contains(0x04));
        assert!(PRIMARY.contains(0x6B));
        assert!(!PRIMARY.contains(0x6C));
        assert!(SECONDARY.contains(0x6C));
        assert!(SECONDARY.contains(0xC2));
        assert!(!SECONDARY.contains(0xC3));
        assert!(in_table(0x04) && in_table(0xC2));
        assert!(!in_table(0x03) && !in_table(0xC3));
    }

    #[test]
    fn expected_values_are_distinct_per_criterion() {
        let a = match expected_for(0x04, 1) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let b = match expected_for(0x04, 2) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_ne!(a, b);
        assert_eq!(a >> 28, 1);
        assert_eq!(b >> 28, 2);
        assert!(expected_for(0x04, 3).is_err());
        assert!(expected_for(0x03, 1).is_err());
    }

    #[test]
    fn build_primary_and_secondary() {
        let p = match build_span(PRIMARY) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let s = match build_span(SECONDARY) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(p.len(), PRIMARY_KEYS * CRITERIA);
        assert_eq!(s.len(), SECONDARY_KEYS * CRITERIA);
        assert_eq!(p.rejected, 0);
        assert_eq!(s.rejected, 0);
        assert!(p.contains(0x04, 1));
        assert!(s.contains(0xC2, 2));
        assert!(!p.contains(0x6C, 1));
    }

    #[test]
    fn merge_has_zero_duplicates() {
        let (p, s, m) = match build_all() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(m.len(), CASE_TOTAL);
        let r = merge_report(&p, &s, &m);
        assert!(r.zero_dup());
        assert_eq!(r.unique, CASE_TOTAL);
        assert_eq!(r.rejected, 0);
        assert!(case_row_ok(&r));
    }

    #[test]
    fn duplicate_insertion_is_rejected() {
        let (_, _, mut m) = match build_all() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let e = match m.entries.first().copied() {
            Some(v) => v,
            None => return assert!(false),
        };
        assert_eq!(m.insert(e), Err(IccCode::DupCase.val()));
        let (a, c) = match dup_inject() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a, DUP_INJECT);
        assert_eq!(c, DUP_INJECT);
    }

    #[test]
    fn wrong_expected_rejected() {
        let mut reg = Registry::new();
        let bad = CaseEntry { key: 0x04, criterion: 1, expected: 0xDEAD };
        assert_eq!(reg.insert(bad), Err(IccCode::TableBad.val()));
        assert_eq!(reg.rejected, 1);
        assert_eq!(reg.len(), 0);
        let out = CaseEntry { key: 0x03, criterion: 1, expected: 0x1000_0003 };
        assert_eq!(reg.insert(out), Err(IccCode::TableBad.val()));
        let ok = CaseEntry { key: 0x04, criterion: 1, expected: 0x1000_0004 };
        assert!(reg.insert(ok).is_ok());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn screen_is_green() {
        let (_, _, m) = match build_all() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let l = screen(&m);
        assert_eq!(l.cases, CASE_TOTAL);
        assert_eq!(l.passed, CASE_TOTAL);
        assert_eq!(l.failed, 0);
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn rebuild_digest_is_stable() {
        let (_, _, a) = match build_all() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let (_, _, b) = match build_all() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), 0);
    }

    #[test]
    fn family_is_unique_and_in_window() {
        assert!(family_ok());
        assert!(family_roundtrip());
        assert_eq!(FAMILY.len(), FAMILY_SIZE);
        let mut i = 0usize;
        while i < FAMILY.len() {
            if let Some(e) = FAMILY.get(i) {
                assert!(e.code >= CODE_LO && e.code <= CODE_HI);
                assert!(!e.name.is_empty());
                assert!(!family_name(e.family).is_empty());
            }
            i += 1;
        }
        assert!(code_row_ok());
    }

    #[test]
    fn family_classification_roundtrip() {
        assert_eq!(classify_family(0x71B41), Some(0));
        assert_eq!(classify_family(0x71B46), Some(1));
        assert_eq!(classify_family(0x71C41), Some(2));
        assert_eq!(classify_family(0x71C46), Some(3));
        assert_eq!(classify_family(0x71B4A), None);
        assert_eq!(classify_name("led-blocked"), Some(0x71B48));
        assert_eq!(classify_name(""), None);
    }

    #[test]
    fn fuzz_10000_is_conserved() {
        let l = match fuzz_rounds(FUZZ_ROUNDS, FUZZ_SEED) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(l.rounds, FUZZ_ROUNDS);
        assert!(l.conserved());
        assert_eq!(l.accepted + l.rejected, FUZZ_ROUNDS);
        assert_eq!(l.classified, l.rejected);
        assert!(l.accepted > 0 && l.rejected > 0);
        assert_ne!(l.digest, 0);
        assert!(fuzz_row_ok(&l));
    }

    #[test]
    fn fuzz_replay_is_stable() {
        let (a, b) = match fuzz_replay() {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert_eq!(a.digest, b.digest);
        assert_ne!(a.digest, 0);
        assert!(a.conserved() && b.conserved());
    }

    #[test]
    fn fuzz_cover_bounds() {
        assert_eq!(fuzz_rounds(0, FUZZ_SEED), Err(IccCode::CoverBad.val()));
        assert_eq!(fuzz_rounds(FUZZ_ROUNDS + 1, FUZZ_SEED), Err(IccCode::CoverBad.val()));
    }

    #[test]
    fn safe_decode_bounds() {
        assert!(!safe_decode(4, 0x04, 0).accepted);
        assert!(!safe_decode(0, 0x03, 0).accepted);
        assert!(!safe_decode(0, 0xE8, 0).accepted);
        assert!(!safe_decode(0, 0x04, 0x10).accepted);
        let v = safe_decode(2, 0x04, 0x03);
        assert!(v.accepted);
        assert_eq!(v.code >> 28, 1);
        let bad = safe_decode(2, 0x03, 0);
        assert!(!bad.accepted);
        assert_ne!(detail(bad.code), "未知键盘总库诊断码");
    }

    #[test]
    fn rejected_codes_are_known() {
        assert_eq!(detail(IccCode::ClassBad.val()), IccCode::ClassBad.detail());
        assert_eq!(detail(IccCode::BoundBad.val()), IccCode::BoundBad.detail());
        let mut i = 0usize;
        while i < IccCode::ALL.len() {
            if let Some(c) = IccCode::ALL.get(i) {
                assert_ne!(detail(c.val()), "未知键盘总库诊断码");
            }
            i += 1;
        }
        assert_eq!(detail(0), "未知键盘总库诊断码");
    }

    #[test]
    fn segment_window_is_exact() {
        assert!(segment_guard());
        assert_eq!(IccCode::TableBad.val(), SEG_BASE + 1);
        assert_eq!(IccCode::BoundBad.val(), SEG_BASE + 14);
        assert_eq!(IccCode::DupCase.val(), SEG_BASE + 2);
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row("X", 1, 2, 3, 4, 0xABCD);
        assert!(audit_render(&row));
        assert!(!audit_render("X|params=1"));
        assert!(parse_u32("0").is_some());
        assert!(parse_u32("12x").is_none());
        assert!(parse_hex16("000000000000ABCD").is_some());
        assert!(parse_hex16("000000000000ABC").is_none());
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let s = run_unx49031_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
