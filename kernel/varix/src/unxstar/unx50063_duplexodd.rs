//! UNX-F50063 双面奇数页补白页语义（显式登记不静默）（0x43400 段）
//!
//! 锚点：UNX-F50063-J1 补白 ×10 显式记账；判据锚定 §AI-63 §2 专题四。
//!
//! 数据段：`SEG_BASE = 0x43400`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 12`，
//! 逐项见 [`PadError`]。
//!
//! 为什么补白必须「显式登记」：双面机上一张纸的两个面是物理资源。奇数页时末张
//! 背面留白、分节时上一节若停在正面也要补一张背面，这些空白面**会改变后续页的
//! 物理位置**，也会进入装订与折页计算。如果实现只是「循环到页数用完，剩下就不
//! 管」，空白面就成了隐式事实：谁补的、为什么补、补在哪张纸的哪一面、紧随哪一页
//! ——全都无从审计。本模块因此把每一个空白面都写成一条 [`BlankRow`]：纸号 + 面 +
//! 原因 + 锚定页 + 序号，缺一即审计失败。
//!
//! 三条补白来源：
//! * [`PadReason::TailParity`]：作业末尾奇数页，补末张背面；
//! * [`PadReason::SectionBreak`]：分节必须从新纸正面起排，上一节停在正面时补背面；
//! * [`PadReason::StartOffset`]：从首张背面起印（半张纸续印），正面先登记为空白；
//! * [`PadReason::ManualInsert`]：手工插入的隔页整张纸，两面都空白，成对登记。
//!
//! 结构清单：
//! * [`PadReason`] + [`Side`] + [`BlankRow`]：空白面的四元登记；
//! * [`BlankLedger`]：定容账本（序号连续 + 槽唯一 + 锚非零 + 封账只读）；
//! * [`SectionPlan`]：分节拼版状态（含闭式审计 [`SectionPlan::audit`]）；
//! * [`reference_blanks`]：与计划器独立的闭式参考表（两种推导必须逐项同解）；
//! * [`injection_core`]：9 类坏样本注入核。
//!
//! 边界纪律：所有槽位运算走 u32 中间量，构造后再收窄到 u16；页数、节数、总槽数
//! 三层上限都在入口显式拒绝，绝不靠溢出回绕「碰巧对上」。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43400 独占 0x20）。
pub const SEG_BASE: u32 = 0x43400;

const E_ZERO_PAGES: u16 = 1;
const E_BAD_SEGMENT: u16 = 2;
const E_TOO_MANY_PAGES: u16 = 3;
const E_TOO_MANY_SECTIONS: u16 = 4;
const E_BAD_REASON: u16 = 5;
const E_ANCHOR_ZERO: u16 = 6;
const E_SLOT_CONFLICT: u16 = 7;
const E_ORDINAL_GAP: u16 = 8;
const E_FULL: u16 = 9;
const E_IMMUTABLE: u16 = 10;
const E_SUM_MISMATCH: u16 = 11;
const E_AUDIT: u16 = 12;

/// 补白语义错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadError {
    /// 总页数为 0（空作业不产生纸）。
    ZeroPages,
    /// 某个分节长度为 0（空节没有「从新纸正面起排」的语义）。
    BadSegment,
    /// 总页数超过拼版上限。
    PageOverflow,
    /// 分节数超过上限。
    SectionOverflow,
    /// 补白原因码不在闭集 1..=4。
    BadReason,
    /// 锚定页为 0（空白面必须锚在某一页之后）。
    AnchorZero,
    /// 同一张纸的同一面被登记两次。
    SlotConflict,
    /// 登记序号跳号（账本必须逐行连续）。
    OrdinalGap,
    /// 账本容量已满。
    Full,
    /// 封账后只读。
    Immutable,
    /// 页数/纸数/空白数的守恒式不成立。
    SumMismatch,
    /// 闭式参考表与账本逐项不一致（原因、锚、位置任一漂移）。
    Audit,
}

impl PadError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PadError::ZeroPages => E_ZERO_PAGES,
            PadError::BadSegment => E_BAD_SEGMENT,
            PadError::PageOverflow => E_TOO_MANY_PAGES,
            PadError::SectionOverflow => E_TOO_MANY_SECTIONS,
            PadError::BadReason => E_BAD_REASON,
            PadError::AnchorZero => E_ANCHOR_ZERO,
            PadError::SlotConflict => E_SLOT_CONFLICT,
            PadError::OrdinalGap => E_ORDINAL_GAP,
            PadError::Full => E_FULL,
            PadError::Immutable => E_IMMUTABLE,
            PadError::SumMismatch => E_SUM_MISMATCH,
            PadError::Audit => E_AUDIT,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PadError::ZeroPages => "total page count is zero",
            PadError::BadSegment => "section length is zero",
            PadError::PageOverflow => "page count above imposition limit",
            PadError::SectionOverflow => "section count above limit",
            PadError::BadReason => "padding reason code outside closed set",
            PadError::AnchorZero => "blank side anchored to page zero",
            PadError::SlotConflict => "physical side registered twice",
            PadError::OrdinalGap => "blank registration ordinal gap",
            PadError::Full => "blank ledger capacity reached",
            PadError::Immutable => "blank ledger is sealed read-only",
            PadError::SumMismatch => "page/sheet/blank conservation broken",
            PadError::Audit => "plan diverges from closed-form reference",
        }
    }
}

/// 纸面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// 正面（先印）。
    Front,
    /// 背面（翻过之后印）。
    Back,
}

impl Side {
    /// 面序 0/1（物理键用）。
    pub fn index(self) -> u8 {
        match self {
            Side::Front => 0,
            Side::Back => 1,
        }
    }

    /// 面名。
    pub fn label(self) -> &'static str {
        match self {
            Side::Front => "front",
            Side::Back => "back",
        }
    }
}

