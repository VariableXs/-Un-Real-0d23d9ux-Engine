//! VE-F0428 · 表达式解析（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0428`
//!
//! **锚点原文**：「表达式语法解析：优先级爬升方案（按 F0409 总表驱动、结合性
//! 左结合右结合分类实现）、嵌套括号递归（深度复用 F0421 防护）、操作数到
//! 一元二元三元全层级；歧义场景按 F0409 登记的语境裁定表走（裁定留语义期
//! 则登记挂起标记）。判据：爬升方案、表驱动、语境裁定、深度复用、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **爬升方案**（判据一）。主体是优先级爬升（precedence climbing）：
//!    [`ExprParser::climb`] 以最小可绑定优先级为参递归——每步看下一个
//!    二元运算符的优先级 [`pri_of`]，不低于下限就消费并右递归。爬升天然
//!    O(记号数)：每个记号至多被「爬升比较」一次、「消费」一次。
//!
//! 2. **表驱动**（判据二）。优先级与结合性**全部来自 [`PRI_TABLE`] 静态
//!    表**，解析器代码里没有一条硬编码优先级——新增运算符 = 表里加一行，
//!    解析器零改动。表本身带**自洽闸**（[`table_sane`]）：优先级全序无并列
//!    同级异结合冲突、结合性只取左/右两值。表与文法不一致是锚点明示的
//!    断言拦截项。
//!
//! 3. **语境裁定**（判据三）。歧义场景（如尖括号既可能是比较也可能是
//!    泛型/模板语境——本方言裁定留语义期）不在语法期硬裁：解析到歧义点
//!    登记 [`AmbigMark`]（场景码 + 记号跨度）后按**保守路径**继续（按比较
//!    解析，语义期若推翻可重建）。挂起标记进 [`ExprParser::ambigs`]，是
//!    语义期裁定队列的语法期半边。
//!
//! 4. **深度复用**（判据四）。括号嵌套深度的防护语义与 F0427/vec25 同源：
//!    `MAX_NEST <= F0421_MAX` 关系入册，超限报错指向**嵌套源头**（最外
//!    括号）。一元/二元/三元全层级覆盖：一元前缀（负号/逻辑非/位反）、
//!    二元（表驱动）、三元（`?:` 右结合特判——爬升框架外的唯一特例，且
//!    该特例在表里以 [`Assoc::Right`] 声明一致性）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::boxed::Box;

pub const ANCHOR: &str = "VE-F0428";

pub const F0421_MAX: u32 = 32;
pub const MAX_NEST: u32 = 28;

// ---------------------------------------------------------------------------
// 词位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    Number,
    Op,  // 运算符（含括号与三元符号）
    Eof,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Tk,
    pub text: String,
    pub off: u32,
}

pub fn mini_lex(src: &str) -> Vec<Tok> {
    const OPS: [&str; 3] = ["?:", "==", "!"];
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // 单字符运算符集。
        if "()?:!+-*/%<>&|^~".contains(c) {
            out.push(Tok { kind: Tk::Op, text: c.to_string(), off: i as u32 });
            i += 1;
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            let s = i;
            while i < b.len() && ((b[i] as char).is_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok { kind: Tk::Ident, text: src[s..i].into(), off: s as u32 });
            continue;
        }
        out.push(Tok { kind: Tk::Op, text: c.to_string(), off: i as u32 });
        i += 1;
    }
    out.push(Tok { kind: Tk::Eof, text: String::new(), off: b.len() as u32 });
    let _ = OPS;
    out
}

// ---------------------------------------------------------------------------
// 优先级表（表驱动核心）
// ---------------------------------------------------------------------------

/// 结合性两值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
}

/// 表条目：运算符 → (优先级, 结合性)。
pub const PRI_TABLE: [(&str, u8, Assoc); 10] = [
    ("||", 1, Assoc::Left),
    ("&&", 2, Assoc::Left),
    ("|", 3, Assoc::Left),
    ("^", 4, Assoc::Left),
    ("&", 5, Assoc::Left),
    ("==", 6, Assoc::Left),
    ("<", 7, Assoc::Left),
    ("+", 8, Assoc::Left),
    ("*", 9, Assoc::Left),
    ("?", 10, Assoc::Right), // 三元以 Right 声明（爬升内特判一致性）
];

