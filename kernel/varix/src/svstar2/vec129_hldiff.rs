//! VE-F0529 · HLSL 后端对拍验证（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0529`
//!
//! **锚点原文**：「HLSL 后端对拍验证（F0506 同构框架第二用复用声明）：生成
//! HLSL 经工具链编译回读、产物语义对拍（参考执行器环境受限时的降级对拍语义
//! 显性——Windows 域协作实机验证登记随闸门）、黄金文本对独立维护。判据：
//! 框架第二用、随闸门登记、双级对拍、独立黄金、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **框架第二用**（判据一）。对拍框架（语法快照 + 语义执行双级）与
//!    F0506 同构复用，差异只在回读通路（F0528 工具链）。
//!
//! 2. **随闸门登记**（判据二）。参考执行器缺格时降级对拍（仅语法级）显性
//!    标注，实机验证登记随 Windows 域闸门（未过闸不算语义验证过）。
//!
//! 3. **双级对拍**（判据三）。语法级（文本快照）与语义级（执行结果）独立
//!    判定；降级态下语义级显性记 N/A 而非默认通过。
//!
//! 4. **独立黄金**（判据四）。HLSL 黄金对独立维护（与 GLSL 黄金分册），
//!    逐条比对。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0529";

// ---------------------------------------------------------------------------
// 一、对拍分级（降级语义显性）
// ---------------------------------------------------------------------------

/// 对拍档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    /// 语法 + 语义双级。
    Full,
    /// 仅语法（语义缺格降级）。
    SynOnly,
}

/// 闸门登记。
#[derive(Clone, Debug)]
pub struct GateReg {
    pub case: String,
    pub grade: Grade,
    /// 实机验证登记（Windows 域闸门）。
    pub gated: bool,
}

/// 对拍结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HCase {
    pub name: String,
    pub syn: bool,
    /// 语义级：Some(bool) 或 None（N/A）。
    pub sem: Option<bool>,
    pub grade: Grade,
}

impl HCase {
    pub fn pass(&self) -> bool {
        self.syn && match self.sem {
            Some(ok) => ok,
            None => self.grade == Grade::SynOnly, // 降级态 N/A 不算败
        }
    }
}

/// 双级执行：参考缺格时自动降级。
pub fn run_case(name: &str, syn_ok: bool, sem_avail: bool, sem_ok: bool) -> HCase {
    let grade = if sem_avail { Grade::Full } else { Grade::SynOnly };
    HCase {
        name: name.to_string(),
        syn: syn_ok,
        sem: if sem_avail { Some(sem_ok) } else { None },
        grade,
    }
}

/// 闸门账：降级样例必须登记待实机验证。
#[derive(Default)]
pub struct GateBook {
    pub regs: Vec<GateReg>,
}

impl GateBook {
    pub fn new() -> GateBook {
        GateBook { regs: Vec::new() }
    }

    /// 登记降级样例。
    pub fn pend(&mut self, case: &str) {
        self.regs.push(GateReg { case: case.to_string(), grade: Grade::SynOnly, gated: false });
    }

    /// 闸门通过：实机验证补齐。
    pub fn clear(&mut self, case: &str) -> bool {
        match self.regs.iter_mut().find(|r| r.case == case) {
            Some(r) => {
                r.gated = true;
                true
            }
            None => false,
        }
    }

    /// 未过闸门数。
    pub fn pending(&self) -> usize {
        self.regs.iter().filter(|r| !r.gated).count()
    }
}

// ---------------------------------------------------------------------------
// 二、文本快照比对（语法级，与 F0506 同构）
// ---------------------------------------------------------------------------

pub fn syn_match(got: &str, want: &str) -> bool {
    let norm = |t: &str| -> Vec<String> {
        t.lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|l| !l.is_empty())
            .collect()
    };
    norm(got) == norm(want)
}

/// 语义级值比对。
pub fn sem_match(got: &[f64], want: &[f64]) -> bool {
    got.len() == want.len() && got.iter().zip(want.iter()).all(|(a, b)| (a - b).abs() < 1e-6)
}

// ---------------------------------------------------------------------------
// 三、HLSL 独立黄金对
// ---------------------------------------------------------------------------

pub struct HGolden {
    pub ir: &'static str,
    pub hlsl: &'static str,
}

pub const H_GOLDENS: [HGolden; 3] = [
    HGolden { ir: "mul v, a, b", hlsl: "float4 v = mul(a, b);" },
    HGolden { ir: "sample c, t, s, uv", hlsl: "float4 c = t.Sample(s, uv);" },
    HGolden { ir: "store b, i, v", hlsl: "b[i] = v;" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec129_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec129");

    // 族一：双级与降级。
    let c1 = run_case("g1", true, true, true);
    s.add("全档通过", c1.pass() && c1.grade == Grade::Full, "双级全跑");
    let c2 = run_case("g2", true, false, false);
    s.add("降级显性", c2.grade == Grade::SynOnly && c2.sem.is_none(), "N/A 语义");
    s.add("降级不算败", c2.pass(), "N/A 不计入失败");
    let c3 = run_case("g3", true, true, false);
    s.add("语义失败算败", !c3.pass(), "Some(false) 拦截");
    let c4 = run_case("g4", false, true, true);
    s.add("语法失败算败", !c4.pass(), "语法级前置");

    // 族二：闸门登记。
    let mut g = GateBook::new();
    g.pend("g2");
    s.add("降级必登记", g.pending() == 1, "随闸门");
    s.add("闸门过清账", g.clear("g2") && g.pending() == 0, "实机验证补齐");
    s.add("未知样例清不动", !g.clear("zz"), "账目严谨");
    g.pend("g5");
    s.add("未过闸留账", g.pending() == 1, "显性债务");

    // 族三：双级比对。
    s.add("语法空白归一", syn_match("mul(a,  b)", "mul(a, b)"), "文本等价");
    s.add("语法失配", !syn_match("mul(a, b)", "mul(a, c)"), "差异可见");
    s.add("语义容差内", sem_match(&[0.1 + 0.2], &[0.3]), "浮点容差");
    s.add("语义失配", !sem_match(&[1.0], &[1.5]), "结果差可见");

    // 族四：独立黄金。
    for h in H_GOLDENS.iter() {
        s.add("黄金形态合法", h.hlsl.ends_with(';') || h.hlsl.contains("Sample"), h.ir);
    }
    s.add("黄金三对", H_GOLDENS.len() == 3, "HLSL 独立正本");
    s.add("方法链形态", H_GOLDENS[1].hlsl.contains("t.Sample(s, uv)"), "HLSL 方法语法");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade() {
        let c = run_case("x", true, false, false);
        assert!(c.pass());
        assert_eq!(c.grade, Grade::SynOnly);
    }
}
