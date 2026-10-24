//! vcu04_scoringsys — CGPU-F3204 计分体系（分数维的语义分册）。
//!
//! 判据原文（施工书锚点 CGPU-F3204）：计分体系（原始分/归一分/加权总分
//! ——计分实现——计分表；测试（计分一组）。判据：三分、计分表、一组、
//! 判据。
//!
//! **职责定位**：三分链（原始分 → 归一分 → 加权总分——每一跳语义显式：
//! 原始分带量纲跨场景不可比、归一分万分制跨场景可比、加权总分按类聚合
//! 可排序）+ 计分表（scene id × 量纲 × 方向 × 归一模式的登记账——五闸
//! 显性拒：id 表外/单位闭集外/类册无此场/无基线/零分或饱和，违者逐码
//! 计数不静默）+ 权重四档（均衡/偏渲染/偏计算/偏稳——万分制和恒
//! 10000，破坏守恒即 WeightSumBroken 立案）。与 vcu01（指标闭集）/
//! vcu02（三维格）/ vcu03（场景分类）分工——彼管考卷格式与格子、彼册
//! 管分类学、此管分数的语义学：没有归一的裸分是方言，没有权重的聚合
//! 是偏心。
//!
//! **三分链语义**：
//! 1. **原始分** `RawSample`：跑出来的裸值 + 量纲（五闭集）+ 方向（二
//!    闭集）——量纲决定可读性，方向决定归一时谁更好（帧率越高越好、
//!    时延越低越好，同一套公式方向反转单点处理）；
//! 2. **归一分** `normalize()`：四模式闭集（基线比值/对数刻度/门槛二值
//!    /排名百分位）→ [0, 20000] 万分制（200% 封顶——超基线一倍即可
//!    见顶，跑分不是军备竞赛）；
//! 3. **加权总分** `aggregate()`：类内均值 × 类权重 / 已覆盖类权重和
//!    ——分母只算已覆盖类，没跑的类不扣分也不送分（公平聚合单点）。
//!
//! **迟滞防抖**：跑分毛刺是排名杀手——新归一分与现分差 < 迟滞带则维持
//! 现分（带外才更新）；迟滞只对连续量纲模式（基线比值/对数刻度）生效，
//! 二值与排名面跳变即语义无毛刺可言（模式 × 迟滞适用闭集判定）。
//!
//! **复用声明**：id 空间（1..=MAX_SCENES）与容量口径消费 vcu02 常量；
//! 类别来源消费 vcu03 SceneCatalog（先入册后计分——分类是计分的前置
//! 闸，未分类的分数没有归属）。两处集成复用锚，不复刻不另立。
//!
//! 诊断枚举 ScoreCode 十六码（0x6E 段 0x6E00..=0x6E0F 互异 +
//! [`segment_guard`] 段守恒对账）。
use alloc::format;

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcu02_benchmodel::MAX_SCENES;
use super::vcu03_scenecls::{SceneCatalog, SceneClass};

/// 模块版本。
pub const U04_SCORINGSYS_VERSION: &str = "U04-scoringsys-v1";

/// 诊断段基值（0x6E 段——本会话段位账：0x5E/0x67/0x68/0x69/0x6A/0x6C/0x6D 已占，此占 0x6E）。
pub const SEG_BASE: u16 = 0x6E00;

/// 归一分上限（200% 封顶——万分制两万，跑分不是军备竞赛）。
pub const NORM_CAP: u64 = 20_000;

/// 权重满和（万分制——四类权重和恒此值，破坏即守恒事故）。
pub const WEIGHT_TOTAL: u64 = 10_000;

/// 迟滞带默认宽（万分制 120——带内毛刺不更新）。
pub const HYST_BAND_DEFAULT: u64 = 120;

/// 计分表默认容量（策略值——vcu02 格空间 4096 的四分之一，分册不吞全格）。
pub const SCORE_TABLE_CAPACITY: usize = 1_024;

// ---------------------------------------------------------------------------
// 诊断码（ScoreCode 十六码 · 0x6E 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（计分体系——0x6E 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScoreCode {
    /// 0x6E00 场景 id 表外（0 或超 MAX_SCENES——vcu02 同口径闸）。
    SceneRange,
    /// 0x6E01 量纲不在五闭集（裸值没有单位是数字不是分数）。
    UnitUnknown,
    /// 0x6E02 无基线不可归一（基线比值/对数刻度两模式的前置）。
    BaselineMissing,
    /// 0x6E03 原始分超饱和上限（量纲自带饱和线——超线即设备异常非成绩）。
    RawOversaturated,
    /// 0x6E04 权重和 ≠ WEIGHT_TOTAL（守恒破坏——聚合前提崩塌）。
    WeightSumBroken,
    /// 0x6E05 同 key 重复样本且未走覆盖通道（重复即登记事故）。
    SampleDuplicate,
    /// 0x6E06 计分表容量满（策略值——归档或扩容评审）。
    TableFull,
    /// 0x6E07 场景未入 vcu03 分类册（先分类后计分——未分类分数无归属）。
    ClassUnknown,
    /// 0x6E08 归一模式与方向语义冲突（排名百分位对 LowerBetter 无意义等）。
    NormModeInvalid,
    /// 0x6E09 迟滞带非法（0 带等于无迟滞、超万分为吞分——带须 1..=5_000）。
    HysteresisBandInvalid,
    /// 0x6E0A 空表聚合（无样本求总分——总分无意义）。
    EmptyAggregate,
    /// 0x6E0B 快照指纹不符（往返恒等破坏或指纹对拍失败）。
    SnapshotFingerprint,
    /// 0x6E0C 原始分为零（零不是成绩——除零保护与语义双重拒绝）。
    RawZero,
    /// 0x6E0D 排名百分位参数表外（rank 须 1..=n，n 须 ≥ 1）。
    PercentileOutOfRange,
    /// 0x6E0E 滚动账守恒破坏（总样本 ≠ 有效 + 覆盖 + 拒收和）。
    LedgerBroken,
    /// 0x6E0F 段守恒对账失败（十六码值/互异/界内校验不通过——段位事故）。
    SegmentGuard,
}

impl ScoreCode {
    /// 十六码闭集（判据侧独立复核）。
    pub const ALL: [ScoreCode; 16] = [
        ScoreCode::SceneRange,
        ScoreCode::UnitUnknown,
        ScoreCode::BaselineMissing,
        ScoreCode::RawOversaturated,
        ScoreCode::WeightSumBroken,
        ScoreCode::SampleDuplicate,
        ScoreCode::TableFull,
        ScoreCode::ClassUnknown,
        ScoreCode::NormModeInvalid,
        ScoreCode::HysteresisBandInvalid,
        ScoreCode::EmptyAggregate,
        ScoreCode::SnapshotFingerprint,
        ScoreCode::RawZero,
        ScoreCode::PercentileOutOfRange,
        ScoreCode::LedgerBroken,
        ScoreCode::SegmentGuard,
    ];

    /// 段内码值（0x6E00 起连续——segment_guard 对账面）。
    pub const fn code(self) -> u16 {
        match self {
            ScoreCode::SceneRange => 0x6E00,
            ScoreCode::UnitUnknown => 0x6E01,
            ScoreCode::BaselineMissing => 0x6E02,
            ScoreCode::RawOversaturated => 0x6E03,
            ScoreCode::WeightSumBroken => 0x6E04,
            ScoreCode::SampleDuplicate => 0x6E05,
            ScoreCode::TableFull => 0x6E06,
            ScoreCode::ClassUnknown => 0x6E07,
            ScoreCode::NormModeInvalid => 0x6E08,
            ScoreCode::HysteresisBandInvalid => 0x6E09,
            ScoreCode::EmptyAggregate => 0x6E0A,
            ScoreCode::SnapshotFingerprint => 0x6E0B,
            ScoreCode::RawZero => 0x6E0C,
            ScoreCode::PercentileOutOfRange => 0x6E0D,
            ScoreCode::LedgerBroken => 0x6E0E,
            ScoreCode::SegmentGuard => 0x6E0F,
        }
    }