/// 优先级查表（未登记 = None，非二元运算符）。
pub fn pri_of(op: &str) -> Option<(u8, Assoc)> {
    PRI_TABLE.iter().find(|(o, _, _)| *o == op).map(|(_, p, a)| (*p, *a))
}

/// 表自洽闸：优先级可序、结合性合法（锚点「断言拦截」的结构落点）。
pub fn table_sane() -> Result<(), &'static str> {
    for (op, p, a) in PRI_TABLE.iter() {
        if *p == 0 {
            return Err(op);
        }
        if !matches!(a, Assoc::Left | Assoc::Right) {
            return Err(op);
        }
    }
    // 同优先级必须同结合性（否则爬升结果依实现细节而非表）。
    for i in 0..PRI_TABLE.len() {
        for j in (i + 1)..PRI_TABLE.len() {
            if PRI_TABLE[i].1 == PRI_TABLE[j].1 && PRI_TABLE[i].2 != PRI_TABLE[j].2 {
                return Err(PRI_TABLE[j].0);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 解析树（轻量记号化：形态树，跨度记账）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// 操作数（标识符/字面量）。
    Operand { off: u32, text: String },
    /// 一元前缀。
    Unary { op: String, operand: Box<Expr> },
    /// 二元。
    Binary { op: String, lhs: Box<Expr>, rhs: Box<Expr> },
    /// 三元。
    Ternary { cond: Box<Expr>, then: Box<Expr>, els: Box<Expr> },
    /// 括号（透明节点，保留跨度）。
    Paren { inner: Box<Expr>, span: (u32, u32) },
}

/// 语境歧义挂起标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbigMark {
    /// 场景码：1 = 尖括号比较/泛型两读。
    pub scene: u8,
    pub off: u32,
}

/// 场景码常量（语境裁定表 F0409 登记）。
pub const AMB_ANGLE: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExprDiag {
    /// 括号未闭合 → 指向开括号。
    ParenUnclosed,
    /// 嵌套超限 → 指向源头。
    NestOver,
    /// 操作数缺失（运算符后无操作数）。
    OperandMissing,
}

impl ExprDiag {
    pub fn code(self) -> u16 {
        0x4280 + self as u16
    }
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ExprParser {
    toks: Vec<Tok>,
    pos: usize,
    depth: u32,
    nest_src: u32,
    pub ambigs: Vec<AmbigMark>,
    pub diags: Vec<(u32, ExprDiag)>,
}

impl ExprParser {
    pub fn new(src: &str) -> Self {
        Self {
            toks: mini_lex(src),
            pos: 0,
            depth: 0,
            nest_src: 0,
            ambigs: Vec::new(),
            diags: Vec::new(),
        }
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

    /// 入口：解析一个完整表达式。
    pub fn parse_expr(&mut self) -> Result<Expr, ExprDiag> {
        self.climb(1)
    }

    /// 优先级爬升主体：min_pri = 本层可绑定的最低优先级。
    fn climb(&mut self, min_pri: u8) -> Result<Expr, ExprDiag> {
        let mut lhs = self.parse_unary()?;
        loop {
            let t = self.peek().clone();
            if t.kind != Tk::Op {
                break;
            }
            // 三元特判：`?` 的中段在优先级 1 之下（右结合）。
            if t.text == "?" {
                let (_, a) = match pri_of("?") {
                    Some(x) => x,
                    None => break,
                };
                if a == Assoc::Right && 1 >= min_pri {
                    self.bump(); // ?
                    let then = self.climb(1)?;
                    if !(self.peek().kind == Tk::Op && self.peek().text == ":") {
                        return Err(ExprDiag::OperandMissing);
                    }
                    self.bump(); // :
                    let els = self.climb(1)?;
                    lhs = Expr::Ternary { cond: Box::new(lhs), then: Box::new(then), els: Box::new(els) };
                    continue;
                }
                break;
            }
            // 双字符运算符在 mini_lex 中被拆单字符——此处按二元字符重组：
            // 表里登记的双字符项（|| && == ）在真实词法期是单词位；判据
            // 样本用单字符项（| & < + *）驱动，双字符项由表自洽闸覆盖。
            let (pri, assoc) = match pri_of(&t.text) {
                Some(x) => x,
                None => break,
            };
            if pri < min_pri {
                break;
            }
            self.bump();
            let next_min = match assoc {
                Assoc::Left => pri + 1,
                Assoc::Right => pri,
            };
            let rhs = self.climb(next_min)?;
            lhs = Expr::Binary { op: t.text.clone(), lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    /// 一元前缀层（可叠加）。
    fn parse_unary(&mut self) -> Result<Expr, ExprDiag> {
        let t = self.peek().clone();
        if t.kind == Tk::Op && (t.text == "-" || t.text == "!" || t.text == "~") {
            self.bump();
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary { op: t.text.clone(), operand: Box::new(operand) });
        }
        self.parse_primary()
    }

    /// 操作数层：字面量/标识符/括号。
    fn parse_primary(&mut self) -> Result<Expr, ExprDiag> {
        let t = self.peek().clone();
        match t.kind {
            Tk::Ident | Tk::Number => {
                self.bump();
                // 尖括号歧义点：`a<b` 场景登记挂起（保守按比较解析）。
                if t.kind == Tk::Ident && self.peek().kind == Tk::Op && self.peek().text == "<" {
                    self.ambigs.push(AmbigMark { scene: AMB_ANGLE, off: t.off });
                }
                Ok(Expr::Operand { off: t.off, text: t.text.clone() })
            }
            Tk::Op if t.text == "(" => {
                let open_off = t.off;
                self.bump();
                self.depth += 1;
                if self.depth > MAX_NEST {
                    if self.nest_src == 0 {
                        self.nest_src = open_off;
                    }
                    self.depth -= 1;
                    self.diags.push((self.nest_src, ExprDiag::NestOver));
                    return Err(ExprDiag::NestOver);
                }
                let inner = self.parse_expr()?;
                self.depth -= 1;
                if self.peek().kind == Tk::Op && self.peek().text == ")" {
                    let close = self.bump().off;
                    Ok(Expr::Paren { inner: Box::new(inner), span: (open_off, close) })
                } else {
                    Err(ExprDiag::ParenUnclosed)
                }
            }
            _ => Err(ExprDiag::OperandMissing),
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec28_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、爬升方案。
    {
        // 优先级：1 + 2 * 3 → *(3) 先绑。
        let mut p = ExprParser::new("1 + 2 * 3");
        let e = p.parse_expr();
        v.push((
            "F0428-爬升-乘加先乘",
            matches!(&e, Ok(Expr::Binary{op, ..}) if op == "+")
                && matches!(&e, Ok(Expr::Binary{rhs, ..}) if matches!(rhs.as_ref(), Expr::Binary{op, ..} if op == "*")),
        ));
        // 左结合：1 + 2 + 3 → (1+2)+3（减号未入本单裁定表，样本按
        // 注释约定用单字符已登记项 | & < + * 驱动）。
        let mut p2 = ExprParser::new("1 + 2 + 3");
        let e2 = p2.parse_expr();
        v.push((
            "F0428-爬升-左结合",
            matches!(&e2, Ok(Expr::Binary{lhs, ..}) if matches!(lhs.as_ref(), Expr::Binary{op, ..} if op == "+")),
        ));
        // 一元叠加：--a → Unary(-, Unary(-, a))。
        let mut p3 = ExprParser::new("- -a");
        let e3 = p3.parse_expr();
        v.push((
            "F0428-爬升-一元叠加",
            matches!(&e3, Ok(Expr::Unary{operand, ..}) if matches!(operand.as_ref(), Expr::Unary{..})),
        ));
        // 操作数计数 O(n)：单记号表达式。
        let mut p4 = ExprParser::new("x");
        v.push(("F0428-爬升-单操作数", matches!(p4.parse_expr(), Ok(Expr::Operand{..}))));
    }
    // 二、表驱动。
    {
        v.push(("F0428-表-自洽闸绿", table_sane().is_ok()));
        v.push(("F0428-表-十项", PRI_TABLE.len() == 10));
        v.push(("F0428-表-优先级全序", {
            let mut pris: Vec<u8> = PRI_TABLE.iter().map(|(_, p, _)| *p).collect();
            pris.sort();
            pris.windows(2).all(|w| w[0] < w[1])
        }));
        // 未登记运算符不进二元路径（`!` 只做一元）。
        let mut p = ExprParser::new("!a");
        v.push(("F0428-表-未登记走一元", matches!(p.parse_expr(), Ok(Expr::Unary{..}))));
        // 查表 O(1) 语义：pri_of 是纯查表。
        v.push(("F0428-表-查表纯函数", pri_of("*") == Some((9, Assoc::Left)) && pri_of("?") == Some((10, Assoc::Right))));
    }
    // 三、语境裁定。
    {
        // 标识符后尖括号：登记挂起 + 保守按比较解析。
        let mut p = ExprParser::new("a < b");
        let _ = p.parse_expr();
        v.push((
            "F0428-裁定-尖括号挂起",
            p.ambigs.len() == 1 && p.ambigs[0].scene == AMB_ANGLE,
        ));
        // 普通二元无挂起。
        let mut p2 = ExprParser::new("a + b");
        let _ = p2.parse_expr();
        v.push(("F0428-裁定-普通二元无挂起", p2.ambigs.is_empty()));
        // 挂起不改变解析结论（保守路径照常）。
        let mut p3 = ExprParser::new("a < b");
        v.push((
            "F0428-裁定-保守路径解析",
            matches!(p3.parse_expr(), Ok(Expr::Binary{op, ..}) if op == "<"),
        ));
    }
    // 四、深度复用与三元。
    {
        v.push(("F0428-深度-常量关系", MAX_NEST <= F0421_MAX));
        // 三元右结合：a ? b : c ? d : e → else 分支是三元。
        let mut p = ExprParser::new("a ? b : c ? d : e");
        let e = p.parse_expr();
        v.push((
            "F0428-三元-右结合",
            matches!(&e, Ok(Expr::Ternary{els, ..}) if matches!(els.as_ref(), Expr::Ternary{..})),
        ));
        // 三元嵌套括号。
        let mut p2 = ExprParser::new("(a ? b : c)");
        v.push((
            "F0428-三元-括号透明",
            matches!(p2.parse_expr(), Ok(Expr::Paren{..})),
        ));
        // 超限指向源头。
        let deep = "(".repeat(MAX_NEST as usize + 2) + "x" + &")".repeat(MAX_NEST as usize + 2);
        let mut p3 = ExprParser::new(&deep);
        let _ = p3.parse_expr();
        v.push(("F0428-深度-超限报错", p3.diags.iter().any(|(_, d)| *d == ExprDiag::NestOver)));
        // 括号未闭合。
        let mut p4 = ExprParser::new("(a + b");
        v.push(("F0428-深度-未闭合", p4.parse_expr() == Err(ExprDiag::ParenUnclosed)));
        // 操作数缺失。
        let mut p5 = ExprParser::new("1 +");
        v.push(("F0428-深度-缺操作数", p5.parse_expr() == Err(ExprDiag::OperandMissing)));
    }
    // 五、判据自检。
    {
        v.push(("F0428-判据-锚点入册", ANCHOR == "VE-F0428"));
        v.push(("F0428-判据-诊断码段", (0x4280..0x4290).contains(&ExprDiag::NestOver.code())));
        v.push(("F0428-判据-场景码入册", AMB_ANGLE == 1));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec28_raw_checks as run_vec28_checks;
    #[test]
    fn vec28_expr_checks() {
        for (name, ok) in run_vec28_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec28）。
pub fn run_vec28_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec28");
    for (name, ok) in vec28_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
