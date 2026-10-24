//! VE-F0538 · HLSL 性能与质量总账（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0538`
//!
//! **锚点原文**：「HLSL 后端性能与质量总账（F0519 同构复用）：性能账（生成
//! 吞吐、工具链编译时长趋势）、质量账（对拍通过率、缺陷密度、评审评分）；
//! 对账一致；版本化入收口证据。判据：双账汇总、对账一致、趋势指标、版本化、
//! 判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **双账汇总**（判据一）。性能账（吞吐 + 工具链编译时长）与质量账
//!    （通过率 + 缺陷密度 + 评审评分）独立成账、独立计数。
//!
//! 2. **对账一致**（判据二）。双账与基线在容差内对账一致，超容差即查因。
//!
//! 3. **趋势指标**（判据三）。吞吐下降与时长上升、通过率下降都显性可见
//!    （劣化即阻断登记）。
//!
//! 4. **版本化**（判据四）。总账版本唯一递增，重复版本拒。

use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0538";

/// 性能点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perf {
    pub ver: u32,
    pub throughput: u32,
    /// 工具链编译时长（计量步）。
    pub tool_time: u32,
}

/// 性能账。
#[derive(Default)]
pub struct PerfBook {
    pub items: Vec<Perf>,
}

impl PerfBook {
    pub fn new() -> PerfBook {
        PerfBook { items: Vec::new() }
    }

    pub fn add(&mut self, ver: u32, tp: u32, tt: u32) -> Result<(), u32> {
        if tp == 0 || tt == 0 {
            return Err(XC_ZERO);
        }
        if self.items.iter().any(|p| p.ver == ver) {
            return Err(XC_DUPVER);
        }
        if let Some(l) = self.items.last() {
            if ver <= l.ver {
                return Err(XC_ORDER);
            }
        }
        self.items.push(Perf { ver, throughput: tp, tool_time: tt });
        Ok(())
    }

    pub fn tp_down(&self) -> bool {
        let n = self.items.len();
        n >= 2 && self.items[n - 1].throughput < self.items[n - 2].throughput
    }

    pub fn tt_up(&self) -> bool {
        let n = self.items.len();
        n >= 2 && self.items[n - 1].tool_time > self.items[n - 2].tool_time
    }
}

/// 质量点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Qual {
    pub ver: u32,
    pub pass_rate: u32,
    pub defect_density: u32,
    pub review: u8,
}

/// 质量账。
#[derive(Default)]
pub struct QualBook {
    pub items: Vec<Qual>,
}

impl QualBook {
    pub fn new() -> QualBook {
        QualBook { items: Vec::new() }
    }

    pub fn add(&mut self, ver: u32, pass: u32, dens: u32, review: u8) -> Result<(), u32> {
        if pass > 1000 {
            return Err(XC_RATE);
        }
        if review > 100 {
            return Err(XC_SCORE);
        }
        if self.items.iter().any(|q| q.ver == ver) {
            return Err(XC_DUPVER);
        }
        self.items.push(Qual { ver, pass_rate: pass, defect_density: dens, review });
        Ok(())
    }

    pub fn healthy(&self, min_pass: u32, max_dens: u32, min_review: u8) -> bool {
        self.items.last().map(|q| q.pass_rate >= min_pass && q.defect_density <= max_dens && q.review >= min_review).unwrap_or(false)
    }

    pub fn q_down(&self) -> bool {
        let n = self.items.len();
        n >= 2 && self.items[n - 1].pass_rate < self.items[n - 2].pass_rate
    }
}

pub const XC_ZERO: u32 = 1;
pub const XC_DUPVER: u32 = 2;
pub const XC_ORDER: u32 = 3;
pub const XC_RATE: u32 = 4;
pub const XC_SCORE: u32 = 5;

/// 基线对账。
pub struct Baseline {
    pub tp: u32,
    pub pass: u32,
    pub tol: u32,
}

pub fn reconcile(p: &Perf, q: &Qual, b: &Baseline) -> Result<(), u32> {
    let ok_tp = (p.throughput as i64 - b.tp as i64).abs() * 1000 <= b.tp as i64 * b.tol as i64;
    let ok_pass = (q.pass_rate as i64 - b.pass as i64).abs() * 1000 <= b.pass as i64 * b.tol as i64;
    if !ok_tp {
        return Err(XC_TPDIF);
    }
    if !ok_pass {
        return Err(XC_PASSDIF);
    }
    Ok(())
}

pub const XC_TPDIF: u32 = 6;
pub const XC_PASSDIF: u32 = 7;

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec138_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec138");

    // 族一：性能账。
    let mut pb = PerfBook::new();
    s.add("零值拒", matches!(pb.add(1, 0, 10), Err(XC_ZERO)) && matches!(pb.add(1, 10, 0), Err(XC_ZERO)), "计量非零");
    let _ = pb.add(1, 100, 400);
    let _ = pb.add(2, 110, 380);
    s.add("版本唯一", matches!(pb.add(1, 100, 400), Err(XC_DUPVER)), "重复拒");
    s.add("版本递增", matches!(pb.add(0, 90, 500), Err(XC_ORDER)), "回退拒");
    s.add("双指标在账", pb.items.len() == 2, "吞吐+工具时长");

    // 族二：趋势。
    let _ = pb.add(3, 95, 420);
    s.add("吞吐下降可见", pb.tp_down(), "110→95");
    s.add("时长上升可见", pb.tt_up(), "380→420");
    s.add("向好不误报", {
        let mut p2 = PerfBook::new();
        let _ = p2.add(1, 100, 500);
        let _ = p2.add(2, 130, 400);
        !p2.tp_down() && !p2.tt_up()
    }, "双指标向好");

    // 族三：质量账。
    let mut qb = QualBook::new();
    s.add("通过率超界拒", matches!(qb.add(1, 1200, 5, 80), Err(XC_RATE)), "≤1000");
    s.add("评分超界拒", matches!(qb.add(1, 900, 5, 200), Err(XC_SCORE)), "≤100");
    let _ = qb.add(1, 940, 15, 85);
    let _ = qb.add(2, 970, 10, 90);
    s.add("健康判定", qb.healthy(900, 20, 80), "三指标达标");
    let _ = qb.add(3, 910, 18, 82);
    s.add("通过率下降可见", qb.q_down(), "970→910");

    // 族四：对账。
    let b = Baseline { tp: 100, pass: 950, tol: 50 };
    let p = Perf { ver: 9, throughput: 102, tool_time: 400 };
    let q = Qual { ver: 9, pass_rate: 955, defect_density: 10, review: 88 };
    s.add("容差内过", reconcile(&p, &q, &b).is_ok(), "±5%");
    s.add("吞吐偏差拦", matches!(reconcile(&Perf { ver: 9, throughput: 180, tool_time: 400 }, &q, &b), Err(XC_TPDIF)), "超容差");
    s.add("通过率偏差拦", matches!(reconcile(&p, &Qual { ver: 9, pass_rate: 600, defect_density: 10, review: 88 }, &b), Err(XC_PASSDIF)), "对不上查因");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recon() {
        let p = Perf { ver: 1, throughput: 100, tool_time: 1 };
        let q = Qual { ver: 1, pass_rate: 950, defect_density: 1, review: 90 };
        assert!(reconcile(&p, &q, &Baseline { tp: 100, pass: 950, tol: 10 }).is_ok());
    }
}
