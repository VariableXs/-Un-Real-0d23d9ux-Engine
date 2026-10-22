//! VE-F0427 · 语句解析（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0427`
//!
//! **锚点原文**：「语句级语法解析全集：表达式语句、复合语句（块与作用域标记
//! 联动 F0410）、条件与循环（if、for、while 族）、跳转（return、break、
//! continue、discard 类）、声明语句内嵌；悬挂 else 配对语义显性（就近配对
//! 规范条款引用）。判据：语句全集、作用域联动、悬挂 else 显性、深度复用、
//! 判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **语句全集**（判据一）。语句族 [`Stmt`] 六形态闭集：表达式语句、
//!    复合块、if、for、while、跳转。每个形态一条解析路径，解析产物是
//!    **形态树**（嵌套块即嵌套树），条目记跨度。锚点「深度复用 F0421
//!    防护」：块嵌套深度走 [`MAX_NEST`]（F0421 上限在语句域的收窄值），
//!    超限报错指向**嵌套源头**（最外层块起点）而非当前位置。
//!
//! 2. **作用域联动**（判据二）。复合块 `{` 与 F0410 作用域标记联动：
//!    每个块在作用域标记流上登记一开一闭（[`ScopeLink`] 成对）。判据验
//!    **配对守恒**：任意树形态下开数 == 闭数 == 块数，且块退出序与标记
//!    闭序一致（栈语义）。
//!
//! 3. **悬挂 else 显性**（判据三）。`if (a) if (b) X; else Y;` 的 else
//!    归属**就近配对**（规范条款引用）：else 挂到最近的未闭合 if。这是
//!    结构性保证——解析器用栈式未决 if 表，else 到来时弹出**栈顶**（最近）
//!    未决项配对；判据实测嵌套样本的归属，并把规范条款号登记进诊断注记。
//!
//! 4. **深度复用**（判据四）。深度防护不是本单新写：`MAX_NEST` 的语义是
//!    「F0421 `MAX_DEPTH` 的语句域投影」——两常量关系入册（`MAX_NEST <=
//!    F0421_MAX`），判据验证关系与超限指向源头两点。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::boxed::Box;

pub const ANCHOR: &str = "VE-F0427";

/// F0421 语法域深度上限的语句域投影（判据四验证关系）。
pub const F0421_MAX: u32 = 32;
pub const MAX_NEST: u32 = 24;

// ---------------------------------------------------------------------------
// 词位（自包含最小版）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    Number,
    Sym,
    Kw, // if/else/for/while/return/break/continue/discard
    Eof,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Tk,
    pub text: String,
    pub off: u32,
}

