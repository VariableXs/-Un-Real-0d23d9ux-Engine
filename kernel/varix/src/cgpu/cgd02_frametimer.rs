//! CGPU-F0482 · 全帧精确计时器（CGPU-D 域 · 帧预算与计时域 · D01 组 · 目标 400 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0482`
//!
//! **判据（锚点原文）**：四段、双源、校准、开销、判据。
//!
//! **职责定位（锚点原文）**：帧计时器：四段计时（输入采样→提交→GPU 执行→
//! 呈现上屏——全链分段）；硬件计时源（QPC/GPU 时间戳双源——精度与漂移
//! 控制）；计时校准（CPU/GPU 时钟域对齐——换算误差可测）；计时开销
//! （<0.1% 帧预算——实测达标）。
//!
//! ## 一、四段全链分段：哪段慢了就说哪段
//!
//! 一帧拆四段（[`FrameStage`]：输入采样/提交/GPU 执行/呈现上屏），每段
//! 独立开合（[`FrameTimer::open`]/[`FrameTimer::close`]），闭段才出时长
//! ——没闭的段查时长给 None（不报幻觉数字）。四段流水线在时间轴上
//! 本应顺序衔接，重叠是记账异常不是正常态：[`FrameTimer::has_overlap`]
//! 检出重叠即标注（锚点「全链分段」的可判定化——分段不是摆设，是
//! 能抓出记账错误的账本）。
//!
//! ## 二、双源与校准：两个时钟域一台账
//!
//! CPU 侧 QPC 与 GPU 时间戳是两个时钟域（[`TimestampSource`]）：频率
//! 不同、零点不同、各自漂移。对齐模型（[`ClockAlign`]）：GPU 原始读数
//! 经（比率+偏移）映射进 CPU 域，映射参数由对齐样本对（同一事件在两
//! 域的读数）标定；**换算误差可测**（判据三）：每个样本对的映射残差
//! 逐点给出，残差超阈即标注——校准不是一句「对齐了」，是每个样本都
//! 有一笔可查的误差账。
//!
//! ## 三、开销账：计时器自己也要计时
//!
//! 计时开销 <0.1% 帧预算（判据四）不是口号：每帧开合操作次数 × 单次
//! 操作耗时（实测注入值）÷ 帧预算 = 开销占比（[`overhead_ratio_ppm`]），
//! 超过 1000ppm 即超标标注——计时器是账本不是负载，账本本身超支就
//! 失去了存在的意义。
//!
//! **零墙钟纪律**：本模块不读任何真实时钟——全部时间戳由上游注入
//! （逻辑时间），计时器只做账务、对齐与异常检测；这是 no_std 内核面
//! 的确定性要求（可回放、可测试、无特权时钟依赖）。
//!
//! **对接**：F0481（80 帧合同模型，帧预算口径 12.5ms/帧@80fps）；
//! F0483（帧时间账本，下游记账）；D 域遥测（D07+）。零 panic 面
//! （下标走 `get`/`Option`，算术饱和）、零 IO、零墙钟、无全局可变
//! 状态、no_std 零 std 依赖。

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、规格常量（判据四的口径唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FRAME_TIMER_VERSION: &str = "D01-frametimer-v1";

/// 帧预算（纳秒；80 帧合同口径 12.5ms/帧，同源 F0481 模型）。
pub const FRAME_BUDGET_NS: u64 = 12_500_000;

/// 计时开销红线（每分比 ppm；0.1% = 1000ppm——锚点原文「<0.1%」）。
pub const OVERHEAD_LIMIT_PPM: u64 = 1_000;

/// 双源漂移容差（纳秒；超此值标注漂移异常——精度与漂移控制的判定线）。
pub const DRIFT_TOLERANCE_NS: u64 = 50_000;

/// 校准残差容差（纳秒；换算误差可测的超阈判定线）。
pub const CALIB_RESIDUAL_TOL_NS: i64 = 100_000;

// ---------------------------------------------------------------------------
// 二、四段全链分段（判据一）
// ---------------------------------------------------------------------------

