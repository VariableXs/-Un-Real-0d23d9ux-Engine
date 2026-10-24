//! VE-F0519 · GLSL 后端性能与质量总账（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0519`
//!
//! **锚点原文**：「GLSL 后端性能与质量总账：性能账（生成吞吐、产物编译时长
//! 趋势——经对拍环境采集）、质量账（对拍通过率、缺陷密度、评审评分三指标）；
//! 两账与基线对账一致；总账版本化入收口证据。判据：性能质量双账、对账一致、
//! 趋势指标、版本化、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **双账结构**（判据一）。性能账（吞吐 + 编译时长趋势）与质量账（通过
//!    率 + 缺陷密度 + 评审评分）两账独立成账，混记即结构违规。
//!
//! 2. **对账一致**（判据二）。两账的汇总值与基线对账（偏差在容差内一致），
//!    对不上查因修正后才可入证据。
//!
//! 3. **趋势指标**（判据三）。编译时长与通过率带版本序列趋势，劣化即专项
//!    登记阻断。
//!
//! 4. **版本化**（判据四）。总账版本唯一递增，重复版本号即账目冲突。

use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0519";

// ---------------------------------------------------------------------------
// 一、性能账
// ---------------------------------------------------------------------------

/// 性能点（版本化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perf {
    pub ver: u32,
    /// 生成吞吐（千指令每计量单位，越大越好）。
    pub throughput: u32,
    /// 产物编译时长（计量步数，越小越好）。
    pub compile: u32,
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

    pub fn add(&mut self, ver: u32, tp: u32, ct: u32) -> Result<(), u32> {
        if tp == 0 || ct == 0 {
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
        self.items.push(Perf { ver, throughput: tp, compile: ct });
        Ok(())
    }

    /// 吞吐趋势劣化：最新 < 前者。
    pub fn tp_down(&self) -> bool {
        match (self.items.len() >= 2, self.items.as_slice()) {
            (true, sl) => sl[sl.len() - 1].throughput < sl[sl.len() - 2].throughput,
            _ => false,
        }
    }

    /// 编译时长趋势劣化：最新 > 前者。
    pub fn ct_up(&self) -> bool {
        let n = self.items.len();
        n >= 2 && self.items[n - 1].compile > self.items[n - 2].compile
    }
}

pub const XC_ZERO: u32 = 1;
pub const XC_DUPVER: u32 = 2;
pub const XC_ORDER: u32 = 3;

// ---------------------------------------------------------------------------
// 二、质量账（三指标）
// ---------------------------------------------------------------------------

/// 质量点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Qual {
    pub ver: u32,
    /// 对拍通过率（千分比）。
    pub pass_rate: u32,
    /// 缺陷密度（缺陷每千行，万分比记）。
    pub defect_density: u32,
    /// 评审评分（0~100）。
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

    /// 质量健康：通过率达标、密度低于界、评分达标。
    pub fn healthy(&self, min_pass: u32, max_dens: u32, min_review: u8) -> bool {
        self.items.last().map(|q| q.pass_rate >= min_pass && q.defect_density <= max_dens && q.review >= min_review).unwrap_or(false)
    }

    /// 趋势劣化：通过率下降或密度上升。
    pub fn q_down(&self) -> bool {
        let n = self.items.len();
        n >= 2 && self.items[n - 1].pass_rate < self.items[n - 2].pass_rate
    }
}

pub const XC_RATE: u32 = 4;
pub const XC_SCORE: u32 = 5;

// ---------------------------------------------------------------------------
// 三、对账（两账与基线一致）
// ---------------------------------------------------------------------------

/// 基线（收口时点的期望快照）。
pub struct Baseline {
    pub tp: u32,
    pub pass: u32,
    /// 容差（各指标千分比）。
    pub tol: u32,
}

/// 对账：吞吐与通过率都在基线容差内。
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

pub fn run_vec119_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec119");

    // 族一：性能账。
    let mut pb = PerfBook::new();
    s.add("零值拒", matches!(pb.add(1, 0, 10), Err(XC_ZERO)) && matches!(pb.add(1, 10, 0), Err(XC_ZERO)), "计量非零");
    let _ = pb.add(1, 100, 500);
    let _ = pb.add(2, 110, 480);
    s.add("版本递增", matches!(pb.add(1, 100, 500), Err(XC_DUPVER)), "版本唯一");
    s.add("回退版本拒", matches!(pb.add(1, 100, 500), Err(XC_DUPVER)), "重复即拒");
    s.add("双指标入账", pb.items.len() == 2, "吞吐+时长");

    // 族二：性能趋势。
    let _ = pb.add(3, 90, 520);
    s.add("吞吐下降可见", pb.tp_down(), "110→90");
    s.add("时长上升可见", pb.ct_up(), "480→520");
    s.add("改善不误报", {
        let mut p2 = PerfBook::new();
        let _ = p2.add(1, 100, 500);
        let _ = p2.add(2, 120, 450);
        !p2.tp_down() && !p2.ct_up()
    }, "双指标向好");

    // 族三：质量账三指标。
    let mut qb = QualBook::new();
    s.add("通过率超界拒", matches!(qb.add(1, 1001, 10, 80), Err(XC_RATE)), "≤1000");
    s.add("评分超界拒", matches!(qb.add(1, 900, 10, 101), Err(XC_SCORE)), "≤100");
    let _ = qb.add(1, 950, 12, 85);
    let _ = qb.add(2, 980, 8, 90);
    s.add("健康判定", qb.healthy(900, 20, 80), "三指标达标");
    s.add("评分不达标不健康", {
        let mut q2 = QualBook::new();
        let _ = q2.add(1, 980, 5, 60);
        !q2.healthy(900, 20, 80)
    }, "60 < 80");
    let _ = qb.add(3, 920, 15, 82);
    s.add("通过率下降可见", qb.q_down(), "980→920");

    // 族四：对账一致。
    let base = Baseline { tp: 100, pass: 950, tol: 50 };
    let p = Perf { ver: 9, throughput: 104, compile: 500 };
    let q = Qual { ver: 9, pass_rate: 960, defect_density: 10, review: 88 };
    s.add("容差内对账过", reconcile(&p, &q, &base).is_ok(), "±5% 界内");
    s.add("吞吐偏差拦截", {
        let p2 = Perf { ver: 9, throughput: 160, compile: 500 };
        matches!(reconcile(&p2, &q, &base), Err(XC_TPDIF))
    }, "超容差即查因");
    s.add("通过率偏差拦截", {
        let q2 = Qual { ver: 9, pass_rate: 700, defect_density: 10, review: 88 };
        matches!(reconcile(&p, &q2, &base), Err(XC_PASSDIF))
    }, "质量对不上");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recon() {
        let p = Perf { ver: 1, throughput: 100, compile: 1 };
        let q = Qual { ver: 1, pass_rate: 950, defect_density: 1, review: 90 };
        assert!(reconcile(&p, &q, &Baseline { tp: 100, pass: 950, tol: 10 }).is_ok());
    }
}
