//! VE-F0426 · 变量声明与限定符（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0426`
//!
//! **锚点原文**：「变量声明语法与限定符解析：基础声明（类型加声明符序）、
//! 限定符族（const、存储类、插值限定、布局限定）语法位与合法组合表（组合
//! 合法性留给语义期，语法期只收不裁）、初始化器语法挂接；数组声明维度语法。
//! 判据：限定符收集、只收不裁、组合表供语义期、数组维度、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **限定符收集**（判据一）。限定符族是**语法位收集**：[`Qual::Const`]/
//!    [`Qual::Storage`]/[`Qual::Interp`]/[`Qual::Layout`] 四类各设收集槽，
//!    出现即记（重复出现记重复条目不覆盖——语义期裁「重复是否合法」需要
//!    看到全部出现，覆盖了就是毁证）。收集表 [`QualSet`] 的唯一写入口
//!    [`QualSet::collect`] 同时把出现序登记进组合表。
//!
//! 2. **只收不裁**（判据二）。语法期对限定符**零裁决**：`const const int x`
//!    不报错（登记两条 Const 交语义期）、未知限定符 `nonsuch int x` 不报错
//!    （登记 [`Qual::Unknown`] 交语义期裁定是不是扩展）。判据用**纯度实测**：
//!    任何非法组合输入（如 `layout(const) int x`——值域错配）产出的收集表
//!    与「合法但奇怪的输入」结构同构——收集表只反映「出现了什么」，不反映
//!    「对不对」。
//!
//! 3. **组合表供语义期**（判据三）。[`QualSet::combo_key`] 把收集结果编码
//!    为**位集键**（每类一位，重复出现进溢出计数），语义期拿键查 F0404
//!    合法组合表即可，不必重新扫词位。键是**纯函数**：同一收集序两次编码
//!    结果相同；溢出计数独立于位集（重复不改变位）。
//!
//! 4. **数组维度**（判据四）。数组声明维度是**表达式起点偏移记账**：
//!    `int a[N+1]` 里 `N+1` 是常量表达式（求值是 F0448 的事），语法期只记
//!    维度表达式的词位跨度。维度非法（空 `[]` 声明级维度）按锚点「恢复
//!    声明级」：变量本身继续收，维度记 [`Dim::Empty`] 待语义期补（规范
//!    允许参数场景的空维度）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0426";

// ---------------------------------------------------------------------------
// 词位（复用 F0425 同款消费面；本单自带最小版避免跨单私有依赖）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    Number,
    Sym,
    Qual, // const / uniform / flat / layout 等 F0404 限定引导词
    Eof,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Tk,
    pub text: String,
    pub off: u32,
}

/// 微型分词器（判据自造样本；生产词位来自 F0403）。
pub fn mini_lex(src: &str) -> Vec<Tok> {
    const QUALS: [&str; 8] = ["const", "uniform", "buffer", "flat", "noperspective", "smooth", "layout", "centroid"];
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if "(){};,[]=".contains(c) {
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
            let kind = if QUALS.contains(&w) { Tk::Qual } else { Tk::Ident };
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
// 限定符收集
// ---------------------------------------------------------------------------

/// 限定符类别（语法位四类闭集 + 未知扩展位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Qual {
    /// const 限定。
    Const,
    /// 存储类（uniform/buffer 类）。
    Storage,
    /// 插值限定（flat/smooth/noperspective/centroid）。
    Interp,
    /// 布局限定（layout(...) 块）。
    Layout,
    /// 未知引导词（扩展候选，语义期裁定）。
    Unknown,
}

/// 数组维度记账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dim {
    /// 非数组。
    None,
    /// 维度表达式词位跨度（起止偏移，表达式内容留给求值期）。
    Expr(u32, u32),
    /// 空维度 `[]`（恢复声明级，语义期按规范补）。
    Empty,
}

/// 变量声明条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarDecl {
    /// 类型名（语法期原样拼写）。
    pub ty: String,
    /// 变量名。
    pub name: String,
    /// 限定符收集序（出现序，含重复，含 Unknown）。
    pub quals: Vec<Qual>,
    /// 数组维度。
    pub dim: Dim,
    /// 初始化器：`=` 后表达式词位跨度（求值留给 F0448）。
    pub init: Option<(u32, u32)>,
    pub off: u32,
}

