//! VE-F3818 · S01 联调（VE-T 域 · 无障碍渲染 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3818`
//!
//! **判据（锚点原文）**：48 格、三故障、断链 P0、重演确定、三分流、判据。
//!
//! **本条是什么**：S01 组合联调——六渲染场景×四态×三设备档分层抽样
//! 48 格；20 分钟无障碍渲染剧本（混流漫游）；三故障注入（态风暴/
//! 注入超时/断链）；重演确定性；三分流处置。
//!
//! 诊断码段：独占 0xCBxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xCBxx）
// ─────────────────────────────────────────────────────────────

/// F3818 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CBCode(pub u16);

impl CBCode {
    /// 断链检出——P0。
    pub const BREAK_P0: CBCode = CBCode(0xCB01);
    /// 覆盖不达标——补跑。
    pub const COVER_LOW: CBCode = CBCode(0xCB02);
    /// 重演分歧——立案。
    pub const REPLAY_DIFF: CBCode = CBCode(0xCB03);
    /// 处置无责——兜底。
    pub const NO_OWNER: CBCode = CBCode(0xCB04);
    /// 抽样缺格。
    pub const CELL_MISS: CBCode = CBCode(0xCB05);
    /// 剧本缺段。
    pub const SCRIPT_MISS: CBCode = CBCode(0xCB06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xCB01 => "联调检出断链，P0。",
            0xCB02 => "48 格覆盖不达标，需补跑。",
            0xCB03 => "重演分歧，需立案。",
            0xCB04 => "处置无责任方，走兜底。",
            0xCB05 => "抽样格缺失。",
            0xCB06 => "剧本段缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 48 格分层抽样
// ─────────────────────────────────────────────────────────────

/// 渲染场景（六场景）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    /// 界面。
    Ui,
    /// 列表。
    List,
    /// 画布。
    Canvas,
    /// 表面。
    Surface,
    /// 图表。
    Chart,
    /// 游戏渲染。
    Game,
}

impl Scene {
    /// 全集。
    pub const ALL: [Scene; 6] = [
        Scene::Ui,
        Scene::List,
        Scene::Canvas,
        Scene::Surface,
        Scene::Chart,
        Scene::Game,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Scene::Ui => 0,
            Scene::List => 1,
            Scene::Canvas => 2,
            Scene::Surface => 3,
            Scene::Chart => 4,
            Scene::Game => 5,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Scene::Ui => "界面",
            Scene::List => "列表",
            Scene::Canvas => "画布",
            Scene::Surface => "表面",
            Scene::Chart => "图表",
            Scene::Game => "游戏渲染",
        }
    }
}

/// 抽样集（72 格中抽 48）。
#[derive(Clone, Debug)]
pub struct Sample48 {
    /// 已跑格位图（72 位压缩为 72 bool）。
    ran: Vec<bool>,
}

impl Sample48 {
    /// 新建（72 格全未跑）。
    pub fn new() -> Self {
        let mut ran = Vec::new();
        for _ in 0..72 {
            ran.push(false);
        }
        Sample48 { ran }
    }

    /// 跑一格（idx 0..72）。
    pub fn run(&mut self, idx: usize) -> bool {
        match self.ran.get_mut(idx) {
            Some(v) => {
                *v = true;
                true
            }
            None => false,
        }
    }

    /// 已跑数。
    pub fn ran_count(&self) -> usize {
        self.ran.iter().filter(|v| **v).count()
    }

    /// 达标（≥48）。
    pub fn coverage_ok(&self) -> bool {
        self.ran_count() >= 48
    }

    /// 缺格数（相对 48 底线）。
    pub fn shortfall(&self) -> usize {
        if self.coverage_ok() {
            0
        } else {
            48 - self.ran_count()
        }
    }
}

impl Default for Sample48 {
    fn default() -> Self {
        Sample48::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 剧本执行 + 重演确定
// ─────────────────────────────────────────────────────────────

/// 剧本段（四漫游）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// 辅具接入漫游。
    Assistive,
    /// 高对比漫游。
    HighContrast,
    /// 大字号漫游。
    LargeFont,
    /// reduce 漫游。
    Reduce,
}

impl Act {
    /// 全集。
    pub const ALL: [Act; 4] = [
        Act::Assistive,
        Act::HighContrast,
        Act::LargeFont,
        Act::Reduce,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Act::Assistive => 0,
            Act::HighContrast => 1,
            Act::LargeFont => 2,
            Act::Reduce => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Act::Assistive => "辅具接入漫游",
            Act::HighContrast => "高对比漫游",
            Act::LargeFont => "大字号漫游",
            Act::Reduce => "reduce漫游",
        }
    }
}

/// 剧本执行器。
#[derive(Clone, Debug)]
pub struct ScriptRunner {
    done: [bool; 4],
    /// 重演指纹（同输入须同指纹）。
    pub fingerprint: u32,
}

impl ScriptRunner {
    /// 新建。
    pub const fn new() -> Self {
        ScriptRunner {
            done: [false; 4],
            fingerprint: 0,
        }
    }

    /// 跑一段。
    pub fn run(&mut self, a: Act) -> bool {
        self.done[a.ordinal()] = true;
        true
    }

