//! VE-F3836 · 色彩适配调试
//!
//! 高对比检视五列仪表 + 对比度热图定位 + 三类色弱/高对比模拟切换。
//! 热图定位红线：对比度问题不可视 = 排查靠猜。家族二十二复述协议。

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码段（0xB8xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB8Code(pub u16);

impl CB8Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const HEATMAP_BLIND: Self = Self(SEG_BASE + 1);
    pub const CALIBRATION_DRIFT: Self = Self(SEG_BASE + 2);
    pub const GAUGE_DIVERGE: Self = Self(SEG_BASE + 3);
    pub const SIM_MISSING: Self = Self(SEG_BASE + 4);
    pub const PROTOCOL_BREAK: Self = Self(SEG_BASE + 5);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::HEATMAP_BLIND => "热图盲区（红线）：对比度问题不可视",
            Self::CALIBRATION_DRIFT => "热图校准漂移，需重校",
            Self::GAUGE_DIVERGE => "五列仪表分歧，立案",
            Self::SIM_MISSING => "模拟通道缺失，需补",
            Self::PROTOCOL_BREAK => "家族协议断，需归一",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 五列仪表列位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaugeColumn {
    /// 映射状态。
    MapState,
    /// 白名单命中。
    WhitelistHit,
    /// 自动修复记录。
    AutoFix,
    /// 对比度采样。
    ContrastSample,
    /// 协议心跳。
    ProtocolBeat,
}

impl GaugeColumn {
    pub const ALL: [GaugeColumn; 5] = [
        GaugeColumn::MapState,
        GaugeColumn::WhitelistHit,
        GaugeColumn::AutoFix,
        GaugeColumn::ContrastSample,
        GaugeColumn::ProtocolBeat,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MapState => "映射状态",
            Self::WhitelistHit => "白名单命中",
            Self::AutoFix => "自动修复",
            Self::ContrastSample => "对比采样",
            Self::ProtocolBeat => "协议心跳",
        }
    }
}

/// 单元格值。
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    /// 文本。
    Text(String),
    /// 数值。
    Number(u32),
    /// 布尔。
    Flag(bool),
}

/// 仪表行。
#[derive(Debug, Clone)]
pub struct GaugeRow {
    /// 节点标识。
    pub node: String,
    /// 五列取值（下标对齐 GaugeColumn::ALL）。
    pub cells: Vec<CellValue>,
}

impl GaugeRow {
    pub fn new(node: &str) -> Self {
        Self {
            node: String::from(node),
            cells: Vec::new(),
        }
    }

    /// 补齐到五列（缺列填 Flag(false)）。
    pub fn pad_to_five(&mut self) {
        while self.cells.len() < 5 {
            self.cells.push(CellValue::Flag(false));
        }
    }

    /// 列是否齐。
    pub fn columns_complete(&self) -> bool {
        self.cells.len() == 5
    }
}

/// 热图像素（对比度采样点）。
#[derive(Debug, Clone, Copy)]
pub struct HeatPixel {
    /// 归一化 x（0..1000）。
    pub x: u16,
    /// 归一化 y（0..1000）。
    pub y: u16,
    /// 对比度比值 ×100（450 = 4.5:1）。
    pub contrast_x100: u16,
    /// 阈值 ×100。
    pub threshold_x100: u16,
}

impl HeatPixel {
    pub fn is_hot(&self) -> bool {
        self.contrast_x100 < self.threshold_x100
    }

    /// 缺口（阈值-实测，正数=不达标）。
    pub fn gap(&self) -> i32 {
        self.threshold_x100 as i32 - self.contrast_x100 as i32
    }
}

/// 热图。
#[derive(Debug, Clone)]
pub struct ContrastHeatmap {
    /// 采样点。
    pub pixels: Vec<HeatPixel>,
    /// 校准系数 ×1000（1000 = 无漂移）。
    pub calibration_x1000: u32,
}

impl ContrastHeatmap {
    pub fn new() -> Self {
        Self {
            pixels: Vec::new(),
            calibration_x1000: 1000,
        }
    }

    /// 热点数（不达标像素）。
    pub fn hot_count(&self) -> usize {
        self.pixels.iter().filter(|p| p.is_hot()).count()
    }

    /// 最大缺口。
    pub fn max_gap(&self) -> i32 {
        let mut g = 0i32;
        for p in self.pixels.iter() {
            if p.is_hot() {
                let d = p.gap();
                if d > g {
                    g = d;
                }
            }
        }
        g
    }

    /// 校准红线：|cal-1000| ≤ 50。
    pub fn calibration_ok(&self) -> bool {
        let d = if self.calibration_x1000 >= 1000 {
            self.calibration_x1000 - 1000
        } else {
            1000 - self.calibration_x1000
        };
        d <= 50
    }

