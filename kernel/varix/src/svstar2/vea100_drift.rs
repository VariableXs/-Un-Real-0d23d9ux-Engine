//! VE-F0100 · 长稳基线漂移检测（VE-A 域 · A04b 诊断·降级·安全组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0100`
//!
//! **职责定位（锚点原文）**：对关键性能指标维护长稳基线并检测漂移——滚动窗口
//! 分位数草图（P50/P95/P99）周期性对比当前分布与基线，均值偏移或方差膨胀超阈值
//! 即产出漂移报告（指标、方向、幅度、起始时间估计）；基线版本化，升级后重置并
//! 标注版本变更原因。
//!
//! **铁律**：
//! 1. **滚动分位**——P50/P95/P99 在线草图，单样本 O(1)。
//! 2. **版本化基线**——升级重置并标注原因。
//! 3. **失联区分**——样本不足跳过、指标停更标失联，不误报漂移。
//! 4. **限频**——报告产出限频防风暴。

use alloc::vec;
use alloc::vec::Vec;
use crate::float::FloatExt;

/// 诊断段基址 0xE8AB（漂移检测）。
pub const SEG_BASE: u16 = 0xE8AB;

/// 阈值配置非法。
pub const XC_THRESHOLD_INVALID: u16 = SEG_BASE | 0x01;
/// 样本不足。
pub const XC_INSUFFICIENT: u16 = SEG_BASE | 0x02;
/// 指标失联。
pub const XC_METRIC_LOST: u16 = SEG_BASE | 0x03;
/// 报告限频中。
pub const XC_RATELIMITED: u16 = SEG_BASE | 0x04;

/// 漂移方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriftDir {
    /// 均值上移（劣化向）。
    Up,
    /// 均值下移。
    Down,
    /// 方差膨胀。
    Inflated,
}

/// 分位快照。
#[derive(Clone, Copy, Debug)]
pub struct Quantiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    /// 样本数。
    pub n: u32,
}

/// 基线快照（版本化）。
#[derive(Clone, Debug)]
pub struct Baseline {
    pub version: u32,
    pub quantiles: Quantiles,
    /// 版本变更原因（文字型码：0 初始 1 升级 2 手动重置）。
    pub reason: u8,
}

/// 漂移报告。
#[derive(Clone, Debug)]
pub struct DriftReport {
    pub metric_id: u32,
    pub dir: DriftDir,
    /// 幅度（相对基线 P50 的比例，千分比有符号）。
    pub magnitude_permille: i32,
    /// 起始时间估计（tick）。
    pub since_tick: u64,
}

/// 在线分位草图（简化：固定窗口排序采样）。
#[derive(Clone, Debug)]
pub struct QuantileSketch {
    window: Vec<f64>,
    cap: usize,
    head: usize,
    filled: usize,
}

impl QuantileSketch {
    pub fn new(cap: usize) -> Self {
        let c = if cap < 8 { 64 } else { cap.min(1024) };
        QuantileSketch {
            window: vec![0.0; c],
            cap: c,
            head: 0,
            filled: 0,
        }
    }

    /// O(1) 推入。
    pub fn push(&mut self, v: f64) {
        self.window[self.head] = v;
        self.head = (self.head + 1) % self.cap;
        if self.filled < self.cap {
            self.filled += 1;
        }
    }

    /// 计算分位（O(n log n) 检测期调用，非采样路径）。
    pub fn quantiles(&self) -> Quantiles {
        if self.filled == 0 {
            return Quantiles {
                p50: 0.0,
                p95: 0.0,
                p99: 0.0,
                n: 0,
            };
        }
        let mut sorted: Vec<f64> = self.window[..self.filled].to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let n = sorted.len();
        let at = |q: f64| -> f64 {
            let idx = ((q * (n as f64 - 1.0)).m_round() as usize).min(n - 1);
            sorted[idx]
        };
        Quantiles {
            p50: at(0.50),
            p95: at(0.95),
            p99: at(0.99),
            n: n as u32,
        }
    }

    pub fn len(&self) -> usize {
        self.filled
    }
}

/// 漂移检测器。
pub struct DriftDetector {
    metric_id: u32,
    sketch: QuantileSketch,
    baseline: Option<Baseline>,
    /// 均值偏移阈值（千分比）。
    shift_threshold_permille: i32,
    /// 方差膨胀阈值（P99/P50 比值 ×1000）。
    inflate_threshold: u32,
    min_samples: u32,
    last_tick: u64,
    /// 报告限频窗口。
    report_cooldown: u64,
    last_report_tick: u64,
    /// 最近一次有样本的 tick（失联判定）。
    last_seen_tick: u64,
    last_code: u16,
    reports: Vec<DriftReport>,
}