/// 补白原因。四个码值构成闭集，任何一条空白面必须能归到其中一类——归不进去
/// 的空白面就是「静默补白」，本项目不接受。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadReason {
    /// 从首张背面起印，正面留白。
    StartOffset,
    /// 分节边界：上一节停在正面，补背面以便下一节从新纸正面起排。
    SectionBreak,
    /// 作业末尾奇数页的末张背面。
    TailParity,
    /// 手工插入的隔页整张纸（两面成对登记）。
    ManualInsert,
}

impl PadReason {
    /// 原因码 1..=4（0 保留为「未登记」哨兵，故 from_code 拒 0）。
    pub fn code(self) -> u8 {
        match self {
            PadReason::StartOffset => 1,
            PadReason::SectionBreak => 2,
            PadReason::TailParity => 3,
            PadReason::ManualInsert => 4,
        }
    }

    /// 码值反解；闭集外返回 [`PadError::BadReason`]。
    pub fn from_code(c: u8) -> Result<PadReason, PadError> {
        match c {
            1 => Ok(PadReason::StartOffset),
            2 => Ok(PadReason::SectionBreak),
            3 => Ok(PadReason::TailParity),
            4 => Ok(PadReason::ManualInsert),
            _ => Err(PadError::BadReason),
        }
    }

    /// 直方图槽位 0..=3。
    pub fn slot(self) -> usize {
        (self.code() - 1) as usize
    }

    /// 原因名。
    pub fn label(self) -> &'static str {
        match self {
            PadReason::StartOffset => "start-offset",
            PadReason::SectionBreak => "section-break",
            PadReason::TailParity => "tail-parity",
            PadReason::ManualInsert => "manual-insert",
        }
    }

    /// 全部四类（遍历用）。
    pub const ALL: [PadReason; 4] = [
        PadReason::StartOffset,
        PadReason::SectionBreak,
        PadReason::TailParity,
        PadReason::ManualInsert,
    ];
}

/// 一条空白面登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlankRow {
    /// 纸号（0 起）。
    pub sheet: u16,
    /// 面。
    pub side: Side,
    /// 补白原因。
    pub reason: PadReason,
    /// 锚定页（1 起）：该空白面紧随的作业页，必须非零。
    pub anchor_page: u16,
    /// 登记序号（0 起，必须与入账位置一致）。
    pub ordinal: u16,
}

impl BlankRow {
    /// 物理键：纸号 × 2 + 面序。
    pub fn key(&self) -> u32 {
        (self.sheet as u32) * 2 + self.side.index() as u32
    }

    /// 一行落账文本。
    pub fn line(&self) -> String {
        format!(
            "#{} s{} {} {} anchor{}",
            self.ordinal,
            self.sheet,
            self.side.label(),
            self.reason.label(),
            self.anchor_page
        )
    }
}

/// 页数上限（A4 手册量级）。
pub const MAX_PAGES: u16 = 10_000;
/// 分节数上限（足够覆盖章节/附件/索引的常规结构）。
pub const MAX_SECTIONS: usize = 64;
/// 空白面登记上限（补白数远小于页数，但仍留足余量）。
pub const MAX_BLANKS: usize = 16_384;

/// 空白面账本：定容、序号连续、槽唯一、锚非零、封账只读。
///
/// 它只负责「登记事实」，不判断补白是否必要；必要性由 [`SectionPlan`] 的闭式
/// 审计负责。两层分开后，账本可以被序列化/回放而不携带拼版算法。
pub struct BlankLedger {
    rows: Vec<BlankRow>,
    capacity: usize,
    sealed: bool,
    rejected: u32,
}

impl BlankLedger {
    /// 建空账；容量 0 按 1 处理，上限 [`MAX_BLANKS`]。
    pub fn new(capacity: usize) -> BlankLedger {
        let cap = if capacity == 0 {
            1
        } else if capacity > MAX_BLANKS {
            MAX_BLANKS
        } else {
            capacity
        };
        BlankLedger {
            rows: Vec::with_capacity(cap),
            capacity: cap,
            sealed: false,
            rejected: 0,
        }
    }

    fn reject(&mut self, e: PadError) -> Result<(), PadError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 登记一条空白面：封账 → 容量 → 锚非零 → 序号连续 → 槽唯一。
    pub fn insert(&mut self, row: BlankRow) -> Result<(), PadError> {
        if self.sealed {
            return self.reject(PadError::Immutable);
        }
        if self.rows.len() >= self.capacity {
            return self.reject(PadError::Full);
        }
        if row.anchor_page == 0 {
            return self.reject(PadError::AnchorZero);
        }
        if row.ordinal as usize != self.rows.len() {
            return self.reject(PadError::OrdinalGap);
        }
        let key = row.key();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(o) = self.rows.get(i) {
                if o.key() == key {
                    return self.reject(PadError::SlotConflict);
                }
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 全表复核：序号连续、槽唯一、锚非零。
    pub fn verify(&self) -> Result<usize, PadError> {
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PadError::Audit),
            };
            if r.ordinal as usize != i || r.anchor_page == 0 {
                return Err(PadError::OrdinalGap);
            }
            let mut j = i + 1;
            while j < self.rows.len() {
                if let Some(o) = self.rows.get(j) {
                    if o.key() == r.key() {
                        return Err(PadError::SlotConflict);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(self.rows.len())
    }

    /// 封账：之后登记一律 [`PadError::Immutable`]。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取第 `i` 条（越界 `None`）。
    pub fn row(&self, i: usize) -> Option<&BlankRow> {
        self.rows.get(i)
    }

    /// 按物理键查找。
    pub fn find(&self, sheet: u16, side: Side) -> Option<&BlankRow> {
        let key = (sheet as u32) * 2 + side.index() as u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.key() == key {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 原因直方图：[StartOffset, SectionBreak, TailParity, ManualInsert]。
    pub fn histogram(&self) -> [u16; 4] {
        let mut hist = [0u16; 4];
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if let Some(slot) = hist.get_mut(r.reason.slot()) {
                    *slot = slot.saturating_add(1);
                }
            }
            i += 1;
        }
        hist
    }

    /// 账本指纹：按序号串位置/原因/锚。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fnv_u16(h, r.sheet);
                h = fnv_byte(h, r.side.index());
                h = fnv_byte(h, r.reason.code());
                h = fnv_u16(h, r.anchor_page);
                h = fnv_u16(h, r.ordinal);
            }
            i += 1;
        }
        h
    }

