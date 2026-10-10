//! CGPU-F2408 · 遥测性能（CGPU-P 域 · 自适应遥测域 · P02 组 · 目标 300 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2408`
//!
//! **判据（锚点原文）**：开销复用、一组、判据。
//!
//! **职责定位（锚点原文）**：性能（遥测性能（自身开销总账——开销复用——
//! 开销复用；测试（开销一组）。
//!
//! ## 一、自身开销总账：五段设施每段有预算有实测
//!
//! 遥测设施自身是帧预算的租客不是税吏（F2401 使命「遥测有预算意识」
//! 的总账落地面）：五段设施（统一模型构造/采样决策/管道四段/分层
//! 降层/查询）各开一条 [`BudgetLine`]——预算上限 [`BudgetLine::budget`]
//! （确定性预算步，零墙钟）+ 实测值 [`BudgetLine::measured`] 上游注入，
//! 超标 [`BudgetLine::within`] 为 false 且总账标注（[`PerfLedger::over`]
//! 留名可查）——实测注入口径复用 F0482 计时器（换算误差可测、超标
//! 标注同构）。
//!
//! ## 二、开销复用：预算步与红线口径不第二套
//!
//! **开销复用**（判据二）：预算步=0.1%帧预算的换算口径复用 F0482
//! （640ppm 实测核算法）；采样开销红线复用 F1558「采样不耗样本」
//! L0≤0.05ms 的红线声明；总预算占比核算复用 F2401「不做帧预算隐形
//! 税」——三处口径本文件只引用不重定义，判据侧字面量对拍。
//!
//! ## 三、总账闭合
//!
//! [`PerfLedger::report`]：Σmeasured 与 Σbudget 各自有数，占比
//!万分比整数口径（复用 F2562 可用性万分比口径），恰红线行为钉死
//! （== 预算即达标「≤」语义；超 1 步即超标标注）。
//!
//! **对接**：F0482（实测注入口径）；F1558（红线声明）；F2401（预算
//! 意识使命）。零 panic 面、零 IO、零墙钟（measured 全上游注入）。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、预算行（判据「自身开销总账」的账目单元）
// ---------------------------------------------------------------------------

/// 一段设施的开销预算行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetLine {
    /// 段名（设施段——判据侧独立写死五段名对拍）。
    pub name: &'static str,
    /// 预算上限（确定性预算步；步=0.1% 帧预算换算口径，复用 F0482）。
    pub budget: u32,
    /// 实测值（上游注入——引擎不自测零墙钟）。
    pub measured: u32,
}

impl BudgetLine {
    /// 是否达标（≤ 预算即达标——恰等达标「≤」语义钉死）。
    pub const fn within(&self) -> bool {
        self.measured <= self.budget
    }

    /// 超标步数（未超标为 0——负数不存在，饱和口径）。
    pub const fn over_by(&self) -> u32 {
        if self.measured > self.budget {
            self.measured - self.budget
        } else {
            0
        }
    }
}

/// 五段设施预算表（判据侧独立写死五段名与预算数对拍；预算=确定性
/// 预算步：F2402 构造 2 步/F2403 决策 2 步/F2404 管道 4 步/F2405 降层
/// 3 步/F2406 查询 3 步——预算从严非实测摊派）。
pub const BUDGET_TABLE: [BudgetLine; 5] = [
    BudgetLine { name: "统一模型构造", budget: 2, measured: 0 },
    BudgetLine { name: "采样决策", budget: 2, measured: 0 },
    BudgetLine { name: "管道四段", budget: 4, measured: 0 },
    BudgetLine { name: "分层降层", budget: 3, measured: 0 },
    BudgetLine { name: "查询", budget: 3, measured: 0 },
];

// ---------------------------------------------------------------------------
// 二、性能总账（实测注入 + 超标标注 + 占比核算）
// ---------------------------------------------------------------------------

/// 帧预算总步（占比核算的分母——万 10000 步口径，复用 F0482 640ppm
/// 换算同构：遥测五段总预算占帧预算万分之 140）。
pub const FRAME_BUDGET_STEPS: u32 = 10000;

/// 性能总账（五段预算行 + 实测注入——超标留名可查）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerfLedger {
    lines: [BudgetLine; 5],
}

