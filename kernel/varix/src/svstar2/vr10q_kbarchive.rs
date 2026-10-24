//! VE-F3797 · R 域知识库与经验归档（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3797`
//!
//! **判据（锚点原文）**：三类归档、五段结构、R 贡献显性、可检索、新域复用、
//! 判据。
//!
//! **本条是什么**：R 域知识条目正式归档——三类（模式/教训/决策）×
//! 五段（问题/方案/证据/边界/链接）；R 贡献六项显性登记；标签可检索；
//! 新域可复用验证。
//!
//! 诊断码段：独占 0xBCxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xBCxx）
// ─────────────────────────────────────────────────────────────

/// F3797 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BCCode(pub u16);

impl BCCode {
    /// 缺段——补齐。
    pub const SEG_MISS: BCCode = BCCode(0xBC01);
    /// 未登贡献——补登。
    pub const CONTRIB_MISS: BCCode = BCCode(0xBC02);
    /// 检索失败。
    pub const SEARCH_FAIL: BCCode = BCCode(0xBC03);
    /// 类型非法。
    pub const TYPE_BAD: BCCode = BCCode(0xBC04);
    /// 复用验证失败。
    pub const REUSE_FAIL: BCCode = BCCode(0xBC05);
    /// 条目缺。
    pub const ENTRY_MISS: BCCode = BCCode(0xBC06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xBC01 => "知识条目缺段，需补齐。",
            0xBC02 => "R 贡献未登记，需补登。",
            0xBC03 => "知识检索失败。",
            0xBC04 => "条目类型非法。",
            0xBC05 => "新域复用验证失败。",
            0xBC06 => "条目缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 三类五段条目
// ─────────────────────────────────────────────────────────────

/// 知识类型（三类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KnowKind {
    /// 模式。
    Pattern,
    /// 教训。
    Lesson,
    /// 决策。
    Decision,
}

impl KnowKind {
    /// 全集。
    pub const ALL: [KnowKind; 3] = [KnowKind::Pattern, KnowKind::Lesson, KnowKind::Decision];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            KnowKind::Pattern => 0,
            KnowKind::Lesson => 1,
            KnowKind::Decision => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            KnowKind::Pattern => "模式",
            KnowKind::Lesson => "教训",
            KnowKind::Decision => "决策",
        }
    }
}

/// 五段（问题/方案/证据/边界/链接）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KnowSeg {
    /// 问题。
    Problem,
    /// 方案。
    Solution,
    /// 证据。
    Evidence,
    /// 边界。
    Boundary,
    /// 链接。
    Link,
}

impl KnowSeg {
    /// 全集。
    pub const ALL: [KnowSeg; 5] = [
        KnowSeg::Problem,
        KnowSeg::Solution,
        KnowSeg::Evidence,
        KnowSeg::Boundary,
        KnowSeg::Link,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            KnowSeg::Problem => 0,
            KnowSeg::Solution => 1,
            KnowSeg::Evidence => 2,
            KnowSeg::Boundary => 3,
            KnowSeg::Link => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            KnowSeg::Problem => "问题",
            KnowSeg::Solution => "方案",
            KnowSeg::Evidence => "证据",
            KnowSeg::Boundary => "边界",
            KnowSeg::Link => "链接",
        }
    }
}

/// 知识条目。
#[derive(Clone, Copy, Debug)]
pub struct KnowEntry {
    /// 类型。
    pub kind: KnowKind,
    /// 五段掩码。
    pub seg_mask: u8,
    /// 标签 id。
    pub tag: u8,
}

impl KnowEntry {
    /// 新建。
    pub const fn new(kind: KnowKind, tag: u8) -> Self {
        KnowEntry {
            kind,
            seg_mask: 0,
            tag,
        }
    }

    /// 填段。
    pub fn fill(&mut self, s: KnowSeg) {
        self.seg_mask |= 1 << s.ordinal();
    }

    /// 五段齐。
    pub fn complete(&self) -> bool {
        self.seg_mask == 0b1_1111
    }

    /// 缺段数。
    pub fn missing(&self) -> u8 {
        5u8.wrapping_sub(self.seg_mask.count_ones() as u8)
    }
}

/// 知识库。
#[derive(Clone, Debug)]
pub struct KnowLib {
    entries: Vec<KnowEntry>,
}

impl KnowLib {
    /// 新建。
    pub fn new() -> Self {
        KnowLib {
            entries: Vec::new(),
        }
    }

    /// 收条目。
    pub fn intake(&mut self, e: KnowEntry) -> usize {
        self.entries.push(e);
        self.entries.len() - 1
    }

    /// 补段。
    pub fn fill(&mut self, idx: usize, s: KnowSeg) -> bool {
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.fill(s);
                true
            }
            None => false,
        }
    }

    /// 缺段条目数。
    pub fn incomplete(&self) -> usize {
        self.entries.iter().filter(|e| !e.complete()).count()
    }

    /// 按标签检索数。
    pub fn search(&self, tag: u8) -> usize {
        self.entries.iter().filter(|e| e.tag == tag).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for KnowLib {
    fn default() -> Self {
        KnowLib::new()
    }
}

// ─────────────────────────────────────────────────────────────
// R 贡献显性 + 复用验证
// ─────────────────────────────────────────────────────────────

/// R 贡献（六项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RContrib {
    /// 双维无障碍资产法。
    DualAsset,
    /// 枢纽对账法。
    HubRecon,
    /// 双走查法。
    DualWalk,
    /// 对外干跑法。
    ExtDryRun,
    /// 增维登记法。
    DimReg,
    /// 重复红线审计法。
    DupAudit,
}