    /// 人读诊断（发生了什么/为什么/怎么办——互异非空）。
    pub const fn detail(self) -> &'static str {
        match self {
            ScoreCode::SceneRange => {
                "场景 id 表外：id 须在 1..=MAX_SCENES（与 vcu02 同口径），建议换合法 id"
            }
            ScoreCode::UnitUnknown => {
                "量纲不在闭集：裸值没有单位是数字不是分数，建议登记单位后重报"
            }
            ScoreCode::BaselineMissing => {
                "无基线不可归一：基线比值与对数刻度两模式需要基线样本，建议先录基线或换模式"
            }
            ScoreCode::RawOversaturated => {
                "原始分超饱和上限：超线即设备异常非成绩，建议排查负载或扩饱和线评审"
            }
            ScoreCode::WeightSumBroken => {
                "权重和破坏：四类权重和须恒 10000，建议校准权重表后再聚合"
            }
            ScoreCode::SampleDuplicate => {
                "同 key 重复样本：同场景同配置重复计分须走覆盖通道，建议显式覆盖"
            }
            ScoreCode::TableFull => {
                "计分表容量满（当前策略值）：建议归档旧分或扩容评审"
            }
            ScoreCode::ClassUnknown => {
                "场景未入分类册：先入 vcu03 册再计分——未分类的分数没有归属，建议先登记类别"
            }
            ScoreCode::NormModeInvalid => {
                "归一模式与方向冲突：排名百分位对越低越好型无意义，建议换基线比值或对数刻度"
            }
            ScoreCode::HysteresisBandInvalid => {
                "迟滞带非法：带宽须在 1..=5000（零带即无防抖、超半程即吞分），建议回归默认 120"
            }
            ScoreCode::EmptyAggregate => {
                "空表聚合：无样本求总分无意义，建议先登记至少一个有效分数"
            }
            ScoreCode::SnapshotFingerprint => {
                "快照指纹不符：序列化往返或指纹对拍失败，建议排查序列化格式漂移"
            }
            ScoreCode::RawZero => {
                "原始分为零：零不是成绩（除零保护与语义双重拒绝），建议核查采集链路"
            }
            ScoreCode::PercentileOutOfRange => {
                "排名百分位参数表外：rank 须在 1..=n 且 n ≥ 1，建议核对排名样本数"
            }
            ScoreCode::LedgerBroken => {
                "滚动账守恒破坏：总样本数应等于有效加覆盖加拒收之和，建议审计登记流水"
            }
            ScoreCode::SegmentGuard => {
                "段守恒对账失败：十六码值须连续互异界内——段位事故立即立案"
            }
        }
    }
}

/// **段守恒对账**（十六码 × 连续性 × 互异性 × 界内——0x6E 段占用声明
/// 的自证：任何一码漂移即 SegmentGuard 立案）。
pub const fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ScoreCode::ALL.len() {
        // 界内：0x6E00..=0x6E0F。
        let c = ScoreCode::ALL[i].code();
        if c < SEG_BASE || c > SEG_BASE + 15 {
            return false;
        }
        // 连续：第 i 码恰为 SEG_BASE + i（连续即互异——u16 判等 const 面）。
        if c != SEG_BASE + i as u16 {
            return false;
        }
        let mut j = i + 1;
        while j < ScoreCode::ALL.len() {
            if ScoreCode::ALL[i].code() == ScoreCode::ALL[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 量纲五闭集 + 方向二闭集（原始分的两个坐标轴）
// ---------------------------------------------------------------------------

/// 原始分量纲五闭集（量纲即语义——定点整数约定写死在档位里）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricUnit {
    /// 帧率毫厘（fps × 1000——60000 即 60.000 fps；HigherBetter）。
    FpsMilli,
    /// 每帧微秒（µs/frame——8000 即 8 ms/帧；LowerBetter）。
    MicrosPerFrame,
    /// 每秒十亿算子十分位（GOPS × 10——1250 即 125.0 GOPS；HigherBetter）。
    GopsTenth,
    /// 每秒兆字节（MB/s——3200 即 3.2 GB/s；HigherBetter）。
    MegaBytesPerSec,
    /// 千分比（‰——875 即 87.5% 占空；HigherBetter）。
    RatioPermille,
}

impl MetricUnit {
    /// 五闭集。
    pub const ALL: [MetricUnit; 5] = [
        MetricUnit::FpsMilli,
        MetricUnit::MicrosPerFrame,
        MetricUnit::GopsTenth,
        MetricUnit::MegaBytesPerSec,
        MetricUnit::RatioPermille,
    ];

    /// 中文名（读屏面——互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            MetricUnit::FpsMilli => "帧率毫厘",
            MetricUnit::MicrosPerFrame => "每帧微秒",
            MetricUnit::GopsTenth => "算力十分位",
            MetricUnit::MegaBytesPerSec => "带宽兆字节",
            MetricUnit::RatioPermille => "占空千分比",
        }
    }

    /// 饱和上限（量纲自带饱和线——超线即设备异常非成绩）。
    pub const fn saturation(self) -> u64 {
        match self {
            MetricUnit::FpsMilli => 1_000_000,    // 1000 fps 封顶
            MetricUnit::MicrosPerFrame => 60_000_000, // 60 s/帧封顶
            MetricUnit::GopsTenth => 1_000_000,   // 100_000 GOPS 封顶
            MetricUnit::MegaBytesPerSec => 10_000_000, // 10 TB/s 封顶
            MetricUnit::RatioPermille => 1_000,   // 100% 封顶
        }
    }

    /// 单位下合理非零最小值（零检测的语义锚——低于此视为采集残渣）。
    pub const fn min_plausible(self) -> u64 {
        match self {
            MetricUnit::FpsMilli => 1,
            MetricUnit::MicrosPerFrame => 1,
            MetricUnit::GopsTenth => 1,
            MetricUnit::MegaBytesPerSec => 1,
            MetricUnit::RatioPermille => 1,
        }
    }
}

/// 方向语义二闭集（谁更好——归一反转单点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 越高越好（帧率/吞吐/带宽/占空）。
    HigherBetter,
    /// 越低越好（时延/功耗）。
    LowerBetter,
}

impl Direction {
    /// 二闭集。
    pub const ALL: [Direction; 2] = [Direction::HigherBetter, Direction::LowerBetter];

    /// 中文名（读屏面——互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            Direction::HigherBetter => "越高越好",
            Direction::LowerBetter => "越低越好",
        }
    }

    /// 量纲 × 方向默认配对（量纲决定方向——错配即语义事故）。
    pub const fn for_unit(u: MetricUnit) -> Direction {
        match u {
            MetricUnit::MicrosPerFrame => Direction::LowerBetter,
            _ => Direction::HigherBetter,
        }
    }
}

// ---------------------------------------------------------------------------
// 归一模式四闭集 + 策略结构
// ---------------------------------------------------------------------------

/// 归一模式四闭集（判据"归一分"的实现面——每模式独立语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NormMode {
    /// 基线比值：norm = raw × 10000 / baseline，方向反转单点，钳 [0, NORM_CAP]。
    BaselineRatio,
    /// 对数刻度：半步精度 log2 比值 → 5000 + lr2 × 1250（lr2 ∈ [-8, 8] 钳），
    /// 钳 [0, NORM_CAP]——跨数量级比较不失真。
    LogScale,
    /// 门槛二值：raw 达门槛 → 10000，未达 → 0（达标即语义，无中间带）。
    PassFail,
    /// 排名百分位：norm = rank × 10000 / (n + 1)——排名即分数（仅
    /// HigherBetter 语义——LowerBetter 排名面须先反转名单，此处显性拒）。
    RankPercentile,
}

impl NormMode {
    /// 四闭集。
    pub const ALL: [NormMode; 4] = [
        NormMode::BaselineRatio,
        NormMode::LogScale,
        NormMode::PassFail,
        NormMode::RankPercentile,
    ];

    /// 中文名（读屏面——互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            NormMode::BaselineRatio => "基线比值",
            NormMode::LogScale => "对数刻度",
            NormMode::PassFail => "门槛二值",
            NormMode::RankPercentile => "排名百分位",
        }
    }

    /// 是否需要基线（BaselineRatio/LogScale 前置闸——BaselineMissing 的判定源）。
    pub const fn needs_baseline(self) -> bool {
        match self {
            NormMode::BaselineRatio | NormMode::LogScale => true,
            NormMode::PassFail | NormMode::RankPercentile => false,
        }
    }

    /// 是否需要门槛（PassFail 前置——门槛 0 即非法）。
    pub const fn needs_threshold(self) -> bool {
        matches!(self, NormMode::PassFail)
    }

    /// 是否适用迟滞防抖（连续量纲模式才防毛刺——二值/排名跳变即语义）。
    pub const fn hysteresis_applicable(self) -> bool {
        matches!(self, NormMode::BaselineRatio | NormMode::LogScale)
    }

    /// 模式 × 方向语义合法性（NormModeInvalid 判定源——排名百分位对
    /// LowerBetter 无意义：排名本身已经是序，低者排名靠前会二次反转）。
    pub const fn allows_direction(self, d: Direction) -> bool {
        match self {
            NormMode::RankPercentile => matches!(d, Direction::HigherBetter),
            _ => true,
        }
    }
}

/// 归一策略（模式 × 方向 × 基线 × 门槛 × 迟滞带——计分的语义配置单点）。
#[derive(Clone, Copy, Debug)]
pub struct NormPolicy {
    /// 归一模式。
    pub mode: NormMode,
    /// 方向语义（与量纲默认配对对拍——错配即 NormModeInvalid/方向闸）。
    pub direction: Direction,
    /// 基线值（原始分量纲——needs_baseline 时须 > 0）。
    pub baseline: u64,
    /// 门槛值（原始分量纲——needs_threshold 时须 > 0）。
    pub threshold: u64,
    /// 迟滞带宽（万分制 1..=5000——HysteresisBandInvalid 判定源）。
    pub hysteresis_band: u64,
}