impl DriftDetector {
    pub fn new(
        metric_id: u32,
        window: usize,
        shift_threshold_permille: i32,
        inflate_threshold: u32,
        min_samples: u32,
        report_cooldown: u64,
    ) -> Result<Self, u16> {
        if shift_threshold_permille == 0 || inflate_threshold < 1000 || min_samples == 0 {
            return Err(XC_THRESHOLD_INVALID);
        }
        Ok(DriftDetector {
            metric_id,
            sketch: QuantileSketch::new(window),
            baseline: None,
            shift_threshold_permille,
            inflate_threshold,
            min_samples,
            last_tick: 0,
            report_cooldown,
            last_report_tick: 0,
            last_seen_tick: 0,
            last_code: 0,
            reports: Vec::new(),
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 推入单样本 O(1)。
    pub fn push_sample(&mut self, v: f64, tick: u64) {
        self.sketch.push(v);
        self.last_tick = tick;
        self.last_seen_tick = tick;
    }

    /// 采集当前分位为基线（版本化）。
    pub fn capture_baseline(&mut self, version: u32, reason: u8) -> Result<Baseline, u16> {
        let q = self.sketch.quantiles();
        if q.n < self.min_samples {
            self.last_code = XC_INSUFFICIENT;
            return Err(XC_INSUFFICIENT);
        }
        let b = Baseline {
            version,
            quantiles: q,
            reason,
        };
        self.baseline = Some(b.clone());
        self.last_code = 0;
        Ok(b)
    }

    /// 基线升级重置（版本变更原因显性）：以当前观察窗分布换版为新基线，
    /// 再重置草图开启新观察窗——必须先采后重置，重置后再采只会得到空草图。
    pub fn upgrade_baseline(&mut self, new_version: u32, reason: u8) -> Result<Baseline, u16> {
        let q = self.sketch.quantiles();
        if q.n < self.min_samples {
            self.last_code = XC_INSUFFICIENT;
            return Err(XC_INSUFFICIENT);
        }
        self.baseline = Some(Baseline {
            version: new_version,
            quantiles: q,
            reason,
        });
        self.sketch = QuantileSketch::new(self.sketch.cap);
        self.last_code = 0;
        Ok(self.baseline.as_ref().unwrap().clone())
    }

    /// 周期检测：样本不足跳过；失联标记；超阈产出报告。
    pub fn detect(&mut self, tick: u64) -> Result<Option<DriftReport>, u16> {
        // 失联区分：超过冷却两倍无样本 = 失联而非漂移。
        if tick > self.last_seen_tick + self.report_cooldown * 2 {
            self.last_code = XC_METRIC_LOST;
            return Err(XC_METRIC_LOST);
        }
        let q = self.sketch.quantiles();
        if q.n < self.min_samples {
            self.last_code = XC_INSUFFICIENT;
            return Err(XC_INSUFFICIENT);
        }
        let base = match &self.baseline {
            Some(b) => b.clone(),
            None => {
                self.last_code = XC_INSUFFICIENT;
                return Err(XC_INSUFFICIENT);
            }
        };
        // 限频。
        if self.last_report_tick != 0 && tick < self.last_report_tick + self.report_cooldown {
            self.last_code = XC_RATELIMITED;
            return Err(XC_RATELIMITED);
        }
        // 均值偏移（以 P50 近似）。
        let b50 = base.quantiles.p50;
        let mut dir: Option<DriftDir> = None;
        let mut mag: i32 = 0;
        if b50 > 0.0 {
            let ratio = ((q.p50 - b50) / b50 * 1000.0) as i32;
            if ratio.abs() >= self.shift_threshold_permille {
                dir = Some(if ratio > 0 {
                    DriftDir::Up
                } else {
                    DriftDir::Down
                });
                mag = ratio;
            }
        }
        // 方差膨胀（P99/P50 比值）。
        if dir.is_none() && q.p50 > 0.0 {
            let spread = (q.p99 / q.p50 * 1000.0) as u32;
            let base_spread = if base.quantiles.p50 > 0.0 {
                (base.quantiles.p99 / base.quantiles.p50 * 1000.0) as u32
            } else {
                1000
            };
            if spread > self.inflate_threshold || spread > base_spread.saturating_mul(2) {
                dir = Some(DriftDir::Inflated);
                mag = (spread as i32) - (base_spread as i32);
            }
        }
        match dir {
            Some(d) => {
                let rep = DriftReport {
                    metric_id: self.metric_id,
                    dir: d,
                    magnitude_permille: mag,
                    since_tick: self.last_seen_tick,
                };
                self.reports.push(rep.clone());
                self.last_report_tick = tick;
                self.last_code = 0;
                Ok(Some(rep))
            }
            None => {
                self.last_code = 0;
                Ok(None)
            }
        }
    }

    pub fn reports(&self) -> &[DriftReport] {
        &self.reports
    }

    pub fn baseline(&self) -> Option<&Baseline> {
        self.baseline.as_ref()
    }

    pub fn sample_count(&self) -> usize {
        self.sketch.len()
    }
}

/// 自检：滚动分位、版本化基线、失联区分、限频。
pub fn run_vea100_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea100_drift");

    // C100-01 滚动分位计算
    {
        let mut sk = QuantileSketch::new(64);
        for i in 1..=100 {
            sk.push(i as f64);
        }
        let q = sk.quantiles();
        // 滚动窗语义：64 容量收 100 样本，保留末 64 个（37..=100），
        // 期望按窗内语料手算：n=64、P50≈68.5、P99=100。
        let ok = q.n == 64 && q.p50 > 65.0 && q.p50 < 72.0 && q.p99 >= 99.0;
        chk(&mut set, "C100-01 滚动分位", ok, "分位偏差");
    }

    // C100-02 阈值非法拒绝
    {
        let r = DriftDetector::new(1, 64, 0, 1500, 10, 5);
        chk(
            &mut set,
            "C100-02 阈值拒绝",
            r.err() == Some(XC_THRESHOLD_INVALID),
            "非法阈值被放行",
        );
    }

    // C100-03 版本化基线
    {
        let mut d = DriftDetector::new(1, 64, 50, 1500, 5, 5).unwrap();
        for i in 0..20 {
            d.push_sample(100.0 + (i % 3) as f64, i);
        }
        let r = d.capture_baseline(1, 0);
        chk(&mut set, "C100-03 版本化基线", r.is_ok() && d.baseline().unwrap().version == 1, "基线失败");
    }

    // C100-04 均值偏移检出
    {
        let mut d = DriftDetector::new(1, 128, 50, 2000, 5, 5).unwrap();
        for _ in 0..20 {
            d.push_sample(100.0, 1);
        }
        let _ = d.capture_baseline(1, 0);
        for _ in 0..20 {
            d.push_sample(200.0, 10);
        }
        let r = d.detect(10);
        let ok = r.is_ok() && r.unwrap().map(|x| x.dir) == Some(DriftDir::Up);
        chk(&mut set, "C100-04 均值偏移", ok, "偏移漏检");
    }

    // C100-05 样本不足跳过不误报
    {
        let mut d = DriftDetector::new(1, 64, 50, 2000, 20, 5).unwrap();
        d.push_sample(1.0, 1);
        let r = d.detect(1);
        chk(
            &mut set,
            "C100-05 不足跳过",
            r.is_err() && d.last_code() == XC_INSUFFICIENT,
            "不足被误报",
        );
    }

    // C100-06 失联区分
    {
        let mut d = DriftDetector::new(1, 64, 50, 2000, 5, 5).unwrap();
        for _ in 0..10 {
            d.push_sample(10.0, 1);
        }
        let _ = d.capture_baseline(1, 0);
        // 100 tick 无样本 → 失联。
        let r = d.detect(100);
        chk(
            &mut set,
            "C100-06 失联区分",
            r.is_err() && d.last_code() == XC_METRIC_LOST,
            "失联被当漂移",
        );
    }

    // C100-07 报告限频
    {
        let mut d = DriftDetector::new(1, 128, 50, 2000, 5, 10).unwrap();
        for _ in 0..20 {
            d.push_sample(100.0, 1);
        }
        let _ = d.capture_baseline(1, 0);
        for _ in 0..20 {
            d.push_sample(300.0, 2);
        }
        let _ = d.detect(2);
        let r2 = d.detect(3);
        chk(
            &mut set,
            "C100-07 报告限频",
            r2.is_err() && d.last_code() == XC_RATELIMITED,
            "限频失效",
        );
    }

    // C100-08 基线升级重置
    {
        let mut d = DriftDetector::new(1, 64, 50, 2000, 5, 5).unwrap();
        for _ in 0..10 {
            d.push_sample(50.0, 1);
        }
        let _ = d.capture_baseline(1, 0);
        let r = d.upgrade_baseline(2, 1);
        let ok = r.is_ok() && d.baseline().unwrap().version == 2 && d.baseline().unwrap().reason == 1;
        chk(&mut set, "C100-08 升级重置", ok, "升级失败");
    }

    // C100-09 方差膨胀检出
    {
        let mut d = DriftDetector::new(1, 256, 500, 1500, 5, 5).unwrap();
        for _ in 0..30 {
            d.push_sample(100.0, 1);
        }
        let _ = d.capture_baseline(1, 0);
        // 半数极低半数极高 → P99/P50 膨胀。
        for i in 0..30 {
            d.push_sample(if i % 2 == 0 { 10.0 } else { 10000.0 }, 2);
        }
        let r = d.detect(2);
        let ok = r.is_ok() && r.unwrap().is_some();
        chk(&mut set, "C100-09 膨胀检出", ok, "膨胀漏检");
    }

    // C100-10 报告带起始估计
    {
        let mut d = DriftDetector::new(7, 128, 50, 2000, 5, 5).unwrap();
        for _ in 0..20 {
            d.push_sample(10.0, 42);
        }
        let _ = d.capture_baseline(1, 0);
        for _ in 0..20 {
            d.push_sample(20.0, 42);
        }
        let r = d.detect(50);
        let ok = r.unwrap().map(|x| x.since_tick) == Some(42) && d.reports().len() == 1;
        chk(&mut set, "C100-10 起始估计", ok, "起始缺失");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea100_all_green() {
        let s = run_vea100_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
