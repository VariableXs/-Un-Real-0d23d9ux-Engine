//! VE-F3810 · 认知渲染辅助（VE-T 域 · 无障碍渲染 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3810`
//!
//! **判据（锚点原文）**：认知突变红线、简化通道、结构强化、渐进披露、判据。
//!
//! **本条是什么**：认知无障碍渲染——突变红线（布局突变 = 认知冲击 P0）；
//! 简化渲染通道（降信息密度）；结构强化（视觉层级/分组）；
//! 渐进披露（复杂信息分层展开）。
//!
//! 诊断码段：独占 0xC3xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC3xx）
// ─────────────────────────────────────────────────────────────

/// F3810 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C3Code(pub u16);

impl C3Code {
    /// 布局突变——P0。
    pub const MUTATION_P0: C3Code = C3Code(0xC301);
    /// 密度超限——简化。
    pub const DENSITY_OVER: C3Code = C3Code(0xC302);
    /// 结构缺强化。
    pub const STRUCT_WEAK: C3Code = C3Code(0xC303);
    /// 披露层级缺。
    pub const DISCLOSE_MISS: C3Code = C3Code(0xC304);
    /// 简化通道缺位。
    pub const SIMPLE_MISS: C3Code = C3Code(0xC305);
    /// 比较项缺。
    pub const DIFF_MISS: C3Code = C3Code(0xC306);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC301 => "布局突变检出，P0——认知冲击红线。",
            0xC302 => "信息密度超限，需简化通道。",
            0xC303 => "结构强化缺失。",
            0xC304 => "渐进披露层级缺失。",
            0xC305 => "简化渲染通道缺位。",
            0xC306 => "帧比较项缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 布局突变红线（帧间比较）
// ─────────────────────────────────────────────────────────────

/// 布局帧快照。
#[derive(Clone, Copy, Debug)]
pub struct LayoutFrame {
    /// 布局指纹（简单哈希代理）。
    pub fingerprint: u16,
    /// 元素数。
    pub elements: u16,
    /// 位移总量（千分比屏宽）。
    pub shift_bp: u16,
}

impl LayoutFrame {
    /// 构造。
    pub const fn new(fingerprint: u16, elements: u16, shift_bp: u16) -> Self {
        LayoutFrame {
            fingerprint,
            elements,
            shift_bp,
        }
    }
}

/// 突变检测（帧间）。
#[derive(Clone, Debug)]
pub struct MutationDetect {
    /// 阈值：位移 >15% 屏宽视为突变。
    pub shift_limit_bp: u16,
}

impl MutationDetect {
    /// 新建。
    pub const fn new() -> Self {
        MutationDetect {
            shift_limit_bp: 1_500,
        }
    }

    /// 比较两帧（返回突变）。
    pub fn detect(&self, a: LayoutFrame, b: LayoutFrame) -> bool {
        b.shift_bp > self.shift_limit_bp || (a.fingerprint != b.fingerprint && b.shift_bp > 800)
    }

    /// 突变量（位移基点）。
    pub fn magnitude(&self, a: LayoutFrame, b: LayoutFrame) -> u16 {
        if a.fingerprint == b.fingerprint {
            0
        } else {
            b.shift_bp
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 简化通道 + 密度
// ─────────────────────────────────────────────────────────────

/// 简化模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimpleMode {
    /// 全量。
    Full,
    /// 降密度。
    LowDensity,
    /// 分步。
    Stepped,
    /// 极简。
    Minimal,
}

impl SimpleMode {
    /// 全集。
    pub const ALL: [SimpleMode; 4] = [
        SimpleMode::Full,
        SimpleMode::LowDensity,
        SimpleMode::Stepped,
        SimpleMode::Minimal,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            SimpleMode::Full => 0,
            SimpleMode::LowDensity => 1,
            SimpleMode::Stepped => 2,
            SimpleMode::Minimal => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            SimpleMode::Full => "全量",
            SimpleMode::LowDensity => "降密度",
            SimpleMode::Stepped => "分步",
            SimpleMode::Minimal => "极简",
        }
    }
}

/// 密度控制器。
#[derive(Clone, Debug)]
pub struct DensityCtrl {
    /// 当前密度（元素/屏）。
    pub density: u16,
    /// 上限。
    pub limit: u16,
}

impl DensityCtrl {
    /// 新建。
    pub const fn new(density: u16) -> Self {
        DensityCtrl {
            density,
            limit: 20,
        }
    }

    /// 超限。
    pub fn over(&self) -> bool {
        self.density > self.limit
    }

    /// 选择模式。
    pub fn mode(&self) -> SimpleMode {
        if self.density <= self.limit {
            SimpleMode::Full
        } else if self.density <= self.limit * 2 {
            SimpleMode::LowDensity
        } else {
            SimpleMode::Minimal
        }
    }

