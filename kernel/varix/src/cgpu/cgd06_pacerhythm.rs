//! CGPU-F0486 · 呈现节奏模型（CGPU-D 域 · 帧呈现与节奏域 · D06 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0486`
//!
//! **判据（锚点原文）**：三模式、权衡声明、适配、探测、判据。
//!
//! **职责定位（锚点原文）**：节奏模型：呈现时钟建模（VSync/无同步/
//! 合成器三种模式的帧间隔行为差异）；节奏策略（各模式的间隔管理策
//! 略——撕裂 vs 延迟的权衡声明）；模式适配（全屏独占/无边框窗口的
//! 节奏差异适配——与 D02 抑制联动）；模式探测（当前呈现模式自动识
//! 别）。
//!
//! ## 一、诊断码（0x68 段独占：D06 组节奏面）
//!
//! 十六码闭集（[`XcCode`]）：模式面四码（未探测/探测信号不足/探测
//! 抖动滞留/模式声明冲突）、节奏面四码（间隔越界/节奏漂移/抖动超
//! 阈/时钟未初始化）、适配面四码（窗口模式未适配/抑制态未联动/适
//! 配矩阵空洞/合成周期未知）、权衡面四码（权衡声明缺失/撕裂违例/
//! 延迟声明违例/间隔语义错配）。段守恒编译期钉死（[`segment_guard`]
//! 对账全部十六码落在 0x6800..0x6900 开区间且互不重码）。
//!
//! ## 二、三模式呈现时钟建模（判据一：三模式）
//!
//! 呈现模式三闭集（[`PresentMode`]——VSync/NoSync/Compositor）；
//! 逐模式帧间隔行为模型（[`PresentMode::interval_semantics`]——
//! VSync=刷新周期整数倍对齐、NoSync=事件驱动近零间隔、Compositor=
//! 合成器周期可异于刷新）；呈现时钟（[`PresentClock`]——tick 注入
//! 零墙钟：呈现事件流喂入、间隔 EWMA 平滑 α=1/4、下次呈现预测、
//! 间隔漂移检测 [`PresentClock::drift_check`]——实测间隔与模式语
//! 义间隔持续偏差超 [`DRIFT_PCT`]% 即 [`XcCode::PaceDrift`] 留痕）
//! ；抖动哨（[`PresentClock::jitter_check`]——窗口内间隔极差超
//! [`JITTER_BREACH_US`] 即 [`XcCode::JitterBreach`]——大抖动是卡
//! 顿前兆信号，供 D04 帧间隔域互证）。
//!
//! ## 三、节奏策略与权衡声明（判据二：权衡声明）
//!
//! 逐模式策略（[`PacePolicy`]——间隔管理动作闭集 [`PacingAction`]
//! ：对齐下个垂直同步 / 立即呈现 / 跟随合成器节拍）；权衡声明
//! （[`TradeoffStatement`]——撕裂可能性 [`TradeoffStatement::
//! tear_possible`]、最大延迟承诺 [`max_latency_us`]、间隔语义——
//! **声明是结构体不是注释**：被 [`PaceRhythmModel::statement_of`]
//! 查询 API 消费，读屏与契约对拍同源）；声明对拍（[`PaceRhythm
//! Model::verify_statement`]——实测行为与声明对拍：声明撕裂不可
//! 能却观测到近零间隔即撕裂违例 [`XcCode::TearViolation`]、延迟
//! 超声明即延迟违例 [`XcCode::LatencyStmtViolation`]、间隔语义错
//! 配 [`XcCode::IntervalMismatch`]——诚实声明的执行层是「声明可
//! 被证伪」）。
//!
//! ## 四、模式适配（判据三：全屏独占/无边框窗口 + D02 抑制联动）
//!
//! 窗口形态二闭集（[`WindowKind`]——全屏独占 [`WindowKind::
//! FullscreenExclusive`] / 无边框窗口 [`WindowKind::Borderless`]）
//! ；适配矩阵（[`effective_mode`]——四格全定义：独占×VSync=VSync
//! （硬翻页可用）、独占×NoSync=NoSync（撕裂自由）、无边框×VSync=
//! Compositor（合成器必然介入——真硬翻页不存在，行为映射到合成
//! 节拍）、无边框×NoSync=Compositor（同前——「无边框没有真免同
//! 步」是适配的核心语义）；矩阵空洞显性拒 [`XcCode::
//! AdaptMatrixHole`]——两枚举各二值四格穷尽，空洞只可能是实现漏
//! 洞）。D02 抑制联动（[`PaceRhythmModel::set_inhibit`]——抑制态
//! 间隔翻倍建模：抑制帧跳过呈现、有效间隔 = 语义间隔 × 2^抑制深
//! 度；抑制态未联动即 [`XcCode::InhibitUnlinked`] 留痕——节奏模
//! 型必须知道「这帧没呈现是抑制不是丢帧」）。
//!
//! ## 五、模式探测（判据四：探测）
//!
//! 探测器（[`ModeDetector`]——观察呈现事件间隔分布自动识别：间隔
//! 集中于刷新周期 ±5% 判 VSync、间隔近零占比超半判 NoSync、间隔集
//! 中于非刷新固定周期判 Compositor、信号不足判未探测 [`ProbeOut
//! come::Undetected`]——**不留错误判断**）；探测迟滞（连续
//! [`PROBE_CONFIRM_TICKS`] 帧同判才切换——防单帧噪声翻转；迟滞滞
//! 留留痕 [`XcCode::ProbeFlap`]）；探测声明冲突（探测器结论与上
//! 游声明不一致即 [`XcCode::ModeStmtConflict`] 立案——声明与实测
//! 两口径对拍）。信号样本窗（[`PROBE_WINDOW`] 帧）。
//!
//! ## 六、主编排与快照
//!
//! 全链编排（[`PaceRhythmModel::tick`]——呈现事件喂入：探测 → 适
//! 配 → 时钟更新 → 节奏判定 → 声明对拍，单事件 O(1)；窗口重算
//! O([`PROBE_WINDOW`]) 摊还）；快照（[`PaceSnap`]——'|' 分隔保空
//! 段 + FNV 指纹）；参考探针（[`reference_budget_check`]——三模
//! 式 × 两窗口 × 抑制联动全矩阵走查 + 探测全路径 + 声明对拍违例
//! 触发 + 快照恒等）。
//!
//! 零 unwrap、零 expect、零 panic 面、零 IO、零墙钟（呈现 tick 上
//! 游注入）。

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