    /// 落账文本。
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                out.push_str(&r.line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// 审计结果：把「为什么补、补了几张」变成可断言的数字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audit {
    /// 总页数。
    pub pages: u16,
    /// 总纸数。
    pub sheets: u16,
    /// 空白面总数。
    pub blanks: usize,
    /// 起印偏移空白面数。
    pub offset_blanks: u16,
    /// 分节边界空白面数。
    pub section_blanks: u16,
    /// 末页奇偶空白面数。
    pub tail_blanks: u16,
    /// 手工隔页空白面数。
    pub manual_blanks: u16,
}

/// 分节拼版计划：pages + blanks = 2 × sheets 是它唯一的不变量。
pub struct SectionPlan {
    pages: u16,
    sections: Vec<u16>,
    start_on_back: bool,
    sheets: u16,
    manual_sheets: u16,
    blanks: BlankLedger,
}

/// 校验分节表：空表、零长节、节数超限、页数超限。
fn validate_sections(sections: &[u16]) -> Result<(u16, usize), PadError> {
    if sections.is_empty() {
        return Err(PadError::ZeroPages);
    }
    if sections.len() > MAX_SECTIONS {
        return Err(PadError::SectionOverflow);
    }
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < sections.len() {
        match sections.get(i) {
            None => return Err(PadError::BadSegment),
            Some(v) if *v == 0 => return Err(PadError::BadSegment),
            Some(v) => sum += *v as u32,
        }
        i += 1;
    }
    if sum == 0 || sum > MAX_PAGES as u32 {
        return Err(PadError::PageOverflow);
    }
    Ok((sum as u16, sections.len()))
}

/// 闭式参考表：由「每节必须整张纸起排」直接算每一条空白面。
///
/// 与 [`SectionPlan::plan`] 的逐步游标是两条推导：
/// * 参考表按节用 `ceil((start + len) / 2)` 一次算到「本节结束占用到第几张纸」；
/// * 计划器逐节推进游标、遇奇数槽补一面。
/// 两者必须逐项同解——同解才说明「顺序推进」与「整张起排」没有一处理解偏差。
pub fn reference_blanks(
    pages: u16,
    sections: &[u16],
    start_on_back: bool,
    manual_sheets: u16,
) -> Result<(Vec<BlankRow>, u16), PadError> {
    let mut rows: Vec<BlankRow> = Vec::with_capacity(sections.len() + 2);
    let mut ordinal: u16 = 0;
    let mut sheet_cursor: u32 = 0;
    if start_on_back {
        rows.push(BlankRow {
            sheet: 0,
            side: Side::Front,
            reason: PadReason::StartOffset,
            anchor_page: 1,
            ordinal,
        });
        ordinal = ordinal.saturating_add(1);
    }
    let mut prefix: u32 = 0;
    let mut i = 0usize;
    while i < sections.len() {
        let len = match sections.get(i) {
            Some(v) => *v as u32,
            None => return Err(PadError::BadSegment),
        };
        // 首节若从背面起印，起点是槽 1；其余节一律从新纸正面（偶数槽）起排。
        let start_slot = if i == 0 && start_on_back {
            1u32
        } else {
            sheet_cursor * 2
        };
        let end_slot = start_slot + len;
        sheet_cursor = (end_slot + 1) / 2;
        prefix += len;
        if end_slot % 2 == 1 {
            let last = i + 1 == sections.len();
            let reason = if last {
                PadReason::TailParity
            } else {
                PadReason::SectionBreak
            };
            let anchor = if last {
                pages
            } else {
                let next_page = prefix + 1;
                if next_page > u16::MAX as u32 {
                    return Err(PadError::PageOverflow);
                }
                next_page as u16
            };
            let sheet = end_slot / 2;
            if sheet > u16::MAX as u32 {
                return Err(PadError::PageOverflow);
            }
            rows.push(BlankRow {
                sheet: sheet as u16,
                side: Side::Back,
                reason,
                anchor_page: anchor,
                ordinal,
            });
            ordinal = ordinal.saturating_add(1);
        }
        i += 1;
    }
    // 手工隔页：整张纸两面都空白，成对登记，锚在作业末页。
    let mut m = 0u32;
    while m < manual_sheets as u32 {
        let sheet = sheet_cursor + m;
        if sheet > u16::MAX as u32 {
            return Err(PadError::PageOverflow);
        }
        rows.push(BlankRow {
            sheet: sheet as u16,
            side: Side::Front,
            reason: PadReason::ManualInsert,
            anchor_page: pages,
            ordinal,
        });
        ordinal = ordinal.saturating_add(1);
        rows.push(BlankRow {
            sheet: sheet as u16,
            side: Side::Back,
            reason: PadReason::ManualInsert,
            anchor_page: pages,
            ordinal,
        });
        ordinal = ordinal.saturating_add(1);
        m += 1;
    }
    let sheets = sheet_cursor + manual_sheets as u32;
    if sheets > u16::MAX as u32 {
        return Err(PadError::PageOverflow);
    }
    Ok((rows, sheets as u16))
}

impl SectionPlan {
    /// 规划：分节必须从新纸正面起排；`start_on_back` 表示首张正面留白续印。
    pub fn plan(sections: &[u16], start_on_back: bool) -> Result<SectionPlan, PadError> {
        let (pages, n) = validate_sections(sections)?;
        let mut blanks = BlankLedger::new(MAX_BLANKS);
        let mut ordinal: u16 = 0;
        let mut cursor: u32 = 0;
        if start_on_back {
            blanks.insert(BlankRow {
                sheet: 0,
                side: Side::Front,
                reason: PadReason::StartOffset,
                anchor_page: 1,
                ordinal,
            })?;
            ordinal = ordinal.saturating_add(1);
            cursor = 1;
        }
        let mut prefix: u32 = 0;
        let mut i = 0usize;
        while i < n {
            let len = match sections.get(i) {
                Some(v) => *v as u32,
                None => return Err(PadError::BadSegment),
            };
            prefix += len;
            cursor += len;
            // 游标落在奇数槽 = 该节停在正面，必须补一面背面才能从新纸起排。
            if cursor % 2 == 1 {
                let sheet = cursor / 2;
                if sheet > u16::MAX as u32 {
                    return Err(PadError::PageOverflow);
                }
                let last = i + 1 == n;
                let reason = if last {
                    PadReason::TailParity
                } else {
                    PadReason::SectionBreak
                };
                let anchor = if last {
                    pages
                } else {
                    let next_page = prefix + 1;
                    if next_page > u16::MAX as u32 {
                        return Err(PadError::PageOverflow);
                    }
                    next_page as u16
                };
                blanks.insert(BlankRow {
                    sheet: sheet as u16,
                    side: Side::Back,
                    reason,
                    anchor_page: anchor,
                    ordinal,
                })?;
                ordinal = ordinal.saturating_add(1);
                cursor += 1;
            }
            i += 1;
        }
        let sheets = cursor / 2;
        if sheets > u16::MAX as u32 {
            return Err(PadError::PageOverflow);
        }
        let plan = SectionPlan {
            pages,
            sections: sections.to_vec(),
            start_on_back,
            sheets: sheets as u16,
            manual_sheets: 0,
            blanks,
        };
        plan.audit()?;
        Ok(plan)
    }