    /// 热图可见性红线：有热点必须能定位到坐标。
    pub fn locate_visible(&self) -> bool {
        for p in self.pixels.iter() {
            if p.is_hot() && (p.x > 1000 || p.y > 1000) {
                return false;
            }
        }
        true
    }
}

/// 色弱/高对比模拟态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimMode {
    /// 无模拟。
    None,
    /// 红色弱。
    Protan,
    /// 绿色弱。
    Deutan,
    /// 蓝色弱。
    Tritan,
    /// 高对比。
    HighContrast,
}

impl SimMode {
    pub const CVD_ALL: [SimMode; 3] = [SimMode::Protan, SimMode::Deutan, SimMode::Tritan];

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "关闭",
            Self::Protan => "红色弱",
            Self::Deutan => "绿色弱",
            Self::Tritan => "蓝色弱",
            Self::HighContrast => "高对比",
        }
    }
}

/// 模拟通道。
#[derive(Debug, Clone)]
pub struct SimChannel {
    /// 模式。
    pub mode: SimMode,
    /// 是否可用。
    pub available: bool,
    /// 预览帧延迟（ms，0 = 立即）。
    pub preview_latency_ms: u32,
}

/// 家族协议成员（二十二成员之本域占位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyMember {
    /// 调试器。
    Debugger,
    /// 热图。
    Heatmap,
    /// 模拟器。
    Simulator,
    /// 仪表。
    Gauge,
}

impl FamilyMember {
    pub const ALL: [FamilyMember; 4] = [
        FamilyMember::Debugger,
        FamilyMember::Heatmap,
        FamilyMember::Simulator,
        FamilyMember::Gauge,
    ];

    pub fn beat(self) -> u8 {
        match self {
            Self::Debugger => 1,
            Self::Heatmap => 2,
            Self::Simulator => 3,
            Self::Gauge => 4,
        }
    }
}

/// 调试器。
#[derive(Debug, Clone)]
pub struct ColorDebugger {
    /// 仪表行。
    pub rows: Vec<GaugeRow>,
    /// 热图。
    pub heatmap: ContrastHeatmap,
    /// 模拟通道。
    pub sims: Vec<SimChannel>,
    /// 家族心跳（成员下标 → 最近节拍）。
    pub beats: Vec<u8>,
    /// 是否打开（打开才采样）。
    pub active: bool,
}

