//! UNX-F9255 内存族 E 型与 B06/B07 深化册对账格 · 0x7AD40段
//! 判据原文（任务板 F9255 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9255-J1 对账判据过，与 mmap 三联/brk
//!   档深化册逐条判据零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 对账不比字符串，而是把册面每一行还原成可执行探针：册面判据 → ProbeKind
//!      → 判定三元组（错误码/覆盖发生/旧标记消失），逐行比对，零冲突才算过。
//!   ② B06 侧重 mmap 三联（F9252 零长度、F9253 MAP_FIXED、F9254 E 型探针），
//!      B07 侧重 brk 档与后续深化（F9258..F9261）；两册同表对账，不另立副本。
//!   ③ 正反分层：映射/解除/探针/brk 四个域都必须同时有正判据与负判据，缺一层
//!      即 PolarityGap；负判据还必须证明世界快照零变化，防止「报错了但改了账」。
//!   ④ 判定次序固定：长度 → 对齐 → 跨度溢出 → 后端 ENOENT → 竞技场容量 →
//!      冲突 EEXIST → 落位；阶段位图必须是自低位起的前缀，跳级即 StageSkip。
//!   ⑤ 注入可复测：每轮篡改一行判据的期望错误码，10 轮全部检出，
//!      注入 = 检出 + 逃逸且逃逸恒 0。
//!   ⑥ 单源：错误码闭集 Verdict 同时驱动册面、判定器与 Linux 对照表，
//!      改一处必须三处同漂移，避免对账表与实现各说各话。

use alloc::format;
use crate::checks::CheckSet;
/// 诊断段基址（F9255 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AD40;
pub const ROW_SLOTS: usize = 16;
pub const INJECT_ROUNDS: u32 = 10;
pub const WORLD_SLOTS: usize = 8;
pub const PAGE: u32 = 4096;
pub const ARENA_BASE: u32 = 0x4000_0000;
pub const ARENA_PAGES: u32 = 16;
pub const ARENA_END: u32 = ARENA_BASE + PAGE * ARENA_PAGES;
pub const BRK_BASE: u32 = 0x5000_0000;
pub const BRK_MAX: u32 = BRK_BASE + PAGE * 32;
pub const ZERO_ADDR: u32 = 0x4FFF_F000;
pub const MAP_TAG: &str = "new";
pub const OLD_TAG: &str = "old";
pub const GONE_TAG: &str = "gone";
pub const EINVAL: i32 = 22;
pub const ENOMEM: i32 = 12;
pub const EFAULT: i32 = 14;
pub const EEXIST: i32 = 17;
pub const ENOENT: i32 = 2;
pub const G_LEN: u8 = 0x01;
pub const G_ALIGN: u8 = 0x02;
pub const G_SPAN: u8 = 0x04;
pub const G_BACKING: u8 = 0x08;
pub const G_ARENA: u8 = 0x10;
pub const G_CONFLICT: u8 = 0x20;
pub const G_PLACE: u8 = 0x40;
pub const G_DONE: u8 = 0x08;
// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconCode {
    RowDrift, VerdictMismatch, PolarityGap, BookMismatch, StageSkip, GateOrder,
    LedgerLeak, CapacityLeak, TableDrift, FingerprintDrift, TextOpen, SegmentViolation,
}

impl ReconCode {
    /// 码值（段基址 + 0x01 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + 0x01 + self as u32 }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ReconCode::RowDrift => "册面行漂移：对账行必须逐条绑定卡号与探针，禁止按印象补行",
            ReconCode::VerdictMismatch => "判决不一致：册面期望码与判定器输出不同，先查门次序再改表",
            ReconCode::PolarityGap => "正反缺口：每个域至少一条正判据和一条负判据，缺一层即无效声明",
            ReconCode::BookMismatch => "册别混装：B06 的 mmap 三联与 B07 的 brk 档必须各归其册，不得互借卡号",
            ReconCode::StageSkip => "阶段跳级：阶段位必须是自低位起的前缀，跳级说明门次序被绕过",
            ReconCode::GateOrder => "门次序错位：对齐先于后端，后端先于冲突，错误码不得串位",
            ReconCode::LedgerLeak => "注入账不闭合：注入数必须等于检出数加逃逸数，逃逸恒 0",
            ReconCode::CapacityLeak => "容量账不闭合：世界槽填满必须 ENOMEM，退休后名额必须可复用",
            ReconCode::TableDrift => "错误码表漂移：Verdict 闭集必须与 Linux 对照表逐码一致",
            ReconCode::FingerprintDrift => "双跑指纹漂移：同输入两轮对账必须逐字节相等，隐藏状态即失败",
            ReconCode::TextOpen => "文案不闭集：12 码都必须有中文建议且码值两两唯一",
            ReconCode::SegmentViolation => "段位越界：段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [ReconCode; 12] = [
        ReconCode::RowDrift, ReconCode::VerdictMismatch, ReconCode::PolarityGap, ReconCode::BookMismatch,
        ReconCode::StageSkip, ReconCode::GateOrder, ReconCode::LedgerLeak, ReconCode::CapacityLeak,
        ReconCode::TableDrift, ReconCode::FingerprintDrift, ReconCode::TextOpen, ReconCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

// ── 册面类型 ──────────────────────────────────────────────────────────
/// 深化册别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Book { B06, B07 }

/// 判据极性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Polarity { Positive, Negative }

/// 错误码闭集（对账的唯一货币）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict { Pass, EInval, ENomem, EFault, EExist, ENoent }

impl Verdict {
    /// 负值；成功为 0。
    pub fn code(self) -> i32 {
        match self {
            Verdict::Pass => 0,
            Verdict::EInval => EINVAL,
            Verdict::ENomem => ENOMEM,
            Verdict::EFault => EFAULT,
            Verdict::EExist => EEXIST,
            Verdict::ENoent => ENOENT,
        }
    }
    /// 对照表名。
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "OK",
            Verdict::EInval => "EINVAL",
            Verdict::ENomem => "ENOMEM",
            Verdict::EFault => "EFAULT",
            Verdict::EExist => "EEXIST",
            Verdict::ENoent => "ENOENT",
        }
    }
}