    /// 从已登记的事实重建计划并审计（回放/持久化路径必须重新过审计）。
    pub fn from_parts(
        pages: u16,
        sections: &[u16],
        start_on_back: bool,
        sheets: u16,
        manual_sheets: u16,
        blanks: BlankLedger,
    ) -> Result<SectionPlan, PadError> {
        let plan = SectionPlan {
            pages,
            sections: sections.to_vec(),
            start_on_back,
            sheets,
            manual_sheets,
            blanks,
        };
        plan.audit()?;
        Ok(plan)
    }

    /// 追加一张手工隔页（两面成对登记），随后重新审计。
    pub fn append_manual_sheet(&mut self) -> Result<(), PadError> {
        let sheet = self.sheets;
        let base = self.blanks.len();
        if base > u16::MAX as usize {
            return Err(PadError::PageOverflow);
        }
        let ordinal = base as u16;
        self.blanks.insert(BlankRow {
            sheet,
            side: Side::Front,
            reason: PadReason::ManualInsert,
            anchor_page: self.pages,
            ordinal,
        })?;
        self.blanks.insert(BlankRow {
            sheet,
            side: Side::Back,
            reason: PadReason::ManualInsert,
            anchor_page: self.pages,
            ordinal: ordinal.saturating_add(1),
        })?;
        self.sheets = self.sheets.saturating_add(1);
        self.manual_sheets = self.manual_sheets.saturating_add(1);
        // 隔离页只在整张闭合状态下追加，守恒式必然成立；仍走审计做最终裁决。
        let a = self.audit()?;
        if a.blanks != self.blanks.len() {
            return Err(PadError::SumMismatch);
        }
        Ok(())
    }

    /// 闭式审计：页数守恒、纸数闭式、空白数守恒、逐条与参考表比对。
    pub fn audit(&self) -> Result<Audit, PadError> {
        let (pages, _n) = validate_sections(&self.sections)?;
        if pages != self.pages {
            return Err(PadError::SumMismatch);
        }
        let (ref_rows, ref_sheets) =
            reference_blanks(self.pages, &self.sections, self.start_on_back, self.manual_sheets)?;
        if ref_sheets != self.sheets {
            return Err(PadError::SumMismatch);
        }
        let got = self.blanks.verify()?;
        if got != ref_rows.len() {
            return Err(PadError::SumMismatch);
        }
        if self.pages as u32 + got as u32 != (self.sheets as u32) * 2 {
            return Err(PadError::SumMismatch);
        }
        let mut i = 0usize;
        while i < ref_rows.len() {
            let want = match ref_rows.get(i) {
                Some(v) => *v,
                None => return Err(PadError::Audit),
            };
            let have = match self.blanks.row(i) {
                Some(v) => *v,
                None => return Err(PadError::Audit),
            };
            if have.sheet != want.sheet
                || have.side != want.side
                || have.reason != want.reason
                || have.anchor_page != want.anchor_page
                || have.ordinal != want.ordinal
            {
                return Err(PadError::Audit);
            }
            i += 1;
        }
        let hist = self.blanks.histogram();
        let mut want_hist = [0u16; 4];
        let mut k = 0usize;
        while k < ref_rows.len() {
            if let Some(r) = ref_rows.get(k) {
                if let Some(slot) = want_hist.get_mut(r.reason.slot()) {
                    *slot = slot.saturating_add(1);
                }
            }
            k += 1;
        }
        if hist != want_hist {
            return Err(PadError::Audit);
        }
        // 直方图恰四槽（原因码闭集 1..=4），逐槽取值不越界。
        let offset_blanks = match hist.get(0) {
            Some(v) => *v,
            None => 0,
        };
        let section_blanks = match hist.get(1) {
            Some(v) => *v,
            None => 0,
        };
        let tail_blanks = match hist.get(2) {
            Some(v) => *v,
            None => 0,
        };
        let manual_blanks = match hist.get(3) {
            Some(v) => *v,
            None => 0,
        };
        Ok(Audit {
            pages: self.pages,
            sheets: self.sheets,
            blanks: got,
            offset_blanks,
            section_blanks,
            tail_blanks,
            manual_blanks,
        })
    }

    /// 总页数。
    pub fn pages(&self) -> u16 {
        self.pages
    }

    /// 总纸数。
    pub fn sheets(&self) -> u16 {
        self.sheets
    }

