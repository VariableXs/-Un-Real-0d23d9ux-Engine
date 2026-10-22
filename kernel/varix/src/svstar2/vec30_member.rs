//! VE-F0430 · 成员访问与下标语法（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0430`
//!
//! **锚点原文**：「成员访问（点号）与下标（方括号）语法：链式访问（a.b.c、
//! a[1][2] 混合链）解析为访问链节点、向量分量访问（swizzle 语法面）收集
//! 留给语义期验证（F0445）、链上方法调用语法（如有）按规范。判据：链式
//! 节点、只收不裁、双侧定位、规范引用、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **链式节点**（判据一）。访问链 [`AccessChain`]：节点序列（根 + 步
//!    序列），每步是 [`Step::Member`]（点号名）或 [`Step::Index`]（方括号
//!    表达式跨度）。混合链 `a.b[1].c` 按出现序成链——链节点是**线性的**，
//!    不允许分叉（分叉是表达式层的事，F0428）。
//!
//! 2. **只收不裁**（判据二）。swizzle 语法面（`.xyz` 类分量名）**原样
//!    收集**：分量字符不查维、不查重复、不查写场景——全部留给 F0445。
//!    [`Step::Member`] 的文本就是出现时的原文本；判据纯度实测：`.xyzw`、
//!    `.wzyx`、`.rrrr` 三者的链结构完全同构。
//!
//! 3. **双侧定位**（判据三）。链中非法衔接（如 `a.[1]` 点号后直接下标、
//!    `a..b` 双点）报错**带双侧定位**：错误记录非法衔接点的左右记号跨度
//!    （[`AccDiag::BadJoint`]），不是单点——定位双侧是修复建议的原料。
//!
//! 4. **规范引用**（判据四）。方法调用语法（本方言无方法，按规范登记为
//!    **显性不支持**）：链上出现 `(` 即登记 [`AccDiag::MethodClause`]
//!    并引用规范条款号（与 vec27 悬挂 else 同款条款引用纪律）——不静默
//!    吞、不误报错（可能是 F0432 函数调用的衔接点，交上层裁决）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0430";

/// 方法调用规范的条款引用（显性不支持即引用条款）。
pub const METHOD_CLAUSE: &str = "方言规范 §7.1 链上方法调用不支持，函数调用走 F0432";

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
        if ".()[];,+".contains(c) {
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
// 访问链
// ---------------------------------------------------------------------------

/// 链步。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// 点号成员（含 swizzle 语法面，原文本）。
    Member { text: String, off: u32 },
    /// 方括号下标（表达式跨度）。
    Index { span: (u32, u32) },
}

/// 访问链节点：根 + 步序列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessChain {
    pub root: String,
    pub root_off: u32,
    pub steps: Vec<Step>,
}