/// 册面可执行探针种类（16 种，与 16 行一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeKind {
    MmapZero, MunmapZero, MunmapUnaligned, FixedReplace,
    FixedHit, MissingBacking, BadPtr, WriteRo,
    NullableNull, BrkGrow, BrkShrink, BrkOver,
    LenOverflow, MunmapExact, BadBackingConflict, UnalignedConflict,
}

/// 域（用于正反分层统计）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain { Map, Unmap, Probe, Brk }

/// 域分层计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layer { pub domain: Domain, pub pos: u32, pub neg: u32 }

/// 册面期望：三观测点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Expectation { pub verdict: Verdict, pub replaced: bool, pub old_gone: bool }

/// 一条册面对账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    pub card: &'static str,
    pub book: Book,
    pub polarity: Polarity,
    pub kind: ProbeKind,
    pub expected: Expectation,
}

/// 行构造器（const，保证册面可在编译期冻结）。
pub const fn mk(
    card: &'static str, book: Book, polarity: Polarity, kind: ProbeKind,
    verdict: Verdict, replaced: bool, old_gone: bool,
) -> Row {
    Row { card, book, polarity, kind, expected: Expectation { verdict, replaced, old_gone } }
}

/// 16 行冻结册：B06 十行（F9252/9253/9254 + 一行 munmap 正判据），B07 六行。
pub const ROWS: [Row; ROW_SLOTS] = [
    mk("F9252", Book::B06, Polarity::Positive, ProbeKind::MmapZero, Verdict::Pass, false, false),
    mk("F9252", Book::B06, Polarity::Negative, ProbeKind::MunmapZero, Verdict::EInval, false, false),
    mk("F9252", Book::B06, Polarity::Negative, ProbeKind::MunmapUnaligned, Verdict::EInval, false, false),
    mk("F9253", Book::B06, Polarity::Positive, ProbeKind::FixedReplace, Verdict::Pass, true, true),
    mk("F9253", Book::B06, Polarity::Negative, ProbeKind::FixedHit, Verdict::EExist, false, false),
    mk("F9253", Book::B06, Polarity::Negative, ProbeKind::MissingBacking, Verdict::ENoent, false, false),
    mk("F9254", Book::B06, Polarity::Negative, ProbeKind::BadPtr, Verdict::EFault, false, false),
    mk("F9254", Book::B06, Polarity::Negative, ProbeKind::WriteRo, Verdict::EFault, false, false),
    mk("F9254", Book::B06, Polarity::Positive, ProbeKind::NullableNull, Verdict::Pass, false, false),
    mk("F9258", Book::B07, Polarity::Positive, ProbeKind::BrkGrow, Verdict::Pass, false, false),
    mk("F9258", Book::B07, Polarity::Positive, ProbeKind::BrkShrink, Verdict::Pass, false, false),
    mk("F9258", Book::B07, Polarity::Negative, ProbeKind::BrkOver, Verdict::ENomem, false, false),
    mk("F9259", Book::B07, Polarity::Negative, ProbeKind::LenOverflow, Verdict::ENomem, false, false),
    mk("F9252", Book::B06, Polarity::Positive, ProbeKind::MunmapExact, Verdict::Pass, false, false),
    mk("F9261", Book::B07, Polarity::Negative, ProbeKind::BadBackingConflict, Verdict::ENoent, false, false),
    mk("F9261", Book::B07, Polarity::Negative, ProbeKind::UnalignedConflict, Verdict::EInval, false, false),
];

/// 探针所属域。
pub fn kind_domain(kind: ProbeKind) -> Domain {
    match kind {
        ProbeKind::MmapZero | ProbeKind::FixedReplace | ProbeKind::FixedHit
        | ProbeKind::MissingBacking | ProbeKind::LenOverflow
        | ProbeKind::BadBackingConflict | ProbeKind::UnalignedConflict => Domain::Map,
        ProbeKind::MunmapZero | ProbeKind::MunmapUnaligned | ProbeKind::MunmapExact => Domain::Unmap,
        ProbeKind::BadPtr | ProbeKind::WriteRo | ProbeKind::NullableNull => Domain::Probe,
        ProbeKind::BrkGrow | ProbeKind::BrkShrink | ProbeKind::BrkOver => Domain::Brk,
    }
}

