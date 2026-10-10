//! CGPU-F0484 · 帧间隔统计（CGPU-D 域 · D04 组 · 目标 340 行 / 深化交付）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0484`
//!
//! **判据（锚点原文）**：直方图、stddev、分级、趋势、判据。
//!
//! **职责定位（锚点原文）**：间隔统计：间隔分布直方图（16.7ms 附近的分布
//! 形态——节奏健康度量化）；抖动 stddev（滚动窗口抖动度量）；jank 量化定义
//! （>1.5× 目标间隔=微卡——阈值分级：轻微/明显/事故）；长期趋势（间隔健康
//! 的周趋势）。
//!
//! ## 深化交付结构（十一节）
//!
//! 一、诊断码（0x63 段独占，D04 组间隔面六码）。
//! 二、间隔样本模型与滚动窗（样本注入 + 窗管理——零墙钟、全上游注入）。
//! 三、间隔直方图（桶闭集 + 累计 + 形态五分类——判据一）。
//! 四、抖动 stddev（整数口径方差与标准差近似——无浮点纪律；判据二）。
//! 五、jank 量化分级（轻微/明显/事故三级阈值；判据三）。
//! 六、jank 分级账（分级留痕 + 分级聚合）。
//! 七、长期趋势（周桶 + 整数斜率 + 趋势五态；判据四）。
//! 八、节奏健康度评分（直方图×抖动×jank 三因子合成）。
//! 九、审计账本（256 帧滚动 + 三态查询——复用 F0483 模式）。
//! 十、遥测导出（B08 五元组——与 F2408 统一模型对齐）。
//! 十一、对外接口冻结 v1 + 仿真走查（判据五）。
//!
//! **复用声明**（判据侧逐条 grep 对拍——只引用不重定义）：
//! - 复用 F0483 帧账的四段耗时与帧间隔记录——本模型消费其间隔序列。
//! - 复用 F2408 万分比整数口径——方差/斜率全部整数化，无浮点。
//! - 复用 F0485 帧率合同三条款（P95/P99/卡顿）——分级阈值与合同对齐。
//! - 复用 C02 帧预算 12500µs 口径——目标间隔锚点一致。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（0x63 段独占：D04 组间隔面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x63 段为本单独占段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FgCode {
    /// 样本非法（零间隔或倒流——时间单调性违约）。
    SampleInvalid = 0x6301,
    /// 窗空查（无样本时求统计量——显性拒不猜）。
    WindowEmpty = 0x6302,
    /// 目标间隔未设（分级/健康度依赖目标——缺锚显性拒）。
    TargetMissing = 0x6303,
    /// 直方图桶越界（桶表被误改——先红）。
    BucketBreach = 0x6304,
    /// 趋势桶越界（周桶表病）。
    TrendBreach = 0x6305,
    /// 样本超容（单帧间隔 > 1s——按异常帧入账但标记）。
    SampleOutlier = 0x6306,
}

impl FgCode {
    /// 人读诊断（异常零静默）。
    pub const fn detail(self) -> &'static str {
        match self {
            FgCode::SampleInvalid => "间隔样本非法：零间隔或时间倒流，建议核对时间戳单调源",
            FgCode::WindowEmpty => "统计窗空：无样本不可求统计量，建议先注入样本",
            FgCode::TargetMissing => "目标间隔未设：分级与健康度依赖目标锚，建议先设 16777µs（60Hz）或 12500µs（80Hz）",
            FgCode::BucketBreach => "直方图桶越界：桶表与样本值域不符，建议核对桶边界",
            FgCode::TrendBreach => "趋势桶越界：周桶索引出界，建议核对桶表",
            FgCode::SampleOutlier => "样本超容：单帧间隔超 1s（挂起/断连），已入账并标记——建议核查该帧上下文",
        }
    }

    /// 段守恒。
    pub const fn in_segment(self) -> bool {
        (self as u16) >= 0x6301 && (self as u16) <= 0x6306
    }
}

// ---------------------------------------------------------------------------
// 二、间隔样本模型与滚动窗
// ---------------------------------------------------------------------------

/// 滚动窗容量（256 样本——与账本滚动纪律一致）。
pub const WINDOW_CAPACITY: usize = 256;

/// 超容阈值（µs；超过按异常帧入账——FgCode::SampleOutlier 标记）。
pub const OUTLIER_THRESHOLD_US: u32 = 1_000_000;

/// 间隔样本（一帧一次；值域守卫在入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GapSample {
    /// 帧序（单调递增——单调性由窗守卫）。
    pub frame_no: u64,
    /// 帧间隔（µs）。
    pub gap_us: u32,
    /// 场景标签（归因维度）。
    pub scene: &'static str,
}

impl GapSample {
    /// 样本合法性（gap > 0；frame 单调由窗管）。
    pub const fn valid(&self) -> bool {
        self.gap_us > 0
    }

    /// 异常帧判。
    pub const fn outlier(&self) -> bool {
        self.gap_us > OUTLIER_THRESHOLD_US
    }
}

/// 滚动窗（256 样本封顶；时间单调守卫）。
#[derive(Clone, Debug, Default)]
pub struct GapWindow {
    samples: Vec<GapSample>,
}

impl GapWindow {
    /// 空窗。
    pub fn new() -> Self {
        GapWindow { samples: Vec::new() }
    }

