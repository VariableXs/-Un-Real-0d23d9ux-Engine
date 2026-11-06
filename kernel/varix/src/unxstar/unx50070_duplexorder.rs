//! UNX-F50070 双面与 B04 页序规范联签（交错序单源）（0x434e0 段）
//!
//! 锚点：UNX-F50070-J1 联签登记，B04 条目零改写；判据锚定 §AI-63 §1 防重声明。
//!
//! 数据段：`SEG_BASE = 0x434e0`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 13`，
//! 逐项见 [`OrderError`]。不得借位相邻卡（0x434c0 IPP 联签卡 / 0x43500
//! 纸路埋点卡）。
//!
//! 「交错序单源」的含义：双面作业的物理槽位 → 页号映射只能由 B04 规范
//! 表里的规则产生一次。本模块把规则表当作只读真值：
//! * `B04Table` 登记 18 条标准规则（3 档 per-side × 3 档双面 × 2 档页序），
//!   规则摘要是规则内容的纯函数；冻结后 `add/amend` 一律 `EntryFrozen`；
//! * `derive` 按规则生成槽位表：**顺序序**逐面顺排，**交错序**让正反面
//!   交替取奇偶页（2-up 双面正 [1,3] 背 [2,4]），两者在 1-up 上退化为同一
//!   结果——这个退化边界由判据钉住；
//! * `OrderJob` 是三态机：空 → 已联签 → 已封存；联签后任何重排都是
//!   `OverrideDenied`，封存后连联签本身也不可重做；
//! * 短边翻（tumble）只镜像背面物理格位，不改变页号归属——页序单源与
//!   翻向语义因此互不污染；
//! * 注入核：8 类坏样本（0 页、per=3、双面码 3、页序码 2、超页数、
//!   冻结后改写、重复规则、联签后重排），逐类断言诊断码。
//!
//! 槽位表用整数与显式循环构造，`verify_order` 独立复算而不信 `derive` 的
//! 自述：页号唯一、空白只在尾部、格位镜像、摘要四项任一不符都返回显式码。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x434e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x434e0;

/// 单活页数上限（与拼版卡同口径）。
pub const MAX_PAGES: u16 = 4000;
/// B04 规则表硬顶：18 条标准规则 + 2 条扩展位，第 21 条起 `TableFull`。
pub const B04_CAP: usize = 20;
/// 联签账本硬顶。
pub const SIGN_CAP: usize = 24;

/// 联签盐。
const B04_SALT: u64 = 0xB04D_0E12_0000_0070;

const E_EMPTY: u16 = 1;
const E_PER: u16 = 2;
const E_DUPLEX: u16 = 3;
const E_MODE: u16 = 4;
const E_PAGES: u16 = 5;
const E_DUPLICATE_PAGE: u16 = 6;
const E_ORDER: u16 = 7;
const E_DUPLICATE_RULE: u16 = 8;
const E_TABLE_FULL: u16 = 9;
const E_FROZEN: u16 = 10;
const E_UNKNOWN_RULE: u16 = 11;
const E_OVERRIDE: u16 = 12;
const E_SEAL: u16 = 13;

/// 页序/联签错误十三型。`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderError {
    /// 0 页作业。
    EmptyJob,
    /// per-side 不在 1/2/4/8 档。
    BadPerSide,
    /// 双面码不在 0..=2。
    BadDuplex,
    /// 页序码不在 0..=1。
    BadMode,
    /// 页数超过 4000。
    PageOverflow,
    /// 同一源页出现两次。
    DuplicatePage,
    /// 槽位结构/页号公式/摘要任一不符。
    OrderBroken,
    /// 同一 (页序, 双面, per-side) 规则重复登记。
    DuplicateRule,
    /// 规则表或联签账本已满。
    TableFull,
    /// 规则表已冻结，拒绝写入。
    EntryFrozen,
    /// 规则不存在。
    UnknownRule,
    /// 已联签/已封存后仍试图重排或重签。
    OverrideDenied,
    /// 联签摘要或封签不符。
    SealBroken,
}

impl OrderError {
    /// 段内序号（1 起），与 `code()` 一一对应。
    fn ordinal(self) -> u16 {
        match self {
            OrderError::EmptyJob => E_EMPTY,
            OrderError::BadPerSide => E_PER,
            OrderError::BadDuplex => E_DUPLEX,
            OrderError::BadMode => E_MODE,
            OrderError::PageOverflow => E_PAGES,
            OrderError::DuplicatePage => E_DUPLICATE_PAGE,
            OrderError::OrderBroken => E_ORDER,
            OrderError::DuplicateRule => E_DUPLICATE_RULE,
            OrderError::TableFull => E_TABLE_FULL,
            OrderError::EntryFrozen => E_FROZEN,
            OrderError::UnknownRule => E_UNKNOWN_RULE,
            OrderError::OverrideDenied => E_OVERRIDE,
            OrderError::SealBroken => E_SEAL,
        }
    }

    /// 诊断码：段基址 + 序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            OrderError::EmptyJob => "order job has no pages",
            OrderError::BadPerSide => "per-side value not in 1/2/4/8",
            OrderError::BadDuplex => "duplex code invalid",
            OrderError::BadMode => "page order mode invalid",
            OrderError::PageOverflow => "page count above 4000",
            OrderError::DuplicatePage => "source page placed twice",
            OrderError::OrderBroken => "slot/order/digest diverged",
            OrderError::DuplicateRule => "b04 rule already registered",
            OrderError::TableFull => "b04 table or sign ledger full",
            OrderError::EntryFrozen => "b04 rule table frozen",
            OrderError::UnknownRule => "b04 rule not found",
            OrderError::OverrideDenied => "order sealed by joint sign",
            OrderError::SealBroken => "plan digest / sign seal mismatch",
        }
    }
}

