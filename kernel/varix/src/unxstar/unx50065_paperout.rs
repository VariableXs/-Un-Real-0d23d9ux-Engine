//! UNX-F50065 缺纸中断 / 换纸续印语义（0x43440 段）
//!
//! 锚点：UNX-F50065-J1 换纸 ×10 续印页序一致；判据锚定 §AI-63 §2 专题五。
//!
//! 数据段：`SEG_BASE = 0x43440`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 15`，
//! 逐项见 [`PaperError`]。
//!
//! 缺纸中断最容易被写成「报个错、等人塞纸、接着打」，但真正决定成品是否
//! 作废的是**续印点**与**页序**：
//! 1. 中断粒度必须是「整张纸」而不是「页」。双面作业里一张纸承载前后两页，
//!    若在正面打完、反面进纸时缺纸，续印必须从这张纸的反面继续；若把中断
//!    记成「第 N 页」，换纸后就会从下一张纸重新开始，成品中间被抽掉一页；
//! 2. 续印点必须等于中断点。换纸（refill）只能把机器从 Suspended 拉回
//!    Feeding，绝不能移动纸张游标；否则同一张纸会被重复进纸或整张丢失；
//! 3. 页序必须逐页、无重、无漏。10 次换纸后，页账本应恰好等于 0..N-1，
//!    任何一次「续印多退一页 / 少打一页」都会在账本里留下重复或空洞。
//!
//! 因此本模块用两层账本表达语义：[`PageLedger`] 逐页记账（页号必须严格
//! 连续，纸号由页号与单/双面推导），[`RefillLedger`] 逐次换纸记账（续印点
//! 必须等于中断点，尝试号严格递增）。J1 让 40 页双面作业在奇数纸号处连续
//! 缺纸 10 次，逐次塞纸，最后与独立参考页序逐页对拍。
//!
//! 结构清单：
//! * [`FeedRun`]：进纸状态机（Idle → Feeding → Suspended → Feeding → Done）；
//! * [`PageEvent`] / [`PageLedger`]：页级账本（连续页号 + 纸号/正反面推导）；
//! * [`RefillRecord`] / [`RefillLedger`]：换纸账本（续印点锚定 + 尝试号递增）；
//! * [`JobReport`]：一次完整作业的状态机 + 两本账；
//! * [`reference_pages`] / [`reference_sheet`]：独立参考实现（闭式推导，不复用账本逻辑）；
//! * [`injection_core`]：8 类坏样本注入核。
//!
//! 纸路约定：纸盒号 1..=8；0 表示无有效来源，构造作业时直接拒绝。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43440 独占 0x20）。
pub const SEG_BASE: u32 = 0x43440;

/// 单作业页数上限（A3 机身双面 256 张，留一倍余量）。
pub const MAX_PAGES: u16 = 512;

/// J1 作业页数：双面 40 页 = 20 张纸。
pub const J1_PAGES: u16 = 40;

/// J1 换纸次数。
pub const J1_REFILLS: usize = 10;

/// 单作业允许的最大换纸次数（防止无限塞纸循环）。
pub const MAX_REFILLS: u8 = 32;

/// 纸盒号上限。
pub const MAX_TRAY: u8 = 8;

const E_ZERO: u16 = 1;
const E_OVERFLOW: u16 = 2;
const E_PARITY: u16 = 3;
const E_PHASE: u16 = 4;
const E_PAPER_OUT: u16 = 5;
const E_TRAY: u16 = 6;
const E_NOT_SUSPENDED: u16 = 7;
const E_SKEW: u16 = 8;
const E_DUP: u16 = 9;
const E_ORDER: u16 = 10;
const E_FULL: u16 = 11;
const E_IMMUTABLE: u16 = 12;
const E_AUDIT: u16 = 13;
const E_BAD_ATTEMPT: u16 = 14;
const E_STOP_RANGE: u16 = 15;

/// 缺纸 / 续印错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperError {
    /// 作业页数为零。
    ZeroPages,
    /// 页数超过 [`MAX_PAGES`]。
    PageOverflow,
    /// 双面作业页数为奇数（一张纸装不下半页）。
    Parity,
    /// 当前状态不允许该操作。
    Phase,
    /// 进纸时检测到纸尽，作业转入 Suspended。
    PaperOut,
    /// 换纸使用了与作业不同的纸盒。
    TrayMismatch,
    /// 非 Suspended 状态尝试换纸。
    NotSuspended,
    /// 续印点与中断点不一致（或纸号与页号推导不一致）。
    ResumeSkew,
    /// 页/换纸记录重复。
    Duplicate,
    /// 页/换纸记录次序断裂。
    OrderGap,
    /// 账本容量已满。
    Full,
    /// 封账后只读。
    Immutable,
    /// 账本复核失败（未封账 / 正反面不符）。
    Audit,
    /// 换纸尝试号为零或纸盒号越界。
    BadAttempt,
    /// 计划中的中断纸号超出作业范围。
    StopRange,
}