    /// 注入（非法/倒流显性拒；封顶滚动——最老先出；异常帧入账）。
    pub fn push(&mut self, s: GapSample) -> Result<(), FgCode> {
        if !s.valid() {
            return Err(FgCode::SampleInvalid);
        }
        // 单调守卫：新帧序必须大于窗内最新帧序。
        if let Some(last) = self.samples.last() {
            if s.frame_no <= last.frame_no {
                return Err(FgCode::SampleInvalid);
            }
        }
        if self.samples.len() >= WINDOW_CAPACITY {
            self.samples.remove(0);
        }
        self.samples.push(s);
        Ok(())
    }

    /// 窗深。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 样本只读快照。
    pub fn snapshot(&self) -> Vec<GapSample> {
        let mut out: Vec<GapSample> = Vec::new();
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                out.push(*s);
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、间隔直方图（判据一：桶闭集 + 累计 + 形态分类）
// ---------------------------------------------------------------------------

/// 直方图桶（值域 [lo_us, hi_us)；闭集六桶——覆盖 0~2×80Hz 基准的关键带）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GapBucket {
    /// 桶名。
    pub name: &'static str,
    /// 下界（µs，含）。
    pub lo_us: u32,
    /// 上界（µs，不含）。
    pub hi_us: u32,
}

/// 桶表（闭集六桶：过快/略快/达标带/略慢/微卡带/超容带——80Hz 锚 12500µs）。
pub const GAP_BUCKETS: [GapBucket; 6] = [
    GapBucket { name: "过快(>160Hz)", lo_us: 0, hi_us: 6_250 },
    GapBucket { name: "略快(80~160Hz)", lo_us: 6_250, hi_us: 11_250 },
    GapBucket { name: "达标带(80Hz±10%)", lo_us: 11_250, hi_us: 13_750 },
    GapBucket { name: "略慢(60~72Hz)", lo_us: 13_750, hi_us: 16_700 },
    GapBucket { name: "微卡带(45~60Hz)", lo_us: 16_700, hi_us: 22_250 },
    GapBucket { name: "卡顿带(<45Hz)", lo_us: 22_250, hi_us: u32::MAX },
];

/// 桶落格（确定性：命中首个 [lo, hi) 区间；末桶吞超容）。
pub const fn bucket_of(gap_us: u32) -> usize {
    let mut i = 0usize;
    // const 循环上界钉死桶数。
    while i < 6 {
        let b = GAP_BUCKETS[i];
        if gap_us >= b.lo_us && gap_us < b.hi_us {
            return i;
        }
        i += 1;
    }
    5
}

/// 直方图（六桶累计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GapHistogram {
    counts: [u32; 6],
}

impl GapHistogram {
    /// 空图。
    pub const fn new() -> GapHistogram {
        GapHistogram { counts: [0; 6] }
    }

    /// 累计一格。
    pub const fn bump(&mut self, gap_us: u32) {
        let b = bucket_of(gap_us);
        // const 内数组下标赋值。
        self.counts[b] += 1;
    }

    /// 桶计数。
    pub const fn count(&self, bucket: usize) -> u32 {
        if bucket >= 6 {
            return 0;
        }
        self.counts[bucket]
    }

    /// 总数。
    pub const fn total(&self) -> u32 {
        self.counts[0] + self.counts[1] + self.counts[2] + self.counts[3] + self.counts[4] + self.counts[5]
    }

    /// 达标带占比（万分比：达标带 / 总数）。
    pub const fn on_target_ppm(&self) -> u32 {
        let t = self.total();
        if t == 0 {
            return 0;
        }
        self.counts[2].saturating_mul(10_000) / t
    }

    /// 卡顿占比（万分比：微卡+卡顿带 / 总数）。
    pub const fn jankish_ppm(&self) -> u32 {
        let t = self.total();
        if t == 0 {
            return 0;
        }
        (self.counts[4] + self.counts[5]).saturating_mul(10_000) / t
    }
}

/// 分布形态五态闭集（判据一：16.7ms 附近分布形态的健康度量化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributionShape {
    /// 单峰达标（≥85% 落达标带——节奏稳）。
    PeakOnTarget,
    /// 偏快（达标带+偏快带 ≥ 90% 且微卡带 < 5%——渲染负荷轻）。
    LeaningFast,
    /// 偏慢（略慢+微卡带 ≥ 30%——负荷偏高）。
    LeaningSlow,
    /// 双峰（达标带与卡顿带同时高——场景切换抖动）。
    Bimodal,
    /// 离散（无主导桶——节奏紊乱）。
    Scattered,
}

impl DistributionShape {
    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            DistributionShape::PeakOnTarget => "单峰达标",
            DistributionShape::LeaningFast => "偏快",
            DistributionShape::LeaningSlow => "偏慢",
            DistributionShape::Bimodal => "双峰",
            DistributionShape::Scattered => "离散",
        }
    }
}