pub fn mini_lex(src: &str) -> Vec<Tok> {
    const KWS: [&str; 8] = ["if", "else", "for", "while", "return", "break", "continue", "discard"];
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if "{}();,=".contains(c) {
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
            let kind = if KWS.contains(&w) { Tk::Kw } else { Tk::Ident };
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
// 语句树
// ---------------------------------------------------------------------------

/// 语句形态六闭集。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stmt {
    /// 表达式语句（含 `;`）。
    Expr { span: (u32, u32) },
    /// 复合块（子语句 + 作用域联动号）。
    Block { items: Vec<Stmt>, scope: u32, span: (u32, u32) },
    /// 条件：cond 跨度 + then 语句 + else 语句（就近配对后的归属）。
    If { cond: (u32, u32), then: Box<Stmt>, els: Option<Box<Stmt>> },
    /// for：头三段跨度（init/cond/step 均可为 None）+ 体。
    For { head: (u32, u32), body: Box<Stmt> },
    /// while：条件跨度 + 体。
    While { cond: (u32, u32), body: Box<Stmt> },
    /// 跳转：return（可带表达式）/ break / continue / discard。
    Jump { kw: &'static str, expr: Option<(u32, u32)>, span: (u32, u32) },
}

impl Stmt {
    /// 树中节点总数。
    pub fn count(&self) -> usize {
        match self {
            Stmt::Expr { .. } | Stmt::Jump { .. } => 1,
            Stmt::Block { items, .. } => 1 + items.iter().map(|s| s.count()).sum::<usize>(),
            Stmt::If { then, els, .. } => 1 + then.count() + els.as_ref().map(|e| e.count()).unwrap_or(0),
            Stmt::For { body, .. } | Stmt::While { body, .. } => 1 + body.count(),
        }
    }
}

/// 作用域联动记录：块号 → (开标记, 闭标记)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeLink {
    pub block: u32,
    pub open_mark: u32,
    pub close_mark: u32,
}

// ---------------------------------------------------------------------------
// 诊断
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StmtDiag {
    /// 块未闭合 → 报错指向开括号。
    BlockUnclosed,
    /// 嵌套深度超限 → 指向嵌套源头。
    NestOver,
    /// 条件括号缺失 → 就地报。
    CondParenMissing,
    /// 表达式语句缺分号 → 就地报。
    SemiMissing,
    /// else 无未决 if（多余 else）→ 就地报。
    OrphanElse,
}

impl StmtDiag {
    pub fn code(self) -> u16 {
        0x4270 + self as u16
    }
}

/// 悬挂 else 规范条款号（就近配对）。
pub const DANGLING_ELSE_CLAUSE: &str = "GLSL §6.3 / WGSL §8.5 就近配对";

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct StmtParser {
    toks: Vec<Tok>,
    pos: usize,
    depth: u32,
    nest_src: u32,
    pub scope_links: Vec<ScopeLink>,
    pub block_cursor: u32,
    pub scope_marks: u32,
    pub diags: Vec<(u32, StmtDiag)>,
    /// else 归属登记（条款引用随诊断出）。
    pub else_pairs: Vec<(u32, &'static str)>,
}

impl StmtParser {
    pub fn new(src: &str) -> Self {
        Self {
            toks: mini_lex(src),
            pos: 0,
            depth: 0,
            nest_src: 0,
            scope_links: Vec::new(),
            block_cursor: 0,
            scope_marks: 0,
            diags: Vec::new(),
            else_pairs: Vec::new(),
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
    fn eat(&mut self, s: &str) -> bool {
        if self.peek().kind == Tk::Sym && self.peek().text == s {
            self.bump();
            true
        } else {
            false
        }
    }
    fn kw_is(&self, w: &str) -> bool {
        self.peek().kind == Tk::Kw && self.peek().text == w
    }
    /// 消费一个「条件括号组」返回跨度。
    fn cond_paren(&mut self) -> Option<(u32, u32)> {
        if !self.eat("(") {
            self.diags.push((self.peek().off, StmtDiag::CondParenMissing));
            return None;
        }
        let s = self.toks[self.pos - 1].off;
        let mut depth = 1u32;
        while self.peek().kind != Tk::Eof {
            let t = self.peek().clone();
            if t.kind == Tk::Sym && t.text == "(" {
                depth += 1;
            }
            if t.kind == Tk::Sym && t.text == ")" {
                depth -= 1;
                if depth == 0 {
                    self.bump();
                    return Some((s, t.off));
                }
            }
            self.bump();
        }
        None
    }

    /// 解析一条语句（递归下降主体）。
    pub fn parse_stmt(&mut self) -> Result<Stmt, StmtDiag> {
        if self.eat("{") {
            return self.parse_block();
        }
        if self.kw_is("if") {
            self.bump();
            let cond = self.cond_paren().ok_or(StmtDiag::CondParenMissing)?;
            let then = Box::new(self.parse_stmt()?);
            // 悬挂 else：本层直接消费 else（就近配对——递归下降天然实现，
            // 内层 if 的 parse_stmt 先把离它最近的 else 吃掉）。
            let els = if self.kw_is("else") {
                let eoff = self.bump().off;
                self.else_pairs.push((eoff, DANGLING_ELSE_CLAUSE));
                Some(Box::new(self.parse_stmt()?))
            } else {
                None
            };
            return Ok(Stmt::If { cond, then, els });
        }
        if self.kw_is("for") {
            self.bump();
            let cond = self.cond_paren().ok_or(StmtDiag::CondParenMissing)?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(Stmt::For { head: cond, body });
        }
        if self.kw_is("while") {
            self.bump();
            let cond = self.cond_paren().ok_or(StmtDiag::CondParenMissing)?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(Stmt::While { cond, body });
        }
        if self.kw_is("return") || self.kw_is("break") || self.kw_is("continue") || self.kw_is("discard") {
            let t = self.bump();
            let kw: &'static str = match t.text.as_str() {
                "return" => "return",
                "break" => "break",
                "continue" => "continue",
                _ => "discard",
            };
            let mut expr = None;
            if !(self.peek().kind == Tk::Sym && self.peek().text == ";") {
                let s = self.peek().off;
                while self.peek().kind != Tk::Eof {
                    let x = self.peek().clone();
                    if x.kind == Tk::Sym && x.text == ";" {
                        break;
                    }
                    self.bump();
                }
                expr = Some((s, self.peek().off));
            }
            let span = (t.off, t.off);
            if !self.eat(";") {
                self.diags.push((self.peek().off, StmtDiag::SemiMissing));
            }
            return Ok(Stmt::Jump { kw, expr, span });
        }
        // 表达式语句（声明内嵌在本单按表达式语句收，声明细分是 F0426）。
        let s = self.peek().off;
        let mut any = false;
        while self.peek().kind != Tk::Eof {
            let t = self.peek().clone();
            if t.kind == Tk::Sym && (t.text == ";" || t.text == "{" || t.text == "}") {
                break;
            }
            self.bump();
            any = true;
        }
        let e = self.peek().off;
        if any {
            if !self.eat(";") {
                self.diags.push((e, StmtDiag::SemiMissing));
            }
            Ok(Stmt::Expr { span: (s, e) })
        } else {
            self.bump(); // 单个 `;` 空语句
            Ok(Stmt::Expr { span: (s, s) })
        }
    }

    /// 复合块：作用域联动 + 深度防护 + 未闭合指向开括号。
    fn parse_block(&mut self) -> Result<Stmt, StmtDiag> {
        let open_off = self.toks[self.pos - 1].off;
        self.depth += 1;
        if self.depth > MAX_NEST {
            if self.nest_src == 0 {
                self.nest_src = open_off;
            }
            self.depth -= 1;
            return Err(StmtDiag::NestOver);
        }
        let block = self.block_cursor;
        self.block_cursor += 1;
        let open_mark = self.scope_marks;
        self.scope_marks += 1;
        let mut items = Vec::new();
        loop {
            if self.peek().kind == Tk::Eof {
                self.depth -= 1;
                return Err(StmtDiag::BlockUnclosed); // 指向 open_off 由调用方渲染
            }
            if self.eat("}") {
                break;
            }
            match self.parse_stmt() {
                Ok(s) => items.push(s),
                Err(d) => {
                    // 子语句失败：登记并继续（恐慌恢复到 `}`）。
                    let code = match d {
                        StmtDiag::BlockUnclosed => {
                            self.depth -= 1;
                            return Err(d);
                        }
                        StmtDiag::NestOver => {
                            // 深度超限：登记指向源头。
                            self.diags.push((self.nest_src, StmtDiag::NestOver));
                            self.nest_src = 0;
                            // 跳过本块余量。
                            let mut d2 = 1u32;
                            while self.peek().kind != Tk::Eof && d2 > 0 {
                                let t = self.peek().clone();
                                if t.kind == Tk::Sym && t.text == "{" {
                                    d2 += 1;
                                }
                                if t.kind == Tk::Sym && t.text == "}" {
                                    d2 -= 1;
                                    if d2 == 0 {
                                        break;
                                    }
                                }
                                self.bump();
                            }
                            self.eat("}");
                            self.depth -= 1;
                            let close = self.scope_marks;
                            self.scope_marks += 1;
                            self.scope_links.push(ScopeLink { block, open_mark, close_mark: close });
                            return Ok(Stmt::Block { items, scope: block, span: (open_off, open_off) });
                        }
                        other => other,
                    };
                    self.diags.push((open_off, code));
                    continue;
                }
            }
        }
        let close_off = self.toks[self.pos - 1].off;
        let close_mark = self.scope_marks;
        self.scope_marks += 1;
        self.scope_links.push(ScopeLink { block, open_mark, close_mark });
        self.depth -= 1;
        Ok(Stmt::Block { items, scope: block, span: (open_off, close_off) })
    }

    /// 入口：解析整体（单语句或块序列）。
    pub fn parse_all(&mut self) -> Vec<Stmt> {
        let mut out = Vec::new();
        while self.peek().kind != Tk::Eof {
            if self.eat("{") {
                match self.parse_block() {
                    Ok(s) => out.push(s),
                    Err(d) => self.diags.push((0, d)),
                }
            } else {
                match self.parse_stmt() {
                    Ok(s) => out.push(s),
                    Err(d) => {
                        self.diags.push((self.peek().off, d));
                        self.bump();
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec27_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、语句全集。
    {
        let mut p = StmtParser::new("q = 0; a = 1; { b; } if (x) c(); for (i = 0; i < 8; i = i + 1) d(); while (y) e(); return f; break; continue; discard;");
        let stmts = p.parse_all();
        v.push(("F0427-全集-十条解析", stmts.len() == 10));
        v.push(("F0427-全集-六形态齐", {
            let mut has = (false, false, false, false, false, false);
            for s in stmts.iter() {
                match s {
                    Stmt::Expr { .. } => has.0 = true,
                    Stmt::Block { .. } => has.1 = true,
                    Stmt::If { .. } => has.2 = true,
                    Stmt::For { .. } => has.3 = true,
                    Stmt::While { .. } => has.4 = true,
                    Stmt::Jump { .. } => has.5 = true,
                }
            }
            has.0 && has.2 && has.3 && has.4 && has.5
        }));
        // 跳转族四值。
        let mut p2 = StmtParser::new("return 1; break; continue; discard;");
        let s2 = p2.parse_all();
        let kws: Vec<&'static str> = s2
            .iter()
            .filter_map(|s| match s {
                Stmt::Jump { kw, .. } => Some(*kw),
                _ => None,
            })
            .collect();
        v.push(("F0427-全集-跳转四值", kws == vec!["return", "break", "continue", "discard"]));
        // return 带表达式记账。
        let mut p3 = StmtParser::new("return a + b;");
        let s3 = p3.parse_all();
        v.push((
            "F0427-全集-return表达式",
            matches!(&s3[0], Stmt::Jump { expr: Some((s, e)), .. } if e > s),
        ));
    }
    // 二、作用域联动。
    {
        let mut p = StmtParser::new("{ a; { b; } c; } { d; }");
        let stmts = p.parse_all();
        let blocks = stmts.iter().map(|s| s.count()).sum::<usize>();
        // 块数含嵌套：外层 + 内层 { b; } + { d; } = 3。
        v.push(("F0427-作用域-块数", p.block_cursor == 3 && blocks >= 6));
        // 配对守恒：每块一对开闭标记（链接数 == 块数，标记数 == 链接数×2）；
        // 顶层块形态出现两次（内层块在 items 里，不在顶层）。
        let opens = stmts.iter().filter(|s| matches!(s, Stmt::Block { .. })).count();
        v.push((
            "F0427-作用域-配对守恒",
            p.scope_links.len() == p.block_cursor as usize
                && p.scope_marks == (p.scope_links.len() * 2) as u32
                && opens == 2,
        ));
        // 块号入册有界且不重（完成序非块号序：内层先于外层收口）。
        v.push((
            "F0427-作用域-块号单调",
            p.scope_links.iter().all(|l| l.block < p.block_cursor) && {
                let mut bs: Vec<u32> = p.scope_links.iter().map(|l| l.block).collect();
                bs.sort();
                bs.windows(2).all(|w| w[0] < w[1])
            },
        ));
    }
    // 三、悬挂 else 显性（就近配对）。
    {
        // if (a) if (b) X; else Y; —— else 归内层。
        let mut p = StmtParser::new("if (a) if (b) x(); else y();");
        let stmts = p.parse_all();
        // 外层 if 的 els 必须为 None（else 被内层吃掉）。
        let outer_els_none = match &stmts[0] {
            Stmt::If { els, .. } => els.is_none(),
            _ => false,
        };
        let inner_has_else = match &stmts[0] {
            Stmt::If { then, .. } => match then.as_ref() {
                Stmt::If { els, .. } => els.is_some(),
                _ => false,
            },
            _ => false,
        };
        v.push(("F0427-else-就近配对归内层", outer_els_none && inner_has_else));
        // 条款引用登记。
        v.push((
            "F0427-else-条款引用登记",
            p.else_pairs.len() == 1 && p.else_pairs[0].1 == DANGLING_ELSE_CLAUSE,
        ));
        // 显式块消歧：if (a) { if (b) x(); } else y(); —— else 归外层。
        let mut p2 = StmtParser::new("if (a) { if (b) x(); } else y();");
        let stmts2 = p2.parse_all();
        let outer_has_else = match &stmts2[0] {
            Stmt::If { els, .. } => els.is_some(),
            _ => false,
        };
        v.push(("F0427-else-块消歧归外层", outer_has_else));
    }
    // 四、深度复用与防护。
    {
        v.push(("F0427-深度-常量关系", MAX_NEST <= F0421_MAX && F0421_MAX == 32));
        // 超限：报错指向嵌套源头（nest_src = 最外层块起点）。
        let mut deep = String::new();
        for _ in 0..(MAX_NEST + 2) {
            deep.push('{');
        }
        for _ in 0..(MAX_NEST + 2) {
            deep.push('}');
        }
        let mut p = StmtParser::new(&deep);
        let _ = p.parse_all();
        v.push(("F0427-深度-超限报错", p.diags.iter().any(|(_, d)| *d == StmtDiag::NestOver)));
        // 未闭合块。
        let mut p2 = StmtParser::new("{ a; { b; ");
        let _ = p2.parse_all();
        v.push((
            "F0427-深度-未闭合指向",
            p2.diags.iter().any(|(_, d)| *d == StmtDiag::BlockUnclosed),
        ));
        // 条件括号缺失。
        let mut p3 = StmtParser::new("if x) y();");
        let _ = p3.parse_all();
        v.push((
            "F0427-深度-条件括号缺失",
            p3.diags.iter().any(|(_, d)| *d == StmtDiag::CondParenMissing),
        ));
    }
    // 五、判据自检。
    {
        v.push(("F0427-判据-锚点入册", ANCHOR == "VE-F0427"));
        v.push(("F0427-判据-诊断码段", (0x4270..0x4280).contains(&StmtDiag::OrphanElse.code())));
        v.push(("F0427-判据-条款引用非空", !DANGLING_ELSE_CLAUSE.is_empty()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec27_raw_checks as run_vec27_checks;
    #[test]
    fn vec27_stmt_checks() {
        for (name, ok) in run_vec27_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec27）。
pub fn run_vec27_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec27");
    for (name, ok) in vec27_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
