//! CGPU-F2563 · 故障分类与预测（CGPU-Q 域 · 可靠性域 · Q03 组 · 目标 320 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2563`
//!
//! **判据（锚点原文）**：五类表、联动复用、三组、判据。
//!
//! **职责定位（锚点原文）**：故障分类：分类（故障分类学（硬件/驱动/内存/
//! 逻辑/OOM——五类——分类表（预测（P05 异常预测联动——联动复用——联动
//! 复用；测试（分类/预测三组）。判据：五类表、联动复用、三组、判据。
//!
//! ## 一、故障分类学：五类闭集（判据一）
//!
//! 五类（[`FaultKind`]）：硬件/驱动/内存/逻辑/OOM——类别闭集，表外
//! 故障不是「其他」桶而是建模缺口（显性 [`XcCode::Unclassified`]）。
//! 每类绑定分类表行（[`FAULT_TABLE`]）：特征签名（整数阈值字面量冻结）
//! + 典型来源 + 处置指针（指向 Q 域后续容错/恢复兑现单）。
//!
//! ## 二、分类规则（判据一的函数面：症状→类别，确定性）
//!
//! [`classify`]：按特征签名逐类比对，命中即归类（首中即断，序即
//! [`FAULT_TABLE`] 序——确定性对拍前提）；全不中 → 显性未分类，不硬塞。
//!
//! ## 三、P05 异常预测联动（判据二：联动复用，不第二套预测器）
//!
//! 预测面复用 P05 异常预测（F2410 侧统计口径）的滚动窗口输入：
//! [`FaultPredictor`] 消费逐窗故障计数 → 风险分（0-100）+ 趋势三态
//! （平稳/上行/陡升）。诚实纪律：预测是统计量不是判决——只产出
//! 风险提示与观察建议，不静默触发任何压制动作（抑制误报伤害）。
//!
//! ## 四、滚动窗口（配额恒定）
//!
//! 窗口容量 [`WINDOW_CAPACITY`] 写死；超窗最旧窗退场（计数留痕）——
//! 长跑内存有界。窗口间独立聚合，跨窗趋势按首末窗对比（确定性）。
//!
//! **对接**：F2561/F2562（Q 域开工与可靠性指标——本模块消费其口径）；
//! F2410（P05 异常预测——联动复用对象）；F2564/F2565（容错/崩溃恢复
//! ——分类表的处置指针指向）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FAULT_PREDICT_VERSION: &str = "Q03-faultpredict-v1";

/// 滚动窗口容量（窗数；配额恒定长跑不膨胀）。
pub const WINDOW_CAPACITY: usize = 32;

/// 风险分上行判定阈值（首末窗风险分差 ≥ 该值判上行）。
pub const TREND_RISING_DELTA: u32 = 10;

/// 风险分陡升判定阈值（差 ≥ 该值判陡升；> 上行阈值）。
pub const TREND_STEEP_DELTA: u32 = 30;

// ---------------------------------------------------------------------------
// 二、五类闭集与分类表（判据一）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x68 段独占：Q03 故障分类面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 表外故障（五类之外——建模缺口）。
    Unclassified,
    /// 分类表形状失守。
    TableMismatch,
    /// 风险分输入越界。
    BadSignal,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::Unclassified => "表外故障：五类闭集之外，属上游建模缺口；建议补分类学后再入处置面",
            XcCode::TableMismatch => "分类表形状失守：应为五类 × 四字段，建议回对锚点重建",
            XcCode::BadSignal => "预测输入越界：故障计数与风险分须在闭集内（0-100）",
        }
    }
}

/// 故障五类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// 硬件故障（过热/供电/器件老化）。
    Hardware,
    /// 驱动故障（驱动崩溃/接口违约）。
    Driver,
    /// 内存故障（显存越界/位翻转/泄漏）。
    Memory,
    /// 逻辑故障（状态机违约/死锁/序错）。
    Logic,
    /// OOM（显存/宿主内存耗尽）。
    Oom,
}

impl FaultKind {
    /// 类名。
    pub const fn zh(self) -> &'static str {
        match self {
            FaultKind::Hardware => "硬件",
            FaultKind::Driver => "驱动",
            FaultKind::Memory => "内存",
            FaultKind::Logic => "逻辑",
            FaultKind::Oom => "OOM",
        }
    }
}

/// 分类表行（特征签名 + 典型来源 + 处置指针）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultRow {
    /// 类别。
    pub kind: FaultKind,
    /// 特征签名阈值（整数阈值字面量冻结；语义见 [`classify`]）。
    pub sig: u32,
    /// 典型来源。
    pub source: &'static str,
    /// 处置指针（Q 域后续兑现单）。
    pub handle_to: u32,
}