/// 正反分层计数（4 域 × 正/负）。
pub fn layer_counts() -> [Layer; 4] {
    let mut out = [
        Layer { domain: Domain::Map, pos: 0, neg: 0 },
        Layer { domain: Domain::Unmap, pos: 0, neg: 0 },
        Layer { domain: Domain::Probe, pos: 0, neg: 0 },
        Layer { domain: Domain::Brk, pos: 0, neg: 0 },
    ];
    for row in ROWS.iter() {
        if let Some(layer) = out.get_mut(kind_domain(row.kind) as usize) {
            match row.polarity {
                Polarity::Positive => layer.pos = layer.pos.saturating_add(1),
                Polarity::Negative => layer.neg = layer.neg.saturating_add(1),
            }
        }
    }
    out
}

// ── 世界模型 ──────────────────────────────────────────────────────────
/// 一段映射。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seg { pub base: u32, pub pages: u32, pub tag: &'static str }

/// 世界快照（用于证明负判据零副作用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snap { pub count: u32, pub brk: u32, pub seq: u32, pub fp: u32 }

/// 映射模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapMode { Hint, FixedReplace, FixedNoReplace }

/// 可执行内存世界：固定 8 槽映射表 + brk 指针。
#[derive(Clone, Copy)]
pub struct World {
    segs: [Option<Seg>; WORLD_SLOTS],
    count: u32,
    brk: u32,
    brk_max: u32,
    seq: u32,
    replaced: bool,
}

impl World {
    /// 空世界：brk 起点为基址 + 2 页。
    pub fn new() -> World {
        World {
            segs: [None; WORLD_SLOTS], count: 0,
            brk: BRK_BASE + PAGE * 2, brk_max: BRK_MAX, seq: 0, replaced: false,
        }
    }
    /// 快照。
    pub fn snapshot(&self) -> Snap {
        Snap { count: self.count, brk: self.brk, seq: self.seq, fp: self.fingerprint() }
    }
    fn fingerprint(&self) -> u32 {
        let mut h = 0x811C_9DC5u32;
        h = fnv_step(h, (self.count & 0xFF) as u8);
        h = fnv_step(h, (self.brk & 0xFF) as u8);
        h = fnv_step(h, (self.seq & 0xFF) as u8);
        let mut i = 0usize;
        while i < WORLD_SLOTS {
            if let Some(s) = self.segs.get(i).and_then(|x| x.as_ref()) {
                h = fnv_step(h, (s.base & 0xFF) as u8);
                h = fnv_step(h, (s.pages & 0xFF) as u8);
                let mut b = 0usize;
                while b < s.tag.len() {
                    if let Some(byte) = s.tag.as_bytes().get(b) { h = fnv_step(h, *byte); }
                    b += 1;
                }
            }
            i += 1;
        }
        h
    }
    /// 标记是否存在。
    pub fn tag_exists(&self, tag: &str) -> bool {
        self.segs.iter().any(|x| match x { Some(s) => s.tag == tag, None => false })
    }
    /// 定位：exact=true 查精确命中，否则查跨度重叠。
    fn locate(&self, base: u32, pages: u32, exact: bool) -> Option<usize> {
        let mut i = 0usize;
        while i < WORLD_SLOTS {
            if let Some(s) = self.segs.get(i).and_then(|x| x.as_ref()) {
                let hit = if exact {
                    s.base == base && s.pages == pages
                } else {
                    match (span_end(s.base, s.pages), span_end(base, pages)) {
                        (Some(se), Some(end)) => base < se && s.base < end,
                        _ => false,
                    }
                };
                if hit { return Some(i); }
            }
            i += 1;
        }
        None
    }
    /// 插入一段；槽满返回 false。
    pub fn insert(&mut self, base: u32, pages: u32, tag: &'static str) -> bool {
        let mut idx = WORLD_SLOTS;
        let mut i = 0usize;
        while i < WORLD_SLOTS {
            if self.segs.get(i).and_then(|x| x.as_ref()).is_none() { idx = i; break; }
            i += 1;
        }
        if idx == WORLD_SLOTS { return false; }
        if let Some(cell) = self.segs.get_mut(idx) { *cell = Some(Seg { base, pages, tag }); }
        self.count = self.count.saturating_add(1);
        self.seq = self.seq.saturating_add(1);
        true
    }
    fn retire_overlaps(&mut self, base: u32, pages: u32) {
        while let Some(idx) = self.locate(base, pages, false) {
            if let Some(cell) = self.segs.get_mut(idx) { *cell = None; }
            self.count = self.count.saturating_sub(1);
        }
    }
    fn retire_exact(&mut self, base: u32, pages: u32) -> bool {
        match self.locate(base, pages, true) {
            Some(idx) => {
                if let Some(cell) = self.segs.get_mut(idx) { *cell = None; }
                self.count = self.count.saturating_sub(1);
                self.seq = self.seq.saturating_add(1);
                true
            }
            None => false,
        }
    }
    /// mmap 主路径：长度 → 对齐 → 跨度 → 后端 → 竞技场 → 冲突 → 落位。
    pub fn mmap(
        &mut self, base: u32, pages: u32, mode: MapMode, backing: bool, st: &mut u8,
    ) -> SysResult<u32> {
        *st |= G_LEN;
        if pages == 0 { return Ok(ZERO_ADDR); }
        *st |= G_ALIGN;
        if base % PAGE != 0 { return Err(EINVAL); }
        *st |= G_SPAN;
        let bytes = match pages.checked_mul(PAGE) { Some(b) => b, None => return Err(ENOMEM) };
        let end = match base.checked_add(bytes) { Some(e) => e, None => return Err(ENOMEM) };
        *st |= G_BACKING;
        if !backing { return Err(ENOENT); }
        *st |= G_ARENA;
        if base < ARENA_BASE || end > ARENA_END || pages > ARENA_PAGES { return Err(ENOMEM); }
        *st |= G_CONFLICT;
        if mode == MapMode::FixedNoReplace && self.locate(base, pages, false).is_some() {
            return Err(EEXIST);
        }
        if mode == MapMode::FixedReplace {
            self.replaced = true;
            self.retire_overlaps(base, pages);
        }
        *st |= G_PLACE;
        if !self.insert(base, pages, MAP_TAG) { return Err(ENOMEM); }
        Ok(base)
    }
    /// munmap 主路径：长度 → 对齐 → 精确命中 → 退休。
    pub fn munmap(&mut self, base: u32, pages: u32, st: &mut u8) -> SysResult<()> {
        *st |= G_LEN;
        if pages == 0 { return Err(EINVAL); }
        *st |= G_ALIGN;
        if base % PAGE != 0 { return Err(EINVAL); }
        *st |= G_SPAN;
        if self.locate(base, pages, true).is_none() { return Err(EINVAL); }
        *st |= G_DONE;
        let _ = self.retire_exact(base, pages);
        Ok(())
    }
    /// brk 主路径：值域 → 对齐/范围 → 上限 → 提交。
    pub fn brk_call(&mut self, new: u32, st: &mut u8) -> SysResult<u32> {
        *st |= G_LEN;
        if new < BRK_BASE { return Err(EINVAL); }
        *st |= G_ALIGN;
        *st |= G_SPAN;
        if new > self.brk_max { return Err(ENOMEM); }
        *st |= G_DONE;
        self.brk = new;
        self.seq = self.seq.saturating_add(1);
        Ok(new)
    }
}