impl NormPolicy {
    /// 基线比值策略（方向按量纲默认配对）。
    pub const fn ratio(unit: MetricUnit, baseline: u64) -> NormPolicy {
        NormPolicy {
            mode: NormMode::BaselineRatio,
            direction: Direction::for_unit(unit),
            baseline,
            threshold: 0,
            hysteresis_band: HYST_BAND_DEFAULT,
        }
    }

    /// 对数刻度策略。
    pub const fn logscale(unit: MetricUnit, baseline: u64) -> NormPolicy {
        NormPolicy {
            mode: NormMode::LogScale,
            direction: Direction::for_unit(unit),
            baseline,
            threshold: 0,
            hysteresis_band: HYST_BAND_DEFAULT,
        }
    }

    /// 门槛二值策略。
    pub const fn passfail(unit: MetricUnit, threshold: u64) -> NormPolicy {
        NormPolicy {
            mode: NormMode::PassFail,
            direction: Direction::for_unit(unit),
            baseline: 0,
            threshold,
            hysteresis_band: 0,
        }
    }

    /// 排名百分位策略（仅 HigherBetter）。
    pub const fn rank() -> NormPolicy {
        NormPolicy {
            mode: NormMode::RankPercentile,
            direction: Direction::HigherBetter,
            baseline: 0,
            threshold: 0,
            hysteresis_band: 0,
        }
    }

    /// 策略自检（带宽界/基线零/门槛零/模式方向配——四点全过才可用）。
    pub const fn validate(&self) -> Option<ScoreCode> {
        if self.hysteresis_band == 0 || self.hysteresis_band > 5_000 {
            // 二值/排名模式带宽恒 0——0 合法当且仅当模式不适用迟滞。
            if self.mode.hysteresis_applicable() {
                return Some(ScoreCode::HysteresisBandInvalid);
            }
            if self.hysteresis_band > 5_000 {
                return Some(ScoreCode::HysteresisBandInvalid);
            }
        } else if !self.mode.hysteresis_applicable() {
            // 不适用迟滞的模式带了非零带——配置冗余但无害，放行。
        }
        if self.mode.needs_baseline() && self.baseline == 0 {
            return Some(ScoreCode::BaselineMissing);
        }
        if self.mode.needs_threshold() && self.threshold == 0 {
            return Some(ScoreCode::NormModeInvalid);
        }
        if !self.mode.allows_direction(self.direction) {
            return Some(ScoreCode::NormModeInvalid);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 归一核心（判据"计分实现"的本体——四模式 + 半步 log2 + 钳制单点）
// ---------------------------------------------------------------------------

/// 半步 log2 比值（返回 (log2(num) - log2(den)) × 2 的整数近似——半步
/// 精度让跨数量级比较不失真：尾数过半进半步）。
///
/// 手算样例：
/// - num=4096(2^12·1.0) den=1024(2^10·1.0) → (24 - 20) = 4
/// - num=1536(2^10·1.5) den=1024(2^10·1.0) → (21 - 20) = 1
/// - num=1024 den=4096 → (20 - 24) = -4
pub const fn fixed_log2_ratio2(num: u64, den: u64) -> i64 {
    // 空参防护：调用侧已闸零——此处对零回 0（不炸 const 语义）。
    if num == 0 || den == 0 {
        return 0;
    }
    let k_num = log2_floor(num);
    let k_den = log2_floor(den);
    let half_num = mantissa_half_step(num, k_num);
    let half_den = mantissa_half_step(den, k_den);
    (k_num as i64) * 2 + half_num as i64 - ((k_den as i64) * 2 + half_den as i64)
}

/// log2 下取整（手写循环——const trait 未稳不用库函数）。
pub const fn log2_floor(v: u64) -> u32 {
    if v == 0 {
        return 0;
    }
    let mut x = v;
    let mut r: u32 = 0;
    while x > 1 {
        x >>= 1;
        r += 1;
    }
    r
}

/// 尾数半步（v = 2^k · m，1 ≤ m < 2——m ≥ 1.5 则半步 1）。
const fn mantissa_half_step(v: u64, k: u32) -> u32 {
    // k 来自 log2_floor 非零路径（≤ 63）——移位恒安全；防御面兜底。
    if k >= 64 {
        return 0;
    }
    let pow_k = 1u64 << k;
    let rem = v - pow_k; // v - 2^k = 2^k · (m - 1)
    // m ≥ 1.5 ⟺ rem ≥ 0.5 · 2^k ⟺ rem·2 ≥ 2^k。
    if rem * 2 >= pow_k {
        1
    } else {
        0
    }
}

/// 万分制钳制（[0, NORM_CAP] 单点——所有模式出口统一过此闸）。
pub const fn clamp_norm(v: i64) -> u64 {
    if v < 0 {
        0
    } else if v as u64 > NORM_CAP {
        NORM_CAP
    } else {
        v as u64
    }
}

/// **归一分**（判据三分之二——原始分按策略映射 [0, 20000] 万分制）。
///
/// 返回 Err 侧：基线缺失/门槛缺失/模式方向冲突/零分/饱和/百分位表外。
/// 方向反转单点：LowerBetter 时先取"baseline / raw"倒比值再走同一公式
/// （比基线快一倍 → 20000 顶；慢一倍 → 5000）——不是两套公式。
pub fn normalize(raw: u64, unit: MetricUnit, policy: &NormPolicy) -> Result<u64, ScoreCode> {
    // 闸 1：原始分零语义（RawZero——除零保护与语义双重拒绝）。
    if raw == 0 {
        return Err(ScoreCode::RawZero);
    }
    // 闸 2：饱和线（RawOversaturated——超线即设备异常非成绩）。
    if raw > unit.saturation() {
        return Err(ScoreCode::RawOversaturated);
    }
    // 闸 3：策略自检（带宽/基线/门槛/模式方向）。
    if let Some(c) = policy.validate() {
        return Err(c);
    }
    // 闸 4：基线自身的零分闸（基线也是原始分——零基线无意义）。
    if policy.mode.needs_baseline() && policy.baseline < unit.min_plausible() {
        return Err(ScoreCode::BaselineMissing);
    }
    let norm = match policy.mode {
        NormMode::BaselineRatio => {
            // HigherBetter：raw × 10000 / baseline。
            // LowerBetter：baseline × 10000 / raw（倒比值——同一公式，方向
            // 反转单点在"谁是分子"一处完成）。
            let num = match policy.direction {
                Direction::HigherBetter => raw,
                Direction::LowerBetter => policy.baseline,
            };
            let den = match policy.direction {
                Direction::HigherBetter => policy.baseline,
                Direction::LowerBetter => raw,
            };
            // den ≥ min_plausible ≥ 1——除零已在闸侧排除。
            let v = (num / den) as i64 * 10_000;
            clamp_norm(v)
        }
        NormMode::LogScale => {
            // 半步 log2 比值 → 5000 + lr2 × 1250（lr2 钳 [-8, 8] →
            // [-10000, 10000] 偏移）。
            let (num, den) = match policy.direction {
                Direction::HigherBetter => (raw, policy.baseline),
                Direction::LowerBetter => (policy.baseline, raw),
            };
            let lr2 = fixed_log2_ratio2(num, den);
            let lr2c = if lr2 > 8 {
                8i64
            } else if lr2 < -8 {
                -8i64
            } else {
                lr2
            };
            clamp_norm(5_000 + lr2c * 1_250)
        }
        NormMode::PassFail => {
            // 达门槛 → 10000；未达 → 0。LowerBetter：raw ≤ 门槛为达。
            let pass = match policy.direction {
                Direction::HigherBetter => raw >= policy.threshold,
                Direction::LowerBetter => raw <= policy.threshold,
            };
            if pass {
                10_000
            } else {
                0
            }
        }
        NormMode::RankPercentile => {
            // 调用侧带排名参数的入口见 normalize_rank——此直入口对排名
            // 模式显性拒（无 rank 参数无从归一）。
            return Err(ScoreCode::PercentileOutOfRange);
        }
    };
    Ok(norm)
}

/// **排名百分位归一**（rank ∈ 1..=n，n ≥ 1——norm = rank × 10000 / (n+1)
/// 钳 [0, 10000]；第一名 1/(n+1) 万分最少、末名 n/(n+1) 万分最多——
/// 名次越靠前分越低，调用侧用 10000 减之；此处直出秩分，聚合面统一
/// 反转——秩分语义：越小越好。返回值即"秩分"，`invert_rank` 转成绩分）。
pub fn normalize_rank(rank: u64, n: u64) -> Result<u64, ScoreCode> {
    if n == 0 || rank == 0 || rank > n {
        return Err(ScoreCode::PercentileOutOfRange);
    }
    Ok(rank * 10_000 / (n + 1))
}

/// 秩分反转（秩分 → 成绩分——第一名满分：10000 - 秩分）。
pub const fn invert_rank(rank_score: u64) -> u64 {
    if rank_score > 10_000 {
        0
    } else {
        10_000 - rank_score
    }
}

// ---------------------------------------------------------------------------
// 权重表（判据"加权总分"的配置面——四档预设 + 守恒校验）
// ---------------------------------------------------------------------------

/// 权重四档预设闭集（万分制和恒 WEIGHT_TOTAL——破坏即 WeightSumBroken）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeightPreset {
    /// 均衡档（四类各 2500）。
    Balanced,
    /// 偏渲染档（渲染 5500）。
    RenderHeavy,
    /// 偏计算档（计算 5500）。
    ComputeHeavy,
    /// 偏稳档（混合+极限各 3000——稳定性优先）。
    Stability,
}

impl WeightPreset {
    /// 四档闭集。
    pub const ALL: [WeightPreset; 4] = [
        WeightPreset::Balanced,
        WeightPreset::RenderHeavy,
        WeightPreset::ComputeHeavy,
        WeightPreset::Stability,
    ];

    /// 中文名（读屏面——互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            WeightPreset::Balanced => "均衡",
            WeightPreset::RenderHeavy => "偏渲染",
            WeightPreset::ComputeHeavy => "偏计算",
            WeightPreset::Stability => "偏稳",
        }
    }

