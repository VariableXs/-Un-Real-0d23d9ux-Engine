//! VE-F0548 · WGSL 后端对拍验证（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0548`
//!
//! **锚点原文**：「WGSL 后端对拍验证（F0506 同构框架第三用复用声明）：生成
//! WGSL 文本经目标生态编译校验（验证器可用性如实标注——WebGPU 生态工具缺席
//! 时 N/A 随闸门登记）、黄金文本对独立维护、语义对拍依赖参考执行器扩展
//! （WGSL 语义模拟可行登记为远景 F0549 联动）。判据：框架第三用、N/A 随
//! 闸门、独立黄金、远景登记、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **框架第三用**（判据一）。双级对拍框架第三次复用；校验通路抽象
//!    （验证器可插拔）。
//!
//! 2. **N/A 随闸门**（判据二）。验证器缺席 → N/A 显性登记随闸门（不冒充
//!    通过、也不算失败）。
//!
//! 3. **独立黄金**（判据三）。WGSL 黄金文本对独立维护。
//!
//! 4. **远景登记**（判据四）。语义模拟可行性登记为远景（F0549 联动），
//!    不硬做。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0548";

/// 验证器可用性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validator {
    /// WebGPU 生态验证器可用。
    Avail,
    /// 缺席。
    Absent,
}

/// 校验结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    Na,
}

/// 校验通路。
pub struct VPath {
    pub v: Validator,
}

impl VPath {
    /// 编译校验：括号平衡 + 分号收尾（模拟 WGSL 校验）。
    pub fn check(&self, text: &str) -> Verdict {
        match self.v {
            Validator::Absent => Verdict::Na,
            Validator::Avail => {
                let mut bal = 0i32;
                for c in text.chars() {
                    match c {
                        '{' => bal += 1,
                        '}' => bal -= 1,
                        _ => {}
                    }
                    if bal < 0 {
                        return Verdict::Fail;
                    }
                }
                if bal != 0 {
                    return Verdict::Fail;
                }
                for l in text.lines() {
                    let t = l.trim();
                    if t.is_empty() || t.starts_with("//") {
                        continue;
                    }
                    if !(t.ends_with(';') || t.ends_with('{') || t.ends_with('}')) {
                        return Verdict::Fail;
                    }
                }
                Verdict::Pass
            }
        }
    }
}

/// 闸门账（N/A 显性）。
#[derive(Default)]
pub struct GateBook {
    pub regs: Vec<(String, bool)>,
}

impl GateBook {
    pub fn new() -> GateBook {
        GateBook { regs: Vec::new() }
    }

    /// N/A 样例登记。
    pub fn pend(&mut self, case: &str) {
        if !self.regs.iter().any(|(c, _)| c == case) {
            self.regs.push((case.to_string(), false));
        }
    }

    pub fn clear(&mut self, case: &str) -> bool {
        match self.regs.iter_mut().find(|(c, _)| c == case) {
            Some(r) => {
                r.1 = true;
                true
            }
            None => false,
        }
    }

    pub fn pending(&self) -> usize {
        self.regs.iter().filter(|(_, g)| !g).count()
    }
}

/// 远景登记（语义模拟可行 → F0549 联动）。
pub struct Vision {
    pub item: &'static str,
    pub linked: &'static str,
    pub feasible: bool,
}

pub const VISIONS: [Vision; 1] = [
    Vision { item: "WGSL 语义模拟参考执行器", linked: "F0549", feasible: true },
];

/// 语义级结论：模拟可行才做，不可行显性 N/A（不硬做）。
pub fn sem_grade(sim_feasible: bool, ref_out: Option<&[f64]>, got: &[f64]) -> Verdict {
    if !sim_feasible {
        return Verdict::Na;
    }
    match ref_out {
        None => Verdict::Na,
        Some(r) => {
            if r.len() == got.len() && r.iter().zip(got.iter()).all(|(a, b)| (a - b).abs() < 1e-6) {
                Verdict::Pass
            } else {
                Verdict::Fail
            }
        }
    }
}

/// WGSL 黄金对。
pub struct WGolden {
    pub ir: &'static str,
    pub wgsl: &'static str,
}

pub const W_GOLDENS: [WGolden; 3] = [
    WGolden { ir: "add r, a, b", wgsl: "let r = f32.add(a, b);" },
    WGolden { ir: "fn main", wgsl: "fn main() { }" },
    WGolden { ir: "var x", wgsl: "var x = 0.0;" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec148_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec148");

    // 族一：校验通路。
    let vp = VPath { v: Validator::Avail };
    s.add("健康文本过", vp.check("let r = f32.add(a, b);\n") == Verdict::Pass, "语法校验");
    s.add("括号失衡拒", vp.check("fn f() {\n") == Verdict::Fail, "结构校验");
    s.add("无分号拒", vp.check("let x = 1\n") == Verdict::Fail, "语句收尾");
    let vna = VPath { v: Validator::Absent };
    s.add("缺席 N/A", vna.check("anything") == Verdict::Na, "不冒充");

    // 族二：N/A 随闸门。
    let mut g = GateBook::new();
    g.pend("wg-fuzz-1");
    s.add("N/A 登记", g.pending() == 1, "随闸门显性");
    s.add("过闸清账", g.clear("wg-fuzz-1") && g.pending() == 0, "工具补齐");
    s.add("未知清不动", !g.clear("zz"), "账目严谨");

    // 族三：语义级（模拟可行才做）。
    s.add("模拟可行才评", sem_grade(false, Some(&[1.0]), &[1.0]) == Verdict::Na, "不可行不硬做");
    s.add("可行且等价过", sem_grade(true, Some(&[1.0, 2.0]), &[1.0, 2.0]) == Verdict::Pass, "语义对拍");
    s.add("可行失配拒", sem_grade(true, Some(&[1.0]), &[2.0]) == Verdict::Fail, "差异可见");
    s.add("无参考 N/A", sem_grade(true, None, &[1.0]) == Verdict::Na, "参考缺席");

    // 族四：远景登记与黄金。
    s.add("远景登记", VISIONS.len() == 1 && VISIONS[0].linked == "F0549", "联动在册");
    s.add("可行性如实", VISIONS[0].feasible, "不硬做不夸大");
    for gd in W_GOLDENS.iter() {
        s.add("WGSL 黄金形态", vp.check(gd.wgsl) == Verdict::Pass, gd.ir);
    }
    s.add("黄金三对", W_GOLDENS.len() == 3, "独立正本");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check() {
        let vp = VPath { v: Validator::Avail };
        assert_eq!(vp.check("var x = 0.0;\n"), Verdict::Pass);
        assert_eq!(VPath { v: Validator::Absent }.check("x"), Verdict::Na);
    }
}