/// 形态分类（确定性：按占比万分比阈值逐层判）。
pub const fn classify_shape(h: &GapHistogram) -> DistributionShape {
    let t = h.total();
    if t == 0 {
        return DistributionShape::Scattered;
    }
    let on_target = h.count(2).saturating_mul(10_000) / t;
    let fast = (h.count(0) + h.count(1)).saturating_mul(10_000) / t;
    let slow = (h.count(3) + h.count(4)).saturating_mul(10_000) / t;
    let bad = h.count(5).saturating_mul(10_000) / t;
    if on_target >= 8_500 {
        return DistributionShape::PeakOnTarget;
    }
    if on_target + fast >= 9_000 && h.count(4).saturating_mul(10_000) / t < 500 {
        return DistributionShape::LeaningFast;
    }
    if slow >= 3_000 {
        return DistributionShape::LeaningSlow;
    }
    if on_target >= 3_000 && bad >= 2_000 {
        return DistributionShape::Bimodal;
    }
    DistributionShape::Scattered
}

// ---------------------------------------------------------------------------
// 四、抖动 stddev（判据二：整数口径——无浮点）
// ---------------------------------------------------------------------------

/// 均值（µs，向下取整——整数纪律）。
pub const fn mean_us(samples: &[u32]) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    let mut sum = 0u64;
    let mut i = 0usize;
    while i < samples.len() {
        let v = samples[i] as u64;
        sum += v;
        i += 1;
    }
    (sum / samples.len() as u64) as u32
}

/// 总方差（µs²，总体方差 Σ(x-µ)²/n；整数）。
pub const fn variance_us2(samples: &[u32]) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    let mu = mean_us(samples) as u64;
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < samples.len() {
        let v = samples[i] as u64;
        let d = if v > mu { v - mu } else { mu - v };
        acc += d * d;
        i += 1;
    }
    (acc / samples.len() as u64) as u32
}

/// 整数平方根（牛顿迭代；无浮点 std 的替代面）。
pub const fn isqrt(x: u32) -> u32 {
    if x < 2 {
        return x;
    }
    // 牛顿迭代：r = (r + x/r) / 2；上界 x。
    let mut r = x;
    let mut prev = 0u32;
    // 收敛循环（const 可迭代；上界 32 轮必收敛）。
    let mut round = 0u32;
    while round < 32 {
        prev = r;
        r = (r + x / r) >> 1;
        if r >= prev {
            // 不再下降——取 prev（向下取整语义）。
            r = prev;
            break;
        }
        round += 1;
    }
    // 修正到 r² ≤ x < (r+1)²。
    let mut rr = r;
    while rr > 1 && rr.saturating_mul(rr) > x {
        rr -= 1;
    }
    while (rr + 1).saturating_mul(rr + 1) <= x {
        rr += 1;
    }
    rr
}

/// 抖动 stddev（µs；整数口径 σ = ⌊√var⌋——判据二）。
pub const fn jitter_stddev_us(samples: &[u32]) -> u32 {
    isqrt(variance_us2(samples))
}

/// 抖动等级（σ 相对目标的万分比 → 四级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitterGrade {
    /// 极稳（σ ≤ 目标 3%）。
    Rock,
    /// 稳（σ ≤ 目标 8%）。
    Steady,
    /// 抖（σ ≤ 目标 20%）。
    Shaky,
    /// 乱（σ > 目标 20%——节奏病）。
    Erratic,
}

impl JitterGrade {
    /// 评级。
    pub const fn grade(sigma_us: u32, target_us: u32) -> JitterGrade {
        if target_us == 0 {
            return JitterGrade::Erratic;
        }
        let ppm = (sigma_us as u64).saturating_mul(10_000) as u32 / target_us;
        if ppm <= 300 {
            JitterGrade::Rock
        } else if ppm <= 800 {
            JitterGrade::Steady
        } else if ppm <= 2_000 {
            JitterGrade::Shaky
        } else {
            JitterGrade::Erratic
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            JitterGrade::Rock => "极稳",
            JitterGrade::Steady => "稳",
            JitterGrade::Shaky => "抖",
            JitterGrade::Erratic => "乱",
        }
    }
}

// ---------------------------------------------------------------------------
// 五、jank 量化分级（判据三：>1.5× 目标 = 微卡；三级阈值）
// ---------------------------------------------------------------------------

/// jank 级别闭集（锚点：轻微/明显/事故）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JankLevel {
    /// 无卡（≤ 1.5× 目标）。
    None,
    /// 轻微（> 1.5× 且 ≤ 2.0×）。
    Mild,
    /// 明显（> 2.0× 且 ≤ 3.0×）。
    Marked,
    /// 事故（> 3.0×——合同级违约）。
    Severe,
}

impl JankLevel {
    /// 全量闭集。
    pub const ALL: [JankLevel; 4] = [JankLevel::None, JankLevel::Mild, JankLevel::Marked, JankLevel::Severe];

    /// 评级（锚点定义 >1.5× = 微卡的显式落地）。
    pub const fn grade(gap_us: u32, target_us: u32) -> JankLevel {
        if target_us == 0 {
            return JankLevel::Severe; // 无锚即不可评——保守归事故，逼显性设锚。
        }
        // gap×10000 与 target×15000 比较——整数口径避免浮点。
        if (gap_us as u64) * 10_000 <= (target_us as u64) * 15_000 {
            JankLevel::None
        } else if (gap_us as u64) * 10_000 <= (target_us as u64) * 20_000 {
            JankLevel::Mild
        } else if (gap_us as u64) * 10_000 <= (target_us as u64) * 30_000 {
            JankLevel::Marked
        } else {
            JankLevel::Severe
        }
    }

