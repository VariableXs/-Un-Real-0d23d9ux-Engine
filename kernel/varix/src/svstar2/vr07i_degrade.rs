//! VE-F3729 · 创作降级策略（VE-S 域 · 批次 R07 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3729`
//!
//! **职责定位（锚点原文）**：创作降级全域矩阵（F3130 模式四用）——
//! - **四维适配**：质量维（预览精度/代理）→ F3606 /
//!   规模维（分块/图层数）→ F3726 /
//!   频率维（自动保存频率）→ F3047 /
//!   精度维（画布浮点精度）——
//!   四维×五级矩阵（语义保全列：创作语义 = 作品数据，
//!   任何降档不得丢画布数据）——数据保全第一列复述 F3330 红线；
//! - **决策器**：帧/内存/功耗/GPU 四源复述；滞回复用。
//!
//! **判据（锚点原文五条）**：数据保全第一列、四源决策、
//! 四用复刻、滞回复用、只读联动、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 降档丢数据→P0（第一列红线最高级实测） | [`D9Code::DATA_LOSS_P0`] |
//! | 决策振荡→滞回（复述） | [`D9Code::OSCILLATION`] |
//! | 矩阵缺格→补齐（复述） | [`D9Code::MATRIX_GAP`] |
//! | 源缺→保守（复述） | [`D9Code::SRC_MISS`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3729 诊断码段（独占 0x78xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct D9Code(pub u16);

impl D9Code {
    /// 降档丢数据（P0 最高级）。
    pub const DATA_LOSS_P0: D9Code = D9Code(0x7801);
    /// 决策振荡（滞回）。
    pub const OSCILLATION: D9Code = D9Code(0x7802);
    /// 矩阵缺格（补齐）。
    pub const MATRIX_GAP: D9Code = D9Code(0x7803);
    /// 源缺（保守）。
    pub const SRC_MISS: D9Code = D9Code(0x7804);
    /// 降级参数非法。
    pub const BAD_DEGRADE: D9Code = D9Code(0x7805);
    /// 只读联动缺。
    pub const READONLY_LINK_MISS: D9Code = D9Code(0x7806);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            D9Code::DATA_LOSS_P0 => "降档丢数据（P0 最高级）。建议：任何降档不得丢画布数据。",
            D9Code::OSCILLATION => "决策振荡（复述）。建议：滞回稳定后再决策。",
            D9Code::MATRIX_GAP => "矩阵缺格（复述）。建议：补齐四维×五级矩阵。",
            D9Code::SRC_MISS => "决策源缺失（保守复述）。建议：缺源时保守降档。",
            D9Code::BAD_DEGRADE => "降级参数非法。建议：级数 0-4。",
            D9Code::READONLY_LINK_MISS => "只读联动缺。建议：临界降档须联动只读态。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四维×五级矩阵（保全第一列） ─────────────────────────────────────

/// 降级维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeDim {
    /// 质量维（预览精度/代理）。
    Quality,
    /// 规模维（分块/图层数）。
    Scale,
    /// 频率维（自动保存频率）。
    Freq,
    /// 精度维（画布浮点精度）。
    Precision,
}

impl DegradeDim {
    pub const ALL: [DegradeDim; 4] = [
        DegradeDim::Quality,
        DegradeDim::Scale,
        DegradeDim::Freq,
        DegradeDim::Precision,
    ];
}

/// 降级级（0-4）。
pub type DegradeLevel = u8;

/// 矩阵条目。
pub struct DegradeAction {
    /// 是否丢数据（必须恒 false —— 保全列）。
    drops_data: bool,
    /// 只读联动。
    readonly_link: bool,
}

/// 降级矩阵。
pub struct DegradeMatrix {
    /// 4 维 × 5 级 = 20 格。
    cells: [Option<DegradeAction>; 20],
}

impl DegradeMatrix {
    pub fn new() -> DegradeMatrix {
        DegradeMatrix {
            cells: [None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None],
        }
    }

    fn idx(dim: DegradeDim, level: DegradeLevel) -> usize {
        (dim as usize) * 5 + level as usize
    }

    /// 填格（drops_data 必须 false）。
    pub fn set(
        &mut self,
        dim: DegradeDim,
        level: DegradeLevel,
        drops_data: bool,
        readonly_link: bool,
    ) -> Result<(), D9Code> {
        if level > 4 {
            return Err(D9Code::BAD_DEGRADE);
        }
        if drops_data {
            // 保全第一列红线：P0
            return Err(D9Code::DATA_LOSS_P0);
        }
        self.cells[DegradeMatrix::idx(dim, level)] = Some(DegradeAction {
            drops_data,
            readonly_link,
        });
        Ok(())
    }

    /// 查格。
    pub fn get(&self, dim: DegradeDim, level: DegradeLevel) -> Option<&DegradeAction> {
        if level > 4 {
            return None;
        }
        self.cells[DegradeMatrix::idx(dim, level)].as_ref()
    }

    /// 完整性（20 格全填）。
    pub fn complete(&self) -> Result<(), D9Code> {
        for c in &self.cells {
            if c.is_none() {
                return Err(D9Code::MATRIX_GAP);
            }
        }
        Ok(())
    }