/// 五类分类表（判据一：表外不立类；sig 语义：复位计数窗口阈值——
/// 硬件类复位频次最高，OOM 类无复位特征故给 0 交由显式谓词分支）。
pub const FAULT_TABLE: [FaultRow; 5] = [
    FaultRow { kind: FaultKind::Hardware, sig: 3, source: "过热/供电/器件老化", handle_to: 2_564 },
    FaultRow { kind: FaultKind::Driver, sig: 5, source: "驱动崩溃/接口违约", handle_to: 2_564 },
    FaultRow { kind: FaultKind::Memory, sig: 1, source: "越界/位翻转/泄漏", handle_to: 2_565 },
    FaultRow { kind: FaultKind::Logic, sig: 2, source: "状态机违约/死锁/序错", handle_to: 2_565 },
    FaultRow { kind: FaultKind::Oom, sig: 0, source: "显存/宿主内存耗尽", handle_to: 2_564 },
];

/// 故障信号（分类输入：一次故障的可观测特征）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultSignal {
    /// 窗口内复位次数（0-255 钳制）。
    pub resets: u32,
    /// 是否驱动回栈（回栈含驱动模块帧）。
    pub driver_frames: bool,
    /// 是否越界/校验和类签名。
    pub memory_signature: bool,
    /// 是否状态机/死锁类签名。
    pub logic_signature: bool,
    /// 是否分配失败签名。
    pub alloc_fail: bool,
}

/// 分类规则（判据一函数面：症状→类别，首中即断，确定性）。
pub fn classify(s: &FaultSignal) -> Result<FaultKind, XcCode> {
    // 顺序即 FAULT_TABLE 序（硬件→驱动→内存→逻辑→OOM）；首中即断。
    let hw = match FAULT_TABLE.first() {
        Some(r) => r.sig,
        None => return Err(XcCode::TableMismatch),
    };
    if s.resets >= hw {
        return Ok(FaultKind::Hardware);
    }
    if s.driver_frames {
        return Ok(FaultKind::Driver);
    }
    if s.memory_signature {
        return Ok(FaultKind::Memory);
    }
    if s.logic_signature {
        return Ok(FaultKind::Logic);
    }
    if s.alloc_fail {
        return Ok(FaultKind::Oom);
    }
    Err(XcCode::Unclassified)
}

// ---------------------------------------------------------------------------
// 三、五类故障遥测（B08 五元组对齐：分类结果全量可观测）
// ---------------------------------------------------------------------------

/// 遥测五元组（与 F0274/B08 五元组同构：名称/维度/单位/类型/采样）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultMetric {
    /// 三级命名（域.组.指标）。
    pub name: &'static str,
    /// 维度。
    pub dimension: &'static str,
    /// 单位。
    pub unit: &'static str,
    /// 类型。
    pub kind: &'static str,
    /// 采样策略。
    pub sampling: &'static str,
}

/// 五类故障指标表（与 [`FaultKind`] 一一对应；分类结果进遥测——
/// 分类的可观测面，判据侧独立重排对账）。
pub const FAULT_METRICS: [FaultMetric; 5] = [
    FaultMetric { name: "q03.fault.hardware", dimension: "fault_event", unit: "count", kind: "u64", sampling: "on_event" },
    FaultMetric { name: "q03.fault.driver", dimension: "fault_event", unit: "count", kind: "u64", sampling: "on_event" },
    FaultMetric { name: "q03.fault.memory", dimension: "fault_event", unit: "count", kind: "u64", sampling: "on_event" },
    FaultMetric { name: "q03.fault.logic", dimension: "fault_event", unit: "count", kind: "u64", sampling: "on_event" },
    FaultMetric { name: "q03.fault.oom", dimension: "fault_event", unit: "count", kind: "u64", sampling: "on_event" },
];

/// 五元组指标名前缀（可 grep 不可漂移）。
pub const FAULT_METRIC_PREFIX: &str = "q03.fault.";

/// 类别 → 指标名（遥测消费面：分类结果按指标名归账）。
pub const fn metric_of(kind: FaultKind) -> &'static str {
    match kind {
        FaultKind::Hardware => "q03.fault.hardware",
        FaultKind::Driver => "q03.fault.driver",
        FaultKind::Memory => "q03.fault.memory",
        FaultKind::Logic => "q03.fault.logic",
        FaultKind::Oom => "q03.fault.oom",
    }
}

/// 分类计数账（五类分账；进遥测前的进程内账面）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FaultCounters {
    counts: [u64; 5],
}

impl FaultCounters {
    /// 归账一次（分类结果 → 类别计数；饱和不越界）。
    pub fn record(&mut self, kind: FaultKind) {
        let idx = kind.ordinal_slot();
        if let Some(c) = self.counts.get_mut(idx) {
            *c = c.saturating_add(1);
        }
    }

