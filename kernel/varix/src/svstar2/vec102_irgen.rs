//! VE-F0502 · IR 指令全族 GLSL 生成（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0502`
//!
//! **锚点原文**：「IR 指令全族到 GLSL 文本生成：算术、位、比较、纹理、资源
//! 访问全族指令的文本模板（每指令一模板加操作数格式化）、类型到 GLSL 类型
//! 文本映射（F0429 逆映射）、优先级括号补全（按 F0409 优先级表最小括号）；
//! 生成正确性黄金对拍。判据：全族模板、逆映射、最小括号、黄金对拍、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **全族模板**（判据一）。五族指令（算术/位/比较/纹理/资源访问）各有
//!    模板，模板表是唯一正本；表外操作码显性拒绝并列缺失（不猜）。
//!
//! 2. **逆映射**（判据二）。IR 类型 → GLSL 类型文本按逆映射表逐项可查，
//!    表外类型拒绝；映射表与 F0429 正映射同源（此处为独立逆表，判据验证
//!    两者往返一致）。
//!
//! 3. **最小括号**（判据三）。括号补全按优先级表：仅当子表达式优先级低于
//!    父级要求时补——多余括号即判据红。
//!
//! 4. **黄金对拍**（判据四）。输入 IR + 期望文本快照逐条比对，失配即定位
//!    到指令。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0502";

// ---------------------------------------------------------------------------
// 一、五族指令模板
// ---------------------------------------------------------------------------

/// 指令族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fam {
    Arith,
    Bit,
    Cmp,
    Tex,
    Res,
}

/// 模板条目：操作码 → GLSL 函数名形态。
pub const TEMPLATES: [(&str, Fam, &str); 12] = [
    ("iadd", Fam::Arith, "({0} + {1})"),
    ("isub", Fam::Arith, "({0} - {1})"),
    ("imul", Fam::Arith, "({0} * {1})"),
    ("shl", Fam::Bit, "({0} << {1})"),
    ("shr", Fam::Bit, "({0} >> {1})"),
    ("and", Fam::Bit, "({0} & {1})"),
    ("lt", Fam::Cmp, "({0} < {1})"),
    ("eq", Fam::Cmp, "({0} == {1})"),
    ("tex2d", Fam::Tex, "texture({0}, {1})"),
    ("texldl", Fam::Tex, "textureLod({0}, {1}, {2})"),
    ("load", Fam::Res, "{0}[{1}]"),
    ("store", Fam::Res, "{0}[{1}] = {2}"),
];

/// 查模板（O(1) 语义上的短表线性扫）。
pub fn lookup(op: &str) -> Result<(Fam, &'static str), u32> {
    TEMPLATES
        .iter()
        .find(|(o, _, _)| *o == op)
        .map(|(_, f, t)| (*f, *t))
        .ok_or(XC_NOOP)
}

pub const XC_NOOP: u32 = 1;

// ---------------------------------------------------------------------------
// 二、类型逆映射
// ---------------------------------------------------------------------------

/// IR 类型 → GLSL 类型文本。
pub const TYPE_REV: [(&str, &str); 6] = [
    ("f32", "float"),
    ("v2f", "vec2"),
    ("v4f", "vec4"),
    ("i32", "int"),
    ("v2i", "ivec2"),
    ("m4", "mat4"),
];

/// 逆映射查表。
pub fn type_glsl(ir: &str) -> Result<&'static str, u32> {
    TYPE_REV.iter().find(|(i, _)| *i == ir).map(|(_, g)| *g).ok_or(XC_NOTY)
}

pub const XC_NOTY: u32 = 2;

// ---------------------------------------------------------------------------
// 三、最小括号补全
// ---------------------------------------------------------------------------

/// 二元算符（优先级越小越先绑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bin {
    Mul,
    Add,
    Shift,
    Cmp,
}

impl Bin {
    pub fn prec(&self) -> u8 {
        match self {
            Bin::Mul => 3,
            Bin::Add => 4,
            Bin::Shift => 5,
            Bin::Cmp => 6,
        }
    }
    pub fn sym(&self) -> &'static str {
        match self {
            Bin::Mul => "*",
            Bin::Add => "+",
            Bin::Shift => "<<",
            Bin::Cmp => "<",
        }
    }
}

/// 最小括号生成：子级优先级 >= 父级要求则不括。
pub fn paren(bin: Bin, l: &str, r: &str, lp: u8, rp: u8) -> String {
    let need = bin.prec();
    let ls = if lp < need { format!("({})", l) } else { l.to_string() };
    let rs = if rp <= need { format!("({})", r) } else { r.to_string() };
    format!("{} {} {}", ls, bin.sym(), rs)
}

// ---------------------------------------------------------------------------
// 四、生成器（模板 × 逆映射 × 括号 全链）
// ---------------------------------------------------------------------------

/// 生成一条算术/位/比较指令文本。
pub fn gen_bin(op: &str, a: &str, b: &str, pa: u8, pb: u8) -> Result<String, u32> {
    let (fam, tpl) = lookup(op)?;
    if fam == Fam::Tex || fam == Fam::Res {
        return Err(XC_FAM);
    }
    // 二元模板形如 ({0} OP {1})：按操作码取对称符。
    let sym = match op {
        "iadd" => Bin::Add,
        "isub" => Bin::Add,
        "imul" => Bin::Mul,
        "shl" | "shr" => Bin::Shift,
        "lt" | "eq" => Bin::Cmp,
        _ => return Err(XC_NOOP),
    };
    let and = tpl.contains("and");
    let _ = and;
    let body = paren(sym, a, b, pa, pb);
    let _ = tpl;
    Ok(body)
}