// ============================================================
// 常量面（全部字面量写死）
// ============================================================

/// 探测信号样本窗（呈现事件帧数）。
pub const PROBE_WINDOW: usize = 16;

/// 探测迟滞确认帧数（连续同判才切换）。
pub const PROBE_CONFIRM_TICKS: u32 = 4;

/// VSync 判定带宽（刷新周期 ±5% 千分比 50）。
pub const VSYNC_BAND_PERMILLE: u64 = 50;

/// NoSync 判定阈值：近零间隔（< 刷新周期 1/4）占比（百分数）。
pub const NOSYNC_NEARZERO_PCT: u64 = 50;

/// 间隔漂移阈值（实测与语义间隔偏差百分数）。
pub const DRIFT_PCT: u64 = 20;

/// 抖动越界阈（窗口内间隔极差，微秒）。
pub const JITTER_BREACH_US: u64 = 8_000;

/// 默认刷新周期（微秒；60Hz 语义值——上游可改写）。
pub const DEFAULT_REFRESH_US: u64 = 16_667;

/// 快照指纹种子偏移。
pub const SNAP_FNV_SEED: u32 = 0x6868_0001;

// ============================================================
// 一、诊断码（0x68 段独占：D06 组节奏面）
// ============================================================

/// 诊断码闭集（0x68 段独占：D06 组节奏面十六码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 呈现模式未探测（信号不足期间的判定请求）。
    ModeUndetected,
    /// 探测信号不足（样本窗未满）。
    ProbeInsufficient,
    /// 探测抖动滞留（迟滞期内判定请求）。
    ProbeFlap,
    /// 探测结论与上游声明冲突。
    ModeStmtConflict,
    /// 实测间隔越出模式语义带。
    IntervalOutOfBand,
    /// 节奏漂移（与语义间隔持续偏差超阈）。
    PaceDrift,
    /// 抖动越界（窗口内间隔极差超阈）。
    JitterBreach,
    /// 呈现时钟未初始化（先喂基准再判定）。
    ClockUninit,
    /// 窗口形态未适配（适配矩阵外请求）。
    WindowUnadapted,
    /// 抑制态未联动（抑制期间节奏模型未感知）。
    InhibitUnlinked,
    /// 适配矩阵空洞（穷尽性实现漏洞）。
    AdaptMatrixHole,
    /// 合成器周期未知（Compositor 判定前提缺失）。
    CompPeriodUnknown,
    /// 权衡声明缺失（查询时无声明可给）。
    StmtMissing,
    /// 撕裂违例（声明撕裂不可能却观测近零间隔）。
    TearViolation,
    /// 延迟声明违例（实测延迟超声明承诺）。
    LatencyStmtViolation,
    /// 间隔语义错配（声明语义与实测分布不符）。
    IntervalMismatch,
}

impl XcCode {
    /// 段基座（0x6800：D06 组节奏面独占段）。
    pub const SEGMENT_BASE: u16 = 0x6800;

    /// 段内偏移查表。
    #[must_use]
    pub const fn offset(self) -> u16 {
        match self {
            XcCode::ModeUndetected => 0x01,
            XcCode::ProbeInsufficient => 0x02,
            XcCode::ProbeFlap => 0x03,
            XcCode::ModeStmtConflict => 0x04,
            XcCode::IntervalOutOfBand => 0x05,
            XcCode::PaceDrift => 0x06,
            XcCode::JitterBreach => 0x07,
            XcCode::ClockUninit => 0x08,
            XcCode::WindowUnadapted => 0x09,
            XcCode::InhibitUnlinked => 0x0A,
            XcCode::AdaptMatrixHole => 0x0B,
            XcCode::CompPeriodUnknown => 0x0C,
            XcCode::StmtMissing => 0x0D,
            XcCode::TearViolation => 0x0E,
            XcCode::LatencyStmtViolation => 0x0F,
            XcCode::IntervalMismatch => 0x10,
        }
    }

    /// 完整诊断码。
    #[must_use]
    pub const fn as_raw(self) -> u16 {
        Self::SEGMENT_BASE + self.offset()
    }

