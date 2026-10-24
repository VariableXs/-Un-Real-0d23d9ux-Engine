//! VE-F3839 · S02 预备自查
//!
//! 十维自查（S02 定制维）+ 风险册 + GO 预判四条件 + 移交四件套。
//! 缺色检出 P0。黄超期转红。NO-GO 缺输入。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xBBxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xBB00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CBBCode(pub u16);

impl CBBCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const RED_OPEN: Self = Self(SEG_BASE + 1);
    pub const INPUT_MISSING: Self = Self(SEG_BASE + 2);
    pub const COLOR_MISSING_P0: Self = Self(SEG_BASE + 3);
    pub const AMBER_EXPIRED: Self = Self(SEG_BASE + 4);
    pub const HANDOFF_INCOMPLETE: Self = Self(SEG_BASE + 5);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::RED_OPEN => "红未清，阻断",
            Self::INPUT_MISSING => "判定缺输入，NO-GO",
            Self::COLOR_MISSING_P0 => "缺色检出（P0）",
            Self::AMBER_EXPIRED => "黄超期，转红",
            Self::HANDOFF_INCOMPLETE => "移交件不全",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 十维（S02 定制）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dim {
    /// 架构。
    Arch,
    /// forced。
    Forced,
    /// 映射。
    Mapping,
    /// 色弱。
    Cvd,
    /// 色板。
    Palette,
    /// 实时。
    Realtime,
    /// 文字。
    Text,
    /// 图形。
    Graphic,
    /// 性能。
    Perf,
    /// 联动。
    Link,
}

impl Dim {
    pub const ALL: [Dim; 10] = [
        Dim::Arch,
        Dim::Forced,
        Dim::Mapping,
        Dim::Cvd,
        Dim::Palette,
        Dim::Realtime,
        Dim::Text,
        Dim::Graphic,
        Dim::Perf,
        Dim::Link,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Arch => "架构",
            Self::Forced => "forced",
            Self::Mapping => "映射",
            Self::Cvd => "色弱",
            Self::Palette => "色板",
            Self::Realtime => "实时",
            Self::Text => "文字",
            Self::Graphic => "图形",
            Self::Perf => "性能",
            Self::Link => "联动",
        }
    }
}

/// 灯色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    /// 绿。
    Green,
    /// 黄。
    Amber,
    /// 红。
    Red,
}

impl Light {
    pub fn rank(self) -> u8 {
        match self {
            Self::Green => 0,
            Self::Amber => 1,
            Self::Red => 2,
        }
    }
}

/// 维状态。
#[derive(Debug, Clone, Copy)]
pub struct DimState {
    /// 维。
    pub dim: Dim,
    /// 灯。
    pub light: Light,
    /// 黄龄（天）。
    pub amber_age_days: u16,
    /// 缺色检出。
    pub color_missing: bool,
}

impl DimState {
    /// 黄超期（>7 天）转红。
    pub fn effective_light(&self) -> Light {
        if self.light == Light::Amber && self.amber_age_days > 7 {
            Light::Red
        } else {
            self.light
        }
    }
}

/// 风险册条目。
#[derive(Debug, Clone)]
pub struct RiskEntry {
    /// 描述。
    pub desc: &'static str,
    /// 灯。
    pub light: Light,
    /// 是否到期。
    pub due: bool,
}

/// GO 预判四条件（S 版）。
#[derive(Debug, Clone, Copy)]
pub struct GoConditions {
    /// 十维红零。
    pub red_zero: bool,
    /// 缺色零。
    pub color_missing_zero: bool,
    /// 自动门全绿。
    pub auto_gates_green: bool,
    /// 同源断言绿。
    pub source_assert_green: bool,
}

impl GoConditions {
    pub fn all_met(&self) -> bool {
        self.red_zero && self.color_missing_zero && self.auto_gates_green && self.source_assert_green
    }
}

/// 移交四件套。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffItem {
    /// 接口总账。
    ApiLedger,
    /// 断言单源。
    AssertSource,
    /// 风险册。
    RiskBook,
    /// 自查矩阵。
    SelfMatrix,
}

impl HandoffItem {
    pub const ALL: [HandoffItem; 4] = [
        HandoffItem::ApiLedger,
        HandoffItem::AssertSource,
        HandoffItem::RiskBook,
        HandoffItem::SelfMatrix,
    ];
}

/// 预备自查器。
#[derive(Debug, Clone)]
pub struct S02Preflight {
    /// 十维状态。
    pub dims: Vec<DimState>,
    /// 风险册。
    pub risks: Vec<RiskEntry>,
    /// GO 条件。
    pub go: GoConditions,
    /// 移交清单（已核收）。
    pub handoff: Vec<HandoffItem>,
}

impl S02Preflight {
    pub fn new() -> Self {
        Self {
            dims: Vec::new(),
            risks: Vec::new(),
            go: GoConditions {
                red_zero: false,
                color_missing_zero: false,
                auto_gates_green: false,
                source_assert_green: false,
            },
            handoff: Vec::new(),
        }
    }