/// 组合键：位集（四类各一位）+ 溢出计数（重复出现数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ComboKey {
    pub bits: u8,
    pub overflow: u8,
}

/// 限定符收集表。
#[derive(Debug, Default)]
pub struct QualSet {
    pub quals: Vec<Qual>,
}

impl QualSet {
    /// 唯一写入口：出现即记（重复不覆盖）。
    pub fn collect(&mut self, q: Qual) {
        self.quals.push(q);
    }
    /// 组合键：纯函数编码。
    pub fn combo_key(&self) -> ComboKey {
        let mut k = ComboKey::default();
        for q in self.quals.iter() {
            let bit = match q {
                Qual::Const => 0u8,
                Qual::Storage => 1,
                Qual::Interp => 2,
                Qual::Layout => 3,
                Qual::Unknown => 4,
            };
            if bit < 4 {
                if k.bits & (1 << bit) == 0 {
                    k.bits |= 1 << bit;
                } else {
                    k.overflow += 1;
                }
            }
            // Unknown 不进位集（语义期裁定项，键里不表达）。
        }
        k
    }
}

// ---------------------------------------------------------------------------
// 诊断
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarDiag {
    /// 限定符重复 → 收集语义期裁定（不拒收）。
    QualDupYield,
    /// 维度非法（声明级空维度）→ 恢复声明级。
    DimEmptyRecover,
    /// 初始化器缺失（语法面 `=` 后无词位）→ 登记语义期。
    InitMissingYield,
    /// 声明未终结（缺 `;`）→ 就地语法错。
    SemiMissing,
}