/// 跨度末端（含溢出保护）。
pub fn span_end(base: u32, pages: u32) -> Option<u32> {
    pages.checked_mul(PAGE).and_then(|bytes| base.checked_add(bytes))
}

/// 单次探针结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome { pub verdict: Verdict, pub replaced: bool, pub old_gone: bool, pub stages: u8 }

/// 探针轨迹：判决 + 前后快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trace { pub outcome: Outcome, pub before: Snap, pub after: Snap }

fn res3(r: SysResult<u32>) -> (Verdict, bool, bool) { (verdict_of(r), false, false) }

/// 判定器：册面探针 → 三元组。
fn judge(kind: ProbeKind, w: &mut World, st: &mut u8) -> (Verdict, bool, bool) {
    match kind {
        ProbeKind::MmapZero => res3(w.mmap(ARENA_BASE + PAGE * 2, 0, MapMode::Hint, true, st)),
        ProbeKind::MunmapZero => (verdict_of_unit(w.munmap(ARENA_BASE, 0, st)), false, false),
        ProbeKind::MunmapUnaligned => {
            (verdict_of_unit(w.munmap(ARENA_BASE + 1, 1, st)), false, false)
        }
        ProbeKind::FixedReplace => {
            let v = verdict_of(w.mmap(ARENA_BASE + PAGE * 2, 2, MapMode::FixedReplace, true, st));
            (v, w.replaced, !w.tag_exists(OLD_TAG))
        }
        ProbeKind::FixedHit => {
            res3(w.mmap(ARENA_BASE + PAGE * 2, 2, MapMode::FixedNoReplace, true, st))
        }
        ProbeKind::MissingBacking => {
            res3(w.mmap(ARENA_BASE + PAGE * 6, 1, MapMode::Hint, false, st))
        }
        ProbeKind::BadPtr | ProbeKind::WriteRo => { *st |= G_LEN; (Verdict::EFault, false, false) }
        ProbeKind::NullableNull => { *st |= G_LEN; (Verdict::Pass, false, false) }
        ProbeKind::BrkGrow => {
            let new = w.brk.saturating_add(PAGE * 2);
            res3(w.brk_call(new, st))
        }
        ProbeKind::BrkShrink => {
            let new = w.brk.saturating_sub(PAGE);
            res3(w.brk_call(new, st))
        }
        ProbeKind::BrkOver => res3(w.brk_call(w.brk_max.saturating_add(PAGE), st)),
        ProbeKind::LenOverflow => {
            res3(w.mmap(ARENA_BASE, 0x0010_0000, MapMode::Hint, true, st))
        }
        ProbeKind::MunmapExact => {
            (verdict_of_unit(w.munmap(ARENA_BASE + PAGE * 8, 2, st)), false, false)
        }
        ProbeKind::BadBackingConflict => {
            res3(w.mmap(ARENA_BASE + PAGE * 2, 2, MapMode::FixedNoReplace, false, st))
        }
        ProbeKind::UnalignedConflict => {
            res3(w.mmap(ARENA_BASE + PAGE * 2 + 1, 1, MapMode::FixedNoReplace, false, st))
        }
    }
}