    /// 简化后密度。
    pub fn simplified(&self) -> u16 {
        match self.mode() {
            SimpleMode::Full => self.density,
            SimpleMode::LowDensity => self.density / 2,
            SimpleMode::Stepped => self.density / 3,
            SimpleMode::Minimal => self.limit,
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 结构强化 + 渐进披露
// ─────────────────────────────────────────────────────────────

/// 结构强化项。
#[derive(Clone, Copy, Debug)]
pub struct StructReinforce {
    /// 视觉层级标注。
    pub has_heading: bool,
    /// 分组标注。
    pub has_group: bool,
    /// 空间分隔。
    pub has_spacing: bool,
}

impl StructReinforce {
    /// 新建缺。
    pub const fn new() -> Self {
        StructReinforce {
            has_heading: false,
            has_group: false,
            has_spacing: false,
        }
    }

    /// 强化分（0..3）。
    pub fn score(self) -> u8 {
        let mut n = 0;
        if self.has_heading {
            n += 1;
        }
        if self.has_group {
            n += 1;
        }
        if self.has_spacing {
            n += 1;
        }
        n
    }

    /// 达标（≥2）。
    pub fn ok(self) -> bool {
        self.score() >= 2
    }
}

/// 渐进披露。
#[derive(Clone, Debug)]
pub struct Progressive {
    /// 层级数。
    pub levels: u8,
    /// 当前展开层。
    pub expanded: u8,
}

impl Progressive {
    /// 新建。
    pub const fn new(levels: u8) -> Self {
        Progressive {
            levels,
            expanded: 1,
        }
    }

    /// 展开一层。
    pub fn expand(&mut self) -> bool {
        if self.expanded < self.levels {
            self.expanded += 1;
            true
        } else {
            false
        }
    }

    /// 收起一层。
    pub fn collapse(&mut self) -> bool {
        if self.expanded > 1 {
            self.expanded -= 1;
            true
        } else {
            false
        }
    }

    /// 层级完整（levels ≥ 2）。
    pub fn layered(&self) -> bool {
        self.levels >= 2
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3810 判据集。
pub fn run_f3810_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3810");

    // 0xC301 布局突变
    {
        let d = MutationDetect::new();
        let a = LayoutFrame::new(100, 10, 0);
        let b = LayoutFrame::new(100, 10, 500);
        set.add("0xC301-同指纹小移不突", !d.detect(a, b), "");
        let c = LayoutFrame::new(200, 10, 1600);
        set.add("0xC301-大移突变", d.detect(a, c), "");
        let e = LayoutFrame::new(300, 10, 900);
        set.add("0xC301-换指纹中移突", d.detect(a, e), "");
    }

    // 0xC306 比较量
    {
        let d = MutationDetect::new();
        let a = LayoutFrame::new(50, 5, 100);
        let b = LayoutFrame::new(50, 5, 100);
        set.add("0xC306-同帧零幅", d.magnitude(a, b) == 0, "");
    }

    // 0xC302 密度
    {
        let c = DensityCtrl::new(15);
        set.add("0xC302-15不超", !c.over() && c.mode() == SimpleMode::Full, "");
        let c2 = DensityCtrl::new(30);
        set.add("0xC302-30超限", c2.over() && c2.mode() == SimpleMode::LowDensity, "");
        set.add("0xC302-简化降半", c2.simplified() == 15, "");
        let c3 = DensityCtrl::new(50);
        set.add("0xC302-极简", c3.mode() == SimpleMode::Minimal, "");
    }

    // 0xC303 结构强化
    {
        let mut s = StructReinforce::new();
        set.add("0xC303-初始零分", s.score() == 0 && !s.ok(), "");
        s.has_heading = true;
        set.add("0xC303-一分不达标", !s.ok(), "");
        s.has_group = true;
        set.add("0xC303-两分达标", s.ok() && s.score() == 2, "");
    }

    // 0xC304 渐进披露
    {
        let mut p = Progressive::new(3);
        set.add("0xC304-分层成立", p.layered(), "");
        set.add("0xC304-展开进", p.expand() && p.expanded == 2, "");
        set.add("0xC304-收起进", p.collapse() && p.expanded == 1, "");
        set.add("0xC304-一层不展开", !p.collapse(), "");
        let p2 = Progressive::new(1);
        set.add("0xC304-单层不满足", !p2.layered(), "");
    }

    // 模式序自洽
    {
        let mut ok = true;
        for (i, m) in SimpleMode::ALL.iter().enumerate() {
            if m.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xC305-模式序自洽", ok, "");
    }

    set
}
