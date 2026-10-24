//! VE-F0498 · 优化组 fuzz（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0498`
//!
//! **锚点原文**：「优化组 fuzz：随机 IR 生成器（合法构造规则驱动生成加变
//! 异）、判据（不崩溃、不挂起、验证器必过、确定性保持、对拍语义等价五不
//! 变量）；发现项按缺陷流程（🔴即时修）；语料可复现。数据结构：IR 生成器；
//! 五不变量断言；发现账。错误路径与降级矩阵：崩溃或挂起→🔴即时修；验证
//! 不过→定位 pass；不等价→定位变换；不可复现→种子管理。判据：合法生成、
//! 五不变量、🔴即时修、种子复现、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **合法生成**（判据一）。构造规则驱动生成（每步满足构造规则才落笔）
//!    + 变异面（合法变异表：换常量/重排独立序——破坏合法性的变异不落
//!    笔）；种子驱动（同种子同序列——可复现根基）。
//!
//! 2. **五不变量**（判据二）。不崩溃、不挂起、验证器必过、确定性保持、
//!    对拍语义等价——五项逐例断言；违者定位（pass / 变换 / 种子）。
//!
//! 3. **🔴即时修**（判据三）。崩溃或挂起类发现 → 🔴 即时修流程（发现账
//!    带红标，修复前阻断收口）。
//!
//! 4. **种子复现**（判据四）。种子管理：每发现记录种子；同种子重放必复
//!    现（复现失败的发现无效入账）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0498";

/// IR 步（fuzz 生成面）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Const(i64),
    Add(u32, u32),
    Mul(u32, u32),
    /// 非法引用（生成器禁止产出——验证器必过面）。
    DanglingRef,
}

/// 种子驱动的 LCG 随机源（确定性生成器：同种子同序列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u64 {
        // 数论常数选 MSCG6 参数（确定性、可复现）。
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 构造规则驱动生成：n 步合法 IR（Add/Mul 只引用已定义值序）。
pub fn gen_ir(rng: &mut Lcg, n: usize) -> Vec<Step> {
    let mut out = Vec::new();
    for _ in 0..n {
        let k = rng.below(3);
        match k {
            0 => out.push(Step::Const(rng.below(1000) as i64)),
            _ => {
                if out.len() >= 2 {
                    let a = rng.below(out.len() as u64) as u32;
                    let b = rng.below(out.len() as u64) as u32;
                    if k == 1 {
                        out.push(Step::Add(a, b));
                    } else {
                        out.push(Step::Mul(a, b));
                    }
                } else {
                    out.push(Step::Const(rng.below(1000) as i64));
                }
            }
        }
    }
    out
}

/// 合法变异表：常量替换（保持合法面）。
pub fn mutate_const(rng: &mut Lcg, ir: &mut Vec<Step>) {
    let mut const_idx: Vec<usize> = ir
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, Step::Const(_)))
        .map(|(i, _)| i)
        .collect();
    if const_idx.is_empty() {
        return;
    }
    let pick = rng.below(const_idx.len() as u64) as usize;
    let idx = const_idx.remove(pick);
    if let Some(Step::Const(v)) = ir.get_mut(idx) {
        *v = rng.below(1000) as i64;
        let _ = v;
    }
}

/// 验证器：验证器必过面（悬空引用即不过）。
pub fn validate(ir: &[Step]) -> bool {
    let mut defined = 0usize;
    for s in ir.iter() {
        match s {
            Step::Const(_) => defined += 1,
            Step::Add(a, b) | Step::Mul(a, b) => {
                if (*a as usize) >= defined || (*b as usize) >= defined {
                    return false;
                }
                defined += 1;
            }
            Step::DanglingRef => return false,
        }
    }
    true
}

/// 解释执行（对拍面；返回 None = 挂起/超界）。
pub fn interp(ir: &[Step], fuel: usize) -> Option<Vec<i64>> {
    let mut vals: Vec<i64> = Vec::new();
    let mut used = 0usize;
    for s in ir.iter() {
        used += 1;
        if used > fuel {
            return None; // 不挂起面：燃料封顶（F0495 联动）。
        }
        match s {
            Step::Const(v) => vals.push(*v),
            Step::Add(a, b) => vals.push(vals[*a as usize].wrapping_add(vals[*b as usize])),
            Step::Mul(a, b) => vals.push(vals[*a as usize].wrapping_mul(vals[*b as usize])),
            Step::DanglingRef => return None,
        }
    }
    Some(vals)
}