impl PaperError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PaperError::ZeroPages => E_ZERO,
            PaperError::PageOverflow => E_OVERFLOW,
            PaperError::Parity => E_PARITY,
            PaperError::Phase => E_PHASE,
            PaperError::PaperOut => E_PAPER_OUT,
            PaperError::TrayMismatch => E_TRAY,
            PaperError::NotSuspended => E_NOT_SUSPENDED,
            PaperError::ResumeSkew => E_SKEW,
            PaperError::Duplicate => E_DUP,
            PaperError::OrderGap => E_ORDER,
            PaperError::Full => E_FULL,
            PaperError::Immutable => E_IMMUTABLE,
            PaperError::Audit => E_AUDIT,
            PaperError::BadAttempt => E_BAD_ATTEMPT,
            PaperError::StopRange => E_STOP_RANGE,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PaperError::ZeroPages => "job has zero pages",
            PaperError::PageOverflow => "job pages exceed limit",
            PaperError::Parity => "duplex job has odd page count",
            PaperError::Phase => "operation not allowed in current phase",
            PaperError::PaperOut => "paper exhausted while feeding",
            PaperError::TrayMismatch => "refill tray differs from job tray",
            PaperError::NotSuspended => "refill attempted while not suspended",
            PaperError::ResumeSkew => "resume point differs from stop point",
            PaperError::Duplicate => "duplicate page or refill record",
            PaperError::OrderGap => "records are not in feed order",
            PaperError::Full => "ledger capacity exhausted",
            PaperError::Immutable => "ledger is sealed",
            PaperError::Audit => "ledger audit failed",
            PaperError::BadAttempt => "refill attempt or tray id invalid",
            PaperError::StopRange => "planned stop sheet outside job",
        }
    }
}

/// 页所在面：单面作业恒为 Front，双面作业偶数页 Front、奇数页 Back。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// 纸张正面。
    Front,
    /// 纸张反面（仅双面）。
    Back,
}

impl Side {
    /// 稳定码值（0 正 / 1 反）。
    pub fn code(self) -> u8 {
        match self {
            Side::Front => 0,
            Side::Back => 1,
        }
    }

    /// 一行标签。
    pub fn label(self) -> &'static str {
        match self {
            Side::Front => "front",
            Side::Back => "back",
        }
    }
}

/// 进纸状态机的四个阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 作业已装载，尚未允许进纸。
    Idle,
    /// 可以进纸。
    Feeding,
    /// 纸尽，等待换纸。
    Suspended,
    /// 作业完成。
    Done,
}

/// 一次进纸产生的单页记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageEvent {
    /// 页号（0 起）。
    pub page: u16,
    /// 承载该页的纸号（0 起）。
    pub sheet: u16,
    /// 正反面。
    pub side: Side,
}

impl PageEvent {
    /// 构造一条页记录。
    pub fn new(page: u16, sheet: u16, side: Side) -> Self {
        PageEvent { page, sheet, side }
    }

    /// 页记录键（页号 << 8 | 面码），用于指纹与比较。
    pub fn key(&self) -> u32 {
        (self.page as u32) << 8 | self.side.code() as u32
    }

    /// 一行文本。
    pub fn line(&self) -> String {
        format!("p{}/s{}/{}", self.page, self.sheet, self.side.label())
    }
}

/// 换纸记录：中断点与续印点必须同纸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefillRecord {
    /// 缺纸发生的纸号。
    pub stop_sheet: u16,
    /// 换纸后重新开始进纸的纸号。
    pub resume_sheet: u16,
    /// 换纸纸盒。
    pub tray: u8,
    /// 第几次换纸（1 起）。
    pub attempt: u8,
}

impl RefillRecord {
    /// 构造一条换纸记录。
    pub fn new(stop_sheet: u16, resume_sheet: u16, tray: u8, attempt: u8) -> Self {
        RefillRecord {
            stop_sheet,
            resume_sheet,
            tray,
            attempt,
        }
    }

    /// 一行文本。
    pub fn line(&self) -> String {
        format!(
            "a{}/stop{}/resume{}/tray{}",
            self.attempt, self.stop_sheet, self.resume_sheet, self.tray
        )
    }
}

/// 进纸状态机。
///
/// 纸尽中断的粒度是「整张纸」：`feed(false)` 在打印该纸任何一页之前就
/// 转入 Suspended，因此续印时同一纸号从头进纸不会重复已出页。
pub struct FeedRun {
    pages: u16,
    duplex: bool,
    tray: u8,
    phase: Phase,
    next_sheet: u16,
    printed: u16,
    refills: u8,
    stop_sheet: Option<u16>,
}

impl FeedRun {
    /// 装载作业：校验页数、奇偶与纸盒号，初始为 Idle。
    pub fn start(pages: u16, duplex: bool, tray: u8) -> Result<Self, PaperError> {
        if pages == 0 {
            return Err(PaperError::ZeroPages);
        }
        if pages > MAX_PAGES {
            return Err(PaperError::PageOverflow);
        }
        if duplex && pages % 2 != 0 {
            return Err(PaperError::Parity);
        }
        if tray == 0 || tray > MAX_TRAY {
            return Err(PaperError::BadAttempt);
        }
        Ok(FeedRun {
            pages,
            duplex,
            tray,
            phase: Phase::Idle,
            next_sheet: 0,
            printed: 0,
            refills: 0,
            stop_sheet: None,
        })
    }

    /// 从 Idle 进入可进纸状态。
    pub fn begin(&mut self) -> Result<(), PaperError> {
        if self.phase != Phase::Idle {
            return Err(PaperError::Phase);
        }
        self.phase = Phase::Feeding;
        Ok(())
    }

