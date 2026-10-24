//! CGPU-F2408 · 遥测性能（CGPU-P 域 · 自适应遥测域 · P02 组）。
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
//! 标注同构）。逐段告警三级滞回（[`AlarmState`]：正常→关注→告警——
//! 升级需连续超标 [`ALARM_ESCALATE_HITS`] 帧，降级需连续达标
//! [`ALARM_DEESCALATE_HITS`] 帧——单帧抖动不触发告警，滞回防抖）。
//!
//! ## 二、开销复用：预算步与红线口径不第二套
//!
//! **开销复用**（判据二）：预算步=0.1%帧预算的换算口径复用 F0482
//! （640ppm 实测核算法）；采样开销红线复用 F1558「采样不耗样本」
//! L0≤0.05ms 的红线声明；总预算占比核算复用 F2401「不做帧预算隐形
//! 税」——三处口径本文件只引用不重定义，判据侧字面量对拍。换算器
//! 三口径（[`ns_to_steps`] / [`steps_to_ns`] / [`steps_to_ppm`]）——
//! ns↔步↔万分比互通可复核，换算不是黑箱。
//!
//! ## 三、总账闭合与逐帧回归
//!
//! [`PerfLedger::report`]：Σmeasured 与 Σbudget 各自有数，占比
//! 万分比整数口径（复用 F2562 可用性万分比口径），恰红线行为钉死
//! （== 预算即达标「≤」语义；超 1 步即超标标注）。逐帧性能簿
//! （[`FramePerfBook`]）保留近期帧快照并做**回归检测**：某段实测
//! 连续 [`REGRESSION_WINDOW`] 帧上升且最新帧超预算 → 回归立案
//! （[`Regression`]，附段名与升幅）——性能劣化不是只看单帧，是看
//! 趋势；预算值变更走 ADR（[`BudgetChangeLedger`]——「当前策略值，
//! 可调」的调字有账）。
//!
//! ## 四、指标归属摊销与采样频档
//!
//! 指标归属表（[`METRIC_OWNER`]：遥测指标 → 归属段 → 预估步）：
//! 总账超标时按归属摊销到指标面——「哪段超」细到「哪批指标超」；
//! 采样频档（[`SamplingTier`]：活跃 10Hz/常速 1Hz/空闲 0.1Hz）带
//! 预期开销折算（[`SamplingTier::expected_factor_bps`]）——频档是
//! 超标处置的第一杠杆（建议面的量化依据）。
//!
//! **对接**：F0482（实测注入口径）；F1558（红线声明）；F2401（预算
//! 意识使命）；F2402-F2406（五段设施本体——预算行的归属方）。
//! 零 panic 面、零 IO、零墙钟（measured 全上游注入）、no_std。

use alloc::format;
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

    /// 占预算万分比（budget=0 给 None——不除零不产幻觉）。
    pub const fn usage_bps(&self) -> Option<u32> {
        if self.budget == 0 {
            None
        } else {
            Some(self.measured.saturating_mul(10_000) / self.budget)
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

/// 预算总步（Σbudget=14；判据侧独立重算对拍锚点）。
pub const BUDGET_SUM: u32 = 14;

const _: () = assert!(
    BUDGET_TABLE[0].budget + BUDGET_TABLE[1].budget + BUDGET_TABLE[2].budget
        + BUDGET_TABLE[3].budget + BUDGET_TABLE[4].budget
        == BUDGET_SUM
);

/// 帧预算总步（占比核算的分母——万 10000 步口径，复用 F0482 640ppm
/// 换算同构：遥测五段总预算占帧预算万分之 140）。
pub const FRAME_BUDGET_STEPS: u32 = 10000;

// ---------------------------------------------------------------------------
// 二、性能总账（实测注入 + 超标标注 + 占比核算 + 告警滞回）
// ---------------------------------------------------------------------------

/// 告警升级所需连续超标帧数（滞回升阈——单帧抖动不告警）。
pub const ALARM_ESCALATE_HITS: u32 = 3;

/// 告警降级所需连续达标帧数（滞回降阈——恢复需确认稳定）。
pub const ALARM_DEESCALATE_HITS: u32 = 8;

/// 告警三级（滞回状态机：正常→关注→告警）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlarmState {
    /// 正常（预算内）。
    Normal,
    /// 关注（1-2 帧连续超标——观察态）。
    Watch,
    /// 告警（≥3 帧连续超标——处置态）。
    Alarming,
}

impl AlarmState {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            AlarmState::Normal => "正常",
            AlarmState::Watch => "关注",
            AlarmState::Alarming => "告警",
        }
    }

    /// 严重度序（遥测维度）。
    pub const fn severity(self) -> u8 {
        match self {
            AlarmState::Normal => 0,
            AlarmState::Watch => 1,
            AlarmState::Alarming => 2,
        }
    }
}