    /// 保全列审计（任一格丢数据即 P0）。
    pub fn preservation_audit(&self) -> Result<(), D9Code> {
        for c in &self.cells {
            if let Some(a) = c {
                if a.drops_data {
                    return Err(D9Code::DATA_LOSS_P0);
                }
            }
        }
        Ok(())
    }
}

// ── 四源决策器 ──────────────────────────────────────────────────────

/// 决策源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Src {
    /// 帧耗。
    Frame,
    /// 内存。
    Mem,
    /// 功耗。
    Power,
    /// GPU。
    Gpu,
}

impl Src {
    pub const ALL: [Src; 4] = [Src::Frame, Src::Mem, Src::Power, Src::Gpu];
}

/// 决策器。
pub struct DegradeDecider {
    /// 缺源记录。
    missing: [bool; 4],
    /// 滞回阈值（×100）。
    hysteresis_x100: u32,
    /// 上次决策级。
    last_level: DegradeLevel,
}

impl DegradeDecider {
    pub fn new(hysteresis_x100: u32) -> DegradeDecider {
        DegradeDecider {
            missing: [false; 4],
            hysteresis_x100,
            last_level: 0,
        }
    }

    /// 标记缺源。
    pub fn mark_missing(&mut self, src: Src) {
        self.missing[src as usize] = true;
    }

    /// 决策（负载 ×100，缺源保守）。
    pub fn decide(&mut self, load_x100: u32) -> Result<DegradeLevel, D9Code> {
        // 缺源 → 保守（取较高档）
        let any_missing = self.missing.iter().any(|m| *m);
        let base: DegradeLevel = if load_x100 >= 900 {
            4
        } else if load_x100 >= 750 {
            3
        } else if load_x100 >= 500 {
            2
        } else if load_x100 >= 250 {
            1
        } else {
            0
        };
        let target = if any_missing && base < 4 {
            base.saturating_add(1)
        } else {
            base
        };
        // 滞回：变化小于阈值不动
        let delta = if target > self.last_level {
            target - self.last_level
        } else {
            self.last_level - target
        };
        if (delta as u32) * 100 < self.hysteresis_x100 {
            return Ok(self.last_level);
        }
        self.last_level = target;
        Ok(target)
    }

    /// 滞回参数。
    pub fn hysteresis_x100(&self) -> u32 {
        self.hysteresis_x100
    }
}

// ── 只读联动（复用 F3727） ──────────────────────────────────────────

/// 只读联动检查。
pub struct ReadonlyLink;

impl ReadonlyLink {
    /// 临界级（4）须只读联动。
    pub fn assert(level: DegradeLevel, readonly_active: bool) -> Result<(), D9Code> {
        if level == 4 && !readonly_active {
            Err(D9Code::READONLY_LINK_MISS)
        } else {
            Ok(())
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3729 判据集（锚点原文五条逐条落位）。
pub fn run_vr07i_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3729");

    // 1) 数据保全第一列：丢数据被 P0
    let mut mx = DegradeMatrix::new();
    out.add(
        "0x7801-保全第一列",
        mx.set(DegradeDim::Quality, 1, true, false).is_err(),
        "丢数据须 P0",
    );

    // 2) 矩阵：缺格被拦
    out.add(
        "0x7803-矩阵缺格",
        mx.complete().is_err(),
        "缺格须拦",
    );

    // 3) 矩阵：全填过
    let mut mx2 = DegradeMatrix::new();
    for d in DegradeDim::ALL {
        for l in 0..5u8 {
            let _ = mx2.set(d, l, false, l == 4);
        }
    }
    out.add(
        "0x7803-全填过",
        mx2.complete().is_ok() && mx2.preservation_audit().is_ok(),
        "全填且保全须过",
    );

    // 4) 四源决策：分级
    let mut dec = DegradeDecider::new(50);
    let lvl = dec.decide(950).unwrap_or(0);
    out.add(
        "0x7804-四源决策",
        lvl == 4,
        "高负载须 4 级",
    );

    // 5) 滞回：小变化不动
    let mut dec2 = DegradeDecider::new(200);
    let _ = dec2.decide(950); // → 4
    let lvl2 = dec2.decide(800).unwrap_or(4); // 想到 3 但滞回
    out.add(
        "0x7802-滞回复用",
        lvl2 == 4,
        "滞回须稳",
    );

    // 6) 源缺：保守
    let mut dec3 = DegradeDecider::new(0);
    dec3.mark_missing(Src::Gpu);
    let lvl3 = dec3.decide(100).unwrap_or(0);
    out.add(
        "0x7804-源缺保守",
        lvl3 >= 1,
        "缺源须保守",
    );

    // 7) 只读联动：临界须只读
    out.add(
        "0x7806-只读联动",
        ReadonlyLink::assert(4, false).is_err() && ReadonlyLink::assert(4, true).is_ok(),
        "临界须联动",
    );

    // 8) 保全审计：全表无丢
    out.add(
        "0x7801-保全审计",
        mx2.preservation_audit().is_ok(),
        "无丢须过",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7806-判据",
        D9Code::DATA_LOSS_P0.value() != D9Code::MATRIX_GAP.value(),
        "码段互异",
    );

    out
}