    /// 稳定序。
    pub const fn ordinal(self) -> usize {
        match self {
            JankLevel::None => 0,
            JankLevel::Mild => 1,
            JankLevel::Marked => 2,
            JankLevel::Severe => 3,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            JankLevel::None => "无卡",
            JankLevel::Mild => "轻微",
            JankLevel::Marked => "明显",
            JankLevel::Severe => "事故",
        }
    }

    /// 人读依据（可追溯面）。
    pub const fn basis(self) -> &'static str {
        match self {
            JankLevel::None => "≤1.5× 目标间隔",
            JankLevel::Mild => ">1.5× 且 ≤2.0× 目标",
            JankLevel::Marked => ">2.0× 且 ≤3.0× 目标",
            JankLevel::Severe => ">3.0× 目标——合同级违约",
        }
    }
}

// ---------------------------------------------------------------------------
// 六、jank 分级账（分级留痕 + 聚合）
// ---------------------------------------------------------------------------

/// jank 账（四桶分级累计 + 事故场景清单——封顶 32 条滚动）。
#[derive(Clone, Debug, Default)]
pub struct JankLedger {
    counts: [u32; 4],
    severe_scenes: Vec<&'static str>,
}

impl JankLedger {
    /// 空账。
    pub const fn new() -> JankLedger {
        JankLedger { counts: [0; 4], severe_scenes: Vec::new() }
    }

    /// 记一帧（评级 + 入账；事故记场景）。
    pub fn record(&mut self, gap_us: u32, target_us: u32, scene: &'static str) -> JankLevel {
        let lv = JankLevel::grade(gap_us, target_us);
        self.counts[lv.ordinal()] += 1;
        if lv == JankLevel::Severe && self.severe_scenes.len() < 32 {
            let mut dup = false;
            let mut i = 0usize;
            while i < self.severe_scenes.len() {
                if let Some(s) = self.severe_scenes.get(i) {
                    if *s == scene {
                        dup = true;
                        break;
                    }
                }
                i += 1;
            }
            if !dup {
                self.severe_scenes.push(scene);
            }
        }
        lv
    }

    /// 分级计数。
    pub const fn count(&self, lv: JankLevel) -> u32 {
        self.counts[lv.ordinal()]
    }

    /// 总帧数。
    pub const fn total(&self) -> u32 {
        self.counts[0] + self.counts[1] + self.counts[2] + self.counts[3]
    }

    /// 事故率（万分比）。
    pub const fn severe_ppm(&self) -> u32 {
        let t = self.total();
        if t == 0 {
            return 0;
        }
        self.counts[3].saturating_mul(10_000) / t
    }

    /// 事故场景清单（只读）。
    pub fn severe_scenes(&self) -> &[&'static str] {
        &self.severe_scenes
    }
}

// ---------------------------------------------------------------------------
// 七、长期趋势（判据四：周桶 + 整数斜率 + 趋势五态）
// ---------------------------------------------------------------------------

/// 周桶数（4 周滚动——月内周趋势）。
pub const TREND_WEEKS: usize = 4;

/// 周桶（一周的聚合：帧数 + Σ间隔 + jank 数）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeekBucket {
    /// 帧数。
    pub frames: u32,
    /// Σ间隔（µs——均值分子）。
    pub gap_sum: u64,
    /// jank 帧数（Mild 及以上）。
    pub janks: u32,
}

impl WeekBucket {
    /// 周均值（µs）。
    pub const fn mean_us(&self) -> u32 {
        if self.frames == 0 {
            return 0;
        }
        (self.gap_sum / self.frames as u64) as u32
    }

    /// 周 jank 率（万分比）。
    pub const fn jank_ppm(&self) -> u32 {
        if self.frames == 0 {
            return 0;
        }
        self.janks.saturating_mul(10_000) / self.frames
    }
}

/// 趋势账（4 周滚动；周序单调推进）。
#[derive(Clone, Debug, Default)]
pub struct TrendLedger {
    weeks: [WeekBucket; 4],
    /// 当前周序（0..∞——取模落桶）。
    week_cursor: u64,
}

impl TrendLedger {
    /// 空账。
    pub const fn new() -> TrendLedger {
        TrendLedger { weeks: [WeekBucket { frames: 0, gap_sum: 0, janks: 0 }; 4], week_cursor: 0 }
    }

    /// 落一格（周内帧聚合——调用方按周批量喂）。
    pub fn feed_week(&mut self, frames: u32, gap_sum_us: u64, janks: u32) {
        let idx = (self.week_cursor % TREND_WEEKS as u64) as usize;
        if let Some(w) = self.weeks.get_mut(idx) {
            *w = WeekBucket { frames, gap_sum: gap_sum_us, janks };
        }
        self.week_cursor += 1;
    }

    /// 周桶查。
    pub fn week(&self, idx: usize) -> Option<&WeekBucket> {
        self.weeks.get(idx)
    }

    /// 周均值序列（旧→新；零帧周以 0 参与但标记）。
    pub fn means(&self) -> [u32; 4] {
        [
            self.weeks[0].mean_us(),
            self.weeks[1].mean_us(),
            self.weeks[2].mean_us(),
            self.weeks[3].mean_us(),
        ]
    }

    /// 趋势斜率（整数口径：末周均值 − 首周均值，µs/3周；正 = 变慢）。
    pub const fn slope_us(&self) -> i64 {
        let first = self.weeks[0].mean_us() as i64;
        let last = self.weeks[3].mean_us() as i64;
        last - first
    }
}

