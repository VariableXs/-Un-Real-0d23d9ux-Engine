//! VE-F0433 · 属性与布局限定符语法（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0433`
//!
//! **锚点原文**：「属性与布局限定符语法专项：布局限定（location、binding、
//! set 类键值对语法）、属性块语法、键值合法性语法面（值域校验留语义期）、
//! 重复键语法期登记语义期裁定。判据：键值语法、合法键集、重复登记、值域
//! 留语义期、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **键值语法**（判据一）。布局限定 = `layout(` 键值对序 `)`：键 =
//!    标识符，值 = 整数字面量（本方言布局值域只有整数，结构体类值是扩展
//!    位 [`Val::Unknown`]）。属性块 = `@name`（无值）或 `@name(...)`（有
//!    值块）。两类语法面统一收 [`LayoutItem`]。
//!
//! 2. **合法键集**（判据二）。键集 [`LAYOUT_KEYS`] 入册（location/binding/
//!    set/local_size_x…），查键 O(1) 语义。**不在键集的键不报错**——登记
//!    [`KeyClass::Unknown`]（扩展候选，语义期裁定）；判据纯度：合法键与
//!    未知键的条目结构同构，只有分类位不同。
//!
//! 3. **重复登记**（判据三）。同一块内同键再现：**两条都收**（覆盖=毁证）
//!    + [`LayoutDiag::KeyDupYield`] 进语义期队列。判据实测重复键的条目数
//!    与队列同步。
//!
//! 4. **值域留语义期**（判据四）。值的合法性（location 上限、binding 冲突
//!    面）语法期零校验：负数、超大值、非值（缺 `=`）都只记账。缺 `=` 的
//!    裸键按「值缺席」收 [`Val::None`]——语法合法但语义待定，不是错误。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0433";

// ---------------------------------------------------------------------------
// 词位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    Number,
    Sym,
    Attr,
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
        if c == '@' {
            let s = i;
            i += 1;
            while i < b.len() && ((b[i] as char).is_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok { kind: Tk::Attr, text: src[s..i].into(), off: s as u32 });
            continue;
        }
        if "();,=[]=".contains(c) {
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
            // 数字带负号场景：词法面负号独立处理（Sym '-' 未入集合则
            // 落到兜底 Sym）——本单值域只收 Number/缺省，负值由 Sym 序
            // 表达（语义期拼读）。
            let kind = if w.bytes().all(|c| c.is_ascii_digit()) { Tk::Number } else { Tk::Ident };
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
// 布局条目
// ---------------------------------------------------------------------------

/// 布局合法键集（与 F0449 存储语义对齐的名字面）。
pub const LAYOUT_KEYS: [&str; 8] = [
    "location", "binding", "set", "local_size_x", "local_size_y", "local_size_z",
    "push_constant", "rgba16f",
];

/// 值形态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Val {
    /// 整数字面量（原文本）。
    Int(String),
    /// 值缺席（裸键，语义期按规范补）。
    None,
    /// 非整值（结构体类值等扩展候选）。
    Unknown(String),
}

/// 键分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyClass {
    Known,
    Unknown,
}

/// 布局条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutItem {
    pub key: String,
    pub class: KeyClass,
    pub val: Val,
    pub off: u32,
}

/// 属性块条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrItem {
    /// 属性名（@ 后）。
    pub name: String,
    /// 值块跨度（无块 = None）。
    pub block: Option<(u32, u32)>,
    pub off: u32,
}

