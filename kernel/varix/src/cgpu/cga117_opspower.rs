//! cga117_opspower — CGPU-F0117 CPU 算子功耗测量。
//!
//! 判据原文（施工书锚点 CGPU-F0117）：CPU 算子的功耗维度：算子级能耗
//! 估算（吞吐 × 功耗采样——每算子的能耗画像）、能效比标尺（吞吐/瓦特）、
//! 能效对放置决策（F0004 成本模型加能耗项）。测量依赖 F0117 前置的功耗
//! 采样（CGPU-J 接口）。判据：能耗画像、能效标尺、决策联动、测量方法
//! 文档、数据入册。
//!
//! **设计定盘**：能耗 = 功耗采样 × 采样时长 的积分（整数 µJ 记账：
//! µJ = µW × µs / 1000，为免浮点用 µW·tick 语义——tick 为采样周期，
//! 能耗账单位 µW·tick，换算由外部口径层负责）。每算子的能耗画像 =
//! 单位工作量的能耗（µW·tick / 千次算子调用）。能效标尺 = 千次调用
//! 每单位能耗的吞吐倒数。决策联动 = 成本模型查询接口：按算子 id 查
//! 能耗画像，缺失画像返回 Unknown（不臆造——诚实红线）。
//!
//! 诊断码段 0x0100~0x010F 第 4 子区（0x0130~0x013F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA117_VERSION: &str = "cga117-opspower-v1";

/// 诊断子区基值（0x01 段第 4 子区）。
pub const SEG_BASE: u16 = 0x0130;

/// 能耗画像采样窗下限（tick）——窗太短采样噪声淹没画像。
pub const SAMPLE_WINDOW_MIN_TICKS: u64 = 1_000;

/// 算子能耗画像。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerProfile {
    /// 算子编号（SIMD 算子册冻结号）。
    pub op: u32,
    /// 采样窗长（tick）。
    pub window_ticks: u64,
    /// 窗内累计功耗采样和（µW × 采样次数的累加，即 µW·tick 口径）。
    pub uw_tick_sum: u64,
    /// 采样点数。
    pub samples: u32,
    /// 窗内算子调用次数。
    pub invocations: u64,
}

impl PowerProfile {
    /// 平均功耗（µW 口径的整数近似：uw_tick_sum / samples）。
    /// 无采样返回 None（不臆造零——零功耗是谎言不是缺省）。
    pub fn avg_uw(&self) -> Option<u64> {
        if self.samples == 0 {
            None
        } else {
            Some(self.uw_tick_sum / self.samples as u64)
        }
    }

    /// 能耗画像：千次调用的能耗（µW·tick / 千调用）。
    /// 调用数为零返回 None。
    pub fn energy_per_kilo_calls(&self) -> Option<u64> {
        if self.invocations == 0 {
            None
        } else {
            Some(self.uw_tick_sum * 1000 / self.invocations)
        }
    }

    /// 能效标尺（判据"吞吐/瓦特"的整数化）：千次调用每单位能耗分。
    /// 画像缺失（无采样/无调用）返回 None。
    pub fn efficiency(&self) -> Option<u64> {
        self.energy_per_kilo_calls()
    }