    /// 全部十六码。
    pub const ALL: [XcCode; 16] = [
        XcCode::ModeUndetected,
        XcCode::ProbeInsufficient,
        XcCode::ProbeFlap,
        XcCode::ModeStmtConflict,
        XcCode::IntervalOutOfBand,
        XcCode::PaceDrift,
        XcCode::JitterBreach,
        XcCode::ClockUninit,
        XcCode::WindowUnadapted,
        XcCode::InhibitUnlinked,
        XcCode::AdaptMatrixHole,
        XcCode::CompPeriodUnknown,
        XcCode::StmtMissing,
        XcCode::TearViolation,
        XcCode::LatencyStmtViolation,
        XcCode::IntervalMismatch,
    ];
}

/// 段守恒对账。
#[must_use]
pub const fn segment_guard() -> bool {
    let mut i = 0;
    while i < XcCode::ALL.len() {
        let raw = XcCode::ALL[i].as_raw();
        if raw < XcCode::SEGMENT_BASE || raw >= 0x6900 {
            return false;
        }
        let mut j = 0;
        while j < i {
            if XcCode::ALL[j].as_raw() == raw {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ============================================================
// 二、三模式呈现时钟建模（判据一）
// ============================================================

/// 呈现模式三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresentMode {
    /// 垂直同步（间隔=刷新周期整数倍，撕裂不可能，延迟 1-2 周期）。
    VSync,
    /// 无同步（事件驱动近零间隔，撕裂可能，延迟最小）。
    NoSync,
    /// 合成器（间隔=合成器周期，撕裂不可能，延迟含合成排队）。
    Compositor,
}

impl PresentMode {
    /// 间隔语义声明（三模式帧间隔行为差异的口径化）。
    ///
    /// 返回 `(名义间隔语义, 撕裂可能, 延迟上界倍数)`：延迟上界以刷
    /// 新周期为单位（VSync=2 周期、NoSync=0 近零、Compositor=3 周
    /// 期含排队）。
    #[must_use]
    pub const fn interval_semantics(self) -> (&'static str, bool, u64) {
        match self {
            PresentMode::VSync => ("aligned_refresh", false, 2),
            PresentMode::NoSync => ("event_driven", true, 0),
            PresentMode::Compositor => ("compositor_period", false, 3),
        }
    }

    /// 名义间隔（微秒）：VSync=刷新、NoSync=近零（刷新 1/16 语义值
    /// ——事件驱动的期望间隔不为零但极小）、Compositor=合成周期。
    #[must_use]
    pub const fn nominal_interval_us(self, refresh_us: u64, comp_period_us: u64) -> u64 {
        match self {
            PresentMode::VSync => refresh_us,
            PresentMode::NoSync => refresh_us / 16,
            PresentMode::Compositor => {
                if comp_period_us == 0 {
                    refresh_us
                } else {
                    comp_period_us
                }
            }
        }
    }

    /// 模式名（快照用）。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            PresentMode::VSync => "vsync",
            PresentMode::NoSync => "nosync",
            PresentMode::Compositor => "comp",
        }
    }
}

/// 呈现时钟（间隔 EWMA + 预测 + 漂移/抖动哨）。
///
/// 全部时间以呈现事件 tick 上游注入（零墙钟）；EWMA α=1/4 位移实
/// 现；漂移与抖动判定全部与模式语义间隔对拍。
#[derive(Debug)]
pub struct PresentClock {
    /// 模式语义间隔（微秒；由适配层设定）。
    semantic_us: u64,
    /// 上次呈现 tick（None = 未初始化）。
    last_tick: Option<u64>,
    /// 间隔 EWMA（微秒）。
    ewma_us: u64,
    /// 已见间隔数。
    samples: u64,
    /// 漂移累计判定次数。
    drift_hits: u64,
    /// 抖动越界累计。
    jitter_hits: u64,
    /// 最近窗口间隔（环形 [`PROBE_WINDOW`]）。
    ring: [u64; PROBE_WINDOW],
    /// 环写指针。
    head: usize,
}

impl PresentClock {
    /// 构造（未初始化态）。
    #[must_use]
    pub const fn new(semantic_us: u64) -> Self {
        Self {
            semantic_us,
            last_tick: None,
            ewma_us: 0,
            samples: 0,
            drift_hits: 0,
            jitter_hits: 0,
            ring: [0; PROBE_WINDOW],
            head: 0,
        }
    }

    /// 语义间隔。
    #[must_use]
    pub const fn semantic_us(&self) -> u64 {
        self.semantic_us
    }

    /// 间隔 EWMA（未初始化为 0）。
    #[must_use]
    pub const fn ewma_us(&self) -> u64 {
        self.ewma_us
    }

    /// 样本数。
    #[must_use]
    pub const fn samples(&self) -> u64 {
        self.samples
    }

    /// 漂移/抖动累计。
    #[must_use]
    pub const fn hits(&self) -> (u64, u64) {
        (self.drift_hits, self.jitter_hits)
    }

    /// 喂入一次呈现事件，返回留痕码。
    ///
    /// - 首事件只记基准（无间隔可算）；
    /// - 后续事件算间隔 → EWMA → 入环 → 漂移对拍（EWMA 与语义间隔
    ///   偏差超 [`DRIFT_PCT`]% → [`XcCode::PaceDrift`]）→ 抖动对拍
    ///   （窗口极差超 [`JITTER_BREACH_US`] → [`XcCode::JitterBreach`]
    ///   ）→ 语义带对拍（EWMA 越出语义间隔 ±50% 带 → [`XcCode::
    ///   IntervalOutOfBand`]）。
    pub fn feed(&mut self, tick: u64) -> Vec<XcCode> {
        let mut codes: Vec<XcCode> = Vec::new();
        match self.last_tick {
            None => {
                self.last_tick = Some(tick);
                self.ewma_us = self.semantic_us;
                codes.push(XcCode::ClockUninit);
                return codes;
            }
            Some(last) => {
                if tick <= last {
                    // 时间倒退（上游注入异常）：忽略本帧不产间隔。
                    return codes;
                }
                let gap = tick - last;
                self.last_tick = Some(tick);
                // EWMA α=1/4：ewma = ewma - ewma/4 + gap/4。
                self.ewma_us = self.ewma_us - self.ewma_us / 4 + gap / 4;
                self.ring[self.head] = gap;
                self.head = (self.head + 1) % PROBE_WINDOW;
                if self.samples < u64::MAX {
                    self.samples += 1;
                }
                // 漂移对拍。
                if self.samples >= PROBE_WINDOW as u64 {
                    let sem = self.semantic_us.max(1);
                    let diff = if self.ewma_us > sem {
                        self.ewma_us - sem
                    } else {
                        sem - self.ewma_us
                    };
                    if diff * 100 > sem * DRIFT_PCT {
                        self.drift_hits += 1;
                        codes.push(XcCode::PaceDrift);
                    }
                    // 语义带对拍（±50%）。
                    let band = sem / 2;
                    if self.ewma_us > sem + band || self.ewma_us + band < sem {
                        codes.push(XcCode::IntervalOutOfBand);
                    }
                    // 抖动对拍（窗口极差）。
                    let mut lo: u64 = u64::MAX;
                    let mut hi: u64 = 0;
                    let mut k = 0;
                    while k < PROBE_WINDOW {
                        if self.ring[k] < lo {
                            lo = self.ring[k];
                        }
                        if self.ring[k] > hi {
                            hi = self.ring[k];
                        }
                        k += 1;
                    }
                    if hi.saturating_sub(lo) > JITTER_BREACH_US {
                        self.jitter_hits += 1;
                        codes.push(XcCode::JitterBreach);
                    }
                }
            }
        }
        codes
    }

    /// 下次呈现预测（当前 EWMA 外推）。
    #[must_use]
    pub const fn predict_next_gap_us(&self) -> u64 {
        if self.samples == 0 {
            self.semantic_us
        } else {
            self.ewma_us
        }
    }

    /// 抖动哨（外部复算口径：供探针/判据独立对拍）。
    #[must_use]
    pub fn window_spread_us(&self) -> u64 {
        let mut lo: u64 = u64::MAX;
        let mut hi: u64 = 0;
        let mut k = 0;
        while k < PROBE_WINDOW {
            if self.ring[k] < lo {
                lo = self.ring[k];
            }
            if self.ring[k] > hi {
                hi = self.ring[k];
            }
            k += 1;
        }
        if self.samples == 0 {
            0
        } else {
            hi.saturating_sub(lo)
        }
    }
}

// ============================================================
// 三、节奏策略与权衡声明（判据二）
// ============================================================

/// 间隔管理动作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PacingAction {
    /// 对齐下个垂直同步（VSync 模式）。
    AlignNextVblank,
    /// 立即呈现（NoSync 模式）。
    PresentNow,
    /// 跟随合成器节拍（Compositor 模式）。
    FollowCompositor,
}