/// 趋势五态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrendShape {
    /// 变快（斜率 ≤ -500µs）。
    Speeding,
    /// 稳中略快（-500 < 斜率 ≤ -100）。
    SlightlyFaster,
    /// 平（|斜率| < 100）。
    Flat,
    /// 稳中略慢（100 ≤ 斜率 < 500）。
    SlightlySlower,
    /// 变慢（斜率 ≥ 500µs——预警线）。
    Slowing,
}

impl TrendShape {
    /// 评级。
    pub const fn grade(slope_us: i64) -> TrendShape {
        if slope_us <= -500 {
            TrendShape::Speeding
        } else if slope_us <= -100 {
            TrendShape::SlightlyFaster
        } else if slope_us < 100 {
            TrendShape::Flat
        } else if slope_us < 500 {
            TrendShape::SlightlySlower
        } else {
            TrendShape::Slowing
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            TrendShape::Speeding => "变快",
            TrendShape::SlightlyFaster => "稳中略快",
            TrendShape::Flat => "平稳",
            TrendShape::SlightlySlower => "稳中略慢",
            TrendShape::Slowing => "变慢",
        }
    }

    /// 预警面（Slowing → 预警语——提前干预而非事后报告）。
    pub const fn alert(self) -> Option<&'static str> {
        match self {
            TrendShape::Slowing => Some("周趋势变慢 ≥500µs：建议核对累积膨胀源（缓存/句柄/账户）"),
            TrendShape::SlightlySlower => Some("周趋势略慢：观察下一周，若持续则预警"),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 八、节奏健康度评分（三因子合成：直方图 × 抖动 × jank）
// ---------------------------------------------------------------------------

/// 健康度评分（0~10000 万分制；整数合成——无浮点）。
///
/// 权重（锚点健康度三因子）：达标带占比 40% + 抖动稳定 30% + 无卡率 30%。
pub const fn health_score(h: &GapHistogram, sigma_us: u32, target_us: u32, jank: &JankLedger) -> u32 {
    let total = h.total();
    if total == 0 {
        return 0;
    }
    // 因子一：达标带占比（万分比直接用）。
    let f1 = h.on_target_ppm();
    // 因子二：抖动稳定度（1 - σ/目标 的万分比；σ 超目标记 0）。
    let f2 = if target_us == 0 {
        0
    } else {
        let sigma_ppm = (sigma_us as u64).saturating_mul(10_000) as u32 / target_us;
        if sigma_ppm >= 10_000 {
            0
        } else {
            10_000 - sigma_ppm
        }
    };
    // 因子三：无卡率。
    let t = jank.total();
    let f3 = if t == 0 {
        10_000
    } else {
        let none_ppm = jank.count(JankLevel::None).saturating_mul(10_000) / t;
        none_ppm
    };
    // 加权合成：40% + 30% + 30%。
    let score = f1 * 4 + f2 * 3 + f3 * 3;
    score / 10
}

/// 健康度等级五态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthGrade {
    /// 优（≥ 9000）。
    Excellent,
    /// 良（≥ 7500）。
    Good,
    /// 中（≥ 6000）。
    Fair,
    /// 差（≥ 4000）。
    Poor,
    /// 危（< 4000——建议干预）。
    Critical,
}

impl HealthGrade {
    /// 评级。
    pub const fn grade(score: u32) -> HealthGrade {
        if score >= 9_000 {
            HealthGrade::Excellent
        } else if score >= 7_500 {
            HealthGrade::Good
        } else if score >= 6_000 {
            HealthGrade::Fair
        } else if score >= 4_000 {
            HealthGrade::Poor
        } else {
            HealthGrade::Critical
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            HealthGrade::Excellent => "优",
            HealthGrade::Good => "良",
            HealthGrade::Fair => "中",
            HealthGrade::Poor => "差",
            HealthGrade::Critical => "危",
        }
    }
}

// ---------------------------------------------------------------------------
// 九、审计账本（判据五之一：256 帧滚动 + 三态查询）
// ---------------------------------------------------------------------------

/// 单帧统计留痕（B08 五元组）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapFrameRecord {
    /// 场景标签。
    pub scene: String,
    /// 窗内样本数。
    pub samples: u32,
    /// 抖动 σ（µs）。
    pub sigma_us: u32,
    /// jank 级（0..3）。
    pub jank_level: u8,
    /// 健康分（万分制）。
    pub health: u32,
    /// 码面（0 = 无错）。
    pub code: u16,
}

/// 统计账本（256 帧滚动）。
#[derive(Clone, Debug, Default)]
pub struct GapLedger {
    records: Vec<GapFrameRecord>,
}

impl GapLedger {
    /// 空账。
    pub fn new() -> Self {
        GapLedger { records: Vec::new() }
    }

    /// 落账（封顶滚动）。
    pub fn record(&mut self, rec: GapFrameRecord) {
        if self.records.len() >= 256 {
            self.records.remove(0);
        }
        self.records.push(rec);
    }

    /// 精确查。
    pub fn exact(&self, scene: &str) -> Option<GapFrameRecord> {
        let mut i = self.records.len();
        while i > 0 {
            i -= 1;
            if let Some(r) = self.records.get(i) {
                if r.scene == scene {
                    return Some(r.clone());
                }
            }
        }
        None
    }