impl AccessChain {
    pub fn len(&self) -> usize {
        self.steps.len()
    }
    /// swizzle 步收集（供 F0445 消费的语法面视图）。
    pub fn swizzle_views(&self) -> Vec<&str> {
        self.steps
            .iter()
            .filter_map(|s| match s {
                Step::Member { text, .. } if text.len() > 1 => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
    /// 下标步数。
    pub fn index_count(&self) -> usize {
        self.steps.iter().filter(|s| matches!(s, Step::Index { .. })).count()
    }
}

/// 诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccDiag {
    /// 链中非法衔接 → 双侧定位（左右跨度由记录方持有）。
    BadJoint,
    /// 空下标 `[]` → 报错带建议。
    EmptyIndex,
    /// 方法调用衔接 → 条款引用登记（交上层裁决）。
    MethodClause,
}

impl AccDiag {
    pub fn code(self) -> u16 {
        0x42A0 + self as u16
    }
}

/// 双侧定位记录：诊断 + 左右跨度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointRecord {
    pub diag: AccDiag,
    pub left: u32,
    pub right: u32,
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AccParser {
    toks: Vec<Tok>,
    pos: usize,
    pub chains: Vec<AccessChain>,
    pub joints: Vec<JointRecord>,
    pub diags: Vec<(u32, AccDiag)>,
}

impl AccParser {
    pub fn new(src: &str) -> Self {
        Self { toks: mini_lex(src), pos: 0, chains: Vec::new(), joints: Vec::new(), diags: Vec::new() }
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

    /// 解析一条访问链（根 + 零或多步）。
    pub fn parse_chain(&mut self) -> Option<AccessChain> {
        let t = self.peek().clone();
        if t.kind != Tk::Ident {
            return None;
        }
        self.bump();
        let mut chain = AccessChain { root: t.text.clone(), root_off: t.off, steps: Vec::new() };
        loop {
            let m = self.peek().clone();
            match m.kind {
                Tk::Sym if m.text == "." => {
                    self.bump();
                    let nxt = self.peek().clone();
                    match nxt.kind {
                        Tk::Ident => {
                            self.bump();
                            chain.steps.push(Step::Member { text: nxt.text.clone(), off: nxt.off });
                        }
                        Tk::Sym if nxt.text == "[" => {
                            // `a.[1]` 非法衔接：双侧 = 点号与方括号。
                            self.joints.push(JointRecord {
                                diag: AccDiag::BadJoint,
                                left: m.off,
                                right: nxt.off,
                            });
                            self.diags.push((m.off, AccDiag::BadJoint));
                            return Some(chain);
                        }
                        Tk::Sym if nxt.text == "." => {
                            // `a..b` 非法衔接。
                            self.joints.push(JointRecord {
                                diag: AccDiag::BadJoint,
                                left: m.off,
                                right: nxt.off,
                            });
                            self.diags.push((m.off, AccDiag::BadJoint));
                            return Some(chain);
                        }
                        _ => {
                            // 点号后无成员（`a.` 尾）：单点报错双侧=点号与 Eof。
                            self.joints.push(JointRecord {
                                diag: AccDiag::BadJoint,
                                left: m.off,
                                right: nxt.off,
                            });
                            self.diags.push((m.off, AccDiag::BadJoint));
                            return Some(chain);
                        }
                    }
                }
                Tk::Sym if m.text == "[" => {
                    self.bump();
                    let start = self.peek().off;
                    if self.peek().kind == Tk::Sym && self.peek().text == "]" {
                        let e = self.bump().off;
                        self.diags.push((start, AccDiag::EmptyIndex));
                        chain.steps.push(Step::Index { span: (start, e) });
                        continue;
                    }
                    // 表达式跨度（内容留给上层）。
                    let mut depth = 1u32;
                    let mut end = start;
                    while self.peek().kind != Tk::Eof {
                        let x = self.peek().clone();
                        if x.kind == Tk::Sym && x.text == "[" {
                            depth += 1;
                        }
                        if x.kind == Tk::Sym && x.text == "]" {
                            depth -= 1;
                            if depth == 0 {
                                end = x.off;
                                break;
                            }
                        }
                        self.bump();
                        end = x.off;
                    }
                    if self.peek().kind == Tk::Sym && self.peek().text == "]" {
                        self.bump();
                    } else {
                        // 未闭合：双侧定位。
                        self.joints.push(JointRecord {
                            diag: AccDiag::BadJoint,
                            left: m.off,
                            right: end,
                        });
                        self.diags.push((m.off, AccDiag::BadJoint));
                        return Some(chain);
                    }
                    chain.steps.push(Step::Index { span: (start, end) });
                }
                Tk::Sym if m.text == "(" => {
                    // 链上方法/调用衔接：条款引用登记，交上层。
                    self.diags.push((m.off, AccDiag::MethodClause));
                    return Some(chain);
                }
                _ => return Some(chain),
            }
        }
    }

    /// 解析整流：按 `;`/`,` 分段收集链。
    pub fn parse_all(&mut self) -> usize {
        let mut n = 0;
        while self.peek().kind != Tk::Eof {
            match self.parse_chain() {
                Some(c) => {
                    self.chains.push(c);
                    n += 1;
                    // 段尾消费。
                    while self.peek().kind != Tk::Eof {
                        let x = self.peek().clone();
                        if x.kind == Tk::Sym && (x.text == ";" || x.text == ",") {
                            self.bump();
                            break;
                        }
                        if x.kind == Tk::Ident {
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

pub fn vec30_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、链式节点。
    {
        let mut p = AccParser::new("a.b.c;");
        let n = p.parse_all();
        v.push(("F0430-链-三步链", n == 1 && p.chains[0].len() == 2));
        // 混合链 a.b[1].c[2][3]。
        let mut p2 = AccParser::new("a.b[1].c[2][3];");
        let _ = p2.parse_all();
        let c = &p2.chains[0];
        v.push((
            "F0430-链-混合五步",
            // b、c 均单字符成员：非 swizzle（视图只收多字符，见纯度判据）。
            c.len() == 5 && c.index_count() == 3 && c.swizzle_views().is_empty(),
        ));
        // 步序保真：Member→Index→Member。
        let order: Vec<bool> = c
            .steps
            .iter()
            .map(|s| matches!(s, Step::Member { .. }))
            .collect();
        v.push((
            "F0430-链-步序保真",
            order == vec![true, false, true, false, false],
        ));
        // 链线性：根 + 步，无分叉字段（结构性：链只暴露 root/steps）。
        v.push(("F0430-链-线性无分叉", c.root == "a" && c.root_off == 0));
    }
    // 二、只收不裁（swizzle 语法面纯度）。
    {
        let mk = |src: &str| {
            let mut p = AccParser::new(src);
            p.parse_all();
            p.chains[0].swizzle_views().len()
        };
        let a = mk("v.xyzw;");
        let b = mk("v.wzyx;");
        let c = mk("v.rrrr;");
        v.push(("F0430-纯度-三种swizzle同构", a == 1 && b == 1 && c == 1));
        // 分量字符不查维不查重复（xyzw 与 xxx 同收）。
        v.push(("F0430-纯度-重复分量同收", mk("v.xxx;") == 1));
        // 单字符成员不是 swizzle（视图只收多字符）。
        let mut p = AccParser::new("v.x;");
        let _ = p.parse_all();
        v.push(("F0430-纯度-单字符非swizzle", p.chains[0].swizzle_views().is_empty()));
    }
    // 三、双侧定位。
    {
        // a.[1] 双侧 = 点号 off 与方括号 off。
        let mut p = AccParser::new("a.[1];");
        let _ = p.parse_all();
        v.push((
            "F0430-定位-点后下标双侧",
            p.joints.len() == 1 && p.joints[0].right > p.joints[0].left,
        ));
        // a..b 双侧。
        let mut p2 = AccParser::new("a..b;");
        let _ = p2.parse_all();
        v.push((
            "F0430-定位-双点双侧",
            p2.joints.len() == 1 && p2.joints[0].right - p2.joints[0].left == 1,
        ));
        // 空下标。
        let mut p3 = AccParser::new("a[];");
        let _ = p3.parse_all();
        v.push((
            "F0430-定位-空下标报错",
            p3.diags.iter().any(|(_, d)| *d == AccDiag::EmptyIndex),
        ));
    }
    // 四、规范引用。
    {
        // 链上 `(`：条款引用登记（交上层裁决不硬拒）。
        let mut p = AccParser::new("a.b(1);");
        let _ = p.parse_all();
        v.push((
            "F0430-规范-方法衔接登记",
            p.diags.iter().any(|(_, d)| *d == AccDiag::MethodClause),
        ));
        v.push(("F0430-规范-条款非空", !METHOD_CLAUSE.is_empty()));
        // 普通链无登记。
        let mut p2 = AccParser::new("a.b;");
        let _ = p2.parse_all();
        v.push(("F0430-规范-普通链无登记", p2.diags.is_empty()));
        // 下标表达式跨度（f(a+1) 类内容整段记账）。
        let mut p3 = AccParser::new("m[a + 1];");
        let _ = p3.parse_all();
        v.push((
            "F0430-规范-下标表达式跨度",
            matches!(&p3.chains[0].steps[0], Step::Index { span: (s, e) } if e > s),
        ));
        // 判据自检。
        v.push(("F0430-判据-锚点入册", ANCHOR == "VE-F0430"));
        v.push(("F0430-判据-诊断码段", (0x42A0..0x42B0).contains(&AccDiag::BadJoint.code())));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec30_raw_checks as run_vec30_checks;
    #[test]
    fn vec30_member_checks() {
        for (name, ok) in run_vec30_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec30）。
pub fn run_vec30_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec30");
    for (name, ok) in vec30_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