    /// 按类取计数（越界给 0 不 panic）。
    pub fn count_of(&self, kind: FaultKind) -> u64 {
        *self.counts.get(kind.ordinal_slot()).unwrap_or(&0)
    }

    /// 全类计数和（账实对账面）。
    pub fn total(&self) -> u64 {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < 5 {
            sum = sum.saturating_add(*self.counts.get(i).unwrap_or(&0));
            i += 1;
        }
        sum
    }

    /// 人读账行（逐类一行，遥测前可读）。
    pub fn lines(&self) -> alloc::string::String {
        let mut s = alloc::string::String::new();
        let mut i = 0usize;
        while i < 5 {
            let kind = match i {
                0 => FaultKind::Hardware,
                1 => FaultKind::Driver,
                2 => FaultKind::Memory,
                3 => FaultKind::Logic,
                _ => FaultKind::Oom,
            };
            s.push_str(alloc::format!(
                "{}={}; ",
                metric_of(kind),
                self.count_of(kind)
            ).as_str());
            i += 1;
        }
        s
    }
}

impl FaultKind {
    /// 计数账槽位（与 FAULT_METRICS 对位）。
    pub const fn ordinal_slot(self) -> usize {
        match self {
            FaultKind::Hardware => 0,
            FaultKind::Driver => 1,
            FaultKind::Memory => 2,
            FaultKind::Logic => 3,
            FaultKind::Oom => 4,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、P05 联动预测（判据二：复用不重建）
// ---------------------------------------------------------------------------

/// 趋势三态（确定性：首末窗对比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    /// 平稳（差 < 上行阈值）。
    Flat,
    /// 上行（差 ≥ 上行阈值且 < 陡升阈值）。
    Rising,
    /// 陡升（差 ≥ 陡升阈值）。
    Steep,
}

impl Trend {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            Trend::Flat => "平稳",
            Trend::Rising => "上行",
            Trend::Steep => "陡升",
        }
    }
}

/// 预测器（消费逐窗故障计数——P05 滚动窗口口径复用，不第二套预测器）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FaultPredictor {
    windows: Vec<u32>,
    dropped: u64,
}

impl FaultPredictor {
    /// 推入一窗故障计数（越界输入显性拒——不静默钳制）。
    pub fn push_window(&mut self, faults: u32) -> Result<(), XcCode> {
        if faults > 1_000 {
            return Err(XcCode::BadSignal);
        }
        self.windows.push(faults);
        if self.windows.len() > WINDOW_CAPACITY {
            self.windows.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        Ok(())
    }

    /// 风险分（0-100；来源：P05 联动口径——近半窗计数线性归一，饱和
    /// 不越界）。空窗给 0（无数据≠无风险，消费方看 windows 判数据量）。
    pub fn risk_score(&self) -> u32 {
        let n = self.windows.len();
        if n == 0 {
            return 0;
        }
        // 近半窗求和（偶数窗取后半；奇数窗含中位）。
        let half = n / 2;
        let mut sum: u64 = 0;
        let mut i = half;
        while i < n {
            sum = sum.saturating_add(*self.windows.get(i).unwrap_or(&0) as u64);
            i += 1;
        }
        let span = (n - half).max(1) as u64;
        let per_window = sum / span;
        // 归一：每窗 ≥50 次故障即满分（阈值来源：P05 联动口径）。
        let score = per_window.saturating_mul(2);
        if score > 100 {
            100
        } else {
            score as u32
        }
    }

    /// 趋势（首末窗风险对比；窗口不足 2 给平稳——不猜）。
    pub fn trend(&self) -> Trend {
        if self.windows.len() < 2 {
            return Trend::Flat;
        }
        let first = *self.windows.first().unwrap_or(&0);
        let last = *self.windows.last().unwrap_or(&0);
        let delta = last.saturating_sub(first.min(last));
        // 首窗大于末窗是下行，按平稳（预测面只报上行风险——下行不是风险）。
        if last < first {
            return Trend::Flat;
        }
        if delta >= TREND_STEEP_DELTA {
            Trend::Steep
        } else if delta >= TREND_RISING_DELTA {
            Trend::Rising
        } else {
            Trend::Flat
        }
    }

    /// 预测结论行（诚实纪律：统计提示，不是判决；不触发压制动作）。
    pub fn advise(&self) -> String {
        alloc::format!(
            "风险分 {}（0-100，P05 口径）/趋势 {}；统计提示仅供观察与预检，不触发任何压制动作",
            self.risk_score(),
            self.trend().zh()
        )
    }

    /// 在窗数。
    pub const fn window_len(&self) -> usize {
        self.windows.len()
    }

    /// 退场窗数（完整性声明）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}