/// 全错误表。
pub const ALL_ERRORS: [OrderError; 13] = [
    OrderError::EmptyJob,
    OrderError::BadPerSide,
    OrderError::BadDuplex,
    OrderError::BadMode,
    OrderError::PageOverflow,
    OrderError::DuplicatePage,
    OrderError::OrderBroken,
    OrderError::DuplicateRule,
    OrderError::TableFull,
    OrderError::EntryFrozen,
    OrderError::UnknownRule,
    OrderError::OverrideDenied,
    OrderError::SealBroken,
];

/// 双面三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duplex {
    Simplex,
    LongEdge,
    ShortEdge,
}

impl Duplex {
    /// 从外部码解析。
    pub fn from_code(v: u8) -> Result<Duplex, OrderError> {
        match v {
            0 => Ok(Duplex::Simplex),
            1 => Ok(Duplex::LongEdge),
            2 => Ok(Duplex::ShortEdge),
            _ => Err(OrderError::BadDuplex),
        }
    }

    /// 对外码。
    pub fn code(self) -> u8 {
        match self {
            Duplex::Simplex => 0,
            Duplex::LongEdge => 1,
            Duplex::ShortEdge => 2,
        }
    }

    /// 每张纸面数。
    pub fn sides(self) -> u8 {
        match self {
            Duplex::Simplex => 1,
            Duplex::LongEdge | Duplex::ShortEdge => 2,
        }
    }

    /// 展示名。
    pub fn label(self) -> &'static str {
        match self {
            Duplex::Simplex => "simplex",
            Duplex::LongEdge => "duplex-long-edge",
            Duplex::ShortEdge => "duplex-short-edge",
        }
    }
}

/// 页序两档。交错序是 B04 的默认印刷序：正反面交替取奇偶页。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderMode {
    Sequential,
    Interleaved,
}

impl OrderMode {
    /// 从外部码解析。
    pub fn from_code(v: u8) -> Result<OrderMode, OrderError> {
        match v {
            0 => Ok(OrderMode::Sequential),
            1 => Ok(OrderMode::Interleaved),
            _ => Err(OrderError::BadMode),
        }
    }

    /// 对外码。
    pub fn code(self) -> u8 {
        match self {
            OrderMode::Sequential => 0,
            OrderMode::Interleaved => 1,
        }
    }

    /// 展示名。
    pub fn label(self) -> &'static str {
        match self {
            OrderMode::Sequential => "sequential",
            OrderMode::Interleaved => "interleaved",
        }
    }
}

/// per-side 合法档：1/2/4/8。档位越界显式拒绝，不做「就近取 4」。
pub fn check_per_side(per: u8) -> Result<u8, OrderError> {
    match per {
        1 | 2 | 4 | 8 => Ok(per),
        _ => Err(OrderError::BadPerSide),
    }
}

/// 一条 B04 页序规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderRule {
    pub mode: OrderMode,
    pub duplex: Duplex,
    pub per_side: u8,
}

impl OrderRule {
    /// 常量构造。
    pub const fn new(mode: OrderMode, duplex: Duplex, per_side: u8) -> OrderRule {
        OrderRule {
            mode,
            duplex,
            per_side,
        }
    }

    /// 规则是否等价（三元组全同）。
    pub fn same_as(&self, other: &OrderRule) -> bool {
        self.mode == other.mode
            && self.duplex == other.duplex
            && self.per_side == other.per_side
    }

    /// 每张纸槽数 = per-side × 面数。
    pub fn per_sheet(&self) -> u8 {
        self.per_side.saturating_mul(self.duplex.sides())
    }
}

/// 单个物理槽位。`slot` 是面内阅读序，`cell` 是翻页后的物理格位；
/// 两者在长边翻时恒等，短边翻时背面按行反向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub sheet: u16,
    pub side: u8,
    pub slot: u8,
    pub cell: u8,
    pub page: u16,
}

/// 槽位表（页序单源的产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderPlan {
    pub mode: OrderMode,
    pub duplex: Duplex,
    pub per_side: u8,
    pub pages: u16,
    pub sheets: u16,
    pub slots: Vec<Slot>,
    pub digest: u64,
}

impl OrderPlan {
    /// 该计划对应的 B04 规则三元组。
    pub fn rule(&self) -> OrderRule {
        OrderRule::new(self.mode, self.duplex, self.per_side)
    }
}

/// 背面物理格位：短边翻按面内槽序反向，长边翻保持。
fn cell_for(duplex: Duplex, per: u8, side: u8, slot: u8) -> u8 {
    if side == 0 {
        return slot;
    }
    match duplex {
        Duplex::Simplex | Duplex::LongEdge => slot,
        Duplex::ShortEdge => per.saturating_sub(1).saturating_sub(slot),
    }
}

