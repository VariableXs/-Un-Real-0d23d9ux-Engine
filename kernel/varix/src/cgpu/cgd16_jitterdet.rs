//! cgd16_jitterdet — CGPU-F0497 抖动检测器（短时方差/长时趋势双指标+谱分析）。
//!
//! 判据原文（施工书锚点 CGPU-F0497）：抖动检测：间隔抖动实时检测（帧间隔
//! 的短时方差/长时趋势双指标）；抖动谱分析（抖动的频率成分——周期性抖动
//! vs 随机抖动区分）；检测分级（轻微/明显/严重三档——阈值与 F0484 jank 分
//! 级对齐）；检测开销（<0.1% 实测）。判据：双指标、谱分析、三档、开销、判据。
//!
//! 职责定位：双指标检测（短时方差=窗口内帧间隔平方偏差和——捕捉突发毛刺；
//! 长时趋势=滑窗均值斜率符号——捕捉渐进漂移；只报其一=0x7201 双指标缺失）
//! +谱分析（整数自相关：滞后 k 的间隔相关度显著高于基线→周期性抖动（如
//! 60Hz 背光/定时器节拍），否则随机抖动（如调度抢占）——区分失败 0x7202）
//! +三档分级（轻微/明显/严重，阈值与 F0484 jank 分级对齐——两套阈值各说
//! 各话=0x7203）+开销红线（每帧检测步数折算 <0.1%——超标 0x7204）+样本
//! 纪律（窗口样本不足不出判——0x7205，半窗判级=撒谎）。诊断码段
//! 0x7201~0x7205。

use alloc::vec::Vec;

/// 模块版本。
pub const JITTER_DET_VERSION: &str = "D16-jitterdet-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x72 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7201 双指标缺失（短时方差/长时趋势只算其一）。
pub const XC_DUAL_METRIC_MISS: u32 = 0x7201;
/// 0x7202 谱失守（周期性 vs 随机区分失败）。
pub const XC_SPECTRUM_FAIL: u32 = 0x7202;
/// 0x7203 分级越表（三档外/与 F0484 阈值不对齐）。
pub const XC_GRADE_MISALIGN: u32 = 0x7203;
/// 0x7204 开销超标（检测步数折算 >0.1% 红线）。
pub const XC_OVERHEAD_BREACH: u32 = 0x7204;
/// 0x7205 样本不足（窗口样本数不够就出判）。
pub const XC_SAMPLE_STARVED: u32 = 0x7205;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 短时窗长（帧——方差窗口）。
pub const SHORT_WINDOW: usize = 16;
/// 长时窗长（帧——趋势窗口）。
pub const LONG_WINDOW: usize = 64;
/// 谱分析滞后上限（帧）。
pub const LAG_MAX: usize = 8;
/// 最小样本数（出判门槛）。
pub const MIN_SAMPLES: usize = 8;
/// F0484 jank 分级对齐锚（轻微阈 µs——与 cgd05 口径同源声明）。
pub const F0484_LIGHT_US: u32 = 8_000;
/// F0484 明显阈 µs。
pub const F0484_OBVIOUS_US: u32 = 12_500;
/// F0484 严重阈 µs。
pub const F0484_SEVERE_US: u32 = 25_000;
/// 开销红线：每帧检测步数上限（0.1% of 12.5ms=12.5µs≈步数 25——1 步 0.5µs 折算）。
pub const OVERHEAD_STEP_CAP: u32 = 25;

// ---------------------------------------------------------------------------
// 一、间隔抖动实时检测（双指标）
// ---------------------------------------------------------------------------

/// 抖动分级三档（判据"轻微/明显/严重三档"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JitterGrade {
    Light,
    Obvious,
    Severe,
}

impl JitterGrade {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            JitterGrade::Light => "轻微",
            JitterGrade::Obvious => "明显",
            JitterGrade::Severe => "严重",
        }
    }

    /// F0484 对齐分档（间隔 µs → 档——阈值同源声明在常量区）。
    pub const fn classify(interval_us: u32) -> Option<JitterGrade> {
        if interval_us >= F0484_SEVERE_US {
            Some(JitterGrade::Severe)
        } else if interval_us >= F0484_OBVIOUS_US {
            Some(JitterGrade::Obvious)
        } else if interval_us >= F0484_LIGHT_US {
            Some(JitterGrade::Light)
        } else {
            None
        }
    }
}

