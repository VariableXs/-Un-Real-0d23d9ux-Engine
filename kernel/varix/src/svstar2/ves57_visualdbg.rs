//! VE-F3657 · 视觉创作调试（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3657`
//!
//! **职责定位（锚点原文）**：工坊调试器——
//! - **创作检视**：图层/时间轴/导出三维仪表（复述家族格式）；
//! - **画布剖析**：绘制操作剖析（复用制度）；
//! - **错误定位**：导出失败 → 画布对象定位（复用定位断言）；
//! - **与家族关系**：家族十八成员（复述协议）。
//!
//! **判据（锚点原文六条）**：三维仪表、定位断言、家族十八、
//! 零常态、复用模式、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 定位缺→断言（复述红线实测） | [`V57Code::LOCATE_MISSING`] |
//! | 仪表分歧→立案（复述） | [`V57Code::PANEL_DIVERGED`] |
//! | 开销→打开才采样（复述） | [`V57Code::SAMPLE_ALWAYS_ON`] |
//! | 协议分歧→单源（复述） | [`V57Code::PROTOCOL_DIVERGED`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3657 诊断码段（独占 0x69xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V57Code(pub u16);

impl V57Code {
    /// 定位缺失（红线实测）。
    pub const LOCATE_MISSING: V57Code = V57Code(0x6901);
    /// 三维仪表分歧（立案）。
    pub const PANEL_DIVERGED: V57Code = V57Code(0x6902);
    /// 未开门即采样（零常态破）。
    pub const SAMPLE_ALWAYS_ON: V57Code = V57Code(0x6903);
    /// 家族协议分歧。
    pub const PROTOCOL_DIVERGED: V57Code = V57Code(0x6904);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            V57Code::LOCATE_MISSING => "错误定位缺失（红线实测）。建议：补画布对象定位映射。",
            V57Code::PANEL_DIVERGED => "三维仪表分歧。建议：立案核对图层/时间轴/导出三源。",
            V57Code::SAMPLE_ALWAYS_ON => "未开门即采样。建议：打开才采样（零常态）。",
            V57Code::PROTOCOL_DIVERGED => "家族协议分歧。建议：回归 F2815 单源。",
            _ => "未知诊断码。",
        }
    }
}

// ── 三维仪表 ────────────────────────────────────────────────────────

/// 检视维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    /// 图层。
    Layers,
    /// 时间轴。
    Timeline,
    /// 导出。
    Export,
}

impl Pane {
    /// 家族十八成员槽位（F2815 协议）。
    pub fn family_slot(self) -> u8 {
        match self {
            Pane::Layers => 14,
            Pane::Timeline => 15,
            Pane::Export => 16,
        }
    }
}

/// 仪表读数。
#[derive(Clone, Copy)]
pub struct Reading {
    pub pane: Pane,
    pub value: u32,
    pub dirty: bool,
}

// ── 画布对象（定位目标） ────────────────────────────────────────────

/// 画布对象。
#[derive(Clone, Copy)]
pub struct CanvasObject {
    pub id: u32,
    /// 图层归属。
    pub layer: u32,
}

// ── 剖析 ────────────────────────────────────────────────────────────

/// 绘制操作采样。
#[derive(Clone, Copy)]
pub struct DrawSample {
    pub op_id: u32,
    pub cost_us: u32,
}

// ── 调试器 ──────────────────────────────────────────────────────────

/// 视觉创作调试器。
pub struct VisualDebugger {
    readings: Vec<Reading>,
    objects: Vec<CanvasObject>,
    samples: Vec<DrawSample>,
    sampling_on: bool,
    early_samples: u32,
    protocol_rev: u32,
    last_code: V57Code,
    has_error: bool,
}

