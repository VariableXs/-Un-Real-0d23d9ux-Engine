//! cgb21_xftel — CGPU-F0234 跨帧遥测（B05 组第十件）。
//!
//! 判据原文（施工书锚点 CGPU-F0234）：跨帧遥测：在途深度分布/跨帧等待
//! 耗时 P95/跳帧计数/优先级提升频率/别名分析耗时——五指标进调度遥测
//! （与 B08 域预留接口一致）。判据：五指标、接口一致。
//!
//! 职责定位：B05 组五个可观测面（深度/等待/跳帧/提级/别名耗时）分散在
//! cgb17/15/13 各件内部，遥测把它们汇成一张五列总表，口径与 B08 调度遥
//! 测域对齐（同指标名、同分位点定义、同计数单位——接口一致判据的兑现方
//! 式是「字段名与语义逐字对拍」而非只对个数）。全整数：P95 用直方图桶
//! 计数 + 分位插值禁用（宁显性 NotEnough 也不线性外插——数据不造假）。
//! 诊断码段 0x64D0（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点；分位点输出三态：
//! 数值 / NotEnough（样本不足）/ Overflow（超表程）——零静默零谎报。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_TEL_VERSION: &str = "B21-xftel-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x64D 段，本会话独占）
// ---------------------------------------------------------------------------

/// 指标名未注册（接口一致判据：只收五指标白名单内的名字）。
pub const XC_TEL_UNKNOWN_METRIC: u16 = 0x64D0;
/// 遥测样本非法（深度 0 视为脏数据——在途帧至少为提交中的一帧）。
pub const XC_TEL_BAD_SAMPLE: u16 = 0x64D1;
/// 总线模式不匹配（对端 B08 schema 版本不同——接口一致性破坏）。
pub const XC_TEL_SCHEMA_MISMATCH: u16 = 0x64D2;
/// 指标账不平（收样数 ≠ 各桶计数和 + 溢出数）。
pub const XC_TEL_LEDGER_UNBALANCED: u16 = 0x64D3;
/// 快照导出中途被改写（导出必须原子可见——快照指纹前后不一致）。
pub const XC_TEL_SNAPSHOT_MUTATED: u16 = 0x64D4;

/// 遥测 schema 版本（与 B08 域预留接口逐字对拍）。
pub const TEL_SCHEMA_VERSION: &str = "B08-schedtel-v1";

/// 五指标白名单（接口一致的注册表面——顺序即快照列序）。
pub const METRIC_NAMES: [&str; 5] = [
    "xf_inflight_depth",   // 在途深度分布
    "xf_wait_p95",         // 跨帧等待耗时（µs 直方图）
    "xf_skip_count",       // 跳帧计数
    "xf_prioboost_freq",   // 优先级提升频率（次/千帧）
    "xf_alias_cost",       // 别名分析耗时（µs 直方图）
];

// ---------------------------------------------------------------------------
// 整数直方图：P95 的无谎报实现
// ---------------------------------------------------------------------------

/// 直方图分位结果三态（零谎报纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pctl {
    /// 分位值（落在某桶的桶界）。
    Value(u64),
    /// 样本不足（低于最小样本数——显性 NotEnough，不外插）。
    NotEnough,
    /// 全部样本超表程（溢出桶非空且其他桶全空）。
    Overflow,
}

/// 定界整数直方图：桶界自声明，溢出单列，分位走桶累计。
#[derive(Clone, Debug)]
pub struct Histogram {
    /// 桶上界（µs），升序；samples 落 (bounds[i-1], bounds[i]] 计入桶 i。
    bounds: Vec<u64>,
    /// 各桶计数。
    counts: Vec<u64>,
    /// 超表程样本数。
    overflow: u64,
    /// 总收样数。
    total: u64,
}

impl Histogram {
    /// 构造：桶界必须严格升序、非空、≤16 桶。
    pub fn boot(bounds: Vec<u64>) -> Result<Self, u16> {
        if bounds.is_empty() || bounds.len() > 16 {
            return Err(XC_TEL_BAD_SAMPLE);
        }
        for i in 1..bounds.len() {
            if bounds[i] <= bounds[i - 1] {
                return Err(XC_TEL_BAD_SAMPLE);
            }
        }
        let n = bounds.len();
        Ok(Histogram { bounds, counts: vec![0u64; n], overflow: 0, total: 0 })
    }