    /// 聚合查。
    pub fn aggregate(&self, prefix: &str) -> Option<(u32, u32, u32, u32)> {
        let mut frames = 0u32;
        let mut samples = 0u32;
        let mut janks = 0u32;
        let mut peak_health = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.scene.starts_with(prefix) {
                    frames += 1;
                    samples += r.samples;
                    if r.jank_level > 0 {
                        janks += 1;
                    }
                    if r.health > peak_health {
                        peak_health = r.health;
                    }
                }
            }
            i += 1;
        }
        if frames == 0 {
            None
        } else {
            Some((frames, samples, janks, peak_health))
        }
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// 三态查询。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GapQuery {
    /// 精确命中。
    Exact(GapFrameRecord),
    /// 聚合命中。
    Aggregated(u32, u32, u32, u32),
    /// 未命中。
    NotFound,
}

/// 统一查账入口。
pub fn query_gap_ledger(ledger: &GapLedger, scene: &str) -> GapQuery {
    if let Some(r) = ledger.exact(scene) {
        return GapQuery::Exact(r);
    }
    if let Some((f, s, j, h)) = ledger.aggregate(scene) {
        return GapQuery::Aggregated(f, s, j, h);
    }
    GapQuery::NotFound
}

// ---------------------------------------------------------------------------
// 十、遥测导出（B08 五元组）
// ---------------------------------------------------------------------------

/// 指标前缀。
pub const GAP_METRIC_PREFIX: &str = "d04.framegap.";

/// 指标 id 闭集（七条）。
pub const GAP_METRIC_IDS: [&str; 7] = [
    "d04.framegap.mean_us",
    "d04.framegap.sigma_us",
    "d04.framegap.on_target_ppm",
    "d04.framegap.jankish_ppm",
    "d04.framegap.severe_ppm",
    "d04.framegap.health",
    "d04.framegap.trend_slope_us",
];

/// 统计引擎（窗 + 直方图 + jank 账 + 趋势账的一体化持有）。
#[derive(Clone, Debug)]
pub struct GapEngine {
    /// 目标间隔（µs——分级/健康度锚；0 = 未设）。
    pub target_us: u32,
    /// 滚动窗。
    pub window: GapWindow,
    /// 直方图。
    pub hist: GapHistogram,
    /// jank 账。
    pub jank: JankLedger,
    /// 趋势账。
    pub trend: TrendLedger,
}

impl GapEngine {
    /// 构造（目标锚必填——0 视为未设显性拒）。
    pub fn new(target_us: u32) -> Result<GapEngine, FgCode> {
        if target_us == 0 {
            return Err(FgCode::TargetMissing);
        }
        Ok(GapEngine {
            target_us,
            window: GapWindow::new(),
            hist: GapHistogram::new(),
            jank: JankLedger::new(),
            trend: TrendLedger::new(),
        })
    }

    /// 喂一帧（入口守卫：非法拒、异常标记入账）。
    pub fn feed(&mut self, s: GapSample) -> Result<JankLevel, FgCode> {
        if s.outlier() {
            // 异常帧也入窗（如实入账纪律）——但标记码返回。
            self.window.push(s)?;
            self.hist.bump(s.gap_us);
            let lv = self.jank.record(s.gap_us, self.target_us, s.scene);
            return Err(FgCode::SampleOutlier);
        }
        self.window.push(s)?;
        self.hist.bump(s.gap_us);
        Ok(self.jank.record(s.gap_us, self.target_us, s.scene))
    }

    /// 窗内间隔序列（只读快照）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let snap = self.window.snapshot();
        let mut i = 0usize;
        while i < snap.len() {
            if let Some(s) = snap.get(i) {
                out.push(s.gap_us);
            }
            i += 1;
        }
        out
    }

    /// 当前统计快照（空窗显性拒）。
    pub fn stats(&self) -> Result<(u32, u32, GapHistogram, JitterGrade, u32), FgCode> {
        let gaps = self.gaps();
        if gaps.is_empty() {
            return Err(FgCode::WindowEmpty);
        }
        let mu = mean_us(&gaps);
        let sigma = jitter_stddev_us(&gaps);
        let jg = JitterGrade::grade(sigma, self.target_us);
        let health = health_score(&self.hist, sigma, self.target_us, &self.jank);
        Ok((mu, sigma, self.hist.clone(), jg, health))
    }
}

// ---------------------------------------------------------------------------
// 十一、对外接口冻结 v1 + 仿真走查（判据五）
// ---------------------------------------------------------------------------

/// 统计请求（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapStatsRequestV1 {
    /// 协议版本（1）。
    pub version: u32,
    /// 场景标签。
    pub scene: String,
    /// 目标间隔（µs）。
    pub target_us: u32,
    /// 样本批。
    pub gaps_us: Vec<u32>,
}

/// 统计答复（冻结 v1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapStatsReplyV1 {
    /// 接受标记。
    pub accepted: bool,
    /// 均值（µs）。
    pub mean_us: u32,
    /// 抖动 σ（µs）。
    pub sigma_us: u32,
    /// 抖动级。
    pub jitter: &'static str,
    /// 形态。
    pub shape: &'static str,
    /// 健康分。
    pub health: u32,
    /// 健康级。
    pub health_grade: &'static str,
    /// 码面。
    pub code: u16,
    /// 明细。
    pub detail: String,
}

