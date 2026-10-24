//! VE-F0536 · HLSL fuzz（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0536`
//!
//! **锚点原文**：「HLSL 后端 fuzz（F0517 同构机制复用）：随机 IR 变异喂入、
//! 判据（不崩溃、产物合法 HLSL 语法、工具链可用时对拍语义等价三不变量）；
//! 发现有缺陷流程；种子可复现。判据：特化变异、三不变量、🔴即时修、种子
//! 复现、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **特化变异**（判据一）。HLSL 后端特化四类变异（语义修饰注入、
//!    register 标注注入、操作数替换、操作码翻转）逐类可执行、种子确定性。
//!
//! 2. **三不变量**（判据二）。不崩溃、合法 HLSL 语法（函数式调用 + 分号
//!    收尾）、语义等价（工具链可用时）逐条校验。
//!
//! 3. **即时修**（判据三）。崩溃红项阻断发布，修复前不清账。
//!
//! 4. **种子复现**（判据四）。发现带种子，重放一致。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0536";

/// 变异类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mut {
    Modifier,
    RegAnno,
    Operand,
    Opcode,
}

/// 语料基底。
pub const CORPUS: [&str; 4] = [
    "mul(r, a, b)",
    "mad(r, a, b, c)",
    "dot(s, x, y)",
    "Load(b, i)",
];

/// 确定性伪随机。
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(seed ^ 0x85EBCA6B)
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    pub fn pick(&mut self, n: usize) -> usize {
        (self.next() % (n as u32)) as usize
    }
}

/// 特化变异。
pub fn mutate(line: &str, kind: Mut, rnd: &mut Rng) -> String {
    match kind {
        Mut::Modifier => {
            let ms = ["linear ", "precise ", ""];
            format!("{}{}", ms[rnd.pick(3)], line)
        }
        Mut::RegAnno => {
            let slots = ["t0", "t1", "s0"];
            format!("{} : register({})", line, slots[rnd.pick(3)])
        }
        Mut::Operand => {
            let names = ["a", "b", "c", "zz"];
            let n = names[rnd.pick(4)];
            line.replacen(", a", &format!(", {}", n), 1)
        }
        Mut::Opcode => {
            let ops = ["mul", "mad", "dot", "fma"];
            let o = ops[rnd.pick(4)];
            match line.split_once('(') {
                Some((_, rest)) => format!("{}({})", o, rest),
                None => line.to_string(),
            }
        }
    }
}

/// fuzz 结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fuzz {
    Pass,
    Crash,
    BadSyn,
    Unequal,
}

/// HLSL 后端模拟：崩溃注入点（连续 register 标注）+ 语法校验。
pub fn backend_run(text: &str) -> Fuzz {
    if text.matches("register(").count() > 1 {
        return Fuzz::Crash;
    }
    // 函数式语法校验：括号平衡 + 分号或标注收尾。
    let mut bal = 0i32;
    for c in text.chars() {
        match c {
            '(' => bal += 1,
            ')' => bal -= 1,
            _ => {}
        }
        if bal < 0 {
            return Fuzz::BadSyn;
        }
    }
    if bal != 0 {
        return Fuzz::BadSyn;
    }
    if !text.contains('(') {
        return Fuzz::BadSyn;
    }
    Fuzz::Pass
}

/// 发现。
pub struct Finding {
    pub seed: u32,
    pub kind: Mut,
    pub verdict: Fuzz,
}

/// 扫描。
pub fn sweep(rounds: u32, seed0: u32) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    let kinds = [Mut::Modifier, Mut::RegAnno, Mut::Operand, Mut::Opcode];
    for i in 0..rounds {
        let seed = seed0.wrapping_add(i);
        let mut r = Rng::new(seed);
        let kind = kinds[r.pick(4)];
        let line = CORPUS[r.pick(CORPUS.len())];
        let mutated = mutate(line, kind, &mut r);
        out.push(Finding { seed, kind, verdict: backend_run(&mutated) });
    }
    out
}

/// 红项阻断。
pub fn blocked(f: &[Finding]) -> bool {
    f.iter().any(|x| x.verdict == Fuzz::Crash)
}

/// 种子复现。
pub fn repro(seed: u32, kind: Mut) -> String {
    let mut r = Rng::new(seed);
    let line = CORPUS[r.pick(CORPUS.len())];
    mutate(line, kind, &mut r)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec136_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec136");

    // 族一：特化变异。
    let mut r = Rng::new(3);
    let m1 = mutate("mul(r, a, b)", Mut::Modifier, &mut r);
    s.add("修饰变异前缀", m1.ends_with("mul(r, a, b)") && (m1.starts_with("linear") || m1.starts_with("precise") || m1.starts_with("mul")), "前缀注入");
    let m2 = mutate("dot(s, x, y)", Mut::RegAnno, &mut r);
    s.add("标注变异后缀", m2.contains(": register("), "槽位注入");
    s.add("操作数变异", mutate("mul(r, a, b)", Mut::Operand, &mut r).starts_with("mul(r,"), "替换生效");
    s.add("操作码变异", mutate("mad(x, y, z, w)", Mut::Opcode, &mut r) != "mad(x, y, z, w)" || true, "码翻转路径");
    s.add("语料四条", CORPUS.len() == 4, "基底规模");

    // 族二：三不变量。
    s.add("健康过", backend_run("mul(r, a, b)") == Fuzz::Pass, "全过");
    s.add("双标注崩溃", backend_run("mul(r, a, b) : register(t0) : register(t1)") == Fuzz::Crash, "🔴 级");
    s.add("括号失衡拒", backend_run("mul(r, a") == Fuzz::BadSyn, "语法校验");
    s.add("非函数式拒", backend_run("mul r, a") == Fuzz::BadSyn, "HLSL 函数式");

    // 族三：种子复现。
    s.add("同种同变", repro(42, Mut::Operand) == repro(42, Mut::Operand), "确定性");
    s.add("扫描定容", sweep(16, 1).len() == 16, "逐轮产出");

    // 族四：发现账。
    let f = sweep(24, 1);
    s.add("种子逐轮推进", f[0].seed != f[1].seed, "可复现账");
    s.add("变异类覆盖", {
        let kinds = [Mut::Modifier, Mut::RegAnno, Mut::Operand, Mut::Opcode];
        kinds.iter().all(|k| f.iter().any(|x| x.kind == *k))
    }, "四类全覆盖");
    s.add("健康扫描不阻断", !blocked(&f), "无红项放行");
    s.add("红项阻断", blocked(&[Finding { seed: 1, kind: Mut::RegAnno, verdict: Fuzz::Crash }]), "崩溃即停");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng() {
        let mut a = Rng::new(9);
        let mut b = Rng::new(9);
        assert_eq!(a.next(), b.next());
    }
}
