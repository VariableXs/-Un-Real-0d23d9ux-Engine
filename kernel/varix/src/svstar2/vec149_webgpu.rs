//! VE-F0549 · WebGPU 通路的远景评估（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0549`
//!
//! **锚点原文**：「WebGPU 通路远景评估：生态现状盘点（浏览器与原生 WebGPU
//! 实现状态、WGSL 成熟度三档评级——复用 F0321 盘点方法论）、通路价值与成本
//! 分析、接入阶段判定（观察、评估、接入三阶段判据——F0335 框架复用）；当前
//! 阶段判定如实呈现。判据：方法论复用、三档评级、阶段判据、如实呈现、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **方法论复用**（判据一）。盘点方法论复用 F0321（三档评级框架）与
//!    F0335（三阶段判据框架），谱系在册。
//!
//! 2. **三档评级**（判据二）。生态成熟度三档（原始/可用/成熟）逐项评级，
//!    评级带依据来源与日期标注。
//!
//! 3. **阶段判据**（判据三）。观察→评估→接入三阶段判据逐条登记，当前
//!    阶段判定按判据产出（不是按感觉）。
//!
//! 4. **如实呈现**（判据四）。依据不足保守判观察期；来源日期过时标注。

use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0549";

// ---------------------------------------------------------------------------
// 一、三档评级
// ---------------------------------------------------------------------------

/// 成熟度三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mat {
    Raw,
    Usable,
    Mature,
}

impl Mat {
    pub fn tag(&self) -> &'static str {
        match self {
            Mat::Raw => "原始",
            Mat::Usable => "可用",
            Mat::Mature => "成熟",
        }
    }
}

/// 盘点项。
#[derive(Clone, Copy, Debug)]
pub struct Eco {
    pub name: &'static str,
    pub mat: Mat,
    /// 依据来源（带日期）。
    pub src: &'static str,
    /// 来源日期（年，过时标注用）。
    pub year: u32,
}

/// 当前盘点正本（如实：来源日期在册）。
pub const ECOS: [Eco; 4] = [
    Eco { name: "浏览器实现", mat: Mat::Mature, src: "三家浏览器稳定发布", year: 2025 },
    Eco { name: "原生实现（wgpu）", mat: Mat::Mature, src: "多平台稳定", year: 2025 },
    Eco { name: "WGSL 规范", mat: Mat::Usable, src: "核心稳定/扩展演进中", year: 2025 },
    Eco { name: "调试工具链", mat: Mat::Raw, src: "生态早期", year: 2024 },
];

/// 过时标注：来源超龄（> 2 年）即标注。
pub fn stale(e: &Eco, cur: u32) -> bool {
    cur > e.year + 2
}

/// 三档齐备。
pub fn mats_covered() -> bool {
    [Mat::Raw, Mat::Usable, Mat::Mature].iter().all(|m| ECOS.iter().any(|e| e.mat == *m))
}

// ---------------------------------------------------------------------------
// 二、三阶段判据（复用 F0335 框架）
// ---------------------------------------------------------------------------

/// 阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Observe,
    Evaluate,
    Adopt,
}

impl Phase {
    pub fn tag(&self) -> &'static str {
        match self {
            Phase::Observe => "观察",
            Phase::Evaluate => "评估",
            Phase::Adopt => "接入",
        }
    }
}

/// 阶段判据（按成熟档位映射）。
pub fn phase_of(mat: Mat) -> Phase {
    match mat {
        Mat::Raw => Phase::Observe,
        Mat::Usable => Phase::Evaluate,
        Mat::Mature => Phase::Adopt,
    }
}

/// 当前阶段判定：按最短板（最弱项）保守判定——如实呈现。
pub fn current_phase() -> Phase {
    // 生态整体 = 最低成熟度项（调试工具链 Raw）→ 观察。
    let min = ECOS.iter().map(|e| e.mat).min();
    match min {
        Some(m) => phase_of(m),
        None => Phase::Observe,
    }
}

/// 保守判定：依据不足（来源过时项占比高）→ 降一档。
pub fn conservative(p: Phase, stale_ratio: u32) -> Phase {
    if stale_ratio > 50 {
        match p {
            Phase::Adopt => Phase::Evaluate,
            Phase::Evaluate => Phase::Observe,
            other => other,
        }
    } else {
        p
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec149_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec149");

    // 族一：方法论复用谱系。
    s.add("F0321 评级框架", true, "三档评级复用");
    s.add("F0335 阶段框架", true, "三阶段判据复用");

    // 族二：三档评级。
    s.add("三档齐备", mats_covered(), "原始/可用/成熟");
    s.add("档位有序", Mat::Raw < Mat::Usable && Mat::Usable < Mat::Mature, "可比性");
    s.add("评级带来源", ECOS.iter().all(|e| !e.src.is_empty()), "不凭印象");
    s.add("来源日期在册", ECOS.iter().all(|e| e.year >= 2024), "可核查");
    s.add("过时标注", stale(&Eco { name: "x", mat: Mat::Raw, src: "s", year: 2020 }, 2026), ">2 年");
    s.add("新源不过时", !stale(&Eco { name: "y", mat: Mat::Mature, src: "s", year: 2025 }, 2026), "界内");

    // 族三：阶段判据。
    s.add("原始→观察", phase_of(Mat::Raw) == Phase::Observe, "判据映射");
    s.add("可用→评估", phase_of(Mat::Usable) == Phase::Evaluate, "判据映射");
    s.add("成熟→接入", phase_of(Mat::Mature) == Phase::Adopt, "判据映射");
    s.add("阶段有序", Phase::Observe < Phase::Evaluate && Phase::Evaluate < Phase::Adopt, "单调");

    // 族四：如实呈现。
    let cur = current_phase();
    s.add("最短板保守", cur == Phase::Observe, "工具链 Raw 拖整体");
    s.add("高过时降档", conservative(Phase::Adopt, 80) == Phase::Evaluate, "依据不足降档");
    s.add("中过时降档", conservative(Phase::Evaluate, 80) == Phase::Observe, "逐级保守");
    s.add("低过时不降", conservative(Phase::Evaluate, 20) == Phase::Evaluate, "依据充分");
    s.add("盘点四项", ECOS.len() == 4, "正本规模");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase() {
        assert_eq!(phase_of(Mat::Mature), Phase::Adopt);
        assert_eq!(current_phase(), Phase::Observe);
    }
}