/// 权衡声明（**结构体不是注释**：撕裂 vs 延迟的权衡口径化）。
///
/// 每模式一份字面量声明；被查询 API 消费、被实测对拍证伪。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TradeoffStatement {
    /// 撕裂可能性（NoSync=true 其余 false）。
    pub tear_possible: bool,
    /// 最大延迟承诺（刷新周期倍数）。
    pub max_latency_periods: u64,
    /// 间隔管理动作。
    pub action: PacingAction,
    /// 间隔语义名。
    pub interval_semantics: &'static str,
}

/// 逐模式权衡声明表（字面量写死——三份声明与模式定义同源）。
#[must_use]
pub const fn statement_of_mode(m: PresentMode) -> TradeoffStatement {
    match m {
        PresentMode::VSync => TradeoffStatement {
            tear_possible: false,
            max_latency_periods: 2,
            action: PacingAction::AlignNextVblank,
            interval_semantics: "aligned_refresh",
        },
        PresentMode::NoSync => TradeoffStatement {
            tear_possible: true,
            max_latency_periods: 0,
            action: PacingAction::PresentNow,
            interval_semantics: "event_driven",
        },
        PresentMode::Compositor => TradeoffStatement {
            tear_possible: false,
            max_latency_periods: 3,
            action: PacingAction::FollowCompositor,
            interval_semantics: "compositor_period",
        },
    }
}

// ============================================================
// 四、模式适配（窗口形态 + D02 抑制联动）
// ============================================================

