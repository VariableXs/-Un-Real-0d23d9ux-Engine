//! VE-F3776 · R 域预备报告（GO/NO-GO）（R09 自查组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3776`
//!
//! **判据（锚点原文）**：十章 R 版、双维硬门、差什么列什么、判定违规 P0、
//! 家族扩展、判据。
//!
//! **本条是什么**：R09→R10 收口闸的**判定器**——十章 R 版报告册
//! （八组状态/对账/清点/汇总/缺口/十维/预评分/预演+双维无障碍章+心流章）
//! + GO/NO-GO 四条件 R 版（十维红零 + 对账零未裁决 + P0 群零闭环 +
//! 双走查五关清）。任一条件不满足即 NO-GO；**判定违规即 P0**。
//!
//! **双维硬门**：工具无障碍维 + 产出无障碍维缺章即 NO-GO（一票联动）。
//!
//! 诊断码段：独占 0xA7xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA7xx）
// ─────────────────────────────────────────────────────────────

/// F3776 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A7Code(pub u16);

impl A7Code {
    /// 输入缺——NO-GO。
    pub const INPUT_MISS: A7Code = A7Code(0xA701);
    /// GO 后逾期——复议。
    pub const GO_EXPIRED: A7Code = A7Code(0xA702);
    /// 长稳/双维缺仍 GO——作废（红线）。
    pub const GO_FRAUD: A7Code = A7Code(0xA703);
    /// 分歧——原始数据。
    pub const DISPUTE: A7Code = A7Code(0xA704);
    /// 十章缺章。
    pub const CHAPTER_MISS: A7Code = A7Code(0xA705);
    /// 四条件之一未过。
    pub const COND_FAIL: A7Code = A7Code(0xA706);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA701 => "报告输入缺失，判定 NO-GO。",
            0xA702 => "GO 后逾期未收口，触发复议。",
            0xA703 => "长稳或双维缺仍判 GO，判定作废（P0 红线）。",
            0xA704 => "判定分歧，回归原始数据。",
            0xA705 => "十章 R 版缺章，报告不完整。",
            0xA706 => "四条件未全过，判定 NO-GO。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 十章 R 版
// ─────────────────────────────────────────────────────────────

/// 报告章（十章 R 版闭集：8 标准 + 双维无障碍 + 心流）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chapter {
    /// 八组状态。
    GroupStatus,
    /// 对账。
    Reconcile,
    /// 清点。
    Inventory,
    /// 汇总。
    Summary,
    /// 缺口。
    Gap,
    /// 十维。
    TenDim,
    /// 预评分。
    PreScore,
    /// 预演+走查。
    Rehearsal,
    /// 双维无障碍章（家族扩展）。
    DualA11y,
    /// 心流章（家族扩展）。
    Flow,
}

impl Chapter {
    /// 全集（十章）。
    pub const ALL: [Chapter; 10] = [
        Chapter::GroupStatus,
        Chapter::Reconcile,
        Chapter::Inventory,
        Chapter::Summary,
        Chapter::Gap,
        Chapter::TenDim,
        Chapter::PreScore,
        Chapter::Rehearsal,
        Chapter::DualA11y,
        Chapter::Flow,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Chapter::GroupStatus => 0,
            Chapter::Reconcile => 1,
            Chapter::Inventory => 2,
            Chapter::Summary => 3,
            Chapter::Gap => 4,
            Chapter::TenDim => 5,
            Chapter::PreScore => 6,
            Chapter::Rehearsal => 7,
            Chapter::DualA11y => 8,
            Chapter::Flow => 9,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Chapter::GroupStatus => "八组状态",
            Chapter::Reconcile => "对账",
            Chapter::Inventory => "清点",
            Chapter::Summary => "汇总",
            Chapter::Gap => "缺口",
            Chapter::TenDim => "十维",
            Chapter::PreScore => "预评分",
            Chapter::Rehearsal => "预演走查",
            Chapter::DualA11y => "双维无障碍",
            Chapter::Flow => "心流",
        }
    }

    /// 是否家族扩展章。
    pub fn is_extension(self) -> bool {
        matches!(self, Chapter::DualA11y | Chapter::Flow)
    }
}

/// 章状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChapterState {
    /// 未写。
    Absent,
    /// 已写未核。
    Drafted,
    /// 已核通过。
    Verified,
}

