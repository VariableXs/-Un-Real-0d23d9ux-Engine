//! UNX-F50028 布局预览账（预览渲染与实际输出一致性抽检）（0x42fa0 段）
//!
//! 锚点：UNX-F50028-J1 抽检 ×10 预览=输出像素一致（模拟器档）；判据锚定 体验五 视觉品质。
//!
//! 数据段：`SEG_BASE = 0x42fa0`，诊断码闭集 `0x42fa0 + 1..=11`。
//!
//! 本模块结构：
//! - 版面几何：`Geometry`（设备点阵 + 预览降采样因子 + 页边距/槽距）与
//!   `CellRect`（一格的设备坐标矩形）；`PreviewJob` 只用 (页数, N-up, 面数)
//!   三个输入，与拼版卡同构但不共享代码（B22 起模块自包含）。
//! - 双路计算：`layout_cells()` 在**设备点阵**上算格矩形，`preview_cells()`
//!   在**预览点阵**（设备 ÷ div）上独立算一遍再乘回 div。两条路径的整除
//!   余数都显式校验，逐项比较是判据 J1 的「逐项一致」来源。
//! - 预览账：`PreviewLedger`（定容抽检记录，样本号去重 + 容量显式拒绝）、
//!   `PreviewReport`（抽检数/几何一致数/像素一致数/失配块数/链摘要）。
//! - 模拟器档像素模型：页内容量化在**预览块格**上（每块 div×div 设备像素
//!   同值），因此「设备像素块平均」与「预览像素」在数学上恒等——这正是
//!   要验的输出管线性质；映射错位（+半块）会立刻产生失配，判据据此区分
//!   「恒等管线」与「近似管线」。
//! - 错误路径：空活 / 非法 N-up / 超页 / 点阵不整除 / 格数不符 / 格重叠 /
//!   页越界 / 缩放因子越界 / 账本满 / 样本重复 / 段内码漂移。
//! - 注入核：`injection_core()` 6 类坏样本（空活、N-up=7、div=7、格重叠、
//!   页越界、样本重复）逐类断言被拒。
//!
//! 零 panic 纪律：全部入口 `Result`；数组访问走 `get()`；无 std。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x42fa0 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x42fa0;

/// A4 @ 600 dpi：210mm ≈ 4960 点，297mm ≈ 7016 点；上取整到 8 点阵 = 7040，
/// 保证 div=8 时整页可整除（点阵补齐 24 点，落在页边距外侧，不打内容）。
pub const PAPER_W: u32 = 4960;
pub const PAPER_H: u32 = 7040;
/// 预览降采样因子：1 预览像素 = 8×8 设备像素。
pub const PREVIEW_DIV: u32 = 8;
/// 页边距 240 点 = 10.16mm；槽距 64 点 = 2.71mm。两者都能被 8 整除，
/// 是「预览点阵整除」判据的硬前提（改常量必须同步改判据推导）。
pub const MARGIN: u32 = 240;
pub const GUTTER: u32 = 64;

/// 抽检窗口：每格取 6×6 预览块。36 块足以暴露半块平移，又不把 debug
/// 构建的抽检成本推高（36×64 设备像素/格 ×10 格）。
pub const WIN: u32 = 6;
/// 抽检格数（J1 卡面口径）。
pub const SAMPLES: usize = 10;

const E_EMPTY: u16 = 1;
const E_NUP: u16 = 2;
const E_PAGES: u16 = 3;
const E_DIVISIBLE: u16 = 4;
const E_CELL: u16 = 5;
const E_OVERLAP: u16 = 6;
const E_UNKNOWN_PAGE: u16 = 7;
const E_SCALE: u16 = 8;
const E_LEDGER_FULL: u16 = 9;
const E_DUP_SAMPLE: u16 = 10;
const E_CODE: u16 = 11;

/// 预览域错误十一型，`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewError {
    EmptyJob,
    BadNUp,
    PageOverflow,
    NotDivisible,
    CellMismatch,
    Overlap,
    UnknownPage,
    ScaleRange,
    LedgerFull,
    DuplicateSample,
    CodeDrift,
}

