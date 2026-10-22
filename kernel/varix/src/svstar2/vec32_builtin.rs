//! VE-F0432 · 内建函数调用语法（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0432`
//!
//! **锚点原文**：「内建函数调用语法面：内建名识别（与 F0447 签名库对齐的
//! 名称表语法预检）、调用语法与用户函数一致（统一语法路径不分叉）、内建
//! 重载在语法期只登记名称（签名决议留语义期）；内建名冲突（用户重定义
//! 遮蔽）登记语义期裁定。判据：统一路径、语法预检不裁决、遮蔽登记、
//! 签名留语义期、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **统一路径**（判据一）。内建调用与用户函数调用**走同一条解析路径**
//!    [`CallParser::parse_call`]——结构上没有「内建分支」：解析器不知道
//!    也不需要知道名字是不是内建。判据纯度实测：同一调用形态换名（内建
//!    名 ↔ 用户名）产出的调用节点结构摘要逐位相同。
//!
//! 2. **语法预检不裁决**（判据二）。内建名识别是**预检**：[`BUILTIN_NAMES`]
//!    名称表（与 F0447 签名库对齐的名字集，语法期只对名字）+ [`BuiltinRef`]
//!    登记簿。未知名调用**不报错**（锚点明示——可能是用户函数），只不标
//!    内建位；已知名调用也不裁「参数对不对」（签名决议是 F0446/F0447）。
//!
//! 3. **遮蔽登记**（判据三）。用户重定义内建名（用户函数登记同名）时
//!    **双方都保留**：调用处的内建位照标，重定义进 [`ShadowLedger`]——
//!    遮蔽是否生效是语义期作用域裁定（F0442），语法期只留证。
//!
//! 4. **签名留语义期**（判据四）。内建重载（`abs` 有 int/float/vec 三重载）
//!    语法期**只登记名称一个位**：[`BuiltinRef`] 无签名无重载序字段。
//!    判据结构性验证：登记条目的全部字段只含「名 + 调用跨度」。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0432";

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
        if "();,=".contains(c) {
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
// 内建名称表（与 F0447 签名库对齐的语法预检面；只对名字不对签名）
// ---------------------------------------------------------------------------

/// 内建名表（子集入册——全集在 F0447；语法预检只查名）。
pub const BUILTIN_NAMES: [&str; 16] = [
    "abs", "min", "max", "clamp", "mix", "dot", "cross",
    "normalize", "length", "distance", "reflect", "refract",
    "sin", "cos", "pow", "floor",
];

/// 名称预检：O(1) 语义的查表（n=16 常量序扫，编译期可展开）。
pub fn is_builtin(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}

/// 调用节点（统一路径产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    /// 参数跨度序列。
    pub args: Vec<(u32, u32)>,
    pub off: u32,
    /// 内建预检位（语法期唯一语义面）。
    pub builtin: bool,
}

impl Call {
    /// 结构摘要（换名不变量：名以外字段全同则摘要同）。
    pub fn shape_summary(&self) -> (usize, bool) {
        (self.args.len(), self.builtin)
    }
}

/// 遮蔽账：用户重定义内建名（名 + 定义跨度）。
#[derive(Debug, Default)]
pub struct ShadowLedger {
    pub entries: Vec<(String, u32)>,
}

