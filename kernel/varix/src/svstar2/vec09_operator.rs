//! VE-F0409 · 运算符全集与优先级（VE-C 域 · 着色器系统 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0409`
//!
//! **判据（锚点原文）**：最长匹配、总表对齐、歧义登记、语境裁定、判据。
//! - 运算符**全集**词法登记（算术/位/逻辑/关系/成员/赋值六类 32 形态），
//!   词法匹配**最长优先**（`<<=` 先于 `<<` 先于 `<`，逐长度层定长尝试，
//!   单次匹配 O(1)）；
//! - **优先级与结合性总表**（对齐规范 F0402 的节号引用：P1…P13 共十三级，
//!   每级登记节号锚）；赋值族全族右结合，其余双目左结合——与 C 系惯例
//!   对齐，不发明私有语义；
//! - **歧义消解登记**：模板实参列表 `<T>` 与小于号、嵌套泛型闭合 `>>`
//!   与移位、一元负号与减法等——词法期固定默认 + 语法期语境裁定的登记
//!   表（判据：歧义未登记 = 缺陷，语法期发现后回填登记）；
//! - 未知符号序列 → 报错**带最近合法运算符建议**（同首字符候选优先）；
//! - 表与规范冲突 → 自检对齐修正（不变量机检：形态唯一/级别连续/右结合
//!   仅赋值族/每级非空）。
//!
//! 性能逐项分解：匹配 O(1) 最长优先；查表 O(1)；登记 O(1)。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、运算符总表（形态 × 最长匹配 × 优先级 × 结合性）
// ---------------------------------------------------------------------------

/// 运算符类别（判据：全集六类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    /// 算术：+ - * / %
    Arith,
    /// 位：& | ^ ~ << >>
    Bitwise,
    /// 逻辑：&& || !
    Logical,
    /// 关系：== != < <= > >=
    Relational,
    /// 成员：.（成员访问）
    Member,
    /// 赋值：= 及复合赋值全族
    Assign,
}

/// 一条运算符登记（总表的行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpSpec {
    /// 词法形态（逐字节精确匹配）
    pub form: &'static str,
    pub kind: OpKind,
    /// 优先级级别（1 最高——成员访问；13 最低——赋值族）
    pub precedence: u8,
    /// 结合性：true = 右结合（仅赋值族与一元前缀）
    pub right_assoc: bool,
    /// 规范节号锚（对齐 F0402 规范文档体系——判据"总表对齐"）
    pub spec_ref: &'static str,
}

/// 优先级级别常量（总表口径，P1…P13）。
pub mod prec {
    pub const MEMBER: u8 = 1;
    pub const UNARY: u8 = 2;
    pub const MUL: u8 = 3;
    pub const ADD: u8 = 4;
    pub const SHIFT: u8 = 5;
    pub const REL: u8 = 6;
    pub const EQ: u8 = 7;
    pub const BIT_AND: u8 = 8;
    pub const BIT_XOR: u8 = 9;
    pub const BIT_OR: u8 = 10;
    pub const LOGIC_AND: u8 = 11;
    pub const LOGIC_OR: u8 = 12;
    pub const ASSIGN: u8 = 13;
}

