//! VE-F3779 · 知识库归档（R09 自查组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3779`
//!
//! **判据（锚点原文）**：三类五段、五域双链、六项贡献、可检索、复用七用、
//! 判据。
//!
//! **本条是什么**：R 域知识条目归档——三类条目（方法/教训/决策）×
//! 五段结构（背景/做法/证据/边界/链接）；五域双链（N/O/P/Q/R 回链）；
//! 六项 R 域贡献登记（双维无障碍资产法/枢纽对账/双走查/对外干跑/
//! 增维登记法/重复红线审计法）。**贡献未登 = 红线补登**。
//!
//! 诊断码段：独占 0xAAxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xAAxx）
// ─────────────────────────────────────────────────────────────

/// F3779 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AACode(pub u16);

impl AACode {
    /// 条目缺段——补。
    pub const SEG_MISS: AACode = AACode(0xAA01);
    /// 链断——修。
    pub const LINK_BREAK: AACode = AACode(0xAA02);
    /// 创新未登——补登（红线）。
    pub const CONTRIB_MISS: AACode = AACode(0xAA03);
    /// 类型非法。
    pub const TYPE_BAD: AACode = AACode(0xAA04);
    /// 检索失败。
    pub const SEARCH_FAIL: AACode = AACode(0xAA05);
    /// 五域缺链。
    pub const DOMAIN_MISS: AACode = AACode(0xAA06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xAA01 => "条目五段缺段，需补齐。",
            0xAA02 => "双链断裂，需修复留痕。",
            0xAA03 => "R 域贡献未登记，红线补登。",
            0xAA04 => "条目类型非法。",
            0xAA05 => "检索失败，索引需修复。",
            0xAA06 => "五域双链缺域。",
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

/// 条目类型（三类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    /// 方法。
    Method,
    /// 教训。
    Lesson,
    /// 决策。
    Decision,
}

impl EntryKind {
    /// 全集。
    pub const ALL: [EntryKind; 3] = [EntryKind::Method, EntryKind::Lesson, EntryKind::Decision];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            EntryKind::Method => 0,
            EntryKind::Lesson => 1,
            EntryKind::Decision => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            EntryKind::Method => "方法",
            EntryKind::Lesson => "教训",
            EntryKind::Decision => "决策",
        }
    }
}

/// 五段（背景/做法/证据/边界/链接）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Segment {
    /// 背景。
    Context,
    /// 做法。
    Approach,
    /// 证据。
    Evidence,
    /// 边界。
    Boundary,
    /// 链接。
    Link,
}

impl Segment {
    /// 全集。
    pub const ALL: [Segment; 5] = [
        Segment::Context,
        Segment::Approach,
        Segment::Evidence,
        Segment::Boundary,
        Segment::Link,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Segment::Context => 0,
            Segment::Approach => 1,
            Segment::Evidence => 2,
            Segment::Boundary => 3,
            Segment::Link => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Segment::Context => "背景",
            Segment::Approach => "做法",
            Segment::Evidence => "证据",
            Segment::Boundary => "边界",
            Segment::Link => "链接",
        }
    }
}

/// 知识条目。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// 类型。
    pub kind: EntryKind,
    /// 五段位掩码（bit0..bit4）。
    pub seg_mask: u8,
    /// 标签 id（0..32）。
    pub tag: u8,
}

impl Entry {
    /// 新条目（零段）。
    pub const fn new(kind: EntryKind, tag: u8) -> Self {
        Entry {
            kind,
            seg_mask: 0,
            tag,
        }
    }

    /// 填一段。
    pub fn fill(&mut self, seg: Segment) {
        self.seg_mask |= 1 << seg.ordinal();
    }

    /// 段齐。
    pub fn complete(&self) -> bool {
        self.seg_mask == 0b1_1111
    }

    /// 缺段数。
    pub fn missing(&self) -> u8 {
        5u8.wrapping_sub(self.seg_mask.count_ones() as u8)
    }
}

/// 标签索引。
#[derive(Clone, Debug)]
pub struct TagIndex {
    /// 每标签条目数（32 标签）。
    counts: [u16; 32],
}

impl TagIndex {
    /// 新建。
    pub const fn new() -> Self {
        TagIndex { counts: [0; 32] }
    }

    /// 登记。
    pub fn touch(&mut self, tag: u8) -> bool {
        if tag < 32 {
            self.counts[tag as usize] = self.counts[tag as usize].saturating_add(1);
            true
        } else {
            false
        }
    }

    /// 某标签计数。
    pub fn count_of(&self, tag: u8) -> u16 {
        if tag < 32 {
            self.counts[tag as usize]
        } else {
            0
        }
    }

    /// 非空标签数。
    pub fn occupied(&self) -> usize {
        self.counts.iter().filter(|c| **c > 0).count()
    }
}