impl VisualDebugger {
    /// 新建；`protocol_rev` 为家族协议版本（单源）。
    pub fn new(protocol_rev: u32) -> Self {
        Self {
            readings: Vec::new(),
            objects: Vec::new(),
            samples: Vec::new(),
            sampling_on: false,
            early_samples: 0,
            protocol_rev,
            last_code: V57Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> V57Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: V57Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 推读数。
    pub fn push_reading(&mut self, pane: Pane, value: u32, dirty: bool) -> usize {
        self.readings.push(Reading { pane, value, dirty });
        self.readings.len()
    }

    /// 三维齐。
    pub fn panes_complete(&self) -> bool {
        [Pane::Layers, Pane::Timeline, Pane::Export]
            .iter()
            .all(|p| self.readings.iter().any(|r| r.pane == *p))
    }

    /// 仪表一致性（dirty 即分歧）。
    pub fn panel_aligned(&mut self) -> Result<bool, V57Code> {
        if self.readings.iter().any(|r| r.dirty) {
            self.fail(V57Code::PANEL_DIVERGED);
            Err(V57Code::PANEL_DIVERGED)
        } else {
            self.last_code = V57Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    /// 登记画布对象。
    pub fn add_object(&mut self, id: u32, layer: u32) -> usize {
        self.objects.push(CanvasObject { id, layer });
        self.objects.len()
    }

    /// 错误定位：导出失败 → 对象 id（缺映射即红线）。
    pub fn locate(&mut self, object_id: u32) -> Result<u32, V57Code> {
        match self.objects.iter().find(|o| o.id == object_id) {
            Some(o) => {
                self.last_code = V57Code(0);
                self.has_error = false;
                Ok(o.layer)
            }
            None => {
                self.fail(V57Code::LOCATE_MISSING);
                Err(V57Code::LOCATE_MISSING)
            }
        }
    }

    /// 打开剖析门。
    pub fn open_sampling(&mut self, on: bool) {
        self.sampling_on = on;
    }

    /// 采样（开门才采样）。
    pub fn sample(&mut self, op_id: u32, cost_us: u32) -> Result<usize, V57Code> {
        if !self.sampling_on {
            self.early_samples += 1;
            self.fail(V57Code::SAMPLE_ALWAYS_ON);
            return Err(V57Code::SAMPLE_ALWAYS_ON);
        }
        self.samples.push(DrawSample { op_id, cost_us });
        self.last_code = V57Code(0);
        self.has_error = false;
        Ok(self.samples.len())
    }

    pub fn early_samples(&self) -> u32 {
        self.early_samples
    }

    /// 家族协议核验。
    pub fn verify_protocol(&mut self, rev: u32) -> Result<bool, V57Code> {
        if rev == self.protocol_rev {
            self.last_code = V57Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(V57Code::PROTOCOL_DIVERGED);
            Err(V57Code::PROTOCOL_DIVERGED)
        }
    }

    /// 家族槽位表。
    pub fn family_slots(&self) -> [u8; 3] {
        [
            Pane::Layers.family_slot(),
            Pane::Timeline.family_slot(),
            Pane::Export.family_slot(),
        ]
    }

    pub fn reading_count(&self) -> usize {
        self.readings.len()
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// 平均绘制开销。
    pub fn avg_cost_us(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        (self.samples.iter().map(|s| s.cost_us as u64).sum::<u64>() / self.samples.len() as u64) as u32
    }

    /// 零常态总评。
    pub fn zero_overhead_green(&self) -> bool {
        self.early_samples == 0 && self.panes_complete()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves57_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves57_visualdbg");
    {
        let d = VisualDebugger::new(2);
        chk(&mut set, "C3657-01", d.reading_count() == 0 && d.sample_count() == 0, "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        let n = d.push_reading(Pane::Layers, 3, false);
        chk(&mut set, "C3657-02", n == 1, "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        d.push_reading(Pane::Layers, 1, false);
        d.push_reading(Pane::Timeline, 2, false);
        d.push_reading(Pane::Export, 3, false);
        chk(&mut set, "C3657-03", d.panes_complete(), "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        d.push_reading(Pane::Layers, 1, true);
        let r = d.panel_aligned();
        chk(&mut set, "C3657-04", r.err() == Some(V57Code::PANEL_DIVERGED), "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        d.add_object(1, 7);
        let r = d.locate(1);
        chk(&mut set, "C3657-05", r == Ok(7), "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        let r = d.locate(99);
        chk(&mut set, "C3657-06", r.err() == Some(V57Code::LOCATE_MISSING), "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        let r = d.sample(1, 10);
        chk(&mut set, "C3657-07", r.err() == Some(V57Code::SAMPLE_ALWAYS_ON) && d.early_samples() == 1, "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        d.open_sampling(true);
        d.sample(1, 100);
        d.sample(2, 200);
        chk(&mut set, "C3657-08", d.sample_count() == 2 && d.avg_cost_us() == 150, "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        let r = d.verify_protocol(2);
        chk(&mut set, "C3657-09", r == Ok(true), "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        let r = d.verify_protocol(9);
        chk(&mut set, "C3657-10", r.err() == Some(V57Code::PROTOCOL_DIVERGED), "err");
    }
    {
        let d = VisualDebugger::new(2);
        chk(&mut set, "C3657-11", d.family_slots() == [14, 15, 16], "err");
    }
    {
        let mut d = VisualDebugger::new(2);
        d.push_reading(Pane::Layers, 1, false);
        d.push_reading(Pane::Timeline, 1, false);
        d.push_reading(Pane::Export, 1, false);
        chk(&mut set, "C3657-12", d.zero_overhead_green(), "err");
    }
    set
}
