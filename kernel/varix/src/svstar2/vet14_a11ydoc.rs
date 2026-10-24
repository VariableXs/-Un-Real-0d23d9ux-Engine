//! VE-F3815 · 无障碍渲染文档（VE-T 域 · 无障碍渲染 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3815`
//!
//! **判据（锚点原文）**：双重自举、义务前置、策略公开、2 跳复用、家族管道、判据。
//!
//! **本条是什么**：无障碍渲染文档——开发者册（注入钩子/策略注册）+
//! 策略册（渲染策略表公开）；义务章前置（义务未写不得发布）；
//! **双重自举**：无障碍文档必须自身无障碍达标；导航 2 跳复用。
//!
//! 诊断码段：独占 0xC8xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC8xx）
// ─────────────────────────────────────────────────────────────

/// F3815 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C8Code(pub u16);

impl C8Code {
    /// 漂移——对账。
    pub const DRIFT: C8Code = C8Code(0xC801);
    /// 义务章缺——审查。
    pub const DUTY_MISS: C8Code = C8Code(0xC802);
    /// 自举不过——修（红线）。
    pub const BOOT_FAIL: C8Code = C8Code(0xC803);
    /// 迷宫——2 跳违规。
    pub const MAZE: C8Code = C8Code(0xC804);
    /// 两册缺册。
    pub const BOOK_MISS: C8Code = C8Code(0xC805);
    /// 策略未公开。
    pub const POLICY_HIDDEN: C8Code = C8Code(0xC806);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC801 => "文档与实现漂移，需对账。",
            0xC802 => "义务章缺失，审查拦截。",
            0xC803 => "双重自举不过，文档自身无障碍不达标。",
            0xC804 => "导航超 2 跳，迷宫红线。",
            0xC805 => "两册缺册。",
            0xC806 => "策略表未公开。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 两册结构 + 义务前置
// ─────────────────────────────────────────────────────────────

/// 文档册（两册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocBook {
    /// 开发者册。
    Dev,
    /// 策略册。
    Policy,
}

impl DocBook {
    /// 全集。
    pub const ALL: [DocBook; 2] = [DocBook::Dev, DocBook::Policy];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            DocBook::Dev => 0,
            DocBook::Policy => 1,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            DocBook::Dev => "开发者册",
            DocBook::Policy => "策略册",
        }
    }
}

/// 文档状态。
#[derive(Clone, Debug)]
pub struct DocState {
    /// 两册在册。
    books: [bool; 2],
    /// 义务章已写（前置）。
    pub duty_written: bool,
    /// 策略表已公开。
    pub policy_public: bool,
    /// 自举达标。
    pub boot_ok: bool,
    /// 最大导航跳数。
    pub max_hops: u8,
}

impl DocState {
    /// 新建。
    pub const fn new() -> Self {
        DocState {
            books: [false; 2],
            duty_written: false,
            policy_public: false,
            boot_ok: false,
            max_hops: 99,
        }
    }

    /// 置册。
    pub fn place(&mut self, b: DocBook) {
        self.books[b.ordinal()] = true;
    }

    /// 缺册数。
    pub fn missing(&self) -> usize {
        self.books.iter().filter(|b| !**b).count()
    }

    /// 可发布（义务前置 + 两册 + 公开 + 自举 + 2 跳）。
    pub fn publishable(&self) -> bool {
        self.duty_written
            && self.missing() == 0
            && self.policy_public
            && self.boot_ok
            && self.max_hops <= 2
    }
}

// ─────────────────────────────────────────────────────────────
// 漂移对账 + 2 跳导航
// ─────────────────────────────────────────────────────────────

/// 对账清单项。
#[derive(Clone, Copy, Debug)]
pub struct ReconItem {
    /// 文档指纹。
    pub doc_fp: u16,
    /// 实现指纹。
    pub impl_fp: u16,
}

impl ReconItem {
    /// 构造。
    pub const fn new(doc_fp: u16, impl_fp: u16) -> Self {
        ReconItem { doc_fp, impl_fp }
    }