/// 采样频档（超标处置的第一杠杆；预期开销折算随档量化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplingTier {
    /// 活跃档：10Hz 全量。
    Active,
    /// 常速档：1Hz 常规。
    Normal,
    /// 空闲档：0.1Hz 保底。
    Idle,
}

/// 频档基准 Hz ×10（整数口径：活跃 100=10Hz、常速 10=1Hz、空闲 1=0.1Hz）。
pub const SAMPLING_TIER_HZ_X10: [u32; 3] = [100, 10, 1];

impl SamplingTier {
    /// 预期开销折算万分比（相对活跃档=10000；常速≈1000、空闲≈100——
    /// 线性折算口径，供建议面量化「降档省多少」）。
    pub const fn expected_factor_bps(self) -> u32 {
        match self {
            SamplingTier::Active => 10_000,
            SamplingTier::Normal => 1_000,
            SamplingTier::Idle => 100,
        }
    }

    /// 档位序（0-2）。
    pub const fn index(self) -> usize {
        match self {
            SamplingTier::Active => 0,
            SamplingTier::Normal => 1,
            SamplingTier::Idle => 2,
        }
    }

    /// 从序号还原（越界回常速——保守默认）。
    pub const fn from_index(i: usize) -> SamplingTier {
        match i {
            0 => SamplingTier::Active,
            2 => SamplingTier::Idle,
            _ => SamplingTier::Normal,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            SamplingTier::Active => "活跃档（10Hz）",
            SamplingTier::Normal => "常速档（1Hz）",
            SamplingTier::Idle => "空闲档（0.1Hz）",
        }
    }

    /// 降一档（空闲档不再降）。
    pub const fn step_down(self) -> SamplingTier {
        match self {
            SamplingTier::Active => SamplingTier::Normal,
            _ => SamplingTier::Idle,
        }
    }

    /// 降档后的预算步预期（当前步 × 折算万分比 ÷ 10000，饱和）。
    pub const fn expected_steps(self, current_steps: u32) -> u32 {
        (current_steps.saturating_mul(self.expected_factor_bps()) / 10_000) as u32
    }
}

/// 单段历史（滚动实测窗口 + 趋势 + 告警滞回状态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentHistory {
    /// 段名。
    pub name: &'static str,
    /// 滚动实测窗口（最新在尾；容量 [`HISTORY_CAPACITY`]）。
    window: Vec<u32>,
    /// 当前告警态。
    pub alarm: AlarmState,
    /// 连续超标帧数（滞回升计）。
    over_streak: u32,
    /// 连续达标帧数（滞回降计）。
    within_streak: u32,
    /// 历史最高实测（预警面）。
    peak: u32,
}

/// 逐段历史窗口容量（趋势与 P95 的取数窗；配额恒定）。
pub const HISTORY_CAPACITY: usize = 32;

/// 回归判定窗口（连续上升帧数阈值）。
pub const REGRESSION_WINDOW: usize = 4;

impl SegmentHistory {
    /// 新历史（段名 + 预算行初始态）。
    pub fn new(name: &'static str) -> SegmentHistory {
        SegmentHistory {
            name,
            window: Vec::new(),
            alarm: AlarmState::Normal,
            over_streak: 0,
            within_streak: 0,
            peak: 0,
        }
    }

    /// 推入一帧实测（窗口滚动 + 滞回推进 + 峰值更新）。
    pub fn push(&mut self, measured: u32, budget: u32) {
        self.window.push(measured);
        if self.window.len() > HISTORY_CAPACITY {
            self.window.remove(0);
        }
        if measured > self.peak {
            self.peak = measured;
        }
        if measured > budget {
            self.over_streak = self.over_streak.saturating_add(1);
            self.within_streak = 0;
            // 滞回升级：连续超标达升阈 → 告警；1 帧起 → 关注。
            if self.over_streak >= ALARM_ESCALATE_HITS {
                self.alarm = AlarmState::Alarming;
            } else if self.alarm == AlarmState::Normal {
                self.alarm = AlarmState::Watch;
            }
        } else {
            self.within_streak = self.within_streak.saturating_add(1);
            self.over_streak = 0;
            // 滞回降级：连续达标达降阈 → 正常；告警态先回关注。
            if self.within_streak >= ALARM_DEESCALATE_HITS {
                self.alarm = AlarmState::Normal;
            } else if self.alarm == AlarmState::Alarming {
                self.alarm = AlarmState::Watch;
            }
        }
    }

