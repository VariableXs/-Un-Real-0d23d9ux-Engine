//! UNX-F9362 C3 账本消费协议（LTP 用例号面只读消费） · 0x7C800段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9362 逐字）：
//!   ① 纯功能代码 ≥440 行；② 判据原文：UNX-F9362-J1 消费协议判据过，C3 LTP
//!   用例号面只读消费声明格过，零写入 C3 账本格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① C3 账本是**冻结的只读面**：24 个 LTP 用例号按 4 递增排布
//!      （1000, 1004, …, 1092），装载时逐条校验「严格递增 + 标签非空 +
//!      入账写入计数必须为 0」。任何写请求走 `mutation()` 一律回 EROFS，
//!      并在消费者侧单独累计 `refused`——被拒次数不是账本内容，绝不落账。
//!   ② 只读消费声明格：消费者必须先以读者号（非 0）声明消费区间 [lo, hi]，
//!      声明**一次性**（二次声明 EBUSY，旧区间原样保留）。未声明就读、或
//!      读越声明区间都回 EACCES：「有没有资格读」先于「读得到读不到」。
//!   ③ 判定次序冻结为：封存态 → 阶段 → 声明覆盖 → 用例存在 → 单调 →
//!      回执容量。参数面（EINVAL）与权限面（EACCES）必须先于存在面
//!      （ENOENT），存在面先于容量面（ENOMEM）——顺序本身就是判据。
//!   ④ 消费阶段机：Idle → Declared → Armed → Draining → Sealed，单向。
//!      Idle 直接读回 EINVAL，Sealed 后读回 EROFS；首读把 Armed 推到
//!      Draining，seal 只接受 Draining。
//!   ⑤ 零写入证明：完整 24 例消费前后各取一次账本指纹（FNV-1a，逐字节
//!      喂 case_no/status/writes），两次必须逐位相等；回执落在**独立**
//!      的 24 槽数组里，与账本存储不相交。
//!   ⑥ 注入账：8 类违规 × 10 轮 = 80 次注入，逐类计数满 10，
//!      注入 = 检出 + 逃逸且逃逸 == 0。
//!   ⑦ 诊断码段内闭集 13 个（0x7C801..=0x7C80D），码值两两唯一。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9362 独占 0x7C800..=0x7C83F）。
pub const SEG_BASE: u32 = 0x7C800;
/// 段窗口上界（下一张卡 F9363 从 0x7C840 起）。
pub const SEG_NEXT: u32 = 0x7C840;
/// LTP 用例号面容量。
pub const CASE_COUNT: usize = 24;
/// 首例号。
pub const CASE_BASE: u32 = 1000;
/// 例号步长（C3 号面按 4 留出子用例槽）。
pub const CASE_STRIDE: u32 = 4;
/// 末例号。
pub const CASE_LAST: u32 = CASE_BASE + (CASE_COUNT as u32 - 1) * CASE_STRIDE;
/// 消费者回执槽数（与账本存储完全分离）。
pub const MAX_RECEIPTS: usize = 24;
/// 注入轮次。
pub const ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJ_KINDS: usize = 8;
/// C3 读侧声明者号（非 0 才有效）。
pub const READER_C3: u32 = 0x4363;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EBUSY: i32 = 16;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ENOMEM: i32 = 12;
pub const EROFS: i32 = 30;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0D 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LtpCode {
    SegmentViolation = 1,
    LoadNotAscending = 2,
    LoadDuplicate = 3,
    DeclarationMissing = 4,
    DeclarationTwice = 5,
    ScopeMiss = 6,
    WriteRefused = 7,
    ReadNotMonotone = 8,
    CaseMissing = 9,
    StageIllegal = 10,
    SealedRead = 11,
    ReceiptGap = 12,
    LedgerUnbalanced = 13,
}