    /// 十维齐备。
    pub fn dims_complete(&self) -> bool {
        Dim::ALL.iter().all(|d| self.dims.iter().any(|s| s.dim == *d))
    }

    /// 有效红零。
    pub fn red_zero(&self) -> bool {
        self.dims.iter().all(|s| s.effective_light() != Light::Red)
    }

    /// 缺色零。
    pub fn color_missing_zero(&self) -> bool {
        self.dims.iter().all(|s| !s.color_missing)
    }

    /// 黄超期检出。
    pub fn amber_expired(&self) -> bool {
        self.dims
            .iter()
            .any(|s| s.light == Light::Amber && s.amber_age_days > 7)
    }

    /// 移交核收。
    pub fn handoff_complete(&self) -> bool {
        HandoffItem::ALL.iter().all(|h| self.handoff.contains(h))
    }

    /// GO 判定。
    pub fn decide(&self) -> Result<(), CBBCode> {
        if !self.dims_complete() {
            return Err(CBBCode::INPUT_MISSING);
        }
        if self.color_missing_zero() == false {
            return Err(CBBCode::COLOR_MISSING_P0);
        }
        if self.amber_expired() {
            return Err(CBBCode::AMBER_EXPIRED);
        }
        if !self.red_zero() {
            return Err(CBBCode::RED_OPEN);
        }
        if !self.go.all_met() {
            return Err(CBBCode::INPUT_MISSING);
        }
        if !self.handoff_complete() {
            return Err(CBBCode::HANDOFF_INCOMPLETE);
        }
        Ok(())
    }

    /// 同步 GO 条件（从维状态推导）。
    pub fn sync_go(&mut self) {
        self.go.red_zero = self.red_zero();
        self.go.color_missing_zero = self.color_missing_zero();
    }
}

impl Default for S02Preflight {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves139_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves139_s02preflight");

    fn ready() -> S02Preflight {
        let mut p = S02Preflight::new();
        for d in Dim::ALL.iter() {
            p.dims.push(DimState {
                dim: *d,
                light: Light::Green,
                amber_age_days: 0,
                color_missing: false,
            });
        }
        p.go = GoConditions {
            red_zero: true,
            color_missing_zero: true,
            auto_gates_green: true,
            source_assert_green: true,
        };
        for h in HandoffItem::ALL.iter() {
            p.handoff.push(*h);
        }
        p
    }

    // 01 十维齐备
    {
        let p = ready();
        chk(&mut set, "C3839-01", p.dims_complete(), "err");
    }
    // 02 缺输入 NO-GO
    {
        let mut p = ready();
        p.dims.clear();
        chk(
            &mut set,
            "C3839-02",
            p.decide() == Err(CBBCode::INPUT_MISSING),
            "err",
        );
    }
    // 03 红未清
    {
        let mut p = ready();
        p.dims[0].light = Light::Red;
        p.sync_go();
        chk(
            &mut set,
            "C3839-03",
            p.decide() == Err(CBBCode::RED_OPEN),
            "err",
        );
    }
    // 04 缺色 P0
    {
        let mut p = ready();
        p.dims[1].color_missing = true;
        p.sync_go();
        chk(
            &mut set,
            "C3839-04",
            p.decide() == Err(CBBCode::COLOR_MISSING_P0),
            "err",
        );
    }
    // 05 黄超期转红
    {
        let mut p = ready();
        p.dims[2].light = Light::Amber;
        p.dims[2].amber_age_days = 10;
        chk(
            &mut set,
            "C3839-05",
            p.dims[2].effective_light() == Light::Red,
            "err",
        );
    }
    // 06 黄超期检出
    {
        let mut p = ready();
        p.dims[3].light = Light::Amber;
        p.dims[3].amber_age_days = 30;
        p.sync_go();
        chk(
            &mut set,
            "C3839-06",
            p.decide() == Err(CBBCode::AMBER_EXPIRED),
            "err",
        );
    }
    // 07 黄未超期
    {
        let mut p = ready();
        p.dims[4].light = Light::Amber;
        p.dims[4].amber_age_days = 3;
        p.sync_go();
        chk(&mut set, "C3839-07", p.decide().is_ok(), "err");
    }
    // 08 移交不全
    {
        let mut p = ready();
        p.handoff.clear();
        chk(
            &mut set,
            "C3839-08",
            p.decide() == Err(CBBCode::HANDOFF_INCOMPLETE),
            "err",
        );
    }
    // 09 GO 条件缺
    {
        let mut p = ready();
        p.go.auto_gates_green = false;
        chk(
            &mut set,
            "C3839-09",
            p.decide() == Err(CBBCode::INPUT_MISSING),
            "err",
        );
    }
    // 10 全绿 GO
    {
        let p = ready();
        chk(&mut set, "C3839-10", p.decide().is_ok(), "err");
    }
    // 11 移交核收
    {
        let p = ready();
        chk(&mut set, "C3839-11", p.handoff_complete(), "err");
    }
    // 12 同步 GO
    {
        let mut p = ready();
        p.dims[0].light = Light::Red;
        p.sync_go();
        chk(&mut set, "C3839-12", !p.go.red_zero, "err");
    }

    set
}