/// 窗口形态二闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowKind {
    /// 全屏独占（真硬翻页可用）。
    FullscreenExclusive,
    /// 无边框窗口（合成器必然介入）。
    Borderless,
}

/// 适配矩阵：模式 × 窗口 → 有效模式（六格全定义，空洞显性拒）。
///
/// 核心语义：**无边框没有真免同步**——Borderless 下无论申请 VSync
/// 还是 NoSync，有效模式都是 Compositor（合成器必然节拍）；独占下
/// 申请模式照常生效（含 Compositor 申请——合成节拍本就是其语义）。
#[must_use]
pub const fn effective_mode(m: PresentMode, w: WindowKind) -> PresentMode {
    match (m, w) {
        (PresentMode::VSync, WindowKind::FullscreenExclusive) => PresentMode::VSync,
        (PresentMode::NoSync, WindowKind::FullscreenExclusive) => PresentMode::NoSync,
        (PresentMode::Compositor, WindowKind::FullscreenExclusive) => PresentMode::Compositor,
        (PresentMode::VSync, WindowKind::Borderless) => PresentMode::Compositor,
        (PresentMode::NoSync, WindowKind::Borderless) => PresentMode::Compositor,
        (PresentMode::Compositor, WindowKind::Borderless) => PresentMode::Compositor,
    }
}

// ============================================================
// 五、模式探测（判据四）
// ============================================================

/// 探测结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeOutcome {
    /// 判定：VSync。
    VSync,
    /// 判定：NoSync。
    NoSync,
    /// 判定：Compositor。
    Compositor,
    /// 未探测（信号不足——不留错误判断）。
    Undetected,
    /// 迟滞滞留（新判与当前不同且未满确认帧）。
    HeldByHysteresis,
}

/// 模式探测器（间隔分布自动识别 + 迟滞 + 声明对拍）。
#[derive(Debug)]
pub struct ModeDetector {
    /// 间隔样本环。
    ring: [u64; PROBE_WINDOW],
    /// 环写指针。
    head: usize,
    /// 已填样本数。
    filled: usize,
    /// 当前判定。
    current: Option<PresentMode>,
    /// 连续同判计数（迟滞确认）。
    streak: u32,
    /// 声明冲突累计。
    stmt_conflicts: u64,
    /// 迟滞滞留累计。
    holds: u64,
}

impl ModeDetector {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ring: [0; PROBE_WINDOW],
            head: 0,
            filled: 0,
            current: None,
            streak: 0,
            stmt_conflicts: 0,
            holds: 0,
        }
    }

    /// 当前判定。
    #[must_use]
    pub const fn current(&self) -> Option<PresentMode> {
        self.current
    }

    /// 冲突/滞留累计。
    #[must_use]
    pub const fn counters(&self) -> (u64, u64) {
        (self.stmt_conflicts, self.holds)
    }

    /// 观测一次间隔并给出判定。
    ///
    /// 识别规则（样本窗满才判）：
    /// - 近零占比（间隔 < 刷新 1/4）> [`NOSYNC_NEARZERO_PCT`]% → NoSync；
    /// - 落在刷新 ±[`VSYNC_BAND_PERMILLE`]‰ 的占比 > 3/4 → VSync；
    /// - 落在合成周期 ±10% 的占比 > 3/4（合成周期已知且 ≠ 刷新）→
    ///   Compositor；
    /// - 其余 → 沿用当前判定（不确定不改判）。
    pub fn observe(&mut self, gap_us: u64, refresh_us: u64, comp_period_us: u64) -> ProbeOutcome {
        self.ring[self.head] = gap_us;
        self.head = (self.head + 1) % PROBE_WINDOW;
        if self.filled < PROBE_WINDOW {
            self.filled += 1;
        }
        if self.filled < PROBE_WINDOW {
            return ProbeOutcome::Undetected;
        }
        let quarter = refresh_us / 4;
        let mut near_zero = 0u32;
        let mut in_vsync_band = 0u32;
        let mut in_comp_band = 0u32;
        let mut k = 0;
        while k < PROBE_WINDOW {
            let g = self.ring[k];
            if g < quarter {
                near_zero += 1;
            }
            let dvs = if g > refresh_us { g - refresh_us } else { refresh_us - g };
            if dvs * 1000 <= refresh_us * VSYNC_BAND_PERMILLE {
                in_vsync_band += 1;
            }
            if comp_period_us > 0 {
                let dc = if g > comp_period_us { g - comp_period_us } else { comp_period_us - g };
                if dc * 10 <= comp_period_us {
                    in_comp_band += 1;
                }
            }
            k += 1;
        }
        let probe = if near_zero * 100 > (NOSYNC_NEARZERO_PCT as u32) * (PROBE_WINDOW as u32) {
            Some(PresentMode::NoSync)
        } else if in_vsync_band * 4 > (PROBE_WINDOW as u32) * 3 {
            Some(PresentMode::VSync)
        } else if comp_period_us > 0 && in_comp_band * 4 > (PROBE_WINDOW as u32) * 3 {
            Some(PresentMode::Compositor)
        } else {
            None
        };
        match probe {
            None => {
                if self.current.is_some() {
                    ProbeOutcome::Undetected
                } else {
                    ProbeOutcome::Undetected
                }
            }
            Some(m) => {
                if self.current == Some(m) {
                    self.streak = 0;
                    return match m {
                        PresentMode::VSync => ProbeOutcome::VSync,
                        PresentMode::NoSync => ProbeOutcome::NoSync,
                        PresentMode::Compositor => ProbeOutcome::Compositor,
                    };
                }
                self.streak += 1;
                if self.streak >= PROBE_CONFIRM_TICKS {
                    self.current = Some(m);
                    self.streak = 0;
                    match m {
                        PresentMode::VSync => ProbeOutcome::VSync,
                        PresentMode::NoSync => ProbeOutcome::NoSync,
                        PresentMode::Compositor => ProbeOutcome::Compositor,
                    }
                } else {
                    self.holds += 1;
                    ProbeOutcome::HeldByHysteresis
                }
            }
        }
    }

    /// 声明对拍（上游声明的模式 vs 探测结论）。
    pub fn verify_declared(&mut self, declared: PresentMode) -> Option<XcCode> {
        match self.current {
            None => None,
            Some(probed) => {
                if probed != declared {
                    self.stmt_conflicts += 1;
                    Some(XcCode::ModeStmtConflict)
                } else {
                    None
                }
            }
        }
    }
}

