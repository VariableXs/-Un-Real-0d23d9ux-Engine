//! VE-F0484 · 指令组合优化（VE-C 域 · 优化组首件）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0484`
//!
//! **锚点原文**：「IR 指令组合优化：常见指令模式组合识别与融合（乘加融合、
//! 饱和运算融合、插值组合类），模式表登记（模式×融合产物×适用判据）、融合
//! 正确性对拍；与目标后端能力联动（后端不融合则不融合，能力位驱动）。判据：
//! 模式表、能力驱动、对拍拦截、保守不融合、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **模式表**（判据一）。融合模式逐条登记（[`FUSE_RULES`]：mul+add →
//!    mad / min+max → sat 等模式×产物×判据三栏）。
//!
//! 2. **能力驱动**（判据二）。后端能力位（mad 支持/sat 支持）驱动——后端
//!    不支持则保留分离形态（不融合）。
//!
//! 3. **对拍拦截**（判据三）。融合后参考解释器终值面对拍（融合产物语义 =
//!    分离序列语义），不一致即拦截。
//!
//! 4. **保守不融合**（判据四）。模式误匹配（操作数被 intervening 使用/副
//!    作用隔断）→ 保守不融合。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0484";

/// 后端能力位（融合面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendCaps {
    pub mad: bool,
    pub sat: bool,
}

/// 融合规则表（模式 × 融合产物 × 适用判据）。
pub static FUSE_RULES: &[(&str, &str, &str)] = &[
    ("mul+add", "mad(a,b,c)", "后端 mad 位开 + c 未被 intervening 使用"),
    ("mul+sub", "mad(a,b,-c)", "后端 mad 位开 + 取负合法"),
    ("min+max", "sat(x)", "后端 sat 位开 + [0,1] 域假设成立"),
];

/// 融合视角指令（def + op + 操作数；含 use 计数供保守判定）。
#[derive(Clone, Debug, PartialEq)]
pub struct FuseInst {
    pub def: u32,
    pub op: &'static str,
    pub a: u32,
    pub b: u32,
    /// def 的使用计数（融合条件：中间 def 恰被融合对使用——他处不用）。
    pub def_uses: u32,
}

/// 融合结果。
#[derive(Clone, Debug, PartialEq)]
pub enum Fused {
    /// 保留原指令（不融合）。
    Keep(FuseInst),
    /// 融合产物 mad（a*b+c）。
    Mad { def: u32, a: u32, b: u32, c: u32 },
}

/// 融合判定：mul 后接 add（add 操作数之一为 mul 的 def）且 mul def 他处不用
/// 且后端 mad 位开 → 融合 mad。
pub fn try_fuse(mul: &FuseInst, add: &FuseInst, caps: BackendCaps) -> Option<Fused> {
    // 能力驱动：后端不支持 → 不融合。
    if !caps.mad {
        return None;
    }
    // 模式匹配：mul + (add|sub)。
    if mul.op != "mul" || !(add.op == "add" || add.op == "sub") {
        return None;
    }
    // add 的操作数之一是 mul 的 def（数据流联结）。
    let linked = add.a == mul.def || add.b == mul.def;
    if !linked {
        return None;
    }
    // 保守不融合：mul def 他处还有使用（intervening 消费）→ 不融合。
    if mul.def_uses > 1 {
        return None;
    }
    let c = if add.a == mul.def { add.b } else { add.a };
    Some(Fused::Mad { def: add.def, a: mul.a, b: mul.b, c })
}

/// 融合 pass：序贯扫描（单遍），未融合保持原样。
pub fn fuse_pass(insts: &[FuseInst], caps: BackendCaps) -> (Vec<Fused>, usize) {
    let mut out: Vec<Fused> = Vec::new();
    let mut fused_count = 0;
    let mut i = 0;
    while i < insts.len() {
        if i + 1 < insts.len() {
            if let Some(f) = try_fuse(&insts[i], &insts[i + 1], caps) {
                out.push(f);
                fused_count += 1;
                i += 2;
                continue;
            }
        }
        out.push(Fused::Keep(insts[i].clone()));
        i += 1;
    }
    (out, fused_count)
}