    /// 窗口快照（只读）。
    pub fn window(&self) -> &[u32] {
        &self.window
    }

    /// 历史最高实测。
    pub const fn peak(&self) -> u32 {
        self.peak
    }

    /// 均值（空窗 None）。
    pub fn mean(&self) -> Option<u32> {
        if self.window.is_empty() {
            return None;
        }
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.window.len() {
            if let Some(v) = self.window.get(i) {
                sum = sum.saturating_add(*v as u64);
            }
            i += 1;
        }
        Some((sum / self.window.len() as u64) as u32)
    }

    /// P95（窗口内升序取 95 分位——小窗插入排序，确定性零分配递归；
    /// 空窗 None）。
    pub fn p95(&self) -> Option<u32> {
        if self.window.is_empty() {
            return None;
        }
        let mut sorted: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.window.len() {
            if let Some(v) = self.window.get(i) {
                // 插入排序（升序；窗口 ≤32——代价有界）。
                let mut pos = 0usize;
                while pos < sorted.len() {
                    if let Some(s) = sorted.get(pos) {
                        if *s >= *v {
                            break;
                        }
                    }
                    pos += 1;
                }
                sorted.insert(pos, *v);
            }
            i += 1;
        }
        let idx = (sorted.len().saturating_sub(1)).saturating_mul(95) / 100;
        sorted.get(idx).copied()
    }

    /// 趋势（最近 n=REGRESSION_WINDOW 帧是否单调不减且至少一升——
    /// 回归前兆；窗口不足给 None 不臆断）。
    pub fn trend_rising(&self) -> Option<bool> {
        if self.window.len() < REGRESSION_WINDOW {
            return None;
        }
        let start = self.window.len().saturating_sub(REGRESSION_WINDOW);
        let mut rising = false;
        let mut i = start;
        while i + 1 < self.window.len() {
            let a = *self.window.get(i)?;
            let b = *self.window.get(i + 1)?;
            if b < a {
                return Some(false);
            }
            if b > a {
                rising = true;
            }
            i += 1;
        }
        Some(rising)
    }
}

/// 性能总账（五段预算行 + 实测注入——超标留名可查 + 逐段历史）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerfLedger {
    lines: [BudgetLine; 5],
    histories: [SegmentHistory; 5],
    /// 当前采样频档（超标处置的第一杠杆的当前位）。
    pub tier: SamplingTier,
}

impl PerfLedger {
    /// 新总账（从预算表克隆——measured 归零待注入）。
    pub fn new() -> PerfLedger {
        PerfLedger {
            lines: BUDGET_TABLE,
            histories: [
                SegmentHistory::new(BUDGET_TABLE[0].name),
                SegmentHistory::new(BUDGET_TABLE[1].name),
                SegmentHistory::new(BUDGET_TABLE[2].name),
                SegmentHistory::new(BUDGET_TABLE[3].name),
                SegmentHistory::new(BUDGET_TABLE[4].name),
            ],
            tier: SamplingTier::Active,
        }
    }