/// 帧四段闭集（锚点原文：输入采样→提交→GPU 执行→呈现上屏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameStage {
    /// 输入采样。
    InputSampling,
    /// 提交。
    Submit,
    /// GPU 执行。
    GpuExecute,
    /// 呈现上屏。
    Present,
}

impl FrameStage {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            FrameStage::InputSampling => "输入采样",
            FrameStage::Submit => "提交",
            FrameStage::GpuExecute => "GPU执行",
            FrameStage::Present => "呈现上屏",
        }
    }

    /// 段槽位（固定四段的下标）。
    pub const fn slot(self) -> usize {
        match self {
            FrameStage::InputSampling => 0,
            FrameStage::Submit => 1,
            FrameStage::GpuExecute => 2,
            FrameStage::Present => 3,
        }
    }
}

/// 四段齐备（判据一的结构断言源）。
pub const FRAME_STAGES: [FrameStage; 4] = [
    FrameStage::InputSampling,
    FrameStage::Submit,
    FrameStage::GpuExecute,
    FrameStage::Present,
];

/// 段计时（开合齐备才有效；时间戳上游注入——零墙钟）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageStamp {
    /// 段。
    pub stage: FrameStage,
    /// 段起点（CPU 域纳秒）。
    pub start_ns: u64,
    /// 段终点（CPU 域纳秒；开而未闭为 None）。
    pub end_ns: Option<u64>,
}

impl StageStamp {
    /// 段时长（未闭段 None——不报幻觉数字）。
    pub fn duration_ns(&self) -> Option<u64> {
        match self.end_ns {
            Some(e) if e >= self.start_ns => Some(e - self.start_ns),
            Some(_) => None, // 终点早于起点：记账异常，不给负数
            None => None,
        }
    }
}

/// 计时异常（重叠/倒序——分段账本的异常账面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerAnomaly {
    /// 段时间轴重叠（流水线顺序段不应重叠）。
    Overlap,
    /// 终点早于起点（记账倒序）。
    Reversed,
}

impl TimerAnomaly {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            TimerAnomaly::Overlap => "段时间轴重叠",
            TimerAnomaly::Reversed => "记账倒序",
        }
    }
}

/// 全帧计时器（四段账本；时间戳上游注入——零墙钟）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameTimer {
    stages: [Option<StageStamp>; 4],
    frame_id: u64,
    anomalies: u32,
}

impl FrameTimer {
    /// 新帧计时器。
    pub const fn new(frame_id: u64) -> FrameTimer {
        FrameTimer {
            stages: [None, None, None, None],
            frame_id,
            anomalies: 0,
        }
    }

    /// 段开（重复开同段覆盖旧账并计异常——开合错序不静默）。
    pub fn open(&mut self, stage: FrameStage, at_ns: u64) {
        let slot = stage.slot();
        if let Some(Some(prev)) = self.stages.get(slot) {
            if prev.end_ns.is_none() {
                self.anomalies = self.anomalies.saturating_add(1);
            }
        }
        if let Some(s) = self.stages.get_mut(slot) {
            *s = Some(StageStamp {
                stage,
                start_ns: at_ns,
                end_ns: None,
            });
        }
    }

    /// 段闭（未开即闭、或终点早于起点——都计异常；闭段才出时长）。
    pub fn close(&mut self, stage: FrameStage, at_ns: u64) {
        let slot = stage.slot();
        let opened = matches!(self.stages.get(slot), Some(Some(_)));
        if !opened {
            self.anomalies = self.anomalies.saturating_add(1);
            return;
        }
        if let Some(Some(s)) = self.stages.get_mut(slot) {
            if at_ns < s.start_ns {
                self.anomalies = self.anomalies.saturating_add(1);
            }
            s.end_ns = Some(at_ns);
        }
    }

    /// 段时长（判据一主查询：O(1) 查表；未闭/异常 None）。
    pub fn duration_ns(&self, stage: FrameStage) -> Option<u64> {
        match self.stages.get(stage.slot()) {
            Some(Some(s)) => s.duration_ns(),
            _ => None,
        }
    }