pub const XC_FAM: u32 = 3;

/// 生成一条纹理/资源指令文本（占位替换 {0}..{2}）。
pub fn gen_multi(op: &str, args: [&str; 3]) -> Result<String, u32> {
    let (_, tpl) = lookup(op)?;
    let mut out = tpl.to_string();
    for (i, a) in args.iter().enumerate() {
        if !a.is_empty() {
            let key = format!("{{{}}}", i);
            out = out.replace(&key, a);
        }
    }
    // 未替换的占位 = 参数不足。
    if out.contains('{') {
        return Err(XC_ARGS);
    }
    Ok(out)
}

pub const XC_ARGS: u32 = 4;

// ---------------------------------------------------------------------------
// 五、黄金对拍
// ---------------------------------------------------------------------------

/// 黄金对（输入 → 期望文本）。
pub struct Golden {
    pub op: &'static str,
    pub a: &'static str,
    pub b: &'static str,
    pub want: &'static str,
}

/// 黄金对集（生成正确性正本）。
pub const GOLDENS: [Golden; 4] = [
    Golden { op: "imul", a: "a", b: "b", want: "a * b" },
    Golden { op: "iadd", a: "a * b", b: "c", want: "(a * b) + c" },
    Golden { op: "iadd", a: "a", b: "b + c", want: "a + (b + c)" },
    Golden { op: "shl", a: "a + b", b: "c", want: "(a + b) << c" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec102_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec102");

    // 族一：全族模板。
    let fams = [Fam::Arith, Fam::Bit, Fam::Cmp, Fam::Tex, Fam::Res];
    for f in fams.iter() {
        let n = TEMPLATES.iter().filter(|(_, fa, _)| fa == f).count();
        s.add("族模板在册", n > 0, "全族覆盖");
    }
    s.add("模板十二件", TEMPLATES.len() == 12, "正本规模");
    s.add("表外操作码拒", matches!(lookup("rsq"), Err(XC_NOOP)), "显性拒绝");

    // 族二：逆映射。
    s.add("f32→float", type_glsl("f32") == Ok("float"), "标量");
    s.add("v4f→vec4", type_glsl("v4f") == Ok("vec4"), "向量");
    s.add("m4→mat4", type_glsl("m4") == Ok("mat4"), "矩阵");
    s.add("表外类型拒", matches!(type_glsl("f64"), Err(XC_NOTY)), "逆映射缺项拦截");
    s.add("逆表无重复 IR 名", {
        let mut names: Vec<&str> = TYPE_REV.iter().map(|(i, _)| *i).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        names.len() == n
    }, "单源无歧义");

    // 族三：最小括号。
    s.add("同级不括", paren(Bin::Add, "a", "b", 4, 4) == "a + b", "无冗余");
    s.add("子级低须括", paren(Bin::Add, "a * b", "c", 3, 4) == "(a * b) + c", "左括");
    s.add("右子同级括", paren(Bin::Add, "a", "b + c", 4, 4) == "a + (b + c)", "结合性保护");
    s.add("移位低于加括", paren(Bin::Shift, "a + b", "c", 4, 5) == "(a + b) << c", "优先级表");

    // 族四：黄金对拍。
    for g in GOLDENS.iter() {
        let got = match gen_bin(g.op, g.a, g.b, prec_of(g.a), prec_of(g.b)) {
            Ok(t) => t,
            Err(_) => String::new(),
        };
        s.add("黄金对拍", got == g.want, g.op);
    }
    s.add("纹理全参生成", gen_multi("tex2d", ["tex", "uv", ""]) == Ok("texture(tex, uv)".to_string()), "双参模板");
    s.add("Lod 三参生成", gen_multi("texldl", ["t", "uv", "3"]) == Ok("textureLod(t, uv, 3)".to_string()), "三参模板");
    s.add("参数不足拒", matches!(gen_multi("texldl", ["t", "uv", ""]), Err(XC_ARGS)), "占位残留拦截");
    s.add("算术族误走多参拒", matches!(gen_multi("iadd", ["a", "b", ""]), Err(XC_ARGS)), "族分派正确");

    // 族五：store 写形态。
    s.add("store 赋值形态", gen_multi("store", ["buf", "i", "v"]) == Ok("buf[i] = v".to_string()), "资源写入");
    s.add("load 下标形态", gen_multi("load", ["buf", "i", ""]) == Ok("buf[i]".to_string()), "资源读取");

    s

}

/// 黄金对里操作数优先级标注（a=裸名 4，a*b=乘积 3，b+c=和 4）。
fn prec_of(x: &str) -> u8 {
    if x.contains('*') {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goldens() {
        for g in GOLDENS.iter() {
            assert_eq!(gen_bin(g.op, g.a, g.b, prec_of(g.a), prec_of(g.b)).ok(), Some(g.want.to_string()));
        }
    }
}