    /// 实测注入（按段名定位；表外段名 None 不臆造；注入即入段历史）。
    pub fn measure(&mut self, name: &str, steps: u32) -> Option<()> {
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get_mut(i) {
                if l.name == name {
                    l.measured = steps;
                    if let Some(h) = self.histories.get_mut(i) {
                        h.push(steps, l.budget);
                    }
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

    /// 段历史（按段名取；表外 None）。
    pub fn history_of(&self, name: &str) -> Option<&SegmentHistory> {
        let mut i = 0usize;
        while i < self.histories.len() {
            if let Some(h) = self.histories.get(i) {
                if h.name == name {
                    return Some(h);
                }
            }
            i += 1;
        }
        None
    }

    /// 告警段清单（告警态 ≥ 关注的段——处置面）。
    pub fn alarmed(&self) -> Vec<(&'static str, AlarmState)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.histories.len() {
            if let Some(h) = self.histories.get(i) {
                if h.alarm != AlarmState::Normal {
                    out.push((h.name, h.alarm));
                }
            }
            i += 1;
        }
        out
    }

    /// 回归立案清单（趋势上升 + 当前超标——性能劣化立案面）。
    pub fn regressions(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.histories.len() {
            let h = match self.histories.get(i) {
                Some(h) => h,
                None => break,
            };
            let line = match self.lines.get(i) {
                Some(l) => l,
                None => break,
            };
            if h.trend_rising() == Some(true) && !line.within() {
                out.push(h.name);
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

    /// 降档预期（降一档后的总步预估——建议面的量化依据；只预估不落账）。
    pub fn projected_steps_after_downshift(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get(i) {
                sum = sum.saturating_add(self.tier.step_down().expected_steps(l.measured));
            }
            i += 1;
        }
        sum
    }

    /// 帧报告（汇总快照——收单报告与遥测消费面）。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str(&alloc::format!(
            "P08 总账：实测 {}ppm / 预算 {}ppm / 频档 {}",
            self.total_ppm_of_frame(),
            self.budget_ppm_of_frame(),
            self.tier.zh()
        ));
        let over_names = self.over();
        if over_names.is_empty() {
            s.push_str("；五段全部达标");
        } else {
            s.push_str("；超标：");
            let mut i = 0usize;
            while i < over_names.len() {
                if let Some(n) = over_names.get(i) {
                    if i > 0 {
                        s.push_str("、");
                    }
                    s.push_str(n);
                }
                i += 1;
            }
        }
        let alarms = self.alarmed();
        if !alarms.is_empty() {
            s.push_str("；告警：");
            let mut i = 0usize;
            while i < alarms.len() {
                if let Some((n, a)) = alarms.get(i) {
                    if i > 0 {
                        s.push_str("、");
                    }
                    s.push_str(&alloc::format!("{}={}", n, a.zh()));
                }
                i += 1;
            }
        }
        s
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
// 四、预算步换算器（开销复用的换算面：ns ↔ 步 ↔ ppm，确定性）
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

/// 预算步 → 帧预算万分比（steps×10000÷FRAME_BUDGET_STEPS——三口径
/// 互通的第三向；饱和不越界）。
pub const fn steps_to_ppm(steps: u32) -> u32 {
    steps.saturating_mul(10_000) / FRAME_BUDGET_STEPS
}

/// 万分比 → 预算步（ppm×FRAME_BUDGET_STEPS÷10000——反向换算；饱和）。
pub const fn ppm_to_steps(ppm: u32) -> u32 {
    ppm.saturating_mul(FRAME_BUDGET_STEPS) / 10_000
}

/// F1558 红线换算核对：L0≤0.05ms = 50_000ns = 4 步（50_000 ÷ 12_500
/// 整除）——红线声明在预算步口径下的落点（换算可复核，不是黑箱）。
pub const F1558_L0_STEPS: u32 = 4;

/// F0482 实测注入参考口径（640ppm——复用声明第二处的数值面）。
pub const F0482_REFERENCE_PPM: u32 = 640;

// ---------------------------------------------------------------------------
// 五、指标归属摊销（总账超标的指标级归因）
// ---------------------------------------------------------------------------

/// 指标归属行（遥测指标 → 归属段 → 预估步——超标摊销的取数面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricOwner {
    /// 指标名（d02/p02 三级命名尾段——消费方按名对账）。
    pub metric: &'static str,
    /// 归属段名（∈ BUDGET_TABLE 五段）。
    pub segment: &'static str,
    /// 单帧预估步（指标采样摊到帧的预算步——从严声明值）。
    pub est_steps: u32,
}

/// 指标归属表（逐指标归属段与预估步——摊销口径单源）。
pub const METRIC_OWNER: [MetricOwner; 6] = [
    MetricOwner { metric: "model_construct", segment: "统一模型构造", est_steps: 2 },
    MetricOwner { metric: "sample_decision", segment: "采样决策", est_steps: 2 },
    MetricOwner { metric: "pipe_collect", segment: "管道四段", est_steps: 2 },
    MetricOwner { metric: "pipe_emit", segment: "管道四段", est_steps: 2 },
    MetricOwner { metric: "layer_down", segment: "分层降层", est_steps: 3 },
    MetricOwner { metric: "query_read", segment: "查询", est_steps: 3 },
];

/// 按段摊销：给定段名，返回归属该段的全部指标与预估步
/// （表外段给空——不臆造）。
pub fn amortize_by_segment(segment: &str) -> Vec<(&'static str, u32)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < METRIC_OWNER.len() {
        if let Some(m) = METRIC_OWNER.get(i) {
            if m.segment == segment {
                out.push((m.metric, m.est_steps));
            }
        }
        i += 1;
    }
    out
}

/// 段级指标预估合计（段超标时与该段实测对照——「预估 vs 实测」的
/// 归因差值面）。
pub fn segment_est_sum(segment: &str) -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < METRIC_OWNER.len() {
        if let Some(m) = METRIC_OWNER.get(i) {
            if m.segment == segment {
                sum = sum.saturating_add(m.est_steps);
            }
        }
        i += 1;
    }
    sum
}

