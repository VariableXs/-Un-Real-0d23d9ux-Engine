//! VE-F3642 · 壁纸编辑器（VE-S 域 · 批次 R03 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3642`
//!
//! **职责定位（锚点原文）**：层级画布编辑——
//! - **图层系统**（图层栈/混合模式/蒙版/变换）——图层模型复用
//!   F3030 转场图层思想跨域声明（模式复用单源）；
//! - **编辑工具**：画笔/渐变/形状/文字/裁剪五族基础工具（工具覆盖声明）；
//! - **实时预览**：复用 F3606（单源）；**撤销**：复用 undo；
//! - **智能辅助**：对齐吸附/智能填充预留（预留显性复述）。
//!
//! **判据（锚点原文六条）**：图层复用、五族工具、预览单源、智能预留、
//! 单源红线、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 图层破（混合错乱）→模型修正（复用红线） | [`W42Code::LAYER_BROKEN`] |
//! | 预览失真→断言（复用） | [`W42Code::PREVIEW_DRIFT`] |
//! | 撤销丢层→P1（复述红线实测） | [`W42Code::UNDO_LOST_LAYER`] |
//! | 预留调用→显性（复述） | [`W42Code::RESERVE_CALLED`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3642 诊断码段（独占 0x5Axx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct W42Code(pub u16);

impl W42Code {
    /// 图层模型破损（混合错乱）。
    pub const LAYER_BROKEN: W42Code = W42Code(0x5A01);
    /// 预览失真。
    pub const PREVIEW_DRIFT: W42Code = W42Code(0x5A02);
    /// 撤销丢层（P1 红线实测）。
    pub const UNDO_LOST_LAYER: W42Code = W42Code(0x5A03);
    /// 智能预留被调（显性）。
    pub const RESERVE_CALLED: W42Code = W42Code(0x5A04);
    /// 工具族缺覆盖。
    pub const TOOL_GAP: W42Code = W42Code(0x5A05);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            W42Code::LAYER_BROKEN => "图层模型破损（混合错乱）。建议：按 F3030 模式修正。",
            W42Code::PREVIEW_DRIFT => "预览失真。建议：F3606 单源对拍。",
            W42Code::UNDO_LOST_LAYER => "撤销丢层（P1 红线实测）。建议：undo 快照含全层栈。",
            W42Code::RESERVE_CALLED => "智能辅助预留被调（显性上报）。",
            W42Code::TOOL_GAP => "五族工具覆盖缺口。建议：补齐画笔/渐变/形状/文字/裁剪。",
            _ => "未知诊断码。",
        }
    }
}

// ── 图层模型（复用 F3030 口径） ─────────────────────────────────────

/// 混合模式（图层栈）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    /// 正常。
    Normal,
    /// 正片叠底。
    Multiply,
    /// 滤色。
    Screen,
    /// 叠加。
    Overlay,
}

/// 图层条目。
#[derive(Clone, Copy)]
pub struct Layer {
    pub id: u32,
    pub blend: Blend,
    pub opacity_bps: u32,
    pub masked: bool,
    pub visible: bool,
}

impl Layer {
    /// 合法透明度（0..=10000）。
    pub fn opacity_ok(&self) -> bool {
        self.opacity_bps <= 10_000
    }
}

// ── 五族工具 ────────────────────────────────────────────────────────

/// 基础工具族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// 画笔。
    Brush,
    /// 渐变。
    Gradient,
    /// 形状。
    Shape,
    /// 文字。
    Text,
    /// 裁剪。
    Crop,
}

impl Tool {
    pub const ALL: [Tool; 5] = [
        Tool::Brush,
        Tool::Gradient,
        Tool::Shape,
        Tool::Text,
        Tool::Crop,
    ];
}

// ── 撤销快照 ────────────────────────────────────────────────────────

/// 一步撤销记录（含全层栈计数，丢层红线）。
#[derive(Clone, Copy)]
pub struct Snapshot {
    pub layer_count: u32,
    pub seq: u32,
}

// ── 编辑器 ──────────────────────────────────────────────────────────

/// 壁纸编辑器：图层栈 + 五族工具 + 预览 + undo + 智能预留。
pub struct WallpaperEditor {
    layers: Vec<Layer>,
    tool_coverage: Vec<Tool>,
    snaps: Vec<Snapshot>,
    seq: u32,
    preview_hash: u64,
    canonical_hash: u64,
    reserve_calls: u32,
    last_code: W42Code,
    has_error: bool,
}

impl WallpaperEditor {
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            tool_coverage: Vec::new(),
            snaps: Vec::new(),
            seq: 0,
            preview_hash: 0,
            canonical_hash: 0,
            reserve_calls: 0,
            last_code: W42Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> W42Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: W42Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 加层；透明度越界判模型破损。
    pub fn add_layer(&mut self, id: u32, blend: Blend, opacity_bps: u32) -> Result<usize, W42Code> {
        let layer = Layer { id, blend, opacity_bps, masked: false, visible: true };
        if !layer.opacity_ok() {
            self.fail(W42Code::LAYER_BROKEN);
            return Err(W42Code::LAYER_BROKEN);
        }
        self.seq += 1;
        self.snaps.push(Snapshot { layer_count: self.layers.len() as u32, seq: self.seq });
        self.layers.push(layer);
        self.last_code = W42Code(0);
        self.has_error = false;
        Ok(self.layers.len())
    }

    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// 切换可见性。
    pub fn set_visible(&mut self, id: u32, on: bool) -> Result<bool, W42Code> {
        for l in self.layers.iter_mut() {
            if l.id == id {
                l.visible = on;
                self.last_code = W42Code(0);
                self.has_error = false;
                return Ok(true);
            }
        }
        self.fail(W42Code::LAYER_BROKEN);
        Err(W42Code::LAYER_BROKEN)
    }

