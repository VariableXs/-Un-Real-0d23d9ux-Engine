//! CGPU-F0484 · 帧间隔统计（CGPU-D 域 · 帧预算与计时域 · D02 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0484`
//!
//! **判据（锚点原文）**：直方图、stddev、分级、趋势、判据。
//!
//! **职责定位（锚点原文）**：间隔统计：间隔分布直方图（16.7ms 附近的分布
//! 形态——节奏健康度量化）；抖动 stddev（滚动窗口抖动度量）；jank 量化
//! 定义（>1.5× 目标间隔=微卡——阈值分级：轻微/明显/事故）；长期趋势
//! （间隔健康的周趋势）。
//!
//! ## 一、间隔直方图（判据一）
//!
//! 直方图以目标间隔（16.7ms=16670μs @60Hz）为锚的分桶：桶宽 = 目标间隔
//! 的 1/8（2083.75μs→取整 2084μs，常量写死并注明来源），共 16 桶覆盖
//! 0~2× 目标间隔，尾桶收纳所有超 2× 的离群间隔（离群不丢——丢离群的
//! 直方图会把事故画成健康）。落桶是纯整数运算（interval/BUCKET_WIDTH
//! 饱和到尾桶），零浮点。
//!
//! ## 二、抖动 stddev（判据二）
//!
//! 滚动窗口（[`JITTER_WINDOW`] 帧）内的间隔标准差：用**平方和/平方差
//! 恒等式**在 u64 域做整数近似（Σx²、Σx、n 三量滚动维护，无浮点）；
//! 输出 μs² 量纲的方差与 μs 量纲的整数平方根近似（牛顿法整数根——
//! 确定性、无 libm 依赖）。窗口容量满后新样本挤掉最老样本（环形账）。
//!
//! ## 三、jank 量化分级（判据三）
//!
//! 阈值分级三档（写死常量，来源=锚点「>1.5×目标=微卡」+ 两级外推）：
//! 轻微（>1.5× 且 ≤2.0×）、明显（>2.0× 且 ≤3.0×）、事故（>3.0×——
//! 用户可感知的冻结）。每档独立计数，分档判定 O(1)。
//!
//! ## 四、长期趋势（判据四）
//!
//! 周趋势 = 按日聚合的健康比（达标间隔数/总间隔数）的**首尾对比**：
//! 窗口内最早一日与最近一日的健康比差值 → 改善/持平/退化三态（阈值
//! ±1%，带宽内持平防抖动误报）。趋势是账面推导不是预测——不做外推。
//!
//! **对接**：F0482（全帧精确计时器——间隔的上游采样源）；F0483（帧
//! 账本——逐帧账的存储面，本模块消费其间隔流）；F2402（遥测统一模型
//! ——本模块统计量可投影为 Metric）。零 panic 面、零 IO、零墙钟、
//! no_std、零浮点。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FRAME_INTERVAL_VERSION: &str = "D02-frameinterval-v1";

/// 目标间隔（μs；60Hz=16667μs 取整——锚点「16.7ms 附近」的精确化）。
pub const TARGET_INTERVAL_US: u64 = 16_667;

/// 直方图桶宽（μs；= TARGET_INTERVAL_US/8 ≈ 2083，写死并注明来源：
/// 目标间隔的 1/8——16 桶覆盖 0~2× 目标）。
pub const BUCKET_WIDTH_US: u64 = 2_083;

/// 直方图桶数（15 桶 + 1 尾桶收纳全部超 2× 离群）。
pub const BUCKET_TOTAL: usize = 16;

/// 抖动滚动窗口（帧数；2 秒 @60Hz≈120——窗口要短于趋势、长于单帧）。
pub const JITTER_WINDOW: usize = 120;

/// jank 分级阈值：轻微下界（μs；=1.5×目标，锚点原文「>1.5×=微卡」）。
pub const JANK_MINOR_US: u64 = 25_000;

/// jank 分级阈值：明显下界（μs；=2.0×目标）。
pub const JANK_MAJOR_US: u64 = 33_334;

/// jank 分级阈值：事故下界（μs；=3.0×目标——用户可感知冻结）。
pub const JANK_SEVERE_US: u64 = 50_001;

/// 趋势持平带宽（健康比差 ±0.01=±1%——带宽内防抖动误报）。
pub const TREND_FLAT_BAND_PPM: i64 = 10_000;

// ---------------------------------------------------------------------------
// 二、诊断码（0x31 段独占：D02 间隔统计面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x31 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 零间隔（同刻双帧——计时器未闭段或采样错位）。
    ZeroInterval,
    /// 窗口为空时请求统计量。
    WindowEmpty,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::ZeroInterval => "零间隔：同刻双帧说明计时器未闭段或采样错位，拒绝入账并检查 F0482 上游",
            XcCode::WindowEmpty => "窗口为空：先喂样本再取统计量，空窗 stddev/趋势无意义",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、jank 分级（判据三）