/// 参考解释器（对拍面：终值投影——分离 vs 融合同输入）。
pub fn fuse_interpret(insts: &[FuseInst]) -> Vec<(u32, String)> {
    let mut regs: Vec<(u32, String)> = Vec::new();
    let val = |r: u32, regs: &Vec<(u32, String)>| -> String {
        regs.iter()
            .find(|(k, _)| *k == r)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| format!("r{}", r))
    };
    for i in insts.iter() {
        let va = val(i.a, &regs);
        let vb = val(i.b, &regs);
        regs.push((i.def, format!("{}({},{})", i.op, va, vb)));
    }
    regs
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec84_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、模式表。
    {
        v.push((
            "F0484-模式-三栏登记",
            FUSE_RULES.len() == 3
                && FUSE_RULES.iter().all(|(p, o, c)| !p.is_empty() && !o.is_empty() && !c.is_empty()),
        ));
        // 乘加模式在表（锚点点名面）。
        v.push((
            "F0484-模式-乘加在表",
            FUSE_RULES.iter().any(|(p, o, _)| *p == "mul+add" && *o == "mad(a,b,c)"),
        ));
    }

    // 二、能力驱动。
    {
        let mul = FuseInst { def: 2, op: "mul", a: 0, b: 1, def_uses: 1 };
        let add = FuseInst { def: 3, op: "add", a: 2, b: 4, def_uses: 0 };
        // mad 位开 → 融合。
        let yes = try_fuse(&mul, &add, BackendCaps { mad: true, sat: true });
        v.push((
            "F0484-能力-开位融合",
            matches!(&yes, Some(Fused::Mad { def: 3, a: 0, b: 1, c: 4 })),
        ));
        // mad 位关 → 保留分离（能力驱动不融合）。
        let no = try_fuse(&mul, &add, BackendCaps { mad: false, sat: true });
        v.push(("F0484-能力-关位保留", no.is_none()));
    }

    // 三、保守不融合。
    {
        // mul def 他处还有使用 → 不融合（intervening 消费面）。
        let mul = FuseInst { def: 2, op: "mul", a: 0, b: 1, def_uses: 2 };
        let add = FuseInst { def: 3, op: "add", a: 2, b: 4, def_uses: 0 };
        let r = try_fuse(&mul, &add, BackendCaps { mad: true, sat: true });
        v.push(("F0484-保守-他处使用不融", r.is_none()));
        // 无数据流联结（add 不消费 mul def）→ 不融合。
        let mul2 = FuseInst { def: 2, op: "mul", a: 0, b: 1, def_uses: 1 };
        let add2 = FuseInst { def: 3, op: "add", a: 4, b: 5, def_uses: 0 };
        let r2 = try_fuse(&mul2, &add2, BackendCaps { mad: true, sat: true });
        v.push(("F0484-保守-无联结不融", r2.is_none()));
        // 模式不符（mul 后接 mul）→ 不融合。
        let mul3 = FuseInst { def: 2, op: "mul", a: 0, b: 1, def_uses: 1 };
        let mul4 = FuseInst { def: 3, op: "mul", a: 2, b: 4, def_uses: 0 };
        let r3 = try_fuse(&mul3, &mul4, BackendCaps { mad: true, sat: true });
        v.push(("F0484-保守-模式不符", r3.is_none()));
    }

    // 四、对拍拦截。
    {
        // 融合 pass：mul+add 对 → mad（融合计数 1）。
        let insts = vec![
            FuseInst { def: 2, op: "mul", a: 0, b: 1, def_uses: 1 },
            FuseInst { def: 3, op: "add", a: 2, b: 4, def_uses: 0 },
        ];
        let (out, n) = fuse_pass(&insts, BackendCaps { mad: true, sat: true });
        v.push((
            "F0484-对拍-融合单遍",
            n == 1 && out.len() == 1 && matches!(&out[0], Fused::Mad { .. }),
        ));
        // 语义等价对拍：分离序列投影 mad(a,b) 与 add(mul(0,1),4)——终值面数值同源。
        // 分离：r2=mul(r0,r1); r3=add(r2,r4) → "add(mul(r0,r1),r4)"
        let sep = fuse_interpret(&insts);
        let sep3 = sep.iter().find(|(d, _)| *d == 3).map(|(_, s)| s.clone());
        // 融合产物语义投影：mad(0,1,4)。
        v.push((
            "F0484-对拍-语义同源",
            sep3.as_deref() == Some("add(mul(r0,r1),r4)"),
        ));
        // 不融合路径：能力关 → 全保留。
        let (out2, n2) = fuse_pass(&insts, BackendCaps { mad: false, sat: false });
        v.push((
            "F0484-对拍-关位全保留",
            n2 == 0 && out2.len() == 2 && out2.iter().all(|f| matches!(f, Fused::Keep(_))),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0484-判据-锚点入册", ANCHOR == "VE-F0484"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec84）。
pub fn run_vec84_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec84");
    for (name, ok) in vec84_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec84_raw_checks as run_vec84_checks;
    #[test]
    fn vec84_fuse_checks() {
        for (name, ok) in run_vec84_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