// ---------------------------------------------------------------------------
// 六、超标处置建议与滚动超标账（错误路径带可操作建议——深度要求）
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

/// 建议覆盖守恒：五段每段有非空建议（判据面——建议表与预算表对齐）。
pub fn suggestions_cover_all() -> bool {
    let mut i = 0usize;
    while i < BUDGET_TABLE.len() {
        let name = match BUDGET_TABLE.get(i) {
            Some(l) => l.name,
            None => return false,
        };
        let sug = over_suggest(name);
        if sug.is_empty() || sug.starts_with("表外段") {
            return false;
        }
        i += 1;
    }
    true
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

    /// 某段超标频次（万分比：该段超标事件 / 全部事件；无事件 None）。
    pub fn over_rate_bps(&self, name: &str) -> Option<u32> {
        if self.events.is_empty() {
            return None;
        }
        let mut hits = 0u32;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(ev) = self.events.get(i) {
                if ev.name == name {
                    hits = hits.saturating_add(1);
                }
            }
            i += 1;
        }
        Some(hits.saturating_mul(10_000) / self.events.len() as u32)
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

/// 预算变更 ADR 账（预算是「当前策略值，可调」——调的每一步有账：
/// 段名/旧值/新值/动因/tick；无因之调显性拒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetChange {
    /// 段名。
    pub segment: &'static str,
    /// 旧预算步。
    pub old_budget: u32,
    /// 新预算步。
    pub new_budget: u32,
    /// 动因（非空——无因不调）。
    pub reason: String,
    /// 变更 tick。
    pub tick: u64,
}

/// 预算变更账（append-only；重估建议落地时的登记面）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BudgetChangeLedger {
    changes: Vec<BudgetChange>,
}

impl BudgetChangeLedger {
    /// 登记变更（动因空/新旧同值显性拒——不产空转账）。
    pub fn record(
        &mut self,
        segment: &'static str,
        old_budget: u32,
        new_budget: u32,
        reason: String,
        tick: u64,
    ) -> Result<(), &'static str> {
        if reason.trim().is_empty() {
            return Err("预算变更动因为空：调预算必须说明动因");
        }
        if old_budget == new_budget {
            return Err("预算新旧同值：无变化不构成变更，拒绝空转 ADR");
        }
        self.changes.push(BudgetChange {
            segment,
            old_budget,
            new_budget,
            reason,
            tick,
        });
        Ok(())
    }

    /// 全部变更。
    pub fn changes(&self) -> &[BudgetChange] {
        &self.changes
    }

    /// 某段变更次数。
    pub fn changes_of(&self, segment: &str) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.changes.len() {
            if let Some(c) = self.changes.get(i) {
                if c.segment == segment {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 变更总数。
    pub const fn len_of(&self) -> usize {
        self.changes.len()
    }
}

/// 总账巡检（判据「总账闭合」的运行时面：逐段核对，超标段入超标账
/// 并附处置建议行——一次巡检产出全部超标事实与建议；回归段追加
/// 立案标注）。
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
                    let mut line = alloc::format!(
                        "超标：{} 实测 {} 步 / 预算 {} 步（超 {} 步）；{}",
                        name,
                        l.measured,
                        l.budget,
                        l.over_by(),
                        over_suggest(name)
                    );
                    // 回归立案标注（趋势上升 + 当前超标）。
                    if ledger.regressions().iter().any(|r| *r == name) {
                        line.push_str("；【回归立案】连续上升且超预算——建议归因上游设施变更");
                    }
                    out.push(line);
                    break;
                }
            }
            j += 1;
        }
        i += 1;
    }
    out
}