    /// 每张纸承载的页数（双面 2 / 单面 1）。
    pub fn per_sheet(&self) -> u16 {
        if self.duplex {
            2
        } else {
            1
        }
    }

    /// 作业总纸数。
    pub fn sheet_count(&self) -> u16 {
        self.pages / self.per_sheet()
    }

    /// 当前状态。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 下一张待进纸张号；Done 后等于总纸数。
    pub fn next_sheet(&self) -> u16 {
        self.next_sheet
    }

    /// 已出页数。
    pub fn printed(&self) -> u16 {
        self.printed
    }

    /// 已换纸次数。
    pub fn refills(&self) -> u8 {
        self.refills
    }

    /// 最近一次中断纸号。
    pub fn stop_sheet(&self) -> Option<u16> {
        self.stop_sheet
    }

    /// 作业页数。
    pub fn pages(&self) -> u16 {
        self.pages
    }

    /// 是否双面。
    pub fn duplex(&self) -> bool {
        self.duplex
    }

    /// 作业纸盒。
    pub fn tray(&self) -> u8 {
        self.tray
    }

    /// 尝试进一张纸：无纸则转入 Suspended 并报 PaperOut，否则整张纸出页。
    pub fn feed(&mut self, has_paper: bool) -> Result<Vec<PageEvent>, PaperError> {
        if self.phase != Phase::Feeding {
            return Err(PaperError::Phase);
        }
        if !has_paper {
            self.phase = Phase::Suspended;
            self.stop_sheet = Some(self.next_sheet);
            return Err(PaperError::PaperOut);
        }
        let per = self.per_sheet();
        let base = self.next_sheet.saturating_mul(per);
        let mut out = Vec::with_capacity(per as usize);
        let mut k = 0u16;
        while k < per {
            let page = base.saturating_add(k);
            if page >= self.pages {
                return Err(PaperError::Audit);
            }
            let side = if k == 0 { Side::Front } else { Side::Back };
            out.push(PageEvent::new(page, self.next_sheet, side));
            k += 1;
        }
        self.printed = self.printed.saturating_add(per);
        self.next_sheet = self.next_sheet.saturating_add(1);
        if self.next_sheet >= self.sheet_count() {
            self.phase = Phase::Done;
        }
        Ok(out)
    }

    /// 换纸：仅 Suspended 且纸盒一致时允许，返回本次尝试号（1 起）。
    pub fn refill(&mut self, tray: u8) -> Result<u8, PaperError> {
        if self.phase != Phase::Suspended {
            return Err(PaperError::NotSuspended);
        }
        if tray != self.tray {
            return Err(PaperError::TrayMismatch);
        }
        if self.refills >= MAX_REFILLS {
            return Err(PaperError::Full);
        }
        self.refills = self.refills.saturating_add(1);
        self.phase = Phase::Feeding;
        Ok(self.refills)
    }
}

/// 页级账本：页号必须严格连续，纸号/正反面由页号与单双面推导。
pub struct PageLedger {
    entries: Vec<PageEvent>,
    cap: usize,
    per_sheet: u16,
    sealed: bool,
}

impl PageLedger {
    /// 新建页账本（容量 + 每纸页数）。
    pub fn new(cap: usize, per_sheet: u16) -> Self {
        PageLedger {
            entries: Vec::with_capacity(cap),
            cap,
            per_sheet,
            sealed: false,
        }
    }

    /// 入账一页：重复、跳号、纸号错位、正反面错位分别给码。
    pub fn insert(&mut self, ev: PageEvent) -> Result<(), PaperError> {
        if self.sealed {
            return Err(PaperError::Immutable);
        }
        if self.entries.len() >= self.cap {
            return Err(PaperError::Full);
        }
        let expected = self.entries.len() as u16;
        if ev.page < expected {
            return Err(PaperError::Duplicate);
        }
        if ev.page > expected {
            return Err(PaperError::OrderGap);
        }
        if self.per_sheet == 0 {
            return Err(PaperError::Audit);
        }
        if ev.sheet != reference_sheet(ev.page, self.per_sheet) {
            return Err(PaperError::ResumeSkew);
        }
        if ev.side != reference_side(ev.page, self.per_sheet) {
            return Err(PaperError::Audit);
        }
        self.entries.push(ev);
        Ok(())
    }

    /// 封账：之后只读。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 账本条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按序取页记录。
    pub fn get(&self, i: usize) -> Option<&PageEvent> {
        self.entries.get(i)
    }

    /// 全账复核：封账、页数、逐页页号/纸号/面一致。
    pub fn verify(&self, pages: u16) -> Result<usize, PaperError> {
        if !self.sealed {
            return Err(PaperError::Audit);
        }
        if self.entries.len() != pages as usize {
            return Err(PaperError::Audit);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let ev = match self.entries.get(i) {
                Some(v) => *v,
                None => return Err(PaperError::Audit),
            };
            if ev.page != i as u16 {
                return Err(PaperError::OrderGap);
            }
            if ev.sheet != reference_sheet(ev.page, self.per_sheet) {
                return Err(PaperError::ResumeSkew);
            }
            if ev.side != reference_side(ev.page, self.per_sheet) {
                return Err(PaperError::Audit);
            }
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 页账指纹（FNV-1a，跨运行确定）。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        h = fnv_u32(h, self.per_sheet as u32);
        h = fnv_u32(h, self.entries.len() as u32);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(ev) = self.entries.get(i) {
                h = fnv_u32(h, ev.key());
                h = fnv_u32(h, ev.sheet as u32);
            }
            i += 1;
        }
        h
    }