/// 条目库。
#[derive(Clone, Debug)]
pub struct EntryLib {
    entries: Vec<Entry>,
    tags: TagIndex,
}

impl EntryLib {
    /// 新建。
    pub fn new() -> Self {
        EntryLib {
            entries: Vec::new(),
            tags: TagIndex::new(),
        }
    }

    /// 收一条。
    pub fn intake(&mut self, mut e: Entry) -> usize {
        e.fill(Segment::Link);
        self.tags.touch(e.tag);
        self.entries.push(e);
        self.entries.len() - 1
    }

    /// 补段。
    pub fn fill(&mut self, idx: usize, seg: Segment) -> bool {
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.fill(seg);
                true
            }
            None => false,
        }
    }

    /// 缺段条目数。
    pub fn incomplete(&self) -> usize {
        self.entries.iter().filter(|e| !e.complete()).count()
    }

    /// 按标签检索。
    pub fn search_tag(&self, tag: u8) -> usize {
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

    /// 标签索引。
    pub fn tags(&self) -> &TagIndex {
        &self.tags
    }
}

impl Default for EntryLib {
    fn default() -> Self {
        EntryLib::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 五域双链 + 六项贡献
// ─────────────────────────────────────────────────────────────

/// 回链域（N/O/P/Q/R）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkDomain {
    /// N 域。
    N,
    /// O 域。
    O,
    /// P 域。
    P,
    /// Q 域。
    Q,
    /// R 域（自身）。
    R,
}

impl LinkDomain {
    /// 全集。
    pub const ALL: [LinkDomain; 5] = [
        LinkDomain::N,
        LinkDomain::O,
        LinkDomain::P,
        LinkDomain::Q,
        LinkDomain::R,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            LinkDomain::N => 0,
            LinkDomain::O => 1,
            LinkDomain::P => 2,
            LinkDomain::Q => 3,
            LinkDomain::R => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            LinkDomain::N => "N",
            LinkDomain::O => "O",
            LinkDomain::P => "P",
            LinkDomain::Q => "Q",
            LinkDomain::R => "R",
        }
    }
}

/// 五域双链账本。
#[derive(Clone, Debug)]
pub struct DomainLinks {
    /// 正向链（R→域）在册。
    fwd: [bool; 5],
    /// 反向链（域→R）在册。
    back: [bool; 5],
}

impl DomainLinks {
    /// 新建。
    pub const fn new() -> Self {
        DomainLinks {
            fwd: [false; 5],
            back: [false; 5],
        }
    }

    /// 接正向。
    pub fn link_fwd(&mut self, d: LinkDomain) {
        self.fwd[d.ordinal()] = true;
    }

    /// 接反向。
    pub fn link_back(&mut self, d: LinkDomain) {
        self.back[d.ordinal()] = true;
    }

    /// 双链完整域数。
    pub fn paired(&self) -> usize {
        let mut n = 0;
        for i in 0..5 {
            if self.fwd[i] && self.back[i] {
                n += 1;
            }
        }
        n
    }

    /// 断链数（单边）。
    pub fn broken(&self) -> usize {
        let mut n = 0;
        for i in 0..5 {
            if self.fwd[i] != self.back[i] {
                n += 1;
            }
        }
        n
    }

    /// 缺域数（双边都无）。
    pub fn missing(&self) -> usize {
        let mut n = 0;
        for i in 0..5 {
            if !self.fwd[i] && !self.back[i] {
                n += 1;
            }
        }
        n
    }

    /// 修断链（把缺失的一边补上）。
    pub fn repair(&mut self, d: LinkDomain) -> bool {
        let i = d.ordinal();
        if self.fwd[i] != self.back[i] {
            self.fwd[i] = true;
            self.back[i] = true;
            true
        } else {
            false
        }
    }
}

/// R 域六项贡献。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Contribution {
    /// 双维无障碍资产法。
    DualA11yAsset,
    /// 枢纽对账。
    HubReconcile,
    /// 双走查。
    DualWalk,
    /// 对外干跑。
    ExternalDryRun,
    /// 增维登记法。
    DimRegister,
    /// 重复红线审计法。
    DupRedlineAudit,
}

impl Contribution {
    /// 全集。
    pub const ALL: [Contribution; 6] = [
        Contribution::DualA11yAsset,
        Contribution::HubReconcile,
        Contribution::DualWalk,
        Contribution::ExternalDryRun,
        Contribution::DimRegister,
        Contribution::DupRedlineAudit,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Contribution::DualA11yAsset => 0,
            Contribution::HubReconcile => 1,
            Contribution::DualWalk => 2,
            Contribution::ExternalDryRun => 3,
            Contribution::DimRegister => 4,
            Contribution::DupRedlineAudit => 5,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Contribution::DualA11yAsset => "双维无障碍资产法",
            Contribution::HubReconcile => "枢纽对账",
            Contribution::DualWalk => "双走查",
            Contribution::ExternalDryRun => "对外干跑",
            Contribution::DimRegister => "增维登记法",
            Contribution::DupRedlineAudit => "重复红线审计法",
        }
    }
}