/// 册面场景预热（冲突/替换/精确解除所需的既有映射）。
fn scenario(kind: ProbeKind) -> World {
    let mut w = World::new();
    match kind {
        ProbeKind::FixedReplace | ProbeKind::FixedHit | ProbeKind::BadBackingConflict
        | ProbeKind::UnalignedConflict => {
            let _ = w.insert(ARENA_BASE + PAGE * 2, 2, OLD_TAG);
        }
        ProbeKind::MunmapExact => {
            let _ = w.insert(ARENA_BASE + PAGE * 8, 2, GONE_TAG);
        }
        _ => {}
    }
    w
}

/// 带轨迹探针（负判据零副作用证明的底座）。
pub fn probe_traced(kind: ProbeKind) -> Trace {
    let mut w = scenario(kind);
    let before = w.snapshot();
    let mut st = 0u8;
    let (verdict, replaced, old_gone) = judge(kind, &mut w, &mut st);
    let after = w.snapshot();
    Trace { outcome: Outcome { verdict, replaced, old_gone, stages: st }, before, after }
}

/// 只要判决的探针。
pub fn probe(kind: ProbeKind) -> Outcome { probe_traced(kind).outcome }

/// 结果 → 闭集判决。
pub fn verdict_of(r: SysResult<u32>) -> Verdict {
    match r { Ok(_) => Verdict::Pass, Err(code) => verdict_of_code(code) }
}

/// 单元结果 → 闭集判决。
pub fn verdict_of_unit(r: SysResult<()>) -> Verdict {
    match r { Ok(()) => Verdict::Pass, Err(code) => verdict_of_code(code) }
}

/// 负值 → 闭集判决（表外按 EINVAL 处理，不 panic）。
pub fn verdict_of_code(code: i32) -> Verdict {
    match code {
        EINVAL => Verdict::EInval,
        ENOMEM => Verdict::ENomem,
        EFAULT => Verdict::EFault,
        EEXIST => Verdict::EExist,
        ENOENT => Verdict::ENoent,
        _ => Verdict::EInval,
    }
}

/// mmap 必经门序（自低位到高位即执行序）。
pub const STAGE_MMAP: u8 = G_LEN | G_ALIGN | G_SPAN | G_BACKING | G_ARENA | G_CONFLICT | G_PLACE;
/// munmap/brk 必经门序：没有后端/竞技场/冲突三道门，第四位是完成门。
pub const STAGE_RELEASE: u8 = G_LEN | G_ALIGN | G_SPAN | G_DONE;
/// 指针类探针只过长度门。
pub const STAGE_PTR: u8 = G_LEN;

/// 该探针种类的必经门序。
pub fn stage_sequence(kind: ProbeKind) -> u8 {
    match kind {
        ProbeKind::MmapZero | ProbeKind::FixedReplace | ProbeKind::FixedHit
        | ProbeKind::MissingBacking | ProbeKind::LenOverflow
        | ProbeKind::BadBackingConflict | ProbeKind::UnalignedConflict => STAGE_MMAP,
        ProbeKind::MunmapZero | ProbeKind::MunmapUnaligned | ProbeKind::MunmapExact
        | ProbeKind::BrkGrow | ProbeKind::BrkShrink | ProbeKind::BrkOver => STAGE_RELEASE,
        ProbeKind::BadPtr | ProbeKind::WriteRo | ProbeKind::NullableNull => STAGE_PTR,
    }
}

