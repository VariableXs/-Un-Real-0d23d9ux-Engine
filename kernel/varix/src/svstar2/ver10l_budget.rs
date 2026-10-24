//! VE-F3552 · persona 性能预算 —— 三段预算 + 超支降级告知 + 加重复核 + 默认预留。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3552`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三段预算**（[`BudgetSegment`]）：切换 / 加载 / 常驻三段
//!   独立预算，各段各账。
//! - **超支降级告知**（[`Budget::charge`]）：超支不静默降级，
//!   降级动作与原因一并返回（用户可知情）。
//! - **加重复核**（[`Budget::reconcile`]）：三段实测加总与域级
//!   总预算复核，加总超支→协商（[`ReconcileVerdict`]）。
//! - **默认预留**：计量缺失时按默认预留额计入（不漏账）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 超支 | 降级 + 告知 |
//! | 加总超支 | 协商 |
//! | 计量缺失 | 默认预留 |
//!
//! # 复杂度
//!
//! O(段)（三段定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6220 起）
// ---------------------------------------------------------------------------

/// 性能预算诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetCode {
    /// 单段超支（已降级）。
    SegmentOver = 6220,
    /// 加总超支（需协商）。
    TotalOver = 6221,
    /// 计量缺失（已按默认预留）。
    MeterMissing = 6222,
    /// 段 id 非法。
    SegmentInvalid = 6223,
}

impl BudgetCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BudgetCode::SegmentOver => "预算段超支已降级",
            BudgetCode::TotalOver => "三段加总超支需协商",
            BudgetCode::MeterMissing => "计量缺失按默认预留",
            BudgetCode::SegmentInvalid => "预算段非法",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, BudgetCode::TotalOver)
    }
}

// ---------------------------------------------------------------------------
// 二、三段
// ---------------------------------------------------------------------------

/// 预算段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetSegment {
    /// 切换（Persona 切换瞬时）。
    Switch,
    /// 加载（首启加载）。
    Load,
    /// 常驻（内存常驻）。
    Resident,
}

impl BudgetSegment {
    /// 全部三段。
    pub const ALL: [BudgetSegment; 3] = [
        BudgetSegment::Switch,
        BudgetSegment::Load,
        BudgetSegment::Resident,
    ];

