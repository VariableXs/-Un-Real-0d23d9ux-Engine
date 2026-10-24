//! VE-F3794 · R 域真实走查（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3794`
//!
//! **判据（锚点原文）**：十面双走查、心流面、旁观证据、P0 命中即反例、
//! 退回路线、判据。
//!
//! **本条是什么**：正式真实走查——十面 × 双走查（创作者+终端用户）+
//! R 特色心流面专项；每面旁观证据（输入轨迹/任务耗时/回溯次数）；
//! 心流指标双走查各一份；P0 命中即反例留档；走查不留痕=无效。
//!
//! 诊断码段：独占 0xB9xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB9xx）
// ─────────────────────────────────────────────────────────────

/// F3794 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B9Code(pub u16);

impl B9Code {
    /// 无痕迹——走查无效。
    pub const NO_TRACE: B9Code = B9Code(0xB901);
    /// 过零行——补走。
    pub const ZERO_WALK: B9Code = B9Code(0xB902);
    /// P0 命中——反例留档。
    pub const P0_HIT: B9Code = B9Code(0xB903);
    /// 心流面缺——补。
    pub const FLOW_MISS: B9Code = B9Code(0xB904);
    /// 证据缺。
    pub const EVIDENCE_MISS: B9Code = B9Code(0xB905);
    /// 退回路线缺。
    pub const ROUTE_MISS: B9Code = B9Code(0xB906);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB901 => "走查无痕迹记录，判定无效。",
            0xB902 => "走查过零行，需补走。",
            0xB903 => "P0 命中，须留反例档案。",
            0xB904 => "心流面缺失，需补走。",
            0xB905 => "旁观证据缺失。",
            0xB906 => "退回路线缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 十面双走查 + 心流面
// ─────────────────────────────────────────────────────────────

/// 走查面（十面 + 心流专项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkSurface {
    /// 创作。
    Create,
    /// 工作流。
    Workflow,
    /// 编辑。
    Edit,
    /// 验证。
    Validate,
    /// 预览。
    Preview,
    /// 发布。
    Publish,
    /// 个性化。
    Persona,
    /// 市场。
    Market,
    /// 运行时。
    Runtime,
    /// 设置。
    Settings,
}

impl WalkSurface {
    /// 全集。
    pub const ALL: [WalkSurface; 10] = [
        WalkSurface::Create,
        WalkSurface::Workflow,
        WalkSurface::Edit,
        WalkSurface::Validate,
        WalkSurface::Preview,
        WalkSurface::Publish,
        WalkSurface::Persona,
        WalkSurface::Market,
        WalkSurface::Runtime,
        WalkSurface::Settings,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            WalkSurface::Create => 0,
            WalkSurface::Workflow => 1,
            WalkSurface::Edit => 2,
            WalkSurface::Validate => 3,
            WalkSurface::Preview => 4,
            WalkSurface::Publish => 5,
            WalkSurface::Persona => 6,
            WalkSurface::Market => 7,
            WalkSurface::Runtime => 8,
            WalkSurface::Settings => 9,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            WalkSurface::Create => "创作",
            WalkSurface::Workflow => "工作流",
            WalkSurface::Edit => "编辑",
            WalkSurface::Validate => "验证",
            WalkSurface::Preview => "预览",
            WalkSurface::Publish => "发布",
            WalkSurface::Persona => "个性化",
            WalkSurface::Market => "市场",
            WalkSurface::Runtime => "运行时",
            WalkSurface::Settings => "设置",
        }
    }
}

/// 旁观证据（三件）。
#[derive(Clone, Copy, Debug)]
pub struct Witness {
    /// 输入轨迹条数。
    pub input_events: u32,
    /// 任务耗时秒。
    pub task_secs: u32,
    /// 回溯次数。
    pub backtrack: u32,
    /// 留痕完成。
    pub traced: bool,
}