    /// 分节表。
    pub fn sections(&self) -> &[u16] {
        &self.sections
    }

    /// 是否从背面起印。
    pub fn start_on_back(&self) -> bool {
        self.start_on_back
    }

    /// 手工隔页张数。
    pub fn manual_sheets(&self) -> u16 {
        self.manual_sheets
    }

    /// 空白面登记数。
    pub fn blank_count(&self) -> usize {
        self.blanks.len()
    }

    /// 取第 `i` 条空白面。
    pub fn blank(&self, i: usize) -> Option<&BlankRow> {
        self.blanks.row(i)
    }

    /// 按物理键查空白面。
    pub fn find_blank(&self, sheet: u16, side: Side) -> Option<&BlankRow> {
        self.blanks.find(sheet, side)
    }

    /// 计划指纹。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        h = fnv_u16(h, self.pages);
        h = fnv_u16(h, self.sheets);
        h = fnv_byte(h, self.start_on_back as u8);
        h = fnv_u16(h, self.manual_sheets);
        let mut i = 0usize;
        while i < self.sections.len() {
            if let Some(v) = self.sections.get(i) {
                h = fnv_u16(h, *v);
            }
            i += 1;
        }
        fnv_u32_local(h, self.blanks.digest())
    }

    /// 落账文本。
    pub fn ledger_text(&self) -> String {
        self.blanks.text()
    }
}

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u16(h: u32, v: u16) -> u32 {
    fnv_byte(fnv_byte(h, (v >> 8) as u8), v as u8)
}

fn fnv_u32_local(h: u32, v: u32) -> u32 {
    fnv_byte(
        fnv_byte(
            fnv_byte(fnv_byte(h, (v >> 24) as u8), (v >> 16) as u8),
            (v >> 8) as u8,
        ),
        v as u8,
    )
}