impl ColorDebugger {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            heatmap: ContrastHeatmap::new(),
            sims: Vec::new(),
            beats: Vec::new(),
            active: false,
        }
    }

    /// 打开（打开才采样——复述开销红线）。
    pub fn open(&mut self) {
        self.active = true;
    }

    /// 采样入口：未打开不采。
    pub fn sample_row(&mut self, node: &str, col: usize, val: CellValue) -> Result<(), CB8Code> {
        if !self.active {
            return Ok(());
        }
        if col >= 5 {
            return Err(CB8Code::GAUGE_DIVERGE);
        }
        let mut row = GaugeRow::new(node);
        row.cells.push(val);
        row.pad_to_five();
        self.rows.push(row);
        Ok(())
    }

    /// 仪表分歧检测：同一节点出现两行不同映射状态。
    pub fn gauge_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            let mut j = i + 1;
            while j < self.rows.len() {
                if self.rows[i].node == self.rows[j].node {
                    let a = &self.rows[i].cells[0];
                    let b = &self.rows[j].cells[0];
                    if a != b {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 注册模拟通道。
    pub fn register_sim(&mut self, mode: SimMode) {
        self.sims.push(SimChannel {
            mode,
            available: true,
            preview_latency_ms: 0,
        });
    }

    /// 模拟通道齐备（三色弱 + 高对比）。
    pub fn sims_complete(&self) -> bool {
        for m in SimMode::CVD_ALL.iter() {
            if !self.sims.iter().any(|s| s.mode == *m && s.available) {
                return false;
            }
        }
        self.sims.iter().any(|s| s.mode == SimMode::HighContrast && s.available)
    }

    /// 切换预览（即时，latency=0）。
    pub fn preview(&self, mode: SimMode) -> Result<u32, CB8Code> {
        for s in self.sims.iter() {
            if s.mode == mode {
                if !s.available {
                    return Err(CB8Code::SIM_MISSING);
                }
                return Ok(s.preview_latency_ms);
            }
        }
        Err(CB8Code::SIM_MISSING)
    }

    /// 家族心跳。
    pub fn beat_member(&mut self, m: FamilyMember) -> u8 {
        let b = m.beat();
        self.beats.push(b);
        b
    }

    /// 协议完整：四成员心跳均出现。
    pub fn protocol_intact(&self) -> bool {
        FamilyMember::ALL.iter().all(|m| self.beats.contains(&m.beat()))
    }

    /// 热图红线总检。
    pub fn heatmap_redline(&self) -> Result<(), CB8Code> {
        if !self.heatmap.calibration_ok() {
            return Err(CB8Code::CALIBRATION_DRIFT);
        }
        if !self.heatmap.locate_visible() {
            return Err(CB8Code::HEATMAP_BLIND);
        }
        Ok(())
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CB8Code> {
        self.heatmap_redline()?;
        if !self.gauge_consistent() {
            return Err(CB8Code::GAUGE_DIVERGE);
        }
        if !self.sims_complete() {
            return Err(CB8Code::SIM_MISSING);
        }
        if !self.protocol_intact() {
            return Err(CB8Code::PROTOCOL_BREAK);
        }
        Ok(())
    }

    /// 常态零开销：未打开时行数为 0。
    pub fn idle_zero(&self) -> bool {
        !self.active && self.rows.is_empty()
    }
}

impl Default for ColorDebugger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves136_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves136_colordebug");

    fn ready() -> ColorDebugger {
        let mut d = ColorDebugger::new();
        d.open();
        d.register_sim(SimMode::Protan);
        d.register_sim(SimMode::Deutan);
        d.register_sim(SimMode::Tritan);
        d.register_sim(SimMode::HighContrast);
        d.beat_member(FamilyMember::Debugger);
        d.beat_member(FamilyMember::Heatmap);
        d.beat_member(FamilyMember::Simulator);
        d.beat_member(FamilyMember::Gauge);
        d
    }

    // 01 热图定位红线（有热点坐标必须可定位）
    {
        let mut d = ready();
        d.heatmap.pixels.push(HeatPixel {
            x: 500,
            y: 500,
            contrast_x100: 200,
            threshold_x100: 450,
        });
        chk(&mut set, "C3836-01", d.heatmap.locate_visible(), "err");
    }
    // 02 热图盲区（坐标越界）
    {
        let mut d = ready();
        d.heatmap.pixels.push(HeatPixel {
            x: 1200,
            y: 10,
            contrast_x100: 100,
            threshold_x100: 450,
        });
        chk(
            &mut set,
            "C3836-02",
            d.heatmap_redline() == Err(CB8Code::HEATMAP_BLIND),
            "err",
        );
    }
    // 03 校准漂移
    {
        let mut d = ready();
        d.heatmap.calibration_x1000 = 1200;
        chk(
            &mut set,
            "C3836-03",
            d.heatmap_redline() == Err(CB8Code::CALIBRATION_DRIFT),
            "err",
        );
    }
    // 04 校准红线内
    {
        let mut d = ready();
        d.heatmap.calibration_x1000 = 1030;
        chk(&mut set, "C3836-04", d.heatmap_redline().is_ok(), "err");
    }
    // 05 仪表分歧
    {
        let mut d = ready();
        d.sample_row("n1", 0, CellValue::Text(String::from("mapped"))).ok();
        d.sample_row("n1", 0, CellValue::Text(String::from("raw"))).ok();
        chk(&mut set, "C3836-05", !d.gauge_consistent(), "err");
    }
    // 06 仪表一致
    {
        let mut d = ready();
        d.sample_row("n1", 0, CellValue::Flag(true)).ok();
        d.sample_row("n2", 0, CellValue::Flag(true)).ok();
        chk(&mut set, "C3836-06", d.gauge_consistent(), "err");
    }
    // 07 模拟缺
    {
        let mut d = ColorDebugger::new();
        d.open();
        d.register_sim(SimMode::Protan);
        chk(&mut set, "C3836-07", !d.sims_complete(), "err");
    }
    // 08 模拟齐
    {
        let d = ready();
        chk(&mut set, "C3836-08", d.sims_complete(), "err");
    }
    // 09 预览即时
    {
        let d = ready();
        chk(&mut set, "C3836-09", d.preview(SimMode::HighContrast) == Ok(0), "err");
    }
    // 10 预览缺通道
    {
        let mut d = ready();
        d.sims.clear();
        chk(
            &mut set,
            "C3836-10",
            d.preview(SimMode::Tritan) == Err(CB8Code::SIM_MISSING),
            "err",
        );
    }
    // 11 协议完整
    {
        let d = ready();
        chk(&mut set, "C3836-11", d.protocol_intact() && d.full_check().is_ok(), "err");
    }
    // 12 常态零开销
    {
        let d = ColorDebugger::new();
        chk(&mut set, "C3836-12", d.idle_zero(), "err");
    }

    set
}