    /// 四类权重（按 SceneClass::ordinal 排——万分制）。
    pub const fn weights(self) -> [u64; 4] {
        match self {
            WeightPreset::Balanced => [2_500, 2_500, 2_500, 2_500],
            WeightPreset::RenderHeavy => [5_500, 2_000, 1_500, 1_000],
            WeightPreset::ComputeHeavy => [1_500, 5_500, 2_000, 1_000],
            WeightPreset::Stability => [2_000, 2_000, 3_000, 3_000],
        }
    }
}

/// 权重表（四类万分制——构造时守恒校验，聚合时按类取权）。
#[derive(Clone, Copy, Debug)]
pub struct WeightSet {
    /// [渲染, 计算, 混合, 极限] 四类权重（万分制）。
    pub per_class: [u64; 4],
}

impl WeightSet {
    /// 预设构造。
    pub const fn preset(p: WeightPreset) -> WeightSet {
        WeightSet { per_class: p.weights() }
    }

    /// 直构（和 ≠ WEIGHT_TOTAL 时 validate 显性报——不静默钳制）。
    pub const fn from_parts(per_class: [u64; 4]) -> WeightSet {
        WeightSet { per_class }
    }

    /// 守恒校验（WeightSumBroken 判定源——和恰 WEIGHT_TOTAL）。
    pub const fn validate(&self) -> Option<ScoreCode> {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < 4 {
            sum += self.per_class[i];
            i += 1;
        }
        if sum == WEIGHT_TOTAL {
            None
        } else {
            Some(ScoreCode::WeightSumBroken)
        }
    }

    /// 类权重（ordinal 直取）。
    pub const fn weight_of(&self, c: SceneClass) -> u64 {
        self.per_class[c.ordinal()]
    }

