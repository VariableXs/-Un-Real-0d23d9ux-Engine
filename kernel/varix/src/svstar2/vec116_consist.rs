//! VE-F0516 · GLSL 后端与软渲解释器一致性维护（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0516`
//!
//! **锚点原文**：「生成 GLSL 与软渲解释器一致性维护：双通路语义一致性长期
//! 维护机制（软渲解释器为参考语义，GLSL 生成产物抽样对拍常态化）、差异归因
//! 流程（解释器错、生成器错、规范歧义三分类）、一致性趋势追踪（差异率随
//! 版本趋势）。判据：参考语义、常态化对拍、三向归因、趋势追踪、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **参考语义**（判据一）。软渲解释器是唯一参考语义（单源），GLSL 产物
//!    与参考不同时默认产物错——参考地位在账目里显性（不是两凭感觉）。
//!
//! 2. **常态化对拍**（判据二）。对拍按周期批次运行（周期号驱动），每批
//!    抽样比对并记录差异清单——对拍是制度不是一次活动。
//!
//! 3. **三向归因**（判据三）。差异逐条归因：解释器错（转软渲缺陷账）、
//!    生成器错（本组修复）、规范歧义（转规范修订）；未归因差异不得销账。
//!
//! 4. **趋势追踪**（判据四）。差异率按版本记录，劣化（差异率上升超容差）
//!    即阻断登记。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0516";

/// 参考语义方（单源声明）。
pub const REFERENCE: &str = "softrender-interp";

// ---------------------------------------------------------------------------
// 一、常态对拍批次
// ---------------------------------------------------------------------------

/// 差异条目。
#[derive(Clone, Debug)]
pub struct Diff {
    pub case: String,
    /// 参考值与产物值（文本形态）。
    pub ref_v: String,
    pub got_v: String,
}

/// 对拍批次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycle {
    pub no: u32,
    pub samples: u32,
    pub diffs: u32,
}

impl Cycle {
    /// 差异率（千分比）。
    pub fn rate_permille(&self) -> u32 {
        if self.samples == 0 {
            return 0;
        }
        self.diffs * 1000 / self.samples
    }
}

/// 一致性账。
#[derive(Default)]
pub struct ConsBook {
    pub cycles: Vec<Cycle>,
}

impl ConsBook {
    pub fn new() -> ConsBook {
        ConsBook { cycles: Vec::new() }
    }

    /// 登记周期：周期号递增、样本非零。
    pub fn record(&mut self, no: u32, samples: u32, diffs: u32) -> Result<(), u32> {
        if samples == 0 {
            return Err(XC_NOSAMPLE);
        }
        if diffs > samples {
            return Err(XC_OVER);
        }
        if let Some(last) = self.cycles.last() {
            if no <= last.no {
                return Err(XC_ORDER);
            }
        }
        self.cycles.push(Cycle { no, samples, diffs });
        Ok(())
    }

    /// 劣化判定：最新周期差异率 > 前一周期的 1 + 容差。
    pub fn regressed(&self, tol_permille: u32) -> Option<(u32, u32)> {
        let n = self.cycles.len();
        if n < 2 {
            return None;
        }
        let a = self.cycles[n - 2].rate_permille() as u64;
        let b = self.cycles[n - 1].rate_permille() as u64;
        if b > a + tol_permille as u64 {
            Some((a as u32, b as u32))
        } else {
            None
        }
    }
}

pub const XC_NOSAMPLE: u32 = 1;
pub const XC_OVER: u32 = 2;
pub const XC_ORDER: u32 = 3;

// ---------------------------------------------------------------------------
// 二、三向归因
// ---------------------------------------------------------------------------

/// 归因三类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    InterpBug,
    GenBug,
    SpecAmb,
}

/// 归因去向。
pub fn route(c: Cause) -> &'static str {
    match c {
        Cause::InterpBug => "软渲缺陷账",
        Cause::GenBug => "本组修复",
        Cause::SpecAmb => "规范修订流程",
    }
}