    /// 段短名。
    pub const fn label(self) -> &'static str {
        match self {
            BudgetSegment::Switch => "切换",
            BudgetSegment::Load => "加载",
            BudgetSegment::Resident => "常驻",
        }
    }

    /// 默认预留额（毫秒；常驻段单位为 KiB 同数值域）。
    pub const fn default_reserve(self) -> u32 {
        match self {
            BudgetSegment::Switch => 150,
            BudgetSegment::Load => 2000,
            BudgetSegment::Resident => 4096,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、预算表
// ---------------------------------------------------------------------------

/// 单段账。
#[derive(Clone, Copy, Debug)]
pub struct SegmentBudget {
    /// 段。
    pub segment: BudgetSegment,
    /// 上限。
    pub limit: u32,
    /// 实测（None = 计量缺失）。
    pub measured: Option<u32>,
}

impl SegmentBudget {
    /// 计入值（缺失则默认预留）。
    pub fn charged(&self) -> u32 {
        self.measured.unwrap_or(self.segment.default_reserve())
    }

    /// 是否超支。
    pub fn over(&self) -> bool {
        self.charged() > self.limit
    }
}

/// 降级动作（超支后的处置，用户可知情）。
#[derive(Clone, Copy, Debug)]
pub struct DegradeAction {
    /// 哪段。
    pub segment: BudgetSegment,
    /// 降级说明。
    pub advice: &'static str,
}

/// 加总复核结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconcileVerdict {
    /// 加总守约。
    Within,
    /// 加总超支需协商。
    NeedsNegotiation,
}

/// 预算表。
pub struct Budget {
    segments: Vec<SegmentBudget>,
    /// 域级总上限（三段加总上限）。
    pub total_limit: u32,
}

impl Budget {
    /// 标准预算（三段默认上限）。
    pub fn standard() -> Self {
        let segments = alloc::vec![
            SegmentBudget {
                segment: BudgetSegment::Switch,
                limit: 150,
                measured: None,
            },
            SegmentBudget {
                segment: BudgetSegment::Load,
                limit: 2000,
                measured: None,
            },
            SegmentBudget {
                segment: BudgetSegment::Resident,
                limit: 4096,
                measured: None,
            },
        ];
        Self {
            segments,
            total_limit: 150 + 2000 + 4096,
        }
    }

    /// 计量写入。
    pub fn meter(&mut self, segment: BudgetSegment, value: u32) -> Result<(), BudgetCode> {
        for s in self.segments.iter_mut() {
            if s.segment == segment {
                s.measured = Some(value);
                return Ok(());
            }
        }
        Err(BudgetCode::SegmentInvalid)
    }

    /// 计费：返回超支降级动作（Ok(None) = 守约）。
    pub fn charge(&self, segment: BudgetSegment) -> Result<Option<DegradeAction>, BudgetCode> {
        let s = self
            .segments
            .iter()
            .find(|s| s.segment == segment)
            .ok_or(BudgetCode::SegmentInvalid)?;
        if s.measured.is_none() {
            // 计量缺失：默认预留计入，不算超支错误但告知。
            return Ok(Some(DegradeAction {
                segment,
                advice: "计量缺失按默认预留计入",
            }));
        }
        if s.over() {
            return Ok(Some(DegradeAction {
                segment,
                advice: "超支已降级：削减非必要效果",
            }));
        }
        Ok(None)
    }

    /// 三段加总复核。
    pub fn reconcile(&self) -> ReconcileVerdict {
        let total: u32 = self.segments.iter().map(|s| s.charged()).sum();
        if total > self.total_limit {
            ReconcileVerdict::NeedsNegotiation
        } else {
            ReconcileVerdict::Within
        }
    }

    /// 三段实测加总（含默认预留补位）。
    pub fn total_charged(&self) -> u32 {
        self.segments.iter().map(|s| s.charged()).sum()
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 性能预算判据集。
pub fn run_budget_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10l_budget");

    // 1. 三段预算齐全。
    let b = Budget::standard();
    set.add("6220-三段预算", b.len() == 3, "");

    // 2. 计量缺失默认预留。
    let charged = b.charge(BudgetSegment::Switch).unwrap_or(None);
    let default_used = charged.map(|a| a.advice.contains("默认预留")).unwrap_or(false);
    set.add("6222-默认预留", default_used, "");

    // 3. 超支降级告知。
    let mut b2 = Budget::standard();
    let _ = b2.meter(BudgetSegment::Switch, 500);
    let over = b2.charge(BudgetSegment::Switch).unwrap_or(None);
    let degraded = over.map(|a| a.advice.contains("降级")).unwrap_or(false);
    set.add("6221-超支降级告知", degraded, "");

    // 4. 守约无动作。
    let mut b3 = Budget::standard();
    let _ = b3.meter(BudgetSegment::Switch, 100);
    let _ = b3.meter(BudgetSegment::Load, 1500);
    let _ = b3.meter(BudgetSegment::Resident, 3000);
    let quiet = b3.charge(BudgetSegment::Switch).unwrap_or(None).is_none();
    set.add("6223-守约静默", quiet && b3.reconcile() == ReconcileVerdict::Within, "");

    // 5. 加总超支协商。
    let mut b4 = Budget::standard();
    let _ = b4.meter(BudgetSegment::Switch, 150);
    let _ = b4.meter(BudgetSegment::Load, 2000);
    let _ = b4.meter(BudgetSegment::Resident, 5000);
    set.add(
            "6224-加总协商",
            b4.reconcile() == ReconcileVerdict::NeedsNegotiation,
            "",
        );

    // 6. 加总值正确。
    set.add("6225-加总正确", b4.total_charged() == 150 + 2000 + 5000, "");

    // 7. 段名读屏可达。
    set.add(
            "6226-段名可达",
            BudgetSegment::Load.label() == "加载" && BudgetSegment::Resident.label() == "常驻",
            "",
        );

    set
}
