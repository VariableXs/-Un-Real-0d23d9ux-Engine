//! VE-F3954 · 无障碍兼容矩阵
//!
//! 全域无障碍兼容矩阵（复用矩阵制度全域化）：S 域特性 × 平台 × 辅具 ×
//! 读屏器四维全域矩阵。矩阵测试全域化；矩阵发布公开红线全域化（不公开
//! →P1）；矩阵维护频率全域化（季度刷新，超期→立案）。
//! 错误路径：缺格→补测；不公开→P1；超期→立案；失配→协商。
//! 性能：矩阵 O(四维)/季度；测试 O(组合全域)；公开 O(断言全域)；维护
//! O(全域)。
//! 跨批对接：F3858/F2939 复用单源全域声明；辅具/读屏厂商对端；F3942
//! 调度消费。无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x29C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x29C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8NCode(pub u16);

impl S8NCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const CELL_GAP: Self = Self(SEG_BASE + 1);
    pub const NOT_PUBLIC: Self = Self(SEG_BASE + 2);
    pub const OVERDUE: Self = Self(SEG_BASE + 3);
    pub const MISMATCH: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CELL_GAP => "缺格→补测",
            Self::NOT_PUBLIC => "不公开→P1",
            Self::OVERDUE => "超期→立案",
            Self::MISMATCH => "失配→协商",
            _ => "未知",
        }
    }
}

// ── 四维矩阵 ─────────────────────────────────────────────────────────

/// 矩阵四维定义（维度容量全域常量）。
pub const DIM_FEATURES: usize = 4;
pub const DIM_PLATFORMS: usize = 3;
pub const DIM_AIDS: usize = 3;
pub const DIM_READERS: usize = 3;

/// 矩阵总格数。
pub const CELL_TOTAL: usize = DIM_FEATURES * DIM_PLATFORMS * DIM_AIDS * DIM_READERS;

/// 格状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellState {
    /// 未测。
    Untested,
    /// 通过。
    Pass,
    /// 失配（需协商）。
    Fail,
}

/// 四维矩阵：扁平存储 + 下标换算。
#[derive(Debug, Clone)]
pub struct CompatMatrix {
    pub cells: [CellState; CELL_TOTAL],
    /// 最近测试季度号。
    pub tested_quarter: u32,
    /// 是否已发布（公开红线）。
    pub published: bool,
}

impl CompatMatrix {
    pub fn new() -> Self {
        Self { cells: [CellState::Untested; CELL_TOTAL], tested_quarter: 0, published: false }
    }

    /// 四维下标 → 扁平下标（行主序）。
    pub fn index(feature: usize, platform: usize, aid: usize, reader: usize) -> Option<usize> {
        if feature >= DIM_FEATURES || platform >= DIM_PLATFORMS || aid >= DIM_AIDS || reader >= DIM_READERS {
            return None;
        }
        Some(((feature * DIM_PLATFORMS + platform) * DIM_AIDS + aid) * DIM_READERS + reader)
    }

    /// 读格。
    pub fn get(&self, f: usize, p: usize, a: usize, r: usize) -> Option<CellState> {
        Self::index(f, p, a, r).map(|i| self.cells[i])
    }

    /// 写格。
    pub fn set(&mut self, f: usize, p: usize, a: usize, r: usize, s: CellState) -> S8NCode {
        match Self::index(f, p, a, r) {
            Some(i) => {
                self.cells[i] = s;
                S8NCode::OK
            }
            None => S8NCode::CELL_GAP,
        }
    }

    /// 缺格检出：未测格数。
    pub fn untested(&self) -> Vec<usize> {
        self.cells.iter().enumerate().filter(|(_, c)| **c == CellState::Untested).map(|(i, _)| i).collect()
    }

    /// 失配格清单（需协商）。
    pub fn failures(&self) -> Vec<usize> {
        self.cells.iter().enumerate().filter(|(_, c)| **c == CellState::Fail).map(|(i, _)| i).collect()
    }

    /// 组合全域：全部格都有结论（Pass 或 Fail）。
    pub fn fully_tested(&self) -> bool {
        self.untested().is_empty()
    }

    /// 公开红线：已发布。
    pub fn is_public(&self) -> bool {
        self.published
    }

    /// 季度超期立案：距上次测试超过 1 季度（4 个季度周期内必须刷新）。
    pub fn overdue(&self, current_quarter: u32) -> bool {
        current_quarter.saturating_sub(self.tested_quarter) > 1
    }
}

impl Default for CompatMatrix {
    fn default() -> Self {
        Self::new()
    }
}

// ── 补测与协商 ───────────────────────────────────────────────────────

/// 补测计划：按未测格生成最小补测批。
pub fn patch_plan(m: &CompatMatrix) -> Vec<usize> {
    m.untested()
}

/// 失配协商记录：格 → 协商结果（重测通过/降级披露）。
#[derive(Debug, Clone)]
pub struct Negotiation {
    pub cell: usize,
    pub resolved: CellState,
    pub note: &'static str,
}