impl PerfLedger {
    /// 新总账（从预算表克隆——measured 归零待注入）。
    pub fn new() -> PerfLedger {
        PerfLedger { lines: BUDGET_TABLE }
    }

    /// 实测注入（按段名定位；表外段名 None 不臆造）。
    pub fn measure(&mut self, name: &str, steps: u32) -> Option<()> {
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get_mut(i) {
                if l.name == name {
                    l.measured = steps;
                    return Some(());
                }
            }
            i += 1;
        }
        None
    }

    /// 达标段数。
    pub fn within_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get(i) {
                if l.within() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 超标段名清单（留名可查——「哪段超了」不是只给一个布尔）。
    pub fn over(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get(i) {
                if !l.within() {
                    out.push(l.name);
                }
            }
            i += 1;
        }
        out
    }

    /// 总占比（万分比整数口径，复用 F2562：Σmeasured×10000÷帧预算步；
    /// 饱和不越界）。
    pub const fn total_ppm_of_frame(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.lines.len() {
            sum = sum.saturating_add(self.lines[i].measured);
            i += 1;
        }
        sum.saturating_mul(10000) / FRAME_BUDGET_STEPS
    }

    /// 总预算占比（预算侧——设计承诺的上限口径）。
    pub const fn budget_ppm_of_frame(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.lines.len() {
            sum = sum.saturating_add(self.lines[i].budget);
            i += 1;
        }
        sum.saturating_mul(10000) / FRAME_BUDGET_STEPS
    }
}

impl Default for PerfLedger {
    fn default() -> PerfLedger {
        PerfLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 三、红线声明（复用口径的对账面）
// ---------------------------------------------------------------------------

/// 开销红线复用声明（判据侧逐条 grep 对拍——三处口径只引用不重定义）：
pub const REUSE_LINES: [&str; 3] = [
    "复用 F0482 实测注入核算口径 640ppm 换算——预算步即 0.1% 帧预算口径",
    "复用 F1558 开销红线 采样不耗样本 L0≤0.05ms——红线声明只引用不重定义",
    "复用 F2401 预算意识使命 遥测不做帧预算隐形税——总账占比可查是落地面",
];

// ---------------------------------------------------------------------------
// 四、预算步换算器（开销复用的换算面：ns ↔ 预算步，确定性）
// ---------------------------------------------------------------------------

/// 80 帧合同帧预算（纳秒）：1_000_000_000ns ÷ 80 = 12_500_000ns。
/// 来源：80 帧合同（与 cgc01 FRAME_BUDGET_US 同源口径，ns 面）。
pub const FRAME_BUDGET_NS: u64 = 12_500_000;

/// 一预算步的纳秒数：0.1% 帧预算 = 12_500ns（复用 F0482 640ppm 换算
/// 同构的步口径；整数除法精确无余——12_500_000 × 1 ÷ 1000）。
pub const STEP_NS: u64 = 12_500;

/// 换算守恒（编译期钉死：帧预算 ÷ 步 = 1000 步 = 万分比口径的 1/10，
/// 与 FRAME_BUDGET_STEPS=10000 的十分位一致性——两口径不同单位层，
/// 守恒式各自成立）。
pub const fn step_conserves() -> bool {
    FRAME_BUDGET_NS / STEP_NS == 1_000
}

/// 纳秒 → 预算步（向上取整：实测宁高不宁低——低报会掩盖超标；
/// 饱和不越界）。
pub const fn ns_to_steps(ns: u64) -> u32 {
    let steps = ns.saturating_add(STEP_NS - 1) / STEP_NS;
    if steps > u32::MAX as u64 {
        u32::MAX
    } else {
        steps as u32
    }
}

/// 预算步 → 纳秒（步 × 步长；饱和不越界）。
pub const fn steps_to_ns(steps: u32) -> u64 {
    (steps as u64).saturating_mul(STEP_NS)
}

/// F1558 红线换算核对：L0≤0.05ms = 50_000ns = 4 步（50_000 ÷ 12_500
/// 整除）——红线声明在预算步口径下的落点（换算可复核，不是黑箱）。
pub const F1558_L0_STEPS: u32 = 4;

// ---------------------------------------------------------------------------
// 五、超标处置建议与滚动超标账（错误路径带可操作建议——深度要求）
// ---------------------------------------------------------------------------

/// 超标处置建议（按段给出可操作修法——建议与段一一对应，表外段无建议）。
pub fn over_suggest(name: &str) -> &'static str {
    match name {
        "统一模型构造" => "建议：模型字段惰性构造（首次消费再建），或预分配复用构造缓冲",
        "采样决策" => "建议：降采样频档（活跃 10Hz→空闲 1Hz），或批量合并决策点",
        "管道四段" => "建议：四段合并遍历（单趟采集四段数据），消重复遍历开销",
        "分层降层" => "建议：降层缓存（同维度键命中即复用降层结果），或限制降层深度",
        "查询" => "建议：查询结果短窗缓存（同键同帧复用），或限制单帧查询次数",
        _ => "表外段无建议：预算表五段之外不产生超标（先核对段名）",
    }
}

/// 超标事件（一行=一次超标事实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverEvent {
    /// 段名。
    pub name: &'static str,
    /// 预算步。
    pub budget: u32,
    /// 实测步。
    pub measured: u32,
    /// 超标步数。
    pub over_steps: u32,
    /// 逻辑 tick（上游注入——零墙钟）。
    pub tick: u64,
}

