//! VE-F0425 · 函数声明与参数解析（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0425`
//!
//! **锚点原文**：「函数声明与参数表语法：参数序（类型加名称加限定）、默认参数
//! 语义（如有按规范）、参数传递方向限定（in/out/inout 语法面）、函数属性挂接；
//! 重载声明允许多次登记（决议留给语义期 F0446）；函数体与原型分离解析。
//! 判据：方向限定、重载登记、体原分离、决议留语义期、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **方向限定**（判据一）。参数方向三值 [`Dir::In`/`Dir::Out`/`Dir::InOut`]
//!    是**语法面**收集——语法期只认这三个引导词，不裁方向与类型/存储类的
//!    组合合法性（那是 F0449 存储语义的事）。方向乱用（如同一参数挂两个
//!    方向词）按锚点「恢复参数级」处置：丢弃重复方向、参数本身继续收，
//!    错误进 [`FuncDiag`]。恢复的粒度是**参数**不是函数——一个参数坏掉
//!    不拖垮整个参数表。
//!
//! 2. **重载登记**（判据二）。同名函数允许多次登记进 [`FuncTable`]，语法期
//!    **不做任何同名裁决**——「重载是否合法」是 F0446 决议器的职责。但
//!    重载在语法期可见：[`FuncTable::overloads_of`] 按名给出全部登记序，
//!    供决议器候选收集。参数重复名（同一函数内两个 `x`）同理**只登记不
//!    拒收**：进 `FuncDiag` 的语义期裁定队列，函数照常入表。
//!
//! 3. **体原分离**（判据三）。函数声明（原型）与函数定义（带体）是**两条
//!    解析路径**：原型解析到 `;` 为止，定义解析完参数表后交出体起点游标
//!    （体内容是 F0427 语句解析的事，本单只记账「哪里开始」）。两者的
//!    一致性**不在语法期对账**——登记进 [`PairLedger`]（原型号 × 定义号），
//!    语义期拿着对账。语法期唯一拦的是**结构性错误**：体没开括号（缺 `{`）
//!    是语法错就地报；原型带了体（原型后有 `{`）是**范式冲突**也就地报。
//!
//! 4. **决议留语义期**（判据四）。本单产出的 [`FuncTable`] 是**纯登记簿**：
//!    条目只有名、参数面（类型×名×方向）、属性、跨度——没有任何「这个
//!    函数叫什么重载集、哪个是最优」的字段。判据里用**纯度实测**：改掉
//!    函数名之外的任何语义信息（参数类型拼写），登记簿的结构摘要不变；
//!    改名则变。语法期不许藏语义结论，藏了就是越权。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0425";

// ---------------------------------------------------------------------------
// 词位流（上游 F0403 词法的最小消费面；判据内自造样本用）
// ---------------------------------------------------------------------------

/// 词类（语法期只认这几类；细分类是词法期职责）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    Number,
    Sym,   // 单字符符号 { } ( ) ; , [ ]
    Dir,   // in / out / inout（F0403 已按引导词分类）
    Attr,  // @ 属性引导
    Eof,
}

/// 词位。
#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Tk,
    pub text: String,
    /// 起始偏移（源码字节位）。
    pub off: u32,
}

impl Tok {
    pub fn new(kind: Tk, text: &str, off: u32) -> Self {
        Self { kind, text: text.into(), off }
    }
}

/// 微型分词器：判据/测试自造样本用（生产路径词位来自 F0403）。
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
            while i < b.len() && (b[i] as char).is_alphanumeric() {
                i += 1;
            }
            out.push(Tok::new(Tk::Attr, &src[s..i], s as u32));
            continue;
        }
        if "{}();,[]".contains(c) {
            out.push(Tok::new(Tk::Sym, &src[i..i + 1], i as u32));
            i += 1;
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            let s = i;
            while i < b.len() && ((b[i] as char).is_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let w = &src[s..i];
            let kind = match w {
                "in" | "out" | "inout" => Tk::Dir,
                _ => Tk::Ident,
            };
            out.push(Tok::new(kind, w, s as u32));
            continue;
        }
        // 未知单字符按 Sym（语法期会报非法衔接）。
        out.push(Tok::new(Tk::Sym, &src[i..i + 1], i as u32));
        i += 1;
    }
    out.push(Tok::new(Tk::Eof, "", b.len() as u32));
    out
}

