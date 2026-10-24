//! VE-F3777 · 预备演练（R09 自查组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3777`
//!
//! **判据（锚点原文）**：六干跑、双承诺通告、对外红线、顺序闸、复用七用、
//! 判据。
//!
//! **本条是什么**：收口前的**干跑彩排**——五干跑（总账冻结/联测抽样/
//! 移交包/宣告材料/对外通告）+ R 特色第六干跑（双承诺通告干跑：
//! 工具+产出双承诺变更通告演练）。对外通告未干跑即发布 = 红线拦截。
//!
//! **顺序闸**：六干跑全过 → 归档 → 收口前置解锁；跳过任一即拦截。
//!
//! 诊断码段：独占 0xA8xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA8xx）
// ─────────────────────────────────────────────────────────────

/// F3777 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A8Code(pub u16);

impl A8Code {
    /// 干跑失败——修正重跑。
    pub const DRYRUN_FAIL: A8Code = A8Code(0xA801);
    /// 跳过——拦截。
    pub const SKIPPED: A8Code = A8Code(0xA802);
    /// 对外未干跑——红线拦截。
    pub const EXTERNAL_RED: A8Code = A8Code(0xA803);
    /// 尾巴——清零。
    pub const TAIL_OPEN: A8Code = A8Code(0xA804);
    /// 六干跑缺项。
    pub const ITEM_MISS: A8Code = A8Code(0xA805);
    /// 顺序闸拦截。
    pub const ORDER_BLOCK: A8Code = A8Code(0xA806);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA801 => "干跑失败，需修正后重跑。",
            0xA802 => "干跑被跳过，顺序闸拦截。",
            0xA803 => "对外通告未干跑即拟发布，红线拦截。",
            0xA804 => "修正尾巴未清零，预演不收口。",
            0xA805 => "六干跑清单缺项。",
            0xA806 => "顺序闸拦截，禁止跳步。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 六干跑清单
// ─────────────────────────────────────────────────────────────

/// 干跑项（五标准 + R 特色双承诺通告）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DryRun {
    /// 总账冻结干跑。
    LedgerFreeze,
    /// 联测抽样干跑。
    JointSample,
    /// 移交包干跑。
    HandoverPack,
    /// 宣告材料干跑。
    DeclareMaterial,
    /// 对外通告干跑。
    ExternalNotice,
    /// 双承诺通告干跑（R 特色）。
    DualPromiseNotice,
}

impl DryRun {
    /// 全集（六项）。
    pub const ALL: [DryRun; 6] = [
        DryRun::LedgerFreeze,
        DryRun::JointSample,
        DryRun::HandoverPack,
        DryRun::DeclareMaterial,
        DryRun::ExternalNotice,
        DryRun::DualPromiseNotice,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            DryRun::LedgerFreeze => 0,
            DryRun::JointSample => 1,
            DryRun::HandoverPack => 2,
            DryRun::DeclareMaterial => 3,
            DryRun::ExternalNotice => 4,
            DryRun::DualPromiseNotice => 5,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            DryRun::LedgerFreeze => "总账冻结干跑",
            DryRun::JointSample => "联测抽样干跑",
            DryRun::HandoverPack => "移交包干跑",
            DryRun::DeclareMaterial => "宣告材料干跑",
            DryRun::ExternalNotice => "对外通告干跑",
            DryRun::DualPromiseNotice => "双承诺通告干跑",
        }
    }

    /// 是否对外红线项（对外通告/双承诺通告）。
    pub fn is_external(self) -> bool {
        matches!(self, DryRun::ExternalNotice | DryRun::DualPromiseNotice)
    }
}

/// 单项干跑状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunState {
    /// 未跑。
    Pending,
    /// 已过。
    Passed,
    /// 失败待修。
    Failed,
    /// 被跳过（违规）。
    Skipped,
}

/// 干跑清单状态机。
#[derive(Clone, Debug)]
pub struct DryRunBoard {
    states: [RunState; 6],
    /// 修正尾巴。
    open_fix: u32,
    /// 归档完成。
    archived: bool,
}

impl DryRunBoard {
    /// 新建。
    pub fn new() -> Self {
        DryRunBoard {
            states: [RunState::Pending; 6],
            open_fix: 0,
            archived: false,
        }
    }

    /// 执行一项（返回是否通过）。
    pub fn run(&mut self, item: DryRun, ok: bool) -> bool {
        let i = item.ordinal();
        if ok {
            self.states[i] = RunState::Passed;
            true
        } else {
            self.states[i] = RunState::Failed;
            self.open_fix += 1;
            false
        }
    }

    /// 跳过一项（违规登记）。
    pub fn skip(&mut self, item: DryRun) {
        self.states[item.ordinal()] = RunState::Skipped;
    }

    /// 状态取回。
    pub fn state(&self, item: DryRun) -> RunState {
        self.states[item.ordinal()]
    }

    /// 已过数。
    pub fn passed_count(&self) -> usize {
        self.states
            .iter()
            .filter(|s| **s == RunState::Passed)
            .count()
    }

    /// 缺项名（Pending/Failed/Skipped）。
    pub fn incomplete(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for it in DryRun::ALL.iter() {
            match self.states[it.ordinal()] {
                RunState::Passed => {}
                _ => out.push(it.name()),
            }
        }
        out
    }

    /// 对外红线：外部项存在 Skipped 或 Pending 且尝试归档。
    pub fn external_blocked(&self) -> bool {
        for it in DryRun::ALL.iter() {
            if it.is_external() {
                match self.states[it.ordinal()] {
                    RunState::Passed => {}
                    _ => return true,
                }
            }
        }
        false
    }