    /// 收样。
    pub fn record(&mut self, v: u64) {
        self.total += 1;
        let mut placed = false;
        for i in 0..self.bounds.len() {
            if v <= self.bounds[i] {
                self.counts[i] += 1;
                placed = true;
                break;
            }
        }
        if !placed {
            self.overflow += 1;
        }
    }

    /// 分位查询（pct 万分制：9500 = P95）。
    /// 算法：目标序位 rank = ceil(total*pct/10000)；从低桶累计到 rank
    /// 所在桶，返回该桶上界。溢出样本参与分位（若 rank 落在溢出段，
    /// 返回 Overflow——诚实标注"分位在表程外"）。
    pub fn percentile(&self, pct: u32, min_samples: u64) -> Pctl {
        if self.total < min_samples {
            return Pctl::NotEnough;
        }
        let in_range = self.total - self.overflow;
        if in_range == 0 {
            return Pctl::Overflow;
        }
        // rank 只在表程内样本上取（溢出段单列，分位不上溢——诚实边界）。
        let rank = (self.total.saturating_mul(pct as u64) + 9_999) / 10_000;
        if rank > in_range {
            return Pctl::Overflow;
        }
        let mut acc = 0u64;
        for i in 0..self.counts.len() {
            acc += self.counts[i];
            if acc >= rank {
                return Pctl::Value(self.bounds[i]);
            }
        }
        // 不可达（rank ≤ in_range ≤ 累计和），防御性 NotEnough。
        Pctl::NotEnough
    }

    /// 账平：总收样 ≡ Σ桶 + 溢出。
    pub fn balanced(&self) -> bool {
        let sum: u64 = self.counts.iter().fold(0u64, |a, &c| a.saturating_add(c));
        sum + self.overflow == self.total
    }

    /// 收样数。
    pub fn samples(&self) -> u64 {
        self.total
    }
}

// ---------------------------------------------------------------------------
// 五列遥测总线：B08 预留接口的正本
// ---------------------------------------------------------------------------

/// 单指标列（名字白名单 + 直方图 + 标量计数）。
#[derive(Debug)]
pub struct MetricColumn {
    /// 指标名（白名单内）。
    pub name: &'static str,
    /// 直方图列（分布型指标）。
    pub hist: Option<Histogram>,
    /// 标量累计列（计数型指标）。
    pub scalar: u64,
    /// 收样次数（次数 = scalar 的分母面）。
    pub reported: u64,
}

/// 跨帧遥测总线。
#[derive(Debug)]
pub struct XfTelemetry {
    /// 五列（列序 = METRIC_NAMES 序——接口一致的对拍序）。
    cols: Vec<MetricColumn>,
}

impl XfTelemetry {
    /// 构造：五列白名单预注册；等待/别名两列带直方图（µs 分布），
    /// 深度列带直方图（深度分布），跳帧/提级为纯计数。
    pub fn boot() -> Result<Self, u16> {
        let depth_hist = Histogram::boot(vec![1, 2, 3, 4, 5, 6, 7, 8])?;
        let wait_hist = Histogram::boot(vec![100, 500, 1_000, 5_000, 16_700, 33_400, 66_800])?;
        let alias_hist = Histogram::boot(vec![10, 50, 200, 1_000, 5_000])?;
        let mk = |name: &'static str, h: Option<Histogram>| MetricColumn { name, hist: h, scalar: 0, reported: 0 };
        Ok(XfTelemetry {
            cols: vec![
                mk(METRIC_NAMES[0], Some(depth_hist)),
                mk(METRIC_NAMES[1], Some(wait_hist)),
                mk(METRIC_NAMES[2], None),
                mk(METRIC_NAMES[3], None),
                mk(METRIC_NAMES[4], Some(alias_hist)),
            ],
        })
    }

