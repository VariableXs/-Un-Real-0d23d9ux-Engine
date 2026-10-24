//! VE-F0533 · HLSL 一致性套件（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0533`
//!
//! **锚点原文**：「HLSL 一致性测试套件：功能层（全族生成用例）、对拍层
//! （F0529 双级对拍样例集）、回归层（随修入册）三层；双档运行（fast 文本
//! 快照、full 工具链回读——工具链缺席 N/A 显性随闸门登记）；结果聚合入
//! 收口。判据：三层用例、双档加闸门、随修入册、N/A 显性、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三层用例**（判据一）。功能/对拍/回归三层独立入册计数，缺层即
//!    套件不完整。
//!
//! 2. **双档加闸门**（判据二）。fast 文本快照、full 工具链回读双档；full
//!    档工具链缺席 N/A 显性并随闸门登记（不冒充通过）。
//!
//! 3. **随修入册**（判据三）。回归用例带修复引用，孤儿拒收。
//!
//! 4. **聚合入收口**（判据四）。套件结果产出聚合形态（层计数 + 通过数 +
//!    N/A 数 + 待闸门数）供 F0540 消费。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0533";

/// 层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Func,
    Diff,
    Regress,
}

/// 用例。
#[derive(Clone, Debug)]
pub struct TCase {
    pub tier: Tier,
    pub name: String,
    pub fix_ref: Option<u32>,
    pub fast_ok: bool,
    /// full 档：Some(bool) 或 None（N/A 随闸门）。
    pub full_ok: Option<bool>,
}

/// 套件。
#[derive(Default)]
pub struct Suite {
    pub cases: Vec<TCase>,
}

impl Suite {
    pub fn new() -> Suite {
        Suite { cases: Vec::new() }
    }

    pub fn add(&mut self, tier: Tier, name: &str, fix_ref: Option<u32>, fast: bool, full: Option<bool>) -> Result<(), u32> {
        if tier == Tier::Regress && fix_ref.is_none() {
            return Err(XC_ORPHAN);
        }
        if name.is_empty() {
            return Err(XC_NONAME);
        }
        if self.cases.iter().any(|c| c.name == name) {
            return Err(XC_DUP);
        }
        self.cases.push(TCase { tier, name: name.to_string(), fix_ref, fast_ok: fast, full_ok: full });
        Ok(())
    }

    pub fn count(&self, t: Tier) -> usize {
        self.cases.iter().filter(|c| c.tier == t).count()
    }

    pub fn complete(&self) -> bool {
        self.count(Tier::Func) > 0 && self.count(Tier::Diff) > 0 && self.count(Tier::Regress) > 0
    }

    /// 双档运行：full 档 None 项按 fast 判（N/A 不计败）。
    pub fn run(&self, full: bool) -> Result<bool, u32> {
        if !self.complete() {
            return Err(XC_NOLAYER);
        }
        Ok(self.cases.iter().all(|c| {
            if full {
                match c.full_ok {
                    Some(ok) => ok && c.fast_ok,
                    None => c.fast_ok,
                }
            } else {
                c.fast_ok
            }
        }))
    }

    pub fn na(&self) -> usize {
        self.cases.iter().filter(|c| c.full_ok.is_none()).count()
    }
}

pub const XC_ORPHAN: u32 = 1;
pub const XC_NONAME: u32 = 2;
pub const XC_DUP: u32 = 3;
pub const XC_NOLAYER: u32 = 4;

/// HLSL 功能层族前缀。
pub const FAM_PREFIX: [&str; 4] = ["arith", "tex", "buf", "flow"];

pub fn fam_covered(suite: &Suite) -> bool {
    FAM_PREFIX.iter().all(|p| suite.cases.iter().any(|c| c.tier == Tier::Func && c.name.starts_with(p)))
}

/// 聚合（供 F0540 收口消费）。
pub struct Agg {
    pub layers: [usize; 3],
    pub na_count: usize,
    pub gate_pending: usize,
}

pub fn aggregate(suite: &Suite, gate_pending: usize) -> Agg {
    Agg {
        layers: [suite.count(Tier::Func), suite.count(Tier::Diff), suite.count(Tier::Regress)],
        na_count: suite.na(),
        gate_pending,
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec133_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec133");

    let mut su = Suite::new();

    // 族一：三层入册。
    s.add("孤儿拒", matches!(su.add(Tier::Regress, "r", None, true, Some(true)), Err(XC_ORPHAN)), "随修入册");
    let _ = su.add(Tier::Func, "arith-mad", None, true, Some(true));
    let _ = su.add(Tier::Diff, "diff-g1", None, true, Some(true));
    let _ = su.add(Tier::Regress, "reg-loopattr", Some(202), true, Some(true));
    s.add("三层齐", su.complete(), "独立计数");
    s.add("重名拒", matches!(su.add(Tier::Func, "arith-mad", None, true, Some(true)), Err(XC_DUP)), "用例唯一");

    // 族二：族覆盖。
    s.add("族未满", !fam_covered(&su), "缺 tex/buf/flow");
    let _ = su.add(Tier::Func, "tex-sample", None, true, Some(true));
    let _ = su.add(Tier::Func, "buf-load", None, true, Some(true));
    let _ = su.add(Tier::Func, "flow-loop", None, true, Some(true));
    s.add("四族覆盖", fam_covered(&su), "全族用例");

    // 族三：双档与 N/A。
    let _ = su.add(Tier::Func, "arith-add", None, true, None);
    s.add("fast 全过", su.run(false) == Ok(true), "快档独立");
    s.add("full 含 N/A 仍过", su.run(true) == Ok(true), "N/A 不计败");
    s.add("N/A 显性计数", su.na() == 1, "随闸门");
    s.add("full 失败算败", {
        let mut su2 = Suite::new();
        let _ = su2.add(Tier::Func, "f", None, true, Some(false));
        let _ = su2.add(Tier::Diff, "d", None, true, Some(true));
        let _ = su2.add(Tier::Regress, "r", Some(1), true, Some(true));
        su2.run(true) == Ok(false)
    }, "Some(false) 拦截");
    s.add("缺层拒跑", matches!(Suite::new().run(false), Err(XC_NOLAYER)), "完整性前置");

    // 族四：聚合入收口。
    let g = 1usize;
    let agg = aggregate(&su, g);
    s.add("层计数", agg.layers == [4, 1, 1], "三清单");
    s.add("N/A 入聚合", agg.na_count == 1, "显性债务");
    s.add("闸门数入聚合", agg.gate_pending == 1, "待实机验证");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite() {
        let mut su = Suite::new();
        assert!(matches!(su.add(Tier::Regress, "r", None, true, Some(true)), Err(XC_ORPHAN)));
        let _ = su.add(Tier::Func, "f", None, true, Some(true));
        let _ = su.add(Tier::Diff, "d", None, true, Some(true));
        let _ = su.add(Tier::Regress, "r", Some(1), true, Some(true));
        assert_eq!(su.run(false), Ok(true));
    }
}
