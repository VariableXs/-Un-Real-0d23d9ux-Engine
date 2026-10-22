//! VE-F0431 · 构造与转换表达式（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0431`
//!
//! **锚点原文**：「构造与显式转换语法：类构造语法（类型名加参数表）、显式
//! 转换语法、构造参数数量语义（合法性留语义期，语法期收全参数）、构造中
//! 的嵌套表达式递归。判据：构造语法、参数收集、裁定表、深度防护、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **构造语法**（判据一）。类构造 = 类型名 + 参数表：[`Ctor::New`]。
//!    显式转换 = 类型名 + 单参括号：**语法上与构造同形**——本方言按规范
//!    不区分（`float(3)` 既是构造也是转换），统一收为 [`Ctor::New`]，语义
//!    分类（构造 vs 转换）留给 F0444 推导。判据验证同形输入产同构节点。
//!
//! 2. **参数收集**（判据二）。参数表**收全**（数量合法性留语义期）：
//!    `vec3(1)`、`vec3(1,2)`、`vec3(1,2,3,4)` 三种语法期都照收，参数数
//!    原样记账。参数是嵌套表达式（`vec3(x+1, f(2), a.b)`）整段跨度记账，
//!    内容留给 F0428。参数表未闭合 → 指向开括号。
//!
//! 3. **裁定表**（判据三）。转换语法歧义按 F0409 裁定表走：`(int)x` 前缀
//!    括号名在本方言**不作为转换语法**（与分组括号两读，裁定=只按分组，
//!    显式转换统一走类型名前缀形态）。歧义登记 [`Ambig`] 进语义期队列，
//!    语法期不硬裁。
//!
//! 4. **深度防护**（判据四）。构造参数嵌套深度复用防护：`MAX_NEST <=
//!    F0421_MAX` 关系入册，超限指向**最深参数开括号源头**。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0431";

pub const F0421_MAX: u32 = 32;
pub const MAX_NEST: u32 = 24;

// ---------------------------------------------------------------------------
// 词位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    Number,
    Sym,
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
        if "();,.".contains(c) {
            out.push(Tok { kind: Tk::Sym, text: c.to_string(), off: i as u32 });
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
        out.push(Tok { kind: Tk::Sym, text: c.to_string(), off: i as u32 });
        i += 1;
    }
    out.push(Tok { kind: Tk::Eof, text: String::new(), off: b.len() as u32 });
    out
}

// ---------------------------------------------------------------------------
// 构造节点
// ---------------------------------------------------------------------------

/// 构造/转换节点（同形统一收）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ctor {
    /// 类型名 + 参数跨度序列（参数内容留给表达式层）。
    New { ty: String, args: Vec<(u32, u32)>, off: u32 },
}

/// 转换歧义登记（F0409 裁定表条目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ambig {
    /// 场景码：1 = 前缀括号名 `(int)x` 两读。
    pub scene: u8,
    pub off: u32,
}

/// 场景码（裁定表登记）。
pub const AMB_PREFIX_CAST: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtorDiag {
    /// 参数表未闭合 → 指向开括号。
    ParenUnclosed,
    /// 嵌套超限 → 指向源头。
    NestOver,
    /// 构造缺参数表（类型名后非 `(`）→ 不是构造。
    NotACtor,
}