/// 单槽页号。
///
/// * 顺序序：跨张、张内先正后反、面内按槽位顺排；
/// * 交错序：仅双面有意义——正反面交替取奇偶页，1-up 时与顺序序重合
///   （退化边界，判据 50070-J1-1up退化 钉住）。
fn page_for(mode: OrderMode, duplex: Duplex, per: u8, pages: u16, sheet: u16, side: u8, slot: u8) -> u16 {
    let sides = duplex.sides() as u32;
    let per32 = per as u32;
    let sequential = (sheet as u32) * sides * per32 + (side as u32) * per32 + slot as u32;
    let n = match mode {
        OrderMode::Sequential => sequential,
        OrderMode::Interleaved => {
            if sides == 1 {
                sequential
            } else {
                (sheet as u32) * per32 * 2 + (slot as u32) * 2 + (side as u32)
            }
        }
    };
    if n < pages as u32 {
        (n + 1) as u16
    } else {
        0
    }
}

/// 槽位表摘要（FNV-1a），覆盖规则、页数、张数与全部槽位。
pub fn plan_digest(p: &OrderPlan) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = mix(h, p.mode.code() as u64);
    h = mix(h, p.duplex.code() as u64);
    h = mix(h, p.per_side as u64);
    h = mix(h, p.pages as u64);
    h = mix(h, p.sheets as u64);
    h = mix(h, p.slots.len() as u64);
    let mut i = 0usize;
    while i < p.slots.len() {
        if let Some(sl) = p.slots.get(i) {
            h = mix(h, sl.sheet as u64);
            h = mix(h, sl.side as u64);
            h = mix(h, sl.slot as u64);
            h = mix(h, sl.cell as u64);
            h = mix(h, sl.page as u64);
        }
        i += 1;
    }
    h
}

/// FNV-1a 混合原语。
fn mix(h: u64, v: u64) -> u64 {
    let x = h ^ v;
    x.wrapping_mul(0x100_0000_01b3)
}

/// 规则摘要：规则三元组 + 一张「满两页」的规范槽位表。
/// 让两条字段相同但实现漂移的规则无法算出同一个摘要。
pub fn rule_digest(rule: &OrderRule) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = mix(h, rule.mode.code() as u64);
    h = mix(h, rule.duplex.code() as u64);
    h = mix(h, rule.per_side as u64);
    let sides = rule.duplex.sides();
    let per = rule.per_side;
    let pages = (per as u16) * (sides as u16) * 2;
    let mut sheet = 0u16;
    while sheet < 2 {
        let mut side = 0u8;
        while side < sides {
            let mut slot = 0u8;
            while slot < per {
                let page = page_for(rule.mode, rule.duplex, per, pages, sheet, side, slot);
                let cell = cell_for(rule.duplex, per, side, slot);
                h = mix(h, page as u64);
                h = mix(h, cell as u64);
                slot += 1;
            }
            side += 1;
        }
        sheet += 1;
    }
    h
}

/// 构造槽位表并自校验。`verify_order` 是独立复算，不信任构造器的中间量。
pub fn derive(pages: u16, per_side: u8, duplex_code: u8, mode_code: u8) -> Result<OrderPlan, OrderError> {
    if pages == 0 {
        return Err(OrderError::EmptyJob);
    }
    if pages > MAX_PAGES {
        return Err(OrderError::PageOverflow);
    }
    let per = check_per_side(per_side)?;
    let duplex = Duplex::from_code(duplex_code)?;
    let mode = OrderMode::from_code(mode_code)?;
    let sides = duplex.sides();
    let per_sheet = (per as u32) * (sides as u32);
    if per_sheet == 0 {
        return Err(OrderError::BadPerSide);
    }
    let sheets32 = ((pages as u32) + per_sheet - 1) / per_sheet;
    // 每槽 1 页，槽数 = sheets × per_sheet，u16 上限内。
    let total = sheets32 * per_sheet;
    let mut slots = Vec::with_capacity(total as usize);
    let mut sheet = 0u16;
    while sheet < sheets32 as u16 {
        let mut side = 0u8;
        while side < sides {
            let mut slot = 0u8;
            while slot < per {
                let page = page_for(mode, duplex, per, pages, sheet, side, slot);
                let cell = cell_for(duplex, per, side, slot);
                slots.push(Slot {
                    sheet,
                    side,
                    slot,
                    cell,
                    page,
                });
                slot += 1;
            }
            side += 1;
        }
        sheet += 1;
    }
    let mut plan = OrderPlan {
        mode,
        duplex,
        per_side: per,
        pages,
        sheets: sheets32 as u16,
        slots,
        digest: 0,
    };
    plan.digest = plan_digest(&plan);
    verify_order(&plan)?;
    Ok(plan)
}