/// 一站式统计（判据一~四串行 + 落账）。
pub fn run_gap_stats(req: &GapStatsRequestV1, ledger: &mut GapLedger) -> GapStatsReplyV1 {
    let mut eng = match GapEngine::new(req.target_us) {
        Ok(e) => e,
        Err(code) => {
            return GapStatsReplyV1 {
                accepted: false,
                mean_us: 0,
                sigma_us: 0,
                jitter: "",
                shape: "",
                health: 0,
                health_grade: "",
                code: code as u16,
                detail: code.detail().to_string(),
            };
        }
    };
    let mut i = 0u64;
    let mut i = 0usize;
    while i < req.gaps_us.len() {
        let g = match req.gaps_us.get(i) {
            Some(x) => *x,
            None => break,
        };
        let _ = eng.feed(GapSample { frame_no: i as u64 + 1, gap_us: g, scene: "batch" });
        i += 1;
    }
    let _ = i;
    match eng.stats() {
        Ok((mu, sigma, hist, jg, health)) => {
            let shape = classify_shape(&hist);
            let hg = HealthGrade::grade(health);
            let jank_lv = JankLevel::grade(mu, req.target_us);
            ledger.record(GapFrameRecord {
                scene: req.scene.clone(),
                samples: eng.window.len() as u32,
                sigma_us: sigma,
                jank_level: jank_lv.ordinal() as u8,
                health,
                code: 0,
            });
            GapStatsReplyV1 {
                accepted: true,
                mean_us: mu,
                sigma_us: sigma,
                jitter: jg.name(),
                shape: shape.name(),
                health,
                health_grade: hg.name(),
                code: 0,
                detail: "统计完成：直方图+抖动+分级+健康度四面对齐".to_string(),
            }
        }
        Err(code) => GapStatsReplyV1 {
            accepted: false,
            mean_us: 0,
            sigma_us: 0,
            jitter: "",
            shape: "",
            health: 0,
            health_grade: "",
            code: code as u16,
            detail: code.detail().to_string(),
        },
    }
}

/// 仿真走查（判据五机检面：标准 80Hz 序列 → 五步对拍）。
pub fn walk_gap_stats() -> Vec<(&'static str, bool)> {
    let mut steps: Vec<(&'static str, bool)> = Vec::new();
    // 步 1：恒定 12500µs 序列 → σ=0 → 极稳。
    let mut eng = match GapEngine::new(12_500) {
        Ok(e) => e,
        Err(_) => {
            steps.push(("目标锚建立", false));
            return steps;
        }
    };
    steps.push(("目标锚建立", true));
    let mut f = 1u64;
    let mut n = 0u32;
    while n < 32 {
        let _ = eng.feed(GapSample { frame_no: f, gap_us: 12_500, scene: "walk" });
        f += 1;
        n += 1;
    }
    let stats = eng.stats();
    let ok1 = match stats {
        Ok((_, sigma, _, jg, _)) => sigma == 0 && jg == JitterGrade::Rock,
        Err(_) => false,
    };
    steps.push(("恒定序列极稳", ok1));
    // 步 2：分桶正确性（12500 落达标带）。
    steps.push(("分桶正确", bucket_of(12_500) == 2 && bucket_of(20_000) == 4));
    // 步 3：分级正确（1.5× 阈值边界：18750 → None；18751 → Mild）。
    steps.push((
        "分级阈值恰边界",
        JankLevel::grade(18_750, 12_500) == JankLevel::None
            && JankLevel::grade(18_751, 12_500) == JankLevel::Mild,
    ));
    // 步 4：健康度满分（恒定 + 全无卡）。
    let ok4 = match stats {
        Ok((_, _, _, _, health)) => health == 10_000,
        Err(_) => false,
    };
    steps.push(("健康度满分", ok4));
    // 步 5：趋势评级（构造变慢序列）。
    let mut tl = TrendLedger::new();
    tl.feed_week(1000, 12_000 * 1000, 10);
    tl.feed_week(1000, 12_200 * 1000, 12);
    tl.feed_week(1000, 12_500 * 1000, 15);
    tl.feed_week(1000, 13_000 * 1000, 20);
    steps.push(("趋势评级", TrendShape::grade(tl.slope_us()) == TrendShape::Slowing));
    steps
}

/// 桶表/闭集守恒断言。
const _: () = {
    assert!(GAP_BUCKETS.len() == 6, "间隔桶必须六桶闭集");
    assert!(JankLevel::ALL.len() == 4, "jank 级必须四态闭集");
    assert!(TREND_WEEKS == 4, "周趋势必须四桶");
};

/// 复用对拍锚点。
pub const REUSE_ANCHORS: [&str; 4] = [
    "F0483 帧账四段耗时与帧间隔——本模型消费其间隔序列",
    "F2408 万分比整数口径——方差/斜率全部整数化",
    "F0485 帧率合同三条款——分级阈值与合同对齐",
    "C02 帧预算 12500µs 口径——目标间隔锚点一致",
];

// ---------------------------------------------------------------------------
// 十二、分位数（整数口径 P50/P95/P99——与 F0485 合同三条款对齐）
// ---------------------------------------------------------------------------

/// 分位点闭集（与 F0485 合同三条款对齐：中位/P95/P99）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantile {
    /// 中位（P50）。
    P50,
    /// P95。
    P95,
    /// P99。
    P99,
}

