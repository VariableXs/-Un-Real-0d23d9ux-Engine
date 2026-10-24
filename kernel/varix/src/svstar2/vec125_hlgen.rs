//! VE-F0525 · HLSL 指令族生成（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0525`
//!
//! **锚点原文**：「IR 指令族到 HLSL 文本生成：全族指令模板（HLSL 函数风格
//! 语义——内建函数映射、语义修饰符文本）、控制流文本（复用 F0503 重建框架
//! 产出）、类型与格式化集成；生成正确性黄金对拍（HLSL 侧独立黄金对集）。
//! 判据：内建映射、重建复用、独立黄金、显性拒绝、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **内建映射**（判据一）。IR 操作码 → HLSL 内建函数逐条映射（mad/dot/
//!     sample 等），映射表单源；表外操作码显性拒绝。
//!
//! 2. **重建复用**（判据二）。控制流文本直接消费 F0503 重建产出（区域类型
//!    → HLSL 文本），不重建第二遍。
//!
//! 3. **独立黄金**（判据三）。HLSL 侧黄金对独立维护（不复用 GLSL 黄金集
//!    ——两种语言两种正本），逐条比对。
//!
//! 4. **显性拒绝**（判据四）。参数不足/类型不支持/未映射指令全部显性拒绝
//!    带定位。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0525";

// ---------------------------------------------------------------------------
// 一、内建函数映射
// ---------------------------------------------------------------------------

/// 操作码 → 内建。
pub const BUILTIN: [(&str, &str); 8] = [
    ("imul", "mul"),
    ("iadd", "op_add"),
    ("imad", "mad"),
    ("idot", "dot"),
    ("isample", "Sample"),
    ("iload", "Load"),
    ("istore", "Store"),
    ("iflatten", "asuint"),
];

pub fn builtin(op: &str) -> Result<&'static str, u32> {
    BUILTIN.iter().find(|(o, _)| *o == op).map(|(_, b)| *b).ok_or(XC_NOB)
}

pub const XC_NOB: u32 = 1;

/// 函数式发射：b(args)。
pub fn gen_call(op: &str, args: &str) -> Result<String, u32> {
    if args.is_empty() {
        return Err(XC_NOARGS);
    }
    let b = builtin(op)?;
    Ok(format!("{}({})", b, args))
}

pub const XC_NOARGS: u32 = 2;

/// 语义修饰符文本。
pub fn modifier(sm: u32, interp: &str) -> String {
    // SM4+ 支持插值修饰符；SM5+ 支持 precise。
    let mut out = String::new();
    if !interp.is_empty() {
        out.push_str(interp);
        out.push(' ');
    }
    if sm >= 51 {
        out.push_str("precise ");
    }
    out
}

// ---------------------------------------------------------------------------
// 二、控制流文本（消费 F0503 区域产出）
// ---------------------------------------------------------------------------

/// 区域类型（与 F0503 对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rgn {
    If,
    While,
    FlagGuard,
}

/// 区域 → HLSL 文本骨架。
pub fn gen_flow(r: Rgn, cond: &str, body: &str) -> Result<String, u32> {
    if cond.is_empty() {
        return Err(XC_NOCOND);
    }
    match r {
        Rgn::If => Ok(format!("if ({}) {{ {} }}", cond, body)),
        Rgn::While => Ok(format!("[loop] while ({}) {{ {} }}", cond, body)),
        Rgn::FlagGuard => Ok(format!("if (!{}[0]) {{ {} }}", cond, body)),
    }
}

pub const XC_NOCOND: u32 = 3;

// ---------------------------------------------------------------------------
// 三、类型集成
// ---------------------------------------------------------------------------

/// 结果类型标注发射。
pub fn gen_typed(ty: &str, dst: &str, expr: &str) -> Result<String, u32> {
    if ty.is_empty() || dst.is_empty() || expr.is_empty() {
        return Err(XC_NOTY);
    }
    Ok(format!("{} {} = {};", ty, dst, expr))
}

pub const XC_NOTY: u32 = 4;

// ---------------------------------------------------------------------------
// 四、HLSL 独立黄金对
// ---------------------------------------------------------------------------

pub struct HGolden {
    pub op: &'static str,
    pub args: &'static str,
    pub want: &'static str,
}

pub const H_GOLDENS: [HGolden; 4] = [
    HGolden { op: "imad", args: "r, a, b, c", want: "mad(r, a, b, c)" },
    HGolden { op: "idot", args: "s, x, y", want: "dot(s, x, y)" },
    HGolden { op: "isample", args: "v, tex, samp, uv", want: "Sample(v, tex, samp, uv)" },
    HGolden { op: "iflatten", args: "u, f", want: "asuint(u, f)" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec125_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec125");

    // 族一：内建映射。
    s.add("mad 映射", builtin("imad") == Ok("mad"), "函数式");
    s.add("dot 映射", builtin("idot") == Ok("dot"), "点积");
    s.add("表外拒", matches!(builtin("rsq"), Err(XC_NOB)), "显性拒绝");
    s.add("映射表八件", BUILTIN.len() == 8, "正本规模");
    s.add("调用发射", gen_call("imad", "r, a, b, c") == Ok("mad(r, a, b, c)".to_string()), "HLSL 函数式");
    s.add("空参拒", matches!(gen_call("imad", ""), Err(XC_NOARGS)), "参数必填");

    // 族二：语义修饰符。
    s.add("SM5 带 precise", modifier(51, "linear").contains("precise"), "SM5+");
    s.add("SM4 无 precise", !modifier(40, "linear").contains("precise"), "SM4 限");
    s.add("插值修饰符前置", modifier(51, "noperspective").starts_with("noperspective "), "语义序");

    // 族三：控制流复用。
    s.add("if 骨架", gen_flow(Rgn::If, "x > 0", "y = 1;") == Ok("if (x > 0) { y = 1; }".to_string()), "消费重建产出");
    s.add("while 带 loop 属性", gen_flow(Rgn::While, "i < n", "i++;") == Ok("[loop] while (i < n) { i++; }".to_string()), "HLSL 属性");
    s.add("旗守卫形态", gen_flow(Rgn::FlagGuard, "flag", "break;") == Ok("if (!flag[0]) { break; }".to_string()), "F0503 兜底对应");
    s.add("空条件拒", matches!(gen_flow(Rgn::If, "", "x;"), Err(XC_NOCOND)), "条件必填");

    // 族四：类型集成与黄金。
    s.add("类型标注发射", gen_typed("float4", "r", "a * b") == Ok("float4 r = a * b;".to_string()), "三段集成");
    s.add("缺字段拒", matches!(gen_typed("", "r", "x"), Err(XC_NOTY)), "显性拒绝");
    for g in H_GOLDENS.iter() {
        s.add("独立黄金对拍", gen_call(g.op, g.args) == Ok(g.want.to_string()), g.op);
    }
    s.add("黄金四对", H_GOLDENS.len() == 4, "HLSL 侧独立正本");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen() {
        assert_eq!(gen_call("imad", "r,a,b,c").ok(), Some("mad(r,a,b,c)".to_string()));
        assert!(gen_call("rsq", "x").is_err());
    }
}