    /// 多行文本（诊断/日志用）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(ev) = self.entries.get(i) {
                s.push_str(&ev.line());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 换纸账本：续印点必须等于中断点，尝试号严格递增。
pub struct RefillLedger {
    records: Vec<RefillRecord>,
    cap: usize,
    sealed: bool,
}

impl RefillLedger {
    /// 新建换纸账本。
    pub fn new(cap: usize) -> Self {
        RefillLedger {
            records: Vec::with_capacity(cap),
            cap,
            sealed: false,
        }
    }

    /// 入账一次换纸：零尝试号、续印错位、重复尝试分别给码。
    pub fn insert(&mut self, rec: RefillRecord) -> Result<(), PaperError> {
        if self.sealed {
            return Err(PaperError::Immutable);
        }
        if self.records.len() >= self.cap {
            return Err(PaperError::Full);
        }
        if rec.attempt == 0 {
            return Err(PaperError::BadAttempt);
        }
        if rec.resume_sheet != rec.stop_sheet {
            return Err(PaperError::ResumeSkew);
        }
        if rec.tray == 0 || rec.tray > MAX_TRAY {
            return Err(PaperError::BadAttempt);
        }
        if let Some(prev) = self.records.last() {
            if rec.attempt <= prev.attempt {
                return Err(PaperError::OrderGap);
            }
        }
        self.records.push(rec);
        Ok(())
    }

    /// 封账：之后只读。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 账本条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 按序取换纸记录。
    pub fn get(&self, i: usize) -> Option<&RefillRecord> {
        self.records.get(i)
    }

    /// 全账复核：封账、条数、逐次中断纸号/续印点/尝试号一致。
    pub fn verify(&self, stops: &[u16]) -> Result<usize, PaperError> {
        if !self.sealed {
            return Err(PaperError::Audit);
        }
        if self.records.len() != stops.len() {
            return Err(PaperError::Audit);
        }
        let mut i = 0usize;
        while i < self.records.len() {
            let rec = match self.records.get(i) {
                Some(v) => *v,
                None => return Err(PaperError::Audit),
            };
            if rec.stop_sheet != stops[i] {
                return Err(PaperError::OrderGap);
            }
            if rec.resume_sheet != rec.stop_sheet {
                return Err(PaperError::ResumeSkew);
            }
            if rec.attempt != (i as u8).saturating_add(1) {
                return Err(PaperError::OrderGap);
            }
            i += 1;
        }
        Ok(self.records.len())
    }

    /// 换纸账指纹。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        h = fnv_u32(h, self.records.len() as u32);
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(rec) = self.records.get(i) {
                h = fnv_u32(h, rec.stop_sheet as u32);
                h = fnv_u32(h, rec.resume_sheet as u32);
                h = fnv_u32(h, rec.tray as u32);
                h = fnv_u32(h, rec.attempt as u32);
            }
            i += 1;
        }
        h
    }

    /// 多行文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(rec) = self.records.get(i) {
                s.push_str(&rec.line());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 一次完整作业的结果：状态机 + 页账 + 换纸账。
pub struct JobReport {
    /// 最终状态机。
    pub run: FeedRun,
    /// 页账本（已封账）。
    pub pages: PageLedger,
    /// 换纸账本（已封账）。
    pub refills: RefillLedger,
}

/// 独立参考：纸号 = 页号 / 每纸页数（闭式，不经过账本）。
pub fn reference_sheet(page: u16, per_sheet: u16) -> u16 {
    if per_sheet == 0 {
        0
    } else {
        page / per_sheet
    }
}

/// 独立参考：双面偶数页正面、奇数页反面；单面恒为正。
pub fn reference_side(page: u16, per_sheet: u16) -> Side {
    if per_sheet == 2 && page % 2 == 1 {
        Side::Back
    } else {
        Side::Front
    }
}

/// 独立参考页序：0..pages（逐页一次）。
pub fn reference_pages(pages: u16) -> Vec<u16> {
    let mut out = Vec::with_capacity(pages as usize);
    let mut i = 0u16;
    while i < pages {
        out.push(i);
        i += 1;
    }
    out
}

/// J1 换纸计划：双面 20 张纸，在奇数纸号 1..19 处各缺纸一次，共 10 次。
pub fn refill_stops() -> Vec<u16> {
    let mut out = Vec::with_capacity(J1_REFILLS);
    let mut i = 0u16;
    while (i as usize) < J1_REFILLS {
        out.push(i.saturating_mul(2).saturating_add(1));
        i += 1;
    }
    out
}