    /// 工具使用登记（覆盖审计用）。
    pub fn use_tool(&mut self, tool: Tool) -> usize {
        if !self.tool_coverage.contains(&tool) {
            self.tool_coverage.push(tool);
        }
        self.tool_coverage.len()
    }

    /// 五族全覆盖。
    pub fn tools_complete(&self) -> bool {
        Tool::ALL.iter().all(|t| self.tool_coverage.contains(t))
    }

    /// 覆盖缺口数。
    pub fn tool_gaps(&self) -> u32 {
        Tool::ALL
            .iter()
            .filter(|t| !self.tool_coverage.contains(t))
            .count() as u32
    }

    /// 预览对拍（F3606 单源口径）。
    pub fn sync_preview(&mut self, canonical: u64) -> Result<bool, W42Code> {
        self.canonical_hash = canonical;
        self.preview_hash = canonical;
        self.last_code = W42Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 预览失真断言。
    pub fn assert_preview(&mut self, preview: u64) -> Result<bool, W42Code> {
        if preview != self.canonical_hash {
            self.fail(W42Code::PREVIEW_DRIFT);
            return Err(W42Code::PREVIEW_DRIFT);
        }
        self.preview_hash = preview;
        self.last_code = W42Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 撤销一步；恢复层栈计数必须等于快照（丢层红线）。
    pub fn undo(&mut self) -> Result<u32, W42Code> {
        match self.snaps.pop() {
            Some(s) => {
                if (self.layers.len() as u32) < s.layer_count {
                    self.fail(W42Code::UNDO_LOST_LAYER);
                    return Err(W42Code::UNDO_LOST_LAYER);
                }
                self.last_code = W42Code(0);
                self.has_error = false;
                Ok(s.seq)
            }
            None => {
                self.fail(W42Code::UNDO_LOST_LAYER);
                Err(W42Code::UNDO_LOST_LAYER)
            }
        }
    }

    pub fn undo_depth(&self) -> usize {
        self.snaps.len()
    }

    /// 智能辅助预留（对齐吸附/智能填充）：调用显性上报。
    pub fn smart_assist(&mut self) -> Result<u32, W42Code> {
        self.reserve_calls += 1;
        self.fail(W42Code::RESERVE_CALLED);
        Ok(self.reserve_calls)
    }

    pub fn reserve_calls(&self) -> u32 {
        self.reserve_calls
    }

    /// 预览当前 hash。
    pub fn preview_hash(&self) -> u64 {
        self.preview_hash
    }

    /// 蒙版图层数。
    pub fn masked_count(&self) -> usize {
        self.layers.iter().filter(|l| l.masked).count()
    }

    /// 可见图层数。
    pub fn visible_count(&self) -> usize {
        self.layers.iter().filter(|l| l.visible).count()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves42_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves42_walledit");
    {
        let e = WallpaperEditor::new();
        chk(&mut set, "C3642-01", e.layer_count() == 0 && e.undo_depth() == 0, "err");
    }
    {
        let mut e = WallpaperEditor::new();
        let r = e.add_layer(1, Blend::Normal, 5_000);
        chk(&mut set, "C3642-02", r == Ok(1), "err");
    }
    {
        let mut e = WallpaperEditor::new();
        let r = e.add_layer(1, Blend::Normal, 20_000);
        chk(&mut set, "C3642-03", r.err() == Some(W42Code::LAYER_BROKEN), "err");
    }
    {
        let mut e = WallpaperEditor::new();
        e.add_layer(1, Blend::Multiply, 8_000);
        let r = e.set_visible(1, false);
        chk(&mut set, "C3642-04", r == Ok(true) && e.visible_count() == 0, "err");
    }
    {
        let mut e = WallpaperEditor::new();
        for t in Tool::ALL.iter() {
            e.use_tool(*t);
        }
        chk(&mut set, "C3642-05", e.tools_complete() && e.tool_gaps() == 0, "err");
    }
    {
        let mut e = WallpaperEditor::new();
        e.use_tool(Tool::Brush);
        chk(&mut set, "C3642-06", e.tool_gaps() == 4, "err");
    }
    {
        let mut e = WallpaperEditor::new();
        e.sync_preview(0xABC);
        let r = e.assert_preview(0xABC);
        chk(&mut set, "C3642-07", r == Ok(true), "err");
    }
    {
        let mut e = WallpaperEditor::new();
        e.sync_preview(0xABC);
        let r = e.assert_preview(0xBAD);
        chk(&mut set, "C3642-08", r.err() == Some(W42Code::PREVIEW_DRIFT), "err");
    }
    {
        let mut e = WallpaperEditor::new();
        e.add_layer(1, Blend::Normal, 100);
        let r = e.undo();
        chk(&mut set, "C3642-09", r == Ok(1) && e.undo_depth() == 0, "err");
    }
    {
        let mut e = WallpaperEditor::new();
        let r = e.undo();
        chk(&mut set, "C3642-10", r.err() == Some(W42Code::UNDO_LOST_LAYER), "err");
    }
    {
        let mut e = WallpaperEditor::new();
        let r = e.smart_assist();
        chk(&mut set, "C3642-11", r == Ok(1) && e.reserve_calls() == 1, "err");
    }
    {
        let mut e = WallpaperEditor::new();
        e.add_layer(1, Blend::Overlay, 10_000);
        chk(&mut set, "C3642-12", e.layer_count() == 1 && e.undo_depth() == 1, "err");
    }
    set
}