    /// 四段总时长（全链分段合计；任一段缺账 None）。
    pub fn total_ns(&self) -> Option<u64> {
        let mut total: u64 = 0;
        let mut i = 0usize;
        while i < FRAME_STAGES.len() {
            let d = match FRAME_STAGES.get(i) {
                Some(s) => self.duration_ns(*s),
                None => return None,
            };
            match d {
                Some(v) => total = total.saturating_add(v),
                None => return None,
            }
            i += 1;
        }
        Some(total)
    }

    /// 是否检出段时间轴重叠（判据一「全链分段」的可判定化：相邻闭段
    /// 区间相交即重叠——流水线顺序段不应交叠，交叠即记账异常）。
    pub fn has_overlap(&self) -> bool {
        let mut prev_end: Option<u64> = None;
        let mut i = 0usize;
        while i < FRAME_STAGES.len() {
            let stage = match FRAME_STAGES.get(i) {
                Some(st) => *st,
                None => break,
            };
            let s = match self.stages.get(stage.slot()) {
                Some(Some(s)) => *s,
                _ => {
                    i += 1;
                    continue;
                }
            };
            if let Some(e) = s.end_ns {
                if e >= s.start_ns {
                    if let Some(pe) = prev_end {
                        if s.start_ns < pe {
                            return true;
                        }
                    }
                    prev_end = Some(e);
                }
            }
            i += 1;
        }
        false
    }

    /// 倒序段检测（终点早于起点的段数）。
    pub fn reversed_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FRAME_STAGES.len() {
            let s = match FRAME_STAGES.get(i).and_then(|st| self.stages.get(st.slot())) {
                Some(Some(s)) => *s,
                _ => {
                    i += 1;
                    continue;
                }
            };
            if let Some(e) = s.end_ns {
                if e < s.start_ns {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 帧编号。
    pub const fn frame_id(&self) -> u64 {
        self.frame_id
    }

    /// 异常计数。
    pub const fn anomalies(&self) -> u32 {
        self.anomalies
    }
}

// ---------------------------------------------------------------------------
// 三、双源与校准（判据二、判据三）
// ---------------------------------------------------------------------------

/// 硬件计时源闭集（锚点原文：QPC/GPU 时间戳双源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampSource {
    /// CPU 侧 QPC（高精度性能计数器）。
    CpuQpc,
    /// GPU 时间戳。
    GpuTimestamp,
}

impl TimestampSource {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            TimestampSource::CpuQpc => "CPU-QPC",
            TimestampSource::GpuTimestamp => "GPU时间戳",
        }
    }
}

/// 双源齐备（判据二的结构断言源）。
pub const TIMESTAMP_SOURCES: [TimestampSource; 2] = [
    TimestampSource::CpuQpc,
    TimestampSource::GpuTimestamp,
];

/// 时钟域对齐模型（判据三：GPU 原始读数 → CPU 域纳秒）。
///
/// 映射：`cpu_ns = (gpu_raw - gpu_base) × ratio_ppm / 1_000_000 + cpu_base`。
/// 参数由对齐样本对标定：ratio = Δcpu/Δgpu（千分比×1000=ppm×1000 级，
/// 这里直接用百万分比），offset = cpu_base。**换算误差可测**：每个
/// 样本对的映射残差逐点给出（[`ClockAlign::residuals`]），超阈即异常。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockAlign {
    /// GPU 域基准读数（标定样本首点）。
    pub gpu_base: u64,
    /// CPU 域基准（纳秒）。
    pub cpu_base_ns: u64,
    /// 频率比（百万分比；1_000_000=两域同频）。
    pub ratio_ppm: u64,
}

impl ClockAlign {
    /// 由两个对齐样本对标定（同频理想 ratio=1e6；实测偏差进 ratio）。
    ///
    /// 样本对 (gpu_raw, cpu_ns)。取首对定零点，次对定比率——两对即可
    /// 完全确定线性映射（可判定标定，不做黑盒拟合）。
    pub fn calibrate(
        p0: (u64, u64),
        p1: (u64, u64),
    ) -> Option<ClockAlign> {
        let (g0, c0) = p0;
        let (g1, c1) = p1;
        if g1 <= g0 || c1 <= c0 {
            return None; // 样本必须单调（非单调样本标不出可信映射）
        }
        let dg = g1 - g0;
        let dc = c1 - c0;
        let ratio = dc * 1_000_000 / dg;
        Some(ClockAlign {
            gpu_base: g0,
            cpu_base_ns: c0,
            ratio_ppm: ratio,
        })
    }