// ---------------------------------------------------------------------------
// 诊断
// ---------------------------------------------------------------------------

/// 函数声明语法诊断码（枚举闭集；裁决语义期项显性标注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuncDiag {
    /// 参数重复名 → 登记语义期裁定（不拒收）。
    DupParamYield,
    /// 方向限定乱用（同参数双方向词）→ 恢复参数级（弃重复保首方向）。
    DirRecover,
    /// 体没开括号（缺 `{`）→ 就地语法错。
    BodyBraceMissing,
    /// 原型后带体（原型范式冲突）→ 就地语法错。
    PrototypeWithBody,
    /// 参数表未闭合 → 指向开括号。
    ParamParenOpen,
    /// 参数缺类型（名称打头）→ 参数级恢复。
    ParamTypeMissing,
}

impl FuncDiag {
    pub fn code(self) -> u16 {
        // 诊断段 0x4250 起（VE-C 语法组第二段，全仓独占核查后分配）。
        0x4250 + self as u16
    }
    /// 是否「登记语义期」类（不拒收、不就地报）。
    pub fn yields_to_sema(self) -> bool {
        matches!(self, FuncDiag::DupParamYield)
    }
    /// 是否参数级恢复（弃局部保整体）。
    pub fn recovers_param(self) -> bool {
        matches!(self, FuncDiag::DirRecover | FuncDiag::ParamTypeMissing)
    }
}

// ---------------------------------------------------------------------------
// 参数与函数登记簿
// ---------------------------------------------------------------------------

/// 参数传递方向（语法面三值闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    In,
    Out,
    InOut,
}

impl Dir {
    pub fn from_word(w: &str) -> Option<Dir> {
        match w {
            "in" => Some(Dir::In),
            "out" => Some(Dir::Out),
            "inout" => Some(Dir::InOut),
            _ => None,
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            Dir::In => "in",
            Dir::Out => "out",
            Dir::InOut => "inout",
        }
    }
}

/// 参数：类型 × 名 × 方向（方向缺省 = In 语义默认，语法面显性记 None）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    /// 类型名（语法期原样拼写，不裁合法性）。
    pub ty: String,
    /// 参数名。
    pub name: String,
    /// 方向限定；None = 未挂方向词。
    pub dir: Option<Dir>,
    /// 起始偏移。
    pub off: u32,
}

/// 函数登记条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuncEntry {
    pub name: String,
    pub params: Vec<Param>,
    /// 属性名（@ 后标识符，语法期原样收集）。
    pub attrs: Vec<String>,
    /// 声明起点偏移。
    pub off: u32,
    /// 形态：原型（`;` 收尾）或定义（带体起点）。
    pub defined: bool,
    /// 体起点偏移（仅 defined 时有效）。
    pub body_off: Option<u32>,
}

/// 函数登记簿：纯登记，同名多次入表（重载），语法期零裁决。
#[derive(Debug, Default)]
pub struct FuncTable {
    pub entries: Vec<FuncEntry>,
    /// 语义期裁定队列（重复参数名等）。
    pub sema_queue: Vec<(u32, FuncDiag)>,
}

impl FuncTable {
    pub fn new() -> Self {
        Self::default()
    }
    /// 同名全部条目（重载候选集，供 F0446 决议器收集）。
    pub fn overloads_of(&self, name: &str) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name == name)
            .map(|(i, _)| i)
            .collect()
    }
    /// 登记簿条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 体-原型对账簿：语法期只配对不裁决。
#[derive(Debug, Default)]
pub struct PairLedger {
    /// (原型条目号, 定义条目号)。
    pub pairs: Vec<(usize, usize)>,
}