impl PreviewError {
    fn ordinal(self) -> u16 {
        match self {
            PreviewError::EmptyJob => E_EMPTY,
            PreviewError::BadNUp => E_NUP,
            PreviewError::PageOverflow => E_PAGES,
            PreviewError::NotDivisible => E_DIVISIBLE,
            PreviewError::CellMismatch => E_CELL,
            PreviewError::Overlap => E_OVERLAP,
            PreviewError::UnknownPage => E_UNKNOWN_PAGE,
            PreviewError::ScaleRange => E_SCALE,
            PreviewError::LedgerFull => E_LEDGER_FULL,
            PreviewError::DuplicateSample => E_DUP_SAMPLE,
            PreviewError::CodeDrift => E_CODE,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            PreviewError::EmptyJob => "preview job has no pages",
            PreviewError::BadNUp => "n-up not in preview profile",
            PreviewError::PageOverflow => "page count above preview cap",
            PreviewError::NotDivisible => "cell lattice not divisible",
            PreviewError::CellMismatch => "preview/layout cell count differs",
            PreviewError::Overlap => "cells overlap on one face",
            PreviewError::UnknownPage => "page outside 1..=pages",
            PreviewError::ScaleRange => "preview scale factor invalid",
            PreviewError::LedgerFull => "preview ledger at capacity",
            PreviewError::DuplicateSample => "sample id already recorded",
            PreviewError::CodeDrift => "diagnostic code outside segment",
        }
    }
}

/// 全错误表：判据按序验证码值闭集。
pub const ALL_ERRORS: [PreviewError; 11] = [
    PreviewError::EmptyJob,
    PreviewError::BadNUp,
    PreviewError::PageOverflow,
    PreviewError::NotDivisible,
    PreviewError::CellMismatch,
    PreviewError::Overlap,
    PreviewError::UnknownPage,
    PreviewError::ScaleRange,
    PreviewError::LedgerFull,
    PreviewError::DuplicateSample,
    PreviewError::CodeDrift,
];

/// 设备像素矩形（半开区间：右/下边界不含）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> u32 {
        self.x.saturating_add(self.w)
    }

    pub fn bottom(&self) -> u32 {
        self.y.saturating_add(self.h)
    }

    /// 半开区间相交（相邻不算重叠）。
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

/// 版面几何：设备点阵 + 预览因子 + 边距/槽距。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub dots_w: u32,
    pub dots_h: u32,
    pub div: u32,
    pub margin: u32,
    pub gutter: u32,
}

impl Geometry {
    pub const fn new(dots_w: u32, dots_h: u32, div: u32, margin: u32, gutter: u32) -> Geometry {
        Geometry {
            dots_w,
            dots_h,
            div,
            margin,
            gutter,
        }
    }

    /// 出厂模拟器档：A4 600dpi + 8 倍降采样。
    pub const fn a4_600dpi() -> Geometry {
        Geometry::new(PAPER_W, PAPER_H, PREVIEW_DIV, MARGIN, GUTTER)
    }

    pub fn preview_w(&self) -> u32 {
        self.dots_w / self.div
    }

    pub fn preview_h(&self) -> u32 {
        self.dots_h / self.div
    }

    /// 点阵前提校验：div 有界、三处尺寸都能被 div 整除、内容区为正。
    pub fn validate(&self) -> Result<(), PreviewError> {
        if self.div == 0 || self.div > 64 {
            return Err(PreviewError::ScaleRange);
        }
        if self.dots_w == 0 || self.dots_h == 0 {
            return Err(PreviewError::ScaleRange);
        }
        if self.margin.saturating_mul(2) >= self.dots_w
            || self.margin.saturating_mul(2) >= self.dots_h
        {
            return Err(PreviewError::ScaleRange);
        }
        if self.dots_w % self.div != 0
            || self.dots_h % self.div != 0
            || self.margin % self.div != 0
            || self.gutter % self.div != 0
        {
            return Err(PreviewError::NotDivisible);
        }
        Ok(())
    }
}

/// 预览作业：只由 (页数, N-up, 面数) 决定，避免两条路径看到不同输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewJob {
    pub pages: u16,
    pub n_up: u8,
    pub sides: u8,
}

impl PreviewJob {
    pub const fn new(pages: u16, n_up: u8, sides: u8) -> PreviewJob {
        PreviewJob { pages, n_up, sides }
    }

    /// 预览档支持的 N-up：{1,2,4,8}。6/9/16 在 8 点阵上除不尽，显式拒绝
    /// 而不是让格宽悄悄截断 1 点（截断正是预览与输出漂移的经典来源）。
    pub fn grid(&self) -> Result<(u32, u32), PreviewError> {
        match self.n_up {
            1 => Ok((1, 1)),
            2 => Ok((2, 1)),
            4 => Ok((2, 2)),
            8 => Ok((4, 2)),
            _ => Err(PreviewError::BadNUp),
        }
    }

