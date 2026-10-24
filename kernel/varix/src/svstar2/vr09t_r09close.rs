//! VE-F3780 · R09 组收口双签（R09 自查组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3780`
//!
//! **判据（锚点原文）**：八步+双维+心流、七件套、双签、退回、GO 闸、判据。
//!
//! **本条是什么**：R09 八步（自查/对账/汇总/缺口/十维/预备报告/预备演练/
//! 双维心流核验）全绿后 20 项自检 → **双签**（自查签+核收签）→
//! 向 R10 移交七件套（预备报告+缺口+风险+十维+双维册+心流册+干跑证据）。
//! 核收分歧 → 退回；GO 撤回 → 回滚。
//!
//! 诊断码段：独占 0xABxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xABxx）
// ─────────────────────────────────────────────────────────────

/// F3780 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ABCode(pub u16);

impl ABCode {
    /// 任一步未完——阻断。
    pub const STEP_OPEN: ABCode = ABCode(0xAB01);
    /// 核收分歧——退回。
    pub const REJECT: ABCode = ABCode(0xAB02);
    /// GO 撤回——回滚。
    pub const GO_ROLLBACK: ABCode = ABCode(0xAB03);
    /// 七件套缺件。
    pub const PACK_MISS: ABCode = ABCode(0xAB04);
    /// 双签缺。
    pub const SIGN_MISS: ABCode = ABCode(0xAB05);
    /// 自检缺项。
    pub const SELFCHECK_MISS: ABCode = ABCode(0xAB06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xAB01 => "八步存在未完项，收口阻断。",
            0xAB02 => "核收分歧，移交退回。",
            0xAB03 => "GO 撤回，收口回滚。",
            0xAB04 => "七件套缺件，移交不完整。",
            0xAB05 => "双签缺失，收口不生效。",
            0xAB06 => "20 项自检缺项。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 八步 + 20 项自检
// ─────────────────────────────────────────────────────────────

/// 收口八步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// 八组自查。
    SelfCheck,
    /// 28 对对账。
    Reconcile,
    /// 汇总。
    Summary,
    /// 缺口。
    Gap,
    /// 十维红清零。
    TenDim,
    /// 预备报告 GO。
    PreReport,
    /// 预备演练。
    DryRun,
    /// 双维+心流核验。
    DualFlow,
}

impl Step {
    /// 全集。
    pub const ALL: [Step; 8] = [
        Step::SelfCheck,
        Step::Reconcile,
        Step::Summary,
        Step::Gap,
        Step::TenDim,
        Step::PreReport,
        Step::DryRun,
        Step::DualFlow,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Step::SelfCheck => 0,
            Step::Reconcile => 1,
            Step::Summary => 2,
            Step::Gap => 3,
            Step::TenDim => 4,
            Step::PreReport => 5,
            Step::DryRun => 6,
            Step::DualFlow => 7,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Step::SelfCheck => "八组自查",
            Step::Reconcile => "对账",
            Step::Summary => "汇总",
            Step::Gap => "缺口",
            Step::TenDim => "十维清零",
            Step::PreReport => "预备报告",
            Step::DryRun => "预备演练",
            Step::DualFlow => "双维心流",
        }
    }
}

/// 八步执行板。
#[derive(Clone, Debug)]
pub struct StepBoard {
    done: [bool; 8],
}

impl StepBoard {
    /// 新建。
    pub const fn new() -> Self {
        StepBoard { done: [false; 8] }
    }

    /// 完成一步。
    pub fn finish(&mut self, s: Step) -> bool {
        let i = s.ordinal();
        if self.done[i] {
            false
        } else {
            self.done[i] = true;
            true
        }
    }

    /// 完成数。
    pub fn count(&self) -> usize {
        self.done.iter().filter(|d| **d).count()
    }

    /// 全完。
    pub fn all_done(&self) -> bool {
        self.count() == 8
    }

    /// 未完名单。
    pub fn open(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for s in Step::ALL.iter() {
            if !self.done[s.ordinal()] {
                out.push(s.name());
            }
        }
        out
    }
}