/// 差异销账：必须先归因。
#[derive(Default)]
pub struct DispBook {
    pub done: Vec<(String, Cause)>,
}

impl DispBook {
    pub fn new() -> DispBook {
        DispBook { done: Vec::new() }
    }

    pub fn dispose(&mut self, case: &str, c: Cause) {
        self.done.push((case.to_string(), c));
    }

    /// 未归因残留。
    pub fn pending<'a>(&self, diffs: &'a [Diff]) -> Vec<&'a Diff> {
        diffs.iter().filter(|d| !self.done.iter().any(|(c, _)| c == &d.case)).collect()
    }
}

// ---------------------------------------------------------------------------
// 三、抽样比对（参考 vs 产物）
// ---------------------------------------------------------------------------

/// 双通路样例。
pub struct Sample {
    pub case: &'static str,
    pub ref_out: &'static str,
    pub got_out: &'static str,
}

/// 批量对拍：产出差异清单。
pub fn batch_diff(samples: &[Sample]) -> Vec<Diff> {
    samples
        .iter()
        .filter(|s| s.ref_out != s.got_out)
        .map(|s| Diff { case: s.case.to_string(), ref_v: s.ref_out.to_string(), got_v: s.got_out.to_string() })
        .collect()
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec116_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec116");

    // 族一：参考语义单源。
    s.add("参考为软渲解释器", REFERENCE == "softrender-interp", "单源声明在册");

    // 族二：常态对拍批次。
    let mut cb = ConsBook::new();
    s.add("零样本拒", matches!(cb.record(1, 0, 0), Err(XC_NOSAMPLE)), "样本必填");
    s.add("差异超样本拒", matches!(cb.record(1, 10, 11), Err(XC_OVER)), "率守恒");
    let _ = cb.record(1, 100, 5);
    s.add("周期号必须递增", matches!(cb.record(1, 100, 3), Err(XC_ORDER)), "时序账");
    let _ = cb.record(2, 100, 3);
    s.add("千分率计算", cb.cycles[0].rate_permille() == 50 && cb.cycles[1].rate_permille() == 30, "5% 与 3%");

    // 族三：趋势追踪。
    s.add("改善不判劣化", cb.regressed(10).is_none(), "50→30 下降");
    let _ = cb.record(3, 100, 20);
    s.add("劣化可见", cb.regressed(10) == Some((30, 200)), "30→200 超 10‰ 容差");
    let mut c3 = ConsBook::new();
    let _ = c3.record(1, 1000, 10);
    let _ = c3.record(2, 1000, 12);
    s.add("容差内放行", c3.regressed(5).is_none(), "10‰→12‰ 差 2");

    // 族四：三向归因与销账。
    let samples = [
        Sample { case: "s1", ref_out: "1.0", got_out: "1.0" },
        Sample { case: "s2", ref_out: "2.0", got_out: "2.5" },
        Sample { case: "s3", ref_out: "3.0", got_out: "3.0" },
        Sample { case: "s4", ref_out: "4.0", got_out: "0.4" },
    ];
    let diffs = batch_diff(&samples);
    s.add("差异清单精确", diffs.len() == 2 && diffs.iter().any(|d| d.case == "s2"), "只有 s2/s4");
    s.add("一致样例不入差异", !diffs.iter().any(|d| d.case == "s1"), "全等过滤");
    let mut db = DispBook::new();
    db.dispose("s2", Cause::GenBug);
    s.add("未归因残留", db.pending(&diffs).len() == 1, "s4 待归因");
    db.dispose("s4", Cause::InterpBug);
    s.add("归因后销清", db.pending(&diffs).is_empty(), "全部归因");
    s.add("三向去向", route(Cause::GenBug) == "本组修复" && route(Cause::InterpBug) == "软渲缺陷账" && route(Cause::SpecAmb) == "规范修订流程", "三类各归其位");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate() {
        let c = Cycle { no: 1, samples: 200, diffs: 2 };
        assert_eq!(c.rate_permille(), 10);
    }
}
