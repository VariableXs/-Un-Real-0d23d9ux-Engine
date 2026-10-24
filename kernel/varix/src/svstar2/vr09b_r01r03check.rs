//! VE-F3762 · R01-R03 组自查核验
//!
//! 锚点：R01-R03 组自查核验——三组联合核验（R01 生态：单源流水/许可红线/保真
//! 对拍/开放格式/收益透明——20 项三段核验；R02 编辑器：控件覆盖/关系保持/历史
//! 保护/正交拦截/脆弱红线——20 项；R03 工坊：单源画布/功耗门/4K 门/隐写 P0/
//! 引导边界——20 项；三段核验复刻）；特色专项（创作 P0 群核验（R 域 P0
//! （伪造用户数据/侵权/隐写/元数据/作品锁死/产出豁免——P0 触发与闭环证据核验
//! （复述闭环红线）；单源群核验（流水/画布/哈希/undo 四单源全域唯一断言
//! （分叉红线复述。
//!
//! 数据结构：三组核验表（60 项）；P0 闭环核验；单源断言核验。
//! 跨批对接点：三组档案；F3365 模式复用；F3765 对账输入。
//!
//! 诊断码段：独占 0x99xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x99xx）
// ─────────────────────────────────────────────────────────────

/// F3762 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Q9Code(pub u16);

impl Q9Code {
    /// P0 无闭环——阻断。
    pub const P0_NO_CLOSURE: Q9Code = Q9Code(0x9901);
    /// 单源分叉——归一红线。
    pub const SOURCE_FORK: Q9Code = Q9Code(0x9902);
    /// 60 项缺——补核验。
    pub const ITEM_GAP: Q9Code = Q9Code(0x9903);
    /// P0 群漏检——实测。
    pub const P0_MISS: Q9Code = Q9Code(0x9904);
    /// 三段核验缺——复刻。
    pub const PHASE_MISS: Q9Code = Q9Code(0x9905);
    /// 分歧未立案——立案。
    pub const CONFLICT_OPEN: Q9Code = Q9Code(0x9906);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9901 => "P0 触发无闭环证据，阻断。",
            0x9902 => "单源断言分叉，归一红线不通过。",
            0x9903 => "三组 60 项核验有缺口。",
            0x9904 => "P0 群漏检，实测不完整。",
            0x9905 => "三段核验缺段，复刻不完整。",
            0x9906 => "核验分歧未立案。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 三组核验（R01/R02/R03 × 20 项）
// ─────────────────────────────────────────────────────────────

/// 组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RGroup {
    /// R01 生态。
    R01,
    /// R02 编辑器。
    R02,
    /// R03 工坊。
    R03,
}

impl RGroup {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            RGroup::R01 => 0,
            RGroup::R02 => 1,
            RGroup::R03 => 2,
        }
    }

    /// 全集。
    pub fn all() -> [RGroup; 3] {
        [RGroup::R01, RGroup::R02, RGroup::R03]
    }
}

/// 组核验表。
#[derive(Clone, Debug)]
pub struct GroupCheck {
    /// 每组已核验项数（目标 20）。
    checked: [u32; 3],
    /// 每组三段核验是否齐（三段核验复刻）。
    phases: [[bool; 3]; 3],
}

impl GroupCheck {
    /// 空表。
    pub fn new() -> GroupCheck {
        GroupCheck {
            checked: [0; 3],
            phases: [[false; 3]; 3],
        }
    }

    /// 记一项核验。
    pub fn check_item(&mut self, g: RGroup) -> u32 {
        let i = g.index() as usize;
        self.checked[i] += 1;
        self.checked[i]
    }

    /// 记一段核验。
    pub fn mark_phase(&mut self, g: RGroup, phase: u8) -> bool {
        if phase >= 3 {
            return false;
        }
        self.phases[g.index() as usize][phase as usize] = true;
        true
    }

    /// 某组 20 项齐。
    pub fn group_complete(&self, g: RGroup) -> bool {
        self.checked[g.index() as usize] >= 20
    }

    /// 全组 60 项齐。
    pub fn all_complete(&self) -> bool {
        self.group_items() >= 60
    }

    /// 总项数。
    pub fn group_items(&self) -> u32 {
        self.checked.iter().sum()
    }

    /// 某组三段齐。
    pub fn phases_complete(&self, g: RGroup) -> bool {
        self.phases[g.index() as usize].iter().all(|b| *b)
    }

    /// 全组三段齐。
    pub fn all_phases_complete(&self) -> bool {
        for g in RGroup::all() {
            if !self.phases_complete(g) {
                return false;
            }
        }
        true
    }

    /// 缺项组清单。
    pub fn incomplete_groups(&self) -> Vec<RGroup> {
        let mut bad = Vec::new();
        for g in RGroup::all() {
            if !self.group_complete(g) {
                bad.push(g);
            }
        }
        bad
    }
}

impl Default for GroupCheck {
    fn default() -> GroupCheck {
        GroupCheck::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 创作 P0 群（六 P0 + 闭环）
// ─────────────────────────────────────────────────────────────

/// P0 类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum P0Kind {
    /// 伪造用户数据。
    FakeUserData,
    /// 侵权。
    Infringe,
    /// 隐写。
    Stego,
    /// 元数据。
    Metadata,
    /// 作品锁死。
    LockIn,
    /// 产出豁免。
    OutputExempt,
}

impl P0Kind {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            P0Kind::FakeUserData => 0,
            P0Kind::Infringe => 1,
            P0Kind::Stego => 2,
            P0Kind::Metadata => 3,
            P0Kind::LockIn => 4,
            P0Kind::OutputExempt => 5,
        }
    }

    /// 全集（六 P0）。
    pub fn all() -> [P0Kind; 6] {
        [
            P0Kind::FakeUserData,
            P0Kind::Infringe,
            P0Kind::Stego,
            P0Kind::Metadata,
            P0Kind::LockIn,
            P0Kind::OutputExempt,
        ]
    }
}