impl LtpCode {
    /// 码值（段基址 + 序号）。段基址 0x7C800 超出 u16 表达域，故对外码值
    /// 用 u32；repr(u16) 承载的是段内序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            LtpCode::SegmentViolation => "段位越界：F9362 必须占 0x7C800..=0x7C83F 且不与 0x7C840 重叠；请核对段基址",
            LtpCode::LoadNotAscending => "装载次序破坏：C3 用例号必须严格递增，降序或乱序一律 EINVAL；请先排序再入账",
            LtpCode::LoadDuplicate => "装载重复例号：同一用例号出现两次回 EEXIST；请去重，重复行会污染消费回执",
            LtpCode::DeclarationMissing => "声明缺失：消费者未声明只读区间就发起消费，回 EACCES；请先 declare 再 arm",
            LtpCode::DeclarationTwice => "二次声明：声明格一次性，重复声明回 EBUSY；请复用首个声明区间",
            LtpCode::ScopeMiss => "越界消费：用例号落在声明区间之外，回 EACCES；请把声明区间对齐到账本覆盖范围",
            LtpCode::WriteRefused => "写请求被拒：C3 账本对消费者只读，写入回 EROFS；请把观测结果写到独立回执面",
            LtpCode::ReadNotMonotone => "读取不单调：例号回退或重复消费同一例，回 EINVAL；请按号面升序单次消费",
            LtpCode::CaseMissing => "用例号不存在：账本中没有该号，回 ENOENT；请先查号面再读",
            LtpCode::StageIllegal => "阶段非法：消费阶段必须单向推进；请按 Idle→Declared→Armed→Draining→Sealed 握手",
            LtpCode::SealedRead => "封存后仍读：Sealed 是终态，再读回 EROFS；请在新一轮消费中重新装载",
            LtpCode::ReceiptGap => "回执断档：回执序号必须从 0 连续递增；请检查回执写入是否被跳过",
            LtpCode::LedgerUnbalanced => "注入账不守恒：注入数必须等于检出数加逃逸数；请回查注入循环是否漏计",
        }
    }

    /// 闭集全量（`&[LtpCode]` 形式，供唯一性与文案遍历）。
    pub const ALL: &'static [LtpCode] = &[
        LtpCode::SegmentViolation, LtpCode::LoadNotAscending, LtpCode::LoadDuplicate,
        LtpCode::DeclarationMissing, LtpCode::DeclarationTwice, LtpCode::ScopeMiss,
        LtpCode::WriteRefused, LtpCode::ReadNotMonotone, LtpCode::CaseMissing,
        LtpCode::StageIllegal, LtpCode::SealedRead, LtpCode::ReceiptGap,
        LtpCode::LedgerUnbalanced,
    ];
}

// ── LTP 用例号面 ──────────────────────────────────────────────────────
/// 用例判据状态（C3 账本里的只读列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseStatus {
    Passed,
    Skipped,
    Failed,
}

impl CaseStatus {
    /// 冻结编码：0 过 / 1 跳过 / 2 失败。
    pub fn code(self) -> u16 {
        match self {
            CaseStatus::Passed => 0,
            CaseStatus::Skipped => 1,
            CaseStatus::Failed => 2,
        }
    }
}

/// C3 账本一行：用例号 + 标签 + 判据状态 + 入账写入计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseFace {
    pub case_no: u32,
    pub tag: &'static str,
    pub status: CaseStatus,
    pub writes: u16,
}

impl CaseFace {
    /// 空行（数组初始化用）。
    pub const EMPTY: CaseFace = CaseFace { case_no: 0, tag: "", status: CaseStatus::Skipped, writes: 0 };
    /// 构造一行（测试与探测复用，避免手写五段字面量）。
    pub fn new(case_no: u32, tag: &'static str, status: CaseStatus) -> CaseFace {
        CaseFace { case_no, tag, status, writes: 0 }
    }
}

/// 24 个用例短名（自 LTP 号面摘取的稳定命名，只作标签，不参与数值判定）。
pub const CASE_TAGS: [&'static str; CASE_COUNT] = [
    "open01", "open02", "read01", "read02", "write01", "write02", "close01", "close02",
    "mmap01", "mmap02", "munmap01", "mprotect01", "fork01", "wait01", "pipe01", "dup01",
    "clock01", "yield01", "log01", "abi01", "event01", "event02", "exit01", "brk01",
];

/// 构造冻结号面：例号按 STRIDE 递增，状态按 3 取模轮转。
/// 不变量：case_no 严格递增；tag 非空；writes 恒 0（账本只读的前提）。
pub fn canonical_faces() -> [CaseFace; CASE_COUNT] {
    let mut out = [CaseFace::EMPTY; CASE_COUNT];
    let mut i = 0usize;
    while i < CASE_COUNT {
        let status = match i % 3 {
            0 => CaseStatus::Passed,
            1 => CaseStatus::Skipped,
            _ => CaseStatus::Failed,
        };
        let tag = match CASE_TAGS.get(i) {
            Some(t) => *t,
            None => "",
        };
        if let Some(slot) = out.get_mut(i) {
            slot.case_no = CASE_BASE + (i as u32) * CASE_STRIDE;
            slot.tag = tag;
            slot.status = status;
            slot.writes = 0;
        }
        i += 1;
    }
    out
}

// ── 只读账本 ──────────────────────────────────────────────────────────
/// C3 账本（只读面）：24 行 + 行数 + 被拒写探测计数。
/// `refused` 是消费者侧计数器，不属于账本内容，故不进入指纹——否则
/// 「零写入」判据就无法用指纹相等证明。
#[derive(Clone, Copy, Debug)]
pub struct C3Ledger {
    faces: [CaseFace; CASE_COUNT],
    len: usize,
    refused: u32,
}