/// 独立复算槽位表：结构 → 页号重复 → 空白位置 → 页号公式 → 覆盖 → 摘要。
pub fn verify_order(p: &OrderPlan) -> Result<(), OrderError> {
    if p.pages == 0 {
        return Err(OrderError::EmptyJob);
    }
    if p.pages > MAX_PAGES {
        return Err(OrderError::PageOverflow);
    }
    let per = check_per_side(p.per_side)?;
    let sides = p.duplex.sides();
    let per_sheet = (per as u32) * (sides as u32);
    let expect_sheets = ((p.pages as u32) + per_sheet - 1) / per_sheet;
    if p.sheets as u32 != expect_sheets {
        return Err(OrderError::OrderBroken);
    }
    if p.slots.len() as u32 != expect_sheets * per_sheet {
        return Err(OrderError::OrderBroken);
    }
    let mut seen = Vec::with_capacity(p.pages as usize + 1);
    let mut i = 0usize;
    while i <= p.pages as usize {
        seen.push(false);
        i += 1;
    }
    let mut blank_seen = false;
    let mut idx = 0usize;
    while idx < p.slots.len() {
        let sl = match p.slots.get(idx) {
            Some(v) => *v,
            None => return Err(OrderError::OrderBroken),
        };
        let expect_sheet = (idx / per_sheet as usize) as u16;
        let within = (idx % per_sheet as usize) as u8;
        if sl.sheet != expect_sheet
            || sl.side != within / per
            || sl.slot != within % per
        {
            return Err(OrderError::OrderBroken);
        }
        let want_cell = cell_for(p.duplex, per, sl.side, sl.slot);
        if sl.cell != want_cell {
            return Err(OrderError::OrderBroken);
        }
        if sl.page != 0 {
            if sl.page > p.pages {
                return Err(OrderError::OrderBroken);
            }
            match seen.get_mut(sl.page as usize) {
                Some(b) => {
                    if *b {
                        return Err(OrderError::DuplicatePage);
                    }
                    *b = true;
                }
                None => return Err(OrderError::OrderBroken),
            }
            if blank_seen && p.mode == OrderMode::Sequential {
                return Err(OrderError::OrderBroken);
            }
            let want = page_for(
                p.mode,
                p.duplex,
                per,
                p.pages,
                sl.sheet,
                sl.side,
                sl.slot,
            );
            if sl.page != want {
                return Err(OrderError::OrderBroken);
            }
        } else {
            blank_seen = true;
        }
        idx += 1;
    }
    let mut k = 1usize;
    while k <= p.pages as usize {
        match seen.get(k) {
            Some(true) => {}
            _ => return Err(OrderError::OrderBroken),
        }
        k += 1;
    }
    if plan_digest(p) != p.digest {
        return Err(OrderError::OrderBroken);
    }
    Ok(())
}

/// B04 规则表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B04Entry {
    pub id: u8,
    pub rule: OrderRule,
    pub digest: u64,
}

/// B04 规则表（登记 + 冻结）。它是联签的**只读真值源**：
/// 本模块只读 `find/entry_digest`，任何写入都必须显式走 `add/amend`。
#[derive(Clone, Debug)]
pub struct B04Table {
    entries: Vec<B04Entry>,
    next_id: u8,
    frozen: bool,
}

impl B04Table {
    /// 空表。
    pub fn new() -> B04Table {
        B04Table {
            entries: Vec::with_capacity(B04_CAP),
            next_id: 1,
            frozen: false,
        }
    }

    /// 18 条标准规则：per-side 1/2/4 × 双面三档 × 页序两档。
    pub fn standard() -> B04Table {
        let mut t = B04Table::new();
        let pers = [1u8, 2, 4];
        let dups = [Duplex::Simplex, Duplex::LongEdge, Duplex::ShortEdge];
        let modes = [OrderMode::Sequential, OrderMode::Interleaved];
        let mut pi = 0usize;
        while pi < pers.len() {
            if let Some(per) = pers.get(pi) {
                let mut di = 0usize;
                while di < dups.len() {
                    if let Some(d) = dups.get(di) {
                        let mut mi = 0usize;
                        while mi < modes.len() {
                            if let Some(m) = modes.get(mi) {
                                let _ = t.add(OrderRule::new(*m, *d, *per));
                            }
                            mi += 1;
                        }
                    }
                    di += 1;
                }
            }
            pi += 1;
        }
        t
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 按下标取条目。
    pub fn at(&self, idx: usize) -> Option<B04Entry> {
        self.entries.get(idx).copied()
    }

    /// 按规则查 ID。
    pub fn find(&self, rule: &OrderRule) -> Option<u8> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.rule.same_as(rule) {
                    return Some(e.id);
                }
            }
            i += 1;
        }
        None
    }

    /// 按 ID 取规则摘要。
    pub fn digest_of(&self, id: u8) -> Option<u64> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    return Some(e.digest);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记规则。重复三元组 `DuplicateRule`；满 20 条 `TableFull`；
    /// 冻结后 `EntryFrozen`。
    pub fn add(&mut self, rule: OrderRule) -> Result<u8, OrderError> {
        if self.frozen {
            return Err(OrderError::EntryFrozen);
        }
        if self.find(&rule).is_some() {
            return Err(OrderError::DuplicateRule);
        }
        if self.entries.len() >= B04_CAP {
            return Err(OrderError::TableFull);
        }
        if self.next_id == 0 || self.next_id == u8::MAX {
            return Err(OrderError::TableFull);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let digest = rule_digest(&rule);
        self.entries.push(B04Entry { id, rule, digest });
        Ok(id)
    }

    /// 显式改写规则（仅冻结前迁移用；冻结后 `EntryFrozen`，未知 ID `UnknownRule`）。
    pub fn amend(&mut self, id: u8, rule: OrderRule) -> Result<(), OrderError> {
        if self.frozen {
            return Err(OrderError::EntryFrozen);
        }
        let mut pos: Option<usize> = None;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    pos = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match pos {
            Some(p) => match self.entries.get_mut(p) {
                Some(e) => {
                    e.rule = rule;
                    e.digest = rule_digest(&rule);
                    Ok(())
                }
                None => Err(OrderError::UnknownRule),
            },
            None => Err(OrderError::UnknownRule),
        }
    }

    /// 冻结规则表：之后任何写入都是 `EntryFrozen`。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 表级内容摘要。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = mix(h, self.entries.len() as u64);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = mix(h, e.id as u64);
                h = mix(h, e.digest);
            }
            i += 1;
        }
        h
    }
}