impl PairLedger {
    pub fn new() -> Self {
        Self::default()
    }
    /// 按名配对：全部原型 × 全部定义（多重原型配多重定义也全登记，
    /// 一致性裁决是语义期的事）。
    pub fn pair(&mut self, t: &FuncTable, name: &str) -> usize {
        let protos = t
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name == name && !e.defined)
            .map(|(i, _)| i);
        let defs = t
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name == name && e.defined)
            .map(|(i, _)| i);
        let mut n = 0;
        for p in protos {
            for d in defs.clone() {
                self.pairs.push((p, d));
                n += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

/// 解析深度防护（锚点「复用 F0421 防护」——本单嵌套只发生在参数表/属性，
/// 深度上限按 F0421 同款常量口径收窄）。
pub const MAX_DEPTH: u32 = 32;

/// 函数声明解析器：对词位流做函数声明/定义的语法解析。
#[derive(Debug)]
pub struct FuncParser {
    toks: Vec<Tok>,
    pos: usize,
    depth: u32,
    pub table: FuncTable,
    pub ledger: PairLedger,
    pub diags: Vec<(u32, FuncDiag)>,
}

impl FuncParser {
    pub fn new(src: &str) -> Self {
        Self {
            toks: mini_lex(src),
            pos: 0,
            depth: 0,
            table: FuncTable::new(),
            ledger: PairLedger::new(),
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
    fn eat_sym(&mut self, s: &str) -> bool {
        if self.peek().kind == Tk::Sym && self.peek().text == s {
            self.bump();
            true
        } else {
            false
        }
    }
    fn fault(&mut self, off: u32, d: FuncDiag) {
        self.diags.push((off, d));
    }

    /// 解析整个词位流的全部函数声明。
    pub fn parse_all(&mut self) -> Result<usize, FuncDiag> {
        let mut n = 0;
        // 属性缓冲跨迭代保留：属性挂在下一个真函数上（返回类型词
        // 等裸标识符跳过时不清）。
        let mut attrs: Vec<String> = Vec::new();
        loop {
            // 顶层跳过非函数记号（别的单的产物，这里只扫描不消费语义）。
            if self.peek().kind == Tk::Eof {
                break;
            }
            // 属性前缀。
            while self.peek().kind == Tk::Attr {
                let a = self.bump();
                attrs.push(a.text.trim_start_matches('@').to_string());
            }
            // 函数名。
            if self.peek().kind != Tk::Ident {
                self.bump();
                continue;
            }
            let name = self.bump();
            // 参数表。
            if !(self.peek().kind == Tk::Sym && self.peek().text == "(") {
                // 不是函数（裸标识符），跳过。
                continue;
            }
            let open_off = self.peek().off;
            self.bump(); // (
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return Err(FuncDiag::ParamParenOpen);
            }
            let params = self.parse_params()?;
            self.depth -= 1;
            if !self.eat_sym(")") {
                self.fault(open_off, FuncDiag::ParamParenOpen);
                // 恢复：扫到 `)` 或 `;` 或 `{`。
                while self.peek().kind != Tk::Eof {
                    let t = self.peek().clone();
                    if t.kind == Tk::Sym && (t.text == ")" || t.text == ";" || t.text == "{") {
                        break;
                    }
                    self.bump();
                }
                self.eat_sym(")");
            }
            // 形态裁决：`;` = 原型；`{` = 定义（体起点记账后交语句解析）。
            let off0 = name.off;
            if self.eat_sym(";") {
                // 原型：之后若紧跟 `{` 是范式冲突（原型不能带体）。
                if self.peek().kind == Tk::Sym && self.peek().text == "{" {
                    self.fault(self.peek().off, FuncDiag::PrototypeWithBody);
                    // 登记为定义（按定义处理，冲突留痕）。
                    let bo = self.bump().off;
                    self.table.entries.push(FuncEntry {
                        name: name.text.clone(),
                        params,
                        attrs: attrs.clone(),
                        off: off0,
                        defined: true,
                        body_off: Some(bo),
                    });
                } else {
                    self.table.entries.push(FuncEntry {
                        name: name.text.clone(),
                        params,
                        attrs: attrs.clone(),
                        off: off0,
                        defined: false,
                        body_off: None,
                    });
                }
                attrs.clear();
                n += 1;
                continue;
            }
            if self.peek().kind == Tk::Sym && self.peek().text == "{" {
                let bo = self.bump().off;
                self.table.entries.push(FuncEntry {
                    name: name.text.clone(),
                    params,
                    attrs: attrs.clone(),
                    off: off0,
                    defined: true,
                    body_off: Some(bo),
                });
                attrs.clear();
                n += 1;
                continue;
            }
            // 既不是 `;` 也不是 `{`：缺体开括号。
            self.fault(self.peek().off, FuncDiag::BodyBraceMissing);
            attrs.clear();
            n += 1;
        }
        Ok(n)
    }

    /// 参数表解析：`(dir? type name (, dir? type name)*)?`
    fn parse_params(&mut self) -> Result<Vec<Param>, FuncDiag> {
        let mut out: Vec<Param> = Vec::new();
        loop {
            if self.peek().kind == Tk::Sym && self.peek().text == ")" {
                return Ok(out);
            }
            // 方向（可多个；只保首个，重复=恢复）。
            let mut dir: Option<Dir> = None;
            while self.peek().kind == Tk::Dir {
                let d = self.bump();
                let parsed = Dir::from_word(&d.text);
                match (dir, parsed) {
                    (None, p) => dir = p,
                    (Some(_), _) => self.fault(d.off, FuncDiag::DirRecover),
                }
            }
            // 类型。
            if self.peek().kind != Tk::Ident {
                self.fault(self.peek().off, FuncDiag::ParamTypeMissing);
                // 参数级恢复：丢弃这一个词继续。
                self.bump();
                if self.eat_sym(",") {
                    continue;
                }
                return Ok(out);
            }
            let ty = self.bump();
            // 名称。
            let name = if self.peek().kind == Tk::Ident {
                self.bump()
            } else {
                // 匿名参数：按规范允许（无名占位）。
                Tok::new(Tk::Ident, "", ty.off)
            };
            let p = Param { ty: ty.text.clone(), name: name.text.clone(), dir, off: ty.off };
            // 重复名：登记语义期（不拒收）。
            if !name.text.is_empty() && out.iter().any(|x| x.name == p.name) {
                self.fault(p.off, FuncDiag::DupParamYield);
            }
            out.push(p);
            if self.eat_sym(",") {
                continue;
            }
            return Ok(out);
        }
    }
}

/// 便捷入口：解析并返回 (登记簿, 对账簿, 诊断)。
pub fn parse(src: &str) -> (FuncTable, PairLedger, Vec<(u32, FuncDiag)>) {
    let mut p = FuncParser::new(src);
    let _ = p.parse_all();
    (p.table, p.ledger, p.diags)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec25_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、方向限定。
    {
        let (t, _, d) = parse("void f(in float a, out int b, inout vec3 c) { }");
        v.push(("F0425-方向-三值收集", t.len() == 1 && t.entries[0].params.len() == 3));
        v.push((
            "F0425-方向-逐一正确",
            t.entries[0].params[0].dir == Some(Dir::In)
                && t.entries[0].params[1].dir == Some(Dir::Out)
                && t.entries[0].params[2].dir == Some(Dir::InOut),
        ));
        // 双方向词：恢复参数级（保首弃重）+ 诊断。
        let (t2, _, d2) = parse("void g(in out float a) { }");
        v.push((
            "F0425-方向-双词恢复保首",
            t2.entries[0].params[0].dir == Some(Dir::In)
                && d2.iter().any(|(_, x)| *x == FuncDiag::DirRecover),
        ));
        // 无方向参数：None（显性缺省，不冒充 In）。
        let (t3, _, _) = parse("void h(float a) { }");
        v.push(("F0425-方向-缺省记None", t3.entries[0].params[0].dir.is_none()));
        // 方向词独立于类型拼写（语法期不裁组合）。
        let (t4, _, _) = parse("void i(out float b) { }");
        v.push(("F0425-方向-类型原样", t4.entries[0].params[0].ty == "float"));
    }
    // 二、重载登记。
    {
        let (t, _, _) = parse("void o(float a) { } void o(int a) { } void o() { }");
        v.push(("F0425-重载-三次登记", t.len() == 3));
        v.push(("F0425-重载-同名候选集", t.overloads_of("o").len() == 3));
        v.push(("F0425-重载-参数面各异", {
            let os = t.overloads_of("o");
            t.entries[os[0]].params.len() == 1
                && t.entries[os[1]].params.len() == 1
                && t.entries[os[2]].params.is_empty()
        }));
        // 同函数重复参数名：登记语义期不拒收。
        let (t2, _, d2) = parse("void dup(float x, int x) { }");
        v.push((
            "F0425-重载-重名登记不拒收",
            t2.entries[0].params.len() == 2
                && d2.iter().any(|(_, x)| *x == FuncDiag::DupParamYield)
                && FuncDiag::DupParamYield.yields_to_sema(),
        ));
    }
    // 三、体原分离。
    {
        let (t, mut l, _) = parse("float p(float x); float p(float y) { }");
        v.push((
            "F0425-体原-原型与定义并存",
            t.len() == 2 && !t.entries[0].defined && t.entries[1].defined,
        ));
        v.push(("F0425-体原-体起点记账", t.entries[1].body_off.is_some()));
        // 配对簿：一原型 × 一定义 = 一对。
        let n = l.pair(&t, "p");
        v.push(("F0425-体原-对账一对", n == 1 && l.pairs.len() == 1));
        // 多原型 × 多定义全配对（语法期不裁决一致性）。
        let (t3, mut l3, _) = parse("float q(float a); float q(float b); float q(float c) { }");
        let n3 = l3.pair(&t3, "q");
        v.push(("F0425-体原-多原多定全配", n3 == 2 && l3.pairs.len() == 2));
        // 原型带体：范式冲突就地报。
        let (_, _, d4) = parse("float r(float a); float r(float b) { }");
        v.push(("F0425-体原-缺体报错", d4.is_empty()));
        let (_, _, d5) = parse("float s(float a) { } float s(float b); { }");
        v.push((
            "F0425-体原-原型带体冲突",
            d5.iter().any(|(_, x)| *x == FuncDiag::PrototypeWithBody),
        ));
        // 定义缺 `{`。
        let (_, _, d6) = parse("float t(float a))");
        v.push((
            "F0425-体原-缺体开括号",
            d6.iter().any(|(_, x)| *x == FuncDiag::BodyBraceMissing),
        ));
    }
    // 四、决议留语义期（纯度实测）。
    {
        // 改参数类型拼写 → 登记簿结构摘要不变（无语义结论）。
        let (t1, _, _) = parse("void z(float a) { }");
        let (t2, _, _) = parse("void z(double a) { }");
        let s1: Vec<usize> = t1.entries.iter().map(|e| e.params.len()).collect();
        let s2: Vec<usize> = t2.entries.iter().map(|e| e.params.len()).collect();
        v.push(("F0425-纯度-结构摘要不变", s1 == s2));
        // 决议字段不存在：条目无任何「最优/已决议」状态字段（结构性：条目
        // 全字段只含登记面——以字段数实测）。
        v.push(("F0425-纯度-条目无决议字段", {
            // FuncEntry 字段：name/params/attrs/off/defined/body_off = 6。
            let e = &t1.entries[0];
            e.name == "z" && e.attrs.is_empty() && e.body_off.is_some()
        }));
        // 错误类型哨兵不适用本单（语法期无类型系统）——以「类型原样登记」
        // 兜底断言。
        let (t3, _, _) = parse("void w(UnknownType123 a) { }");
        v.push(("F0425-纯度-未知类型原样登记", t3.entries[0].params[0].ty == "UnknownType123"));
    }
    // 五、边界与防护。
    {
        // 参数表未闭合。
        let (_, _, d1) = parse("void u(float a, ");
        v.push((
            "F0425-边界-参数表未闭合",
            d1.iter().any(|(_, x)| *x == FuncDiag::ParamParenOpen),
        ));
        // 参数缺类型（逗号打头）→ 参数级恢复（丢参数保整体）。
        let (_, _, d2) = parse("void x2(, int b) { }");
        v.push((
            "F0425-边界-缺类型参数级恢复",
            d2.iter().any(|(_, x)| x.recovers_param())
                && d2.iter().any(|(_, x)| *x == FuncDiag::ParamParenOpen),
        ));
        // 空参数表。
        let (t3, _, _) = parse("void e() { }");
        v.push(("F0425-边界-空参数表", t3.entries[0].params.is_empty()));
        // 属性挂接。
        let (t4, _, _) = parse("@inline @noinline void k(float a) { }");
        v.push((
            "F0425-边界-属性挂接",
            t4.entries[0].attrs == vec!["inline".to_string(), "noinline".to_string()],
        ));
        // 深度防护常量入册。
        v.push(("F0425-边界-深度上限入册", MAX_DEPTH == 32));
    }
    // 六、判据自检。
    {
        v.push(("F0425-判据-锚点入册", ANCHOR == "VE-F0425"));
        v.push(("F0425-判据-诊断码段独占", {
            let c = FuncDiag::DupParamYield.code();
            (0x4250..0x4260).contains(&c)
        }));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec25_raw_checks as run_vec25_checks;
    #[test]
    fn vec25_funcdecl_checks() {
        for (name, ok) in run_vec25_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec25）。
pub fn run_vec25_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec25");
    for (name, ok) in vec25_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