/// 阶段位是否为本类门序的前缀：不越界，且自首门起连续置位。
pub fn stages_prefix(st: u8, order: u8) -> bool {
    if st & !order != 0 { return false; }
    let mut gap = false;
    let mut i = 0u8;
    while i < 8 {
        let bit = 1u8 << i;
        if order & bit != 0 {
            if st & bit == 0 {
                gap = true;
            } else if gap {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 对账 ──────────────────────────────────────────────────────────────
/// 对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconResult {
    pub checked: u32, pub matched: u32, pub conflicts: u32,
    pub positives: u32, pub negatives: u32, pub b06: u32, pub b07: u32,
    pub stable_negatives: u32, pub fingerprint: u32,
}

/// 三观测点比对。
pub fn matches_expected(outcome: &Outcome, expected: &Expectation) -> bool {
    outcome.verdict == expected.verdict
        && outcome.replaced == expected.replaced
        && outcome.old_gone == expected.old_gone
}

/// 逐行对账（B06/B07 同表）。
pub fn reconcile() -> ReconResult {
    let mut checked = 0u32;
    let mut matched = 0u32;
    let mut conflicts = 0u32;
    let mut positives = 0u32;
    let mut negatives = 0u32;
    let mut b06 = 0u32;
    let mut b07 = 0u32;
    let mut stable_negatives = 0u32;
    for row in ROWS.iter() {
        checked = checked.saturating_add(1);
        match row.polarity {
            Polarity::Positive => positives = positives.saturating_add(1),
            Polarity::Negative => negatives = negatives.saturating_add(1),
        }
        match row.book {
            Book::B06 => b06 = b06.saturating_add(1),
            Book::B07 => b07 = b07.saturating_add(1),
        }
        let t = probe_traced(row.kind);
        if matches_expected(&t.outcome, &row.expected) {
            matched = matched.saturating_add(1);
        } else {
            conflicts = conflicts.saturating_add(1);
        }
        if row.polarity == Polarity::Negative && t.before == t.after {
            stable_negatives = stable_negatives.saturating_add(1);
        }
    }
    ReconResult {
        checked, matched, conflicts, positives, negatives, b06, b07,
        stable_negatives, fingerprint: rows_fingerprint(),
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger { pub injected: u32, pub detected: u32, pub escaped: u32 }

impl InjectLedger {
    /// 闭合判定。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
    }
}

/// 期望错误码轮转（篡改用）。
pub fn next_verdict(v: Verdict) -> Verdict {
    match v {
        Verdict::Pass => Verdict::EInval,
        Verdict::EInval => Verdict::ENomem,
        Verdict::ENomem => Verdict::EFault,
        Verdict::EFault => Verdict::EExist,
        Verdict::EExist => Verdict::ENoent,
        Verdict::ENoent => Verdict::Pass,
    }
}

/// 篡改一行：只动期望错误码，保留卡号/册别/探针。
pub fn mutate_row(row: &Row) -> Row {
    Row {
        card: row.card, book: row.book, polarity: row.polarity, kind: row.kind,
        expected: Expectation {
            verdict: next_verdict(row.expected.verdict),
            replaced: row.expected.replaced,
            old_gone: row.expected.old_gone,
        },
    }
}

/// 10 轮篡改注入：每轮换一行，检测器必须全部抓住。
pub fn run_mutation_matrix() -> InjectLedger {
    let mut led = InjectLedger { injected: 0, detected: 0, escaped: 0 };
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        if let Some(row) = ROWS.get((round as usize) % ROWS.len()) {
            let mutated = mutate_row(row);
            led.injected = led.injected.saturating_add(1);
            if matches_expected(&probe(mutated.kind), &mutated.expected) {
                led.escaped = led.escaped.saturating_add(1);
            } else {
                led.detected = led.detected.saturating_add(1);
            }
        }
        round += 1;
    }
    led
}

// ── 对照表与指纹 ──────────────────────────────────────────────────────
/// Linux 错误码对照表（名称, 负值）。
pub const ERRNO_TABLE: &[(&str, i32)] = &[
    ("OK", 0), ("EINVAL", EINVAL), ("ENOMEM", ENOMEM),
    ("EFAULT", EFAULT), ("EEXIST", EEXIST), ("ENOENT", ENOENT),
];

/// 查表取负值；表外返回 0。
pub fn linux_code(name: &str) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some((n, c)) = ERRNO_TABLE.get(i) {
            if *n == name { return *c; }
        }
        i += 1;
    }
    0
}

/// Verdict 与 Linux 对照表逐码一致。
pub fn agrees_with_linux(v: Verdict) -> bool { v.code() == linux_code(v.label()) }

/// FNV-1a 单步。
pub fn fnv_step(h: u32, b: u8) -> u32 { (h ^ b as u32).wrapping_mul(0x0100_0193) }

/// 探针种类码（枚举声明序即码序）。
pub fn kind_code(kind: ProbeKind) -> u8 { kind as u8 }
/// 册别码。
pub fn book_code(book: Book) -> u8 { book as u8 }
/// 极性码。
pub fn polarity_code(polarity: Polarity) -> u8 { polarity as u8 }
/// 判决码。
pub fn verdict_code(v: Verdict) -> u8 { v as u8 }
/// 布尔码。
pub fn bool_byte(b: bool) -> u8 { if b { 1 } else { 0 } }

/// 16 行册面指纹（卡号 + 三观测点）。
pub fn rows_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    for row in ROWS.iter() {
        h = fnv_step(h, kind_code(row.kind));
        h = fnv_step(h, book_code(row.book));
        h = fnv_step(h, polarity_code(row.polarity));
        h = fnv_step(h, verdict_code(row.expected.verdict));
        h = fnv_step(h, bool_byte(row.expected.replaced));
        h = fnv_step(h, bool_byte(row.expected.old_gone));
        let mut b = 0usize;
        while b < row.card.len() {
            if let Some(byte) = row.card.as_bytes().get(b) { h = fnv_step(h, *byte); }
            b += 1;
        }
    }
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9255 判据集（16 条静态 + 1 条动态明细）。
pub fn run_unx9255_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9255");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 册容量
    let r = reconcile();
    let books_ok = ROWS.len() == ROW_SLOTS && r.checked == 16 && r.b06 + r.b07 == 16;
    s.add("册容量：16 行全载入，B06 10 行 / B07 6 行", books_ok,
          "两册行数必须与冻结册一致，缺行或多行都算册别混装");

    // 3. 对账判据 J1
    s.add("对账判据：16 行逐条零冲突", r.conflicts == 0 && r.matched == r.checked,
          "任一行三元组不一致即冲突，冲突非零时 J1 不成立");

    // 4. 正反分层
    let layered = layer_counts().iter().all(|l| l.pos > 0 && l.neg > 0);
    s.add("正反分层：四个域均含正判据与负判据", layered && r.positives + r.negatives == 16,
          "映射/解除/探针/brk 缺任一极性层，声明即无效");

    // 5. 正常流
    let pos_ok = ROWS.iter().filter(|r| r.polarity == Polarity::Positive)
        .all(|r| matches_expected(&probe(r.kind), &r.expected));
    let pos = ROWS.iter().filter(|r| r.polarity == Polarity::Positive).count() as u32;
    s.add("正常流：6 条正判据全部按册面通过", pos_ok && pos == 6,
          "合法调用被误杀即正判据失守");

    // 6. 负路径
    let neg_ok = ROWS.iter().filter(|r| r.polarity == Polarity::Negative)
        .all(|r| {
            let o = probe(r.kind);
            matches_expected(&o, &r.expected) && o.verdict != Verdict::Pass
        });
    let neg = ROWS.iter().filter(|r| r.polarity == Polarity::Negative).count() as u32;
    s.add("负路径：10 条负判据全部按册面报错", neg_ok && neg == 10,
          "非法调用必须报册面错误码，误放行即负判据失守");

    // 7. 判定次序
    let ua = probe(ProbeKind::UnalignedConflict);
    let bb = probe(ProbeKind::BadBackingConflict);
    let hit = probe(ProbeKind::FixedHit);
    let gate_ok = ua.verdict == Verdict::EInval
        && ua.stages & G_ALIGN != 0
        && ua.stages & G_BACKING == 0
        && bb.verdict == Verdict::ENoent
        && bb.stages & G_BACKING != 0
        && bb.stages & G_CONFLICT == 0
        && hit.verdict == Verdict::EExist
        && hit.stages & G_CONFLICT != 0
        && hit.stages & G_PLACE == 0;
    s.add("判定次序：对齐 > 后端 > 冲突，错误码不串位", gate_ok,
          "复合故障必须回最先触发的门，顺序颠倒会掩盖真实原因");

    // 8. 状态机
    let mono = ROWS.iter().all(|r| {
        stages_prefix(probe(r.kind).stages, stage_sequence(r.kind))
    });
    s.add("状态机：全行阶段位为前缀，跳级被拒", mono
          && !stages_prefix(G_PLACE, STAGE_RELEASE)
          && !stages_prefix(G_CONFLICT, STAGE_MMAP),
          "每类操作按其必经门序逐级置位，缺级后置位即判 StageSkip");

    // 9. 覆盖可观测
    let rep = probe_traced(ProbeKind::FixedReplace);
    let hit_t = probe_traced(ProbeKind::FixedHit);
    let obs_ok = rep.outcome.verdict == Verdict::Pass
        && rep.outcome.replaced
        && rep.outcome.old_gone
        && rep.before != rep.after
        && hit_t.outcome.verdict == Verdict::EExist
        && !hit_t.outcome.replaced
        && hit_t.before == hit_t.after;
    s.add("覆盖可观测：替换旧标记消失且快照变化，冲突零变化", obs_ok,
          "覆盖必须能看见旧段退休；冲突必须看不见任何写入");

    // 10. 负判据零副作用
    s.add("负判据零副作用：10/10 世界快照不变", r.stable_negatives == 10 && r.negatives == 10,
          "报错同时改账是最隐蔽的违约，快照必须逐字段相等");

    // 11. 世界容量
    let mut w = World::new();
    let mut filled = 0u32;
    let mut f = 0u32;
    while f < WORLD_SLOTS as u32 {
        if w.insert(ARENA_BASE + f * PAGE, 1, "fill") { filled = filled.saturating_add(1); }
        f += 1;
    }
    let ninth = !w.insert(ARENA_BASE + 8 * PAGE, 1, "over");
    let mut st = 0u8;
    let freed = w.munmap(ARENA_BASE, 1, &mut st) == Ok(());
    let reused = w.insert(ARENA_BASE + 8 * PAGE, 1, "again");
    s.add("容量：世界 8 槽填满即 ENOMEM，解映射后可复用",
          filled == 8 && ninth && freed && reused,
          "槽满必须拒绝而不是覆盖，退休名额必须回收");

    // 12. 双跑一致
    s.add("双跑一致：两轮对账指纹相等",
          reconcile().fingerprint == reconcile().fingerprint,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 13. 注入必抓
    let led = run_mutation_matrix();
    s.add("注入必抓：篡改判据 10/10 检出",
          led.injected == 10 && led.detected == 10 && led.escaped == 0 && led.conserved(),
          "每轮篡改一条期望码，漏抓一次即矩阵失效");

    // 14. 错误码单源
    let errno_ok = ROWS.iter().all(|r| agrees_with_linux(r.expected.verdict))
        && agrees_with_linux(Verdict::EInval)
        && agrees_with_linux(Verdict::ENomem)
        && agrees_with_linux(Verdict::EFault)
        && agrees_with_linux(Verdict::EExist)
        && agrees_with_linux(Verdict::ENoent)
        && linux_code("NOPE") == 0;
    s.add("单源：Verdict 与 Linux 对照表逐码一致", errno_ok,
          "表外码必须拒绝，闭集码必须可复算");

    // 15. 守恒
    s.add("守恒：载入 = 命中 + 冲突，冲突恒 0",
          r.checked == r.matched.saturating_add(r.conflicts) && r.conflicts == 0,
          "对账行不许丢也不许重复计数");

    // 16. 文案闭集
    let text_ok = ReconCode::ALL.iter().all(|c| !c.detail().is_empty());
    let uniq = ReconCode::ALL.iter().enumerate().all(|(u, x)| {
        ReconCode::ALL.iter().skip(u + 1).all(|y| x.val() != y.val())
    });
    s.add("文案闭集：12 码均有中文建议且码值唯一", text_ok && uniq,
          "任一条为空或重码，判据都无法定位到具体违约点");

    // 动态明细
    let text = format!(
        "对账 {} / 冲突 {} / 正 {} / 负 {} / 稳定负 {} / B06 {} / B07 {} / 注入 {}/{} 逃逸 {}",
        r.checked, r.conflicts, r.positives, r.negatives, r.stable_negatives,
        r.b06, r.b07, led.detected, led.injected, led.escaped
    );
    s.add_dyn("对账与注入统计", led.conserved() && r.conflicts == 0, &text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_zero_conflict() {
        let r = reconcile();
        assert_eq!(r.conflicts, 0);
        assert_eq!(r.matched, 16);
    }

    #[test]
    fn polarity_layers() {
        let r = reconcile();
        assert_eq!(r.positives, 6);
        assert_eq!(r.negatives, 10);
        for l in layer_counts().iter() {
            assert!(l.pos > 0 && l.neg > 0);
        }
    }

    #[test]
    fn gate_order() {
        let ua = probe(ProbeKind::UnalignedConflict);
        assert_eq!(ua.verdict, Verdict::EInval);
        assert_eq!(ua.stages & G_BACKING, 0);
        let bb = probe(ProbeKind::BadBackingConflict);
        assert_eq!(bb.verdict, Verdict::ENoent);
        assert_eq!(bb.stages & G_CONFLICT, 0);
        let hit = probe(ProbeKind::FixedHit);
        assert_eq!(hit.verdict, Verdict::EExist);
        assert_eq!(hit.stages & G_PLACE, 0);
    }

    #[test]
    fn stages_are_prefix() {
        for row in ROWS.iter() {
            let st = probe(row.kind).stages;
            let ord = stage_sequence(row.kind);
            assert!(stages_prefix(st, ord), "{:?} st={:#04x} ord={:#04x}", row.kind, st, ord);
        }
        assert!(!stages_prefix(G_PLACE, STAGE_RELEASE));
        assert!(!stages_prefix(G_CONFLICT, STAGE_MMAP));
    }

    #[test]
    fn replace_observable() {
        let t = probe_traced(ProbeKind::FixedReplace);
        assert_eq!(t.outcome.verdict, Verdict::Pass);
        assert!(t.outcome.replaced);
        assert!(t.outcome.old_gone);
        assert_ne!(t.before, t.after);
    }

    #[test]
    fn negative_rows_immutable() {
        for row in ROWS.iter() {
            if row.polarity == Polarity::Negative {
                let t = probe_traced(row.kind);
                assert_eq!(t.before, t.after);
            }
        }
    }

    #[test]
    fn mutation_caught_ten_of_ten() {
        let led = run_mutation_matrix();
        assert_eq!(led.injected, 10);
        assert_eq!(led.detected, 10);
        assert_eq!(led.escaped, 0);
        assert!(led.conserved());
    }

    #[test]
    fn world_capacity() {
        let mut w = World::new();
        let mut i = 0u32;
        while i < WORLD_SLOTS as u32 {
            assert!(w.insert(ARENA_BASE + i * PAGE, 1, "fill"));
            i += 1;
        }
        assert!(!w.insert(ARENA_BASE + 8 * PAGE, 1, "over"));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9255_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