    /// 画像合法性：窗长下限 + 采样点数下限 + 调用数一致性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.window_ticks < SAMPLE_WINDOW_MIN_TICKS {
            return Err(SEG_BASE + 1);
        }
        if self.samples == 0 {
            return Err(SEG_BASE + 2);
        }
        if self.invocations == 0 {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 功耗采样接口的确定性建模（CGPU-J 前置）：采样序列由注入器给入
/// （零墙钟——采样值由上游确定性时钟面提供）。
pub struct PowerSampler {
    /// 注入的采样序列（µW）。
    series: Vec<u32>,
    /// 消费游标。
    cursor: usize,
}

impl PowerSampler {
    /// 以注入序列构造。
    pub fn new(series: Vec<u32>) -> Self {
        PowerSampler { series, cursor: 0 }
    }

    /// 取一个采样点（耗尽返回 None）。
    pub fn next(&mut self) -> Option<u32> {
        let v = self.series.get(self.cursor).copied();
        if v.is_some() {
            self.cursor += 1;
        }
        v
    }

    /// 剩余采样点。
    pub fn remaining(&self) -> usize {
        self.series.len() - self.cursor
    }
}

/// 测量会话：一个算子在固定窗内的采样聚合。
pub struct MeasureSession {
    /// 算子编号。
    op: u32,
    /// 窗长（tick）。
    window_ticks: u64,
    /// 累计 µW·tick。
    sum: u64,
    /// 采样点数。
    samples: u32,
    /// 调用计数。
    invocations: u64,
    /// 是否已收口。
    closed: bool,
}

impl MeasureSession {
    /// 开会话（算子号 0 保留值即拒；窗长低于下限即拒——防噪声画像）。
    pub fn begin(op: u32, window_ticks: u64) -> Result<Self, u16> {
        if op == 0 {
            return Err(SEG_BASE + 4);
        }
        if window_ticks < SAMPLE_WINDOW_MIN_TICKS {
            return Err(SEG_BASE + 1);
        }
        Ok(MeasureSession {
            op,
            window_ticks,
            sum: 0,
            samples: 0,
            invocations: 0,
            closed: false,
        })
    }

    /// 记一个采样点（µW）。
    pub fn sample(&mut self, uw: u32) -> Result<(), u16> {
        if self.closed {
            return Err(SEG_BASE + 5);
        }
        self.sum += uw as u64;
        self.samples += 1;
        Ok(())
    }

    /// 记一批算子调用（次数为零即拒——空批不产生画像）。
    pub fn invoke(&mut self, count: u64) -> Result<(), u16> {
        if self.closed {
            return Err(SEG_BASE + 5);
        }
        if count == 0 {
            return Err(SEG_BASE + 6);
        }
        self.invocations += count;
        Ok(())
    }

    /// 收口出画像（零采样收口即拒——没采样就没有画像）。
    pub fn close(self) -> Result<PowerProfile, u16> {
        if self.closed {
            return Err(SEG_BASE + 5);
        }
        if self.samples == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(PowerProfile {
            op: self.op,
            window_ticks: self.window_ticks,
            uw_tick_sum: self.sum,
            samples: self.samples,
            invocations: self.invocations,
        })
    }
}

/// 能耗画像册（判据"数据入册"）。
pub struct PowerBook {
    /// 画像（登记序）。
    profiles: Vec<PowerProfile>,
    /// 封册指纹。
    sealed: Option<u64>,
}

impl PowerBook {
    /// 空册。
    pub fn new() -> Self {
        PowerBook {
            profiles: Vec::new(),
            sealed: None,
        }
    }

    /// 入册（算子重复即拒；画像自检不过即拒）。
    pub fn ingest(&mut self, p: PowerProfile) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 7);
        }
        p.validate()?;
        if self.profiles.iter().any(|q| q.op == p.op) {
            return Err(SEG_BASE + 8);
        }
        self.profiles.push(p);
        Ok(())
    }

    /// 成本模型联动查询（判据"决策联动"）：按算子查画像；
    /// 无画像返回 None——决策器必须按 Unknown 消费（诚实红线）。
    pub fn profile_of(&self, op: u32) -> Option<&PowerProfile> {
        self.profiles.iter().find(|p| p.op == op)
    }

    /// 能效排名（整数稳定序：能耗/千调用升序，同值按算子号升序）。
    /// 只含有画像的算子。
    pub fn ranked_by_efficiency(&self) -> Vec<(u32, u64)> {
        let mut out: Vec<(u32, u64)> = self
            .profiles
            .iter()
            .filter_map(|p| p.energy_per_kilo_calls().map(|e| (p.op, e)))
            .collect();
        out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        out
    }

    /// 画像数。
    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    /// 封册。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 9);
        }
        if self.profiles.is_empty() {
            return Err(SEG_BASE + 10);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for p in &self.profiles {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ p.op as u64;
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ p.uw_tick_sum;
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ p.invocations;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for PowerBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 能耗预算红线（判据"能效对放置决策"的立案线）：
/// 单算子千次调用能耗超过此线即立案复审。
pub const ENERGY_BUDGET_PER_KILO: u64 = 400_000;

/// 相对能效比（判据"能效比标尺"的双算子面）：b 相对 a 的能效比
/// 千分比（>1000 表示 b 每千调用更省能）。任一画像缺失返回 None。
pub fn relative_efficiency_permille(
    a: &PowerProfile,
    b: &PowerProfile,
) -> Option<u64> {
    let ea = a.energy_per_kilo_calls()?;
    let eb = b.energy_per_kilo_calls()?;
    if ea == 0 {
        return None;
    }
    Some(eb * 1000 / ea)
}

impl PowerBook {
    /// 超标画像清单（判据"决策联动"的立案面）：能耗超红线的算子。
    pub fn over_budget_ops(&self) -> Vec<(u32, u64)> {
        self.profiles
            .iter()
            .filter_map(|p| {
                p.energy_per_kilo_calls()
                    .filter(|e| *e > ENERGY_BUDGET_PER_KILO)
                    .map(|e| (p.op, e))
            })
            .collect()
    }

    /// 域总能耗账（判据"数据入册"的聚合面）：全部画像的
    /// uw_tick_sum 合计（µW·tick 口径）。
    pub fn total_uw_tick(&self) -> u64 {
        self.profiles.iter().map(|p| p.uw_tick_sum).sum()
    }

    /// 全域平均能效（千调用能耗均值；空册 None）。
    pub fn avg_energy_per_kilo(&self) -> Option<u64> {
        if self.profiles.is_empty() {
            return None;
        }
        let sum: u64 = self
            .profiles
            .iter()
            .filter_map(|p| p.energy_per_kilo_calls())
            .sum();
        let n = self
            .profiles
            .iter()
            .filter(|p| p.energy_per_kilo_calls().is_some())
            .count();
        if n == 0 {
            None
        } else {
            Some(sum / n as u64)
        }
    }

    /// 最耗能算子（整数稳定序：能耗降序，同值算子号升序）。
    pub fn worst_op(&self) -> Option<(u32, u64)> {
        let mut rows: Vec<(u32, u64)> = self
            .profiles
            .iter()
            .filter_map(|p| p.energy_per_kilo_calls().map(|e| (p.op, e)))
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        rows.first().copied()
    }
}

/// 采样驱动（判据"测量依赖功耗采样"的接线面）：把采样器序列灌进
/// 会话直至窗满或采样耗尽；返回实际灌入的采样点数。
pub fn drive_session(
    session: &mut MeasureSession,
    sampler: &mut PowerSampler,
    budget: usize,
) -> usize {
    let mut fed = 0usize;
    while fed < budget {
        match sampler.next() {
            Some(uw) => {
                if session.sample(uw).is_err() {
                    break;
                }
                fed += 1;
            }
            None => break,
        }
    }
    fed
}