/// 跑完整作业：按计划在指定纸号前抽走纸张，换纸后从同一纸号续印。
pub fn run_job(
    pages: u16,
    duplex: bool,
    tray: u8,
    stops: &[u16],
) -> Result<JobReport, PaperError> {
    let mut run = FeedRun::start(pages, duplex, tray)?;
    run.begin()?;
    let sheets = run.sheet_count();
    let mut k = 0usize;
    while k < stops.len() {
        let s = stops[k];
        if s >= sheets {
            return Err(PaperError::StopRange);
        }
        if k > 0 && stops[k - 1] > s {
            return Err(PaperError::OrderGap);
        }
        k += 1;
    }
    let mut ledger = PageLedger::new(MAX_PAGES as usize, run.per_sheet());
    let mut refills = RefillLedger::new(MAX_REFILLS as usize);
    let mut stop_i = 0usize;
    while run.phase() != Phase::Done {
        let sheet = run.next_sheet();
        // 同一张纸可能连续两次塞纸仍未装上：允许重复中断同一纸号。
        while stop_i < stops.len() && stops[stop_i] == sheet {
            if run.feed(false).err() != Some(PaperError::PaperOut) {
                return Err(PaperError::Audit);
            }
            if run.stop_sheet() != Some(sheet) {
                return Err(PaperError::ResumeSkew);
            }
            let attempt = run.refill(tray)?;
            refills.insert(RefillRecord::new(sheet, sheet, tray, attempt))?;
            stop_i += 1;
        }
        let events = run.feed(true)?;
        let mut e = 0usize;
        while e < events.len() {
            if let Some(ev) = events.get(e) {
                ledger.insert(*ev)?;
            }
            e += 1;
        }
    }
    if stop_i != stops.len() {
        return Err(PaperError::Audit);
    }
    ledger.seal();
    refills.seal();
    Ok(JobReport {
        run,
        pages: ledger,
        refills,
    })
}

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_u32(mut h: u32, v: u32) -> u32 {
    h ^= v;
    h = h.wrapping_mul(FNV_PRIME);
    h
}

