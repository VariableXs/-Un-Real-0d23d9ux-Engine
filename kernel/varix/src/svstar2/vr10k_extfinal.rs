//! VE-F3791 · R 域可扩展性终审（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3791`
//!
//! **判据（锚点原文）**：预演门唯一、变更五文件、四档合议、模版八键、判据。
//!
//! **本条是什么**：变更机制终审——预演门唯一；变更链五文件走完；
//! 影响四档合议制；治理模版八键。四档合议未过不得变更。
//!
//! 诊断码段：独占 0xB6xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB6xx）
// ─────────────────────────────────────────────────────────────

/// F3791 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B6Code(pub u16);

impl B6Code {
    /// 预演门缺失——补。
    pub const GATE_MISS: B6Code = B6Code(0xB601);
    /// 五文件缺——补全。
    pub const FILE_MISS: B6Code = B6Code(0xB602);
    /// 四档未过——合议。
    pub const BOARD_FAIL: B6Code = B6Code(0xB603);
    /// 模版缺键——补齐。
    pub const TPL_MISS: B6Code = B6Code(0xB604);
    /// 变更未走完链——阻断。
    pub const CHAIN_OPEN: B6Code = B6Code(0xB605);
    /// 档位非法。
    pub const LEVEL_BAD: B6Code = B6Code(0xB606);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB601 => "预演门缺失，需补齐。",
            0xB602 => "变更五文件缺件。",
            0xB603 => "四档合议未通过。",
            0xB604 => "治理模版缺键。",
            0xB605 => "变更链未走完，阻断。",
            0xB606 => "影响档位非法。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 变更链五文件
// ─────────────────────────────────────────────────────────────

/// 变更文件（五文件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChgFile {
    /// 申请。
    Request,
    /// 影响分析。
    Impact,
    /// 合议记录。
    Board,
    /// 实施记录。
    Impl,
    /// 验证记录。
    Verify,
}

impl ChgFile {
    /// 全集。
    pub const ALL: [ChgFile; 5] = [
        ChgFile::Request,
        ChgFile::Impact,
        ChgFile::Board,
        ChgFile::Impl,
        ChgFile::Verify,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            ChgFile::Request => 0,
            ChgFile::Impact => 1,
            ChgFile::Board => 2,
            ChgFile::Impl => 3,
            ChgFile::Verify => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            ChgFile::Request => "申请",
            ChgFile::Impact => "影响分析",
            ChgFile::Board => "合议记录",
            ChgFile::Impl => "实施记录",
            ChgFile::Verify => "验证记录",
        }
    }
}

/// 变更链状态机。
#[derive(Clone, Debug)]
pub struct ChgChain {
    done: [bool; 5],
}

impl ChgChain {
    /// 新建。
    pub const fn new() -> Self {
        ChgChain {
            done: [false; 5],
        }
    }

    /// 完成一环（须按序）。
    pub fn finish(&mut self, f: ChgFile) -> bool {
        let i = f.ordinal();
        // 按序：前一环必须已完
        if i > 0 && !self.done[i - 1] {
            return false;
        }
        if self.done[i] {
            return false;
        }
        self.done[i] = true;
        true
    }

    /// 完成数。
    pub fn count(&self) -> usize {
        self.done.iter().filter(|d| **d).count()
    }

    /// 全走完。
    pub fn complete(&self) -> bool {
        self.count() == 5
    }

    /// 跳步拒验证。
    pub fn can_skip(&self) -> bool {
        !self.complete()
    }
}

// ─────────────────────────────────────────────────────────────
// 影响四档合议
// ─────────────────────────────────────────────────────────────

/// 影响档（四档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImpactLevel {
    /// 局部。
    Local,
    /// 模块。
    Module,
    /// 跨域。
    CrossDomain,
    /// 全局。
    Global,
}

impl ImpactLevel {
    /// 全集。
    pub const ALL: [ImpactLevel; 4] = [
        ImpactLevel::Local,
        ImpactLevel::Module,
        ImpactLevel::CrossDomain,
        ImpactLevel::Global,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            ImpactLevel::Local => 0,
            ImpactLevel::Module => 1,
            ImpactLevel::CrossDomain => 2,
            ImpactLevel::Global => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            ImpactLevel::Local => "局部",
            ImpactLevel::Module => "模块",
            ImpactLevel::CrossDomain => "跨域",
            ImpactLevel::Global => "全局",
        }
    }

    /// 须合议的最低档（Local 免合议）。
    pub fn needs_board(self) -> bool {
        self.ordinal() >= 1
    }
}

/// 四档合议会。
#[derive(Clone, Debug)]
pub struct BoardVote {
    /// 投票：每档需要的票数（0..4 档 → 1/2/3/4 票）。
    need: [u8; 4],
    /// 已得票。
    got: [u8; 4],
}

impl BoardVote {
    /// 新建。
    pub const fn new() -> Self {
        BoardVote {
            need: [1, 2, 3, 4],
            got: [0; 4],
        }
    }