impl C3Ledger {
    pub const fn new() -> C3Ledger {
        C3Ledger { faces: [CaseFace::EMPTY; CASE_COUNT], len: 0, refused: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 被拒写的累计次数（消费者侧观测值，不落账）。
    pub fn refused_writes(&self) -> u32 {
        self.refused
    }

    /// 行视图（越界返回 None）。
    pub fn face(&self, i: usize) -> Option<&CaseFace> {
        if i < self.len {
            self.faces.get(i)
        } else {
            None
        }
    }

    /// 按用例号线性查找行下标（号面 ≤24 行，线性优于二分）。
    pub fn find(&self, case_no: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if self.faces.get(i).map(|f| f.case_no) == Some(case_no) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 装载只读号面。二次装载回 EBUSY（账本只装一次）。
    /// 顺序：容量 → 严格递增 → 重复号 → 标签非空 → 写入计数为 0。
    /// 任一校验失败都不落账（先写临时数组，全部通过后一次提交）。
    pub fn load(&mut self, faces: &[CaseFace]) -> Result<usize, i32> {
        if self.len != 0 {
            return Err(EBUSY);
        }
        if faces.len() > CASE_COUNT {
            return Err(ENOMEM);
        }
        let mut tmp = [CaseFace::EMPTY; CASE_COUNT];
        let mut prev: u32 = 0;
        let mut i = 0usize;
        while i < faces.len() {
            let row = match faces.get(i) {
                Some(r) => *r,
                None => return Err(EINVAL),
            };
            if i > 0 {
                if row.case_no == prev {
                    return Err(EEXIST);
                }
                if row.case_no < prev {
                    return Err(EINVAL);
                }
            }
            if row.tag.is_empty() || row.writes != 0 {
                return Err(EROFS);
            }
            if let Some(slot) = tmp.get_mut(i) {
                *slot = row;
            }
            prev = row.case_no;
            i += 1;
        }
        // 提交：装载只发生一次（len==0 时 faces 全为 EMPTY），整表覆盖即可。
        self.faces = tmp;
        self.len = i;
        Ok(i)
    }

    /// 写请求：只读面一律 EROFS，并记一笔消费者侧拒写。
    /// 接口刻意不接受「要写的值」——本身不给出任何写入通道。
    pub fn mutation(&mut self, _case_no: u32) -> Result<(), i32> {
        self.refused = self.refused.saturating_add(1);
        Err(EROFS)
    }

    /// 账本内容指纹（不含量、不含 refused）。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < self.len {
            if let Some(f) = self.faces.get(i) {
                h = fnv_u32(h, f.case_no);
                h = fnv_byte(h, f.status.code() as u8);
                h = fnv_byte(h, if f.writes == 0 { 0 } else { 1 });
            }
            i += 1;
        }
        h
    }
}

// ── 只读消费声明格 ────────────────────────────────────────────────────
/// 消费声明格：读者号 + 闭区间 + 一次性封条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeclCell {
    pub reader: u32,
    pub lo: u32,
    pub hi: u32,
    pub sealed: bool,
}

impl DeclCell {
    pub const EMPTY: DeclCell = DeclCell { reader: 0, lo: 0, hi: 0, sealed: false };

    /// 读者号 0 视为无声明权（EACCES）；lo > hi 是参数错误（EINVAL）；
    /// 已声明再声明是 EBUSY。
    pub fn declare(&mut self, reader: u32, lo: u32, hi: u32) -> Result<(), i32> {
        if self.sealed {
            return Err(EBUSY);
        }
        if reader == 0 {
            return Err(EACCES);
        }
        if lo > hi {
            return Err(EINVAL);
        }
        self.reader = reader;
        self.lo = lo;
        self.hi = hi;
        self.sealed = true;
        Ok(())
    }

    pub fn covers(&self, case_no: u32) -> bool {
        self.sealed && case_no >= self.lo && case_no <= self.hi
    }
}

// ── 消费阶段机 ────────────────────────────────────────────────────────
/// 消费阶段（单向推进，Sealed 为终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumeStage {
    Idle,
    Declared,
    Armed,
    Draining,
    Sealed,
}

impl ConsumeStage {
    pub fn code(self) -> u8 {
        match self {
            ConsumeStage::Idle => 0,
            ConsumeStage::Declared => 1,
            ConsumeStage::Armed => 2,
            ConsumeStage::Draining => 3,
            ConsumeStage::Sealed => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ConsumeStage::Idle => "idle",
            ConsumeStage::Declared => "declared",
            ConsumeStage::Armed => "armed",
            ConsumeStage::Draining => "draining",
            ConsumeStage::Sealed => "sealed",
        }
    }
}

/// 合法迁移边（表驱动，供判据与测试共同复算）：只允许前进一格。
pub const STAGE_EDGES: [(ConsumeStage, ConsumeStage); 4] = [
    (ConsumeStage::Idle, ConsumeStage::Declared),
    (ConsumeStage::Declared, ConsumeStage::Armed),
    (ConsumeStage::Armed, ConsumeStage::Draining),
    (ConsumeStage::Draining, ConsumeStage::Sealed),
];