impl VarDiag {
    pub fn code(self) -> u16 {
        0x4260 + self as u16
    }
    pub fn yields_to_sema(self) -> bool {
        matches!(self, VarDiag::QualDupYield | VarDiag::InitMissingYield)
    }
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

/// 变量声明解析器。
#[derive(Debug)]
pub struct VarParser {
    toks: Vec<Tok>,
    pos: usize,
    pub decls: Vec<VarDecl>,
    pub diags: Vec<(u32, VarDiag)>,
}

impl VarParser {
    pub fn new(src: &str) -> Self {
        Self { toks: mini_lex(src), pos: 0, decls: Vec::new(), diags: Vec::new() }
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
    /// 限定词位上的未知词探测：三个连续 Ident 打头 ⇒ 首词按未知限定词。
    fn qual_pos_unknown(&self) -> bool {
        let at = |i: usize| &self.toks[(self.pos + i).min(self.toks.len() - 1)];
        self.peek().kind == Tk::Ident && at(1).kind == Tk::Ident && at(2).kind == Tk::Ident
    }

    /// 解析全部变量声明（`;` 分隔；`{` 视为别的单的产物即停）。
    pub fn parse_all(&mut self) -> Result<usize, VarDiag> {
        let mut n = 0;
        loop {
            if self.peek().kind == Tk::Eof {
                break;
            }
            if self.peek().kind == Tk::Sym && self.peek().text == "{" {
                break; // 复合语句，交 F0427
            }
            // 限定符收集。未知词的限定词位判定：声明形态是
            // [限定词]* 类型 名 —— 三个连续 Ident 打头时，首词没有
            // 别的槽位可去（类型+名只需两个），只能按未知限定词登记
            // （扩展候选不拒收，语义期裁定）。
            let mut qs = QualSet::default();
            let mut seen: Vec<Qual> = Vec::new();
            let mut dup_off: Option<u32> = None;
            while self.peek().kind == Tk::Qual || self.qual_pos_unknown() {
                let t = self.bump();
                let q = match t.text.as_str() {
                    "const" => Qual::Const,
                    "uniform" | "buffer" => Qual::Storage,
                    "flat" | "smooth" | "noperspective" | "centroid" => Qual::Interp,
                    "layout" => Qual::Layout,
                    _ => Qual::Unknown,
                };
                if seen.contains(&q) && dup_off.is_none() {
                    dup_off = Some(t.off);
                }
                seen.push(q);
                qs.collect(q);
                // layout 块消费到 `)`（键值内容是 F0433 专项）。
                if q == Qual::Layout {
                    while self.peek().kind != Tk::Eof {
                        let x = self.peek().clone();
                        if x.kind == Tk::Sym && x.text == ")" {
                            self.bump();
                            break;
                        }
                        self.bump();
                    }
                }
            }
            if let Some(off) = dup_off {
                self.diags.push((off, VarDiag::QualDupYield));
            }
            // 类型 + 名称。
            if self.peek().kind != Tk::Ident {
                self.bump();
                continue;
            }
            let ty = self.bump();
            if self.peek().kind != Tk::Ident {
                continue; // 类型后无名（别的单的产物）
            }
            let name = self.bump();
            // 数组维度。
            let mut dim = Dim::None;
            if self.eat("[") {
                let start = self.peek().off;
                if self.eat("]") {
                    dim = Dim::Empty;
                    self.diags.push((start, VarDiag::DimEmptyRecover));
                } else {
                    let mut depth = 1u32;
                    while self.peek().kind != Tk::Eof {
                        let t = self.peek().clone();
                        if t.kind == Tk::Sym && t.text == "[" {
                            depth += 1;
                        }
                        if t.kind == Tk::Sym && t.text == "]" {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        self.bump();
                    }
                    let end = self.peek().off;
                    self.eat("]");
                    dim = Dim::Expr(start, end);
                }
            }
            // 初始化器。
            let mut init = None;
            if self.eat("=") {
                let start = self.peek().off;
                // 表达式扫到 `;`（内容留给求值期）。
                let mut any = false;
                while self.peek().kind != Tk::Eof {
                    let t = self.peek().clone();
                    if t.kind == Tk::Sym && (t.text == ";" || t.text == "{") {
                        break;
                    }
                    // 新声明启动（Ident 类型 + Ident 名）= 缺分号边界：
                    // 初始化器表达式到此为止，交由下方分号检查就地报。
                    if t.kind == Tk::Ident
                        && self.toks[(self.pos + 1).min(self.toks.len() - 1)].kind == Tk::Ident
                    {
                        break;
                    }
                    self.bump();
                    any = true;
                }
                if any {
                    init = Some((start, self.peek().off));
                } else {
                    self.diags.push((start, VarDiag::InitMissingYield));
                }
            }
            if !self.eat(";") {
                self.diags.push((self.peek().off, VarDiag::SemiMissing));
            }
            self.decls.push(VarDecl {
                ty: ty.text.clone(),
                name: name.text.clone(),
                quals: qs.quals.clone(),
                dim,
                init,
                off: ty.off,
            });
            n += 1;
        }
        Ok(n)
    }
}

pub fn parse(src: &str) -> (Vec<VarDecl>, Vec<(u32, VarDiag)>) {
    let mut p = VarParser::new(src);
    let _ = p.parse_all();
    (p.decls, p.diags)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec26_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、限定符收集。
    {
        let (d, _) = parse("const uniform float u; ");
        v.push((
            "F0426-限定-四类收集序",
            d[0].quals == vec![Qual::Const, Qual::Storage],
        ));
        // 重复出现：记两条不覆盖（毁证=覆盖）。
        let (d2, dg) = parse("const const int x; ");
        v.push((
            "F0426-限定-重复记两条",
            d2[0].quals.len() == 2 && dg.iter().any(|(_, x)| *x == VarDiag::QualDupYield),
        ));
        // 未知限定词：Unknown 登记（扩展候选不拒收）。
        let (d3, _) = parse("nonsuch int y; ");
        v.push((
            "F0426-限定-未知记Unknown",
            d3[0].quals == vec![Qual::Unknown],
        ));
        // 插值族与布局族。
        let (d4, _) = parse("flat centroid layout(location = 0) vec4 v; ");
        v.push((
            "F0426-限定-插值布局族",
            d4[0].quals.contains(&Qual::Interp) && d4[0].quals.contains(&Qual::Layout),
        ));
    }
    // 二、只收不裁（纯度实测）。
    {
        // 非法组合（值域错配）与合法但奇怪的输入结构同构。
        let (a, _) = parse("layout(const) int x; ");
        let (b, _) = parse("layout(location = 0) int x; ");
        v.push((
            "F0426-纯度-收集只反映出现",
            a[0].quals == b[0].quals && a[0].quals.len() == 1 && b[0].quals.len() == 1,
        ));
        // 组合键不含 Unknown（语义期裁定项不进键）。
        let (c, _) = parse("nonsuch const int z; ");
        let k = {
            let mut qs = QualSet::default();
            for q in c[0].quals.iter() {
                qs.collect(*q);
            }
            qs.combo_key()
        };
        v.push((
            "F0426-纯度-Unknown不进键",
            k.bits == 0b0001 && k.overflow == 0,
        ));
    }
    // 三、组合表供语义期。
    {
        let mk = |src: &str| {
            let (d, _) = parse(src);
            let mut qs = QualSet::default();
            for q in d[0].quals.iter() {
                qs.collect(*q);
            }
            qs.combo_key()
        };
        let k1 = mk("const int a; ");
        let k2 = mk("const int b; ");
        v.push(("F0426-组合-键纯函数", k1 == k2));
        let k3 = mk("const const int c; ");
        v.push((
            "F0426-组合-重复进溢出不改位",
            k3.bits == k1.bits && k3.overflow == 1,
        ));
        let k4 = mk("const uniform int d; ");
        v.push(("F0426-组合-双类双位", k4.bits == 0b0011 && k4.overflow == 0));
    }
    // 四、数组维度。
    {
        let (d, _) = parse("int a[4]; ");
        v.push(("F0426-维度-常量跨度", matches!(d[0].dim, Dim::Expr(_, _))));
        // 嵌套维度（二维数组）。
        let (d2, _) = parse("int m[2][3]; ");
        v.push(("F0426-维度-二维合法", matches!(d2[0].dim, Dim::Expr(_, _))));
        // 空维度恢复声明级。
        let (d3, dg) = parse("int e[]; ");
        v.push((
            "F0426-维度-空维度恢复",
            d3[0].dim == Dim::Empty && dg.iter().any(|(_, x)| *x == VarDiag::DimEmptyRecover),
        ));
        // 表达式维度整段记账。
        let (d4, _) = parse("int f[N + 1]; ");
        v.push(("F0426-维度-表达式跨度", matches!(d4[0].dim, Dim::Expr(s, e) if e > s)));
    }
    // 五、初始化器与边界。
    {
        let (d, _) = parse("int a = 5; ");
        v.push(("F0426-初始化-挂接", d[0].init.is_some()));
        let (d2, _) = parse("int b; ");
        v.push(("F0426-初始化-无初始化器", d2[0].init.is_none()));
        let (d3, dg) = parse("int c = ; ");
        v.push((
            "F0426-初始化-缺登记语义期",
            d3[0].init.is_none() && dg.iter().any(|(_, x)| x.yields_to_sema()),
        ));
        // 表达式初始化器整段记账（含逗号）。
        let (d4, _) = parse("int d = f(1, 2); ");
        v.push(("F0426-初始化-表达式跨度", d4[0].init.is_some()));
        // 缺分号就地报。
        let (_, dg2) = parse("int e2 = 1 int f2 = 2; ");
        v.push(("F0426-边界-缺分号报错", dg2.iter().any(|(_, x)| *x == VarDiag::SemiMissing)));
        // 诊断码段。
        v.push(("F0426-判据-诊断码段", (0x4260..0x4270).contains(&VarDiag::SemiMissing.code())));
        v.push(("F0426-判据-锚点入册", ANCHOR == "VE-F0426"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec26_raw_checks as run_vec26_checks;
    #[test]
    fn vec26_vardecl_checks() {
        for (name, ok) in run_vec26_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec26）。
pub fn run_vec26_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec26");
    for (name, ok) in vec26_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