pub fn negotiate(cell: usize, resolved: CellState, note: &'static str) -> Negotiation {
    Negotiation { cell, resolved, note }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves208_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves208_s08matrix");

    // 01 四维容量守恒（4×3×3×3=108 格）
    {
        chk(&mut set, "C3954-01", CELL_TOTAL == 108 && DIM_FEATURES == 4 && DIM_PLATFORMS == 3 && DIM_AIDS == 3 && DIM_READERS == 3, "err");
    }
    // 02 下标换算双射性抽查
    {
        let i1 = CompatMatrix::index(0, 0, 0, 0);
        let i2 = CompatMatrix::index(3, 2, 2, 2);
        let i3 = CompatMatrix::index(1, 1, 1, 1);
        chk(&mut set, "C3954-02", i1 == Some(0) && i2 == Some(107) && i3 == Some((1 * 3 + 1) * 3 + 1), "err");
    }
    // 03 越界下标拒收
    {
        chk(&mut set, "C3954-03", CompatMatrix::index(4, 0, 0, 0).is_none() && CompatMatrix::index(0, 0, 0, 3).is_none(), "err");
    }
    // 04 写格读格一致
    {
        let mut m = CompatMatrix::new();
        chk(&mut set, "C3954-04", m.set(2, 1, 0, 2, CellState::Pass) == S8NCode::OK && m.get(2, 1, 0, 2) == Some(CellState::Pass), "err");
    }
    // 05 缺格检出：初始全未测
    {
        let m = CompatMatrix::new();
        chk(&mut set, "C3954-05", m.untested().len() == CELL_TOTAL && !m.fully_tested(), "err");
    }
    // 06 补测计划 = 未测格
    {
        let mut m = CompatMatrix::new();
        for f in 0..DIM_FEATURES {
            for p in 0..DIM_PLATFORMS {
                for a in 0..DIM_AIDS {
                    for r in 0..DIM_READERS {
                        if (f + p + a + r) % 3 != 0 {
                            m.set(f, p, a, r, CellState::Pass);
                        }
                    }
                }
            }
        }
        let plan = patch_plan(&m);
        chk(&mut set, "C3954-06", plan.len() == m.untested().len() && !m.fully_tested(), "err");
    }
    // 07 全格补测后组合全域
    {
        let mut m = CompatMatrix::new();
        for f in 0..DIM_FEATURES {
            for p in 0..DIM_PLATFORMS {
                for a in 0..DIM_AIDS {
                    for r in 0..DIM_READERS {
                        m.set(f, p, a, r, CellState::Pass);
                    }
                }
            }
        }
        chk(&mut set, "C3954-07", m.fully_tested() && m.untested().is_empty(), "err");
    }
    // 08 失配格 → 协商记录 → 回写
    {
        let mut m = CompatMatrix::new();
        m.set(1, 0, 2, 1, CellState::Fail);
        let fails = m.failures();
        let n = negotiate(fails[0], CellState::Pass, "厂商对齐后重测通过");
        m.cells[n.cell] = n.resolved;
        chk(&mut set, "C3954-08", m.failures().is_empty() && m.get(1, 0, 2, 1) == Some(CellState::Pass), "err");
    }
    // 09 公开红线：未发布 = P1
    {
        let m = CompatMatrix::new();
        chk(&mut set, "C3954-09", !m.is_public(), "err");
    }
    // 10 发布断言
    {
        let mut m = CompatMatrix::new();
        m.published = true;
        chk(&mut set, "C3954-10", m.is_public(), "err");
    }
    // 11 季度频率：未超期
    {
        let mut m = CompatMatrix::new();
        m.tested_quarter = 10;
        chk(&mut set, "C3954-11", !m.overdue(10) && !m.overdue(11), "err");
    }
    // 12 季度超期→立案
    {
        let mut m = CompatMatrix::new();
        m.tested_quarter = 10;
        chk(&mut set, "C3954-12", m.overdue(12) && m.overdue(20), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3954-13", S8NCode::MISMATCH.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8NCode::OK, S8NCode::CELL_GAP, S8NCode::NOT_PUBLIC, S8NCode::OVERDUE, S8NCode::MISMATCH];
        chk(&mut set, "C3954-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_matrix_all_green() {
        let s = run_ves208_checks();
        assert!(s.all_passed(), "ves208 red: {}", s.red_items_text());
    }

    #[test]
    fn test_index_bijective() {
        // 全格下标互不冲突（双射）。
        let mut seen = [false; CELL_TOTAL];
        for f in 0..DIM_FEATURES {
            for p in 0..DIM_PLATFORMS {
                for a in 0..DIM_AIDS {
                    for r in 0..DIM_READERS {
                        let i = CompatMatrix::index(f, p, a, r).unwrap();
                        assert!(!seen[i], "index collision at {}", i);
                        seen[i] = true;
                    }
                }
            }
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn test_quarter_overdue_boundary() {
        let mut m = CompatMatrix::new();
        m.tested_quarter = 5;
        assert!(!m.overdue(6));
        assert!(m.overdue(7));
    }
}