/// 20 项自检账。
#[derive(Clone, Debug)]
pub struct SelfCheck20 {
    marks: [bool; 20],
}

impl SelfCheck20 {
    /// 新建。
    pub const fn new() -> Self {
        SelfCheck20 { marks: [false; 20] }
    }

    /// 勾一项（0..20）。
    pub fn tick(&mut self, i: usize) -> bool {
        if i < 20 {
            self.marks[i] = true;
            true
        } else {
            false
        }
    }

    /// 已勾数。
    pub fn count(&self) -> usize {
        self.marks.iter().filter(|m| **m).count()
    }

    /// 全勾。
    pub fn complete(&self) -> bool {
        self.count() == 20
    }

    /// 越界拒。
    pub fn tick_bad(&mut self) -> bool {
        !self.tick(20)
    }
}

// ─────────────────────────────────────────────────────────────
// 双签 + 七件套移交
// ─────────────────────────────────────────────────────────────

/// 双签（自查签 + 核收签）。
#[derive(Clone, Copy, Debug)]
pub struct DualSign {
    /// 自查签。
    pub self_signed: bool,
    /// 核收签。
    pub recv_signed: bool,
}

impl DualSign {
    /// 新建。
    pub const fn new() -> Self {
        DualSign {
            self_signed: false,
            recv_signed: false,
        }
    }

    /// 自查签。
    pub fn sign_self(&mut self) {
        self.self_signed = true;
    }

    /// 核收签。
    pub fn sign_recv(&mut self) {
        self.recv_signed = true;
    }

    /// 双签齐。
    pub fn complete(&self) -> bool {
        self.self_signed && self.recv_signed
    }
}

/// 七件套。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackItem {
    /// 预备报告。
    PreReport,
    /// 缺口。
    Gap,
    /// 风险。
    Risk,
    /// 十维。
    TenDim,
    /// 双维册。
    DualBook,
    /// 心流册。
    FlowBook,
    /// 干跑证据。
    DryRunEvidence,
}

impl PackItem {
    /// 全集。
    pub const ALL: [PackItem; 7] = [
        PackItem::PreReport,
        PackItem::Gap,
        PackItem::Risk,
        PackItem::TenDim,
        PackItem::DualBook,
        PackItem::FlowBook,
        PackItem::DryRunEvidence,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            PackItem::PreReport => 0,
            PackItem::Gap => 1,
            PackItem::Risk => 2,
            PackItem::TenDim => 3,
            PackItem::DualBook => 4,
            PackItem::FlowBook => 5,
            PackItem::DryRunEvidence => 6,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            PackItem::PreReport => "预备报告",
            PackItem::Gap => "缺口",
            PackItem::Risk => "风险",
            PackItem::TenDim => "十维",
            PackItem::DualBook => "双维册",
            PackItem::FlowBook => "心流册",
            PackItem::DryRunEvidence => "干跑证据",
        }
    }
}

/// 移交状态机。
#[derive(Clone, Debug)]
pub struct Handover {
    /// 七件在册。
    packed: [bool; 7],
    /// 双签。
    pub sign: DualSign,
    /// 核收结果：0=待核 1=核收 2=退回。
    verdict: u8,
    /// 已移交。
    delivered: bool,
}

impl Handover {
    /// 新建。
    pub fn new() -> Self {
        Handover {
            packed: [false; 7],
            sign: DualSign::new(),
            verdict: 0,
            delivered: false,
        }
    }

    /// 装一件。
    pub fn pack(&mut self, p: PackItem) -> bool {
        let i = p.ordinal();
        if self.packed[i] {
            false
        } else {
            self.packed[i] = true;
            true
        }
    }

    /// 缺件数。
    pub fn missing(&self) -> usize {
        self.packed.iter().filter(|p| !**p).count()
    }

    /// 七件齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 核收（分歧→退回）。
    pub fn review(&mut self, accept: bool) -> bool {
        if !self.complete() || !self.sign.complete() {
            return false;
        }
        if accept {
            self.verdict = 1;
            true
        } else {
            self.verdict = 2;
            false
        }
    }

