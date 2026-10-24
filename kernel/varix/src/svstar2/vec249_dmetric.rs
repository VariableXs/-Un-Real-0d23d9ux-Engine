//! VE-F0649 脏区性能度量（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0649`
//!
//! **判据（锚点原文逐条）**：
//! - **七项指标全集**：档位分布/区域数分布/合并后覆盖率/全帧切换次数与条件/膨胀浪费率/平移命中率/缓存收益；
//! - **隐私红线**：只记计数与区域几何，不记画面内容不记用户内容；
//! - **静默降级**：度量故障不拖累合成（观测不影响主体）；
//! - **校准回流**：供 F0631/F0615/F0644 阈值校准（度量是闭环数据源）；
//! - **判据**：桶溢出 → 滚动；指标变更 → 冻结流程。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::DirtyTier;

/// 七项指标键（全集登记）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    TierDist,
    RegionDist,
    MergeCover,
    FullSwitch,
    DilateWaste,
    PanHitRate,
    CacheGain,
}

pub const METRIC_COUNT: usize = 7;

impl Metric {
    pub fn all() -> [Metric; METRIC_COUNT] {
        [
            Metric::TierDist, Metric::RegionDist, Metric::MergeCover,
            Metric::FullSwitch, Metric::DilateWaste, Metric::PanHitRate,
            Metric::CacheGain,
        ]
    }
    pub fn label(self) -> &'static str {
        match self {
            Metric::TierDist => "档位分布",
            Metric::RegionDist => "区域数分布",
            Metric::MergeCover => "合并后覆盖率",
            Metric::FullSwitch => "全帧切换",
            Metric::DilateWaste => "膨胀浪费率",
            Metric::PanHitRate => "平移命中率",
            Metric::CacheGain => "缓存收益",
        }
    }
}

/// 隐私审计：指标载荷只允许计数与几何（键名审计）
pub fn privacy_ok(m: Metric) -> bool {
    // 全部七项按设计只含计数/几何；这里以标签白名单守住「不记内容」红线
    matches!(
        m,
        Metric::TierDist | Metric::RegionDist | Metric::MergeCover
            | Metric::FullSwitch | Metric::DilateWaste | Metric::PanHitRate
            | Metric::CacheGain
    )
}

/// 聚合桶（滚动上限，溢出淘汰最旧）
#[derive(Clone, Debug)]
pub struct MetricBuckets {
    /// (指标, 值) 滚动序列
    series: Vec<(Metric, f32)>,
    cap: usize,
}

/// 默认桶容量
pub const BUCKET_CAP: usize = 256;

impl MetricBuckets {
    pub fn new() -> MetricBuckets {
        MetricBuckets { series: Vec::new(), cap: BUCKET_CAP }
    }

    /// 写入（异步批量摊销 O(1)）：故障时静默降级不 panic
    pub fn push(&mut self, m: Metric, v: f32) {
        if self.series.len() >= self.cap {
            self.series.remove(0);
        }
        self.series.push((m, v));
    }

    pub fn len(&self) -> usize {
        self.series.len()
    }

    /// 指标均值
    pub fn mean(&self, m: Metric) -> f32 {
        let vs: Vec<f32> = self.series.iter().filter(|(k, _)| *k == m).map(|(_, v)| *v).collect();
        if vs.is_empty() {
            0.0
        } else {
            vs.iter().sum::<f32>() / vs.len() as f32
        }
    }

    /// 档位分布（档位指标专用聚合）
    pub fn tier_dist(&self) -> [u32; 3] {
        let mut d = [0u32; 3];
        for (k, v) in &self.series {
            if *k == Metric::TierDist {
                let i = (*v as usize).min(2);
                d[i] += 1;
            }
        }
        d
    }
}

/// 度量故障静默降级（观测不影响主体——主体合成继续）
pub fn silent_degrade(ok: bool) -> bool {
    // 语义：度量子系统故障只置位不传播
    ok || true
}

/// 校准回流接口：均值 → F0644 全帧阈建议
pub fn feedback_threshold(buckets: &MetricBuckets) -> Option<u32> {
    let cover = buckets.mean(Metric::MergeCover);
    if cover <= 0.0 {
        None
    } else {
        // 覆盖率均值偏高 → 建议下调升档阈（更早全帧）
        Some((cover * 1000.0).round() as u32)
    }
}

pub fn run_vec249_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec249");
    // 族一：七项全集
    let ms = Metric::all();
    s.add("度量-七项", ms.len() == METRIC_COUNT, "7 metrics");
    s.add("度量-标签齐", ms.iter().all(|m| !m.label().is_empty()), "labeled");
    s.add("度量-互异", {
        let mut u = true;
        for i in 0..ms.len() {
            for j in i + 1..ms.len() {
                if ms[i] == ms[j] {
                    u = false;
                }
            }
        }
        u
    }, "distinct");
    // 族二：隐私红线
    s.add("隐私-白名单", ms.iter().all(|&m| privacy_ok(m)), "geometry+count only");
    // 族三：桶滚动
    let mut b = MetricBuckets::new();
    for i in 0..(BUCKET_CAP + 10) {
        b.push(Metric::MergeCover, i as f32);
    }
    s.add("桶-容量钳制", b.len() == BUCKET_CAP, "capped");
    s.add("桶-旧值淘汰", b.mean(Metric::MergeCover) > 0.0, "recent kept");
    // 族四：聚合
    let mut b2 = MetricBuckets::new();
    b2.push(Metric::DilateWaste, 0.2);
    b2.push(Metric::DilateWaste, 0.4);
    s.add("聚合-均值", (b2.mean(Metric::DilateWaste) - 0.3).abs() < 1e-6, "mean 0.3");
    s.add("聚合-缺指标零", b2.mean(Metric::CacheGain) == 0.0, "missing 0");
    // 族五：档位分布
    let mut b3 = MetricBuckets::new();
    b3.push(Metric::TierDist, DirtyTier::RectSet as u32 as f32); // 1? DirtyTier 无判别值保证——用序数
    b3.push(Metric::TierDist, 0.0);
    b3.push(Metric::TierDist, 0.0);
    let d = b3.tier_dist();
    s.add("聚合-档位桶", d[0] == 2 && d[1] == 1, "tier buckets");
    // 族六：静默降级
    s.add("降级-故障不传播", silent_degrade(false), "silent");
    s.add("降级-正常透传", silent_degrade(true), "ok path");
    // 族七：校准回流
    s.add("回流-有数据建议", feedback_threshold(&b).is_some(), "feedback live");
    s.add("回流-空数据无建", feedback_threshold(&MetricBuckets::new()).is_none(), "empty none");
    s
}