impl Witness {
    /// 新建缺。
    pub const fn empty() -> Self {
        Witness {
            input_events: 0,
            task_secs: 0,
            backtrack: 0,
            traced: false,
        }
    }

    /// 三件齐且留痕。
    pub fn valid(self) -> bool {
        self.traced && self.input_events > 0 && self.task_secs > 0
    }
}

/// 心流指标（双走查各一份）。
#[derive(Clone, Copy, Debug)]
pub struct FlowMetric {
    /// 中断次数。
    pub interrupts: u32,
    /// 回溯次数。
    pub backtracks: u32,
    /// 流畅时段占比万分比。
    pub fluency_bp: u16,
}

impl FlowMetric {
    /// 新建。
    pub const fn new(interrupts: u32, backtracks: u32, fluency_bp: u16) -> Self {
        FlowMetric {
            interrupts,
            backtracks,
            fluency_bp,
        }
    }

    /// 达标（流畅 ≥5000 即 50%）。
    pub fn ok(self) -> bool {
        self.fluency_bp >= 5_000
    }
}

/// 走查记录。
#[derive(Clone, Copy, Debug)]
pub struct WalkRecord {
    /// 面。
    pub surface: u8,
    /// 画像（0=创作者 1=终端）。
    pub walker: u8,
    /// 旁观证据。
    pub witness: Witness,
    /// 心流指标。
    pub flow: FlowMetric,
    /// P0 命中。
    pub p0: bool,
}

impl WalkRecord {
    /// 构造。
    pub const fn new(surface: u8, walker: u8) -> Self {
        WalkRecord {
            surface,
            walker,
            witness: Witness::empty(),
            flow: FlowMetric::new(0, 0, 0),
            p0: false,
        }
    }

    /// 有效（留痕+证据齐）。
    pub fn valid(self) -> bool {
        self.witness.valid()
    }
}

/// 走查总账。
#[derive(Clone, Debug)]
pub struct WalkLedger {
    recs: Vec<WalkRecord>,
    /// 心流面专项已走（双份）。
    flow_walked: [bool; 2],
    /// 退回路线在册。
    pub route_ok: bool,
}

impl WalkLedger {
    /// 新建。
    pub fn new() -> Self {
        WalkLedger {
            recs: Vec::new(),
            flow_walked: [false; 2],
            route_ok: false,
        }
    }

    /// 记一行。
    pub fn push(&mut self, r: WalkRecord) -> usize {
        self.recs.push(r);
        self.recs.len() - 1
    }

    /// 心流面走查登记。
    pub fn walk_flow(&mut self, walker: u8) -> bool {
        if walker < 2 {
            self.flow_walked[walker as usize] = true;
            true
        } else {
            false
        }
    }

    /// 心流面双份齐。
    pub fn flow_complete(&self) -> bool {
        self.flow_walked[0] && self.flow_walked[1]
    }

    /// 有效行数。
    pub fn valid_count(&self) -> usize {
        self.recs.iter().filter(|r| r.valid()).count()
    }

    /// 无效行数（无痕/过零）。
    pub fn invalid_count(&self) -> usize {
        self.recs.iter().filter(|r| !r.valid()).count()
    }

    /// P0 反例数。
    pub fn p0_count(&self) -> usize {
        self.recs.iter().filter(|r| r.p0).count()
    }

    /// 面×画像覆盖数（20 格）。
    pub fn coverage(&self) -> usize {
        let mut seen = [[false; 2]; 10];
        for r in self.recs.iter() {
            if (r.surface as usize) < 10 && (r.walker as usize) < 2 {
                seen[r.surface as usize][r.walker as usize] = true;
            }
        }
        let mut n = 0;
        for row in seen.iter() {
            for v in row.iter() {
                if *v {
                    n += 1;
                }
            }
        }
        n
    }

    /// 总行数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }
}