impl Quantile {
    /// 万分位点。
    pub const fn ppm(self) -> u32 {
        match self {
            Quantile::P50 => 5_000,
            Quantile::P95 => 9_500,
            Quantile::P99 => 9_900,
        }
    }
}

/// 升序拷贝（分位数前提——排序面）。
pub fn sorted_copy(samples: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < samples.len() {
        if let Some(x) = samples.get(i) {
            out.push(*x);
        }
        i += 1;
    }
    out.sort_by(|a, b| a.cmp(b));
    out
}

/// 分位数取值（最近秩整数口径：idx = ⌈ppm×n/10000⌉ − 1；越界钳位）。
///
/// 空表 → None（显性拒不猜）——判据面的空窗语义与 [`FgCode::WindowEmpty`] 一致。
pub fn quantile_us(sorted: &[u32], q: Quantile) -> Option<u32> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    // idx = ⌈ppm×n/10000⌉ − 1，等价 (ppm×n + 9999)/10000 − 1。
    let ppm = q.ppm() as u64;
    let n64 = n as u64;
    let mut idx = ((ppm * n64 + 9_999) / 10_000) as usize;
    if idx == 0 {
        idx = 1;
    }
    if idx > n {
        idx = n;
    }
    let v = sorted.get(idx - 1)?;
    Some(*v)
}

/// 合同三条款快照（P95/P99/事故率——F0485 消费面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractSnapshot {
    /// P95（µs）。
    pub p95_us: u32,
    /// P99（µs）。
    pub p99_us: u32,
    /// 事故率（万分比）。
    pub severe_ppm: u32,
}

/// 三条款快照导出。
pub fn contract_snapshot(gaps: &[u32], jank: &JankLedger) -> Option<ContractSnapshot> {
    let sorted = sorted_copy(gaps);
    let p95 = quantile_us(&sorted, Quantile::P95)?;
    let p99 = quantile_us(&sorted, Quantile::P99)?;
    Some(ContractSnapshot { p95_us: p95, p99_us: p99, severe_ppm: jank.severe_ppm() })
}

// ---------------------------------------------------------------------------
// 十三、场景归因对比（双场景直方图差异面——归因前提）
// ---------------------------------------------------------------------------

/// 场景对比结论（一资源一行的差异面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneCompare {
    /// 场景 A 标签。
    pub scene_a: String,
    /// 场景 B 标签。
    pub scene_b: String,
    /// A 均值（µs）。
    pub mean_a: u32,
    /// B 均值（µs）。
    pub mean_b: u32,
    /// 均值差（B−A；i64 允许负——A 更快为负）。
    pub mean_delta_i: i64,
    /// 达标带占比差（B−A，万分比）。
    pub on_target_delta_ppm: i64,
    /// 归因结论。
    pub verdict: &'static str,
}

/// 双场景对比（两窗快照 → 差异面 + 归因一句话）。
pub fn compare_scenes(gaps_a: &[u32], gaps_b: &[u32], name_a: &str, name_b: &str) -> Option<SceneCompare> {
    if gaps_a.is_empty() || gaps_b.is_empty() {
        return None;
    }
    // A 直方图。
    let mut ha = GapHistogram::new();
    let mut i = 0usize;
    while i < gaps_a.len() {
        if let Some(x) = gaps_a.get(i) {
            ha.bump(*x);
        }
        i += 1;
    }
    let mut hb = GapHistogram::new();
    let mut i = 0usize;
    while i < gaps_b.len() {
        if let Some(x) = gaps_b.get(i) {
            hb.bump(*x);
        }
        i += 1;
    }
    let ma = mean_us(gaps_a);
    let mb = mean_us(gaps_b);
    let delta = (mb as i64) - (ma as i64);
    let dt = (hb.on_target_ppm() as i64) - (ha.on_target_ppm() as i64);
    // 归因（确定性阈值判）。
    let verdict = if delta <= -500 && dt >= 500 {
        "B 显著更快：建议核对 B 场景的预算配比是否可复制到 A"
    } else if delta >= 500 && dt <= -500 {
        "B 显著更慢：建议对 B 场景走 C 域预算修正（F0485 联动）"
    } else if delta.abs() < 100 {
        "两场景节奏等价：无归因动作"
    } else {
        "差异在噪声带：建议加大样本窗后复比对"
    };
    Some(SceneCompare {
        scene_a: name_a.to_string(),
        scene_b: name_b.to_string(),
        mean_a: ma,
        mean_b: mb,
        mean_delta_i: delta,
        on_target_delta_ppm: dt,
        verdict,
    })
}

/// 归因三态闭集（差异是否显著的机器判据——与 compare_scenes 的人读结论对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attribution {
    /// B 更快（显著）。
    BFaster,
    /// 等价（噪声带内）。
    Equivalent,
    /// B 更慢（显著）。
    BSlower,
}

impl Attribution {
    /// 机判（确定性阈值）。
    pub const fn judge(mean_delta_i: i64) -> Attribution {
        if mean_delta_i <= -500 {
            Attribution::BFaster
        } else if mean_delta_i >= 500 {
            Attribution::BSlower
        } else {
            Attribution::Equivalent
        }
    }
}

/// 尾守恒断言。
const _: () = {
    assert!(Quantile::P50.ppm() == 5_000, "P50 位点必须五千");
};