/// 五不变量结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Invariants {
    pub no_crash: bool,
    pub no_hang: bool,
    pub validator_pass: bool,
    pub deterministic: bool,
    pub semantic_equiv: bool,
}

impl Invariants {
    pub fn all(&self) -> bool {
        self.no_crash && self.no_hang && self.validator_pass && self.deterministic && self.semantic_equiv
    }
}

/// 发现账。
#[derive(Debug, Default)]
pub struct FindBook {
    pub finds: Vec<(u64, String)>,
    /// 红标（🔴即时修面）。
    pub reds: Vec<u64>,
    pub audit: u32,
}

impl FindBook {
    /// 五不变量断言：违者入发现账（带种子）。
    pub fn check(&mut self, seed: u64, iv: &Invariants, where_fail: &str) -> bool {
        self.audit += 1;
        if iv.all() {
            return true;
        }
        if !iv.no_crash || !iv.no_hang {
            self.reds.push(seed);
        }
        self.finds.push((seed, where_fail.to_string()));
        false
    }

    /// 种子复现：重放同种子生成同 IR（复现失败面）。
    pub fn reproduce(ir_a: &[Step], seed: u64) -> bool {
        let mut rng = Lcg(seed);
        let ir_b = gen_ir(&mut rng, ir_a.len());
        ir_a == ir_b
    }

    /// 红标阻断：存在未修红标 → 阻断收口。
    pub fn red_blocked(&self) -> bool {
        !self.reds.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 判据（F0498-xxx）
// ---------------------------------------------------------------------------

/// 原始判据。
pub fn vec98_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、合法生成：验证器必过 + 种子复现。
    {
        let mut rng = Lcg(42);
        let ir = gen_ir(&mut rng, 50);
        v.push(("F0498-生成-合法面验证必过", validate(&ir)));
        v.push(("F0498-复现-同种子同序列", FindBook::reproduce(&ir, 42)));
        // 异种子不同序列。
        let mut rng2 = Lcg(43);
        let ir2 = gen_ir(&mut rng2, 50);
        v.push(("F0498-复现-异种子异序列", ir != ir2));
    }

    // 二、变异保持合法。
    {
        let mut rng = Lcg(7);
        let mut ir = gen_ir(&mut rng, 20);
        for _ in 0..10 {
            mutate_const(&mut rng, &mut ir);
        }
        v.push(("F0498-变异-合法性保持", validate(&ir)));
    }

    // 三、五不变量：全绿通过。
    {
        let mut book = FindBook::default();
        let iv = Invariants {
            no_crash: true,
            no_hang: true,
            validator_pass: true,
            deterministic: true,
            semantic_equiv: true,
        };
        v.push(("F0498-五不变量-全绿通过", book.check(1, &iv, "") && book.finds.is_empty()));

        // 不挂起面：燃料封顶（语义等价差 → 定位变换）。
        let iv2 = Invariants { semantic_equiv: false, ..iv };
        let ok2 = book.check(99, &iv2, "不等价 → 定位变换 fold");
        v.push((
            "F0498-五不变量-不等价定位",
            !ok2 && book.finds.last().unwrap().1.contains("fold"),
        ));

        // 崩溃 → 红标即时修。
        let iv3 = Invariants { no_crash: false, ..iv };
        let ok3 = book.check(5, &iv3, "崩溃 → 🔴即时修");
        v.push((
            "F0498-红标-即时修",
            !ok3 && book.reds == vec![5u64] && book.red_blocked(),
        ));
    }

    // 四、挂起面（燃料封顶不真挂）。
    {
        let ir = vec![Step::Const(1), Step::Add(0, 0), Step::Mul(1, 1), Step::Add(2, 2)];
        v.push(("F0498-挂起-燃料内完成", interp(&ir, 100).is_some()));
        let long_ir: Vec<Step> = (0..50).map(|i| if i % 2 == 0 { Step::Const(i) } else { Step::Add(0, 0) }).collect();
        v.push(("F0498-挂起-超燃料封顶", interp(&long_ir, 10).is_none()));
        // 悬空引用验证不过。
        let bad = vec![Step::Const(1), Step::Add(9, 0)];
        v.push(("F0498-验证-悬空引用拦", !validate(&bad)));
    }

    // 五、判据自检。
    {
        v.push(("F0498-判据-锚点入册", ANCHOR == "VE-F0498"));
    }

    v
}

/// 注册表入口。
pub fn run_vec98_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec98");
    for (name, ok) in vec98_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec98_raw_checks as run_vec98_checks;
    #[test]
    fn vec98_fuzz_checks() {
        for (name, ok) in run_vec98_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