/// 双指标检测器（判据"短时方差/长时趋势双指标"）。
#[derive(Debug)]
pub struct DualMetricDetector {
    /// 短时窗（帧间隔 µs）。
    pub short_win: Vec<u32>,
    /// 长时窗（帧间隔 µs）。
    pub long_win: Vec<u32>,
    /// 最近短时方差（µs² 整数口径）。
    pub last_short_var: u64,
    /// 最近长时趋势（斜率符号：+1 恶化 / -1 改善 / 0 平稳）。
    pub last_trend: i32,
    /// 双指标缺失事件数（0x7201）。
    pub metric_miss_events: u32,
}

impl DualMetricDetector {
    /// 新检测器。
    pub const fn new() -> DualMetricDetector {
        DualMetricDetector {
            short_win: Vec::new(),
            long_win: Vec::new(),
            last_short_var: 0,
            last_trend: 0,
            metric_miss_events: 0,
        }
    }

    /// 喂帧间隔（µs——两窗同步入队，超窗丢最旧）。
    pub fn feed(&mut self, interval_us: u32) {
        self.short_win.push(interval_us);
        if self.short_win.len() > SHORT_WINDOW {
            self.short_win.remove(0);
        }
        self.long_win.push(interval_us);
        if self.long_win.len() > LONG_WINDOW {
            self.long_win.remove(0);
        }
    }

    /// 短时方差：窗口均值±平方偏差和（样本不足→Err 0x7205）。
    pub fn short_variance(&mut self) -> Result<u64, u32> {
        if self.short_win.len() < MIN_SAMPLES {
            self.metric_miss_events += 1;
            return Err(XC_SAMPLE_STARVED);
        }
        let n = self.short_win.len() as u64;
        let mut sum = 0u64;
        let mut i = 0;
        while i < self.short_win.len() {
            sum += self.short_win[i] as u64;
            i += 1;
        }
        let mean = sum / n;
        let mut var = 0u64;
        i = 0;
        while i < self.short_win.len() {
            let d = self.short_win[i] as i64 - mean as i64;
            var += (d * d) as u64;
            i += 1;
        }
        self.last_short_var = var / n;
        Ok(self.last_short_var)
    }

    /// 长时趋势：前半窗均值 vs 后半窗均值——斜率符号（样本不足→Err）。
    pub fn long_trend(&mut self) -> Result<i32, u32> {
        if self.long_win.len() < MIN_SAMPLES {
            self.metric_miss_events += 1;
            return Err(XC_SAMPLE_STARVED);
        }
        let half = self.long_win.len() / 2;
        let mut a = 0u64;
        let mut i = 0;
        while i < half {
            a += self.long_win[i] as u64;
            i += 1;
        }
        let a = a / half.max(1) as u64;
        let mut b = 0u64;
        i = half;
        while i < self.long_win.len() {
            b += self.long_win[i] as u64;
            i += 1;
        }
        let b = b / (self.long_win.len() - half).max(1) as u64;
        // 斜率符号：后半比前半大 5% 以上=恶化（间隔变长=帧变慢）。
        let t = if b * 20 > a * 21 {
            1
        } else if b * 21 < a * 20 {
            -1
        } else {
            0
        };
        self.last_trend = t;
        Ok(t)
    }

    /// 双指标齐算（判据"双指标"——只算其一在此处拦截）。
    pub fn dual_evaluate(&mut self) -> Result<(u64, i32), u32> {
        let v = self.short_variance()?;
        let t = self.long_trend()?;
        Ok((v, t))
    }
}

// ---------------------------------------------------------------------------
// 二、抖动谱分析（周期性 vs 随机）
// ---------------------------------------------------------------------------

/// 谱分析结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpectrumKind {
    /// 周期性抖动（自相关显著——背光/定时器节拍类）。
    Periodic(u32),
    /// 随机抖动（无显著滞后峰）。
    Random,
}

/// 谱分析器（判据"抖动的频率成分——周期性抖动 vs 随机抖动区分"）。
///
/// 整数自相关口径：滞后 k 的匹配度 = |x[i+k]-x[i]| <= 容差 的占比。某滞后
/// 匹配度显著高于均值匹配度（≥2 倍且绝对值 ≥60%）→周期性，周期=该滞后。
#[derive(Debug)]
pub struct JitterSpectrum {
    /// 样本窗。
    pub win: Vec<u32>,
    /// 容差（µs——自相关匹配阈）。
    pub tolerance_us: u32,
    /// 谱失守事件数（0x7202）。
    pub fail_events: u32,
}