    /// GPU 原始读数 → CPU 域纳秒（饱和算术；零 panic 面）。
    pub fn to_cpu_ns(&self, gpu_raw: u64) -> u64 {
        if gpu_raw < self.gpu_base {
            return self.cpu_base_ns;
        }
        let dg = gpu_raw - self.gpu_base;
        let scaled = (dg as u128 * self.ratio_ppm as u128 / 1_000_000) as u64;
        self.cpu_base_ns.saturating_add(scaled)
    }

    /// 样本对换算残差（判据三「换算误差可测」：逐点给账，不汇总掩盖）。
    pub fn residual_ns(&self, gpu_raw: u64, cpu_expect_ns: u64) -> i64 {
        let got = self.to_cpu_ns(gpu_raw) as i64;
        cpu_expect_ns as i64 - got
    }

    /// 残差是否在容差内。
    pub fn residual_ok(&self, gpu_raw: u64, cpu_expect_ns: u64) -> bool {
        let r = self.residual_ns(gpu_raw, cpu_expect_ns);
        r.abs() <= CALIB_RESIDUAL_TOL_NS
    }
}

/// 双源漂移账（同一 GPU 段在两源的跨度差——漂移控制的可判定化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftCheck {
    /// CPU 域测得跨度（纳秒）。
    pub cpu_span_ns: u64,
    /// GPU 时间戳测得跨度（换算到纳秒后）。
    pub gpu_span_ns: u64,
    /// 漂移量（两跨度差的绝对值）。
    pub drift_ns: u64,
    /// 是否超容差（超阈即标注——漂移控制不是摆设）。
    pub drifted: bool,
}

/// 双源漂移核对（判据二：精度与漂移控制——同一时段两源各测一次对账）。
pub fn drift_check(cpu_span_ns: u64, gpu_span_ns: u64) -> DriftCheck {
    let drift = cpu_span_ns.abs_diff(gpu_span_ns);
    DriftCheck {
        cpu_span_ns,
        gpu_span_ns,
        drift_ns: drift,
        drifted: drift > DRIFT_TOLERANCE_NS,
    }
}

// ---------------------------------------------------------------------------
// 四、计时开销账（判据四：<0.1% 帧预算——实测达标）
// ---------------------------------------------------------------------------

/// 开销结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverheadReport {
    /// 每帧开合操作次数。
    pub ops_per_frame: u64,
    /// 单次操作实测耗时（纳秒；实测注入值）。
    pub ns_per_op: u64,
    /// 总开销（纳秒）。
    pub total_ns: u64,
    /// 占帧预算比（每分比 ppm）。
    pub ratio_ppm: u64,
    /// 是否达标（<0.1% 帧预算）。
    pub within_budget: bool,
}

/// 计时开销核算（判据四：实测值注入核算，不拍脑袋宣称）。
pub fn overhead_report(ops_per_frame: u64, ns_per_op: u64) -> OverheadReport {
    let total = ops_per_frame.saturating_mul(ns_per_op);
    let ratio = total * 1_000_000 / FRAME_BUDGET_NS;
    OverheadReport {
        ops_per_frame,
        ns_per_op,
        total_ns: total,
        ratio_ppm: ratio,
        within_budget: ratio < OVERHEAD_LIMIT_PPM,
    }
}

/// 开销摘要单行（读屏可查——达标与否一句话）。
pub fn overhead_line(rep: &OverheadReport) -> String {
    format!(
        "计时开销：{}次操作×{}ns={}ns，占帧预算{}ppm（红线{}ppm）——{}",
        rep.ops_per_frame,
        rep.ns_per_op,
        rep.total_ns,
        rep.ratio_ppm,
        OVERHEAD_LIMIT_PPM,
        if rep.within_budget { "达标" } else { "超标，计时器负载过重" }
    )
}