/// 贡献登记簿。
#[derive(Clone, Debug)]
pub struct ContribBook {
    done: [bool; 6],
}

impl ContribBook {
    /// 新建。
    pub const fn new() -> Self {
        ContribBook { done: [false; 6] }
    }

    /// 登记。
    pub fn register(&mut self, c: Contribution) -> bool {
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

    /// 未登名单。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for c in Contribution::ALL.iter() {
            if !self.done[c.ordinal()] {
                out.push(c.name());
            }
        }
        out
    }

    /// 全登。
    pub fn complete(&self) -> bool {
        self.count() == 6
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3779 判据集。
pub fn run_vr09s_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3779");

    // 0xAA01 三类五段
    {
        let mut lib = EntryLib::new();
        set.add("0xAA01-初始空", lib.is_empty(), "");
        let i = lib.intake(Entry::new(EntryKind::Method, 1));
        set.add("0xAA01-新条目缺段", !lib.entries_complete_at(i), "");
        lib.fill(i, Segment::Context);
        lib.fill(i, Segment::Approach);
        lib.fill(i, Segment::Evidence);
        lib.fill(i, Segment::Boundary);
        set.add("0xAA01-五段齐", lib.entry_complete(i), "");
        set.add("0xAA01-库完整", lib.incomplete() == 0, "");
    }

    // 0xAA04 三类
    {
        let mut ok = true;
        for (i, k) in EntryKind::ALL.iter().enumerate() {
            if k.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xAA04-类序自洽", ok, "");
    }

    // 0xAA05 标签检索
    {
        let mut lib = EntryLib::new();
        lib.intake(Entry::new(EntryKind::Lesson, 3));
        lib.intake(Entry::new(EntryKind::Lesson, 3));
        lib.intake(Entry::new(EntryKind::Decision, 7));
        set.add("0xAA05-标签计数", lib.search_tag(3) == 2, "");
        set.add("0xAA05-单条检索", lib.search_tag(7) == 1, "");
        set.add("0xAA05-无标签零", lib.search_tag(9) == 0, "");
        set.add("0xAA05-占两标签", lib.tags().occupied() == 2, "");
    }

    // 0xAA06 五域双链
    {
        let mut d = DomainLinks::new();
        set.add("0xAA06-初始缺五域", d.missing() == 5, "");
        d.link_fwd(LinkDomain::N);
        set.add("0xAA06-单边断链", d.broken() == 1, "");
        d.link_back(LinkDomain::N);
        set.add("0xAA06-补齐一对", d.paired() == 1 && d.broken() == 0, "");
        for dom in LinkDomain::ALL.iter() {
            d.link_fwd(*dom);
            d.link_back(*dom);
        }
        set.add("0xAA06-五域全双", d.paired() == 5 && d.missing() == 0, "");
    }

    // 0xAA02 修断链
    {
        let mut d = DomainLinks::new();
        d.link_fwd(LinkDomain::Q);
        set.add("0xAA02-检出断链", d.broken() == 1, "");
        set.add("0xAA02-修进", d.repair(LinkDomain::Q), "");
        set.add("0xAA02-修后不断", d.broken() == 0, "");
        set.add("0xAA02-重复修拒", !d.repair(LinkDomain::Q), "");
    }

    // 0xAA03 六项贡献
    {
        let mut b = ContribBook::new();
        set.add("0xAA03-初始零登", b.count() == 0, "");
        set.add("0xAA03-缺六名", b.missing().len() == 6, "");
        for c in Contribution::ALL.iter() {
            b.register(*c);
        }
        set.add("0xAA03-六项全登", b.complete() && b.count() == 6, "");
        set.add("0xAA03-重登拒", !b.register(Contribution::DualWalk), "");
    }

    // 段序自洽
    {
        let mut ok = true;
        for (i, s) in Segment::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xAA01-段序自洽", ok, "");
    }

    set
}

// 辅助：EntryLib 条目完整性访问（测试支撑）。
impl EntryLib {
    /// 条目是否完整（供判据）。
    pub fn entry_complete(&self, idx: usize) -> bool {
        match self.entries.get(idx) {
            Some(e) => e.complete(),
            None => false,
        }
    }

    /// 条目是否非完整（供判据——新条目应缺段）。
    pub fn entries_complete_at(&self, idx: usize) -> bool {
        self.entry_complete(idx)
    }
}