    /// 缺段数。
    pub fn missing(&self) -> usize {
        self.done.iter().filter(|d| !**d).count()
    }

    /// 四段齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 重演校验（两次指纹一致）。
    pub fn replay_ok(fp_a: u32, fp_b: u32) -> bool {
        fp_a == fp_b
    }
}

// ─────────────────────────────────────────────────────────────
// 三故障注入 + 三分流
// ─────────────────────────────────────────────────────────────

/// 故障（三种）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 态风暴。
    Storm,
    /// 注入超时。
    Timeout,
    /// 断链。
    ChainBreak,
}

impl Fault {
    /// 全集。
    pub const ALL: [Fault; 3] = [Fault::Storm, Fault::Timeout, Fault::ChainBreak];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Fault::Storm => 0,
            Fault::Timeout => 1,
            Fault::ChainBreak => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Fault::Storm => "态风暴",
            Fault::Timeout => "注入超时",
            Fault::ChainBreak => "断链",
        }
    }

    /// 是否 P0。
    pub fn is_p0(self) -> bool {
        self == Fault::ChainBreak
    }
}

/// 三分流处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Triage {
    /// 立即修。
    FixNow,
    /// 立案。
    Case,
    /// 兜底。
    Fallback,
}

/// 处置单。
#[derive(Clone, Copy, Debug)]
pub struct Ticket {
    /// 故障。
    pub fault: Fault,
    /// 处置。
    pub action: Triage,
    /// 有责任方。
    pub owned: bool,
}

impl Ticket {
    /// 构造。
    pub const fn new(fault: Fault) -> Self {
        Ticket {
            fault,
            action: Triage::Case,
            owned: true,
        }
    }

    /// 合规（P0 必须 FixNow；无主必须兜底）。
    pub fn compliant(self) -> bool {
        if self.fault.is_p0() {
            self.action == Triage::FixNow
        } else if !self.owned {
            self.action == Triage::Fallback
        } else {
            true
        }
    }
}

/// 处置清单。
#[derive(Clone, Debug)]
pub struct TicketList {
    items: Vec<Ticket>,
}

impl TicketList {
    /// 新建。
    pub fn new() -> Self {
        TicketList { items: Vec::new() }
    }

    /// 立单。
    pub fn push(&mut self, t: Ticket) -> usize {
        self.items.push(t);
        self.items.len() - 1
    }

    /// 不合规数。
    pub fn non_compliant(&self) -> usize {
        self.items.iter().filter(|t| !t.compliant()).count()
    }

    /// 无主数。
    pub fn unowned(&self) -> usize {
        self.items.iter().filter(|t| !t.owned).count()
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

impl Default for TicketList {
    fn default() -> Self {
        TicketList::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3818 判据集。
pub fn run_f3818_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3818");

    // 0xCB05 48 格
    {
        let mut s = Sample48::new();
        set.add("0xCB05-初始零跑", s.ran_count() == 0, "");
        set.add("0xCB02-未达48", !s.coverage_ok() && s.shortfall() == 48, "");
        for i in 0..48 {
            s.run(i);
        }
        set.add("0xCB05-48达标", s.coverage_ok() && s.shortfall() == 0, "");
        set.add("0xCB05-越界拒", !s.run(72), "");
    }

    // 0xCB06 剧本四段
    {
        let mut r = ScriptRunner::new();
        set.add("0xCB06-初始缺四", r.missing() == 4, "");
        for a in Act::ALL.iter() {
            r.run(*a);
        }
        set.add("0xCB06-四段齐", r.complete(), "");
    }

    // 0xCB03 重演确定
    {
        set.add("0xCB03-同指纹过", ScriptRunner::replay_ok(777, 777), "");
        set.add("0xCB03-异指纹分歧", !ScriptRunner::replay_ok(777, 778), "");
    }

    // 0xCB01 三故障
    {
        set.add("0xCB01-断链是P0", Fault::ChainBreak.is_p0(), "");
        set.add("0xCB01-风暴非P0", !Fault::Storm.is_p0(), "");
    }

    // 0xCB04 三分流
    {
        let mut l = TicketList::new();
        let mut t = Ticket::new(Fault::ChainBreak);
        t.action = Triage::Case;
        l.push(t);
        set.add("0xCB01-P0须立即修", l.non_compliant() == 1, "");
        let mut t2 = Ticket::new(Fault::ChainBreak);
        t2.action = Triage::FixNow;
        l.push(t2);
        set.add("0xCB01-P0立即修过", l.non_compliant() == 1, "");
        let mut t3 = Ticket::new(Fault::Storm);
        t3.owned = false;
        t3.action = Triage::Case;
        l.push(t3);
        set.add("0xCB04-无主须兜底", l.non_compliant() == 2, "");
        let mut t4 = Ticket::new(Fault::Storm);
        t4.owned = false;
        t4.action = Triage::Fallback;
        l.push(t4);
        set.add("0xCB04-兜底合规", l.non_compliant() == 2 && l.unowned() == 2, "");
    }

    // 场景序自洽
    {
        let mut ok = true;
        for (i, s) in Scene::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xCB05-场景序自洽", ok, "");
    }

    set
}