/// P0 闭环记录。
#[derive(Clone, Debug)]
pub struct P0Closure {
    /// P0 类别。
    pub kind: P0Kind,
    /// 是否触发。
    pub triggered: bool,
    /// 是否有闭环证据。
    pub closure_evidence: bool,
}

impl P0Closure {
    /// 构造。
    pub fn new(kind: P0Kind) -> P0Closure {
        P0Closure {
            kind,
            triggered: false,
            closure_evidence: false,
        }
    }

    /// 合规：未触发 或 触发但有闭环。
    pub fn ok(&self) -> bool {
        !self.triggered || self.closure_evidence
    }
}

/// P0 群核验器。
#[derive(Clone, Debug, Default)]
pub struct P0Auditor {
    records: Vec<P0Closure>,
}

impl P0Auditor {
    /// 空。
    pub fn new() -> P0Auditor {
        P0Auditor {
            records: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, r: P0Closure) {
        self.records.push(r);
    }

    /// 扫描无闭环。
    pub fn scan_open(&self) -> Vec<P0Kind> {
        let mut bad = Vec::new();
        for r in self.records.iter() {
            if !r.ok() {
                bad.push(r.kind);
            }
        }
        bad
    }

    /// 六 P0 齐备登记。
    pub fn all_registered(&self) -> bool {
        for k in P0Kind::all() {
            if !self.records.iter().any(|r| r.kind == k) {
                return false;
            }
        }
        true
    }
}

// ─────────────────────────────────────────────────────────────
// 四单源断言（流水/画布/哈希/undo）
// ─────────────────────────────────────────────────────────────

/// 单源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SingleSource {
    /// 流水。
    Pipeline,
    /// 画布。
    Canvas,
    /// 哈希。
    Hash,
    /// undo。
    Undo,
}

impl SingleSource {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            SingleSource::Pipeline => 0,
            SingleSource::Canvas => 1,
            SingleSource::Hash => 2,
            SingleSource::Undo => 3,
        }
    }

    /// 全集（四单源）。
    pub fn all() -> [SingleSource; 4] {
        [
            SingleSource::Pipeline,
            SingleSource::Canvas,
            SingleSource::Hash,
            SingleSource::Undo,
        ]
    }
}

/// 单源断言表。
#[derive(Clone, Debug, Default)]
pub struct SourceAssert {
    /// 每单源是否有且仅有一个实现。
    unique: [bool; 4],
}

impl SourceAssert {
    /// 空。
    pub fn new() -> SourceAssert {
        SourceAssert { unique: [false; 4] }
    }

    /// 标记单源唯一。
    pub fn mark_unique(&mut self, s: SingleSource) {
        self.unique[s.index() as usize] = true;
    }

    /// 标记分叉（演练注入）。
    pub fn inject_fork(&mut self, s: SingleSource) {
        self.unique[s.index() as usize] = false;
    }

    /// 全单源唯一。
    pub fn all_unique(&self) -> bool {
        self.unique.iter().all(|b| *b)
    }

    /// 分叉清单。
    pub fn forking(&self) -> Vec<SingleSource> {
        let mut bad = Vec::new();
        for s in SingleSource::all() {
            if !self.unique[s.index() as usize] {
                bad.push(s);
            }
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3762 判据集。
pub fn run_vr09b_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3762");

    // 0x9903 60 项
    {
        let mut g = GroupCheck::new();
        out.add("0x9903-空表不全", !g.all_complete(), "");
        for _ in 0..20 {
            for grp in RGroup::all() {
                g.check_item(grp);
            }
        }
        out.add("0x9903-60项齐", g.all_complete() && g.group_items() == 60, "");
        out.add("0x9903-每组20", RGroup::all().iter().all(|x| g.group_complete(*x)), "");
    }

    // 0x9905 三段
    {
        let mut g = GroupCheck::new();
        for grp in RGroup::all() {
            for ph in 0..3u8 {
                g.mark_phase(grp, ph);
            }
        }
        out.add("0x9905-三段齐", g.all_phases_complete(), "");
    }

    // 0x9901/0x9904 P0 闭环
    {
        let mut a = P0Auditor::new();
        let mut r = P0Closure::new(P0Kind::Stego);
        r.triggered = true;
        a.add(r);
        out.add("0x9901-无闭环检出", a.scan_open().len() == 1, "");
        for k in P0Kind::all() {
            let mut c = P0Closure::new(k);
            c.triggered = true;
            c.closure_evidence = true;
            a.add(c);
        }
        out.add("0x9904-六P0齐", a.all_registered(), "");
    }

    // 0x9902 单源唯一
    {
        let mut s = SourceAssert::new();
        for src in SingleSource::all() {
            s.mark_unique(src);
        }
        out.add("0x9902-四单源全唯一", s.all_unique(), "");
        s.inject_fork(SingleSource::Hash);
        out.add("0x9902-分叉检出", s.forking().len() == 1, "");
    }

    out
}