/// 注入核：9 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    // 1) 空分节表：没有页就没有补白。
    if SectionPlan::plan(&[], false).err() == Some(PadError::ZeroPages) {
        caught += 1;
    }

    // 2) 零长节：空节没有「从新纸起排」的语义。
    if SectionPlan::plan(&[0], false).err() == Some(PadError::BadSegment) {
        caught += 1;
    }

    // 3) 总页数超限：两节合计越过上限。
    if SectionPlan::plan(&[MAX_PAGES, 1], false).err() == Some(PadError::PageOverflow) {
        caught += 1;
    }

    // 4) 分节数超限。
    let mut many = Vec::with_capacity(MAX_SECTIONS + 1);
    let mut i = 0usize;
    while i <= MAX_SECTIONS {
        many.push(1u16);
        i += 1;
    }
    if SectionPlan::plan(&many, false).err() == Some(PadError::SectionOverflow) {
        caught += 1;
    }

    // 5) 原因码 0 / 9 都不在闭集 1..=4。
    if PadReason::from_code(0).err() == Some(PadError::BadReason)
        && PadReason::from_code(9).err() == Some(PadError::BadReason)
    {
        caught += 1;
    }

    // 6) 锚定页为 0。
    let mut anchor = BlankLedger::new(4);
    let zero_anchor = BlankRow {
        sheet: 0,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 0,
        ordinal: 0,
    };
    if anchor.insert(zero_anchor).err() == Some(PadError::AnchorZero) {
        caught += 1;
    }

    // 7) 同一物理面登记两次。
    let mut twice = BlankLedger::new(4);
    let one = BlankRow {
        sheet: 0,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 3,
        ordinal: 0,
    };
    let two = BlankRow {
        sheet: 0,
        side: Side::Back,
        reason: PadReason::SectionBreak,
        anchor_page: 3,
        ordinal: 1,
    };
    if twice.insert(one).is_ok()
        && twice.insert(two).err() == Some(PadError::SlotConflict)
        && twice.len() == 1
    {
        caught += 1;
    }

    // 8) 序号跳号。
    let mut gap = BlankLedger::new(4);
    let skipped = BlankRow {
        sheet: 1,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 4,
        ordinal: 3,
    };
    if gap.insert(skipped).err() == Some(PadError::OrdinalGap) {
        caught += 1;
    }

    // 9) 封账后登记必拒。
    let mut sealed = BlankLedger::new(4);
    let tail = BlankRow {
        sheet: 0,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 1,
        ordinal: 0,
    };
    let first = sealed.insert(tail).is_ok();
    sealed.seal();
    if first && sealed.insert(tail).err() == Some(PadError::Immutable) && sealed.len() == 1 {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50063 自检：18 条判据（J1 原文：补白 ×10 显式记账）。
pub fn run_unx50063_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50063");

    // 十个代表性分节案：单节/多节、奇偶、起印偏移、空补与满补。
    let cases: [(&[u16], bool); 10] = [
        (&[10], false),
        (&[9], false),
        (&[1], false),
        (&[3, 5], false),
        (&[4, 6], false),
        (&[1, 1, 1], false),
        (&[7, 2], false),
        (&[10], true),
        (&[9], true),
        (&[2, 2, 2, 2], false),
    ];

    // J1-1 十案例显式记账：每个案都审计通过，且每条空白面都有原因+锚+序号。
    let mut explicit_ok = true;
    let mut solved = 0usize;
    let mut total_blanks = 0usize;
    let mut total_pages = 0u32;
    let mut total_sheets = 0u32;
    let mut ci = 0usize;
    while ci < cases.len() {
        if let Some(&(secs, off)) = cases.get(ci) {
            match SectionPlan::plan(secs, off) {
                Ok(p) => match p.audit() {
                    Ok(a) => {
                        solved += 1;
                        total_blanks += a.blanks;
                        total_pages += a.pages as u32;
                        total_sheets += a.sheets as u32;
                        // 空白面守恒：页 + 空白 = 2 × 纸。
                        if a.pages as u32 + a.blanks as u32 != (a.sheets as u32) * 2 {
                            explicit_ok = false;
                        }
                        // 逐条：序号连续、锚在 1..=pages、原因码可反解。
                        let mut k = 0usize;
                        while k < p.blank_count() {
                            match p.blank(k) {
                                Some(r) => {
                                    if r.ordinal as usize != k
                                        || r.anchor_page == 0
                                        || r.anchor_page > a.pages
                                        || PadReason::from_code(r.reason.code()).is_err()
                                    {
                                        explicit_ok = false;
                                    }
                                }
                                None => explicit_ok = false,
                            }
                            k += 1;
                        }
                        // 账本直方图必须覆盖全部空白面（不静默）。
                        let hist = p.blanks.histogram();
                        let sum: u32 = hist.iter().map(|v| *v as u32).sum();
                        if sum != p.blank_count() as u32 {
                            explicit_ok = false;
                        }
                    }
                    Err(_) => explicit_ok = false,
                },
                Err(_) => explicit_ok = false,
            }
        }
        ci += 1;
    }
    if solved != cases.len() || total_blanks == 0 {
        explicit_ok = false;
    }
    let det = format!(
        "cases={} blanks={} p={} s={}",
        solved, total_blanks, total_pages, total_sheets
    );
    s.add_dyn("50063-J1-十案例显式记账", explicit_ok, &det);

    // J1-2 全局守恒：十案例合计 pages + blanks = 2 × sheets。
    let ok = total_pages + total_blanks as u32 == total_sheets * 2;
    let det = format!("{} + {} = {}", total_pages, total_blanks, total_sheets * 2);
    s.add_dyn("50063-J1-合计守恒", ok, &det);

    // J1-3 单节奇偶闭式：1..=20 页，空白数恒等于 pages % 2，纸数 = ceil(pages/2)。
    let mut parity_ok = true;
    let mut pages: u16 = 1;
    while pages <= 20 {
        let secs = [pages];
        match SectionPlan::plan(&secs, false) {
            Ok(p) => {
                let want_blanks = (pages % 2) as usize;
                let want_sheets = (pages + 1) / 2;
                if p.blank_count() != want_blanks || p.sheets() != want_sheets {
                    parity_ok = false;
                }
                if p.pages() as u32 + p.blank_count() as u32 != (p.sheets() as u32) * 2 {
                    parity_ok = false;
                }
            }
            Err(_) => parity_ok = false,
        }
        pages += 1;
    }
    s.add("50063-J1-单节奇偶闭式", parity_ok, if parity_ok { "" } else { "奇偶闭式不符" });

    // J1-4 分节原因归属：[1,1,1] 必须两节 SectionBreak + 一末节 TailParity。
    let mut reason_ok = false;
    let mut reason_det = String::new();
    if let Ok(p) = SectionPlan::plan(&[1, 1, 1], false) {
        if let Ok(a) = p.audit() {
            reason_ok = a.sheets == 3
                && a.blanks == 3
                && a.offset_blanks == 0
                && a.section_blanks == 2
                && a.tail_blanks == 1
                && a.manual_blanks == 0;
            reason_det = format!(
                "off={} sec={} tail={} man={}",
                a.offset_blanks, a.section_blanks, a.tail_blanks, a.manual_blanks
            );
            if p.blank_count() != 3 {
                reason_ok = false;
            }
        }
    }
    s.add_dyn("50063-J1-原因归属", reason_ok, &reason_det);

    // J1-5 锚定页：分节补白锚在下一节首页，末页补白锚在总页数。
    let anchor_ok;
    if let Ok(p) = SectionPlan::plan(&[3, 5], false) {
        // [3,5]：首节停在正面 → 纸 1 背面分节补白（锚 4）；末节 8 页停在正面
        // → 纸 4 背面末页补白（锚 8）。
        match (p.find_blank(1, Side::Back), p.find_blank(4, Side::Back)) {
            (Some(a), Some(b)) => {
                anchor_ok = a.reason == PadReason::SectionBreak
                    && a.anchor_page == 4
                    && b.reason == PadReason::TailParity
                    && b.anchor_page == 8;
            }
            _ => anchor_ok = false,
        }
    } else {
        anchor_ok = false;
    }
    s.add("50063-J1-锚定页", anchor_ok, if anchor_ok { "" } else { "锚页不符" });

    // J1-6 起印偏移：[10] 从背面起印 → 首张正面 StartOffset(锚1) + 末张背面
    // TailParity(锚10)，恰 2 个空白面、6 张纸。
    let mut offset_ok = false;
    let mut offset_det = String::new();
    if let Ok(p) = SectionPlan::plan(&[10], true) {
        match (p.find_blank(0, Side::Front), p.find_blank(5, Side::Back)) {
            (Some(a), Some(b)) => {
                offset_ok = p.sheets() == 6
                    && p.blank_count() == 2
                    && a.reason == PadReason::StartOffset
                    && a.anchor_page == 1
                    && b.reason == PadReason::TailParity
                    && b.anchor_page == 10
                    && p.start_on_back();
                offset_det = format!("sheets={} blanks={}", p.sheets(), p.blank_count());
            }
            _ => offset_ok = false,
        }
    }
    s.add_dyn("50063-J1-起印偏移", offset_ok, &offset_det);

    // J1-7 确定性：同参两次计划指纹逐位相同；换分节结构指纹必变。
    let a = SectionPlan::plan(&[7, 2], false);
    let b = SectionPlan::plan(&[7, 2], false);
    let c = SectionPlan::plan(&[2, 7], false);
    let ok = match (&a, &b, &c) {
        (Ok(x), Ok(y), Ok(z)) => x.digest() == y.digest() && x.digest() != z.digest(),
        _ => false,
    };
    s.add("50063-J1-计划确定", ok, if ok { "" } else { "两次计划不一致" });

    // J1-8 手工隔页：追加整张隔页后纸数+1、空白+2、两面成对、审计仍通过。
    let mut manual_ok = false;
    let mut manual_det = String::new();
    if let Ok(mut p) = SectionPlan::plan(&[10], false) {
        let before = p.digest();
        if p.append_manual_sheet().is_ok() {
            if let Ok(a) = p.audit() {
                let front = p.find_blank(5, Side::Front);
                let back = p.find_blank(5, Side::Back);
                let reason_ok = match (front, back) {
                    (Some(f), Some(bb)) => {
                        f.reason == PadReason::ManualInsert && bb.reason == PadReason::ManualInsert
                    }
                    _ => false,
                };
                manual_ok = a.sheets == 6
                    && a.blanks == 2
                    && a.manual_blanks == 2
                    && p.digest() != before
                    && reason_ok
                    && p.pages() as u32 + p.blank_count() as u32 == (p.sheets() as u32) * 2;
                manual_det = format!("sheets={} blanks={} man={}", a.sheets, a.blanks, a.manual_blanks);
            }
        }
    }
    s.add_dyn("50063-J1-手工隔页", manual_ok, &manual_det);

    // J1-9 账本纪律：重复槽 / 序号缺口 / 容量满 / 封账只读。
    let ledger_ok;
    let mut led = BlankLedger::new(2);
    let r0 = BlankRow {
        sheet: 0,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 1,
        ordinal: 0,
    };
    let r1 = BlankRow {
        sheet: 0,
        side: Side::Back,
        reason: PadReason::SectionBreak,
        anchor_page: 1,
        ordinal: 1,
    };
    let dup = led.insert(r0).is_ok() && led.insert(r1).err() == Some(PadError::SlotConflict);
    let mut gap = BlankLedger::new(2);
    let g = BlankRow {
        sheet: 1,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 2,
        ordinal: 1,
    };
    let gap_ok = gap.insert(g).err() == Some(PadError::OrdinalGap);
    let mut tiny = BlankLedger::new(1);
    let t0 = BlankRow {
        sheet: 0,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 1,
        ordinal: 0,
    };
    let t1 = BlankRow {
        sheet: 1,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 3,
        ordinal: 1,
    };
    let full = tiny.insert(t0).is_ok() && tiny.insert(t1).err() == Some(PadError::Full);
    tiny.seal();
    let imm = tiny.insert(t1).err() == Some(PadError::Immutable);
    ledger_ok = dup && gap_ok && full && imm && tiny.len() == 1 && tiny.is_sealed();
    s.add("50063-J1-账本纪律", ledger_ok, if ledger_ok { "" } else { "重复/跳号/容量/封账未拦截" });

    // J1-10 闭式审计：十个案例的 audit 字段与手推闭式逐项一致。
    let mut audit_ok = true;
    let mut ci2 = 0usize;
    while ci2 < cases.len() {
        if let Some(&(secs, off)) = cases.get(ci2) {
            match SectionPlan::plan(secs, off) {
                Ok(p) => match p.audit() {
                    Ok(a) => {
                        let (ref_rows, ref_sheets) =
                            match reference_blanks(a.pages, secs, off, 0) {
                                Ok(v) => v,
                                Err(_) => {
                                    audit_ok = false;
                                    ci2 += 1;
                                    continue;
                                }
                            };
                        if a.sheets != ref_sheets
                            || a.blanks != ref_rows.len()
                            || a.blanks != p.blank_count()
                        {
                            audit_ok = false;
                        }
                        let hist = p.blanks.histogram();
                        let sum: u32 = hist.iter().map(|v| *v as u32).sum();
                        if sum != ref_rows.len() as u32 {
                            audit_ok = false;
                        }
                    }
                    Err(_) => audit_ok = false,
                },
                Err(_) => audit_ok = false,
            }
        }
        ci2 += 1;
    }
    s.add("50063-J1-闭式审计", audit_ok, if audit_ok { "" } else { "闭式与账本不一致" });

    // J1-11 审计篡改必抓：纸数改错 → SumMismatch；空白面原因改错 → Audit。
    let mut tamper_ok = false;
    if let Ok((rows, sheets)) = reference_blanks(9, &[9], false, 0) {
        let mut good = BlankLedger::new(8);
        let mut i = 0usize;
        while i < rows.len() {
            if let Some(r) = rows.get(i) {
                let _ = good.insert(*r);
            }
            i += 1;
        }
        let wrong_sheets =
            SectionPlan::from_parts(9, &[9], false, sheets + 1, 0, good).err()
                == Some(PadError::SumMismatch);

        let mut bad_reason = BlankLedger::new(8);
        let mut j = 0usize;
        while j < rows.len() {
            if let Some(r) = rows.get(j) {
                let mut m = *r;
                if j == 0 {
                    m.reason = PadReason::ManualInsert;
                }
                let _ = bad_reason.insert(m);
            }
            j += 1;
        }
        let wrong_reason = SectionPlan::from_parts(9, &[9], false, sheets, 0, bad_reason).err()
            == Some(PadError::Audit);
        tamper_ok = wrong_sheets && wrong_reason;
    }
    s.add("50063-J1-审计篡改必抓", tamper_ok, if tamper_ok { "" } else { "篡改未被审计抓住" });

    // J1-12 非法入口六连：零页 / 零长节 / 超页 / 超节 / 非法原因码 / 锚零。
    let mut many = Vec::with_capacity(MAX_SECTIONS + 1);
    let mut i = 0usize;
    while i <= MAX_SECTIONS {
        many.push(1u16);
        i += 1;
    }
    let mut anchor_led = BlankLedger::new(2);
    let zero_anchor = BlankRow {
        sheet: 0,
        side: Side::Back,
        reason: PadReason::TailParity,
        anchor_page: 0,
        ordinal: 0,
    };
    let ok = SectionPlan::plan(&[], false).err() == Some(PadError::ZeroPages)
        && SectionPlan::plan(&[0], false).err() == Some(PadError::BadSegment)
        && SectionPlan::plan(&[MAX_PAGES, 1], false).err() == Some(PadError::PageOverflow)
        && SectionPlan::plan(&many, false).err() == Some(PadError::SectionOverflow)
        && PadReason::from_code(5).err() == Some(PadError::BadReason)
        && anchor_led.insert(zero_anchor).err() == Some(PadError::AnchorZero);
    s.add("50063-J1-非法必拒", ok, if ok { "" } else { "非法入口被接受" });

    // J1-13 原因码往返：1..=4 正解恒等，0/5 拒绝，四类直方图槽位互异。
    let mut code_ok = true;
    let mut code = 1u8;
    while code <= 4 {
        match PadReason::from_code(code) {
            Ok(r) => {
                if r.code() != code || r.slot() != (code - 1) as usize {
                    code_ok = false;
                }
            }
            Err(_) => code_ok = false,
        }
        code += 1;
    }
    if PadReason::from_code(0).is_ok() || PadReason::from_code(5).is_ok() {
        code_ok = false;
    }
    let mut seen_slots = [false; 4];
    let mut ri = 0usize;
    while ri < PadReason::ALL.len() {
        if let Some(r) = PadReason::ALL.get(ri) {
            match seen_slots.get_mut(r.slot()) {
                Some(v) => *v = true,
                None => code_ok = false,
            }
        }
        ri += 1;
    }
    if seen_slots.iter().any(|v| !*v) {
        code_ok = false;
    }
    s.add("50063-J1-原因码闭集", code_ok, if code_ok { "" } else { "原因码不正" });

    // J1-14 空白位序：账本按（纸号, 面序）严格升序，且不出现同槽两行。
    let mut order_ok = true;
    if let Ok(p) = SectionPlan::plan(&[1, 1, 1], false) {
        let mut last_key: i64 = -1;
        let mut i = 0usize;
        while i < p.blank_count() {
            match p.blank(i) {
                Some(r) => {
                    let key = r.key() as i64;
                    if key <= last_key {
                        order_ok = false;
                    }
                    last_key = key;
                }
                None => order_ok = false,
            }
            i += 1;
        }
    } else {
        order_ok = false;
    }
    s.add("50063-J1-空白位序", order_ok, if order_ok { "" } else { "空白面乱序或同槽" });

    // J1-15 页数守恒（sweep）：1..=30 页单节，累加页与空白恒等于 2×纸。
    let mut sweep_ok = true;
    let mut n: u16 = 1;
    while n <= 30 {
        let secs = [n];
        match SectionPlan::plan(&secs, false) {
            Ok(p) => {
                let blanks = p.blank_count() as u32;
                if p.pages() as u32 + blanks != (p.sheets() as u32) * 2 {
                    sweep_ok = false;
                }
                if blanks > 1 || blanks != (n % 2) as u32 {
                    sweep_ok = false;
                }
            }
            Err(_) => sweep_ok = false,
        }
        n += 1;
    }
    s.add("50063-J1-页数守恒sweep", sweep_ok, if sweep_ok { "" } else { "sweep 守恒破坏" });

    // J1-16 计划不可变事实：同一计划重复 audit 结果逐位相同（幂等）。
    let mut idem_ok = false;
    if let Ok(p) = SectionPlan::plan(&[5, 4, 7], false) {
        let a1 = p.audit();
        let a2 = p.audit();
        // [5,4,7]：5 结尾补分节空白、4 偶数不补、7 结尾补末页空白 → 恰 2 条。
        idem_ok = a1.is_ok() && a1 == a2 && p.blank_count() == 2;
    }
    s.add("50063-J1-审计幂等", idem_ok, if idem_ok { "" } else { "两次审计不一致" });

    // J1-17 空账合法：零空白计划（偶数页单节）账本为空且 verify 通过。
    let empty_ok = match SectionPlan::plan(&[8], false) {
        Ok(p) => p.blank_count() == 0 && p.sheets() == 4 && p.audit().is_ok(),
        Err(_) => false,
    };
    s.add("50063-J1-无补白合法", empty_ok, if empty_ok { "" } else { "偶数页被强行补白" });

    // J1-18 九类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50063-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50063_checks_green() {
        let set = run_unx50063_checks();
        assert!(set.all_passed(), "UNX-F50063 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50063_odd_pages_register_tail_blank() {
        for n in 1..=12u16 {
            let plan = SectionPlan::plan(&[n], false);
            assert!(plan.is_ok(), "n={}", n);
            if let Ok(p) = plan {
                let a = p.audit();
                assert!(a.is_ok(), "n={} audit", n);
                if let Ok(x) = a {
                    assert_eq!(x.blanks, (n % 2) as usize, "n={}", n);
                    if n % 2 == 1 {
                        let last = p.find_blank((n - 1) / 2, Side::Back);
                        assert!(last.is_some());
                        if let Some(r) = last {
                            assert_eq!(r.reason, PadReason::TailParity);
                            assert_eq!(r.anchor_page, n);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn unx50063_sections_start_on_front() {
        let plan = SectionPlan::plan(&[1, 1, 1], false);
        assert!(plan.is_ok());
        if let Ok(p) = plan {
            assert_eq!(p.sheets(), 3);
            assert_eq!(p.blank_count(), 3);
            let hist = p.blanks.histogram();
            assert_eq!(hist, [0, 2, 1, 0]);
            for i in 0..3usize {
                if let Some(r) = p.blank(i) {
                    assert_eq!(r.side, Side::Back);
                    assert_eq!(r.reason.code(), if i + 1 == 3 { 3 } else { 2 });
                }
            }
        }
    }

    #[test]
    fn unx50063_manual_sheet_is_registered_explicitly() {
        let plan = SectionPlan::plan(&[4], false);
        assert!(plan.is_ok());
        if let Ok(mut p) = plan {
            assert_eq!(p.sheets(), 2);
            assert_eq!(p.blank_count(), 0);
            assert!(p.append_manual_sheet().is_ok());
            assert_eq!(p.sheets(), 3);
            assert_eq!(p.blank_count(), 2);
            assert_eq!(p.audit().map(|a| a.manual_blanks), Ok(2));
        }
    }

    #[test]
    fn unx50063_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 9);
    }
}