    /// 投一票。
    pub fn vote(&mut self, level: ImpactLevel) -> bool {
        let i = level.ordinal();
        if self.got[i] < 255 {
            self.got[i] += 1;
            true
        } else {
            false
        }
    }

    /// 某档过。
    pub fn passed(&self, level: ImpactLevel) -> bool {
        let i = level.ordinal();
        self.got[i] >= self.need[i]
    }

    /// 某档需票数。
    pub fn need_of(&self, level: ImpactLevel) -> u8 {
        self.need[level.ordinal()]
    }
}

// ─────────────────────────────────────────────────────────────
// 治理模版八键
// ─────────────────────────────────────────────────────────────

/// 模版键（八键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TplKey {
    /// 标题。
    Title,
    /// 动机。
    Motive,
    /// 范围。
    Scope,
    /// 影响。
    Impact,
    /// 回滚。
    Rollback,
    /// 验证。
    Verify,
    /// 时间窗。
    Window,
    /// 签核。
    Signoff,
}

impl TplKey {
    /// 全集。
    pub const ALL: [TplKey; 8] = [
        TplKey::Title,
        TplKey::Motive,
        TplKey::Scope,
        TplKey::Impact,
        TplKey::Rollback,
        TplKey::Verify,
        TplKey::Window,
        TplKey::Signoff,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            TplKey::Title => 0,
            TplKey::Motive => 1,
            TplKey::Scope => 2,
            TplKey::Impact => 3,
            TplKey::Rollback => 4,
            TplKey::Verify => 5,
            TplKey::Window => 6,
            TplKey::Signoff => 7,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            TplKey::Title => "标题",
            TplKey::Motive => "动机",
            TplKey::Scope => "范围",
            TplKey::Impact => "影响",
            TplKey::Rollback => "回滚",
            TplKey::Verify => "验证",
            TplKey::Window => "时间窗",
            TplKey::Signoff => "签核",
        }
    }
}

/// 模版填充器。
#[derive(Clone, Debug)]
pub struct Tpl {
    filled: [bool; 8],
}

impl Tpl {
    /// 新建。
    pub const fn new() -> Self {
        Tpl {
            filled: [false; 8],
        }
    }

    /// 填一键。
    pub fn fill(&mut self, k: TplKey) -> bool {
        let i = k.ordinal();
        if self.filled[i] {
            false
        } else {
            self.filled[i] = true;
            true
        }
    }

    /// 缺键数。
    pub fn missing(&self) -> usize {
        self.filled.iter().filter(|f| !**f).count()
    }

    /// 八键齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3791 判据集。
pub fn run_vr10k_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3791");

    // 0xB602 五文件按序
    {
        let mut c = ChgChain::new();
        set.add("0xB602-初始零", c.count() == 0, "");
        set.add("0xB602-跳步拒", !c.finish(ChgFile::Impl), "");
        for f in ChgFile::ALL.iter() {
            c.finish(*f);
        }
        set.add("0xB602-五文件齐", c.complete() && c.count() == 5, "");
        set.add("0xB602-重完拒", !c.finish(ChgFile::Request), "");
    }

    // 0xB605 链未完阻断
    {
        let mut c = ChgChain::new();
        c.finish(ChgFile::Request);
        set.add("0xB605-一环未完链", !c.complete(), "");
        set.add("0xB605-可跳标志", c.can_skip(), "");
    }

    // 0xB603 四档合议
    {
        let mut b = BoardVote::new();
        set.add("0xB603-局部一票", b.need_of(ImpactLevel::Local) == 1, "");
        set.add("0xB603-全局四票", b.need_of(ImpactLevel::Global) == 4, "");
        set.add("0xB603-初始不过", !b.passed(ImpactLevel::Module), "");
        b.vote(ImpactLevel::Module);
        set.add("0xB603-一票不足", !b.passed(ImpactLevel::Module), "");
        b.vote(ImpactLevel::Module);
        set.add("0xB603-两票过", b.passed(ImpactLevel::Module), "");
    }

    // 0xB606 档位
    {
        set.add("0xB606-局部免议", !ImpactLevel::Local.needs_board(), "");
        set.add("0xB606-模块须议", ImpactLevel::Module.needs_board(), "");
        set.add("0xB606-全局须议", ImpactLevel::Global.needs_board(), "");
        let mut ok = true;
        for (i, l) in ImpactLevel::ALL.iter().enumerate() {
            if l.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB606-档序自洽", ok, "");
    }

    // 0xB604 模版八键
    {
        let mut t = Tpl::new();
        set.add("0xB604-初始缺八", t.missing() == 8, "");
        for k in TplKey::ALL.iter() {
            t.fill(*k);
        }
        set.add("0xB604-八键齐", t.complete(), "");
        set.add("0xB604-重填拒", !t.fill(TplKey::Title), "");
    }

    // 文件序自洽
    {
        let mut ok = true;
        for (i, f) in ChgFile::ALL.iter().enumerate() {
            if f.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB602-文件序自洽", ok, "");
    }

    set
}
