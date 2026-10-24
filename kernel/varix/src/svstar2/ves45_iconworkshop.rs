//! VE-F3645 · 图标工坊（VE-S 域 · 批次 R03 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3645`
//!
//! **职责定位（锚点原文）**：图标创作——
//! - **网格系统**：标准网格（24/48/96 网格 + 关键线）（网格纪律红线：
//!   图标出格 = 视觉不齐 → 网格吸附 + 越格警告）；
//! - **多尺寸导出**：一套图标多尺寸导出（16-512 全档）；
//! - **风格一致性**：线宽/圆角/透视一致检查（套一致红线：套内不一致 = 设计崩；
//!   一致性断言 = 线宽方差门）；
//! - **模板与批量**：套模板批量生成。
//!
//! **判据（锚点原文六条）**：网格纪律、全档导出、套一致方差门、
//! 模板批量、精度复用、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 网格越格→警告+吸附（红线实测） | [`I45Code::OFF_GRID`] |
//! | 尺寸缺档→补（复述红线） | [`I45Code::SIZE_GAP`] |
//! | 套不一致→断言（红线实测——方差超标列出清单） | [`I45Code::SET_INCONSISTENT`] |
//! | 批量失真→抽查（复述） | [`I45Code::BATCH_DRIFT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3645 诊断码段（独占 0x5Dxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct I45Code(pub u16);

impl I45Code {
    /// 越格（网格纪律红线）。
    pub const OFF_GRID: I45Code = I45Code(0x5D01);
    /// 导出尺寸缺档。
    pub const SIZE_GAP: I45Code = I45Code(0x5D02);
    /// 套内不一致（方差门破）。
    pub const SET_INCONSISTENT: I45Code = I45Code(0x5D03);
    /// 批量失真。
    pub const BATCH_DRIFT: I45Code = I45Code(0x5D04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            I45Code::OFF_GRID => "图标越格（网格纪律红线）。建议：吸附到网格并警告。",
            I45Code::SIZE_GAP => "导出尺寸缺档。建议：补齐 16-512 全档。",
            I45Code::SET_INCONSISTENT => "套内线宽方差超标（红线实测）。建议：统一线宽后重出。",
            I45Code::BATCH_DRIFT => "批量产物失真。建议：抽查比对模板。",
            _ => "未知诊断码。",
        }
    }
}

// ── 网格系统 ────────────────────────────────────────────────────────

/// 标准网格档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grid {
    /// 24 网格。
    G24,
    /// 48 网格。
    G48,
    /// 96 网格。
    G96,
}

impl Grid {
    pub fn unit(self) -> u32 {
        match self {
            Grid::G24 => 24,
            Grid::G48 => 48,
            Grid::G96 => 96,
        }
    }

    /// 吸附到最近网格线。
    pub fn snap(self, v: u32) -> u32 {
        let u = self.unit();
        let half = u / 2;
        if v % u <= half {
            v - (v % u)
        } else {
            v + (u - v % u)
        }
    }

    /// 是否贴线。
    pub fn on_line(self, v: u32) -> bool {
        v % self.unit() == 0
    }
}

// ── 导出档（16-512 全档） ───────────────────────────────────────────

/// 全档尺寸表。
pub const EXPORT_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 512];

// ── 套一致性（线宽方差门） ──────────────────────────────────────────

/// 套内图标度量。
#[derive(Clone, Copy)]
pub struct IconMetric {
    pub id: u32,
    /// 线宽（像素×10，整数化）。
    pub stroke_x10: u32,
    /// 圆角半径。
    pub radius: u32,
}

/// 线宽方差门。
pub struct VarianceGate {
    /// 允许的最大线宽偏差（×10）。
    pub max_dev_x10: u32,
    pub breaks: u32,
}

impl VarianceGate {
    /// 线宽均值。
    pub fn mean(metrics: &[IconMetric]) -> u32 {
        if metrics.is_empty() {
            return 0;
        }
        (metrics.iter().map(|m| m.stroke_x10 as u64).sum::<u64>() / metrics.len() as u64) as u32
    }

    /// 超差清单（返回超差图标 id）。
    pub fn outliers(&self, metrics: &[IconMetric]) -> Vec<u32> {
        let mean = Self::mean(metrics);
        metrics
            .iter()
            .filter(|m| {
                let d = if m.stroke_x10 > mean { m.stroke_x10 - mean } else { mean - m.stroke_x10 };
                d > self.max_dev_x10
            })
            .map(|m| m.id)
            .collect()
    }

    /// 执法：有超差即破门。
    pub fn check(&mut self, metrics: &[IconMetric]) -> bool {
        if self.outliers(metrics).is_empty() {
            true
        } else {
            self.breaks += 1;
            false
        }
    }
}

// ── 图标工坊 ────────────────────────────────────────────────────────

/// 图标工坊：网格 + 导出 + 方差 + 模板批量。
pub struct IconWorkshop {
    grid: Grid,
    exports: Vec<u32>,
    metrics: Vec<IconMetric>,
    gate: VarianceGate,
    offgrid_hits: u32,
    last_code: I45Code,
    has_error: bool,
}