    pub fn faces(&self) -> Result<u32, PreviewError> {
        match self.sides {
            1 => Ok(1),
            2 => Ok(2),
            _ => Err(PreviewError::BadNUp),
        }
    }

    pub fn sheets(&self) -> Result<u32, PreviewError> {
        if self.pages == 0 {
            return Err(PreviewError::EmptyJob);
        }
        let (c, r) = self.grid()?;
        let per_sheet = c * r * self.faces()?;
        if per_sheet == 0 {
            return Err(PreviewError::CellMismatch);
        }
        if self.pages > 512 {
            return Err(PreviewError::PageOverflow);
        }
        Ok(((self.pages as u32) + per_sheet - 1) / per_sheet)
    }
}

/// 一格的落位：面内槽位决定矩形，页号 0 表示补白（不抽检）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRect {
    pub sheet: u16,
    pub side: u8,
    pub slot: u8,
    pub page: u16,
    pub rect: Rect,
}

/// 设备路径：在整点阵上算格矩形。
pub fn layout_cells(job: &PreviewJob, geo: &Geometry) -> Result<Vec<CellRect>, PreviewError> {
    geo.validate()?;
    let (cols, rows) = job.grid()?;
    let faces = job.faces()?;
    let sheets = job.sheets()?;
    let usable_w = geo
        .dots_w
        .checked_sub(geo.margin.saturating_mul(2))
        .ok_or(PreviewError::ScaleRange)?;
    let usable_h = geo
        .dots_h
        .checked_sub(geo.margin.saturating_mul(2))
        .ok_or(PreviewError::ScaleRange)?;
    let span_w = usable_w
        .checked_sub(geo.gutter.saturating_mul(cols.saturating_sub(1)))
        .ok_or(PreviewError::ScaleRange)?;
    let span_h = usable_h
        .checked_sub(geo.gutter.saturating_mul(rows.saturating_sub(1)))
        .ok_or(PreviewError::ScaleRange)?;
    if cols == 0 || rows == 0 || span_w % cols != 0 || span_h % rows != 0 {
        return Err(PreviewError::NotDivisible);
    }
    let cw = span_w / cols;
    let ch = span_h / rows;
    if cw < geo.div || ch < geo.div {
        return Err(PreviewError::ScaleRange);
    }
    let per_sheet = (cols * rows * faces) as usize;
    let total = sheets as usize * per_sheet;
    let mut out = Vec::with_capacity(total);
    let mut k = 0usize;
    while k < total {
        let sheet = (k / per_sheet) as u16;
        let within = (k % per_sheet) as u32;
        let side = (within / (cols * rows)) as u8;
        let slot = (within % (cols * rows)) as u16;
        let page = if k < job.pages as usize {
            (k as u16) + 1
        } else {
            0
        };
        let col = slot as u32 % cols;
        let row = slot as u32 / cols;
        let rect = Rect::new(
            geo.margin + col * (cw + geo.gutter),
            geo.margin + row * (ch + geo.gutter),
            cw,
            ch,
        );
        out.push(CellRect {
            sheet,
            side,
            slot: slot as u8,
            page,
            rect,
        });
        k += 1;
    }
    validate_cells(&out, job.pages)?;
    Ok(out)
}

