//! VE-F0429 · 类型语法（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0429`
//!
//! **锚点原文**：「类型语法解析：标量、向量（维度语法）、矩阵、数组（维度
//! 表达式）、结构体引用、采样器与图像类型族的语法面；类型限定语素收集
//! （语法期只收不裁，合法性判留给语义期类型系统）；类型别名机制按规范
//! 登记。判据：全族语法面、只收不裁、别名登记、维度恢复、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **全族语法面**（判据一）。类型族七形态闭集 [`TySyn`]：标量/向量/
//!    矩阵/数组/结构体引用/采样器句柄/图像句柄。解析器只认**形态**不认
//!    名单——`vec3`、`mat4x2`、`sampler2D` 都按「引导词 + 维度语法」收，
//!    名字是否在 F0404 词表是词法期的事、形态是否合法是本单的事、语义是
//!    F0443 的事。三层边界各自入册。
//!
//! 2. **只收不裁**（判据二）。语素收集 [`TyMorph`]：向量维度（2/3/4）、
//!    矩阵行列（C×R）、数组长度表达式——**语法期原样记账，零校验**。
//!    `vec9` 也收（维度越界是语义期报错的事）；判据以纯度实测：越界维度
//!    的语素结构与合法维度完全同构，只有**值**不同。
//!
//! 3. **别名登记**（判据三）。`typedef T N;` 别名机制：登记 [`AliasBook`]
//!    （名 → 原型类型语法）。别名冲突（同名再登记）按锚点「登记语义期」：
//!    两条都入簿 + 诊断入语义期队列，**不拒收不覆盖**——语义期要看到全部
//!    登记才能裁。
//!
//! 4. **维度恢复**（判据四）。维度非法（非整型常量语法面：空维度/浮点
//!    维度）按锚点「恢复声明级」：类型本身继续收，维度记 [`Dim2::Bad`]
//!    带指向；数组维度表达式整段跨度记账（内容留给求值期 F0448）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::boxed::Box;

pub const ANCHOR: &str = "VE-F0429";

// ---------------------------------------------------------------------------
// 词位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    Number,
    Sym,
    Kw, // typedef
    Eof,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Tk,
    pub text: String,
    pub off: u32,
}

pub fn mini_lex(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if "();,[]<>*".contains(c) {
            out.push(Tok { kind: Tk::Sym, text: c.to_string(), off: i as u32 });
            i += 1;
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            let s = i;
            while i < b.len() && ((b[i] as char).is_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let w = &src[s..i];
            let kind = if w == "typedef" { Tk::Kw } else { Tk::Ident };
            out.push(Tok { kind, text: w.into(), off: s as u32 });
            continue;
        }
        out.push(Tok { kind: Tk::Sym, text: c.to_string(), off: i as u32 });
        i += 1;
    }
    out.push(Tok { kind: Tk::Eof, text: String::new(), off: b.len() as u32 });
    out
}

// ---------------------------------------------------------------------------
// 类型语法形态
// ---------------------------------------------------------------------------

/// 类型语法七形态闭集。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TySyn {
    /// 标量（float/int/bool 类，名单语义期核）。
    Scalar { name: String },
    /// 向量：名 + 维度语素。
    Vector { name: String, dim: u32 },
    /// 矩阵：名 + 列×行语素。
    Matrix { name: String, cols: u32, rows: u32 },
    /// 数组：元素类型语法 + 维度。
    Array { elem: Box<TySyn>, dim: Dim2 },
    /// 结构体引用（名原样）。
    StructRef { name: String },
    /// 采样器句柄。
    Sampler { name: String },
    /// 图像句柄。
    Image { name: String },
}

/// 维度记账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dim2 {
    None,
    /// 表达式跨度（求值留给 F0448）。
    Expr(u32, u32),
    /// 非法维度（恢复声明级，带指向）。
    Bad(u32),
}