    /// 退回态。
    pub fn rejected(&self) -> bool {
        self.verdict == 2
    }

    /// 移交（须核收通过）。
    pub fn deliver(&mut self) -> bool {
        if self.verdict == 1 && self.sign.complete() {
            self.delivered = true;
            true
        } else {
            false
        }
    }

    /// 已移交。
    pub fn delivered(&self) -> bool {
        self.delivered
    }

    /// 回滚（GO 撤回）。
    pub fn rollback(&mut self) -> bool {
        if self.delivered {
            self.delivered = false;
            self.verdict = 0;
            self.sign = DualSign::new();
            true
        } else {
            false
        }
    }
}

impl Default for Handover {
    fn default() -> Self {
        Handover::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3780 判据集。
pub fn run_vr09t_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3780");

    // 0xAB01 八步
    {
        let mut b = StepBoard::new();
        set.add("0xAB01-初始零步", b.count() == 0, "");
        set.add("0xAB01-缺八名", b.open().len() == 8, "");
        for s in Step::ALL.iter() {
            b.finish(*s);
        }
        set.add("0xAB01-八步全完", b.all_done() && b.count() == 8, "");
        set.add("0xAB01-重完拒", !b.finish(Step::Gap), "");
    }

    // 0xAB06 20 项自检
    {
        let mut c = SelfCheck20::new();
        set.add("0xAB06-初始零", c.count() == 0, "");
        for i in 0..20 {
            c.tick(i);
        }
        set.add("0xAB06-二十齐", c.complete(), "");
        set.add("0xAB06-越界拒", c.tick_bad(), "");
    }

    // 0xAB05 双签
    {
        let mut h = Handover::new();
        set.add("0xAB05-初始未签", !h.sign.complete(), "");
        h.sign.sign_self();
        set.add("0xAB05-单签不齐", !h.sign.complete(), "");
        h.sign.sign_recv();
        set.add("0xAB05-双签齐", h.sign.complete(), "");
    }

    // 0xAB04 七件套
    {
        let mut h = Handover::new();
        set.add("0xAB04-初始缺七", h.missing() == 7, "");
        for p in PackItem::ALL.iter() {
            h.pack(*p);
        }
        set.add("0xAB04-七件齐", h.complete(), "");
        set.add("0xAB04-重装拒", !h.pack(PackItem::Gap), "");
    }

    // 0xAB02 核收与退回
    {
        let mut h = Handover::new();
        for p in PackItem::ALL.iter() {
            h.pack(*p);
        }
        h.sign.sign_self();
        h.sign.sign_recv();
        set.add("0xAB02-核收进", h.review(true), "");
        set.add("0xAB02-可移交", h.deliver(), "");
        set.add("0xAB02-已移交", h.delivered(), "");
    }

    // 0xAB02 退回路径
    {
        let mut h = Handover::new();
        for p in PackItem::ALL.iter() {
            h.pack(*p);
        }
        h.sign.sign_self();
        h.sign.sign_recv();
        set.add("0xAB02-退回返回假", !h.review(false), "");
        set.add("0xAB02-退回态", h.rejected(), "");
        set.add("0xAB02-退回不移交", !h.deliver(), "");
    }

    // 缺签不核收
    {
        let mut h = Handover::new();
        for p in PackItem::ALL.iter() {
            h.pack(*p);
        }
        h.sign.sign_self();
        set.add("0xAB05-缺签不核收", !h.review(true), "");
    }

    // 0xAB03 回滚
    {
        let mut h = Handover::new();
        for p in PackItem::ALL.iter() {
            h.pack(*p);
        }
        h.sign.sign_self();
        h.sign.sign_recv();
        h.review(true);
        h.deliver();
        set.add("0xAB03-回滚进", h.rollback(), "");
        set.add("0xAB03-回滚后未交", !h.delivered(), "");
        set.add("0xAB03-未交回滚拒", !h.rollback(), "");
    }

    // 步序自洽
    {
        let mut ok = true;
        for (i, s) in Step::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xAB01-步序自洽", ok, "");
    }

    set
}