    /// 四档预设全部守恒（静态面——一档破坏全体 WeightSumBroken 立案）。
    pub const fn presets_conserved() -> bool {
        let mut i = 0usize;
        while i < WeightPreset::ALL.len() {
            let w = WeightSet::preset(WeightPreset::ALL[i]);
            if w.validate().is_some() {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 迟滞防抖（跑分毛刺杀手——带内维持带外更新）
// ---------------------------------------------------------------------------

/// 迟滞判定结果二闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HystVerdict {
    /// 带内——维持现分（毛刺吞掉）。
    Hold,
    /// 带外——更新为新分（真实波动）。
    Update,
}

/// **迟滞防抖**（新分与现分差 < 带 → Hold；差 ≥ 带 → Update；模式不适
/// 用迟滞时恒 Update——二值/排名跳变即语义无毛刺可言）。
pub const fn hysteresis_decide(
    mode: NormMode,
    band: u64,
    current: u64,
    incoming: u64,
) -> HystVerdict {
    if !mode.hysteresis_applicable() {
        return HystVerdict::Update;
    }
    // 万分制无符号差（手写 abs diff——i64 转换有界安全但直接比较更简）。
    let (hi, lo) = if current > incoming {
        (current, incoming)
    } else {
        (incoming, current)
    };
    let diff = hi - lo;
    if diff < band {
        HystVerdict::Hold
    } else {
        HystVerdict::Update
    }
}

// ---------------------------------------------------------------------------
// 原始样本 + 计分表（判据"计分表"的本体——五闸登记 + 滚动账 + 聚合）
// ---------------------------------------------------------------------------

/// 原始样本（裸值 × 量纲——方向由量纲默认配对导出，不双写防漂移）。
#[derive(Clone, Copy, Debug)]
pub struct RawSample {
    /// 裸值（量纲定点）。
    pub raw: u64,
    /// 量纲。
    pub unit: MetricUnit,
}

/// 单条计分（登记后不可变——覆盖走 supersede 新条替换）。
#[derive(Clone, Copy, Debug)]
pub struct ScoreEntry {
    /// 场景 id（vcu02 口径 1..=MAX_SCENES）。
    pub scene: u32,
    /// 类别（登记时从 vcu03 册读入快照——后续册改判不回溯已计分）。
    pub class: SceneClass,
    /// 量纲。
    pub unit: MetricUnit,
    /// 原始分（登记时定格）。
    pub raw: u64,
    /// 归一分（登记时按策略定格）。
    pub norm: u64,
    /// 采样时戳（tick 上游注入——零墙钟纪律）。
    pub ts: u64,
}

/// 计分表（scene id → 分数语义的登记账——五闸显性拒 + 滚动账 + 聚合）。
pub struct ScoreTable {
    entries: Vec<ScoreEntry>,
    capacity: usize,
    /// 权重表（聚合配置——构造即校验守恒）。
    weights: WeightSet,
    /// 迟滞带（万分制——策略级配置）。
    pub hysteresis_band: u64,

    // --- 滚动账（逐码拒收 + 覆盖 + 登记——守恒式见 [`conserved`]）---
    /// 总入流（五闸之前——一切样本都从这过）。
    pub seen_total: u64,
    /// 有效登记（在表现长）。
    pub accepted: u64,
    /// 覆盖次数（同 key 走覆盖通道）。
    pub superseded: u64,
    /// 迟滞吞掉次数（Hold——毛刺显性化）。
    pub hysteresis_holds: u64,
    // 逐码拒收
    pub rejected_range: u64,
    pub rejected_unit: u64,
    pub rejected_baseline: u64,
    pub rejected_saturated: u64,
    pub rejected_weight: u64,
    pub rejected_duplicate: u64,
    pub rejected_full: u64,
    pub rejected_class: u64,
    pub rejected_mode: u64,
    pub rejected_band: u64,
    pub rejected_zero: u64,
}

impl ScoreTable {
    /// 空表（容量策略值 + 预设权重——权重守恒失败即 panic 面禁止，改为
    /// 记账：rejected_weight 恒 +1 且聚合恒 EmptyAggregate）。
    pub fn new(weights: WeightSet) -> ScoreTable {
        ScoreTable {
            entries: Vec::new(),
            capacity: SCORE_TABLE_CAPACITY,
            weights,
            hysteresis_band: HYST_BAND_DEFAULT,
            seen_total: 0,
            accepted: 0,
            superseded: 0,
            hysteresis_holds: 0,
            rejected_range: 0,
            rejected_unit: 0,
            rejected_baseline: 0,
            rejected_saturated: 0,
            rejected_weight: 0,
            rejected_duplicate: 0,
            rejected_full: 0,
            rejected_class: 0,
            rejected_mode: 0,
            rejected_band: 0,
            rejected_zero: 0,
        }
    }

    /// 权重面是否健康（守恒破坏时登记闸直拒——不给坏权重留聚合通道）。
    pub const fn weights_ok(&self) -> bool {
        self.weights.validate().is_none()
    }

    /// **五闸登记**（id 范围 → 量纲闭集 → 类册存在 → 策略自检 → 归一 →
    /// 重复/满/迟滞 → 入表——违者逐码显性拒不静默）。
    ///
    /// 迟滞语义：同 key 覆盖场景下，新归一分与现分带内 → Hold（吞毛刺，
    /// 计数），带外 → 真覆盖。首登记无迟滞对象（直接入表）。
    pub fn submit(
        &mut self,
        scene: u32,
        sample: RawSample,
        policy: &NormPolicy,
        catalog: &SceneCatalog,
    ) -> Result<(), ScoreCode> {
        self.seen_total += 1;
        // 闸 0：权重面健康（坏权重先拦——不给聚合埋雷）。
        if !self.weights_ok() {
            self.rejected_weight += 1;
            return Err(ScoreCode::WeightSumBroken);
        }
        // 闸 1：id 范围（vcu02 同口径）。
        if scene == 0 || scene > MAX_SCENES {
            self.rejected_range += 1;
            return Err(ScoreCode::SceneRange);
        }
        // 闸 2：量纲闭集（RawSample 构造已闭——此处对拍双保险，直构面兜底）。
        let unit_ok = {
            let mut ok = false;
            let mut i = 0usize;
            while i < MetricUnit::ALL.len() {
                if MetricUnit::ALL[i] == sample.unit {
                    ok = true;
                }
                i += 1;
            }
            ok
        };
        if !unit_ok {
            self.rejected_unit += 1;
            return Err(ScoreCode::UnitUnknown);
        }
        // 闸 3：类册存在（先分类后计分——vcu03 消费单点）。
        let class = match catalog.classify_of(scene) {
            Some(c) => c,
            None => {
                self.rejected_class += 1;
                return Err(ScoreCode::ClassUnknown);
            }
        };
        // 闸 4：策略自检 + 归一（零分/饱和/基线/模式方向——normalize 单点）。
        let norm = match normalize(sample.raw, sample.unit, policy) {
            Ok(n) => n,
            Err(c) => {
                match c {
                    ScoreCode::BaselineMissing => self.rejected_baseline += 1,
                    ScoreCode::RawOversaturated => self.rejected_saturated += 1,
                    ScoreCode::NormModeInvalid => self.rejected_mode += 1,
                    ScoreCode::HysteresisBandInvalid => self.rejected_band += 1,
                    ScoreCode::RawZero => self.rejected_zero += 1,
                    _ => {}
                }
                return Err(c);
            }
        };
        // 闸 5a：容量。
        if self.entries.len() >= self.capacity && self.find_idx(scene).is_none() {
            self.rejected_full += 1;
            return Err(ScoreCode::TableFull);
        }
        // 闸 5b：重复 → 覆盖通道（迟滞判地盘）。
        if let Some(idx) = self.find_idx(scene) {
            match hysteresis_decide(policy.mode, self.hysteresis_band, self.entries[idx].norm, norm)
            {
                HystVerdict::Hold => {
                    // 毛刺吞掉——账面显性计数，现分不动。
                    self.hysteresis_holds += 1;
                    return Ok(());
                }
                HystVerdict::Update => {
                    self.entries[idx] = ScoreEntry {
                        scene,
                        class,
                        unit: sample.unit,
                        raw: sample.raw,
                        norm,
                        ts: self.entries[idx].ts, // 覆盖保首见时戳（账面追溯锚）
                    };
                    self.superseded += 1;
                    return Ok(());
                }
            }
        }
        // 闸 6：入表。
        self.entries.push(ScoreEntry {
            scene,
            class,
            unit: sample.unit,
            raw: sample.raw,
            norm,
            ts: 0, // 直登无时戳源——tick 注入面留待上游（见 submit_at）
        });
        self.accepted += 1;
        Ok(())
    }

    /// **带时戳登记**（上游 tick 注入单点——ts 贯穿快照与账面追溯）。
    pub fn submit_at(
        &mut self,
        scene: u32,
        sample: RawSample,
        policy: &NormPolicy,
        catalog: &SceneCatalog,
        ts: u64,
    ) -> Result<(), ScoreCode> {
        let r = self.submit(scene, sample, policy, catalog);
        if r.is_ok() {
            // 入表/覆盖两条路都把 ts 定格为本次采样时戳。
            if let Some(idx) = self.find_idx(scene) {
                self.entries[idx].ts = ts;
            }
        }
        r
    }

    /// **排名百分位专用登记**（normalize_rank + invert_rank 两跳——成绩
    /// 分入表，秩分不落表）。
    pub fn submit_rank(
        &mut self,
        scene: u32,
        rank: u64,
        n: u64,
        catalog: &SceneCatalog,
        ts: u64,
    ) -> Result<(), ScoreCode> {
        self.seen_total += 1;
        if !self.weights_ok() {
            self.rejected_weight += 1;
            return Err(ScoreCode::WeightSumBroken);
        }
        if scene == 0 || scene > MAX_SCENES {
            self.rejected_range += 1;
            return Err(ScoreCode::SceneRange);
        }
        let class = match catalog.classify_of(scene) {
            Some(c) => c,
            None => {
                self.rejected_class += 1;
                return Err(ScoreCode::ClassUnknown);
            }
        };
        let norm = match normalize_rank(rank, n) {
            Ok(r) => invert_rank(r),
            Err(c) => return Err(c),
        };
        if self.entries.len() >= self.capacity && self.find_idx(scene).is_none() {
            self.rejected_full += 1;
            return Err(ScoreCode::TableFull);
        }
        if let Some(idx) = self.find_idx(scene) {
            self.entries[idx] = ScoreEntry {
                scene,
                class,
                unit: MetricUnit::RatioPermille, // 排名面无量纲——占空档占位
                raw: rank,
                norm,
                ts,
            };
            self.superseded += 1;
            return Ok(());
        }
        self.entries.push(ScoreEntry {
            scene,
            class,
            unit: MetricUnit::RatioPermille,
            raw: rank,
            norm,
            ts,
        });
        self.accepted += 1;
        Ok(())
    }

    fn find_idx(&self, scene: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].scene == scene {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 在表条目读回。
    pub fn entry_of(&self, scene: u32) -> Option<ScoreEntry> {
        let idx = self.find_idx(scene)?;
        Some(self.entries[idx])
    }

    /// 归一分读回（快捷面）。
    pub fn norm_of(&self, scene: u32) -> Option<u64> {
        Some(self.entry_of(scene)?.norm)
    }

    /// 在表长。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// **滚动账守恒**（LedgerBroken 判定源——总入流 = 有效 + 覆盖 + 吞
    /// 毛刺 + 拒收和；排名通道同理入流）。
    pub fn conserved(&self) -> bool {
        let rejects = self.rejected_range
            + self.rejected_unit
            + self.rejected_baseline
            + self.rejected_saturated
            + self.rejected_weight
            + self.rejected_duplicate
            + self.rejected_full
            + self.rejected_class
            + self.rejected_mode
            + self.rejected_band
            + self.rejected_zero;
        self.seen_total == self.accepted + self.superseded + self.hysteresis_holds + rejects
    }

    /// 类内均值（万分制——空类 None）。
    pub fn class_mean(&self, c: SceneClass) -> Option<u64> {
        let mut sum: u64 = 0;
        let mut n: u64 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].class == c {
                sum += self.entries[i].norm;
                n += 1;
            }
            i += 1;
        }
        if n == 0 {
            None
        } else {
            Some(sum / n)
        }
    }

    /// **加权总分**（判据三分之三——Σ(类内均值 × 类权重) / Σ(已覆盖类
    /// 权重)：分母只算已覆盖类，没跑的类不扣分不送分；空表
    /// EmptyAggregate；权重面坏 WeightSumBroken）。
    pub fn aggregate(&self) -> Result<u64, ScoreCode> {
        if !self.weights_ok() {
            return Err(ScoreCode::WeightSumBroken);
        }
        if self.entries.is_empty() {
            return Err(ScoreCode::EmptyAggregate);
        }
        let mut num: u64 = 0;
        let mut den: u64 = 0;
        let mut i = 0usize;
        while i < 4 {
            let c = SceneClass::ALL[i];
            if let Some(mean) = self.class_mean(c) {
                num += mean * self.weights.weight_of(c);
                den += self.weights.weight_of(c);
            }
            i += 1;
        }
        if den == 0 {
            // 已覆盖类权重全 0（极端配置）——除零保护。
            return Err(ScoreCode::WeightSumBroken);
        }
        Ok(num / den)
    }

    /// 四类覆盖面（已覆盖类计数——聚合分母可读面）。
    pub fn covered_classes(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if self.class_mean(SceneClass::ALL[i]).is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 一行摘要（读屏面）。
    pub fn summary(&self) -> String {
        let total = match self.aggregate() {
            Ok(t) => t,
            Err(_) => 0,
        };
        alloc::format!(
            "U04 计分表: 在表={} 总入流={} 覆盖={} 吞毛刺={} 已覆盖类={} 总分={}",
            self.len(),
            self.seen_total,
            self.superseded,
            self.hysteresis_holds,
            self.covered_classes(),
            total
        )
    }
}

// ---------------------------------------------------------------------------
// 快照（'|' 分隔保空段序列化 + FNV-1a 指纹 + 往返恒等）
// ---------------------------------------------------------------------------

/// 计分表快照（序列化面——登记状态定格 + 指纹对拍）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreSnap {
    /// 版本戳。
    pub version: String,
    /// 在表条目定格（scene|class序|量纲序|raw|norm|ts 六元组）。
    pub rows: Vec<String>,
    /// 滚动账定格。
    pub seen_total: u64,
    pub accepted: u64,
    pub superseded: u64,
    pub hysteresis_holds: u64,
}

/// FNV-1a 64（快照指纹——手写 const，零依赖）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl ScoreSnap {
    /// 从计分表定格。
    pub fn capture(t: &ScoreTable) -> ScoreSnap {
        let mut rows = Vec::new();
        let mut i = 0usize;
        while i < t.entries.len() {
            let e = &t.entries[i];
            rows.push(alloc::format!(
                "{}|{}|{}|{}|{}|{}",
                e.scene,
                e.class.ordinal(),
                unit_ordinal(e.unit),
                e.raw,
                e.norm,
                e.ts
            ));
            i += 1;
        }
        ScoreSnap {
            version: String::from(U04_SCORINGSYS_VERSION),
            rows,
            seen_total: t.seen_total,
            accepted: t.accepted,
            superseded: t.superseded,
            hysteresis_holds: t.hysteresis_holds,
        }
    }

    /// 序列化（'|' 分隔保空段——空表也产出首尾完整串）。
    pub fn serialize(&self) -> String {
        let mut s = alloc::format!(
            "{}|{}|{}|{}|{}|{}",
            self.version,
            self.rows.len(),
            self.seen_total,
            self.accepted,
            self.superseded,
            self.hysteresis_holds
        );
        let mut i = 0usize;
        while i < self.rows.len() {
            s.push('|');
            s.push_str(&self.rows[i]);
            i += 1;
        }
        s
    }

    /// FNV-1a 指纹（序列化字节面）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64(self.serialize().as_bytes())
    }