    /// 清一条尾巴（对应一项 Failed 修正后重跑）。
    pub fn fix_and_pass(&mut self, item: DryRun) -> bool {
        let i = item.ordinal();
        if self.states[i] == RunState::Failed {
            self.states[i] = RunState::Passed;
            if self.open_fix > 0 {
                self.open_fix -= 1;
            }
            true
        } else {
            false
        }
    }

    /// 归档（顺序闸：六项全过 + 尾巴清零 + 对外红线清）。
    pub fn archive(&mut self) -> bool {
        if self.passed_count() == 6 && self.open_fix == 0 && !self.external_blocked() {
            self.archived = true;
            true
        } else {
            false
        }
    }

    /// 开放尾巴。
    pub fn open_fix(&self) -> u32 {
        self.open_fix
    }

    /// 已归档。
    pub fn archived(&self) -> bool {
        self.archived
    }
}

impl Default for DryRunBoard {
    fn default() -> Self {
        DryRunBoard::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 修正记录
// ─────────────────────────────────────────────────────────────

/// 修正记录项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FixRec {
    /// 干跑项序号。
    pub item: u8,
    /// 重跑次数。
    pub reruns: u8,
}

/// 修正记录。
#[derive(Clone, Debug)]
pub struct FixLog {
    recs: Vec<FixRec>,
}

impl FixLog {
    /// 新建。
    pub fn new() -> Self {
        FixLog { recs: Vec::new() }
    }

    /// 记一次修正重跑。
    pub fn note(&mut self, item: DryRun) {
        let ord = item.ordinal() as u8;
        for r in self.recs.iter_mut() {
            if r.item == ord {
                if r.reruns < 255 {
                    r.reruns += 1;
                }
                return;
            }
        }
        self.recs.push(FixRec {
            item: ord,
            reruns: 1,
        });
    }

    /// 某项重跑次数。
    pub fn reruns_of(&self, item: DryRun) -> u8 {
        let ord = item.ordinal() as u8;
        for r in self.recs.iter() {
            if r.item == ord {
                return r.reruns;
            }
        }
        0
    }

    /// 总修正项数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }
}

impl Default for FixLog {
    fn default() -> Self {
        FixLog::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3777 判据集。
pub fn run_vr09q_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3777");

    // 0xA805 六干跑齐全性
    {
        let b = DryRunBoard::new();
        set.add("0xA805-初始零过", b.passed_count() == 0, "");
        set.add("0xA805-六缺", b.incomplete().len() == 6, "");
    }

    // 0xA801 失败记尾巴
    {
        let mut b = DryRunBoard::new();
        set.add("0xA801-首跑失败", !b.run(DryRun::LedgerFreeze, false), "");
        set.add("0xA801-尾巴加一", b.open_fix() == 1, "");
        set.add("0xA801-修后重跑过", b.fix_and_pass(DryRun::LedgerFreeze), "");
        set.add("0xA801-尾巴清", b.open_fix() == 0, "");
        set.add("0xA801-非失败修拒", !b.fix_and_pass(DryRun::JointSample), "");
    }

    // 0xA803 对外红线
    {
        let mut b = DryRunBoard::new();
        for it in DryRun::ALL.iter() {
            if !it.is_external() {
                b.run(*it, true);
            }
        }
        set.add("0xA803-对外未跑阻归档", b.external_blocked(), "");
        set.add("0xA803-未全过拒归档", !b.archive(), "");
        b.run(DryRun::ExternalNotice, true);
        set.add("0xA803-一半对外仍阻", b.external_blocked(), "");
        b.run(DryRun::DualPromiseNotice, true);
        set.add("0xA803-对外全过放行", !b.external_blocked(), "");
    }

    // 0xA802 跳过拦截
    {
        let mut b = DryRunBoard::new();
        for it in DryRun::ALL.iter() {
            b.run(*it, true);
        }
        set.add("0xA802-全过可归档", b.archive(), "");
        set.add("0xA802-已归档", b.archived(), "");
    }

    // 0xA802 跳过态
    {
        let mut b = DryRunBoard::new();
        b.skip(DryRun::JointSample);
        set.add("0xA802-跳过计缺", b.incomplete().len() == 6, "");
        set.add("0xA802-跳过不计过", b.passed_count() == 0, "");
    }

    // 0xA806 顺序闸：归档后状态锁定
    {
        let mut b = DryRunBoard::new();
        for it in DryRun::ALL.iter() {
            b.run(*it, true);
        }
        set.add("0xA806-归档前不可见", !b.archived(), "");
        b.archive();
        set.add("0xA806-归档后可见", b.archived(), "");
    }

    // 修正记录
    {
        let mut f = FixLog::new();
        set.add("0xA804-空记录", f.is_empty() && f.len() == 0, "");
        f.note(DryRun::LedgerFreeze);
        f.note(DryRun::LedgerFreeze);
        f.note(DryRun::JointSample);
        set.add("0xA804-两修正项", f.len() == 2, "");
        set.add("0xA804-重跑计数", f.reruns_of(DryRun::LedgerFreeze) == 2, "");
        set.add("0xA804-单项计数", f.reruns_of(DryRun::JointSample) == 1, "");
        set.add("0xA804-未记项零", f.reruns_of(DryRun::HandoverPack) == 0, "");
    }

    // 项序自洽
    {
        let mut ok = true;
        for (i, it) in DryRun::ALL.iter().enumerate() {
            if it.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xA805-项序自洽", ok, "");
        set.add("0xA803-双承诺对外", DryRun::DualPromiseNotice.is_external(), "");
    }

    set
}