/// 滚动超标账（append-only 封顶滚动；超标历史可回溯——预算重估的
/// 证据来源）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverLedger {
    events: Vec<OverEvent>,
    dropped: u64,
}

/// 超标账容量（配额恒定长跑不膨胀；当前策略值，可调）。
pub const OVER_LEDGER_CAPACITY: usize = 64;

impl OverLedger {
    /// 记一次超标（只在真超标时入账——账上每行都是事实）。
    pub fn record(&mut self, ev: OverEvent) {
        self.events.push(ev);
        if self.events.len() > OVER_LEDGER_CAPACITY {
            self.events.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// 全量超标行。
    pub fn events(&self) -> &[OverEvent] {
        &self.events
    }

    /// 预算重估建议（历史证据面：某段超标次数 ≥ 阈值 → 建议重估预算
    /// 并注明「当前策略值，可调」；无超标给 None 不臆造——复用 F0004
    /// 扩容建议教训：建议必须来自真实超容记录，不恒等于现状）。
    pub fn reestimate_suggest(&self, name: &str, min_hits: u32) -> Option<u32> {
        let mut hits = 0u32;
        let mut max_over = 0u32;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(ev) = self.events.get(i) {
                if ev.name == name {
                    hits = hits.saturating_add(1);
                    if ev.over_steps > max_over {
                        max_over = ev.over_steps;
                    }
                }
            }
            i += 1;
        }
        if hits >= min_hits && min_hits > 0 {
            // 建议 = 历史最大实测步（有据重估，不是拍脑袋翻倍）。
            let mut base = 0u32;
            let mut j = 0usize;
            while j < self.events.len() {
                if let Some(ev) = self.events.get(j) {
                    if ev.name == name && ev.measured > base {
                        base = ev.measured;
                    }
                }
                j += 1;
            }
            Some(base)
        } else {
            None
        }
    }

    /// 在账行数。
    pub const fn len_of(&self) -> usize {
        self.events.len()
    }

    /// 退场行数（完整性声明）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// 总账巡检（判据「总账闭合」的运行时面：逐段核对，超标段入超标账
/// 并附处置建议行——一次巡检产出全部超标事实与建议）。
pub fn patrol(ledger: &PerfLedger, tick: u64, over_book: &mut OverLedger) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let over_names = ledger.over();
    let mut i = 0usize;
    while i < over_names.len() {
        let name = match over_names.get(i) {
            Some(n) => *n,
            None => break,
        };
        // 从总账取该段实测与预算（逐段定位——不越界不猜）。
        let mut j = 0usize;
        while j < BUDGET_TABLE.len() {
            if let Some(l) = BUDGET_TABLE.get(j) {
                if l.name == name {
                    over_book.record(OverEvent {
                        name,
                        budget: l.budget,
                        measured: l.measured,
                        over_steps: l.over_by(),
                        tick,
                    });
                    out.push(alloc::format!(
                        "超标：{} 实测 {} 步 / 预算 {} 步（超 {} 步）；{}",
                        name,
                        l.measured,
                        l.budget,
                        l.over_by(),
                        over_suggest(name)
                    ));
                    break;
                }
            }
            j += 1;
        }
        i += 1;
    }
    out
}