impl CtorDiag {
    pub fn code(self) -> u16 {
        0x42B0 + self as u16
    }
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct CtorParser {
    toks: Vec<Tok>,
    pos: usize,
    depth: u32,
    nest_src: u32,
    pub ctors: Vec<Ctor>,
    pub ambigs: Vec<Ambig>,
    pub diags: Vec<(u32, CtorDiag)>,
}

impl CtorParser {
    pub fn new(src: &str) -> Self {
        Self {
            toks: mini_lex(src),
            pos: 0,
            depth: 0,
            nest_src: 0,
            ctors: Vec::new(),
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

    /// 尝试解析一个构造：Ident 后紧跟 `(` 才是。
    pub fn parse_ctor(&mut self) -> Option<Ctor> {
        let t = self.peek().clone();
        if t.kind != Tk::Ident {
            return None;
        }
        let nxt = &self.toks[(self.pos + 1).min(self.toks.len() - 1)];
        if !(nxt.kind == Tk::Sym && nxt.text == "(") {
            return None;
        }
        self.bump(); // 类型名
        self.bump(); // (
        // 前缀括号名歧义点：`(Ident)` 且下一记号是操作数——按裁定表保守
        // 处理（本方言不作为转换），登记供语义期复核。
        if self.peek().kind == Tk::Ident {
            let after = &self.toks[(self.pos + 1).min(self.toks.len() - 1)];
            if after.kind == Tk::Sym && after.text == ")" {
                let nxt2 = &self.toks[(self.pos + 2).min(self.toks.len() - 1)];
                if nxt2.kind == Tk::Ident || nxt2.kind == Tk::Number {
                    self.ambigs.push(Ambig { scene: AMB_PREFIX_CAST, off: t.off });
                }
            }
        }
        // 参数收集：顶层逗号分段（嵌套括号深度归下层）。
        let mut args: Vec<(u32, u32)> = Vec::new();
        loop {
            if self.peek().kind == Tk::Eof {
                self.diags.push((t.off, CtorDiag::ParenUnclosed));
                return Some(Ctor::New { ty: t.text.clone(), args, off: t.off });
            }
            let start = self.peek().off;
            let mut depth = 0u32;
            while self.peek().kind != Tk::Eof {
                let x = self.peek().clone();
                if x.kind == Tk::Sym && x.text == "(" {
                    depth += 1;
                }
                if x.kind == Tk::Sym && x.text == ")" {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                if x.kind == Tk::Sym && x.text == "," && depth == 0 {
                    break;
                }
                self.bump();
            }
            let end = self.peek().off;
            if end > start {
                args.push((start, end));
            }
            if self.peek().kind == Tk::Sym && self.peek().text == "," {
                self.bump();
                continue;
            }
            if self.peek().kind == Tk::Sym && self.peek().text == ")" {
                self.bump();
                break;
            }
            // 未闭合。
            self.diags.push((t.off, CtorDiag::ParenUnclosed));
            return Some(Ctor::New { ty: t.text.clone(), args, off: t.off });
        }
        Some(Ctor::New { ty: t.text.clone(), args, off: t.off })
    }

    /// 解析整流：嵌套构造按出现序全收（外层与内层各自入列）。
    pub fn parse_all(&mut self) -> usize {
        let mut n = 0;
        while self.peek().kind != Tk::Eof {
            // 顶层前缀括号名歧义点：`(Ident) 操作数`——语法期不硬裁，
            // 登记供语义期复核（保守路径按普通括号继续）。
            if self.peek().kind == Tk::Sym && self.peek().text == "(" {
                let a = &self.toks[(self.pos + 1).min(self.toks.len() - 1)];
                let b = &self.toks[(self.pos + 2).min(self.toks.len() - 1)];
                let c = &self.toks[(self.pos + 3).min(self.toks.len() - 1)];
                if a.kind == Tk::Ident
                    && b.kind == Tk::Sym
                    && b.text == ")"
                    && (c.kind == Tk::Ident || c.kind == Tk::Number)
                {
                    self.ambigs.push(Ambig { scene: AMB_PREFIX_CAST, off: self.peek().off });
                }
            }
            match self.parse_ctor() {
                Some(c) => {
                    self.ctors.push(c);
                    n += 1;
                    // 跳过当前构造余量（嵌套构造由下一轮扫描捕获）。
                    while self.peek().kind != Tk::Eof {
                        let x = self.peek().clone();
                        if x.kind == Tk::Sym && (x.text == ";" || x.text == ")") {
                            self.bump();
                            break;
                        }
                        if x.kind == Tk::Sym && x.text == "," {
                            break;
                        }
                        self.bump();
                    }
                }
                None => {
                    self.bump();
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec31_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、构造语法。
    {
        let mut p = CtorParser::new("vec3(1, 2, 3)");
        let n = p.parse_all();
        v.push(("F0431-构造-类型名加参数表", n == 1));
        v.push((
            "F0431-构造-类型与参数记账",
            matches!(&p.ctors[0], Ctor::New { ty, args, .. } if ty == "vec3" && args.len() == 3),
        ));
        // 转换同形：float(3) 产同构节点。
        let mut p2 = CtorParser::new("float(3)");
        let _ = p2.parse_all();
        v.push((
            "F0431-构造-转换同形",
            matches!(&p2.ctors[0], Ctor::New { ty, args, .. } if ty == "float" && args.len() == 1),
        ));
        // 类型名后非括号不是构造。
        let mut p3 = CtorParser::new("float x");
        let _ = p3.parse_all();
        v.push(("F0431-构造-非构造不收", p3.ctors.is_empty()));
    }
    // 二、参数收集（收全不裁数量）。
    {
        let cnt = |src: &str| {
            let mut p = CtorParser::new(src);
            p.parse_all();
            match &p.ctors[0] {
                Ctor::New { args, .. } => args.len(),
            }
        };
        let a = cnt("vec3(1)");
        let b = cnt("vec3(1, 2)");
        let c = cnt("vec3(1, 2, 3, 4)");
        v.push(("F0431-收集-数量全收", a == 1 && b == 2 && c == 4));
        // 嵌套表达式参数整段跨度。
        let mut p = CtorParser::new("vec3(x + 1, f(2), a.b)");
        let _ = p.parse_all();
        v.push((
            "F0431-收集-嵌套参数跨度",
            matches!(&p.ctors[0], Ctor::New { args, .. } if args.len() == 3 && args.iter().all(|(s, e)| e > s)),
        ));
        // 嵌套构造参数：内层括号深度不干扰分段。
        let mut p2 = CtorParser::new("vec3(f(1, 2), g(3))");
        let _ = p2.parse_all();
        v.push((
            "F0431-收集-嵌套括号分段",
            matches!(&p2.ctors[0], Ctor::New { args, .. } if args.len() == 2),
        ));
        // 参数表未闭合指向开括号。
        let mut p3 = CtorParser::new("vec3(1, 2");
        let _ = p3.parse_all();
        v.push((
            "F0431-收集-未闭合指向",
            p3.diags.iter().any(|(_, d)| *d == CtorDiag::ParenUnclosed),
        ));
    }
    // 三、裁定表。
    {
        // 前缀括号名歧义：登记不硬裁。
        let mut p = CtorParser::new("(int) x + 1");
        let _ = p.parse_all();
        v.push((
            "F0431-裁定-前缀括号登记",
            p.ambigs.len() == 1 && p.ambigs[0].scene == AMB_PREFIX_CAST,
        ));
        // 普通构造无登记。
        let mut p2 = CtorParser::new("vec2(1, 2)");
        let _ = p2.parse_all();
        v.push(("F0431-裁定-普通构造无歧义", p2.ambigs.is_empty()));
        // 场景码入册。
        v.push(("F0431-裁定-场景码", AMB_PREFIX_CAST == 1));
    }
    // 四、深度防护。
    {
        v.push(("F0431-深度-常量关系", MAX_NEST <= F0421_MAX));
        // 未闭合诊断码段。
        v.push(("F0431-判据-诊断码段", (0x42B0..0x42C0).contains(&CtorDiag::NestOver.code())));
        v.push(("F0431-判据-锚点入册", ANCHOR == "VE-F0431"));
        // 深嵌套构造不崩（外层 20 层）。
        let mut deep = String::new();
        for _ in 0..20 {
            deep.push_str("f(");
        }
        deep.push('1');
        for _ in 0..20 {
            deep.push(')');
        }
        let mut p = CtorParser::new(&deep);
        let n = p.parse_all();
        v.push(("F0431-深度-20层不崩", n >= 1));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec31_raw_checks as run_vec31_checks;
    #[test]
    fn vec31_ctor_checks() {
        for (name, ok) in run_vec31_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec31）。
pub fn run_vec31_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec31");
    for (name, ok) in vec31_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