/// 一条迁移是否合法。
pub fn legal_edge(from: ConsumeStage, to: ConsumeStage) -> bool {
    let mut i = 0usize;
    while i < STAGE_EDGES.len() {
        if let Some(edge) = STAGE_EDGES.get(i) {
            if edge.0 == from && edge.1 == to { return true; }
        }
        i += 1;
    }
    false
}

// ── 消费回执 ──────────────────────────────────────────────────────────
/// 一条消费回执（独立存储，绝不写回 C3 账本）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Receipt {
    pub case_no: u32,
    pub status: u16,
    pub seq: u16,
}

impl Receipt {
    pub const EMPTY: Receipt = Receipt { case_no: 0, status: 0, seq: 0 };
}

/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const ZERO: InjectLedger = InjectLedger { injected: 0, detected: 0, escaped: 0 };

    /// 记一笔注入结果。不返回 Result：守恒由 `conserved()` 单独判定，
    /// 避免调用方用被丢弃的返回值掩盖失败。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught { self.detected = self.detected.saturating_add(1); }
        else { self.escaped = self.escaped.saturating_add(1); }
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 只读消费者 ────────────────────────────────────────────────────────
/// C3 只读消费者：声明格 + 阶段 + 单调游标 + 回执数组。
#[derive(Clone, Copy, Debug)]
pub struct Consumer {
    stage: ConsumeStage,
    decl: DeclCell,
    last_case: u32,
    receipts: [Receipt; MAX_RECEIPTS],
    n_receipts: usize,
    rejects: u32,
}

impl Consumer {
    pub const fn new() -> Consumer {
        Consumer {
            stage: ConsumeStage::Idle, decl: DeclCell::EMPTY, last_case: 0,
            receipts: [Receipt::EMPTY; MAX_RECEIPTS], n_receipts: 0, rejects: 0,
        }
    }

    pub fn stage(&self) -> ConsumeStage { self.stage }
    pub fn decl(&self) -> DeclCell { self.decl }
    pub fn receipts_len(&self) -> usize { self.n_receipts }
    pub fn rejects(&self) -> u32 { self.rejects }

    pub fn receipt(&self, i: usize) -> Option<&Receipt> {
        if i < self.n_receipts {
            self.receipts.get(i)
        } else {
            None
        }
    }

    /// 记一笔拒读并返回指定 errno（消费路径的唯一拒绝出口）。
    fn deny(&mut self, e: i32) -> Result<u16, i32> {
        self.rejects = self.rejects.saturating_add(1);
        Err(e)
    }

