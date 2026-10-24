//! VE-F0544 · WGSL 指令族生成（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0544`
//!
//! **锚点原文**：「IR 指令族到 WGSL 文本生成：全族指令模板（WGSL 内建函数
//! 命名映射、显性类型转换文本插入——强类型要求）、控制流文本（复用 F0503
//! 重建框架第三用复用声明）；生成正确性黄金对拍独立维护。判据：内建映射、
//! 显性转换插入、框架第三用、独立黄金、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **内建映射**（判据一）。IR 操作码 → WGSL 内建逐条映射（ WGSL 内建
//!    点分形态 f32.add / vec 加法），表外显性拒绝。
//!
//! 2. **显性转换插入**（判据二）。混型操作数自动插 f32()/i32() 转换（F0542
//!    机制消费）。
//!
//! 3. **框架第三用**（判据三）。控制流直接消费 F0503 重建产出（区域 →
//!    WGSL 语法），第三次复用。
//!
//! 4. **独立黄金**（判据四）。WGSL 黄金对独立维护。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0544";

// ---------------------------------------------------------------------------
// 一、内建映射
// ---------------------------------------------------------------------------

pub const BUILTIN: [(&str, &str); 6] = [
    ("iadd", "f32.add"),
    ("imul", "f32.mul"),
    ("idot", "dot"),
    ("inorm", "normalize"),
    ("ilen", "length"),
    ("imin", "min"),
];

pub fn builtin(op: &str) -> Result<&'static str, u32> {
    BUILTIN.iter().find(|(o, _)| *o == op).map(|(_, b)| *b).ok_or(XC_NOB)
}

pub const XC_NOB: u32 = 1;

/// 语句发射（WGSL let 形态）。
pub fn gen_let(op: &str, dst: &str, args: &str) -> Result<String, u32> {
    if dst.is_empty() || args.is_empty() {
        return Err(XC_NOARGS);
    }
    let b = builtin(op)?;
    Ok(format!("let {} = {}({});", dst, b, args))
}

pub const XC_NOARGS: u32 = 2;

// ---------------------------------------------------------------------------
// 二、显性转换插入（消费 F0542）
// ---------------------------------------------------------------------------

/// 操作数类型标注 → 生成参数文本（混型插转换）。
pub fn coerce_args(types: &[&str], vals: &[&str], target: &str) -> Result<String, u32> {
    if types.len() != vals.len() || vals.is_empty() {
        return Err(XC_LEN);
    }
    let mixed = types.iter().any(|t| *t == "i32") && types.iter().any(|t| *t == "f32");
    let parts: Vec<String> = vals
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if mixed && types[i] != target {
                format!("{}({})", target, v)
            } else {
                v.to_string()
            }
        })
        .collect();
    Ok(parts.join(", "))
}

pub const XC_LEN: u32 = 3;

// ---------------------------------------------------------------------------
// 三、控制流（第三用）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rgn {
    If,
    Loop,
    Guard,
}

/// 区域 → WGSL 语法。
pub fn gen_flow(r: Rgn, cond: &str, body: &str) -> Result<String, u32> {
    if cond.is_empty() {
        return Err(XC_NOCOND);
    }
    match r {
        Rgn::If => Ok(format!("if ({}) {{ {} }}", cond, body)),
        Rgn::Loop => Ok(format!("loop {{ {} }}", body)),
        Rgn::Guard => Ok(format!("if ({}[0]) {{ continue; }}", cond)),
    }
}

pub const XC_NOCOND: u32 = 4;

// ---------------------------------------------------------------------------
// 四、WGSL 黄金对
// ---------------------------------------------------------------------------

pub struct WGolden {
    pub op: &'static str,
    pub dst: &'static str,
    pub args: &'static str,
    pub want: &'static str,
}

pub const W_GOLDENS: [WGolden; 3] = [
    WGolden { op: "iadd", dst: "r", args: "a, b", want: "let r = f32.add(a, b);" },
    WGolden { op: "idot", dst: "d", args: "x, y", want: "let d = dot(x, y);" },
    WGolden { op: "ilen", dst: "l", args: "v", want: "let l = length(v);" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec144_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec144");

    // 族一：内建映射。
    s.add("加法映射", builtin("iadd") == Ok("f32.add"), "点分形态");
    s.add("点积映射", builtin("idot") == Ok("dot"), "WGSL 内建");
    s.add("表外拒", matches!(builtin("rsq"), Err(XC_NOB)), "显性拒绝");
    s.add("映射六件", BUILTIN.len() == 6, "正本规模");
    s.add("let 发射", gen_let("iadd", "r", "a, b") == Ok("let r = f32.add(a, b);".to_string()), "语句形态");
    s.add("空参拒", matches!(gen_let("iadd", "r", ""), Err(XC_NOARGS)), "参数必填");
    s.add("空目标拒", matches!(gen_let("iadd", "", "a"), Err(XC_NOARGS)), "目标必填");

    // 族二：显性转换插入。
    s.add("同型直通", coerce_args(&["f32", "f32"], &["a", "b"], "f32") == Ok("a, b".to_string()), "无转换");
    s.add("混型插转换", coerce_args(&["f32", "i32"], &["a", "b"], "f32") == Ok("a, f32(b)".to_string()), "强类型合规");
    s.add("反向混型", coerce_args(&["i32", "f32"], &["x", "y"], "i32") == Ok("x, i32(y)".to_string()), "目标主导");
    s.add("长度不齐拒", matches!(coerce_args(&["f32"], &["a", "b"], "f32"), Err(XC_LEN)), "维度校验");
    s.add("空参拒", matches!(coerce_args(&[], &[], "f32"), Err(XC_LEN)), "空守卫");

    // 族三：控制流第三用。
    s.add("if 形态", gen_flow(Rgn::If, "x > 0", "y = 1;") == Ok("if (x > 0) { y = 1; }".to_string()), "WGSL 语法");
    s.add("loop 形态", gen_flow(Rgn::Loop, "", "i++;").is_ok(), "loop 无条件");
    s.add("守卫 continue", gen_flow(Rgn::Guard, "done", "") == Ok("if (done[0]) { continue; }".to_string()), "F0503 旗对应");
    s.add("空条件拒", matches!(gen_flow(Rgn::If, "", "x;"), Err(XC_NOCOND)), "条件必填");

    // 族四：黄金对拍。
    for g in W_GOLDENS.iter() {
        s.add("WGSL 黄金", gen_let(g.op, g.dst, g.args) == Ok(g.want.to_string()), g.op);
    }
    s.add("黄金三对", W_GOLDENS.len() == 3, "独立正本");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen() {
        assert_eq!(gen_let("imin", "m", "a, b").ok(), Some("let m = min(a, b);".to_string()));
        assert!(builtin("rsq").is_err());
    }
}