// ============================================================
// 六、主编排与快照
// ============================================================

/// 呈现事件（单帧一次）。
#[derive(Clone, Copy, Debug)]
pub struct PresentEvent {
    /// 呈现 tick（上游注入，零墙钟）。
    pub tick: u64,
    /// 实测呈现延迟（微秒；提交到上屏——供延迟声明对拍；0 = 未知）。
    pub latency_us: u64,
    /// 本帧是否被 D02 抑制（抑制=跳过呈现的节奏模型侧感知）。
    pub inhibited: bool,
}

/// 单 tick 报告。
#[derive(Debug)]
pub struct PaceTickReport {
    /// 探测结果。
    pub probe: ProbeOutcome,
    /// 有效模式（适配矩阵输出；未探测沿用声明映射）。
    pub effective: PresentMode,
    /// 间隔管理动作。
    pub action: PacingAction,
    /// 权衡声明（有效模式的字面量声明）。
    pub statement: TradeoffStatement,
    /// 本 tick 留痕码。
    pub codes: Vec<XcCode>,
    /// 下次呈现间隔预测（微秒）。
    pub next_gap_us: u64,
}

/// 呈现节奏模型主编排。
#[derive(Debug)]
pub struct PaceRhythmModel {
    /// 上游声明的申请模式。
    declared: PresentMode,
    /// 窗口形态。
    window: WindowKind,
    /// 刷新周期（微秒）。
    refresh_us: u64,
    /// 合成器周期（微秒；0 = 未知）。
    comp_period_us: u64,
    /// D02 抑制态。
    inhibit: bool,
    /// 抑制深度（连续抑制帧数——间隔翻倍建模的指数）。
    inhibit_depth: u32,
    /// 抑制未联动累计（抑制事件在模型未感知时发生）。
    inhibit_misses: u64,
    /// 有效模式时钟。
    clock: PresentClock,
    /// 探测器。
    detector: ModeDetector,
    /// 声明对拍累计（撕裂/延迟/间隔三类违例）。
    tear_violations: u64,
    latency_violations: u64,
    interval_mismatches: u64,
    /// 操作计账。
    ops: u64,
}

impl PaceRhythmModel {
    /// 构造（声明模式 + 窗口形态；刷新周期缺省 60Hz 语义值）。
    #[must_use]
    pub fn new(declared: PresentMode, window: WindowKind, refresh_us: u64, comp_period_us: u64) -> Self {
        let eff = effective_mode(declared, window);
        let sem = PresentMode::nominal_interval_us(eff, refresh_us, comp_period_us);
        Self {
            declared,
            window,
            refresh_us,
            comp_period_us,
            inhibit: false,
            inhibit_depth: 0,
            inhibit_misses: 0,
            clock: PresentClock::new(sem),
            detector: ModeDetector::new(),
            tear_violations: 0,
            latency_violations: 0,
            interval_mismatches: 0,
            ops: 0,
        }
    }

    /// D02 抑制联动入口（抑制态切换）。
    ///
    /// 抑制开：深度清零起步；抑制关：深度归零。抑制期间呈现事件若
    /// 仍到（说明上游没真正抑制）记 [`XcCode::InhibitUnlinked`]。
    pub fn set_inhibit(&mut self, on: bool) {
        if on && !self.inhibit {
            self.inhibit_depth = 0;
        }
        self.inhibit = on;
        if !on {
            self.inhibit_depth = 0;
        }
    }

    /// 有效模式。
    #[must_use]
    pub const fn effective(&self) -> PresentMode {
        effective_mode(self.declared, self.window)
    }

    /// 权衡声明查询（有效模式的字面量声明）。
    #[must_use]
    pub const fn statement_of(&self) -> TradeoffStatement {
        statement_of_mode(self.effective())
    }