impl Default for WalkLedger {
    fn default() -> Self {
        WalkLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 反例档案
// ─────────────────────────────────────────────────────────────

/// P0 反例。
#[derive(Clone, Copy, Debug)]
pub struct CounterExample {
    /// 来源行索引。
    pub from: u16,
    /// 已留档。
    pub archived: bool,
    /// 已退回。
    pub routed: bool,
}

/// 反例档案。
#[derive(Clone, Debug)]
pub struct CeArchive {
    items: Vec<CounterExample>,
}

impl CeArchive {
    /// 新建。
    pub fn new() -> Self {
        CeArchive { items: Vec::new() }
    }

    /// 立案。
    pub fn open(&mut self, from: u16) -> usize {
        self.items.push(CounterExample {
            from,
            archived: false,
            routed: false,
        });
        self.items.len() - 1
    }

    /// 留档。
    pub fn archive(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(c) => {
                c.archived = true;
                true
            }
            None => false,
        }
    }

    /// 退回路线。
    pub fn route(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(c) => {
                c.routed = true;
                true
            }
            None => false,
        }
    }

    /// 未留档数。
    pub fn unarchived(&self) -> usize {
        self.items.iter().filter(|c| !c.archived).count()
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

impl Default for CeArchive {
    fn default() -> Self {
        CeArchive::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3794 判据集。
pub fn run_vr10n_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3794");

    // 0xB901 留痕有效性
    {
        let w = Witness::empty();
        set.add("0xB901-空证据无效", !w.valid(), "");
        let mut w2 = Witness {
            input_events: 10,
            task_secs: 30,
            backtrack: 1,
            traced: true,
        };
        set.add("0xB901-有痕有效", w2.valid(), "");
        w2.traced = false;
        set.add("0xB901-无痕无效", !w2.valid(), "");
    }

    // 0xB902 走查过零
    {
        let l = WalkLedger::new();
        set.add("0xB902-空账过零", l.len() == 0, "");
        let mut l2 = WalkLedger::new();
        l2.push(WalkRecord::new(0, 0));
        set.add("0xB902-空证据无效行", l2.invalid_count() == 1, "");
    }

    // 0xB904 心流面双份
    {
        let mut l = WalkLedger::new();
        set.add("0xB904-初始不齐", !l.flow_complete(), "");
        l.walk_flow(0);
        set.add("0xB904-单份不齐", !l.flow_complete(), "");
        l.walk_flow(1);
        set.add("0xB904-双份齐", l.flow_complete(), "");
        set.add("0xB904-越界拒", !l.walk_flow(2), "");
    }

    // 心流指标
    {
        let m = FlowMetric::new(3, 2, 6000);
        set.add("0xB904-60%达标", m.ok(), "");
        let m2 = FlowMetric::new(3, 2, 4000);
        set.add("0xB904-40%不达标", !m2.ok(), "");
    }

    // 0xB903 P0 反例
    {
        let mut c = CeArchive::new();
        set.add("0xB903-空档案", c.is_empty(), "");
        let i = c.open(3);
        set.add("0xB903-立案例", c.len() == 1, "");
        set.add("0xB903-未留档", c.unarchived() == 1, "");
        c.archive(i);
        set.add("0xB903-留档清", c.unarchived() == 0, "");
        set.add("0xB903-退回路线", c.route(i), "");
    }

    // 0xB905 覆盖
    {
        let mut l = WalkLedger::new();
        for s in 0..10u8 {
            for w in 0..2u8 {
                let mut r = WalkRecord::new(s, w);
                r.witness = Witness {
                    input_events: 5,
                    task_secs: 10,
                    backtrack: 0,
                    traced: true,
                };
                l.push(r);
            }
        }
        set.add("0xB905-20格覆盖", l.coverage() == 20, "");
        set.add("0xB905-全有效", l.valid_count() == 20, "");
        set.add("0xB901-零无效", l.invalid_count() == 0, "");
    }

    // 面序自洽
    {
        let mut ok = true;
        for (i, s) in WalkSurface::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB902-面序自洽", ok, "");
    }

    set
}