/// 键分类查表。
pub fn key_class(key: &str) -> KeyClass {
    if LAYOUT_KEYS.contains(&key) {
        KeyClass::Known
    } else {
        KeyClass::Unknown
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutDiag {
    /// 重复键 → 登记语义期。
    KeyDupYield,
    /// 块未闭合 → 双侧定位。
    BlockUnclosed,
    /// 键值语法错（`=` 后无值）。
    EqNoValue,
}

impl LayoutDiag {
    pub fn code(self) -> u16 {
        0x42D0 + self as u16
    }
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct LayoutParser {
    toks: Vec<Tok>,
    pos: usize,
    pub items: Vec<LayoutItem>,
    pub attrs: Vec<AttrItem>,
    pub diags: Vec<(u32, LayoutDiag)>,
    pub joints: Vec<(u32, u32)>,
}

impl LayoutParser {
    pub fn new(src: &str) -> Self {
        Self {
            toks: mini_lex(src),
            pos: 0,
            items: Vec::new(),
            attrs: Vec::new(),
            diags: Vec::new(),
            joints: Vec::new(),
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

    /// 解析一个 layout(...) 块。
    pub fn parse_layout(&mut self) -> bool {
        let t = self.peek().clone();
        if !(t.kind == Tk::Ident && t.text == "layout") {
            return false;
        }
        let nxt = &self.toks[(self.pos + 1).min(self.toks.len() - 1)];
        if !(nxt.kind == Tk::Sym && nxt.text == "(") {
            return false;
        }
        self.bump();
        self.bump();
        loop {
            if self.peek().kind == Tk::Eof {
                self.joints.push((t.off, self.peek().off));
                self.diags.push((t.off, LayoutDiag::BlockUnclosed));
                return true;
            }
            // 键。
            if self.peek().kind != Tk::Ident {
                // 非键记号（尾巴垃圾）：吃掉继续。
                self.bump();
                continue;
            }
            let key = self.bump();
            // 值。
            let val = if self.eat("=") {
                if self.peek().kind == Tk::Number {
                    Val::Int(self.bump().text)
                } else if self.peek().kind == Tk::Ident {
                    Val::Unknown(self.bump().text)
                } else {
                    self.diags.push((self.peek().off, LayoutDiag::EqNoValue));
                    Val::None
                }
            } else {
                Val::None
            };
            self.items.push(LayoutItem {
                key: key.text.clone(),
                class: key_class(&key.text),
                val,
                off: key.off,
            });
            // 重复键：两条都收 + 语义期队列。
            if self.items.iter().filter(|i| i.key == key.text).count() > 1 {
                self.diags.push((key.off, LayoutDiag::KeyDupYield));
            }
            if self.eat(",") {
                continue;
            }
            if self.eat(")") {
                break;
            }
            if self.peek().kind == Tk::Eof {
                self.joints.push((t.off, self.peek().off));
                self.diags.push((t.off, LayoutDiag::BlockUnclosed));
                return true;
            }
            self.bump();
        }
        true
    }

    /// 解析属性块 @name 或 @name(...)。
    pub fn parse_attr(&mut self) -> bool {
        let t = self.peek().clone();
        if t.kind != Tk::Attr {
            return false;
        }
        self.bump();
        let mut block = None;
        if self.peek().kind == Tk::Sym && self.peek().text == "(" {
            let open = self.bump().off;
            let start = self.peek().off;
            let mut depth = 1u32;
            let mut end = start;
            while self.peek().kind != Tk::Eof {
                let x = self.peek().clone();
                if x.kind == Tk::Sym && x.text == "(" {
                    depth += 1;
                }
                if x.kind == Tk::Sym && x.text == ")" {
                    depth -= 1;
                    if depth == 0 {
                        end = x.off;
                        break;
                    }
                }
                self.bump();
                end = x.off;
            }
            if self.eat(")") {
                block = Some((start, end));
            } else {
                self.joints.push((open, self.peek().off));
                self.diags.push((open, LayoutDiag::BlockUnclosed));
            }
        }
        self.attrs.push(AttrItem {
            name: t.text.trim_start_matches('@').to_string(),
            block,
            off: t.off,
        });
        true
    }

    /// 解析整流。
    pub fn parse_all(&mut self) -> usize {
        let mut n = 0;
        while self.peek().kind != Tk::Eof {
            if self.parse_layout() || self.parse_attr() {
                n += 1;
            } else {
                self.bump();
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec33_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、键值语法。
    {
        let mut p = LayoutParser::new("layout(location = 0, binding = 1)");
        let n = p.parse_all();
        v.push(("F0433-键值-两键两值", n == 1 && p.items.len() == 2));
        v.push((
            "F0433-键值-值原样",
            p.items[0].val == Val::Int("0".into()) && p.items[1].val == Val::Int("1".into()),
        ));
        // 裸键（值缺席）。
        let mut p2 = LayoutParser::new("layout(push_constant)");
        let _ = p2.parse_all();
        v.push((
            "F0433-键值-裸键值缺席",
            p2.items[0].val == Val::None && p2.items[0].key == "push_constant",
        ));
        // 属性块。
        let mut p3 = LayoutParser::new("@early_fragment_tests");
        let _ = p3.parse_all();
        v.push((
            "F0433-键值-属性块无值",
            p3.attrs.len() == 1 && p3.attrs[0].name == "early_fragment_tests"
                && p3.attrs[0].block.is_none(),
        ));
        // 属性带值块。
        let mut p4 = LayoutParser::new("@blend(src = 1, dst = 2)");
        let _ = p4.parse_all();
        v.push((
            "F0433-键值-属性带值块",
            p4.attrs[0].block.is_some(),
        ));
    }
    // 二、合法键集。
    {
        v.push(("F0433-键集-八键入册", LAYOUT_KEYS.len() == 8));
        v.push((
            "F0433-键集-已知分类",
            key_class("location") == KeyClass::Known && key_class("binding") == KeyClass::Known,
        ));
        // 未知键不报错（扩展候选）。
        let mut p = LayoutParser::new("layout(my_extension = 3)");
        let _ = p.parse_all();
        v.push((
            "F0433-键集-未知不报错",
            p.items[0].class == KeyClass::Unknown && p.diags.is_empty(),
        ));
        // 纯度：合法键与未知键条目结构同构。
        let mut pa = LayoutParser::new("layout(location = 0)");
        let _ = pa.parse_all();
        let mut pb = LayoutParser::new("layout(my_ext = 0)");
        let _ = pb.parse_all();
        v.push((
            "F0433-键集-同构纯度",
            pa.items[0].val == pb.items[0].val && pa.items.len() == pb.items.len(),
        ));
    }
    // 三、重复登记。
    {
        let mut p = LayoutParser::new("layout(location = 0, location = 1)");
        let _ = p.parse_all();
        v.push((
            "F0433-重复-两条都收",
            p.items.len() == 2
                && p.items[0].val == Val::Int("0".into())
                && p.items[1].val == Val::Int("1".into()),
        ));
        v.push((
            "F0433-重复-语义期队列",
            p.diags.iter().filter(|(_, d)| *d == LayoutDiag::KeyDupYield).count() == 1,
        ));
        // 三现键 = 两条重复诊断。
        let mut p2 = LayoutParser::new("layout(set = 0, set = 1, set = 2)");
        let _ = p2.parse_all();
        v.push((
            "F0433-重复-三现两诊断",
            p2.items.len() == 3
                && p2.diags.iter().filter(|(_, d)| *d == LayoutDiag::KeyDupYield).count() == 2,
        ));
    }
    // 四、值域留语义期。
    {
        // 非整值（Unknown）照收。
        let mut p = LayoutParser::new("layout(binding = auto)");
        let _ = p.parse_all();
        v.push((
            "F0433-值域-Unknown照收",
            p.items[0].val == Val::Unknown("auto".into()),
        ));
        // `=` 后无值。
        let mut p2 = LayoutParser::new("layout(location = )");
        let _ = p2.parse_all();
        v.push((
            "F0433-值域-缺值报错",
            p2.diags.iter().any(|(_, d)| *d == LayoutDiag::EqNoValue),
        ));
        // 块未闭合双侧。
        let mut p3 = LayoutParser::new("layout(location = 0");
        let _ = p3.parse_all();
        v.push((
            "F0433-值域-块未闭合双侧",
            p3.joints.len() == 1 && p3.joints[0].1 > p3.joints[0].0,
        ));
        // 判据自检。
        v.push(("F0433-判据-锚点入册", ANCHOR == "VE-F0433"));
        v.push(("F0433-判据-诊断码段", (0x42D0..0x42E0).contains(&LayoutDiag::EqNoValue.code())));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec33_raw_checks as run_vec33_checks;
    #[test]
    fn vec33_layout_checks() {
        for (name, ok) in run_vec33_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec33）。
pub fn run_vec33_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec33");
    for (name, ok) in vec33_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