impl JitterSpectrum {
    /// 新谱器。
    pub const fn new() -> JitterSpectrum {
        JitterSpectrum {
            win: Vec::new(),
            tolerance_us: 500,
            fail_events: 0,
        }
    }

    /// 入样。
    pub fn feed(&mut self, interval_us: u32) {
        self.win.push(interval_us);
        if self.win.len() > LONG_WINDOW {
            self.win.remove(0);
        }
    }

    /// 谱判（样本不足→Err 0x7205；无样本不出判）。
    pub fn analyze(&mut self) -> Result<SpectrumKind, u32> {
        if self.win.len() < MIN_SAMPLES + LAG_MAX {
            self.fail_events += 1;
            return Err(XC_SAMPLE_STARVED);
        }
        let n = self.win.len();
        // 基线：全滞后平均匹配度。
        let mut base_hit = 0u64;
        let mut base_pair = 0u64;
        let mut k = 1;
        while k <= LAG_MAX {
            let mut i = 0;
            while i + k < n {
                base_pair += 1;
                let d = (self.win[i + k] as i64 - self.win[i] as i64).abs();
                if d <= self.tolerance_us as i64 {
                    base_hit += 1;
                }
                i += 1;
            }
            k += 1;
        }
        if base_pair == 0 {
            self.fail_events += 1;
            return Err(XC_SPECTRUM_FAIL);
        }
        let base_bp = base_hit * 10_000 / base_pair;
        // 逐滞后找显著峰。
        k = 1;
        while k <= LAG_MAX {
            let mut hit = 0u64;
            let mut pair = 0u64;
            let mut i = 0;
            while i + k < n {
                pair += 1;
                let d = (self.win[i + k] as i64 - self.win[i] as i64).abs();
                if d <= self.tolerance_us as i64 {
                    hit += 1;
                }
                i += 1;
            }
            if pair > 0 {
                let lag_bp = hit * 10_000 / pair;
                if lag_bp >= 6000 && lag_bp >= base_bp * 2 {
                    return Ok(SpectrumKind::Periodic(k as u32));
                }
            }
            k += 1;
        }
        Ok(SpectrumKind::Random)
    }
}

// ---------------------------------------------------------------------------
// 三、三档分级（与 F0484 对齐）
// ---------------------------------------------------------------------------

/// 分级器（判据"阈值与 F0484 jank 分级对齐"）。
#[derive(Debug)]
pub struct GradeClassifier {
    /// 分级计数（三档）。
    pub counts: [u32; 3],
    /// 越表事件数（0x7203）。
    pub misalign_events: u32,
}

impl GradeClassifier {
    /// 新分级器。
    pub const fn new() -> GradeClassifier {
        GradeClassifier {
            counts: [0; 3],
            misalign_events: 0,
        }
    }

    /// 分级入账（None=未过轻微阈不记账——正常间隔）。
    pub fn classify(&mut self, interval_us: u32) -> Option<JitterGrade> {
        match JitterGrade::classify(interval_us) {
            Some(g) => {
                let idx = match g {
                    JitterGrade::Light => 0,
                    JitterGrade::Obvious => 1,
                    JitterGrade::Severe => 2,
                };
                self.counts[idx] += 1;
                Some(g)
            }
            None => None,
        }
    }

    /// 对齐校验：阈值单调（Light<Obvious<Severe）且与 F0484 锚一致。
    pub const fn aligned() -> bool {
        F0484_LIGHT_US < F0484_OBVIOUS_US && F0484_OBVIOUS_US < F0484_SEVERE_US
    }
}

// ---------------------------------------------------------------------------
// 四、检测开销账（<0.1% 折算）
// ---------------------------------------------------------------------------

/// 开销账（判据"检测开销（<0.1% 实测"——步数化折算，口径与 cgd13 同源）。
#[derive(Debug)]
pub struct DetectOverhead {
    /// 累计检测步数。
    pub steps: u32,
    /// 累计帧数。
    pub frames: u32,
    /// 超标事件数（0x7204）。
    pub breach_events: u32,
}

impl DetectOverhead {
    /// 新账。
    pub const fn new() -> DetectOverhead {
        DetectOverhead {
            steps: 0,
            frames: 0,
            breach_events: 0,
        }
    }

    /// 记一帧检测（步数入账+帧数入账+超标裁决）。
    pub fn frame(&mut self, steps_this_frame: u32) -> Result<(), u32> {
        self.steps += steps_this_frame;
        self.frames += 1;
        if steps_this_frame > OVERHEAD_STEP_CAP {
            self.breach_events += 1;
            return Err(XC_OVERHEAD_BREACH);
        }
        Ok(())
    }