impl IconWorkshop {
    pub fn new(grid: Grid, max_dev_x10: u32) -> Self {
        Self {
            grid,
            exports: Vec::new(),
            metrics: Vec::new(),
            gate: VarianceGate { max_dev_x10, breaks: 0 },
            offgrid_hits: 0,
            last_code: I45Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> I45Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: I45Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 落点校验：越格警告 + 吸附（返回吸附后坐标）。
    pub fn place(&mut self, x: u32, y: u32) -> Result<(u32, u32), I45Code> {
        if self.grid.on_line(x) && self.grid.on_line(y) {
            self.last_code = I45Code(0);
            self.has_error = false;
            return Ok((x, y));
        }
        self.offgrid_hits += 1;
        self.fail(I45Code::OFF_GRID);
        Ok((self.grid.snap(x), self.grid.snap(y)))
    }

    pub fn offgrid_hits(&self) -> u32 {
        self.offgrid_hits
    }

    /// 登记导出档。
    pub fn export_size(&mut self, px: u32) -> Result<usize, I45Code> {
        if !EXPORT_SIZES.contains(&px) {
            self.fail(I45Code::SIZE_GAP);
            return Err(I45Code::SIZE_GAP);
        }
        if !self.exports.contains(&px) {
            self.exports.push(px);
        }
        self.last_code = I45Code(0);
        self.has_error = false;
        Ok(self.exports.len())
    }

    /// 全档覆盖审计。
    pub fn export_complete(&self) -> bool {
        EXPORT_SIZES.iter().all(|s| self.exports.contains(s))
    }

    /// 缺档清单。
    pub fn missing_sizes(&self) -> Vec<u32> {
        EXPORT_SIZES.iter().copied().filter(|s| !self.exports.contains(s)).collect()
    }

    /// 登记套内图标度量。
    pub fn add_metric(&mut self, id: u32, stroke_x10: u32, radius: u32) -> usize {
        self.metrics.push(IconMetric { id, stroke_x10, radius });
        self.metrics.len()
    }

    /// 套一致执法（方差门）。
    pub fn check_set(&mut self) -> Result<bool, I45Code> {
        if self.gate.check(&self.metrics) {
            self.last_code = I45Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(I45Code::SET_INCONSISTENT);
            Err(I45Code::SET_INCONSISTENT)
        }
    }

    /// 超差清单（红线要求列出清单）。
    pub fn outlier_list(&self) -> Vec<u32> {
        self.gate.outliers(&self.metrics)
    }

    /// 模板批量：n 个副本，抽查偏移超差判失真。
    pub fn batch_from_template(&mut self, n: u32, template_stroke_x10: u32, drift_x10: u32) -> Result<u32, I45Code> {
        let mut produced = 0u32;
        for i in 0..n {
            let stroke = template_stroke_x10 + if i == n / 2 && drift_x10 > self.gate.max_dev_x10 { drift_x10 } else { 0 };
            self.metrics.push(IconMetric { id: 1_000 + i, stroke_x10: stroke, radius: 4 });
            produced += 1;
        }
        if self.gate.check(&self.metrics) {
            self.last_code = I45Code(0);
            self.has_error = false;
            Ok(produced)
        } else {
            self.fail(I45Code::BATCH_DRIFT);
            Err(I45Code::BATCH_DRIFT)
        }
    }

    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }

    pub fn export_count(&self) -> usize {
        self.exports.len()
    }

    pub fn gate_breaks(&self) -> u32 {
        self.gate.breaks
    }

    /// 网格档。
    pub fn grid(&self) -> Grid {
        self.grid
    }

    /// 精度复用总评：全档导出 + 套无超差。
    pub fn precision_green(&self) -> bool {
        self.export_complete() && self.outlier_list().is_empty()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves45_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves45_iconworkshop");
    {
        let w = IconWorkshop::new(Grid::G48, 50);
        chk(&mut set, "C3645-01", w.metric_count() == 0 && w.export_count() == 0, "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G24, 50);
        let r = w.place(48, 24);
        chk(&mut set, "C3645-02", r == Ok((48, 24)) && w.offgrid_hits() == 0, "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G24, 50);
        let r = w.place(50, 27);
        chk(&mut set, "C3645-03", r == Ok((48, 24)) && w.offgrid_hits() == 1, "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G48, 50);
        let r = w.export_size(16);
        chk(&mut set, "C3645-04", r == Ok(1), "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G48, 50);
        let r = w.export_size(99);
        chk(&mut set, "C3645-05", r.err() == Some(I45Code::SIZE_GAP), "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G48, 50);
        for s in EXPORT_SIZES.iter() {
            w.export_size(*s);
        }
        chk(&mut set, "C3645-06", w.export_complete() && w.missing_sizes().is_empty(), "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G48, 50);
        w.add_metric(1, 40, 4);
        w.add_metric(2, 40, 4);
        w.add_metric(3, 42, 4);
        let r = w.check_set();
        chk(&mut set, "C3645-07", r == Ok(true), "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G48, 20);
        w.add_metric(1, 20, 4);
        w.add_metric(2, 80, 4);
        let r = w.check_set();
        chk(&mut set, "C3645-08", r.err() == Some(I45Code::SET_INCONSISTENT) && !w.outlier_list().is_empty(), "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G48, 50);
        w.add_metric(1, 30, 4);
        w.add_metric(2, 90, 4);
        w.check_set().ok();
        let list = w.outlier_list();
        chk(&mut set, "C3645-09", list.contains(&1) || list.contains(&2), "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G48, 50);
        let r = w.batch_from_template(4, 40, 0);
        chk(&mut set, "C3645-10", r == Ok(4) && w.metric_count() == 4, "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G48, 50);
        let r = w.batch_from_template(4, 40, 200);
        chk(&mut set, "C3645-11", r.err() == Some(I45Code::BATCH_DRIFT), "err");
    }
    {
        let mut w = IconWorkshop::new(Grid::G96, 50);
        for s in EXPORT_SIZES.iter() {
            w.export_size(*s);
        }
        w.add_metric(1, 40, 4);
        chk(&mut set, "C3645-12", w.precision_green(), "err");
    }
    set
}