/// 联签记录：只引用计划摘要与 B04 条目摘要，不复制槽位表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointSign {
    pub plan_digest: u64,
    pub b04_digest: u64,
    pub signer: u32,
    pub seal: u64,
}

/// 联签账本（登记 20 条 J1 记录）。
#[derive(Clone, Debug)]
pub struct SignLedger {
    records: Vec<JointSign>,
}

impl SignLedger {
    /// 空账本。
    pub fn new() -> SignLedger {
        SignLedger {
            records: Vec::with_capacity(SIGN_CAP),
        }
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 按下标取记录。
    pub fn at(&self, idx: usize) -> Option<JointSign> {
        self.records.get(idx).copied()
    }

    /// 追加一条联签；满 24 条 `TableFull`。
    pub fn record(&mut self, sign: JointSign) -> Result<(), OrderError> {
        if self.records.len() >= SIGN_CAP {
            return Err(OrderError::TableFull);
        }
        self.records.push(sign);
        Ok(())
    }
}

/// 联签封装：计划摘要 + B04 摘要 + 签署人 → 封签。
fn make_seal(plan_digest: u64, b04_digest: u64, signer: u32) -> u64 {
    mix(
        mix(plan_digest, b04_digest),
        (signer as u64) ^ B04_SALT,
    )
}

/// 作业页序三态机：空 → 已联签 → 已封存。
#[derive(Clone, Debug)]
pub struct OrderJob {
    plan: Option<OrderPlan>,
    signed: Option<JointSign>,
    sealed: bool,
}

impl OrderJob {
    /// 空作业。
    pub fn new() -> OrderJob {
        OrderJob {
            plan: None,
            signed: None,
            sealed: false,
        }
    }

    /// 当前计划。
    pub fn plan(&self) -> Option<&OrderPlan> {
        self.plan.as_ref()
    }

    /// 是否已联签。
    pub fn is_signed(&self) -> bool {
        self.signed.is_some()
    }

    /// 是否已封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 联签记录。
    pub fn sign_record(&self) -> Option<JointSign> {
        self.signed
    }

    /// 从外部计划载入（必须先通过 `verify_order`）。已联签/封存后拒绝。
    pub fn load(&mut self, plan: OrderPlan) -> Result<(), OrderError> {
        if self.sealed || self.signed.is_some() {
            return Err(OrderError::OverrideDenied);
        }
        verify_order(&plan)?;
        self.plan = Some(plan);
        Ok(())
    }

    /// 推导并载入计划。已联签/封存后拒绝——这是「联签后页序不可改写」的入口。
    pub fn build(&mut self, pages: u16, per_side: u8, duplex: u8, mode: u8) -> Result<(), OrderError> {
        if self.sealed || self.signed.is_some() {
            return Err(OrderError::OverrideDenied);
        }
        let plan = derive(pages, per_side, duplex, mode)?;
        self.plan = Some(plan);
        Ok(())
    }

    /// 联签：B04 规则必须存在；同一作业只签一次。
    pub fn sign(&mut self, table: &B04Table, signer: u32) -> Result<JointSign, OrderError> {
        if self.sealed || self.signed.is_some() {
            return Err(OrderError::OverrideDenied);
        }
        let plan = match self.plan.as_ref() {
            Some(p) => p,
            None => return Err(OrderError::EmptyJob),
        };
        let id = match table.find(&plan.rule()) {
            Some(v) => v,
            None => return Err(OrderError::UnknownRule),
        };
        let b04 = match table.digest_of(id) {
            Some(v) => v,
            None => return Err(OrderError::UnknownRule),
        };
        let sign = JointSign {
            plan_digest: plan.digest,
            b04_digest: b04,
            signer,
            seal: make_seal(plan.digest, b04, signer),
        };
        self.signed = Some(sign);
        Ok(sign)
    }

    /// 联签并封存：封存后连重新联签都拒绝。
    pub fn seal_with(&mut self, table: &B04Table, signer: u32) -> Result<JointSign, OrderError> {
        let sign = self.sign(table, signer)?;
        self.sealed = true;
        Ok(sign)
    }