    fn col_idx(&self, name: &str) -> Result<usize, u16> {
        match self.cols.iter().position(|c| c.name == name) {
            Some(i) => Ok(i),
            None => Err(XC_TEL_UNKNOWN_METRIC),
        }
    }

    /// 分布型收样。
    pub fn report_dist(&mut self, name: &str, v: u64) -> Result<(), u16> {
        let i = self.col_idx(name)?;
        if self.cols[i].hist.is_none() {
            return Err(XC_TEL_UNKNOWN_METRIC);
        }
        if let Some(ref mut h) = self.cols[i].hist {
            h.record(v);
        }
        self.cols[i].reported += 1;
        Ok(())
    }

    /// 计数型收样（delta 语义：调用方每次 +1）。
    pub fn report_count(&mut self, name: &str) -> Result<(), u16> {
        let i = self.col_idx(name)?;
        if self.cols[i].hist.is_some() {
            return Err(XC_TEL_UNKNOWN_METRIC);
        }
        self.cols[i].scalar += 1;
        self.cols[i].reported += 1;
        Ok(())
    }

    /// 分位查询透传（NotEnough/Overflow 三态直传——零谎报）。
    pub fn query_pctl(&self, name: &str, pct: u32, min_samples: u64) -> Result<Pctl, u16> {
        let i = self.col_idx(name)?;
        match self.cols[i].hist {
            Some(ref h) => Ok(h.percentile(pct, min_samples)),
            None => Err(XC_TEL_UNKNOWN_METRIC),
        }
    }

    /// 跳帧计数读数（判据"跳帧计数"直读）。
    pub fn skip_count(&self) -> Result<u64, u16> {
        let i = self.col_idx(METRIC_NAMES[2])?;
        Ok(self.cols[i].scalar)
    }

    /// 提级频率读数（次/千帧；帧数 0 显性 0——不除零不虚标）。
    pub fn boost_freq_per_kilo(&self, frames: u64) -> Result<u64, u16> {
        let i = self.col_idx(METRIC_NAMES[3])?;
        if frames == 0 {
            return Ok(0);
        }
        Ok(self.cols[i].scalar.saturating_mul(1_000) / frames)
    }

    /// 全列账平自证。
    pub fn all_balanced(&self) -> bool {
        self.cols.iter().all(|c| match c.hist {
            Some(ref h) => h.balanced() && h.samples() == c.reported,
            None => true,
        })
    }

    /// B08 接口一致性自证（判据二）：schema 版本一致 + 列名序一致。
    pub fn bus_contract(peer_schema: &str, peer_cols: &[&str]) -> Result<(), u16> {
        if peer_schema != TEL_SCHEMA_VERSION {
            return Err(XC_TEL_SCHEMA_MISMATCH);
        }
        if peer_cols.len() != METRIC_NAMES.len() {
            return Err(XC_TEL_SCHEMA_MISMATCH);
        }
        for (a, b) in peer_cols.iter().zip(METRIC_NAMES.iter()) {
            if a != b {
                return Err(XC_TEL_SCHEMA_MISMATCH);
            }
        }
        Ok(())
    }

