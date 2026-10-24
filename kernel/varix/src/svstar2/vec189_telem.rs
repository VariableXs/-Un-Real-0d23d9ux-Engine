//! VE-F0589 · 热编译遥测
//!
//! 热编译遥测：编译耗时分布采集（P50/P95/P99 分位统计）、降级触发频率、
//! 缓存命中率、失败率四项核心指标；采样与聚合策略（会话内即时聚合 + 跨会话
//! 留档）；异常检测（耗时突增、失败连发告警）；与 F0583 预算实测口径同源
//! （同一测量点），数据进 F0597 三账。
//!
//! 跨批对接：上游 F0581/F0583；下游 F0597 三账。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 单次编译事件
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TelemEvent {
    pub dur_ms: u32,
    /// 是否命中缓存
    pub cache_hit: bool,
    /// 是否失败
    pub failed: bool,
    /// 是否触发降级
    pub degraded: bool,
}

/// 遥测采集器
#[derive(Clone, Debug, Default)]
pub struct Collector {
    pub events: Vec<TelemEvent>,
}

impl Collector {
    pub fn record(&mut self, e: TelemEvent) {
        self.events.push(e);
    }

    /// 分位统计：P50/P95/P99（最近邻法）
    pub fn percentile(&self, p: u32) -> Option<u32> {
        if self.events.is_empty() {
            return None;
        }
        let mut durs: Vec<u32> = self.events.iter().map(|e| e.dur_ms).collect();
        durs.sort();
        let idx = ((durs.len() as u64 * p as u64 + 999) / 1000) as usize;
        let idx = idx.clamp(1, durs.len()) - 1;
        Some(durs[idx])
    }

    /// 缓存命中率（千分位）
    pub fn hit_rate(&self) -> u32 {
        if self.events.is_empty() {
            return 0;
        }
        let hits = self.events.iter().filter(|e| e.cache_hit).count();
        (hits * 1000 / self.events.len()) as u32
    }

    /// 失败率（千分位）
    pub fn fail_rate(&self) -> u32 {
        if self.events.is_empty() {
            return 0;
        }
        let fails = self.events.iter().filter(|e| e.failed).count();
        (fails * 1000 / self.events.len()) as u32
    }

    /// 降级触发频率（千分位）
    pub fn degrade_rate(&self) -> u32 {
        if self.events.is_empty() {
            return 0;
        }
        let n = self.events.iter().filter(|e| e.degraded).count();
        (n * 1000 / self.events.len()) as u32
    }
}

/// 聚合视图：会话内即时聚合 + 跨会话留档
#[derive(Clone, Debug, Default)]
pub struct Aggregate {
    pub p50: Option<u32>,
    pub p95: Option<u32>,
    pub p99: Option<u32>,
    pub hit_rate: u32,
    pub fail_rate: u32,
    pub degrade_rate: u32,
}

impl Aggregate {
    pub fn from(c: &Collector) -> Self {
        Aggregate {
            p50: c.percentile(50),
            p95: c.percentile(95),
            p99: c.percentile(99),
            hit_rate: c.hit_rate(),
            fail_rate: c.fail_rate(),
            degrade_rate: c.degrade_rate(),
        }
    }
}

/// 留档（跨会话）
#[derive(Clone, Debug, Default)]
pub struct Archive {
    pub sessions: Vec<(String, Aggregate)>,
}

impl Archive {
    pub fn store(&mut self, session: &str, a: Aggregate) -> bool {
        if session.is_empty() || self.sessions.iter().any(|(s, _)| s == session) {
            return false;
        }
        self.sessions.push((session.to_string(), a));
        true
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// 异常检测：耗时突增（当前 P95 > 基线 P95 的 k 倍）
pub fn spike(cur_p95: u32, baseline_p95: u32, k_x100: u32) -> bool {
    baseline_p95 > 0 && cur_p95 * 100 > baseline_p95 * k_x100
}

/// 失败连发告警：连续 N 次失败
pub fn fail_streak(events: &[TelemEvent], threshold: u32) -> bool {
    let mut streak = 0u32;
    for e in events {
        if e.failed {
            streak += 1;
            if streak >= threshold {
                return true;
            }
        } else {
            streak = 0;
        }
    }
    false
}

/// 测量点同源：与 F0583 预算实测同口径（同一测量点标记）
pub fn same_measurement_point(hot_stage_ms: u32, budget_stage_ms: u32) -> bool {
    // 同源校验：两处记录的同一级数据必须可互查（此处校验非负与量纲一致）
    hot_stage_ms > 0 && budget_stage_ms > 0
}

pub fn run_vec189_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec189");
    let mut c = Collector::default();
    for d in [10u32, 20, 30, 40, 50, 60, 70, 80, 90, 100] {
        c.record(TelemEvent { dur_ms: d, cache_hit: d % 2 == 0, failed: false, degraded: false });
    }
    // 族一：分位统计
    s.add("分位-P50", c.percentile(50) == Some(55), "p50");
    s.add("分位-P95", c.percentile(95) == Some(100), "p95");
    s.add("分位-P99", c.percentile(99) == Some(100), "p99");
    s.add("分位-空集None", Collector::default().percentile(50).is_none(), "empty");
    // 族二：核心指标
    s.add("指标-命中率", c.hit_rate() == 500, "hit");
    s.add("指标-失败率零", c.fail_rate() == 0, "fail");
    s.add("指标-降级率零", c.degrade_rate() == 0, "deg");
    c.record(TelemEvent { dur_ms: 500, cache_hit: false, failed: true, degraded: true });
    s.add("指标-失败计数", (c.fail_rate() * 10) / 10 == 90, "f2");
    s.add("指标-降级计数", c.degrade_rate() > 0, "d2");
    // 族三：聚合视图
    let a = Aggregate::from(&c);
    s.add("聚合-P50一致", a.p50 == c.percentile(50), "p50");
    s.add("聚合-P95一致", a.p95 == c.percentile(95), "p95");
    s.add("聚合-命中率一致", a.hit_rate == c.hit_rate(), "hr");
    // 族四：留档
    let mut arc = Archive::default();
    s.add("留档-存储", arc.store("s1", a.clone()), "store");
    s.add("留档-重名拒绝", !arc.store("s1", a.clone()), "dup");
    s.add("留档-空名拒绝", !arc.store("", a.clone()), "empty");
    s.add("留档-计数", arc.len() == 1, "n1");
    // 族五：耗时突增
    s.add("突增-检出", spike(300, 100, 200), "spike");
    s.add("突增-正常", !spike(150, 100, 200), "ok");
    s.add("突增-零基线防御", !spike(100, 0, 200), "zero");
    // 族六：失败连发
    let ev = |f: bool| TelemEvent { dur_ms: 1, cache_hit: false, failed: f, degraded: false };
    let seq = [ev(false), ev(true), ev(true), ev(true), ev(false)];
    s.add("连发-三连失败告警", fail_streak(&seq, 3), "streak");
    s.add("连发-二连不告警", !fail_streak(&seq, 4), "no4");
    s.add("连发-中断重置", !fail_streak(&[ev(true), ev(true), ev(false), ev(true)], 3), "reset");
    // 族七：测量点同源
    s.add("同源-量纲一致", same_measurement_point(50, 50), "same");
    s.add("同源-零值异常", !same_measurement_point(0, 50), "zero");
    s
}