/// 报告册。
#[derive(Clone, Debug)]
pub struct ReportBook {
    states: [ChapterState; 10],
    /// 长稳证据在册。
    pub long_stable: bool,
    /// 工具无障碍维在册。
    pub tool_a11y: bool,
    /// 产出无障碍维在册。
    pub output_a11y: bool,
}

impl ReportBook {
    /// 新建（全缺）。
    pub fn new() -> Self {
        ReportBook {
            states: [ChapterState::Absent; 10],
            long_stable: false,
            tool_a11y: false,
            output_a11y: false,
        }
    }

    /// 写一章。
    pub fn draft(&mut self, ch: Chapter) {
        self.states[ch.ordinal()] = ChapterState::Drafted;
    }

    /// 核一章。
    pub fn verify(&mut self, ch: Chapter) -> bool {
        let i = ch.ordinal();
        if self.states[i] == ChapterState::Drafted {
            self.states[i] = ChapterState::Verified;
            true
        } else {
            false
        }
    }

    /// 缺章名单。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for ch in Chapter::ALL.iter() {
            if self.states[ch.ordinal()] == ChapterState::Absent {
                out.push(ch.name());
            }
        }
        out
    }

    /// 已核数。
    pub fn verified_count(&self) -> usize {
        self.states
            .iter()
            .filter(|s| **s == ChapterState::Verified)
            .count()
    }

    /// 十章齐且已核。
    pub fn complete(&self) -> bool {
        self.verified_count() == 10
    }
}

impl Default for ReportBook {
    fn default() -> Self {
        ReportBook::new()
    }
}

// ─────────────────────────────────────────────────────────────
// GO/NO-GO 判定器（四条件 R 版）
// ─────────────────────────────────────────────────────────────

/// 判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 通过。
    Go,
    /// 不通过。
    NoGo,
    /// 已作废（判定违规）。
    Void,
}

/// 四条件判定器。
#[derive(Clone, Debug)]
pub struct GoJudge {
    /// 十维红项数（须为 0）。
    pub red_dims: u32,
    /// 对账未裁决数（须为 0）。
    pub unreconciled: u32,
    /// P0 群未闭环数（须为 0）。
    pub p0_open: u32,
    /// 双走查五关是否清。
    pub walk_gates_clear: bool,
    /// 判定是否已被作废。
    pub voided: bool,
}

impl GoJudge {
    /// 新建（默认不满足）。
    pub fn new() -> Self {
        GoJudge {
            red_dims: 1,
            unreconciled: 1,
            p0_open: 1,
            walk_gates_clear: false,
            voided: false,
        }
    }

    /// 四条件逐条。
    pub fn cond_red_zero(&self) -> bool {
        self.red_dims == 0
    }

    /// 对账清。
    pub fn cond_reconcile_zero(&self) -> bool {
        self.unreconciled == 0
    }

    /// P0 群闭环。
    pub fn cond_p0_zero(&self) -> bool {
        self.p0_open == 0
    }

    /// 双走查五关清。
    pub fn cond_walk_clear(&self) -> bool {
        self.walk_gates_clear
    }

    /// 通过条件数。
    pub fn pass_count(&self) -> u32 {
        let mut n = 0;
        if self.cond_red_zero() {
            n += 1;
        }
        if self.cond_reconcile_zero() {
            n += 1;
        }
        if self.cond_p0_zero() {
            n += 1;
        }
        if self.cond_walk_clear() {
            n += 1;
        }
        n
    }

    /// 判定（长稳/双维缺仍 GO → Void 红线）。
    pub fn judge(&self, book: &ReportBook) -> Verdict {
        if self.voided {
            return Verdict::Void;
        }
        // 红线：长稳或双维缺仍 GO → 作废
        let dual_ok = book.tool_a11y && book.output_a11y;
        let go_attempt = self.pass_count() == 4 && book.complete();
        if go_attempt && (!book.long_stable || !dual_ok) {
            return Verdict::Void;
        }
        if go_attempt {
            Verdict::Go
        } else {
            Verdict::NoGo
        }
    }

    /// 显式作废（判定违规立案后）。
    pub fn void(&mut self) {
        self.voided = true;
    }
}