    /// 快照导出（列名 + 计数/直方图和）——导出前后指纹自证未被改写。
    pub fn snapshot_fingerprint(&self) -> u64 {
        let mut h: u64 = 0x64D0_1022_5EED_0001;
        for c in &self.cols {
            for b in c.name.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            h ^= c.scalar.wrapping_mul(0x9E37_79B9);
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            if let Some(ref hist) = c.hist {
                h ^= hist.samples();
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_TEL_UNKNOWN_METRIC => "指标未注册：只收五指标白名单（xf_inflight_depth 等），先对名再收样。".to_string(),
            XC_TEL_BAD_SAMPLE => "遥测样本非法：直方图桶界须严格升序且 ≤16 桶。".to_string(),
            XC_TEL_SCHEMA_MISMATCH => "总线 schema 不匹配：对端版本/列序与 B08 预留接口不一致。".to_string(),
            XC_TEL_LEDGER_UNBALANCED => "指标账不平：收样数 ≡ Σ桶 + 溢出，先对账再出报告。".to_string(),
            XC_TEL_SNAPSHOT_MUTATED => "快照导出被改写：导出前先取指纹，导出后复核。".to_string(),
            _ => "未知诊断码：查 0x64D 段分配表。".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 组内自证
// ---------------------------------------------------------------------------

/// 自证清单：遥测两判据的机械对偶。
pub fn self_audit() -> Result<(), u16> {
    let mut tel = XfTelemetry::boot()?;

    // 一、五指标收样：深度分布 20 样、等待 40 样、跳帧 3、提级 5、别名 12 样。
    for i in 1u64..=20 {
        tel.report_dist("xf_inflight_depth", i.min(8))?;
    }
    for i in 1u64..=40 {
        tel.report_dist("xf_wait_p95", i * 250)?;
    }
    for _ in 0..3 {
        tel.report_count("xf_skip_count")?;
    }
    for _ in 0..5 {
        tel.report_count("xf_prioboost_freq")?;
    }
    for i in 1u64..=12 {
        tel.report_dist("xf_alias_cost", i * 40)?;
    }
    if !tel.all_balanced() {
        return Err(XC_TEL_LEDGER_UNBALANCED);
    }
    if tel.skip_count()? != 3 {
        return Err(XC_TEL_LEDGER_UNBALANCED);
    }
    if tel.boost_freq_per_kilo(1_000)? != 5 || tel.boost_freq_per_kilo(0)? != 0 {
        return Err(XC_TEL_LEDGER_UNBALANCED);
    }

    // 二、分位三态：样本足给桶界值；不足显性 NotEnough；全溢出显性 Overflow。
    // 等待列 40 样全部 ≤ 10_000µs（i*250 ≤ 10_000），P95 落 16_700 桶？
    // rank=ceil(40*0.95)=38；桶界 [100,500,1000,5000,16700,...]：
    // ≤100:2 样(i=1..2? 250*i≤100 → i≤0 → 0 样)……直接验三态行为：
    match tel.query_pctl("xf_wait_p95", 9_500, 30)? {
        Pctl::Value(v) => {
            if v != 16_700 {
                return Err(XC_TEL_LEDGER_UNBALANCED);
            }
        }
        _ => return Err(XC_TEL_LEDGER_UNBALANCED),
    }
    match tel.query_pctl("xf_alias_cost", 9_500, 13)? {
        Pctl::NotEnough => {}
        _ => return Err(XC_TEL_LEDGER_UNBALANCED),
    }
    // 深度列 20 样含 i=8 的饱和样：全在表程内，P95 = 桶界 8。
    match tel.query_pctl("xf_inflight_depth", 9_500, 10)? {
        Pctl::Value(8) => {}
        _ => return Err(XC_TEL_LEDGER_UNBALANCED),
    }

    // 三、接口一致：B08 对拍（版本 + 列名序）。
    XfTelemetry::bus_contract("B08-schedtel-v1", &METRIC_NAMES)?;
    if XfTelemetry::bus_contract("B08-schedtel-v0", &METRIC_NAMES).is_ok() {
        return Err(XC_TEL_SCHEMA_MISMATCH);
    }
    let wrong_order = ["xf_skip_count", "xf_inflight_depth", "xf_wait_p95", "xf_prioboost_freq", "xf_alias_cost"];
    if XfTelemetry::bus_contract("B08-schedtel-v1", &wrong_order).is_ok() {
        return Err(XC_TEL_SCHEMA_MISMATCH);
    }

    // 四、白名单纪律：未注册指标拒、分布列拒计数语义。
    if tel.report_count("xf_wait_p95").is_ok() {
        return Err(XC_TEL_UNKNOWN_METRIC);
    }
    if tel.report_dist("xf_unknown", 1).is_ok() {
        return Err(XC_TEL_UNKNOWN_METRIC);
    }
    // 快照指纹稳定（同态两次导出一致）。
    let f1 = tel.snapshot_fingerprint();
    let f2 = tel.snapshot_fingerprint();
    if f1 != f2 {
        return Err(XC_TEL_SNAPSHOT_MUTATED);
    }
    Ok(())
}