/// 8 类坏样本注入核：每类都必须命中各自的诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1) 零页 / 奇数双面：装载阶段拒绝。
    if FeedRun::start(0, false, 1).err() == Some(PaperError::ZeroPages)
        && FeedRun::start(3, true, 1).err() == Some(PaperError::Parity)
    {
        caught += 1;
    }

    // 2) 页数上限。
    if FeedRun::start(MAX_PAGES.saturating_add(1), false, 1).err()
        == Some(PaperError::PageOverflow)
    {
        caught += 1;
    }

    // 3) 状态机：Idle 进纸 / Done 后进纸都给 Phase。
    let mut m = match FeedRun::start(4, true, 2) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let idle_feed = m.feed(true).err() == Some(PaperError::Phase);
    if m.begin().is_ok() && m.feed(false).err() == Some(PaperError::PaperOut) {
        let stop_ok = m.stop_sheet() == Some(0) && m.phase() == Phase::Suspended;
        if idle_feed && stop_ok {
            caught += 1;
        }
    }

    // 4) 换纸错误：未悬挂换纸 / 错盒换纸。
    let mut n = match FeedRun::start(4, true, 2) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let not_susp = n.refill(2).err() == Some(PaperError::NotSuspended);
    let _ = n.begin();
    let _ = n.feed(false);
    let wrong_tray = n.refill(3).err() == Some(PaperError::TrayMismatch);
    if not_susp && wrong_tray {
        caught += 1;
    }

    // 5) 页账：跳号 / 重复。
    let mut pl = PageLedger::new(8, 2);
    let gap = pl.insert(PageEvent::new(1, 0, Side::Back)).err()
        == Some(PaperError::OrderGap);
    let dup = pl.insert(PageEvent::new(0, 0, Side::Front)).is_ok()
        && pl.insert(PageEvent::new(0, 0, Side::Front)).err() == Some(PaperError::Duplicate);
    if gap && dup {
        caught += 1;
    }

    // 6) 页账：纸号错位 / 正反面错位。
    let mut pl2 = PageLedger::new(8, 2);
    let skew = pl2.insert(PageEvent::new(0, 1, Side::Front)).err()
        == Some(PaperError::ResumeSkew);
    let mut pl3 = PageLedger::new(8, 2);
    let side = pl3.insert(PageEvent::new(0, 0, Side::Back)).err() == Some(PaperError::Audit);
    if skew && side {
        caught += 1;
    }

    // 7) 换纸账：零尝试号 / 续印错位。
    let mut rl = RefillLedger::new(8);
    let zero = rl.insert(RefillRecord::new(1, 1, 2, 0)).err()
        == Some(PaperError::BadAttempt);
    let mis = rl.insert(RefillRecord::new(1, 2, 2, 1)).err()
        == Some(PaperError::ResumeSkew);
    if zero && mis {
        caught += 1;
    }

    // 8) 换纸账：尝试号不递增 / 封账后写入。
    let mut rl2 = RefillLedger::new(8);
    let ok_first = rl2.insert(RefillRecord::new(1, 1, 2, 1)).is_ok();
    let order = rl2.insert(RefillRecord::new(3, 3, 2, 1)).err()
        == Some(PaperError::OrderGap);
    rl2.seal();
    let imm = rl2.insert(RefillRecord::new(5, 5, 2, 2)).err()
        == Some(PaperError::Immutable);
    if ok_first && order && imm {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50065 自检：18 条判据（J1 原文：换纸 ×10 续印页序一致）。
pub fn run_unx50065_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50065");
    let stops = refill_stops();
    let job = run_job(J1_PAGES, true, 2, &stops);

    // J1-1 十次换纸后逐页对拍：页号必须恰为 0..39、每页一次。
    let mut order_ok = false;
    let mut detail = String::new();
    if let Ok(rep) = &job {
        let mut exact = rep.pages.len() == J1_PAGES as usize;
        let mut i = 0usize;
        while i < rep.pages.len() {
            if let Some(ev) = rep.pages.get(i) {
                if ev.page != i as u16 {
                    exact = false;
                }
            } else {
                exact = false;
            }
            i += 1;
        }
        order_ok = exact && rep.run.refills() as usize == J1_REFILLS;
        detail = format!(
            "pages={} refills={}",
            rep.pages.len(),
            rep.run.refills()
        );
    }
    s.add_dyn("50065-J1-换纸十次续印", order_ok, &detail);

    // J1-2 续印点一致：每次换纸的中断纸号与续印纸号必须相同。
    let mut anchor_ok = false;
    if let Ok(rep) = &job {
        anchor_ok = rep.refills.verify(&stops).is_ok() && rep.refills.len() == J1_REFILLS;
        if anchor_ok {
            let mut i = 0usize;
            while i < rep.refills.len() {
                if let Some(rec) = rep.refills.get(i) {
                    if rec.stop_sheet != stops[i] || rec.resume_sheet != rec.stop_sheet {
                        anchor_ok = false;
                    }
                }
                i += 1;
            }
        }
    }
    s.add(
        "50065-J1-续印点一致",
        anchor_ok,
        if anchor_ok { "" } else { "续印点偏离中断点" },
    );

    // J1-3 独立参考页序：闭式 0..39 与账本逐页相等。
    let mut ref_ok = false;
    if let Ok(rep) = &job {
        let want = reference_pages(J1_PAGES);
        ref_ok = want.len() == rep.pages.len();
        let mut i = 0usize;
        while i < want.len() {
            if let (Some(w), Some(g)) = (want.get(i), rep.pages.get(i)) {
                if *w != g.page {
                    ref_ok = false;
                }
            } else {
                ref_ok = false;
            }
            i += 1;
        }
    }
    s.add(
        "50065-J1-页序独立对拍",
        ref_ok,
        if ref_ok { "" } else { "页序与闭式参考不符" },
    );

    // J1-4 单面 17 页：换纸 2 次，页序仍为 0..16，纸号等于页号。
    let simplex_stops = [3u16, 8u16];
    let simplex = run_job(17, false, 1, &simplex_stops);
    let mut simplex_ok = false;
    if let Ok(rep) = &simplex {
        let mut exact = rep.pages.len() == 17 && rep.run.refills() == 2;
        let mut i = 0usize;
        while i < rep.pages.len() {
            if let Some(ev) = rep.pages.get(i) {
                if ev.page != i as u16 || ev.sheet != i as u16 || ev.side != Side::Front {
                    exact = false;
                }
            } else {
                exact = false;
            }
            i += 1;
        }
        simplex_ok = exact && rep.pages.verify(17).is_ok() && rep.refills.verify(&simplex_stops).is_ok();
    }
    s.add(
        "50065-J1-单面十七页",
        simplex_ok,
        if simplex_ok { "" } else { "单面续印页序错" },
    );

    // J1-5 同一纸号连续两次缺纸：两次换纸都不得移动游标，页序仍完整。
    let twice = [2u16, 2u16];
    let repeated = run_job(8, true, 1, &twice);
    let mut twice_ok = false;
    if let Ok(rep) = &repeated {
        twice_ok = rep.run.refills() == 2
            && rep.pages.len() == 8
            && rep.pages.verify(8).is_ok()
            && rep.refills.verify(&twice).is_ok();
    }
    s.add(
        "50065-J1-重停同页",
        twice_ok,
        if twice_ok { "" } else { "同页重停后续印错位" },
    );

    // J1-6 双页邻接：每张纸的两页必须相邻（2s, 2s+1）且一出俱出。
    let mut pair_ok = false;
    if let Ok(rep) = &job {
        pair_ok = rep.pages.len() == J1_PAGES as usize;
        let mut sheet = 0u16;
        while sheet < rep.run.sheet_count() {
            let a = rep.pages.get(sheet as usize * 2);
            let b = rep.pages.get(sheet as usize * 2 + 1);
            match (a, b) {
                (Some(x), Some(y)) => {
                    if x.page != sheet * 2
                        || y.page != sheet * 2 + 1
                        || x.sheet != sheet
                        || y.sheet != sheet
                        || x.side != Side::Front
                        || y.side != Side::Back
                    {
                        pair_ok = false;
                    }
                }
                _ => pair_ok = false,
            }
            sheet += 1;
        }
    }
    s.add(
        "50065-J1-双页邻接",
        pair_ok,
        if pair_ok { "" } else { "双面同纸页不邻接" },
    );

    // J1-7 状态机：Idle 不得进纸，Suspended 不得直接进纸，Done 后拒绝。
    let mut phase_ok = false;
    if let Ok(mut m) = FeedRun::start(4, true, 1) {
        let idle = m.feed(true).err() == Some(PaperError::Phase);
        let begun = m.begin().is_ok();
        let out = m.feed(false).err() == Some(PaperError::PaperOut);
        let during = m.feed(true).err() == Some(PaperError::Phase);
        let refill = m.refill(1).is_ok();
        let mut steps = 0u16;
        while steps < 2 {
            if m.feed(true).is_err() {
                steps = 99;
            }
            steps += 1;
        }
        let done = m.phase() == Phase::Done && steps == 2;
        let after = m.feed(true).err() == Some(PaperError::Phase);
        phase_ok = idle && begun && out && during && refill && done && after;
    }
    s.add(
        "50065-J1-状态机",
        phase_ok,
        if phase_ok { "" } else { "状态迁移不符合纸路" },
    );

    // J1-8 换纸计数：尝试号必须恰为 1..10，与计划次数相等。
    let mut count_ok = false;
    if let Ok(rep) = &job {
        count_ok = rep.run.refills() as usize == J1_REFILLS
            && rep.refills.len() == J1_REFILLS;
        let mut i = 0usize;
        while i < rep.refills.len() {
            if let Some(rec) = rep.refills.get(i) {
                if rec.attempt != (i as u8).saturating_add(1) || rec.tray != 2 {
                    count_ok = false;
                }
            }
            i += 1;
        }
    }
    s.add(
        "50065-J1-换纸计数",
        count_ok,
        if count_ok { "" } else { "换纸尝试号断裂" },
    );

    // J1-9 页账复核：封账后 verify 通过，且指纹可复现。
    let again = run_job(J1_PAGES, true, 2, &stops);
    let mut audit_ok = false;
    if let (Ok(a), Ok(b)) = (&job, &again) {
        audit_ok = a.pages.verify(J1_PAGES).is_ok()
            && a.refills.verify(&stops).is_ok()
            && a.pages.digest() == b.pages.digest()
            && a.refills.digest() == b.refills.digest()
            && a.pages.is_sealed()
            && a.refills.is_sealed()
            && !a.pages.text().is_empty();
    }
    s.add(
        "50065-J1-账本复核",
        audit_ok,
        if audit_ok { "" } else { "封账/指纹不可复现" },
    );

    // J1-10 页账纪律：跳号 / 重复 / 容量 / 封账各自给码。
    let mut page_disc = false;
    if let Ok(rep) = &job {
        page_disc = rep.pages.verify(J1_PAGES).is_ok()
            && rep.refills.verify(&stops).is_ok();
    }
    let mut pl = PageLedger::new(2, 2);
    let gap = pl.insert(PageEvent::new(3, 1, Side::Back)).err()
        == Some(PaperError::OrderGap);
    let first = pl.insert(PageEvent::new(0, 0, Side::Front)).is_ok()
        && pl.insert(PageEvent::new(1, 0, Side::Back)).is_ok();
    let full = pl.insert(PageEvent::new(2, 1, Side::Front)).err() == Some(PaperError::Full);
    pl.seal();
    let imm = pl.insert(PageEvent::new(2, 1, Side::Front)).err()
        == Some(PaperError::Immutable);
    s.add(
        "50065-J1-页账纪律",
        page_disc && gap && first && full && imm,
        if page_disc && gap && first && full && imm {
            ""
        } else {
            "页账纪律缺口"
        },
    );

    // J1-11 换纸账纪律：零尝试 / 错位 / 不递增 / 封账各自给码。
    let mut rl = RefillLedger::new(2);
    let zero = rl.insert(RefillRecord::new(0, 0, 1, 0)).err()
        == Some(PaperError::BadAttempt);
    let skew = rl.insert(RefillRecord::new(0, 1, 1, 1)).err()
        == Some(PaperError::ResumeSkew);
    let ok1 = rl.insert(RefillRecord::new(0, 0, 1, 1)).is_ok();
    let ord = rl.insert(RefillRecord::new(2, 2, 1, 1)).err()
        == Some(PaperError::OrderGap);
    let ok2 = rl.insert(RefillRecord::new(2, 2, 1, 2)).is_ok();
    let full = rl.insert(RefillRecord::new(4, 4, 1, 3)).err() == Some(PaperError::Full);
    rl.seal();
    let imm = rl.insert(RefillRecord::new(4, 4, 1, 3)).err()
        == Some(PaperError::Immutable);
    let disc = zero && skew && ok1 && ord && ok2 && full && imm;
    s.add(
        "50065-J1-续账纪律",
        disc,
        if disc { "" } else { "换纸账纪律缺口" },
    );

    // J1-12 诊断码闭集：15 个变体码值连续、互异、基址正确。
    let all = [
        PaperError::ZeroPages,
        PaperError::PageOverflow,
        PaperError::Parity,
        PaperError::Phase,
        PaperError::PaperOut,
        PaperError::TrayMismatch,
        PaperError::NotSuspended,
        PaperError::ResumeSkew,
        PaperError::Duplicate,
        PaperError::OrderGap,
        PaperError::Full,
        PaperError::Immutable,
        PaperError::Audit,
        PaperError::BadAttempt,
        PaperError::StopRange,
    ];
    let mut codes_ok = all.len() == 15;
    let mut i = 0usize;
    while i < all.len() {
        if all[i].code() != SEG_BASE + (i as u32) + 1 || all[i].message().is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < all.len() {
            if all[j].code() == all[i].code() {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "50065-J1-诊断码闭集",
        codes_ok,
        if codes_ok { "n=15" } else { "码值不连续/重复" },
    );

    // J1-13 坏样本注入：8 类逐类命中。
    let (caught, total) = injection_core();
    s.add(
        "50065-J1-坏样本注入",
        caught == total && total == 8,
        if caught == total { "8/8" } else { "注入未全命中" },
    );

    // J1-14 装载边界：零页 / 上限 / 奇偶 / 纸盒号。
    let mut load_ok = FeedRun::start(0, false, 1).err() == Some(PaperError::ZeroPages)
        && FeedRun::start(MAX_PAGES.saturating_add(1), false, 1).err()
            == Some(PaperError::PageOverflow)
        && FeedRun::start(5, true, 1).err() == Some(PaperError::Parity)
        && FeedRun::start(4, true, 0).err() == Some(PaperError::BadAttempt)
        && FeedRun::start(4, true, MAX_TRAY.saturating_add(1)).err()
            == Some(PaperError::BadAttempt);
    if let Ok(m) = FeedRun::start(MAX_PAGES, true, 8) {
        load_ok = load_ok && m.sheet_count() == MAX_PAGES / 2 && m.per_sheet() == 2;
    } else {
        load_ok = false;
    }
    s.add(
        "50065-J1-装载边界",
        load_ok,
        if load_ok { "" } else { "装载校验缺口" },
    );

    // J1-15 换纸错误：未悬挂换纸 / 错盒换纸 / 换满上限。
    let mut refill_ok = false;
    if let Ok(mut m) = FeedRun::start(6, false, 3) {
        let before = m.refill(3).err() == Some(PaperError::NotSuspended);
        let _ = m.begin();
        let _ = m.feed(false);
        let wrong = m.refill(4).err() == Some(PaperError::TrayMismatch);
        let right = m.refill(3).is_ok();
        m.refills = MAX_REFILLS;
        let _ = m.feed(false);
        let full = m.refill(3).err() == Some(PaperError::Full);
        refill_ok = before && wrong && right && full;
    }
    s.add(
        "50065-J1-换纸错误",
        refill_ok,
        if refill_ok { "" } else { "换纸校验缺口" },
    );

    // J1-16 计划越界：中断纸号 >= 总纸数必须拒绝，逆序必须拒绝。
    let out = run_job(10, true, 1, &[5u16]);
    let rev = run_job(10, true, 1, &[4u16, 2u16]);
    let range_ok = out.err() == Some(PaperError::StopRange)
        && rev.err() == Some(PaperError::OrderGap)
        && run_job(10, true, 1, &[4u16, 4u16]).is_ok();
    s.add(
        "50065-J1-计划边界",
        range_ok,
        if range_ok { "" } else { "中断计划越界/逆序未拒" },
    );

    // J1-17 页数守恒：printed == pages == sheets × per_sheet。
    let mut sum_ok = false;
    if let Ok(rep) = &job {
        sum_ok = rep.run.printed() == J1_PAGES
            && rep.pages.len() == J1_PAGES as usize
            && rep.run.sheet_count() * rep.run.per_sheet() == J1_PAGES
            && rep.run.next_sheet() == rep.run.sheet_count();
    }
    s.add(
        "50065-J1-页数守恒",
        sum_ok,
        if sum_ok { "" } else { "页数/纸数不闭合" },
    );

    // J1-18 末纸中断：在最后一张纸缺纸一次，换纸后必须仍能完成且页序完整。
    let last_stop = [9u16];
    let tail = run_job(20, true, 1, &last_stop);
    let mut tail_ok = false;
    if let Ok(rep) = &tail {
        tail_ok = rep.run.phase() == Phase::Done
            && rep.pages.verify(20).is_ok()
            && rep.refills.verify(&last_stop).is_ok()
            && rep.run.refills() == 1;
    }
    s.add(
        "50065-J1-末纸中断",
        tail_ok,
        if tail_ok { "" } else { "末纸中断未能完成" },
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50065_ten_refills_preserve_order() {
        let stops = refill_stops();
        let rep = run_job(J1_PAGES, true, 2, &stops);
        assert!(rep.is_ok());
        if let Ok(r) = rep {
            assert_eq!(r.run.refills() as usize, J1_REFILLS);
            assert_eq!(r.pages.len(), J1_PAGES as usize);
            assert_eq!(r.pages.verify(J1_PAGES), Ok(J1_PAGES as usize));
            assert_eq!(r.refills.verify(&stops), Ok(J1_REFILLS));
            let mut i = 0usize;
            while i < J1_PAGES as usize {
                if let Some(ev) = r.pages.get(i) {
                    assert_eq!(ev.page, i as u16);
                    assert_eq!(ev.sheet, (i as u16) / 2);
                }
                i += 1;
            }
        }
    }

    #[test]
    fn unx50065_simplex_resume_keeps_sheet_cursor() {
        let rep = run_job(17, false, 1, &[3u16, 8u16]);
        assert!(rep.is_ok());
        if let Ok(r) = rep {
            assert_eq!(r.pages.verify(17), Ok(17));
            assert_eq!(r.refills.verify(&[3u16, 8u16]), Ok(2));
            if let Some(ev) = r.pages.get(3) {
                assert_eq!(ev.page, 3);
                assert_eq!(ev.sheet, 3);
            }
        }
    }

    #[test]
    fn unx50065_repeated_stop_on_same_sheet() {
        let rep = run_job(8, true, 1, &[2u16, 2u16]);
        assert!(rep.is_ok());
        if let Ok(r) = rep {
            assert_eq!(r.run.refills(), 2);
            assert_eq!(r.pages.verify(8), Ok(8));
            assert_eq!(r.refills.verify(&[2u16, 2u16]), Ok(2));
        }
    }

    #[test]
    fn unx50065_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 8);
    }
}