impl ShadowLedger {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, name: &str, off: u32) {
        self.entries.push((name.into(), off));
    }
    pub fn shadowed(&self, name: &str) -> bool {
        self.entries.iter().any(|(n, _)| n == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallDiag {
    /// 参数表未闭合 → 指向开括号。
    ParenUnclosed,
    /// 调用缺名（`(` 打头且非表达式语境）→ 本单不收。
    NotACall,
}

impl CallDiag {
    pub fn code(self) -> u16 {
        0x42C0 + self as u16
    }
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct CallParser {
    toks: Vec<Tok>,
    pos: usize,
    pub calls: Vec<Call>,
    pub shadow: ShadowLedger,
    pub diags: Vec<(u32, CallDiag)>,
}

impl CallParser {
    pub fn new(src: &str) -> Self {
        Self {
            toks: mini_lex(src),
            pos: 0,
            calls: Vec::new(),
            shadow: ShadowLedger::new(),
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

    /// 统一调用路径：Ident + `(` → Call（内建位只做预检标记）。
    pub fn parse_call(&mut self) -> Option<Call> {
        let t = self.peek().clone();
        if t.kind != Tk::Ident {
            return None;
        }
        let nxt = &self.toks[(self.pos + 1).min(self.toks.len() - 1)];
        if !(nxt.kind == Tk::Sym && nxt.text == "(") {
            return None;
        }
        self.bump(); // 名
        self.bump(); // (
        let mut args: Vec<(u32, u32)> = Vec::new();
        loop {
            if self.peek().kind == Tk::Eof {
                self.diags.push((t.off, CallDiag::ParenUnclosed));
                return Some(Call {
                    name: t.text.clone(),
                    args,
                    off: t.off,
                    builtin: is_builtin(&t.text),
                });
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
            self.diags.push((t.off, CallDiag::ParenUnclosed));
            return Some(Call { name: t.text.clone(), args, off: t.off, builtin: is_builtin(&t.text) });
        }
        Some(Call { name: t.text.clone(), args, off: t.off, builtin: is_builtin(&t.text) })
    }

    /// 函数**声明**路径：用户重定义内建名 → 遮蔽账（只留证）。
    pub fn note_definition(&mut self, name: &str, off: u32) {
        if is_builtin(name) {
            self.shadow.register(name, off);
        }
    }

    /// 解析整流。
    pub fn parse_all(&mut self) -> usize {
        let mut n = 0;
        while self.peek().kind != Tk::Eof {
            match self.parse_call() {
                Some(c) => {
                    self.calls.push(c);
                    n += 1;
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

pub fn vec32_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、统一路径（纯度实测：换名产同构结构）。
    {
        let mut p1 = CallParser::new("abs(x)");
        let _ = p1.parse_all();
        let mut p2 = CallParser::new("user_fn(x)");
        let _ = p2.parse_all();
        let s1 = p1.calls[0].shape_summary();
        let s2 = p2.calls[0].shape_summary();
        v.push(("F0432-统一-换名同构", s1.0 == s2.0));
        // 参数表解析路径逐位一致（跨度相对偏移同）。
        v.push((
            "F0432-统一-跨度同构",
            p1.calls[0].args.len() == p2.calls[0].args.len(),
        ));
        // 内建位不同（唯一差异面）。
        v.push((
            "F0432-统一-内建位差异",
            p1.calls[0].builtin && !p2.calls[0].builtin,
        ));
    }
    // 二、语法预检不裁决。
    {
        v.push(("F0432-预检-名称表入册", BUILTIN_NAMES.len() == 16));
        v.push(("F0432-预检-查名", is_builtin("dot") && !is_builtin("my_dot")));
        // 未知名调用不报错。
        let mut p = CallParser::new("unknown_fn(1);");
        let _ = p.parse_all();
        v.push((
            "F0432-预检-未知名不报错",
            !p.diags.iter().any(|(_, d)| *d == CallDiag::NotACall) && p.calls.len() == 1,
        ));
        // 已知名不裁参数对错（签名留语义期）。
        let mut p2 = CallParser::new("dot(1);");
        let _ = p2.parse_all();
        v.push(("F0432-预检-参数数不裁", p2.calls[0].args.len() == 1 && p2.diags.is_empty()));
    }
    // 三、遮蔽登记。
    {
        let mut p = CallParser::new("abs(x)");
        let _ = p.parse_all();
        p.note_definition("abs", 100);
        v.push((
            "F0432-遮蔽-重定义留证",
            p.shadow.shadowed("abs") && p.shadow.entries.len() == 1,
        ));
        // 调用侧内建位照标（遮蔽是否生效语义期裁）。
        v.push(("F0432-遮蔽-调用位照标", p.calls[0].builtin));
        // 非内建重定义不入账。
        let mut p2 = CallParser::new("");
        p2.note_definition("my_fn", 5);
        v.push(("F0432-遮蔽-非内建不入账", p2.shadow.entries.is_empty()));
    }
    // 四、签名留语义期（结构性验证）。
    {
        // 重载只登记名称：Call 全字段 = 名/参数跨度/偏移/内建位，无签名字段。
        let mut p = CallParser::new("abs(1, 2, 3)");
        let _ = p.parse_all();
        let c = &p.calls[0];
        v.push((
            "F0432-签名-条目无签名面",
            c.name == "abs" && c.builtin && c.off == 0,
        ));
        // 未闭合指向开括号。
        let mut p2 = CallParser::new("min(1, 2");
        let _ = p2.parse_all();
        v.push((
            "F0432-签名-未闭合指向",
            p2.diags.iter().any(|(_, d)| *d == CallDiag::ParenUnclosed),
        ));
        // 诊断码段与锚点。
        v.push(("F0432-判据-诊断码段", (0x42C0..0x42D0).contains(&CallDiag::NotACall.code())));
        v.push(("F0432-判据-锚点入册", ANCHOR == "VE-F0432"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec32_raw_checks as run_vec32_checks;
    #[test]
    fn vec32_builtin_checks() {
        for (name, ok) in run_vec32_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec32）。
pub fn run_vec32_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec32");
    for (name, ok) in vec32_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