/// 运算符全集（32 形态；判据"全集"的单一事实源——加形态必须过对齐机检）。
pub const OPERATORS: &[OpSpec] = &[
    // 成员（P1，左结合 postfix）
    OpSpec { form: ".", kind: OpKind::Member, precedence: prec::MEMBER, right_assoc: false, spec_ref: "F0402#S2-P1" },
    // 一元前缀（P2，右结合；+/- 的一元用法不重复登记——语境裁定见歧义表）
    OpSpec { form: "!", kind: OpKind::Logical, precedence: prec::UNARY, right_assoc: true, spec_ref: "F0402#S2-P2" },
    OpSpec { form: "~", kind: OpKind::Bitwise, precedence: prec::UNARY, right_assoc: true, spec_ref: "F0402#S2-P2" },
    // 乘除模（P3）
    OpSpec { form: "*", kind: OpKind::Arith, precedence: prec::MUL, right_assoc: false, spec_ref: "F0402#S2-P3" },
    OpSpec { form: "/", kind: OpKind::Arith, precedence: prec::MUL, right_assoc: false, spec_ref: "F0402#S2-P3" },
    OpSpec { form: "%", kind: OpKind::Arith, precedence: prec::MUL, right_assoc: false, spec_ref: "F0402#S2-P3" },
    // 加减（P4）
    OpSpec { form: "+", kind: OpKind::Arith, precedence: prec::ADD, right_assoc: false, spec_ref: "F0402#S2-P4" },
    OpSpec { form: "-", kind: OpKind::Arith, precedence: prec::ADD, right_assoc: false, spec_ref: "F0402#S2-P4" },
    // 移位（P5）
    OpSpec { form: "<<", kind: OpKind::Bitwise, precedence: prec::SHIFT, right_assoc: false, spec_ref: "F0402#S2-P5" },
    OpSpec { form: ">>", kind: OpKind::Bitwise, precedence: prec::SHIFT, right_assoc: false, spec_ref: "F0402#S2-P5" },
    // 关系（P6）
    OpSpec { form: "<=", kind: OpKind::Relational, precedence: prec::REL, right_assoc: false, spec_ref: "F0402#S2-P6" },
    OpSpec { form: ">=", kind: OpKind::Relational, precedence: prec::REL, right_assoc: false, spec_ref: "F0402#S2-P6" },
    OpSpec { form: "<", kind: OpKind::Relational, precedence: prec::REL, right_assoc: false, spec_ref: "F0402#S2-P6" },
    OpSpec { form: ">", kind: OpKind::Relational, precedence: prec::REL, right_assoc: false, spec_ref: "F0402#S2-P6" },
    // 相等（P7）
    OpSpec { form: "==", kind: OpKind::Relational, precedence: prec::EQ, right_assoc: false, spec_ref: "F0402#S2-P7" },
    OpSpec { form: "!=", kind: OpKind::Relational, precedence: prec::EQ, right_assoc: false, spec_ref: "F0402#S2-P7" },
    // 位与/异或/或（P8-P10）
    OpSpec { form: "&", kind: OpKind::Bitwise, precedence: prec::BIT_AND, right_assoc: false, spec_ref: "F0402#S2-P8" },
    OpSpec { form: "^", kind: OpKind::Bitwise, precedence: prec::BIT_XOR, right_assoc: false, spec_ref: "F0402#S2-P9" },
    OpSpec { form: "|", kind: OpKind::Bitwise, precedence: prec::BIT_OR, right_assoc: false, spec_ref: "F0402#S2-P10" },
    // 逻辑（P11-P12）
    OpSpec { form: "&&", kind: OpKind::Logical, precedence: prec::LOGIC_AND, right_assoc: false, spec_ref: "F0402#S2-P11" },
    OpSpec { form: "||", kind: OpKind::Logical, precedence: prec::LOGIC_OR, right_assoc: false, spec_ref: "F0402#S2-P12" },
    // 赋值族（P13，全族右结合）
    OpSpec { form: "=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "+=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "-=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "*=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "/=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "%=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "&=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "|=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "^=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: "<<=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
    OpSpec { form: ">>=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
];

/// 语境标记：同一形态在一元/二元语境的歧义登记键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpContext {
    /// 前缀位置（一元语境）
    Prefix,
    /// 中缀位置（二元语境）
    Infix,
}

/// 查表 O(1)：形态 → 登记（match 逐形态直派，返回值直接构造、不依赖
/// 总表下标——总表是遍历/对拍口径，查表是热路径口径，两口径由机检对齐）。
pub fn lookup(form: &str) -> Option<OpSpec> {
    // 一元/二元复用形态（+ - & | ^ *）：登记同一行——优先级/结合性以中缀
    // 口径为准，一元语境按 UNARY 级处理（语境裁定见歧义登记表）。
    let s = match form {
        "." => OpSpec { form: ".", kind: OpKind::Member, precedence: prec::MEMBER, right_assoc: false, spec_ref: "F0402#S2-P1" },
        "!" => OpSpec { form: "!", kind: OpKind::Logical, precedence: prec::UNARY, right_assoc: true, spec_ref: "F0402#S2-P2" },
        "~" => OpSpec { form: "~", kind: OpKind::Bitwise, precedence: prec::UNARY, right_assoc: true, spec_ref: "F0402#S2-P2" },
        "*" => OpSpec { form: "*", kind: OpKind::Arith, precedence: prec::MUL, right_assoc: false, spec_ref: "F0402#S2-P3" },
        "/" => OpSpec { form: "/", kind: OpKind::Arith, precedence: prec::MUL, right_assoc: false, spec_ref: "F0402#S2-P3" },
        "%" => OpSpec { form: "%", kind: OpKind::Arith, precedence: prec::MUL, right_assoc: false, spec_ref: "F0402#S2-P3" },
        "+" => OpSpec { form: "+", kind: OpKind::Arith, precedence: prec::ADD, right_assoc: false, spec_ref: "F0402#S2-P4" },
        "-" => OpSpec { form: "-", kind: OpKind::Arith, precedence: prec::ADD, right_assoc: false, spec_ref: "F0402#S2-P4" },
        "<<" => OpSpec { form: "<<", kind: OpKind::Bitwise, precedence: prec::SHIFT, right_assoc: false, spec_ref: "F0402#S2-P5" },
        ">>" => OpSpec { form: ">>", kind: OpKind::Bitwise, precedence: prec::SHIFT, right_assoc: false, spec_ref: "F0402#S2-P5" },
        "<=" => OpSpec { form: "<=", kind: OpKind::Relational, precedence: prec::REL, right_assoc: false, spec_ref: "F0402#S2-P6" },
        ">=" => OpSpec { form: ">=", kind: OpKind::Relational, precedence: prec::REL, right_assoc: false, spec_ref: "F0402#S2-P6" },
        "<" => OpSpec { form: "<", kind: OpKind::Relational, precedence: prec::REL, right_assoc: false, spec_ref: "F0402#S2-P6" },
        ">" => OpSpec { form: ">", kind: OpKind::Relational, precedence: prec::REL, right_assoc: false, spec_ref: "F0402#S2-P6" },
        "==" => OpSpec { form: "==", kind: OpKind::Relational, precedence: prec::EQ, right_assoc: false, spec_ref: "F0402#S2-P7" },
        "!=" => OpSpec { form: "!=", kind: OpKind::Relational, precedence: prec::EQ, right_assoc: false, spec_ref: "F0402#S2-P7" },
        "&" => OpSpec { form: "&", kind: OpKind::Bitwise, precedence: prec::BIT_AND, right_assoc: false, spec_ref: "F0402#S2-P8" },
        "^" => OpSpec { form: "^", kind: OpKind::Bitwise, precedence: prec::BIT_XOR, right_assoc: false, spec_ref: "F0402#S2-P9" },
        "|" => OpSpec { form: "|", kind: OpKind::Bitwise, precedence: prec::BIT_OR, right_assoc: false, spec_ref: "F0402#S2-P10" },
        "&&" => OpSpec { form: "&&", kind: OpKind::Logical, precedence: prec::LOGIC_AND, right_assoc: false, spec_ref: "F0402#S2-P11" },
        "||" => OpSpec { form: "||", kind: OpKind::Logical, precedence: prec::LOGIC_OR, right_assoc: false, spec_ref: "F0402#S2-P12" },
        "=" => OpSpec { form: "=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "+=" => OpSpec { form: "+=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "-=" => OpSpec { form: "-=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "*=" => OpSpec { form: "*=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "/=" => OpSpec { form: "/=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "%=" => OpSpec { form: "%=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "&=" => OpSpec { form: "&=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "|=" => OpSpec { form: "|=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "^=" => OpSpec { form: "^=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        "<<=" => OpSpec { form: "<<=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        ">>=" => OpSpec { form: ">>=", kind: OpKind::Assign, precedence: prec::ASSIGN, right_assoc: true, spec_ref: "F0402#S2-P13" },
        _ => return None,
    };
    Some(s)
}

// ---------------------------------------------------------------------------
// 二、最长匹配扫描
// ---------------------------------------------------------------------------

/// 匹配产物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperatorToken {
    /// 命中的形态（如 "<<="）
    pub form: &'static str,
    pub kind: OpKind,
    pub precedence: u8,
    pub right_assoc: bool,
    /// 消耗的字节数
    pub len: usize,
}

/// 匹配错误（未知符号序列：三要素 + 最近合法建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpError {
    pub code: &'static str,
    pub pos: usize,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl OpError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 最长优先匹配（判据：匹配 O(1) 最长优先）——从 pos 起，按 3/2/1 字节
/// 三层定长尝试，命中即返回（长形态优先于短形态）。
pub fn match_operator(input: &[u8], pos: usize) -> Result<Option<OperatorToken>, OpError> {
    if pos >= input.len() {
        return Ok(None);
    }
    for len in (1..=3).rev() {
        if pos + len > input.len() {
            continue;
        }
        // 边界防护：候选片段必须是 ASCII（运算符形态全 ASCII）
        let cand = &input[pos..pos + len];
        if cand.iter().any(|&b| !b.is_ascii()) {
            continue;
        }
        let s = core::str::from_utf8(cand).map_err(|_| {
            OpError {
                code: "E_OP_NON_ASCII",
                pos,
                what: "运算符位置出现非 ASCII 字节".to_string(),
                why: "运算符形态全集为 ASCII——非 ASCII 字节不属于运算符词法".to_string(),
                next: "核对源码编码（上游 F0415 源编码）".to_string(),
            }
        })?;
        if let Some(spec) = lookup(s) {
            // 长形态优先命中即返回——但两字符候选是单字符运算符 + 非运算符
            // 字符时（如 "+,"）不许贪吃：命中长度 = 形态长度即真匹配
            return Ok(Some(OperatorToken {
                form: spec.form,
                kind: spec.kind,
                precedence: spec.precedence,
                right_assoc: spec.right_assoc,
                len,
            }));
        }
    }
    // 全层未命中：未知符号序列 → 最近合法建议
    let ch = input[pos] as char;
    let near = suggest_near(input[pos]);
    Err(OpError {
        code: "E_OP_UNKNOWN",
        pos,
        what: format!("第 {} 字节的 {:?} 不是合法运算符", pos, ch),
        why: "未知符号序列在词法期拒绝——不猜写的人想要什么运算".to_string(),
        next: if near.is_empty() {
            format!("合法运算符形态全集见运算符总表（{} 条登记）", OPERATORS.len())
        } else {
            format!("最近合法运算符候选：{}", near.join(" "))
        },
    })
}

/// 最近合法运算符建议：同首字符候选优先（判据：报错带最近合法建议）。
pub fn suggest_near(lead: u8) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for spec in OPERATORS {
        if spec.form.as_bytes()[0] == lead && !out.contains(&spec.form) {
            out.push(spec.form);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 三、歧义消解登记（判据：歧义登记 + 语境裁定）
// ---------------------------------------------------------------------------

/// 裁定阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjudicationPhase {
    /// 词法期固定（无歧义）
    LexicalFixed,
    /// 语法期语境裁定（词法期给默认形态，语法期按语境改判）
    SyntaxContext,
}

/// 一条歧义登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbiguityEntry {
    /// 歧义符号（词法形态）
    pub symbol: &'static str,
    /// 歧义描述（人话）
    pub desc: &'static str,
    /// 词法期默认裁定
    pub lexical_default: &'static str,
    /// 裁定阶段
    pub phase: AdjudicationPhase,
    /// 语境键（语法期改判的语境）
    pub context: OpContext,
}

/// 歧义登记表（判据：歧义未登记 = 缺陷；语法期发现未登记歧义须回填）。
pub const AMBIGUITIES: &[AmbiguityEntry] = &[
    AmbiguityEntry {
        symbol: "<",
        desc: "小于号 vs 模板实参列表开符",
        lexical_default: "关系小于",
        phase: AdjudicationPhase::SyntaxContext,
        context: OpContext::Infix,
    },
    AmbiguityEntry {
        symbol: ">",
        desc: "大于号 vs 模板实参列表闭符",
        lexical_default: "关系大于",
        phase: AdjudicationPhase::SyntaxContext,
        context: OpContext::Infix,
    },
    AmbiguityEntry {
        symbol: ">>",
        desc: "右移 vs 嵌套泛型连续闭符 > >",
        lexical_default: "右移",
        phase: AdjudicationPhase::SyntaxContext,
        context: OpContext::Infix,
    },
    AmbiguityEntry {
        symbol: "-",
        desc: "二元减法 vs 一元负号",
        lexical_default: "二元减法",
        phase: AdjudicationPhase::SyntaxContext,
        context: OpContext::Prefix,
    },
    AmbiguityEntry {
        symbol: "+",
        desc: "二元加法 vs 一元正号",
        lexical_default: "二元加法",
        phase: AdjudicationPhase::SyntaxContext,
        context: OpContext::Prefix,
    },
    AmbiguityEntry {
        symbol: "*",
        desc: "乘法 vs 解引用",
        lexical_default: "乘法",
        phase: AdjudicationPhase::SyntaxContext,
        context: OpContext::Prefix,
    },
    AmbiguityEntry {
        symbol: "&",
        desc: "位与 vs 取址",
        lexical_default: "位与",
        phase: AdjudicationPhase::SyntaxContext,
        context: OpContext::Prefix,
    },
];

/// 歧义查询 O(1)（符号 → 该符号全部登记；判据：登记 O(1)）。
pub fn ambiguity_lookup(symbol: &str) -> Vec<AmbiguityEntry> {
    AMBIGUITIES
        .iter()
        .filter(|e| e.symbol == symbol)
        .copied()
        .collect()
}

/// 语法期回填登记的载体（运行期登记：语法期发现新歧义 → 回填入册）。
pub struct AmbiguityLedger {
    backfilled: Vec<AmbiguityEntry>,
}

impl AmbiguityLedger {
    pub fn new() -> AmbiguityLedger {
        AmbiguityLedger {
            backfilled: Vec::new(),
        }
    }

    /// 语法期发现未登记歧义 → 回填登记（判据：歧义未登记→发现后回填）。
    pub fn backfill(&mut self, entry: AmbiguityEntry) -> Result<(), OpError> {
        // 防重：同符号同语境同描述不许重复登记
        if self.backfilled.iter().any(|e| {
            e.symbol == entry.symbol && e.context == entry.context && e.desc == entry.desc
        }) {
            return Err(OpError {
                code: "E_OP_DUP_REG",
                pos: 0,
                what: format!("歧义 {}({:?}) 重复登记", entry.symbol, entry.context),
                why: "同一歧义的重复登记说明两处裁定不一致——登记表只认一份事实"
                    .to_string(),
                next: "合并两条登记或修正其中一处语境键".to_string(),
            });
        }
        self.backfilled.push(entry);
        Ok(())
    }

    pub fn backfilled(&self) -> &[AmbiguityEntry] {
        &self.backfilled
    }
}

// ---------------------------------------------------------------------------
// 四、总表对齐机检（判据：表与规范冲突→对齐修正）
// ---------------------------------------------------------------------------

/// 总表不变量机检：形态唯一 / 级别连续非空 / 右结合仅赋值与一元前缀 /
/// 节号锚齐全 / 查表与扫描表一致。返回违例清单（空 = 对齐通过）。
pub fn table_alignment_check() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    // 1) 形态唯一（同形态允许一元/二元共用一行登记——本表按中缀口径单行）
    for (i, a) in OPERATORS.iter().enumerate() {
        for b in OPERATORS.iter().skip(i + 1) {
            if a.form == b.form {
                v.push(format!("形态 {} 重复登记", a.form));
            }
        }
    }
    // 2) 级别连续非空（1..=13 每级至少一条）
    for p in 1..=13u8 {
        if !OPERATORS.iter().any(|s| s.precedence == p) {
            v.push(format!("优先级 P{} 无任何登记", p));
        }
    }
    // 3) 右结合仅赋值族与一元前缀
    for s in OPERATORS {
        if s.right_assoc && s.precedence != prec::ASSIGN && s.precedence != prec::UNARY {
            v.push(format!("形态 {} 右结合但级别 P{} 不在允许集", s.form, s.precedence));
        }
    }
    // 4) 节号锚齐全（每条都有 F0402 锚）
    for s in OPERATORS {
        if !s.spec_ref.starts_with("F0402#S2-P") {
            v.push(format!("形态 {} 缺规范节号锚", s.form));
        }
    }
    // 5) 查表与总表一致（lookup 漏登记 = 表分裂——对拍机检）
    for s in OPERATORS {
        match lookup(s.form) {
            Some(got) if got.form == s.form && got.precedence == s.precedence => {}
            _ => v.push(format!("查表与总表不一致：{}", s.form)),
        }
    }
    v
}

/// F0409 判据自检（域聚合入口）。
pub fn run_vec09_checks() -> crate::checks::CheckSet {
    super::vec09_checks::run_vec09_checks()
}