// ---------------------------------------------------------------------------

/// jank 分级三态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JankGrade {
    /// 达标（≤1.5× 目标）。
    Ok,
    /// 轻微（>1.5× 且 ≤2.0×）。
    Minor,
    /// 明显（>2.0× 且 ≤3.0×）。
    Major,
    /// 事故（>3.0×）。
    Severe,
}

impl JankGrade {
    /// 分级判定（O(1)；阈值写死见常量区——锚点「>1.5×=微卡」+外推）。
    pub const fn classify(interval_us: u64) -> JankGrade {
        if interval_us <= JANK_MINOR_US {
            JankGrade::Ok
        } else if interval_us <= JANK_MAJOR_US {
            JankGrade::Minor
        } else if interval_us <= JANK_SEVERE_US {
            JankGrade::Major
        } else {
            JankGrade::Severe
        }
    }

    /// 档名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            JankGrade::Ok => "达标",
            JankGrade::Minor => "轻微",
            JankGrade::Major => "明显",
            JankGrade::Severe => "事故",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、整数平方根（判据二的确定性根——无 libm）
// ---------------------------------------------------------------------------

/// 牛顿法整数平方根（u64 域；floor(sqrt(n))；确定性零浮点）。
pub const fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    // 初值：2^ceil(bits/2) 量级；牛顿迭代收敛到 floor 根。
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

// ---------------------------------------------------------------------------
// 五、间隔统计器（判据一、二、三、四的承载）
// ---------------------------------------------------------------------------

/// 帧间隔统计器（滚动窗口 + 直方图 + 分级账 + 日聚合账）。
#[derive(Clone, Debug)]
pub struct IntervalStats {
    /// 直方图（16 桶；尾桶含离群）。
    hist: [u64; BUCKET_TOTAL],
    /// 滚动窗口（环形：样本按序入，满后挤最老）。
    window: [u64; JITTER_WINDOW],
    /// 窗口头（下一写入位）。
    head: usize,
    /// 已入窗样本数（≤JITTER_WINDOW，饱和后恒满）。
    filled: usize,
    /// 窗口 Σx（μs）。
    sum_x: u64,
    /// 窗口 Σx²（μs²，u64 上限 2^64：样本 ≤50ms=5e4μs，x²≤2.5e9，
    /// 120 样本 Σx²≤3e11——远离溢出，checked 仍保留）。
    sum_x2: u128,
    /// 分级计数。
    grade_ok: u64,
    grade_minor: u64,
    grade_major: u64,
    grade_severe: u64,
    /// 日聚合（健康样本,总样本）按日滚动——趋势面。
    daily: Vec<(u64, u64)>,
    /// 当前日索引（喂料方推进——零墙钟：日界由上游日账注入）。
    cur_day: u64,
    /// 拒收的零间隔数。
    zero_rejects: u64,
}

impl IntervalStats {
    /// 新统计器。
    pub const fn new() -> IntervalStats {
        IntervalStats {
            hist: [0; BUCKET_TOTAL],
            window: [0; JITTER_WINDOW],
            head: 0,
            filled: 0,
            sum_x: 0,
            sum_x2: 0,
            grade_ok: 0,
            grade_minor: 0,
            grade_major: 0,
            grade_severe: 0,
            daily: Vec::new(),
            cur_day: 0,
            zero_rejects: 0,
        }
    }

    /// 喂入一个间隔样本（μs；零间隔显性拒——计时器未闭段不是数据）。
    pub fn feed(&mut self, interval_us: u64, day: u64) -> Result<(), XcCode> {
        if interval_us == 0 {
            self.zero_rejects = self.zero_rejects.saturating_add(1);
            return Err(XcCode::ZeroInterval);
        }
        // 直方图落桶（饱和到尾桶——离群不丢）。
        let mut bucket = (interval_us / BUCKET_WIDTH_US) as usize;
        if bucket >= BUCKET_TOTAL {
            bucket = BUCKET_TOTAL - 1;
        }
        self.hist[bucket] = self.hist[bucket].saturating_add(1);
        // 分级计数。
        match JankGrade::classify(interval_us) {
            JankGrade::Ok => self.grade_ok = self.grade_ok.saturating_add(1),
            JankGrade::Minor => self.grade_minor = self.grade_minor.saturating_add(1),
            JankGrade::Major => self.grade_major = self.grade_major.saturating_add(1),
            JankGrade::Severe => self.grade_severe = self.grade_severe.saturating_add(1),
        }
        // 滚动窗口（满后挤最老：减去被挤样本的 Σ 贡献）。
        if self.filled == JITTER_WINDOW {
            let old = self.window[self.head];
            self.sum_x = self.sum_x.saturating_sub(old);
            self.sum_x2 = self.sum_x2.saturating_sub((old as u128) * (old as u128));
        } else {
            self.filled = self.filled.saturating_add(1);
        }
        self.window[self.head] = interval_us;
        self.head = (self.head + 1) % JITTER_WINDOW;
        self.sum_x = self.sum_x.saturating_add(interval_us);
        self.sum_x2 = self
            .sum_x2
            .saturating_add((interval_us as u128) * (interval_us as u128));
        // 日聚合（日界推进时开新行；同日累加）。
        if day != self.cur_day || self.daily.is_empty() {
            self.cur_day = day;
            self.daily.push((if interval_us <= JANK_MINOR_US { 1 } else { 0 }, 1));
            // 日账封顶 400 日（约一年）；满后滚动挤最老（趋势看近不看远）。
            if self.daily.len() > 400 {
                self.daily.remove(0);
            }
        } else if let Some(last) = self.daily.last_mut() {
            last.1 = last.1.saturating_add(1);
            if interval_us <= JANK_MINOR_US {
                last.0 = last.0.saturating_add(1);
            }
        }
        Ok(())
    }

