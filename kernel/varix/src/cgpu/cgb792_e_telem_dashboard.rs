//! CGPU-F0792 · E 域遥测看板数据模型（目标 320 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0792，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域遥测看板数据模型——看板行（DashboardRow）的
//!   数据结构、指标序列（定容环形）、整数聚合口径（min/max/均值）
//!   与行间守恒校验；为上层渲染提供纯数据，不涉及任何显示。
//!
//! 设计纪律：
//! - 指标序列定容：SERIES_CAP=64 个采样点，超容覆盖最旧并计数，
//!   守恒恒等式：total_samples == held + overwritten。
//! - 聚合口径闭集三种：Min / Max / Avg（整数均值向下取整）；
//!   空序列聚合即 Err。
//! - 看板行闭集三种：Gauge（即时值）/ Counter（累计值）/
//!   Rate（速率，分母为固定窗口）；行内持有自己的序列。
//! - 看板守恒：Σ各行 total_samples == 看板总采样数。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6E40 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x6E40，批内 CGPU-F0792 独占）。
pub const SEG_BASE: u16 = 0x6E40;

/// 行名重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 采样口径非法（空行名或Counter 采样值回退）。
pub const E_SAMPLE: u16 = SEG_BASE + 2;
/// 看板行数已满。
pub const E_CAP: u16 = SEG_BASE + 3;
/// 序列为空即聚合。
pub const E_EMPTY: u16 = SEG_BASE + 4;
/// 看板守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 5;

/// 序列容量。
pub const SERIES_CAP: usize = 64;
/// 看板行数上限。
pub const BOARD_ROWS: usize = 16;
/// 速率窗口（采样点个数）。
pub const RATE_WINDOW: u32 = 8;

/// 聚合口径（闭集三种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agg {
    Min,
    Max,
    Avg,
}

/// 指标序列（定容环形）。
#[derive(Clone, Debug)]
pub struct MetricSeries {
    ring: Vec<u64>,
    w: usize,
    held: usize,
    overwritten: u64,
    total: u64,
}

impl MetricSeries {
    pub fn new() -> Self {
        MetricSeries {
            ring: Vec::new(),
            w: 0,
            held: 0,
            overwritten: 0,
            total: 0,
        }
    }
    /// 采样入列：满则覆盖最旧。
    pub fn push(&mut self, v: u64) {
        if self.held < SERIES_CAP {
            self.ring.push(v);
            self.held += 1;
        } else {
            self.ring[self.w] = v;
            self.overwritten += 1;
        }
        self.w = (self.w + 1) % SERIES_CAP;
        self.total += 1;
    }
    /// 守恒审计：total == held + overwritten。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.held as u64 + self.overwritten {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 视图（旧→新）。
    pub fn view(&self) -> Vec<u64> {
        let start = if self.held < SERIES_CAP { 0 } else { self.w };
        let mut out = Vec::with_capacity(self.held);
        for i in 0..self.held {
            let idx = (start + i) % SERIES_CAP;
            if idx < self.ring.len() {
                out.push(self.ring[idx]);
            }
        }
        out
    }
    pub fn held(&self) -> usize {
        self.held
    }
    pub fn total(&self) -> u64 {
        self.total
    }
    /// 整数聚合。
    pub fn aggregate(&self, a: Agg) -> Result<u64, u16> {
        let v = self.view();
        if v.is_empty() {
            return Err(E_EMPTY);
        }
        Ok(match a {
            Agg::Min => *v.iter().min().ok_or(E_EMPTY)?,
            Agg::Max => *v.iter().max().ok_or(E_EMPTY)?,
            Agg::Avg => v.iter().sum::<u64>() / v.len() as u64,
        })
    }
    /// 速率：最近 RATE_WINDOW 点的首尾差（点数不足按现有窗口差）。
    pub fn rate(&self) -> Result<u64, u16> {
        let v = self.view();
        if v.len() < 2 {
            return Err(E_EMPTY);
        }
        let n = v.len().min(RATE_WINDOW as usize);
        let tail = v[v.len() - 1];
        let head = v[v.len() - n];
        Ok(tail.saturating_sub(head))
    }
}

/// 看板行类别（闭集三种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Gauge,
    Counter,
    Rate,
}

/// 看板行。
#[derive(Clone, Debug)]
pub struct DashboardRow {
    /// 行名（看板内唯一）。
    pub name: String,
    /// 行类别。
    pub kind: RowKind,
    /// 指标序列。
    pub series: MetricSeries,
}

impl DashboardRow {
    pub fn new(name: &str, kind: RowKind) -> Self {
        DashboardRow {
            name: name.to_string(),
            kind,
            series: MetricSeries::new(),
        }
    }
    /// 采样：Counter 类要求单调不减（回退即口径非法）。
    pub fn sample(&mut self, v: u64) -> Result<(), u16> {
        if self.kind == RowKind::Counter {
            if let Some(last) = self.series.view().last() {
                if v < *last {
                    return Err(E_SAMPLE);
                }
            }
        }
        self.series.push(v);
        Ok(())
    }
}