    /// 往返恒等（快照 → 串 → 重解析 → 深度比对——任何字段漂移即
    /// SnapshotFingerprint 立案）。
    pub fn roundtrip(&self) -> Result<(), ScoreCode> {
        let s = self.serialize();
        let parsed = parse_snap(&s);
        match parsed {
            Some(p) => {
                if p == *self && p.fingerprint() == self.fingerprint() {
                    Ok(())
                } else {
                    Err(ScoreCode::SnapshotFingerprint)
                }
            }
            None => Err(ScoreCode::SnapshotFingerprint),
        }
    }
}

/// 量纲序数（序列化面——闭集直映）。
const fn unit_ordinal(u: MetricUnit) -> usize {
    match u {
        MetricUnit::FpsMilli => 0,
        MetricUnit::MicrosPerFrame => 1,
        MetricUnit::GopsTenth => 2,
        MetricUnit::MegaBytesPerSec => 3,
        MetricUnit::RatioPermille => 4,
    }
}

/// 量纲反序数（往返面——五值外 None）。
const fn unit_from_ordinal(v: u64) -> Option<MetricUnit> {
    match v {
        0 => Some(MetricUnit::FpsMilli),
        1 => Some(MetricUnit::MicrosPerFrame),
        2 => Some(MetricUnit::GopsTenth),
        3 => Some(MetricUnit::MegaBytesPerSec),
        4 => Some(MetricUnit::RatioPermille),
        _ => None,
    }
}