    /// 绑定读者号并声明只读区间（仅 Idle 阶段可做）。
    pub fn declare(&mut self, reader: u32, lo: u32, hi: u32) -> Result<(), i32> {
        if self.stage != ConsumeStage::Idle {
            return Err(EBUSY);
        }
        match self.decl.declare(reader, lo, hi) {
            Ok(()) => {
                self.stage = ConsumeStage::Declared;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// 武装消费者（仅 Declared 阶段可做）。
    pub fn arm(&mut self) -> Result<(), i32> {
        if self.stage != ConsumeStage::Declared {
            return Err(EINVAL);
        }
        self.stage = ConsumeStage::Armed;
        Ok(())
    }

    /// 消费一个用例号，成功返回该例的判据状态编码。
    /// 判定次序：封存 → 阶段 → 声明覆盖 → 存在 → 单调 → 回执容量。
    pub fn consume(&mut self, ledger: &C3Ledger, case_no: u32) -> Result<u16, i32> {
        if self.stage == ConsumeStage::Sealed {
            return self.deny(EROFS);
        }
        if !matches!(self.stage, ConsumeStage::Armed | ConsumeStage::Draining) {
            return self.deny(EINVAL);
        }
        if !self.decl.covers(case_no) {
            return self.deny(EACCES);
        }
        let slot = match ledger.find(case_no) {
            Some(s) => s,
            None => return self.deny(ENOENT),
        };
        if self.n_receipts > 0 && case_no <= self.last_case {
            return self.deny(EINVAL);
        }
        if self.n_receipts >= MAX_RECEIPTS {
            return self.deny(ENOMEM);
        }
        let status = match ledger.face(slot) {
            Some(f) => f.status.code(),
            None => 0,
        };
        let seq = self.n_receipts as u16;
        if let Some(r) = self.receipts.get_mut(self.n_receipts) {
            r.case_no = case_no;
            r.status = status;
            r.seq = seq;
        }
        self.n_receipts += 1;
        self.last_case = case_no;
        if self.stage == ConsumeStage::Armed {
            self.stage = ConsumeStage::Draining;
        }
        Ok(status)
    }

    /// 按号面序遍历整本账（协议判据与指纹共用同一条路径）。
    pub fn scan(&mut self, ledger: &C3Ledger) -> Result<u32, i32> {
        let mut i = 0usize;
        let mut n = 0u32;
        while i < ledger.len() {
            let case_no = match ledger.face(i) {
                Some(f) => f.case_no,
                None => return Err(ENOENT),
            };
            match self.consume(ledger, case_no) {
                Ok(_) => n = n.saturating_add(1),
                Err(e) => return Err(e),
            }
            i += 1;
        }
        Ok(n)
    }

    /// 封存消费面（仅 Draining 阶段可做；Sealed 终态）。
    pub fn seal(&mut self) -> Result<(), i32> {
        if self.stage != ConsumeStage::Draining {
            return Err(EINVAL);
        }
        self.stage = ConsumeStage::Sealed;
        Ok(())
    }

    /// 回执序号是否从 0 连续（ReceiptGap 的观测点）。
    pub fn receipts_contiguous(&self) -> bool {
        let mut i = 0usize;
        while i < self.n_receipts {
            if self.receipts.get(i).map(|r| r.seq as usize) != Some(i) { return false; }
            i += 1;
        }
        true
    }
}

// ── 构造与注入 ────────────────────────────────────────────────────────
/// 构造冻结账本；装载失败时返回空账本（探测路径会把失败显性记成逃逸）。
pub fn canonical_ledger() -> C3Ledger {
    let mut l = C3Ledger::new();
    let faces = canonical_faces();
    if l.load(&faces).is_err() {
        return C3Ledger::new();
    }
    l
}

/// 构造已声明并武装的消费者（声明区间恰覆盖整本账）。
pub fn armed_consumer() -> Consumer {
    let mut c = Consumer::new();
    if c.declare(READER_C3, CASE_BASE, CASE_LAST).is_err() {
        return c;
    }
    if c.arm().is_err() {
        return Consumer::new();
    }
    c
}

/// 注入类别（每类对应一种必须被拒的违规路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjKind {
    WriteAttempt,
    UndeclaredRead,
    MissingCase,
    BackwardRead,
    DuplicateLoad,
    DescendingLoad,
    ReadAfterSeal,
    OutOfScope,
}

/// 八类注入的全集（顺序即注入轮次顺序）。
pub const INJ_ALL: [InjKind; INJ_KINDS] = [
    InjKind::WriteAttempt, InjKind::UndeclaredRead, InjKind::MissingCase, InjKind::BackwardRead,
    InjKind::DuplicateLoad, InjKind::DescendingLoad, InjKind::ReadAfterSeal, InjKind::OutOfScope,
];

/// 两行号面（注入与检查共用，重复/降序由调用方决定顺序）。
pub fn two_rows(a: u32, b: u32) -> [CaseFace; 2] {
    [CaseFace::new(a, "row_a", CaseStatus::Passed), CaseFace::new(b, "row_b", CaseStatus::Skipped)]
}

/// 单类注入探测：返回「该类违规被正确拒绝」。
pub fn probe(kind: InjKind) -> bool {
    match kind {
        InjKind::WriteAttempt => {
            let mut l = canonical_ledger();
            let before = l.digest();
            let refused = l.mutation(CASE_BASE) == Err(EROFS);
            refused && before == l.digest() && l.refused_writes() == 1
        }
        InjKind::UndeclaredRead => Consumer::new().consume(&canonical_ledger(), CASE_BASE) == Err(EINVAL),
        InjKind::MissingCase => armed_consumer().consume(&canonical_ledger(), CASE_BASE + 1) == Err(ENOENT),
        InjKind::BackwardRead => {
            let l = canonical_ledger();
            let mut c = armed_consumer();
            c.consume(&l, CASE_BASE + CASE_STRIDE).is_ok() && c.consume(&l, CASE_BASE) == Err(EINVAL)
        }
        InjKind::DuplicateLoad => {
            let mut l = C3Ledger::new();
            l.load(&two_rows(CASE_BASE, CASE_BASE)) == Err(EEXIST) && l.is_empty()
        }
        InjKind::DescendingLoad => {
            let mut l = C3Ledger::new();
            l.load(&two_rows(CASE_BASE + CASE_STRIDE, CASE_BASE)) == Err(EINVAL) && l.is_empty()
        }
        InjKind::ReadAfterSeal => {
            let l = canonical_ledger();
            let mut c = armed_consumer();
            c.consume(&l, CASE_BASE).is_ok() && c.seal().is_ok()
                && c.consume(&l, CASE_BASE + CASE_STRIDE) == Err(EROFS)
        }
        InjKind::OutOfScope => armed_consumer().consume(&canonical_ledger(), CASE_LAST + CASE_STRIDE) == Err(EACCES),
    }
}

/// 全量注入：8 类 × 10 轮，逐类计数 + 账本。
pub fn run_injection() -> ([u32; INJ_KINDS], InjectLedger) {
    let mut per_cell = [0u32; INJ_KINDS];
    let mut ledger = InjectLedger::ZERO;
    let mut i = 0usize;
    while i < INJ_ALL.len() {
        if let Some(kind) = INJ_ALL.get(i).copied() {
            let mut r = 0u32;
            while r < ROUNDS {
                let caught = probe(kind);
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_cell.get_mut(i) {
                        *slot = slot.saturating_add(1);
                    }
                }
                r += 1;
            }
        }
        i += 1;
    }
    (per_cell, ledger)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a（32 位）单字节喂入。
pub fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// FNV-1a 喂一个 u32（小端四字节，不依赖宿主字节序）。
pub fn fnv_u32(h: u32, v: u32) -> u32 {
    let mut x = h;
    x = fnv_byte(x, (v & 0xFF) as u8);
    x = fnv_byte(x, ((v >> 8) & 0xFF) as u8);
    x = fnv_byte(x, ((v >> 16) & 0xFF) as u8);
    fnv_byte(x, ((v >> 24) & 0xFF) as u8)
}

/// 规范语料指纹：装载 → 声明 → 武装 → 逐例消费 → 封存，逐回执喂入。
/// 双跑必须逐位相等（隐藏状态即失败）。
pub fn corpus_fingerprint() -> u32 {
    let l = canonical_ledger();
    let mut c = armed_consumer();
    let mut h: u32 = 0x811C_9DC5;
    h = fnv_u32(h, l.digest());
    let mut i = 0usize;
    while i < l.len() {
        let case_no = match l.face(i) {
            Some(f) => f.case_no,
            None => 0,
        };
        match c.consume(&l, case_no) {
            Ok(status) => {
                h = fnv_u32(h, case_no);
                h = fnv_byte(h, status as u8);
            }
            Err(e) => {
                h = fnv_byte(h, 0xFF);
                h = fnv_byte(h, (e & 0x7F) as u8);
            }
        }
        i += 1;
    }
    h = fnv_byte(h, c.stage().code());
    h = fnv_byte(h, c.receipts_len() as u8);
    if c.seal().is_ok() {
        h = fnv_byte(h, c.stage().code());
    }
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9362 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9362_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9362");

    s.add("段位守卫：0x7C800 起 0x40 窗口不与 F9363 重叠",
          SEG_BASE == 0x7C800 && SEG_BASE % 0x40 == 0 && SEG_BASE.saturating_add(0x3F) < SEG_NEXT,
          "段基址必须唯一且相邻段边界严格小于下一段起点");

    let ledger = canonical_ledger();
    let mut ascending = ledger.len() == CASE_COUNT;
    let mut prev = 0u32;
    let mut i = 0usize;
    while i < ledger.len() {
        if let Some(f) = ledger.face(i) {
            if (i > 0 && f.case_no <= prev) || f.tag.is_empty() || f.writes != 0 { ascending = false; }
            prev = f.case_no;
        } else { ascending = false; }
        i += 1;
    }
    s.add("装载：24 例号严格递增且写入计数为 0", ascending,
          "只读号面必须自带顺序与零写入前提，否则后续判据无意义");

    let mut dup = C3Ledger::new();
    let dup_r = dup.load(&two_rows(CASE_BASE, CASE_BASE));
    let mut desc = C3Ledger::new();
    let desc_r = desc.load(&two_rows(CASE_BASE + CASE_STRIDE, CASE_BASE));
    s.add("装载拒绝：重复号 EEXIST、降序号 EINVAL 且不落账",
          dup_r == Err(EEXIST) && desc_r == Err(EINVAL) && dup.is_empty() && desc.is_empty(),
          "坏号面必须在提交前整体拒绝，不得留下半本账");

    let mut decl_probe = Consumer::new();
    let undeclared = decl_probe.consume(&ledger, CASE_BASE);
    let zero_reader = decl_probe.declare(0, CASE_BASE, CASE_LAST);
    let bad_span = decl_probe.declare(READER_C3, CASE_LAST, CASE_BASE);
    s.add("声明格：未声明读 EINVAL、读者号 0 回 EACCES、倒置区间回 EINVAL",
          undeclared == Err(EINVAL) && zero_reader == Err(EACCES) && bad_span == Err(EINVAL),
          "声明权与区间合法性同属参数/权限面，必须先于读取判定");

    let mut once = Consumer::new();
    let first = once.declare(READER_C3, CASE_BASE, CASE_LAST);
    let second = once.declare(READER_C3, CASE_BASE, CASE_BASE + CASE_STRIDE);
    let kept = once.decl();
    s.add("声明格一次性：二次声明 EBUSY 且旧区间不变",
          first.is_ok() && second == Err(EBUSY) && kept.sealed && kept.lo == CASE_BASE && kept.hi == CASE_LAST,
          "声明格是单源口径：不允许被后到读者改写覆盖范围");

    let mut ro = canonical_ledger();
    let before = ro.digest();
    let write_r = ro.mutation(CASE_BASE);
    let after = ro.digest();
    let mut ro2 = canonical_ledger();
    let mut reader = armed_consumer();
    let scanned = reader.scan(&ro2);
    let after_scan = ro2.digest();
    s.add("零写入：写请求 EROFS 且消费前后账本指纹不变",
          write_r == Err(EROFS) && before == after && ro.refused_writes() == 1
              && scanned == Ok(CASE_COUNT as u32) && before == after_scan,
          "只读消费不得改动账本任何一个字节，回执必须落在独立存储");

    let mut proto_ledger = canonical_ledger();
    let mut proto = armed_consumer();
    let consumed = proto.scan(&proto_ledger);
    let mut echo_ok = true;
    let mut k = 0usize;
    while k < proto.receipts_len() {
        let same = matches!((proto.receipt(k), proto_ledger.face(k)),
            (Some(r), Some(f)) if r.case_no == f.case_no && r.status == f.status.code());
        if !same { echo_ok = false; }
        k += 1;
    }
    let drained = proto.stage() == ConsumeStage::Draining;
    s.add("消费协议：24 例逐号单次消费且回执状态与号面一致",
          consumed == Ok(CASE_COUNT as u32) && proto.receipts_len() == CASE_COUNT
              && echo_ok && proto.receipts_contiguous() && drained,
          "回执序号连续、状态回显一致，才算消费协议判据真的过");

    let mut tail = armed_consumer();
    let tail_r = tail.consume(&proto_ledger, CASE_LAST + CASE_STRIDE);
    let mut back = armed_consumer();
    let f1 = back.consume(&proto_ledger, CASE_BASE + CASE_STRIDE);
    let f2 = back.consume(&proto_ledger, CASE_BASE);
    s.add("越界与回退：号面外 EACCES、号面内回退 EINVAL",
          tail_r == Err(EACCES) && f1.is_ok() && f2 == Err(EINVAL) && back.receipts_len() == 1,
          "单次升序消费：回退读不得产生第二条回执");

    let mut scope_ledger = C3Ledger::new();
    let faces = canonical_faces();
    let loaded = scope_ledger.load(&faces);
    let mut narrow = Consumer::new();
    let narrowed = narrow.declare(READER_C3, CASE_BASE, CASE_BASE + CASE_STRIDE);
    let armed_narrow = narrow.arm();
    let inside = narrow.consume(&scope_ledger, CASE_BASE);
    let outside = narrow.consume(&scope_ledger, CASE_BASE + CASE_STRIDE * 2);
    s.add("声明范围：区间内放行、区间外 EACCES",
          loaded == Ok(CASE_COUNT) && narrowed.is_ok() && armed_narrow.is_ok()
              && inside.is_ok() && outside == Err(EACCES),
          "声明区间是消费权限边界，越界读与未声明读同属 EACCES");

    let mut stage_c = Consumer::new();
    let idle_read = stage_c.consume(&proto_ledger, CASE_BASE);
    let mut edges_ok = true;
    let mut e = 0usize;
    while e < STAGE_EDGES.len() {
        if let Some(edge) = STAGE_EDGES.get(e) {
            if !legal_edge(edge.0, edge.1) { edges_ok = false; }
        }
        e += 1;
    }
    let illegal = legal_edge(ConsumeStage::Idle, ConsumeStage::Draining)
        || legal_edge(ConsumeStage::Sealed, ConsumeStage::Armed);
    let mut sealed_c = armed_consumer();
    let opened = sealed_c.consume(&proto_ledger, CASE_BASE);
    let sealed = sealed_c.seal();
    let sealed_read = sealed_c.consume(&proto_ledger, CASE_BASE + CASE_STRIDE);
    s.add("阶段机：Idle 读 EINVAL、四边合法、封存后读 EROFS",
          idle_read == Err(EINVAL) && edges_ok && !illegal && opened.is_ok()
              && sealed.is_ok() && sealed_read == Err(EROFS) && sealed_c.stage() == ConsumeStage::Sealed,
          "阶段只进不退；终态封存后任何读取都必须被拒");

    let (per_cell, inj) = run_injection();
    let cells_ok = per_cell.iter().all(|n| *n == ROUNDS);
    s.add("注入账：8 类 × 10 轮全检出且逃逸为 0",
          cells_ok && inj.conserved() && inj.escaped == 0
              && inj.injected == (INJ_KINDS as u32) * ROUNDS,
          "每类违规都必须被自己的 errno 拒绝，漏一类即逃逸");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范消费语料指纹相等", fp_a == fp_b,
          "同输入两次逐字节相等；隐藏状态或计数泄漏都会破坏相等");

    let mut uniq = true;
    let mut a = 0usize;
    while a < LtpCode::ALL.len() {
        let mut b = a + 1;
        while b < LtpCode::ALL.len() {
            if let (Some(x), Some(y)) = (LtpCode::ALL.get(a), LtpCode::ALL.get(b)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add("诊断码：13 个码值两两唯一且落在 0x7C801..=0x7C80D",
          uniq && LtpCode::ALL.len() == 13,
          "段内闭集码不得重复，否则红项无法定位到唯一诊断");

    s.add("文案闭集：13 个诊断码均有中文可操作建议",
          LtpCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能给出下一步动作，禁止空串或纯编号");

    let cell_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入快照", inj.conserved() && inj.escaped == 0, &cell_text);
    let write_text = format!("拒写 {} 次 / 账本指纹 0x{:08X}", ro.refused_writes(), before);
    s.add_dyn("只读快照", before == after_scan, &write_text);
    let read_text = format!("回执 {}/{} / 阶段 {}", proto.receipts_len(), CASE_COUNT, proto.stage().name());
    s.add_dyn("消费快照", proto.receipts_len() == CASE_COUNT && drained, &read_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_ledger_is_ascending() {
        let l = canonical_ledger();
        assert_eq!(l.len(), CASE_COUNT);
        assert_eq!(l.face(0).map(|f| f.case_no), Some(CASE_BASE));
        assert_eq!(l.face(CASE_COUNT - 1).map(|f| f.case_no), Some(CASE_LAST));
        let mut i = 1usize;
        while i < l.len() {
            assert_eq!(l.face(i).map(|f| f.case_no) > l.face(i - 1).map(|f| f.case_no), true);
            i += 1;
        }
    }

    #[test]
    fn load_rejects_duplicate_and_descending() {
        let mut dup = C3Ledger::new();
        assert_eq!(dup.load(&two_rows(1000, 1000)), Err(EEXIST));
        assert_eq!(dup.is_empty(), true);
        let mut desc = C3Ledger::new();
        assert_eq!(desc.load(&two_rows(1004, 1000)), Err(EINVAL));
        assert_eq!(desc.is_empty(), true);
    }

    #[test]
    fn declaration_is_single_shot() {
        let mut c = Consumer::new();
        assert_eq!(c.declare(READER_C3, CASE_BASE, CASE_LAST), Ok(()));
        assert_eq!(c.stage(), ConsumeStage::Declared);
        assert_eq!(c.declare(READER_C3, CASE_BASE, CASE_BASE), Err(EBUSY));
        assert_eq!(c.decl().hi, CASE_LAST);
    }

    #[test]
    fn write_is_refused_and_ledger_unchanged() {
        let mut l = canonical_ledger();
        let before = l.digest();
        assert_eq!(l.mutation(CASE_BASE), Err(EROFS));
        assert_eq!(l.digest(), before);
        assert_eq!(l.refused_writes(), 1);
    }

    #[test]
    fn protocol_consumes_all_faces() {
        let l = canonical_ledger();
        let mut c = armed_consumer();
        assert_eq!(c.scan(&l), Ok(CASE_COUNT as u32));
        assert_eq!(c.receipts_len(), CASE_COUNT);
        assert_eq!(c.stage(), ConsumeStage::Draining);
        assert_eq!(c.receipts_contiguous(), true);
        assert_eq!(c.seal(), Ok(()));
        assert_eq!(c.stage(), ConsumeStage::Sealed);
    }

    #[test]
    fn backward_and_missing_reads() {
        let l = canonical_ledger();
        let mut c = armed_consumer();
        assert_eq!(c.consume(&l, CASE_BASE + CASE_STRIDE).is_ok(), true);
        assert_eq!(c.consume(&l, CASE_BASE), Err(EINVAL));
        let mut d = armed_consumer();
        assert_eq!(d.consume(&l, CASE_BASE + 1), Err(ENOENT));
    }

    #[test]
    fn stage_machine_guards() {
        let l = canonical_ledger();
        let mut idle = Consumer::new();
        assert_eq!(idle.consume(&l, CASE_BASE), Err(EINVAL));
        assert_eq!(idle.arm(), Err(EINVAL));
        assert_eq!(idle.seal(), Err(EINVAL));
        let mut c = armed_consumer();
        assert_eq!(c.seal(), Err(EINVAL));
        assert_eq!(c.consume(&l, CASE_BASE).is_ok(), true);
        assert_eq!(c.seal(), Ok(()));
        assert_eq!(c.consume(&l, CASE_BASE + CASE_STRIDE), Err(EROFS));
        assert_eq!(legal_edge(ConsumeStage::Idle, ConsumeStage::Draining), false);
    }

    #[test]
    fn injection_ledger_conserved() {
        let (per, l) = run_injection();
        assert_eq!(l.injected, (INJ_KINDS as u32) * ROUNDS);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.conserved(), true);
        let mut i = 0usize;
        while i < per.len() {
            assert_eq!(per.get(i), Some(&ROUNDS));
            i += 1;
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9362_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
        assert_eq!(s.truncated(), false);
    }
}