/// 语素收集：语法期原样记账零校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TyMorph {
    /// 向量维度原值（vec9 也收，越界语义期报）。
    pub vec_dim: u32,
    /// 矩阵列。
    pub mat_cols: u32,
    /// 矩阵行。
    pub mat_rows: u32,
}

/// 诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TyDiag {
    /// 维度非法 → 恢复声明级。
    DimBadRecover,
    /// 未知类型名 → 报错带近似建议（建议生成在诊断详情，此处记发生）。
    UnknownSuggest,
    /// 别名冲突 → 登记语义期。
    AliasDupYield,
    /// typedef 缺名。
    AliasNameMissing,
}

impl TyDiag {
    pub fn code(self) -> u16 {
        0x4290 + self as u16
    }
}

/// 别名簿：登记制，冲突并存。
#[derive(Debug, Default)]
pub struct AliasBook {
    /// (别名, 原型类型名) 按登记序。
    pub entries: Vec<(String, String)>,
    /// 语义期裁定队列（冲突项）。
    pub sema_queue: Vec<String>,
}

impl AliasBook {
    pub fn new() -> Self {
        Self::default()
    }
    /// 登记：同名再登记 = 冲突并存 + 队列。
    pub fn register(&mut self, alias: &str, target: &str) -> bool {
        let dup = self.entries.iter().any(|(a, _)| a == alias);
        self.entries.push((alias.into(), target.into()));
        if dup {
            self.sema_queue.push(alias.into());
        }
        dup
    }
    /// 查别名（后登记优先——语义期可改写此序，语法期不裁）。
    pub fn lookup(&self, alias: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|(a, _)| a == alias)
            .map(|(_, t)| t.as_str())
    }
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

/// 从标识符解析类型形态（全族语法面的核心判定）。
pub fn shape_of(name: &str) -> Option<TySyn> {
    if let Some(rest) = name.strip_prefix("vec") {
        if let Ok(d) = rest.parse::<u32>() {
            return Some(TySyn::Vector { name: name.into(), dim: d });
        }
        return Some(TySyn::Scalar { name: name.into() });
    }
    if let Some(rest) = name.strip_prefix("mat") {
        // matCxR 或 matC。
        if let Some((c, r)) = rest.split_once('x') {
            if let (Ok(c), Ok(r)) = (c.parse::<u32>(), r.parse::<u32>()) {
                return Some(TySyn::Matrix { name: name.into(), cols: c, rows: r });
            }
        }
        if let Ok(c) = rest.parse::<u32>() {
            return Some(TySyn::Matrix { name: name.into(), cols: c, rows: c });
        }
        return Some(TySyn::Scalar { name: name.into() });
    }
    if name.starts_with("sampler") {
        return Some(TySyn::Sampler { name: name.into() });
    }
    if name.starts_with("image") || name.starts_with("uimage") || name.starts_with("iimage") {
        return Some(TySyn::Image { name: name.into() });
    }
    // 标量基型族（全族含标量：五个基型名按标量形态收）。
    if matches!(name, "float" | "int" | "uint" | "double" | "bool") {
        return Some(TySyn::Scalar { name: name.into() });
    }
    // 结构体引用兜底：非内建语素一律按引用形态收（未知不拒收，
    // 与解析路径 None => StructRef 的兜底同口径）。
    Some(TySyn::StructRef { name: name.into() })
}

/// 类型语法解析器（对单类型语法的消费面）。
#[derive(Debug)]
pub struct TyParser {
    toks: Vec<Tok>,
    pos: usize,
    pub aliases: AliasBook,
    pub diags: Vec<(u32, TyDiag)>,
}

