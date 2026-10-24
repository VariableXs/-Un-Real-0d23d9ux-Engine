//! VE-F3584 · E 域内存账 —— 五段加总 + 峰值记录 + 泄漏入口 + 超限降级。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3584`
//!
//! # 职责（锚点原文拆解）
//!
//! - **五段加总**：令牌/换肤/壁纸/Persona/色温五段内存域级加总。
//! - **峰值记录**：内存水位峰值。
//! - **泄漏排查入口**。
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
// 一、诊断码（E21 段续：6860 起）
// ---------------------------------------------------------------------------

/// 内存账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemCode {
    /// 加总超限。
    TotalOverLimit = 6860,
    /// 峰值异常。
    PeakAnomaly = 6861,
    /// 泄漏线索。
    LeakHint = 6862,
}

impl MemCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            MemCode::TotalOverLimit => "内存加总超限",
            MemCode::PeakAnomaly => "峰值异常",
            MemCode::LeakHint => "泄漏线索",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, MemCode::TotalOverLimit)
    }
}

// ---------------------------------------------------------------------------
// 二、内存账
// ---------------------------------------------------------------------------

/// 单段内存。
#[derive(Clone, Copy, Debug)]
pub struct MemSegment {
    /// 段名 id。
    pub id: u32,
    /// 当前占用（KiB）。
    pub current_kib: u32,
    /// 峰值（KiB）。
    pub peak_kib: u32,
    /// 上限（KiB）。
    pub limit_kib: u32,
}

/// E 域内存账。
pub struct MemLedger {
    segments: Vec<MemSegment>,
}

impl MemLedger {
    /// 新建。
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// 记一段。
    pub fn record(&mut self, s: MemSegment) {
        self.segments.push(s);
    }

    /// 五段加总（当前）。
    pub fn total_current(&self) -> u32 {
        self.segments.iter().map(|s| s.current_kib).sum()
    }

    /// 五段峰值加总。
    pub fn total_peak(&self) -> u32 {
        self.segments.iter().map(|s| s.peak_kib).sum()
    }

    /// 超限段数。
    pub fn over_limit_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| s.current_kib > s.limit_kib)
            .count()
    }

    /// 泄漏线索（当前 > 80% 上限的段 id 列表）。
    pub fn leak_hints(&self) -> Vec<u32> {
        self.segments
            .iter()
            .filter(|s| s.current_kib.saturating_mul(5) > s.limit_kib.saturating_mul(4))
            .map(|s| s.id)
            .collect()
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

impl Default for MemLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 内存账判据集。
pub fn run_mem_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12d");

    // 1. 加总正确。
    let mut l = MemLedger::new();
    l.record(MemSegment {
        id: 1,
        current_kib: 100,
        peak_kib: 200,
        limit_kib: 500,
    });
    l.record(MemSegment {
        id: 2,
        current_kib: 300,
        peak_kib: 400,
        limit_kib: 500,
    });
    chk.add("6860-加总", l.total_current() == 400 && l.total_peak() == 600, "");

    // 2. 超限检出。
    let mut l2 = MemLedger::new();
    l2.record(MemSegment {
        id: 1,
        current_kib: 600,
        peak_kib: 600,
        limit_kib: 500,
    });
    chk.add("6861-超限检出", l2.over_limit_count() == 1, "");

    // 3. 泄漏线索（80% 以上）。
    let mut l3 = MemLedger::new();
    l3.record(MemSegment {
        id: 7,
        current_kib: 450,
        peak_kib: 450,
        limit_kib: 500,
    });
    let hints = l3.leak_hints();
    chk.add("6862-泄漏线索", hints.len() == 1 && hints[0] == 7, "");

    // 4. 安全段无线索。
    let mut l4 = MemLedger::new();
    l4.record(MemSegment {
        id: 1,
        current_kib: 100,
        peak_kib: 100,
        limit_kib: 500,
    });
    chk.add("6863-安全无线索", l4.leak_hints().is_empty(), "");

    // 5. 空账。
    chk.add("6864-空账", MemLedger::new().is_empty(), "");

    chk
}