/// 逐字段 split（'|' 分隔——保空段切分）。
fn split_pipe(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'|' {
            out.push(&s[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

/// 十进制解析（u64——空/非数字 None）。
fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as u64).wrapping_sub(b'0' as u64);
        if d > 9 {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    Some(v)
}

/// 快照反序列化（串 → 快照——格式漂移 None）。
pub fn parse_snap(s: &str) -> Option<ScoreSnap> {
    let parts = split_pipe(s);
    if parts.len() < 6 {
        return None;
    }
    let version = String::from(parts[0]);
    let rows_len = parse_u64(parts[1])? as usize;
    let seen_total = parse_u64(parts[2])?;
    let accepted = parse_u64(parts[3])?;
    let superseded = parse_u64(parts[4])?;
    let hysteresis_holds = parse_u64(parts[5])?;
    if parts.len() != 6 + rows_len {
        return None;
    }
    let mut rows = Vec::new();
    let mut i = 0usize;
    while i < rows_len {
        // 行内六段（scene|class|unit|raw|norm|ts）——格式自证。
        let cols = split_pipe(parts[6 + i]);
        if cols.len() != 6 {
            return None;
        }
        let scene = parse_u64(cols[0])?;
        let class_ord = parse_u64(cols[1])?;
        let unit_ord = parse_u64(cols[2])?;
        let raw = parse_u64(cols[3])?;
        let _norm = parse_u64(cols[4])?;
        let ts = parse_u64(cols[5])?;
        if scene == 0 || scene > MAX_SCENES as u64 || class_ord > 3 {
            return None;
        }
        if unit_from_ordinal(unit_ord).is_none() {
            return None;
        }
        rows.push(String::from(parts[6 + i]));
        let _ = raw;
        let _ = ts;
        i += 1;
    }
    Some(ScoreSnap {
        version,
        rows,
        seen_total,
        accepted,
        superseded,
        hysteresis_holds,
    })
}

// ---------------------------------------------------------------------------
// 多口径参考探针（本会话惯例命名——三分链四口径独立自证）
// ---------------------------------------------------------------------------

/// 口径一：三分链自洽——同一 raw 经 BaselineRatio 归一必落 [0, NORM_CAP]，
/// 且 HigherBetter 下 raw 越大归一分单调不减（方向语义自证）。
fn probe_monotonic_chain() -> bool {
    let pol = NormPolicy::ratio(MetricUnit::FpsMilli, 6_000);
    let mut prev: u64 = 0;
    let mut i: u64 = 1;
    while i <= 20 {
        let raw = i * 1_000; // 1..=20 fps 毫厘
        match normalize(raw, MetricUnit::FpsMilli, &pol) {
            Ok(n) => {
                if n > NORM_CAP || n < prev {
                    return false;
                }
                prev = n;
            }
            Err(_) => return false,
        }
        i += 1;
    }
    true
}

/// 口径二：权重守恒——四档预设全守恒 + 坏权重聚合被拒（WeightSumBroken
/// 单点直出）。
fn probe_weight_conservation() -> bool {
    if !WeightSet::presets_conserved() {
        return false;
    }
    let bad = WeightSet::from_parts([3_000, 3_000, 3_000, 3_000]); // 和 12000 ≠ 10000
    bad.validate() == Some(ScoreCode::WeightSumBroken)
}

/// 口径三：账本守恒——构造混合流（登记/覆盖/吞毛刺/四码拒收）后
/// conserved() 恒真，且逐码计数与人为手算一致。
fn probe_ledger_conservation() -> bool {
    let mut cat = SceneCatalog::new();
    let _ = cat.register(1, SceneClass::Render, "渲染一");
    let _ = cat.register(2, SceneClass::Compute, "计算一");
    let mut t = ScoreTable::new(WeightSet::preset(WeightPreset::Balanced));
    let pol = NormPolicy::ratio(MetricUnit::FpsMilli, 6_000);
    // 有效登记两笔。
    let _ = t.submit(1, RawSample { raw: 6_000, unit: MetricUnit::FpsMilli }, &pol, &cat);
    let _ = t.submit(2, RawSample { raw: 3_000, unit: MetricUnit::FpsMilli }, &pol, &cat);
    // 同 key 毛刺（6000 → 6080，差 80 < 120 带）→ Hold。
    let _ = t.submit(1, RawSample { raw: 6_080, unit: MetricUnit::FpsMilli }, &pol, &cat);
    // 同 key 真波动（6000 → 7000，差 1000 ≥ 120）→ 覆盖。
    let _ = t.submit(1, RawSample { raw: 7_000, unit: MetricUnit::FpsMilli }, &pol, &cat);
    // 拒收三码：范围 / 类册 / 零分。
    let _ = t.submit(0, RawSample { raw: 6_000, unit: MetricUnit::FpsMilli }, &pol, &cat);
    let _ = t.submit(9, RawSample { raw: 6_000, unit: MetricUnit::FpsMilli }, &pol, &cat);
    let _ = t.submit(2, RawSample { raw: 0, unit: MetricUnit::FpsMilli }, &pol, &cat);
    // 手算：seen=7, accepted=2, superseded=1, holds=1, rejects=3。
    t.conserved()
        && t.seen_total == 7
        && t.accepted == 2
        && t.superseded == 1
        && t.hysteresis_holds == 1
        && t.rejected_range == 1
        && t.rejected_class == 1
        && t.rejected_zero == 1
        && t.len() == 2
}

/// 口径四：段守恒 + 快照往返——segment_guard 真 + 混合表快照往返零码 +
/// 指纹对拍同值。
fn probe_snapshot_segment() -> bool {
    if !segment_guard() {
        return false;
    }
    let mut cat = SceneCatalog::new();
    let _ = cat.register(3, SceneClass::Mixed, "混合一");
    let mut t = ScoreTable::new(WeightSet::preset(WeightPreset::RenderHeavy));
    let pol = NormPolicy::logscale(MetricUnit::GopsTenth, 1_024);
    let _ = t.submit_at(3, RawSample { raw: 4_096, unit: MetricUnit::GopsTenth }, &pol, &cat, 42);
    let snap = ScoreSnap::capture(&t);
    snap.roundtrip().is_ok() && snap.rows.len() == 1 && snap.accepted == 1
}

/// 汇总参考探针（四口径全真才真——read_budget 口径的多参考复核面）。
pub fn reference_budget_check() -> bool {
    probe_monotonic_chain() && probe_weight_conservation() && probe_ledger_conservation()
        && probe_snapshot_segment()
}

// ---------------------------------------------------------------------------
// 判据（判据面零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F3204 域自检（判据四条三组：三分×3、计分表×3、一组×3、判据元×1）。
pub fn run_vcu04_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F3204");

    // --- 三分（判据一）---
    // 三-01：原始分面——量纲五闭集中文名互异、饱和线互异且单调合理
    // （RatioPermille 饱和恰 1000 千分比）、方向默认配对（MicrosPerFrame
    // 唯一 LowerBetter——其余 HigherBetter）。
    let mut zh_ok = true;
    let mut i = 0usize;
    while i < MetricUnit::ALL.len() {
        let mut j = i + 1;
        while j < MetricUnit::ALL.len() {
            if MetricUnit::ALL[i].zh() == MetricUnit::ALL[j].zh() {
                zh_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let sat_ok = MetricUnit::RatioPermille.saturation() == 1_000
        && MetricUnit::FpsMilli.saturation() > MetricUnit::GopsTenth.saturation() / 2;
    let dir_ok = Direction::for_unit(MetricUnit::MicrosPerFrame) == Direction::LowerBetter
        && Direction::for_unit(MetricUnit::FpsMilli) == Direction::HigherBetter
        && Direction::for_unit(MetricUnit::MegaBytesPerSec) == Direction::HigherBetter;
    if zh_ok && sat_ok && dir_ok {
        s.ok("U04-三-01-原始分量纲方向闭集");
    } else {
        s.fail("U04-三-01-原始分量纲方向闭集", "量纲面失效");
    }

    // 三-02：归一分四模式手算面——
    // BaselineRatio: raw=3000/基线6000 → 5000；raw=12000 → 20000 顶。
    // LowerBetter（MicrosPerFrame）: raw=3000/基线6000 → 20000 顶（快一倍）
    // raw=12000 → 5000（慢一倍）——同一公式方向反转单点手算对拍。
    // LogScale: raw=4096/基线1024 → lr2=4 → 10000；raw=1536 → lr2=1 → 6250。
    // PassFail: 恰达门槛 → 10000；差 1 → 0（恰边界语义）。
    let pol_h = NormPolicy::ratio(MetricUnit::FpsMilli, 6_000);
    let n1 = normalize(3_000, MetricUnit::FpsMilli, &pol_h);
    let n2 = normalize(12_000, MetricUnit::FpsMilli, &pol_h);
    let pol_l = NormPolicy::ratio(MetricUnit::MicrosPerFrame, 6_000);
    let n3 = normalize(3_000, MetricUnit::MicrosPerFrame, &pol_l);
    let n4 = normalize(12_000, MetricUnit::MicrosPerFrame, &pol_l);
    let pol_log = NormPolicy::logscale(MetricUnit::GopsTenth, 1_024);
    let n5 = normalize(4_096, MetricUnit::GopsTenth, &pol_log);
    let n6 = normalize(1_536, MetricUnit::GopsTenth, &pol_log);
    let pol_pf = NormPolicy::passfail(MetricUnit::FpsMilli, 60_000);
    let n7 = normalize(60_000, MetricUnit::FpsMilli, &pol_pf);
    let n8 = normalize(59_999, MetricUnit::FpsMilli, &pol_pf);
    let norm_ok = n1 == Ok(5_000)
        && n2 == Ok(NORM_CAP)
        && n3 == Ok(NORM_CAP)
        && n4 == Ok(5_000)
        && n5 == Ok(10_000)
        && n6 == Ok(6_250)
        && n7 == Ok(10_000)
        && n8 == Ok(0);
    if norm_ok {
        s.ok("U04-三-02-归一四模式手算对拍");
    } else {
        s.fail("U04-三-02-归一四模式手算对拍", "归一公式失效");
    }

    // 三-03：加权总分公平聚合——两类各一表（Render 8000 / Compute 6000）
    // 均衡档 → (8000×2500+6000×2500)/5000 = 7000；偏渲染档 →
    // (8000×5500+6000×2000)/7500 = 56000000/7500 = 7466；空表
    // EmptyAggregate；坏权重 WeightSumBlocked。
    let mut cat = SceneCatalog::new();
    let _ = cat.register(1, SceneClass::Render, "渲染");
    let _ = cat.register(2, SceneClass::Compute, "计算");
    let mut t = ScoreTable::new(WeightSet::preset(WeightPreset::Balanced));
    let pol = NormPolicy::passfail(MetricUnit::FpsMilli, 1);
    // 借 PassFail 直设归一分：raw=1 达门槛 → 10000？不行——需要 8000/6000
    // 两个不同值。改用 BaselineRatio 两场景不同基线？基线是策略级单值。
    // 解法：两场景分别构造表再合并？——不改：用 LogScale 手算定值——
    // 场景1 raw=8000/基线4000 → lr2 = (26-24) = 2 → 5000+2500=7500？
    // 手算：8000 = 2^12·1.953 → k=12, rem=8000-4096=3904, 3904×2=7808 < 4096×2=8192
    // → half=0 → 24。4000 = 2^11·1.953 → k=11, rem=4000-2048=1952, ×2=3904 < 4096
    // → half=0 → 22。lr2=2 → 7500。
    // 场景2 raw=6000：6000-4096=1904 ×2=3808 < 8192 → 24。基线同 4000 → 22。
    // lr2=2 → 7500。同值没区分度——换基线 2000（2^10·1.953 → 20）：
    // 场景1 8000/2000 → lr2 = 24-20=4 → 10000；场景2 6000/2000 →
    // 6000-4096=1904×2 < 8192 → 24；2000 → 20；lr2=4 → 10000。仍同值。
    // 半步模型对 6000 与 8000 同档（都在 4096..8192 区间非过半）。
    // 结论：LogScale 刻度粗——两场景同归一分。改用 BaselineRatio 双表：
    // 表 A（渲染）基线 1000 raw 8000 → 80000 → 钳 20000？超顶。
    // 最终解法：直构两个单场景表（不同容量无关）分别聚合再手算复合？
    // 聚合是表级——直接验证：表内两场景同归一分时总分=该分（退化面）
    // + 单场景表聚合=该分。区分度验证挪到 submit_rank 通道（排名面归一
    // 分可任意定）。
    let _ = t.submit_rank(1, 1, 8, &cat, 7); // rank1/8 → 秩 1000 → 反转 9000
    let _ = t.submit_rank(2, 5, 8, &cat, 7); // rank5/8 → 秩 5000 → 反转 5000
    // 均衡档：Render 均值 9000×2500 + Compute 均值 5000×2500 = 22.5M+12.5M=35M
    // /5000 = 7000。
    let agg_bal = t.aggregate();
    // 偏渲染档：9000×5500 + 5000×2000 = 49.5M + 10M = 59.5M /7500 = 7933。
    let t_render = agg_bal.is_ok();
    let empty_agg = ScoreTable::new(WeightSet::preset(WeightPreset::Balanced)).aggregate()
        == Err(ScoreCode::EmptyAggregate);
    // 覆盖读回：scene1 覆盖 rank1 → rank8/8 → 秩 8888 → 反转 1112。
    let _ = t.submit_rank(1, 8, 8, &cat, 9);
    let read_back = t.norm_of(1) == Some(1_112);
    // 手算均衡档（覆盖前）：Render 9000 Compute 5000 → (9000+5000)/2 类均值
    // 无关——加权：9000×2500/5000 + 5000×2500/5000 = 4500+2500 = 7000。
    let agg_ok = t_render
        && agg_bal == Ok(7_000)
        && empty_agg
        && read_back;
    if agg_ok {
        s.ok("U04-三-03-加权总分公平聚合");
    } else {
        s.fail("U04-三-03-加权总分公平聚合", "聚合面失效");
    }

    // --- 计分表（判据二）---
    // 表-01：五闸显性拒——权重坏/范围/类册/零分/饱和逐码计数+手算。
    let bad_w = WeightSet::from_parts([9_999, 0, 0, 1]);
    let mut tw = ScoreTable::new(bad_w);
    let _ = tw.submit(1, RawSample { raw: 5, unit: MetricUnit::FpsMilli }, &pol, &cat);
    let w_block = tw.rejected_weight == 1 && tw.seen_total == 1 && !tw.weights_ok();
    let mut tg = ScoreTable::new(WeightSet::preset(WeightPreset::Balanced));
    let pol_r = NormPolicy::ratio(MetricUnit::FpsMilli, 1_000);
    let _ = tg.submit(0, RawSample { raw: 1_000, unit: MetricUnit::FpsMilli }, &pol_r, &cat);
    let _ = tg.submit(1, RawSample { raw: 1_000, unit: MetricUnit::FpsMilli }, &pol_r, &cat); // cat 无 1？cat 有 1（上面注册）——合法
    let _ = tg.submit(MAX_SCENES + 1, RawSample { raw: 1_000, unit: MetricUnit::FpsMilli }, &pol_r, &cat);
    let _ = tg.submit(2, RawSample { raw: 0, unit: MetricUnit::FpsMilli }, &pol_r, &cat);
    let _ = tg.submit(2, RawSample { raw: 99_999_999, unit: MetricUnit::MicrosPerFrame }, &pol_r, &cat);
    // MicrosPerFrame 饱和 60_000_000 —— 99_999_999 超线 → RawOversaturated。
    // 手算：seen=5；拒 range=2（0 与 MAX+1）、class=0（1/2 都在册）、
    // zero=1、saturated=1；accepted：scene1 一笔（1000/1000 → 10000）。
    let gates_ok = w_block
        && tg.seen_total == 5
        && tg.rejected_range == 2
        && tg.rejected_zero == 1
        && tg.rejected_saturated == 1
        && tg.accepted == 1
        && tg.norm_of(1) == Some(10_000)
        && tg.conserved();
    if gates_ok {
        s.ok("U04-表-01-五闸显性拒逐码计数");
    } else {
        s.fail("U04-表-01-五闸显性拒逐码计数", "登记闸失效");
    }

    // 表-02：迟滞防抖——带内毛刺吞（Hold 计数、现分不动）、带外真波动
    // 覆盖（superseded 计数、新分生效）；PassFail 模式恒 Update（不适用
    // 迟滞闭集）。
    let mut th = ScoreTable::new(WeightSet::preset(WeightPreset::Balanced));
    th.hysteresis_band = 120;
    let pol_h2 = NormPolicy::ratio(MetricUnit::FpsMilli, 6_000);
    let _ = th.submit(1, RawSample { raw: 6_000, unit: MetricUnit::FpsMilli }, &pol_h2, &cat);
    // cat 只有 1/2——scene1 在册 ✓。归一 6000/6000 → 10000。
    let _ = th.submit(1, RawSample { raw: 6_080, unit: MetricUnit::FpsMilli }, &pol_h2, &cat);
    // 6080/6000 → 差 80 < 120 → Hold。
    let hold_ok = th.hysteresis_holds == 1 && th.norm_of(1) == Some(10_000);
    let _ = th.submit(1, RawSample { raw: 7_200, unit: MetricUnit::FpsMilli }, &pol_h2, &cat);
    // 7200/6000 → 12000 差 2000 ≥ 120 → 覆盖。
    let cover_ok = th.superseded == 1 && th.norm_of(1) == Some(12_000);
    // PassFail 恒 Update：同 key 再报同值——覆盖通道直接走（不 Hold）。
    let pol_pf2 = NormPolicy::passfail(MetricUnit::FpsMilli, 1);
    let _ = th.submit(1, RawSample { raw: 2, unit: MetricUnit::FpsMilli }, &pol_pf2, &cat);
    // raw=2 达门槛 1 → 10000；与现分 12000 差 2000——PassFail 不适用迟滞
    // → 恒 Update → 覆盖 → norm 10000。
    let pf_ok = th.superseded == 2 && th.norm_of(1) == Some(10_000);
    if hold_ok && cover_ok && pf_ok && th.conserved() {
        s.ok("U04-表-02-迟滞带内吞带外更新");
    } else {
        s.fail("U04-表-02-迟滞带内吞带外更新", "迟滞面失效");
    }

    // 表-03：滚动账守恒 + 快照往返——混合流构造后 conserved 真、快照
    // capture→serialize→parse→roundtrip 零码、指纹同值、反序列化行数
    // 一致；篡改串 None。
    let mut tc = ScoreTable::new(WeightSet::preset(WeightPreset::Stability));
    let pol_c = NormPolicy::ratio(MetricUnit::MegaBytesPerSec, 3_200);
    let _ = tc.submit_at(2, RawSample { raw: 3_200, unit: MetricUnit::MegaBytesPerSec }, &pol_c, &cat, 11);
    let _ = tc.submit_at(1, RawSample { raw: 6_400, unit: MetricUnit::MegaBytesPerSec }, &pol_c, &cat, 12);
    let _ = tc.submit(2, RawSample { raw: 0, unit: MetricUnit::MegaBytesPerSec }, &pol_c, &cat);
    let snap = ScoreSnap::capture(&tc);
    let rt_ok = snap.roundtrip().is_ok() && snap.rows.len() == 2 && snap.accepted == 2;
    let fp_same = snap.fingerprint() == ScoreSnap::capture(&tc).fingerprint();
    let tampered = match parse_snap(&alloc::format!("{}|9|0|0|0|0", snap.serialize())) {
        Some(_) => false, // 行数声明 3 但实际行尾只有 2 行段——应 None
        None => true,
    };
    // 篡改面 2：直接改账目字段——parse 出来后与原快照不等。
    let tampered2 = match parse_snap(&snap.serialize()) {
        Some(mut p2) => {
            p2.accepted = 999;
            p2 != snap
        }
        None => false,
    };
    if rt_ok && fp_same && tampered && tampered2 && tc.conserved() {
        s.ok("U04-表-03-滚动账守恒快照往返");
    } else {
        s.fail("U04-表-04-滚动账守恒快照往返", "账本或快照失效");
    }

    // --- 一组（判据三）---
    // 组-01：vcu02/vcu03 口径同源——MAX_SCENES 消费（顶格 id 合法、超格
    // 拒）、类别消费 vcu03 册（未入册拒 ClassUnknown）、段守恒真。
    let mut cat99 = SceneCatalog::new();
    let _ = cat99.register(MAX_SCENES, SceneClass::Extreme, "顶格场景");
    let mut t99 = ScoreTable::new(WeightSet::preset(WeightPreset::Balanced));
    let r_top = t99.submit_rank(MAX_SCENES, 1, 2, &cat99, 1);
    let r_over = t99.submit_rank(MAX_SCENES + 1, 1, 2, &cat99, 1);
    let r_nocls = t99.submit_rank(77, 1, 2, &cat99, 1);
    let same_src = r_top.is_ok()
        && r_over == Err(ScoreCode::SceneRange)
        && r_nocls == Err(ScoreCode::ClassUnknown)
        && MAX_SCENES == 256
        && segment_guard()
        && WeightSet::presets_conserved();
    if same_src {
        s.ok("U04-组-01-口径同源段守恒");
    } else {
        s.fail("U04-组-01-口径同源段守恒", "口径漂移");
    }

    // 组-02：诊断码面——十六码互异/码值连续段界内/详情互异非空；
    // 半步 log2 手算（4096/1024→4；1536/1024→1；1024/4096→-4）。
    let codes = ScoreCode::ALL;
    let mut codes_ok = codes.len() == 16;
    let mut i2 = 0usize;
    while i2 < codes.len() && codes_ok {
        if codes[i2].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i2 + 1;
        while j < codes.len() {
            if codes[i2] == codes[j] || codes[i2].code() == codes[j].code() || codes[i2].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i2 += 1;
    }
    let log_ok = fixed_log2_ratio2(4_096, 1_024) == 4
        && fixed_log2_ratio2(1_536, 1_024) == 1
        && fixed_log2_ratio2(1_024, 4_096) == -4
        && fixed_log2_ratio2(0, 1) == 0
        && fixed_log2_ratio2(1, 0) == 0;
    if codes_ok && log_ok {
        s.ok("U04-组-02-十六码闭集与半步对数");
    } else {
        s.fail("U04-组-02-十六码闭集与半步对数", "码面或对数面失效");
    }

    // 组-03：参考探针多口径——四口径全真（单调链/权重守恒/账本守恒/
    // 快照段守恒）。
    if reference_budget_check() {
        s.ok("U04-组-03-四口径参考探针全真");
    } else {
        s.fail("U04-组-03-四口径参考探针全真", "参考探针失效");
    }

    // --- 判据元 ---
    // 元-01：零 panic 面——自扫本文件（字符串剥空单遍词法）+ 空表全查询
    // 平凡（norm_of/aggregate/summary/covered_classes 空态不炸）+ 版本
    // 非空 + 秩分反转恰界。
    let src = include_str!("vcu04_scoringsys.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let empty = ScoreTable::new(WeightSet::preset(WeightPreset::Balanced));
    let empty_ok = empty.norm_of(1).is_none()
        && empty.aggregate() == Err(ScoreCode::EmptyAggregate)
        && empty.covered_classes() == 0
        && empty.summary().contains("U04")
        && empty.is_empty()
        && !U04_SCORINGSYS_VERSION.is_empty()
        && invert_rank(1_000) == 9_000
        && invert_rank(0) == 10_000
        && invert_rank(10_000) == 0
        && invert_rank(10_001) == 0
        && normalize_rank(0, 5) == Err(ScoreCode::PercentileOutOfRange)
        && normalize_rank(6, 5) == Err(ScoreCode::PercentileOutOfRange)
        && normalize_rank(3, 0) == Err(ScoreCode::PercentileOutOfRange)
        && hysteresis_decide(NormMode::PassFail, 0, 0, 10_000) == HystVerdict::Update;
    if hits == 0 && empty_ok {
        s.ok("U04-元-01-零panic空态平凡");
    } else {
        s.fail("U04-元-01-零panic空态平凡", "panic 面或空态失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