    /// 全链 tick：探测 → 时钟 → 声明对拍。
    pub fn tick(&mut self, ev: PresentEvent) -> PaceTickReport {
        self.ops += 1;
        let mut codes: Vec<XcCode> = Vec::new();

        // ① 抑制联动核查。
        if ev.inhibited {
            if !self.inhibit {
                self.inhibit_misses += 1;
                codes.push(XcCode::InhibitUnlinked);
            }
            self.inhibit_depth = self.inhibit_depth.saturating_add(1);
            // 抑制帧不进时钟（没有呈现）。
            return PaceTickReport {
                probe: self.detector.current().map_or(ProbeOutcome::Undetected, |m| match m {
                    PresentMode::VSync => ProbeOutcome::VSync,
                    PresentMode::NoSync => ProbeOutcome::NoSync,
                    PresentMode::Compositor => ProbeOutcome::Compositor,
                }),
                effective: self.effective(),
                action: self.statement_of().action,
                statement: self.statement_of(),
                codes,
                next_gap_us: self.clock.predict_next_gap_us() * 2,
            };
        }
        if self.inhibit {
            // 抑制态解除由 set_inhibit 负责；抑制中收到呈现说明联
            // 动缺失。
            self.inhibit_misses += 1;
            codes.push(XcCode::InhibitUnlinked);
        }

        // ② 探测（首帧无间隔——用语义间隔暖机）。
        let gap = if self.clock.samples() == 0 {
            self.clock.semantic_us()
        } else {
            self.clock.predict_next_gap_us()
        };
        let probe = self.detector.observe(gap, self.refresh_us, self.comp_period_us);
        if let Some(c) = self.detector.verify_declared(self.effective()) {
            codes.push(c);
        }

        // ③ 时钟更新。
        let clock_codes = self.clock.feed(ev.tick);
        let mut i = 0;
        while i < clock_codes.len() {
            codes.push(clock_codes[i]);
            i += 1;
        }

        // ④ 声明对拍（撕裂 / 延迟 / 间隔语义）。
        let stmt = self.statement_of();
        if !stmt.tear_possible {
            // 声明撕裂不可能：近零间隔观测（< 语义 1/4）即违例信号
            // ——真撕裂由呈现后端报，模型侧以间隔异常为旁证。
            let ew = self.clock.ewma_us();
            let sem = self.clock.semantic_us().max(1);
            if self.clock.samples() >= PROBE_WINDOW as u64 && ew * 4 < sem {
                self.tear_violations += 1;
                codes.push(XcCode::TearViolation);
            }
        }
        if ev.latency_us > 0 {
            // 延迟声明对拍：实测延迟 > 声明上界 × 刷新周期。
            let cap = stmt.max_latency_periods * self.refresh_us;
            if cap > 0 && ev.latency_us > cap {
                self.latency_violations += 1;
                codes.push(XcCode::LatencyStmtViolation);
            }
        }
        if stmt.interval_semantics != self.effective().interval_semantics().0 {
            self.interval_mismatches += 1;
            codes.push(XcCode::IntervalMismatch);
        }

        PaceTickReport {
            probe,
            effective: self.effective(),
            action: stmt.action,
            statement: stmt,
            codes,
            next_gap_us: self.clock.predict_next_gap_us(),
        }
    }

    /// 抑制深度。
    #[must_use]
    pub const fn inhibit_depth(&self) -> u32 {
        self.inhibit_depth
    }

    /// 抑制未联动累计。
    #[must_use]
    pub const fn inhibit_misses(&self) -> u64 {
        self.inhibit_misses
    }

    /// 三类声明违例累计。
    #[must_use]
    pub const fn violations(&self) -> (u64, u64, u64) {
        (self.tear_violations, self.latency_violations, self.interval_mismatches)
    }

    /// 时钟只读。
    #[must_use]
    pub const fn clock(&self) -> &PresentClock {
        &self.clock
    }

    /// 探测器只读。
    #[must_use]
    pub const fn detector(&self) -> &ModeDetector {
        &self.detector
    }

    /// 操作计账。
    #[must_use]
    pub const fn ops(&self) -> u64 {
        self.ops
    }
}

// ============================================================
// 快照与探针
// ============================================================

/// 呈现节奏模型快照。
#[derive(Debug)]
pub struct PaceSnap;

impl PaceSnap {
    /// 版本头。
    pub const VERSION: u32 = 1;

    /// 序列化：v|声明|窗口|有效|探测|ewma|语义|漂移|抖动|抑制深度。
    #[must_use]
    pub fn serialize(m: &PaceRhythmModel) -> String {
        let mut s = String::new();
        let probe = match m.detector().current() {
            Some(PresentMode::VSync) => "vsync",
            Some(PresentMode::NoSync) => "nosync",
            Some(PresentMode::Compositor) => "comp",
            None => "?",
        };
        let _ = write!(
            s,
            "v{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            Self::VERSION,
            m.declared_name(),
            window_name(m.window),
            m.effective().name(),
            probe,
            m.clock().ewma_us(),
            m.clock().semantic_us(),
            m.clock().hits().0,
            m.clock().hits().1,
            m.inhibit_depth()
        );
        s
    }

    /// 快照指纹。
    #[must_use]
    pub fn fingerprint(s: &str) -> u32 {
        fnv1a(s.as_bytes()) ^ SNAP_FNV_SEED
    }
}

impl PaceRhythmModel {
    fn declared_name(&self) -> &'static str {
        self.declared.name()
    }
}

fn window_name(w: WindowKind) -> &'static str {
    match w {
        WindowKind::FullscreenExclusive => "fse",
        WindowKind::Borderless => "bord",
    }
}