impl RContrib {
    /// 全集。
    pub const ALL: [RContrib; 6] = [
        RContrib::DualAsset,
        RContrib::HubRecon,
        RContrib::DualWalk,
        RContrib::ExtDryRun,
        RContrib::DimReg,
        RContrib::DupAudit,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            RContrib::DualAsset => 0,
            RContrib::HubRecon => 1,
            RContrib::DualWalk => 2,
            RContrib::ExtDryRun => 3,
            RContrib::DimReg => 4,
            RContrib::DupAudit => 5,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            RContrib::DualAsset => "双维无障碍资产法",
            RContrib::HubRecon => "枢纽对账法",
            RContrib::DualWalk => "双走查法",
            RContrib::ExtDryRun => "对外干跑法",
            RContrib::DimReg => "增维登记法",
            RContrib::DupAudit => "重复红线审计法",
        }
    }
}

/// 贡献登记。
#[derive(Clone, Debug)]
pub struct ContribReg {
    done: [bool; 6],
}

impl ContribReg {
    /// 新建。
    pub const fn new() -> Self {
        ContribReg { done: [false; 6] }
    }

    /// 登记。
    pub fn register(&mut self, c: RContrib) -> bool {
        let i = c.ordinal();
        if self.done[i] {
            false
        } else {
            self.done[i] = true;
            true
        }
    }

    /// 已登数。
    pub fn count(&self) -> usize {
        self.done.iter().filter(|d| **d).count()
    }

    /// 全登。
    pub fn complete(&self) -> bool {
        self.count() == 6
    }

    /// 未登名。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for c in RContrib::ALL.iter() {
            if !self.done[c.ordinal()] {
                out.push(c.name());
            }
        }
        out
    }
}

/// 新域复用验证。
#[derive(Clone, Debug)]
pub struct ReuseCheck {
    /// 新域 id。
    pub domain: u8,
    /// 复用条目数。
    pub reused: u16,
    /// 验证通过。
    pub verified: bool,
}

impl ReuseCheck {
    /// 新建。
    pub const fn new(domain: u8) -> Self {
        ReuseCheck {
            domain,
            reused: 0,
            verified: false,
        }
    }

    /// 复用一条。
    pub fn reuse(&mut self) {
        self.reused += 1;
    }

    /// 验证（须复用过至少一条）。
    pub fn verify(&mut self) -> bool {
        if self.reused > 0 {
            self.verified = true;
            true
        } else {
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3797 判据集。
pub fn run_vr10q_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3797");

    // 0xBC01 三类五段
    {
        let mut lib = KnowLib::new();
        set.add("0xBC06-初始空", lib.is_empty(), "");
        let i = lib.intake(KnowEntry::new(KnowKind::Pattern, 1));
        set.add("0xBC01-新条目缺五", lib.entries_missing(i) == 5, "");
        for s in KnowSeg::ALL.iter() {
            lib.fill(i, *s);
        }
        set.add("0xBC01-五段齐", lib.incomplete() == 0, "");
    }

    // 0xBC04 三类
    {
        let mut ok = true;
        for (i, k) in KnowKind::ALL.iter().enumerate() {
            if k.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xBC04-类序自洽", ok, "");
    }

    // 0xBC03 检索
    {
        let mut lib = KnowLib::new();
        lib.intake(KnowEntry::new(KnowKind::Lesson, 3));
        lib.intake(KnowEntry::new(KnowKind::Lesson, 3));
        lib.intake(KnowEntry::new(KnowKind::Decision, 7));
        set.add("0xBC03-标签检索", lib.search(3) == 2, "");
        set.add("0xBC03-单条", lib.search(7) == 1, "");
        set.add("0xBC03-无标签零", lib.search(9) == 0, "");
    }

    // 0xBC02 贡献登记
    {
        let mut r = ContribReg::new();
        set.add("0xBC02-初始零登", r.count() == 0, "");
        set.add("0xBC02-缺六", r.missing().len() == 6, "");
        for c in RContrib::ALL.iter() {
            r.register(*c);
        }
        set.add("0xBC02-六项全登", r.complete(), "");
        set.add("0xBC02-重登拒", !r.register(RContrib::DualWalk), "");
    }

    // 0xBC05 复用验证
    {
        let mut u = ReuseCheck::new(1);
        set.add("0xBC05-零复用不验", !u.verify() && !u.verified, "");
        u.reuse();
        set.add("0xBC05-复用后验", u.verify() && u.verified, "");
        set.add("0xBC05-计数", u.reused == 1, "");
    }

    // 段序自洽
    {
        let mut ok = true;
        for (i, s) in KnowSeg::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xBC01-段序自洽", ok, "");
    }

    set
}

// KnowLib 辅助（判据支撑）。
impl KnowLib {
    /// 条目缺段数。
    pub fn entries_missing(&self, idx: usize) -> u8 {
        match self.entries.get(idx) {
            Some(e) => e.missing(),
            None => 5,
        }
    }
}