/// 预览路径：先在预览点阵独立算一遍，再乘回 div。它与设备路径的输入只有
/// job/geo，但中间量全是 `preview_*`，因此能抓「某一路少除了 div」的漂移。
pub fn preview_cells(job: &PreviewJob, geo: &Geometry) -> Result<Vec<CellRect>, PreviewError> {
    geo.validate()?;
    let (cols, rows) = job.grid()?;
    let faces = job.faces()?;
    let sheets = job.sheets()?;
    let pw = geo.preview_w();
    let ph = geo.preview_h();
    let m = geo.margin / geo.div;
    let g = geo.gutter / geo.div;
    if m.saturating_mul(2) >= pw || m.saturating_mul(2) >= ph {
        return Err(PreviewError::ScaleRange);
    }
    let span_w = pw
        .checked_sub(m * 2)
        .and_then(|v| v.checked_sub(g.saturating_mul(cols.saturating_sub(1))))
        .ok_or(PreviewError::ScaleRange)?;
    let span_h = ph
        .checked_sub(m * 2)
        .and_then(|v| v.checked_sub(g.saturating_mul(rows.saturating_sub(1))))
        .ok_or(PreviewError::ScaleRange)?;
    if span_w % cols != 0 || span_h % rows != 0 {
        return Err(PreviewError::NotDivisible);
    }
    let cw = span_w / cols;
    let ch = span_h / rows;
    if cw < 1 || ch < 1 {
        return Err(PreviewError::ScaleRange);
    }
    let per_sheet = (cols * rows * faces) as usize;
    let total = sheets as usize * per_sheet;
    let mut out = Vec::with_capacity(total);
    let mut k = 0usize;
    while k < total {
        let sheet = (k / per_sheet) as u16;
        let within = (k % per_sheet) as u32;
        let side = (within / (cols * rows)) as u8;
        let slot = (within % (cols * rows)) as u16;
        let page = if k < job.pages as usize {
            (k as u16) + 1
        } else {
            0
        };
        let col = slot as u32 % cols;
        let row = slot as u32 / cols;
        let rect = Rect::new(
            (m + col * (cw + g)) * geo.div,
            (m + row * (ch + g)) * geo.div,
            cw * geo.div,
            ch * geo.div,
        );
        out.push(CellRect {
            sheet,
            side,
            slot: slot as u8,
            page,
            rect,
        });
        k += 1;
    }
    Ok(out)
}