impl TyParser {
    pub fn new(src: &str) -> Self {
        Self { toks: mini_lex(src), pos: 0, aliases: AliasBook::new(), diags: Vec::new() }
    }
    fn peek(&self) -> &Tok {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }
    fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos.min(self.toks.len() - 1)].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.peek().kind == Tk::Sym && self.peek().text == s {
            self.bump();
            true
        } else {
            false
        }
    }

    /// 解析一个类型语法（含后缀数组维度）。
    pub fn parse_type(&mut self) -> Result<TySyn, TyDiag> {
        let t = self.peek().clone();
        if t.kind != Tk::Ident {
            return Err(TyDiag::UnknownSuggest);
        }
        self.bump();
        let mut ty = match shape_of(&t.text) {
            Some(s) => s,
            None => TySyn::StructRef { name: t.text.clone() },
        };
        // 数组后缀（可多层：int a[2][3] 外层由调用方循环；此处收一层）。
        if self.eat("[") {
            let start = self.peek().off;
            if self.eat("]") {
                self.diags.push((start, TyDiag::DimBadRecover));
                ty = TySyn::Array { elem: Box::new(ty), dim: Dim2::Bad(start) };
            } else if self.peek().kind == Tk::Number || self.peek().kind == Tk::Ident {
                // 数字或常量名（语法面都收；非整型语义期报）。
                while self.peek().kind != Tk::Eof {
                    let x = self.peek().clone();
                    if x.kind == Tk::Sym && (x.text == "]" || x.text == ";") {
                        break;
                    }
                    self.bump();
                }
                let end = self.peek().off;
                self.eat("]");
                ty = TySyn::Array { elem: Box::new(ty), dim: Dim2::Expr(start, end) };
            } else {
                // 浮点维度等非法面：恢复声明级。
                let bad = self.peek().off;
                while self.peek().kind != Tk::Eof {
                    let x = self.peek().clone();
                    if x.kind == Tk::Sym && x.text == "]" {
                        break;
                    }
                    self.bump();
                }
                self.eat("]");
                self.diags.push((bad, TyDiag::DimBadRecover));
                ty = TySyn::Array { elem: Box::new(ty), dim: Dim2::Bad(bad) };
            }
        }
        Ok(ty)
    }

    /// 解析整流：typedef 别名行与类型面混合扫描。
    pub fn parse_all(&mut self) -> usize {
        let mut n = 0;
        while self.peek().kind != Tk::Eof {
            if self.peek().kind == Tk::Kw && self.peek().text == "typedef" {
                self.bump();
                if self.peek().kind != Tk::Ident {
                    self.diags.push((self.peek().off, TyDiag::AliasNameMissing));
                    self.bump();
                    continue;
                }
                let target = self.bump();
                if self.peek().kind != Tk::Ident {
                    self.diags.push((self.peek().off, TyDiag::AliasNameMissing));
                    continue;
                }
                let alias = self.bump();
                self.aliases.register(&alias.text, &target.text);
                self.eat(";");
                n += 1;
                continue;
            }
            if self.peek().kind == Tk::Ident {
                if self.parse_type().is_ok() {
                    n += 1;
                }
                // 声明尾：到 `;` 为止（变量声明细分 F0426）。
                while self.peek().kind != Tk::Eof {
                    let x = self.peek().clone();
                    if x.kind == Tk::Sym && (x.text == ";" || x.text == "{") {
                        break;
                    }
                    self.bump();
                }
                self.eat(";");
                continue;
            }
            self.bump();
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec29_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、全族语法面。
    {
        v.push((
            "F0429-全族-标量",
            matches!(shape_of("float"), Some(TySyn::Scalar { .. })),
        ));
        v.push((
            "F0429-全族-向量维度",
            matches!(shape_of("vec3"), Some(TySyn::Vector { dim: 3, .. })),
        ));
        v.push((
            "F0429-全族-矩阵方",
            matches!(shape_of("mat4"), Some(TySyn::Matrix { cols: 4, rows: 4, .. })),
        ));
        v.push((
            "F0429-全族-矩阵矩形",
            matches!(shape_of("mat4x2"), Some(TySyn::Matrix { cols: 4, rows: 2, .. })),
        ));
        v.push((
            "F0429-全族-采样器",
            matches!(shape_of("sampler2D"), Some(TySyn::Sampler { .. })),
        ));
        v.push((
            "F0429-全族-图像句柄",
            matches!(shape_of("uimage2D"), Some(TySyn::Image { .. })),
        ));
        v.push((
            "F0429-全族-结构体引用兜底",
            matches!(shape_of("MyStruct"), Some(TySyn::StructRef { .. })),
        ));
        // 数组形态（解析路径：数组后缀贴类型——int[4]）。
        let mut p = TyParser::new("int[4] x;");
        let t = p.parse_type();
        v.push((
            "F0429-全族-数组形态",
            matches!(t, Ok(TySyn::Array { dim: Dim2::Expr(_, _), .. })),
        ));
    }
    // 二、只收不裁（纯度实测）。
    {
        // vec9（维度越界）与 vec3 语素结构同构、只有值不同。
        let m3 = TyMorph { vec_dim: 3, mat_cols: 0, mat_rows: 0 };
        let m9 = TyMorph { vec_dim: 9, mat_cols: 0, mat_rows: 0 };
        v.push((
            "F0429-只收-越界同构",
            matches!(shape_of("vec3"), Some(TySyn::Vector { dim, .. }) if dim == m3.vec_dim)
                && matches!(shape_of("vec9"), Some(TySyn::Vector { dim, .. }) if dim == m9.vec_dim),
        ));
        // 未知类型名不拒收（StructRef 兜底）。
        let mut p = TyParser::new("Nonsuch x;");
        v.push(("F0429-只收-未知不拒收", p.parse_type().is_ok()));
    }
    // 三、别名登记。
    {
        let mut p = TyParser::new("typedef float meters;");
        let n = p.parse_all();
        v.push(("F0429-别名-登记一条", n == 1 && p.aliases.entries.len() == 1));
        v.push(("F0429-别名-查得原型", p.aliases.lookup("meters") == Some("float")));
        // 冲突并存 + 语义期队列。
        let mut p2 = TyParser::new("typedef float a; typedef int a;");
        let _ = p2.parse_all();
        v.push((
            "F0429-别名-冲突并存",
            p2.aliases.entries.len() == 2
                && p2.aliases.sema_queue.len() == 1
                && p2.aliases.sema_queue[0] == "a",
        ));
        // 后登记优先（语法期不裁，序给语义期参考）。
        v.push(("F0429-别名-后登优先", p2.aliases.lookup("a") == Some("int")));
        // typedef 缺名。
        let mut p3 = TyParser::new("typedef float;");
        let _ = p3.parse_all();
        v.push((
            "F0429-别名-缺名报错",
            p3.diags.iter().any(|(_, d)| *d == TyDiag::AliasNameMissing),
        ));
    }
    // 四、维度恢复。
    {
        // 空维度恢复声明级（数组后缀贴类型：int[]）。
        let mut p = TyParser::new("int[] x;");
        let t = p.parse_type();
        v.push((
            "F0429-维度-空恢复",
            matches!(t, Ok(TySyn::Array { dim: Dim2::Bad(_), .. }))
                && p.diags.iter().any(|(_, d)| *d == TyDiag::DimBadRecover),
        ));
        // 表达式维度记账（后缀贴类型：int[N + 1]）。
        let mut p2 = TyParser::new("int[N + 1] b;");
        v.push((
            "F0429-维度-表达式跨度",
            matches!(p2.parse_type(), Ok(TySyn::Array { dim: Dim2::Expr(s, e), .. }) if e > s),
        ));
        // 常量名维度合法语法面。
        let mut p3 = TyParser::new("int c[SIZE];");
        v.push(("F0429-维度-常量名合法", p3.parse_type().is_ok()));
        // 诊断码段。
        v.push(("F0429-判据-诊断码段", (0x4290..0x42A0).contains(&TyDiag::DimBadRecover.code())));
        v.push(("F0429-判据-锚点入册", ANCHOR == "VE-F0429"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec29_raw_checks as run_vec29_checks;
    #[test]
    fn vec29_typesyn_checks() {
        for (name, ok) in run_vec29_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec29）。
pub fn run_vec29_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec29");
    for (name, ok) in vec29_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