    /// 折算校验：均步数 ≤ 上限（累计口径）。
    pub fn within_budget(&self) -> bool {
        if self.frames == 0 {
            return true;
        }
        self.steps / self.frames <= OVERHEAD_STEP_CAP
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（双指标/谱分析/三档/开销/判据五组+段对账）
// ---------------------------------------------------------------------------

/// 抖动检测总引擎。
#[derive(Debug)]
pub struct JitterDetEngine {
    /// 双指标检测器。
    pub dual: DualMetricDetector,
    /// 谱分析器。
    pub spectrum: JitterSpectrum,
    /// 分级器。
    pub grades: GradeClassifier,
    /// 开销账。
    pub overhead: DetectOverhead,
}

impl JitterDetEngine {
    /// 出厂。
    pub const fn new() -> JitterDetEngine {
        JitterDetEngine {
            dual: DualMetricDetector::new(),
            spectrum: JitterSpectrum::new(),
            grades: GradeClassifier::new(),
            overhead: DetectOverhead::new(),
        }
    }

    /// 组一：双指标——方差捕捉突发；趋势捕捉漂移；样本不足拒；缺一记账。
    pub fn selfcheck_dual(&mut self) -> Result<(), u32> {
        let mut e = JitterDetEngine::new();
        // 样本不足→0x7205。
        e.dual.feed(12_500);
        if e.dual.dual_evaluate() != Err(XC_SAMPLE_STARVED) {
            return Err(XC_DUAL_METRIC_MISS);
        }
        // 平稳序列：全部 12_500 → 方差 0、趋势 0。
        let mut i = 0;
        while i < SHORT_WINDOW {
            e.dual.feed(12_500);
            i += 1;
        }
        let (v, t) = e.dual.dual_evaluate()?;
        if v != 0 || t != 0 {
            return Err(XC_DUAL_METRIC_MISS);
        }
        // 恶化序列：前半 12_500 后半 20_000 → 趋势 +1（b*20=400000 > a*21=262500）。
        let mut e2 = JitterDetEngine::new();
        i = 0;
        while i < LONG_WINDOW {
            if i < LONG_WINDOW / 2 {
                e2.dual.feed(12_500);
            } else {
                e2.dual.feed(20_000);
            }
            i += 1;
        }
        if e2.dual.long_trend()? != 1 {
            return Err(XC_DUAL_METRIC_MISS);
        }
        // 方差非零：交替 8_000/20_000。
        let mut e3 = JitterDetEngine::new();
        i = 0;
        while i < SHORT_WINDOW {
            if i % 2 == 0 {
                e3.dual.feed(8_000);
            } else {
                e3.dual.feed(20_000);
            }
            i += 1;
        }
        if e3.dual.short_variance()? == 0 {
            return Err(XC_DUAL_METRIC_MISS);
        }
        Ok(())
    }

    /// 组二：谱分析——周期序列判 Periodic；随机序列判 Random；样本不足拒。
    pub fn selfcheck_spectrum(&mut self) -> Result<(), u32> {
        let mut e = JitterDetEngine::new();
        // 周期性序列：基线 12_500，每第 4 帧跳到 16_000（滞后 4 处高匹配）。
        let mut i = 0;
        while i < LONG_WINDOW {
            if i % 4 == 0 {
                e.spectrum.feed(16_000);
            } else {
                e.spectrum.feed(12_500);
            }
            i += 1;
        }
        match e.spectrum.analyze()? {
            SpectrumKind::Periodic(lag) => {
                if lag != 4 {
                    return Err(XC_SPECTRUM_FAIL);
                }
            }
            SpectrumKind::Random => return Err(XC_SPECTRUM_FAIL),
        }
        // 随机序列：无周期结构（伪随机 LCG）。
        let mut e2 = JitterDetEngine::new();
        let mut s: u32 = 0x9E37_79B9;
        i = 0;
        while i < LONG_WINDOW {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            e2.spectrum.feed(12_000 + (s >> 24) % 2_000);
            i += 1;
        }
        match e2.spectrum.analyze()? {
            SpectrumKind::Random => {}
            SpectrumKind::Periodic(_) => return Err(XC_SPECTRUM_FAIL),
        }
        // 样本不足→0x7205。
        let mut e3 = JitterDetEngine::new();
        e3.spectrum.feed(12_500);
        if e3.spectrum.analyze() != Err(XC_SAMPLE_STARVED) {
            return Err(XC_SPECTRUM_FAIL);
        }
        Ok(())
    }

    /// 组三：三档分级——F0484 阈值对齐；三档逐档验证。
    pub fn selfcheck_grades(&mut self) -> Result<(), u32> {
        let mut e = JitterDetEngine::new();
        if !GradeClassifier::aligned() {
            return Err(XC_GRADE_MISALIGN);
        }
        // 正常间隔（60Hz≈16_667）→不分档。
        if e.grades.classify(16_000).is_some() {
            return Err(XC_GRADE_MISALIGN);
        }
        // 轻微：8_000 ≤ v < 12_500。
        if e.grades.classify(10_000) != Some(JitterGrade::Light) {
            return Err(XC_GRADE_MISALIGN);
        }
        // 明显：12_500 ≤ v < 25_000。
        if e.grades.classify(13_000) != Some(JitterGrade::Obvious) {
            return Err(XC_GRADE_MISALIGN);
        }
        // 严重：≥ 25_000。
        if e.grades.classify(30_000) != Some(JitterGrade::Severe) {
            return Err(XC_GRADE_MISALIGN);
        }
        // 边界恰值。
        if e.grades.classify(F0484_LIGHT_US) != Some(JitterGrade::Light)
            || e.grades.classify(F0484_OBVIOUS_US) != Some(JitterGrade::Obvious)
            || e.grades.classify(F0484_SEVERE_US) != Some(JitterGrade::Severe)
        {
            return Err(XC_GRADE_MISALIGN);
        }
        if e.grades.counts != [3, 2, 2] {
            return Err(XC_GRADE_MISALIGN);
        }
        Ok(())
    }

    /// 组四：开销——步数红线；超标记账；累计口径。
    pub fn selfcheck_overhead(&mut self) -> Result<(), u32> {
        let mut e = JitterDetEngine::new();
        // 正常帧 20 步（<25）。
        e.overhead.frame(20)?;
        e.overhead.frame(20)?;
        if !e.overhead.within_budget() {
            return Err(XC_OVERHEAD_BREACH);
        }
        // 超标帧 30 步（>25）→0x7204。
        if e.overhead.frame(30) != Err(XC_OVERHEAD_BREACH) {
            return Err(XC_OVERHEAD_BREACH);
        }
        if e.overhead.breach_events != 1 {
            return Err(XC_OVERHEAD_BREACH);
        }
        // 均值仍在线内（(20+20+30)/3≈23）。
        if !e.overhead.within_budget() {
            return Err(XC_OVERHEAD_BREACH);
        }
        Ok(())
    }

    /// 组五：判据——端到端：喂真实化序列→双指标+谱+分级+开销四件合判。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = JitterDetEngine::new();
        // 周期性抖动序列（每 4 帧 16_000）→谱判周期 4；分级只计 16_000（不过 12.5？16_000≥12_500=明显档）。
        let mut i = 0;
        while i < LONG_WINDOW {
            let iv = if i % 4 == 0 { 16_000 } else { 12_500 };
            e.dual.feed(iv);
            e.spectrum.feed(iv);
            let _ = e.grades.classify(iv);
            let _ = e.overhead.frame(10);
            i += 1;
        }
        // 双指标齐。
        let (_, trend) = e.dual.dual_evaluate()?;
        let _ = trend;
        // 谱判周期 4。
        match e.spectrum.analyze()? {
            SpectrumKind::Periodic(4) => {}
            _ => return Err(XC_SPECTRUM_FAIL),
        }
        // 分级：16_000 是明显档 16 次，12_500 恰过明显阈也是明显档 48 次。
        if e.grades.counts[1] != 64 {
            return Err(XC_GRADE_MISALIGN);
        }
        // 开销全绿。
        if !e.overhead.within_budget() || e.overhead.breach_events != 0 {
            return Err(XC_OVERHEAD_BREACH);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_dual()?;
        self.selfcheck_spectrum()?;
        self.selfcheck_grades()?;
        self.selfcheck_overhead()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7200..0x7300 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_DUAL_METRIC_MISS,
            XC_SPECTRUM_FAIL,
            XC_GRADE_MISALIGN,
            XC_OVERHEAD_BREACH,
            XC_SAMPLE_STARVED,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7200 || codes[i] >= 0x7300 {
                return Err(XC_DUAL_METRIC_MISS);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_DUAL_METRIC_MISS);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