impl Default for GoJudge {
    fn default() -> Self {
        GoJudge::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 核销清单
// ─────────────────────────────────────────────────────────────

/// 核销项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CancelItem {
    /// 来源条件序号 0..4（4=双维/长稳）。
    pub cond: u8,
    /// 已核销。
    pub done: bool,
}

/// 核销清单。
#[derive(Clone, Debug)]
pub struct CancelList {
    items: Vec<CancelItem>,
}

impl CancelList {
    /// 新建。
    pub fn new() -> Self {
        CancelList { items: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, cond: u8) {
        self.items.push(CancelItem { cond, done: false });
    }

    /// 核销。
    pub fn cancel(&mut self, cond: u8) -> bool {
        for it in self.items.iter_mut() {
            if it.cond == cond && !it.done {
                it.done = true;
                return true;
            }
        }
        false
    }

    /// 未核销数。
    pub fn open(&self) -> usize {
        self.items.iter().filter(|i| !i.done).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for CancelList {
    fn default() -> Self {
        CancelList::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3776 判据集。
pub fn run_vr09p_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3776");

    // 0xA705 十章
    {
        let mut b = ReportBook::new();
        set.add("0xA705-初始缺十章", b.missing().len() == 10, "");
        for ch in Chapter::ALL.iter() {
            b.draft(*ch);
        }
        set.add("0xA705-草稿后零缺", b.missing().is_empty(), "");
        set.add("0xA705-草稿未核不完整", !b.complete(), "");
        for ch in Chapter::ALL.iter() {
            b.verify(*ch);
        }
        set.add("0xA705-十章核完", b.complete() && b.verified_count() == 10, "");
        set.add("0xA705-重复核拒", !b.verify(Chapter::Gap), "");
    }

    // 扩展章标记
    {
        set.add("0xA705-双维是扩展", Chapter::DualA11y.is_extension(), "");
        set.add("0xA705-心流是扩展", Chapter::Flow.is_extension(), "");
        set.add("0xA705-对账非扩展", !Chapter::Reconcile.is_extension(), "");
    }

    // 0xA701/0xA706 四条件
    {
        let mut j = GoJudge::new();
        set.add("0xA706-初始四条全不过", j.pass_count() == 0, "");
        j.red_dims = 0;
        set.add("0xA706-红零一条", j.pass_count() == 1 && j.cond_red_zero(), "");
        j.unreconciled = 0;
        j.p0_open = 0;
        j.walk_gates_clear = true;
        set.add("0xA706-四条全过", j.pass_count() == 4, "");
    }

    // 0xA703 判定与作废红线
    {
        let mut b = ReportBook::new();
        for ch in Chapter::ALL.iter() {
            b.draft(*ch);
            b.verify(*ch);
        }
        b.tool_a11y = true;
        b.output_a11y = true;
        // 长稳缺仍 GO → Void
        let mut j = GoJudge::new();
        j.red_dims = 0;
        j.unreconciled = 0;
        j.p0_open = 0;
        j.walk_gates_clear = true;
        set.add("0xA703-长稳缺判Void", j.judge(&b) == Verdict::Void, "");
        b.long_stable = true;
        set.add("0xA703-长稳在判GO", j.judge(&b) == Verdict::Go, "");
        // 双维缺仍 GO → Void
        b.output_a11y = false;
        set.add("0xA703-双维缺判Void", j.judge(&b) == Verdict::Void, "");
        b.output_a11y = true;
        // 不满足条件 → NoGo
        j.p0_open = 1;
        set.add("0xA701-条件不满足NoGo", j.judge(&b) == Verdict::NoGo, "");
    }

    // 显式作废
    {
        let mut j = GoJudge::new();
        j.void();
        let b = ReportBook::new();
        set.add("0xA703-作废后恒Void", j.judge(&b) == Verdict::Void, "");
    }

    // 0xA704 核销清单
    {
        let mut c = CancelList::new();
        set.add("0xA704-空清单", c.is_empty() && c.open() == 0, "");
        c.push(0);
        c.push(1);
        c.push(4);
        set.add("0xA704-三未销", c.open() == 3, "");
        set.add("0xA704-销条件0", c.cancel(0), "");
        set.add("0xA704-重复销拒", !c.cancel(0), "");
        set.add("0xA704-剩二", c.open() == 2, "");
    }

    // 章序自洽
    {
        let mut ok = true;
        for (i, ch) in Chapter::ALL.iter().enumerate() {
            if ch.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xA705-章序自洽", ok, "");
    }

    set
}