    /// 复核联签：计划摘要与封签双项一致才算通过。
    pub fn verify_sign(&self, sign: &JointSign) -> Result<(), OrderError> {
        let plan = match self.plan.as_ref() {
            Some(p) => p,
            None => return Err(OrderError::EmptyJob),
        };
        if sign.plan_digest != plan.digest {
            return Err(OrderError::SealBroken);
        }
        if make_seal(sign.plan_digest, sign.b04_digest, sign.signer) != sign.seal {
            return Err(OrderError::SealBroken);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 注入核：八类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 构造 8 类坏样本并断言诊断码。返回 (抓到, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1. 0 页。
    if derive(0, 2, 1, 1).err() == Some(OrderError::EmptyJob) {
        caught += 1;
    }
    // 2. per-side = 3 不在档位。
    if derive(8, 3, 1, 0).err() == Some(OrderError::BadPerSide) {
        caught += 1;
    }
    // 3. 双面码 3。
    if derive(8, 2, 3, 0).err() == Some(OrderError::BadDuplex) {
        caught += 1;
    }
    // 4. 页序码 2。
    if derive(8, 2, 1, 2).err() == Some(OrderError::BadMode) {
        caught += 1;
    }
    // 5. 超页数。
    if derive(MAX_PAGES + 1, 2, 1, 0).err() == Some(OrderError::PageOverflow) {
        caught += 1;
    }
    // 6. 冻结后改写规则。
    let mut frozen = B04Table::standard();
    frozen.freeze();
    if frozen.amend(1, OrderRule::new(OrderMode::Sequential, Duplex::Simplex, 1)).err()
        == Some(OrderError::EntryFrozen)
    {
        caught += 1;
    }
    // 7. 重复规则。
    let mut dup = B04Table::new();
    let rule = OrderRule::new(OrderMode::Interleaved, Duplex::LongEdge, 2);
    if dup.add(rule).is_ok() && dup.add(rule).err() == Some(OrderError::DuplicateRule) {
        caught += 1;
    }
    // 8. 联签后重排。
    let table = B04Table::standard();
    let mut job = OrderJob::new();
    if job.build(8, 2, 1, 1).is_ok() && job.sign(&table, 70).is_ok()
        && job.build(9, 2, 1, 1).err() == Some(OrderError::OverrideDenied)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// J1 的 20 组作业：档位 × 双面 × 页序 × 页数四轴交叠。
fn audit_jobs() -> [(u16, u8, u8, u8); 20] {
    [
        (1, 1, 0, 0),
        (2, 1, 1, 0),
        (3, 1, 2, 1),
        (8, 2, 1, 1),
        (7, 2, 1, 0),
        (16, 2, 2, 0),
        (9, 4, 1, 1),
        (20, 4, 2, 0),
        (33, 4, 0, 0),
        (5, 2, 0, 0),
        (12, 1, 1, 1),
        (64, 4, 1, 0),
        (3, 4, 2, 1),
        (17, 2, 2, 1),
        (2, 4, 0, 1),
        (31, 1, 2, 0),
        (40, 4, 1, 1),
        (6, 2, 1, 1),
        (11, 1, 0, 1),
        (25, 4, 2, 1),
    ]
}

/// J1 主审计：20 组作业逐项推导 → 自校验 → 联签 → 记录；
/// 随后核对 (联签数, 总数, B04 摘要是否全程不变, 冻结后改写是否被拒)。
pub fn audit_b04_joint() -> (usize, usize, bool, bool) {
    let mut table = B04Table::standard();
    let before = table.digest();
    let mut ledger = SignLedger::new();
    let jobs = audit_jobs();
    let mut signed = 0usize;
    let mut i = 0usize;
    while i < jobs.len() {
        if let Some((pages, per, dup, mode)) = jobs.get(i) {
            if let Ok(plan) = derive(*pages, *per, *dup, *mode) {
                let mut job = OrderJob::new();
                if job.load(plan).is_ok() {
                    if let Ok(sign) = job.sign(&table, 70) {
                        if job.verify_sign(&sign).is_ok() && ledger.record(sign).is_ok() {
                            signed += 1;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    let stable = table.digest() == before;
    table.freeze();
    let frozen = table.amend(1, OrderRule::new(OrderMode::Sequential, Duplex::Simplex, 1)).err()
        == Some(OrderError::EntryFrozen)
        && table
            .add(OrderRule::new(OrderMode::Interleaved, Duplex::ShortEdge, 8))
            .err()
            == Some(OrderError::EntryFrozen);
    (signed, jobs.len(), stable, frozen)
}

/// 页覆盖核查：1..=pages 恰好各一次，返回 (重复数, 缺失数)。
fn coverage_audit(plan: &OrderPlan) -> (usize, usize) {
    let mut seen = Vec::with_capacity(plan.pages as usize + 1);
    let mut i = 0usize;
    while i <= plan.pages as usize {
        seen.push(false);
        i += 1;
    }
    let mut dup = 0usize;
    let mut k = 0usize;
    while k < plan.slots.len() {
        if let Some(sl) = plan.slots.get(k) {
            if sl.page != 0 {
                match seen.get_mut(sl.page as usize) {
                    Some(b) => {
                        if *b {
                            dup += 1;
                        }
                        *b = true;
                    }
                    None => dup += 1,
                }
            }
        }
        k += 1;
    }
    let mut missing = 0usize;
    let mut p = 1usize;
    while p <= plan.pages as usize {
        if seen.get(p) != Some(&true) {
            missing += 1;
        }
        p += 1;
    }
    (dup, missing)
}

pub fn run_unx50070_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50070");

    // J1-1 主判据：20 组联签登记，B04 摘要零漂移，冻结后改写被拒。
    let (signed, total, stable, frozen) = audit_b04_joint();
    let ok = signed == 20 && total == 20 && stable && frozen;
    let det = format!("signed={}/{} stable={} frozen={}", signed, total, stable, frozen);
    s.add_dyn("50070-J1-联签零改写", ok, &det);

    // J1-2 交错序与顺序序在 2-up 双面上必须不同：正 [1,2] vs 正 [1,3]。
    let seq = derive(8, 2, 1, 0);
    let ilv = derive(8, 2, 1, 1);
    let ok = match (&seq, &ilv) {
        (Ok(a), Ok(b)) => {
            let sa: Vec<u16> = a.slots.iter().take(4).map(|x| x.page).collect();
            let sb: Vec<u16> = b.slots.iter().take(4).map(|x| x.page).collect();
            sa == [1, 2, 3, 4] && sb == [1, 3, 2, 4]
        }
        _ => false,
    };
    s.add("50070-J1-交错序分档", ok, if ok { "" } else { "交错序未分档" });

    // J1-3 交错序页覆盖：2-up 长边 8 页，1..8 恰好各一次。
    let ok = match &ilv {
        Ok(p) => {
            let (dup, missing) = coverage_audit(p);
            dup == 0 && missing == 0 && verify_order(p).is_ok()
        }
        Err(_) => false,
    };
    s.add("50070-J1-交错页覆盖", ok, if ok { "" } else { "交错序漏页/重页" });

    // J1-4 退化边界：1-up 双面时交错序与顺序序槽位逐位相同。
    // 摘要含页序档位，故只比较槽位表与两者各自可校验，不比较 digest。
    let one_seq = derive(7, 1, 1, 0);
    let one_ilv = derive(7, 1, 1, 1);
    let ok = match (&one_seq, &one_ilv) {
        (Ok(a), Ok(b)) => {
            a.slots == b.slots && verify_order(a).is_ok() && verify_order(b).is_ok()
        }
        _ => false,
    };
    s.add("50070-J1-1up退化", ok, if ok { "" } else { "1-up 两序不等价" });

    // J1-5 短边翻镜像：仅背面格位反向，正面与页号不变。
    let long = derive(16, 4, 1, 0);
    let short = derive(16, 4, 2, 0);
    let ok = match (&long, &short) {
        (Ok(a), Ok(b)) => {
            let mut back_long = [255u8; 4];
            let mut back_short = [255u8; 4];
            let mut n1 = 0usize;
            let mut n2 = 0usize;
            let mut i = 0usize;
            while i < a.slots.len() {
                if let Some(sl) = a.slots.get(i) {
                    if sl.side == 1 && n1 < 4 {
                        back_long[n1] = sl.cell;
                        n1 += 1;
                    }
                }
                if let Some(sl) = b.slots.get(i) {
                    if sl.side == 1 && n2 < 4 {
                        back_short[n2] = sl.cell;
                        n2 += 1;
                    }
                }
                i += 1;
            }
            n1 == 4
                && n2 == 4
                && back_long == [0, 1, 2, 3]
                && back_short == [3, 2, 1, 0]
        }
        _ => false,
    };
    s.add("50070-J1-短边镜像", ok, if ok { "" } else { "短边背面未镜像" });

    // J1-6 空白只在尾部：7 页 2-up 双面 = 8 槽，最后一槽为空白。
    let blank = derive(7, 2, 1, 0);
    let ok = match &blank {
        Ok(p) => {
            let last = p.slots.last().map(|x| x.page);
            let blank_count = p.slots.iter().filter(|x| x.page == 0).count();
            last == Some(0) && blank_count == 1 && verify_order(p).is_ok()
        }
        Err(_) => false,
    };
    s.add("50070-J1-尾部补白", ok, if ok { "" } else { "空白位置错误" });

    // J1-7 确定性：同一组参数两次推导，槽位与摘要逐位一致。
    let d1 = derive(31, 4, 2, 1);
    let d2 = derive(31, 4, 2, 1);
    let ok = d1 == d2 && matches!(d1, Ok(_));
    s.add("50070-J1-推导确定", ok, if ok { "" } else { "两次推导不一致" });

    // J1-8 摘要敏感：页数/档位/双面/页序任一变化都必须改摘要。
    let base = derive(8, 2, 1, 0);
    let p9 = derive(9, 2, 1, 0);
    let p4 = derive(8, 4, 1, 0);
    let dpx = derive(8, 2, 2, 0);
    let mode = derive(8, 2, 1, 1);
    let ok = match (&base, &p9, &p4, &dpx, &mode) {
        (Ok(a), Ok(b), Ok(c), Ok(d), Ok(e)) => {
            a.digest != b.digest
                && a.digest != c.digest
                && a.digest != d.digest
                && a.digest != e.digest
        }
        _ => false,
    };
    s.add("50070-J1-摘要敏感", ok, if ok { "" } else { "摘要对输入不敏感" });

    // J1-9 输入分码：0 页/坏档位/坏双面/坏页序/超页数 五条各命中自身。
    let ok = derive(0, 2, 1, 0).err() == Some(OrderError::EmptyJob)
        && derive(8, 3, 1, 0).err() == Some(OrderError::BadPerSide)
        && derive(8, 2, 3, 0).err() == Some(OrderError::BadDuplex)
        && derive(8, 2, 1, 2).err() == Some(OrderError::BadMode)
        && derive(MAX_PAGES + 1, 2, 1, 0).err() == Some(OrderError::PageOverflow);
    s.add("50070-J1-输入分码", ok, if ok { "" } else { "输入错误混码" });

    // J1-10 校验器抓两类人工破坏：页号重复 / 页号乱序。
    let mut caught = 0usize;
    if let Ok(mut p) = derive(8, 2, 1, 0) {
        if let Some(sl) = p.slots.get_mut(1) {
            sl.page = 1;
        }
        p.digest = plan_digest(&p);
        if verify_order(&p).err() == Some(OrderError::DuplicatePage) {
            caught += 1;
        }
    }
    if let Ok(mut p) = derive(8, 2, 1, 0) {
        if let Some(sl) = p.slots.get_mut(1) {
            sl.page = 99;
        }
        p.digest = plan_digest(&p);
        if verify_order(&p).err() == Some(OrderError::OrderBroken) {
            caught += 1;
        }
    }
    let ok = caught == 2;
    let det = format!("{}/2", caught);
    s.add_dyn("50070-J1-校验抓破坏", ok, &det);

    // J1-11 联签状态机：正常签可复核；重排/重签 OverrideDenied；
    // 摘要或封签被改 SealBroken。
    let table = B04Table::standard();
    let mut job = OrderJob::new();
    let ok = job.build(8, 2, 1, 1).is_ok()
        && job.sign(&table, 70).is_ok()
        && match job.sign(&table, 70) {
            Err(OrderError::OverrideDenied) => true,
            _ => false,
        }
        && job.build(9, 2, 1, 1).err() == Some(OrderError::OverrideDenied)
        && match job.sign_record() {
            Some(sig) => {
                let mut bad_plan = sig;
                bad_plan.plan_digest ^= 0x1;
                let mut bad_seal = sig;
                bad_seal.seal ^= 0x2;
                job.verify_sign(&sig).is_ok()
                    && job.verify_sign(&bad_plan).err() == Some(OrderError::SealBroken)
                    && job.verify_sign(&bad_seal).err() == Some(OrderError::SealBroken)
            }
            None => false,
        };
    s.add("50070-J1-联签状态机", ok, if ok { "" } else { "状态机失守" });

    // J1-12 未知规则：规则表只登记顺序序时，交错序作业联签必须 UnknownRule。
    let mut partial = B04Table::new();
    let _ = partial.add(OrderRule::new(OrderMode::Sequential, Duplex::LongEdge, 2));
    let mut job2 = OrderJob::new();
    let ok = job2.build(8, 2, 1, 1).is_ok()
        && job2.sign(&partial, 70).err() == Some(OrderError::UnknownRule);
    s.add("50070-J1-未知规则", ok, if ok { "" } else { "缺规则仍可联签" });

    // J1-13 B04 表语义：重复规则分码；20 条满后 TableFull；冻结后拒写。
    let mut t = B04Table::standard();
    let rule = OrderRule::new(OrderMode::Sequential, Duplex::Simplex, 1);
    let dup_err = t.add(rule).err();
    let add8a = t.add(OrderRule::new(OrderMode::Sequential, Duplex::Simplex, 8));
    let add8b = t.add(OrderRule::new(OrderMode::Interleaved, Duplex::Simplex, 8));
    let full_err = t.add(OrderRule::new(OrderMode::Sequential, Duplex::LongEdge, 8)).err();
    let ok = t.len() == 20
        && dup_err == Some(OrderError::DuplicateRule)
        && add8a.is_ok()
        && add8b.is_ok()
        && full_err == Some(OrderError::TableFull);
    let det = format!("len={} dup={:?} full={:?}", t.len(), dup_err, full_err);
    s.add_dyn("50070-J1-B04表满", ok, &det);

    // J1-14 B04 冻结：amend 已知/未知 ID 均不得穿透冻结。
    let mut fz = B04Table::standard();
    let before = fz.digest();
    fz.freeze();
    let ok = fz.amend(1, OrderRule::new(OrderMode::Interleaved, Duplex::ShortEdge, 8)).err()
        == Some(OrderError::EntryFrozen)
        && fz.amend(99, OrderRule::new(OrderMode::Sequential, Duplex::Simplex, 1)).err()
            == Some(OrderError::EntryFrozen)
        && fz.digest() == before;
    s.add("50070-J1-B04冻结", ok, if ok { "" } else { "冻结后仍可写" });

    // J1-15 联签账本硬顶：24 条满后 TableFull。
    let mut ledger = SignLedger::new();
    let mut n = 0usize;
    while n < SIGN_CAP {
        let sign = JointSign {
            plan_digest: n as u64 + 1,
            b04_digest: 0x44,
            signer: 70,
            seal: n as u64,
        };
        if ledger.record(sign).is_ok() {
            n += 1;
        } else {
            break;
        }
    }
    let ok = ledger.len() == 24
        && ledger
            .record(JointSign {
                plan_digest: 0,
                b04_digest: 0,
                signer: 0,
                seal: 0,
            })
            .err()
            == Some(OrderError::TableFull);
    let det = format!("len={}", ledger.len());
    s.add_dyn("50070-J1-联签账满", ok, &det);

    // J1-16 错误码闭集：13 条，码值 = SEG_BASE + 1..=13 且两两不同。
    let mut codes_ok = ALL_ERRORS.len() == 13;
    let mut marks = [false; 14];
    let mut e = 0usize;
    while e < ALL_ERRORS.len() {
        if let Some(err) = ALL_ERRORS.get(e) {
            if err.code() != SEG_BASE + (e as u32) + 1 {
                codes_ok = false;
            }
            let off = (err.code() - SEG_BASE) as usize;
            match marks.get_mut(off) {
                Some(m) => {
                    if *m {
                        codes_ok = false;
                    }
                    *m = true;
                }
                None => codes_ok = false,
            }
        }
        e += 1;
    }
    s.add("50070-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    // J1-17 注入核 8 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50070-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50070_checks_green() {
        let set = run_unx50070_checks();
        assert!(set.all_passed(), "UNX-F50070 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50070_interleave_pages_are_unique() {
        let p = derive(23, 4, 1, 1);
        match p {
            Ok(plan) => {
                let (dup, missing) = coverage_audit(&plan);
                assert_eq!(dup, 0);
                assert_eq!(missing, 0);
            }
            Err(_) => panic!("derive failed"),
        }
    }

    #[test]
    fn unx50070_sign_seal_is_deterministic() {
        let table = B04Table::standard();
        let mut a = OrderJob::new();
        let mut b = OrderJob::new();
        let _ = a.build(12, 2, 1, 1);
        let _ = b.build(12, 2, 1, 1);
        assert_eq!(a.sign(&table, 7), b.sign(&table, 7));
    }
}
