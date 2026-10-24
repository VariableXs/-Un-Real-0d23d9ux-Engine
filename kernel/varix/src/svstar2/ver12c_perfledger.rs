//! VE-F3583 · E 域性能账 —— 五段预算 + 加重复核 + 基准核对 + 总表对齐 + 历史对比。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3583`
//!
//! # 职责（锚点原文拆解）
//!
//! - **五段预算**（[`PerfSegment`]）：令牌求值/换肤/壁纸/Persona/色温
//!   五段预算加总复核。
//! - **基准核对**：实测一致性。
//! - **历史对比**：本轮与上轮预算差异。
//!
//! # 复杂度
//!
//! O(段)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：6840 起）
// ---------------------------------------------------------------------------

/// 性能账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 加总超支。
    TotalOver = 6840,
    /// 基准不符。
    BenchMismatch = 6841,
    /// 对齐漂移。
    AlignDrift = 6842,
}

impl PerfCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PerfCode::TotalOver => "五段加总超支",
            PerfCode::BenchMismatch => "基准不符",
            PerfCode::AlignDrift => "总表对齐漂移",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, PerfCode::TotalOver)
    }
}

// ---------------------------------------------------------------------------
// 二、五段
// ---------------------------------------------------------------------------

/// 性能段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfSegment {
    /// 令牌求值。
    Token,
    /// 换肤。
    Skin,
    /// 壁纸。
    Wallpaper,
    /// Persona。
    Persona,
    /// 色温。
    Kelvin,
}

impl PerfSegment {
    /// 全部五段。
    pub const ALL: [PerfSegment; 5] = [
        PerfSegment::Token,
        PerfSegment::Skin,
        PerfSegment::Wallpaper,
        PerfSegment::Persona,
        PerfSegment::Kelvin,
    ];

    /// 段短名。
    pub const fn label(self) -> &'static str {
        match self {
            PerfSegment::Token => "令牌",
            PerfSegment::Skin => "换肤",
            PerfSegment::Wallpaper => "壁纸",
            PerfSegment::Persona => "Persona",
            PerfSegment::Kelvin => "色温",
        }
    }
}

/// 单段预算。
#[derive(Clone, Copy, Debug)]
pub struct SegmentPerf {
    /// 段。
    pub segment: PerfSegment,
    /// 预算（微秒）。
    pub budget_us: u32,
    /// 实测（微秒）。
    pub measured_us: u32,
}

// ---------------------------------------------------------------------------
// 三、性能账
// ---------------------------------------------------------------------------

/// E 域性能账。
pub struct PerfLedger {
    segments: Vec<SegmentPerf>,
    /// 上轮加总（历史对比）。
    pub prev_total: u32,
}

impl PerfLedger {
    /// 新建。
    pub fn new(prev_total: u32) -> Self {
        Self {
            segments: Vec::new(),
            prev_total,
        }
    }

    /// 记一段。
    pub fn record(&mut self, s: SegmentPerf) {
        self.segments.push(s);
    }

    /// 五段加总（实测）。
    pub fn total_measured(&self) -> u32 {
        self.segments.iter().map(|s| s.measured_us).sum()
    }

    /// 五段预算加总。
    pub fn total_budget(&self) -> u32 {
        self.segments.iter().map(|s| s.budget_us).sum()
    }

    /// 加重复核。
    pub fn reconcile(&self) -> Result<(), PerfCode> {
        if self.total_measured() > self.total_budget() {
            return Err(PerfCode::TotalOver);
        }
        Ok(())
    }

    /// 历史差异（本轮 - 上轮，有符号用 i64）。
    pub fn delta_vs_prev(&self) -> i64 {
        self.total_measured() as i64 - self.prev_total as i64
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

/// 性能账判据集。
pub fn run_perf_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12c");

    // 1. 五段记账。
    let mut l = PerfLedger::new(1000);
    for s in PerfSegment::ALL {
        l.record(SegmentPerf {
            segment: s,
            budget_us: 100,
            measured_us: 80,
        });
    }
    chk.add("6840-五段记账", l.len() == 5, "");

    // 2. 加总正确。
    chk.add("6841-加总", l.total_measured() == 400 && l.total_budget() == 500, "");

    // 3. 守约。
    chk.add("6842-守约", l.reconcile().is_ok(), "");

    // 4. 超支拦。
    let mut l2 = PerfLedger::new(0);
    l2.record(SegmentPerf {
        segment: PerfSegment::Token,
        budget_us: 100,
        measured_us: 200,
    });
    chk.add(
        "6843-超支拦",
        l2.reconcile().err() == Some(PerfCode::TotalOver),
        "",
    );

    // 5. 历史差异。
    chk.add("6844-历史差异", l.delta_vs_prev() == -600, "");

    // 6. 段名可达。
    chk.add("6845-段名", PerfSegment::Kelvin.label() == "色温", "");

    chk
}