/// 遥测看板。
#[derive(Clone, Debug, Default)]
pub struct Dashboard {
    rows: Vec<DashboardRow>,
    total_samples: u64,
}

impl Dashboard {
    pub fn new() -> Self {
        Dashboard {
            rows: Vec::new(),
            total_samples: 0,
        }
    }
    /// 加行（行数定容 + 行名唯一）。
    pub fn add_row(&mut self, name: &str, kind: RowKind) -> Result<(), u16> {
        if self.rows.len() >= BOARD_ROWS {
            return Err(E_CAP);
        }
        if name.is_empty() || self.rows.iter().any(|r| r.name == name) {
            return Err(E_DUP);
        }
        self.rows.push(DashboardRow::new(name, kind));
        Ok(())
    }
    /// 定位行。
    pub fn row_mut(&mut self, name: &str) -> Result<&mut DashboardRow, u16> {
        self.rows.iter_mut().find(|r| r.name == name).ok_or(E_EMPTY)
    }
    /// 采样入口（统一记账）。
    pub fn sample(&mut self, name: &str, v: u64) -> Result<(), u16> {
        self.row_mut(name)?.sample(v)?;
        self.total_samples += 1;
        Ok(())
    }
    /// 看板守恒：Σ行 total == 看板 total。
    pub fn audit(&self) -> Result<(), u16> {
        let sum: u64 = self.rows.iter().map(|r| r.series.total()).sum();
        if sum != self.total_samples {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

/// 自例程：主路径（Gauge/Counter/Rate 全覆盖）+ 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut b = Dashboard::new();
    b.add_row("fps", RowKind::Gauge)?;
    b.add_row("frames", RowKind::Counter)?;
    b.add_row("mbps", RowKind::Rate)?;
    if b.add_row("fps", RowKind::Gauge) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if b.add_row("", RowKind::Gauge) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 主路径 1：Gauge 采样与聚合。
    for v in [60u64, 58, 62, 61].iter() {
        b.sample("fps", *v)?;
    }
    b.audit()?;
    {
        let r = b.row_mut("fps")?;
        if r.series.aggregate(Agg::Min)? != 58
            || r.series.aggregate(Agg::Max)? != 62
            || r.series.aggregate(Agg::Avg)? != 60
        {
            return Err(E_SAMPLE);
        }
    }
    // 主路径 2：Counter 单调。
    for v in [10u64, 20, 35, 50].iter() {
        b.sample("frames", *v)?;
    }
    // Counter 回退拒绝。
    if b.sample("frames", 40) != Err(E_SAMPLE) {
        return Err(E_SAMPLE);
    }
    // 主路径 3：Rate 窗口差。
    for v in [100u64, 200, 400, 800].iter() {
        b.sample("mbps", *v)?;
    }
    b.audit()?;
    {
        let r = b.row_mut("mbps")?;
        // 最近 8 点取全部 4 点：800-100=700。
        if r.series.rate()? != 700 {
            return Err(E_SAMPLE);
        }
    }
    // 定容覆盖：单行灌 70 个采样进 64 容序列。
    for i in 0..70u64 {
        b.sample("fps", 1000 + i)?;
    }
    {
        let r = b.row_mut("fps")?;
        r.series.audit()?;
        if r.series.held() != SERIES_CAP || r.series.overwritten != 10 {
            return Err(E_LEDGER);
        }
        // 覆盖后最旧值是 1000+60。
        let v = r.series.view();
        if v[0] != 1060 || v[63] != 1069 {
            return Err(E_SAMPLE);
        }
    }
    b.audit()?;
    // 错误路径 1：空序列聚合。
    let s0 = MetricSeries::new();
    if s0.aggregate(Agg::Avg) != Err(E_EMPTY) || s0.rate() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 2：未知行。
    if b.sample("nope", 1) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 3：行数定容。
    let mut b2 = Dashboard::new();
    for i in 0..BOARD_ROWS {
        let name = alloc::format!("r{}", i);
        b2.add_row(&name, RowKind::Gauge)?;
    }
    if b2.add_row("overflow", RowKind::Gauge) != Err(E_CAP) {
        return Err(E_CAP);
    }
    if b2.row_count() != BOARD_ROWS {
        return Err(E_CAP);
    }
    // Rate 行不足 2 点。
    let mut b3 = Dashboard::new();
    b3.add_row("thin", RowKind::Rate)?;
    if b3.sample("thin", 5) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 汇总守恒：全板再审一次。
    b.audit()?;
    Ok(0)
}