/// 格表校验：页面范围、槽位唯一、同面不重叠、矩形不越纸。
pub fn validate_cells(cells: &[CellRect], pages: u16) -> Result<(), PreviewError> {
    if cells.is_empty() {
        return Err(PreviewError::EmptyJob);
    }
    let mut i = 0usize;
    while i < cells.len() {
        let a = match cells.get(i) {
            Some(v) => v,
            None => return Err(PreviewError::CellMismatch),
        };
        if a.page > pages {
            return Err(PreviewError::UnknownPage);
        }
        if a.rect.w == 0 || a.rect.h == 0 {
            return Err(PreviewError::CellMismatch);
        }
        if a.rect.right() > PAPER_W || a.rect.bottom() > PAPER_H {
            return Err(PreviewError::CellMismatch);
        }
        let mut j = i + 1;
        while j < cells.len() {
            if let Some(b) = cells.get(j) {
                if a.sheet == b.sheet && a.side == b.side {
                    if a.slot == b.slot {
                        return Err(PreviewError::CellMismatch);
                    }
                    if a.rect.intersects(&b.rect) {
                        return Err(PreviewError::Overlap);
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 页面墨量模型：量化在预览块格上（同块内 div×div 设备像素同值）。
/// 75% 覆盖 + 32 底色，既让窗口内既有同值块也有异值块，平移后必然失配。
fn ink(page: u16, bx: u32, by: u32) -> u8 {
    if page == 0 {
        return 0;
    }
    let mut h = (page as u64).wrapping_mul(0x9E37_79B1_85EB_CA87);
    h ^= (bx as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= (by as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 29;
    h = h.wrapping_mul(0x2545_F491_4F6C_DD1D);
    h ^= h >> 32;
    if h & 3 != 0 {
        255
    } else {
        32
    }
}

fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 抽检记录：样本号唯一，附格摘要与失配块数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewEntry {
    pub sample_id: u16,
    pub page: u16,
    pub blocks: u16,
    pub mismatched: u16,
    pub digest: u64,
}

/// 预览账：定容、去重、显式拒绝满/重。
#[derive(Clone, Debug)]
pub struct PreviewLedger {
    pub entries: Vec<PreviewEntry>,
    pub cap: usize,
}

impl PreviewLedger {
    pub fn new(cap: usize) -> PreviewLedger {
        PreviewLedger {
            entries: Vec::new(),
            cap,
        }
    }

    /// 记账：先查容量再查样本号，两错误类别不混。
    pub fn record(&mut self, e: PreviewEntry) -> Result<(), PreviewError> {
        if self.entries.len() >= self.cap {
            return Err(PreviewError::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(x) => {
                    if x.sample_id == e.sample_id {
                        return Err(PreviewError::DuplicateSample);
                    }
                }
                None => return Err(PreviewError::CellMismatch),
            }
            i += 1;
        }
        self.entries.push(e);
        Ok(())
    }

    pub fn total_mismatch(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                s += e.mismatched as u32;
            }
            i += 1;
        }
        s
    }

    pub fn audit(&self) -> Result<(), PreviewError> {
        if self.entries.is_empty() {
            return Err(PreviewError::EmptyJob);
        }
        if self.entries.len() > self.cap {
            return Err(PreviewError::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) => {
                    if e.blocks != (WIN * WIN) as u16 {
                        return Err(PreviewError::CellMismatch);
                    }
                }
                None => return Err(PreviewError::CellMismatch),
            }
            i += 1;
        }
        Ok(())
    }
}

/// 抽检报告（不可变快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewReport {
    pub cells: usize,
    pub sampled: usize,
    pub geometric_equal: usize,
    pub pixel_equal: usize,
    pub mismatched_blocks: u32,
    pub digest: u64,
}

/// 一次抽检的完整输出（报告 + 账本）。
#[derive(Clone, Debug)]
pub struct PreviewRun {
    pub report: PreviewReport,
    pub ledger: PreviewLedger,
}

/// 单格窗口比对：(失配块数, 格摘要)。`shift` 是输出侧整体平移（坏样本注入
/// 用；正常路径恒 0）。预览值取块采样，输出值取 div×div 设备像素平均。
fn compare_cell(cell: &CellRect, div: u32, sample: u16, shift: u32) -> (u32, u64) {
    let bw = cell.rect.w / div;
    let bh = cell.rect.h / div;
    let ox = if bw > WIN {
        (sample as u32).wrapping_mul(13) % (bw - WIN)
    } else {
        0
    };
    let oy = if bh > WIN {
        (sample as u32).wrapping_mul(7) % (bh - WIN)
    } else {
        0
    };
    let mut mis = 0u32;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut j = 0u32;
    while j < WIN {
        let mut i = 0u32;
        while i < WIN {
            let bx = ox + i;
            let by = oy + j;
            let pv = ink(cell.page, bx, by);
            let base_x = cell.rect.x.saturating_add(bx.saturating_mul(div)).saturating_add(shift);
            let base_y = cell.rect.y.saturating_add(by.saturating_mul(div));
            let mut sum = 0u32;
            let mut cnt = 0u32;
            let mut v = 0u32;
            while v < div {
                let mut u = 0u32;
                while u < div {
                    let px = base_x.saturating_add(u);
                    let py = base_y.saturating_add(v);
                    let qx = px.saturating_sub(cell.rect.x) / div;
                    let qy = py.saturating_sub(cell.rect.y) / div;
                    sum += ink(cell.page, qx, qy) as u32;
                    cnt += 1;
                    u += 1;
                }
                v += 1;
            }
            let avg = if cnt > 0 { sum / cnt } else { 0 };
            if avg != pv as u32 {
                mis += 1;
            }
            h = mix(h, ((bx as u64) << 24) ^ ((by as u64) << 8) ^ pv as u64);
            i += 1;
        }
        j += 1;
    }
    (mis, h)
}

/// 主抽检入口：几何逐项 + 10 格像素窗口 + 账本记账。`shift` 仅供坏样本路径。
pub fn run_compare(
    job: &PreviewJob,
    geo: &Geometry,
    shift: u32,
) -> Result<PreviewRun, PreviewError> {
    let layout = layout_cells(job, geo)?;
    let mut preview = preview_cells(job, geo)?;
    if shift != 0 {
        // 坏样本路径：预览账整版平移（等价于预览侧少算/多算一段边距）。
        let mut i = 0usize;
        while i < preview.len() {
            if let Some(c) = preview.get_mut(i) {
                c.rect.x = c.rect.x.saturating_add(shift);
            }
            i += 1;
        }
    }
    if layout.len() != preview.len() || layout.is_empty() {
        return Err(PreviewError::CellMismatch);
    }
    let mut geometric = 0usize;
    let mut i = 0usize;
    while i < layout.len() {
        match (layout.get(i), preview.get(i)) {
            (Some(a), Some(b)) => {
                if a.rect == b.rect && a.page == b.page && a.slot == b.slot && a.side == b.side {
                    geometric += 1;
                }
            }
            _ => return Err(PreviewError::CellMismatch),
        }
        i += 1;
    }
    let mut picks: Vec<usize> = Vec::with_capacity(layout.len());
    let mut k = 0usize;
    while k < layout.len() {
        if let Some(c) = layout.get(k) {
            if c.page != 0 {
                picks.push(k);
            }
        }
        k += 1;
    }
    if picks.is_empty() {
        return Err(PreviewError::EmptyJob);
    }
    let mut ledger = PreviewLedger::new(32);
    let mut sampled = 0usize;
    let mut pixel_equal = 0usize;
    let mut mism = 0u32;
    let mut chain: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut s = 0usize;
    while s < SAMPLES {
        let at = (s * picks.len()) / SAMPLES;
        let cell = match picks.get(at).and_then(|idx| layout.get(*idx)) {
            Some(c) => *c,
            None => return Err(PreviewError::CellMismatch),
        };
        let (mis, dg) = compare_cell(&cell, geo.div, s as u16, shift);
        if mis == 0 {
            pixel_equal += 1;
        }
        mism += mis;
        chain = mix(chain, dg);
        chain = mix(chain, cell.rect.x as u64 ^ ((cell.rect.y as u64) << 20));
        let e = PreviewEntry {
            sample_id: s as u16,
            page: cell.page,
            blocks: (WIN * WIN) as u16,
            mismatched: mis as u16,
            digest: dg,
        };
        ledger.record(e)?;
        sampled += 1;
        s += 1;
    }
    let report = PreviewReport {
        cells: layout.len(),
        sampled,
        geometric_equal: geometric,
        pixel_equal,
        mismatched_blocks: mism,
        digest: chain,
    };
    Ok(PreviewRun { report, ledger })
}

// ---------------------------------------------------------------------------
// 注入核：六类坏样本
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let total = 6usize;
    let mut caught = 0usize;
    let geo = Geometry::a4_600dpi();

    if PreviewJob::new(0, 4, 2).sheets().err() == Some(PreviewError::EmptyJob) {
        caught += 1;
    }
    if PreviewJob::new(8, 7, 2).sheets().err() == Some(PreviewError::BadNUp) {
        caught += 1;
    }
    let bad_div = Geometry::new(PAPER_W, PAPER_H, 7, MARGIN, GUTTER);
    if bad_div.validate().err() == Some(PreviewError::NotDivisible) {
        caught += 1;
    }
    // 重叠：把第二格平移到与第一格完全重合。
    match layout_cells(&PreviewJob::new(8, 4, 1), &geo) {
        Ok(mut cells) => {
            let first = cells.get(0).copied();
            if let (Some(a), Some(b)) = (first, cells.get_mut(1)) {
                b.rect = a.rect;
            }
            if validate_cells(&cells, 8).err() == Some(PreviewError::Overlap) {
                caught += 1;
            }
        }
        Err(_) => {}
    }
    // 页越界：把末格页号抬到 pages+1。
    match layout_cells(&PreviewJob::new(8, 4, 1), &geo) {
        Ok(mut cells) => {
            let last = cells.len().saturating_sub(1);
            if let Some(c) = cells.get_mut(last) {
                c.page = 9;
            }
            if validate_cells(&cells, 8).err() == Some(PreviewError::UnknownPage) {
                caught += 1;
            }
        }
        Err(_) => {}
    }
    // 样本重复：同一样本号记两次。
    let mut led = PreviewLedger::new(4);
    let e = PreviewEntry {
        sample_id: 3,
        page: 1,
        blocks: (WIN * WIN) as u16,
        mismatched: 0,
        digest: 1,
    };
    if led.record(e).is_ok() && led.record(e).err() == Some(PreviewError::DuplicateSample) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50028_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50028");
    let geo = Geometry::a4_600dpi();
    let job = PreviewJob::new(13, 4, 2);

    // J1-1 抽检 ×10：几何一致、像素一致、零失配块。
    let mut main_ok = false;
    let mut main_det = String::new();
    let mut digest_a = 0u64;
    if let Ok(run) = run_compare(&job, &geo, 0) {
        let r = run.report;
        main_ok = r.sampled == SAMPLES
            && r.pixel_equal == SAMPLES
            && r.geometric_equal == r.cells
            && r.mismatched_blocks == 0
            && run.ledger.total_mismatch() == 0
            && run.ledger.audit().is_ok();
        digest_a = r.digest;
        main_det = format!(
            "样本{}/{}/失配{}",
            r.sampled, r.pixel_equal, r.mismatched_blocks
        );
    }
    s.add_dyn("50028-J1-抽检10像素一致", main_ok, &main_det);

    // J1-2 几何逐项一致：设备路径与预览路径的 16 格矩形逐项相等。
    let mut geo_ok = false;
    if let (Ok(a), Ok(b)) = (layout_cells(&job, &geo), preview_cells(&job, &geo)) {
        geo_ok = a.len() == b.len()
            && a.len() == 16
            && a.iter().zip(b.iter()).all(|(x, y)| x.rect == y.rect);
    }
    s.add("50028-J1-双路几何一致", geo_ok, if geo_ok { "" } else { "两路矩形不一致" });

    // J1-3 块降采样恒等：整格块值与输出块平均逐块相等（不是近似）。
    let mut ident_ok = false;
    let mut ident_max = 0u32;
    if let Ok(cells) = layout_cells(&job, &geo) {
        if let Some(c) = cells.get(0) {
            let (mis, _) = compare_cell(c, geo.div, 0, 0);
            ident_max = mis;
            ident_ok = mis == 0;
        }
    }
    let det = format!("失配块={}", ident_max);
    s.add_dyn("50028-J1-降采样恒等", ident_ok, &det);

    // J1-4 预览摘要确定：同输入两次抽检，链摘要逐位一致。
    let mut det_ok = false;
    if let Ok(r2) = run_compare(&job, &geo, 0) {
        det_ok = digest_a != 0 && r2.report.digest == digest_a;
        if main_ok {
            det_ok = det_ok && r2.report.pixel_equal == SAMPLES;
        }
    }
    s.add("50028-J1-摘要确定", det_ok, if det_ok { "" } else { "抽检摘要非确定" });

    // J1-5 采样点确定且不重复：样本号 0..9 恰好各一次。
    let mut samples_ok = false;
    if let Ok(run) = run_compare(&job, &geo, 0) {
        let mut seen = [false; SAMPLES];
        let mut i = 0usize;
        while i < run.ledger.entries.len() {
            if let Some(e) = run.ledger.entries.get(i) {
                let sid = e.sample_id as usize;
                if sid < SAMPLES {
                    seen[sid] = true;
                }
            }
            i += 1;
        }
        samples_ok = seen.iter().all(|b| *b);
    }
    s.add("50028-J1-样本号唯一", samples_ok, if samples_ok { "" } else { "样本号缺失/重复" });

    // J1-6 半块平移注入必被抓：几何一致数下降且失配块 > 0。
    let mut shift_ok = false;
    let mut shift_det = String::new();
    if let Ok(run) = run_compare(&job, &geo, geo.div / 2) {
        let r = run.report;
        shift_ok = r.geometric_equal < r.cells && r.mismatched_blocks > 0;
        shift_det = format!(
            "geo={}/{} mis={}",
            r.geometric_equal, r.cells, r.mismatched_blocks
        );
    }
    s.add_dyn("50028-J1-平移必抓", shift_ok, &shift_det);

    // J1-7 点阵不整除 / 缩放越界：两种错误码不混。
    let bad = Geometry::new(PAPER_W, PAPER_H, 7, MARGIN, GUTTER);
    let zero = Geometry::new(PAPER_W, PAPER_H, 0, MARGIN, GUTTER);
    let ok = bad.validate().err() == Some(PreviewError::NotDivisible)
        && zero.validate().err() == Some(PreviewError::ScaleRange);
    s.add("50028-J1-点阵前提", ok, if ok { "" } else { "几何前提未分码" });

    // J1-8 重叠拒绝：同面两格相交必须 Overlap。
    let mut overlap_ok = false;
    if let Ok(mut cells) = layout_cells(&PreviewJob::new(8, 4, 1), &geo) {
        let first = cells.get(0).copied();
        if let (Some(a), Some(b)) = (first, cells.get_mut(1)) {
            b.rect = Rect::new(a.rect.x + 1, a.rect.y, a.rect.w, a.rect.h);
        }
        overlap_ok = validate_cells(&cells, 8).err() == Some(PreviewError::Overlap);
    }
    s.add("50028-J1-重叠拒绝", overlap_ok, if overlap_ok { "" } else { "重叠未拒绝" });

    // J1-9 页越界拒绝：页号 > pages 必须 UnknownPage。
    let mut page_ok = false;
    if let Ok(mut cells) = layout_cells(&PreviewJob::new(8, 4, 1), &geo) {
        if let Some(c) = cells.get_mut(0) {
            c.page = 9;
        }
        page_ok = validate_cells(&cells, 8).err() == Some(PreviewError::UnknownPage);
    }
    s.add("50028-J1-页越界拒绝", page_ok, if page_ok { "" } else { "越界页未拒绝" });

    // J1-10 账本去重 + 容量显式拒绝（两码不混）。
    let led_ok;
    let mut led = PreviewLedger::new(2);
    let e0 = PreviewEntry {
        sample_id: 0,
        page: 1,
        blocks: (WIN * WIN) as u16,
        mismatched: 0,
        digest: 7,
    };
    let dup = led.record(e0).is_ok() && led.record(e0).err() == Some(PreviewError::DuplicateSample);
    let e1 = PreviewEntry {
        sample_id: 1,
        page: 1,
        blocks: (WIN * WIN) as u16,
        mismatched: 0,
        digest: 1,
    };
    let e2 = PreviewEntry {
        sample_id: 2,
        page: 1,
        blocks: (WIN * WIN) as u16,
        mismatched: 0,
        digest: 2,
    };
    let full = led.record(e1).is_ok() && led.record(e2).err() == Some(PreviewError::LedgerFull);
    led_ok = dup && full && led.entries.len() == 2;
    s.add("50028-J1-账本去重容量", led_ok, if led_ok { "" } else { "账本去重/容量失守" });

    // J1-11 边角半开区间：末列右边界恰为纸宽 - 边距，末行同理。
    let mut edge_ok = false;
    if let Ok(cells) = layout_cells(&job, &geo) {
        let mut max_right = 0u32;
        let mut max_bottom = 0u32;
        let mut i = 0usize;
        while i < cells.len() {
            if let Some(c) = cells.get(i) {
                if c.rect.right() > max_right {
                    max_right = c.rect.right();
                }
                if c.rect.bottom() > max_bottom {
                    max_bottom = c.rect.bottom();
                }
            }
            i += 1;
        }
        edge_ok = max_right == PAPER_W - MARGIN && max_bottom == PAPER_H - MARGIN;
    }
    s.add("50028-J1-边角半开", edge_ok, if edge_ok { "" } else { "边界留白漂移" });

    // J1-12 补白不入样：13 页 4-up 双面 = 16 格，抽检页号必须全非零。
    let mut blank_ok = false;
    if let Ok(run) = run_compare(&job, &geo, 0) {
        blank_ok = run.report.cells == 16
            && run
                .ledger
                .entries
                .iter()
                .all(|e| e.page != 0 && e.page <= 13);
    }
    s.add("50028-J1-补白不入样", blank_ok, if blank_ok { "" } else { "补白格被抽检" });

    // J1-13 页码影响墨量：不同页的窗口值不同（防「常墨」假实现）。
    let mut vary_ok = false;
    if let (Ok(a), Ok(b)) = (
        layout_cells(&PreviewJob::new(13, 4, 2), &geo),
        layout_cells(&PreviewJob::new(13, 4, 2), &geo),
    ) {
        if let (Some(ca), Some(cb)) = (a.get(0), b.get(1)) {
            let (_, da) = compare_cell(ca, geo.div, 0, 0);
            let (_, db) = compare_cell(cb, geo.div, 0, 0);
            vary_ok = da != db;
        }
    }
    s.add("50028-J1-页码影响墨量", vary_ok, if vary_ok { "" } else { "墨量对页码不敏感" });

    // J1-14 错误码闭集：11 条，码值 = 基址 + 序号且唯一。
    let mut codes_ok = ALL_ERRORS.len() == 11;
    let mut seen_codes = [false; 12];
    let mut i = 0usize;
    while i < ALL_ERRORS.len() {
        if let Some(e) = ALL_ERRORS.get(i) {
            let c = e.code();
            if c != SEG_BASE + (i as u32) + 1 {
                codes_ok = false;
            }
            let off = (c - SEG_BASE) as usize;
            if off < seen_codes.len() {
                if seen_codes[off] {
                    codes_ok = false;
                }
                seen_codes[off] = true;
            } else {
                codes_ok = false;
            }
        }
        i += 1;
    }
    s.add("50028-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    // J1-15 注入核 6 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50028-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50028_checks_green() {
        let set = run_unx50028_checks();
        assert!(set.all_passed(), "UNX-F50028 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50028_preview_matches_layout() {
        let geo = Geometry::a4_600dpi();
        let job = PreviewJob::new(13, 4, 2);
        let a = layout_cells(&job, &geo);
        let b = preview_cells(&job, &geo);
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.len(), y.len());
                for (p, q) in x.iter().zip(y.iter()) {
                    assert_eq!(p.rect, q.rect);
                }
            }
            _ => panic!("cell computation failed"),
        }
    }
}