    /// 漂移。
    pub fn drifted(self) -> bool {
        self.doc_fp != self.impl_fp
    }
}

/// 对账器。
#[derive(Clone, Debug)]
pub struct Reconciler {
    items: Vec<ReconItem>,
}

impl Reconciler {
    /// 新建。
    pub fn new() -> Self {
        Reconciler { items: Vec::new() }
    }

    /// 加项。
    pub fn push(&mut self, it: ReconItem) {
        self.items.push(it);
    }

    /// 漂移数。
    pub fn drift_count(&self) -> usize {
        self.items.iter().filter(|i| i.drifted()).count()
    }

    /// 零漂移。
    pub fn zero_drift(&self) -> bool {
        self.drift_count() == 0
    }
}

impl Default for Reconciler {
    fn default() -> Self {
        Reconciler::new()
    }
}

/// 导航节点。
#[derive(Clone, Copy, Debug)]
pub struct NavNode {
    /// 到入口跳数。
    pub hops: u8,
}

impl NavNode {
    /// 构造。
    pub const fn new(hops: u8) -> Self {
        NavNode { hops }
    }

    /// 2 跳内。
    pub fn within_limit(self) -> bool {
        self.hops <= 2
    }
}

/// 导航审计。
#[derive(Clone, Debug)]
pub struct NavAudit {
    nodes: Vec<NavNode>,
}

impl NavAudit {
    /// 新建。
    pub fn new() -> Self {
        NavAudit { nodes: Vec::new() }
    }

    /// 加节点。
    pub fn push(&mut self, n: NavNode) {
        self.nodes.push(n);
    }

    /// 迷宫数（>2 跳）。
    pub fn maze_count(&self) -> usize {
        self.nodes.iter().filter(|n| !n.within_limit()).count()
    }

    /// 零迷宫。
    pub fn zero_maze(&self) -> bool {
        self.maze_count() == 0
    }
}

impl Default for NavAudit {
    fn default() -> Self {
        NavAudit::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3815 判据集。
pub fn run_f3815_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3815");

    // 0xC805 两册
    {
        let mut d = DocState::new();
        set.add("0xC805-初始缺两", d.missing() == 2, "");
        d.place(DocBook::Dev);
        set.add("0xC805-缺一", d.missing() == 1, "");
        d.place(DocBook::Policy);
        set.add("0xC805-两册齐", d.missing() == 0, "");
    }

    // 0xC802/0xC803/0xC804 发布门
    {
        let mut d = DocState::new();
        d.place(DocBook::Dev);
        d.place(DocBook::Policy);
        set.add("0xC802-无义务不发", !d.publishable(), "");
        d.duty_written = true;
        set.add("0xC806-未公开不发", !d.publishable(), "");
        d.policy_public = true;
        set.add("0xC803-自举不过不发", !d.publishable(), "");
        d.boot_ok = true;
        d.max_hops = 3;
        set.add("0xC804-超2跳不发", !d.publishable(), "");
        d.max_hops = 2;
        set.add("0xC804-全备可发", d.publishable(), "");
    }

    // 0xC801 漂移对账
    {
        let mut r = Reconciler::new();
        set.add("0xC801-空账零漂", r.zero_drift(), "");
        r.push(ReconItem::new(10, 10));
        set.add("0xC801-同指纹不漂", r.zero_drift(), "");
        r.push(ReconItem::new(10, 11));
        set.add("0xC801-漂移检出", r.drift_count() == 1 && !r.zero_drift(), "");
    }

    // 0xC804 2 跳导航
    {
        let mut a = NavAudit::new();
        a.push(NavNode::new(1));
        a.push(NavNode::new(2));
        set.add("0xC804-2跳内零迷宫", a.zero_maze(), "");
        a.push(NavNode::new(3));
        set.add("0xC804-3跳迷宫", a.maze_count() == 1, "");
    }

    // 册序自洽
    {
        let mut ok = true;
        for (i, b) in DocBook::ALL.iter().enumerate() {
            if b.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xC805-册序自洽", ok, "");
    }

    set
}