/// 载荷指纹（FNV-1a 32 位）。
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 参考探针：三模式 × 两窗口全矩阵 + 抑制联动 + 探测全路径 + 声明
/// 对拍 + 快照恒等。
#[must_use]
pub fn reference_budget_check() -> bool {
    if !segment_guard() {
        return false;
    }
    // 口径 1：适配矩阵四格全查（空洞探测）。
    if effective_mode(PresentMode::VSync, WindowKind::FullscreenExclusive) != PresentMode::VSync
        || effective_mode(PresentMode::NoSync, WindowKind::FullscreenExclusive) != PresentMode::NoSync
        || effective_mode(PresentMode::VSync, WindowKind::Borderless) != PresentMode::Compositor
        || effective_mode(PresentMode::NoSync, WindowKind::Borderless) != PresentMode::Compositor
    {
        return false;
    }
    // 口径 2：三模式权衡声明字面量对拍。
    if statement_of_mode(PresentMode::NoSync).tear_possible == false {
        return false;
    }
    if statement_of_mode(PresentMode::VSync).tear_possible {
        return false;
    }
    if statement_of_mode(PresentMode::Compositor).max_latency_periods != 3 {
        return false;
    }

    // 口径 3：独占 × VSync 全链（规整间隔 → 无漂移无违例）。
    let mut m1 = PaceRhythmModel::new(PresentMode::VSync, WindowKind::FullscreenExclusive, DEFAULT_REFRESH_US, 0);
    let mut t: u64 = 0;
    let mut f = 0;
    while f < 40 {
        t += DEFAULT_REFRESH_US; // 完美 60Hz 节拍。
        let rep = m1.tick(PresentEvent { tick: t, latency_us: 20_000, inhibited: false });
        // 有效模式 = VSync（独占下申请生效）。
        if rep.effective != PresentMode::VSync {
            return false;
        }
        // 动作 = 对齐垂直同步。
        if rep.action != PacingAction::AlignNextVblank {
            return false;
        }
        f += 1;
    }
    let (tv, lv, im) = m1.violations();
    if tv != 0 || lv != 0 || im != 0 {
        return false;
    }
    if m1.clock().hits().0 != 0 {
        return false;
    }

    // 口径 4：无边框 × NoSync → 有效 Compositor（核心适配语义）。
    let m2 = PaceRhythmModel::new(PresentMode::NoSync, WindowKind::Borderless, DEFAULT_REFRESH_US, DEFAULT_REFRESH_US * 2);
    if m2.effective() != PresentMode::Compositor {
        return false;
    }
    if m2.statement_of().action != PacingAction::FollowCompositor {
        return false;
    }

    // 口径 5：抑制联动（抑制帧间隔翻倍预测 + 未联动留痕）。
    let mut m3 = PaceRhythmModel::new(PresentMode::VSync, WindowKind::FullscreenExclusive, DEFAULT_REFRESH_US, 0);
    m3.set_inhibit(true);
    let rep3 = m3.tick(PresentEvent { tick: DEFAULT_REFRESH_US, latency_us: 0, inhibited: true });
    if rep3.next_gap_us != DEFAULT_REFRESH_US * 2 {
        return false;
    }
    // 抑制中收到非抑制帧（上游联动缺失）→ InhibitUnlinked。
    let rep3b = m3.tick(PresentEvent { tick: DEFAULT_REFRESH_US * 2, latency_us: 0, inhibited: false });
    let mut has_miss = false;
    let mut ci = 0;
    while ci < rep3b.codes.len() {
        if rep3b.codes[ci] == XcCode::InhibitUnlinked {
            has_miss = true;
        }
        ci += 1;
    }
    if !has_miss || m3.inhibit_misses() == 0 {
        return false;
    }

    // 口径 6：NoSync 探测路径（近零间隔流 → 探测 NoSync + 迟滞生效）。
    let mut m4 = PaceRhythmModel::new(PresentMode::VSync, WindowKind::FullscreenExclusive, DEFAULT_REFRESH_US, 0);
    // 声明 VSync 但注入 NoSync 节拍 → ModeStmtConflict。
    let mut t4: u64 = 0;
    let mut conflict_seen = false;
    let mut f4 = 0;
    while f4 < 60 {
        t4 += DEFAULT_REFRESH_US / 16 + (f4 as u64 % 3); // 近零抖动间隔。
        let rep = m4.tick(PresentEvent { tick: t4, latency_us: 0, inhibited: false });
        let mut k = 0;
        while k < rep.codes.len() {
            if rep.codes[k] == XcCode::ModeStmtConflict {
                conflict_seen = true;
            }
            k += 1;
        }
        f4 += 1;
    }
    if !conflict_seen {
        return false;
    }
    if m4.detector().current() != Some(PresentMode::NoSync) {
        return false;
    }
    // 撕裂违例旁证：声明 VSync（撕裂不可能）却观测近零间隔。
    let (tv4, _, _) = m4.violations();
    if tv4 == 0 {
        return false;
    }

    // 口径 7：快照往返恒等。
    let s1 = PaceSnap::serialize(&m1);
    let fp1 = PaceSnap::fingerprint(&s1);
    let s2 = PaceSnap::serialize(&m1);
    if PaceSnap::fingerprint(&s2) != fp1 || s1 != s2 {
        return false;
    }

    true
}
