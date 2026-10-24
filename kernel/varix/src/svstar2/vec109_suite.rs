//! VE-F0509 · GLSL 后端一致性套件（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0509`
//!
//! **锚点原文**：「GLSL 后端一致性测试套件：功能层（全族指令生成用例）、
//! 对拍层（F0506 双级对拍样例集）、回归层（历史问题回归用例随修复入册）
//! 三层；fast 档（文本快照对拍）与 full 档（驱动回读执行）双档运行。判据：
//! 三层用例、双档、随修入册、N/A 语义、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三层用例**（判据一）。功能层（每指令族至少一生成用例）、对拍层
//!    （双级样例）、回归层（修复即入册）三层清单独立计数，缺层即套件
//!    不完整。
//!
//! 2. **双档运行**（判据二）。fast 档只做文本快照比对，full 档追加回读
//!    执行；档位选择显性（结果带档位标注），不宣称运行了没跑的档。
//!
//! 3. **随修入册**（判据三）。回归用例必须带修复引用（问题号），无引用
//!    的回归用例是孤儿，拒入册。
//!
//! 4. **N/A 语义**（判据四）。full 档在无环境时整档 N/A 显性标注，fast
//!    档结果不受影响——N/A 不计入失败也不计入通过。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0509";

// ---------------------------------------------------------------------------
// 一、三层用例
// ---------------------------------------------------------------------------

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
    /// 回归层专用：修复引用（问题号）。其他层为空。
    pub fix_ref: Option<u32>,
    /// fast 档结果。
    pub fast_ok: bool,
    /// full 档结果（None = 未跑/N/A）。
    pub full_ok: Option<bool>,
}

/// 套件账。
#[derive(Default)]
pub struct Suite {
    pub cases: Vec<TCase>,
}

impl Suite {
    pub fn new() -> Suite {
        Suite { cases: Vec::new() }
    }

    /// 入册：回归用例必须带修复引用（随修入册，孤儿拒收）。
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
        self.cases.push(TCase {
            tier,
            name: name.to_string(),
            fix_ref,
            fast_ok: fast,
            full_ok: full,
        });
        Ok(())
    }

    /// 层计数。
    pub fn count(&self, t: Tier) -> usize {
        self.cases.iter().filter(|c| c.tier == t).count()
    }

    /// 三层齐备。
    pub fn tiers_complete(&self) -> bool {
        self.count(Tier::Func) > 0 && self.count(Tier::Diff) > 0 && self.count(Tier::Regress) > 0
    }

    /// 档位运行：fast 档全部用例判 fast_ok；full 档只判 full_ok 为 Some(true)
    /// 的（None = N/A 不计失败）。
    pub fn run(&self, full: bool) -> Result<bool, u32> {
        if !self.tiers_complete() {
            return Err(XC_NOLAYER);
        }
        Ok(self.cases.iter().all(|c| {
            if full {
                match c.full_ok {
                    Some(ok) => ok && c.fast_ok,
                    None => c.fast_ok, // N/A：fast 仍须过
                }
            } else {
                c.fast_ok
            }
        }))
    }

    /// N/A 计数（full 档未跑的用例）。
    pub fn na(&self) -> usize {
        self.cases.iter().filter(|c| c.full_ok.is_none()).count()
    }
}

pub const XC_ORPHAN: u32 = 1;
pub const XC_NONAME: u32 = 2;
pub const XC_DUP: u32 = 3;
pub const XC_NOLAYER: u32 = 4;

// ---------------------------------------------------------------------------
// 二、全族功能用例登记（五族至少各一）
// ---------------------------------------------------------------------------

/// 功能层族覆盖校验：五族名前缀各至少一用例。
pub const FAM_PREFIX: [&str; 5] = ["arith", "bit", "cmp", "tex", "res"];

pub fn fam_covered(suite: &Suite) -> bool {
    FAM_PREFIX.iter().all(|p| {
        suite
            .cases
            .iter()
            .any(|c| c.tier == Tier::Func && c.name.starts_with(p))
    })
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec109_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec109");

    let mut su = Suite::new();

    // 族一：三层用例入册。
    s.add("孤儿回归拒", matches!(su.add(Tier::Regress, "reg-1", None, true, Some(true)), Err(XC_ORPHAN)), "随修入册前置");
    let _ = su.add(Tier::Func, "arith-add", None, true, Some(true));
    let _ = su.add(Tier::Func, "bit-shl", None, true, Some(true));
    let _ = su.add(Tier::Diff, "diff-g1", None, true, Some(true));
    let _ = su.add(Tier::Regress, "reg-paren", Some(77), true, Some(true));
    s.add("三层齐备", su.tiers_complete(), "三清单独立计数");
    s.add("回归带修复引用", su.cases.iter().filter(|c| c.tier == Tier::Regress).all(|c| c.fix_ref.is_some()), "问题号 77");
    s.add("重名拒", matches!(su.add(Tier::Func, "arith-add", None, true, Some(true)), Err(XC_DUP)), "用例唯一");
    s.add("空名拒", matches!(su.add(Tier::Func, "", None, true, Some(true)), Err(XC_NONAME)), "命名必填");

    // 族二：族覆盖。
    s.add("五族未满", !fam_covered(&su), "缺 cmp/tex/res");
    let _ = su.add(Tier::Func, "cmp-lt", None, true, Some(true));
    let _ = su.add(Tier::Func, "tex-tex2d", None, true, Some(true));
    let _ = su.add(Tier::Func, "res-load", None, true, Some(true));
    s.add("五族覆盖达成", fam_covered(&su), "全族用例在册");

    // 族三：双档运行。
    s.add("fast 档全过", su.run(false) == Ok(true), "快档独立");
    s.add("full 档全过", su.run(true) == Ok(true), "回读档独立");
    s.add("N/A 计数", su.na() == 0, "全部跑过");

    // 族四：N/A 语义。
    let mut su2 = Suite::new();
    let _ = su2.add(Tier::Func, "arith-mul", None, true, None);
    let _ = su2.add(Tier::Diff, "diff-g2", None, true, None);
    let _ = su2.add(Tier::Regress, "reg-fold", Some(80), true, Some(false));
    s.add("N/A 不算败（fast 档）", su2.run(false) == Ok(true), "fast 不依赖 full");
    s.add("full 失败算败", su2.run(true) == Ok(false), "Some(false) 计失败");
    s.add("N/A 显性计数", su2.na() == 2, "未跑可见");

    // 族五：缺层拦截。
    let mut su3 = Suite::new();
    let _ = su3.add(Tier::Func, "arith-x", None, true, Some(true));
    s.add("缺层拒跑", matches!(su3.run(false), Err(XC_NOLAYER)), "套件完整性前置");

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
        assert!(su.run(false) == Ok(true));
    }
}