    /// 滚动窗口抖动方差（μs²；整数恒等式 var=Σx²/n - (Σx/n)²，
    /// u128 中间量防溢出；空窗显性拒）。
    pub fn jitter_var_us2(&self) -> Result<u64, XcCode> {
        if self.filled == 0 {
            return Err(XcCode::WindowEmpty);
        }
        let n = self.filled as u128;
        let sx = self.sum_x as u128;
        // var = Σx²/n − (Σx/n)²；先乘后除保精度：n·Σx² − (Σx)² 再除 n²。
        let numer = self
            .sum_x2
            .saturating_mul(n)
            .saturating_sub(sx.saturating_mul(sx));
        let var = numer / (n * n);
        if var > u64::MAX as u128 {
            return Ok(u64::MAX);
        }
        Ok(var as u64)
    }

    /// 滚动窗口抖动 stddev（μs；方差整数根）。
    pub fn jitter_stddev_us(&self) -> Result<u64, XcCode> {
        Ok(isqrt(self.jitter_var_us2()?))
    }

    /// 直方图只读（16 桶；判据/遥测消费）。
    pub const fn histogram(&self) -> &[u64; BUCKET_TOTAL] {
        &self.hist
    }

    /// jank 分级计数 (ok, minor, major, severe)。
    pub const fn grades(&self) -> (u64, u64, u64, u64) {
        (self.grade_ok, self.grade_minor, self.grade_major, self.grade_severe)
    }

    /// 长期趋势（判据四）：首日 vs 最近日健康比差 → 改善/持平/退化。
    /// 健康比用百万分比（ppm）整数口径：healthy*1_000_000/total。
    pub fn trend(&self) -> Result<Trend, XcCode> {
        let first = match self.daily.first() {
            Some(f) => f,
            None => return Err(XcCode::WindowEmpty),
        };
        let last = match self.daily.last() {
            Some(l) => l,
            None => return Err(XcCode::WindowEmpty),
        };
        if first.1 == 0 || last.1 == 0 {
            return Err(XcCode::WindowEmpty);
        }
        let r_first = ((first.0 as i64) * 1_000_000 / (first.1 as i64)) as i64;
        let r_last = ((last.0 as i64) * 1_000_000 / (last.1 as i64)) as i64;
        let delta = r_last - r_first;
        if delta > TREND_FLAT_BAND_PPM {
            Ok(Trend::Improving { delta_ppm: delta })
        } else if delta < -TREND_FLAT_BAND_PPM {
            Ok(Trend::Degrading { delta_ppm: delta })
        } else {
            Ok(Trend::Flat)
        }
    }

    /// 统计摘要（人读；台账/遥测渲染面）。
    pub fn summary(&self) -> String {
        let mut s = String::from("间隔账：直方 ");
        let mut i = 0usize;
        while i < BUCKET_TOTAL {
            if let Some(v) = self.hist.get(i) {
                if *v > 0 {
                    s.push_str(&format!("{:?}", i));
                    s.push('桶');
                    s.push_str(&v.to_string());
                    s.push(' ');
                }
            }
            i += 1;
        }
        s.push_str("｜事故 ");
        s.push_str(&self.grade_severe.to_string());
        s
    }
}

impl Default for IntervalStats {
    fn default() -> IntervalStats {
        IntervalStats::new()
    }
}

/// 长期趋势三态（判据四的结论面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    /// 改善（健康比升幅 >1%）。
    Improving {
        /// 升幅（ppm）。
        delta_ppm: i64,
    },
    /// 持平（±1% 带宽内）。
    Flat,
    /// 退化（健康比降幅 >1%）。
    Degrading {
        /// 降幅（ppm，负值）。
        delta_ppm: i64,
    },
}

impl Trend {
    /// 趋势名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            Trend::Improving { .. } => "改善",
            Trend::Flat => "持平",
            Trend::Degrading { .. } => "退化",
        }
    }
}
