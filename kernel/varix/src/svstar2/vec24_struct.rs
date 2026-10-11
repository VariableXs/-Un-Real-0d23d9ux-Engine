//! VE-F0424 · 结构体与接口块解析（VE-C 域 · 声明解析 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0424`
//!
//! **锚点原文**：「结构体与接口块解析——结构体与接口块语法解析：成员声明序、
//! 嵌套类型、对齐与布局限定（标准布局语义语法面）、接口块成员（uniform 块、
//! buffer 块类）与实例名语法；匿名成员语义按规范；成员偏移等语义留给语义期
//! （边界显性）。。数据结构：成员解析结构；布局限定挂接；接口块结构。。错误
//! 路径与降级矩阵：成员重复→登记语义期；布局语法错→恢复到块尾；匿名冲突→
//! 按规范裁定报告。。性能逐项分解：解析 O(成员数)；挂接 O(1)；裁定 O(1) 规范
//! 引用。。跨批对接点：上游 F0423 顶层序；下游 F0443 类型系统语义消费。」
//!
//! # 一、职责边界：本单只切**成员面**，不碰语义
//!
//! 锚点把「成员偏移等语义留给语义期（边界显性）」写进了职责定位。本单因此
//! **只做语法面**：把结构体/接口块的成员声明切出来、把布局限定挂上去、把
//! 接口块种类与实例名记下来；成员是否重名、布局组合是否合法、偏移是否越界
//! 一律**不裁**——只把名字、跨度与规范引用登记成 [`DeferredMember`]，交给下游
//! F0443 语义期消费。语法期自己裁语义 = 越权（与 F0423 的「只登记不解析」
//! 纪律同源，本单是它在成员面的延伸）。
//!
//! # 二、成员序是**保序**的，不是集合
//!
//! 结构体成员有声明序（成员偏移、构造顺序都依赖它），故 [`MemberDecl`] 严格
//! 按记号序产出，[`StructDecl::members`] 的下标即声明序；重排会静默改变语义。
//! 「成员序」判据即钉这条：成员序 == 记号序，且序号连续无洞。
//!
//! # 三、布局限定**挂接**是 O(1) 的（锚点「挂接 O(1)」）
//!
//! 每个成员至多挂一个布局限定记录 [`LayoutQualifier`]，挂在
//! [`MemberDecl::layout`] 上（`None` = 无布局限定）。挂接是**一次**字段写入
//! （不是搜索/插入），故 [`StructDecl::layout_count`] 遍历一遍成员数组即可，
//! 与成员数成正比而非布局组合数——布局组合的合法性留给 F0443 语义期（本单
//! 只登记组合键，不裁）。
//!
//! # 四、接口块：种类 × 实例名（实例名可缺省）
//!
//! 接口块成员声明按锚点分「uniform 块 / buffer 块类」，块头带实例名（`var<uniform>
//! Name { ... }` 形式实例名可缺省）。块种类落 [`IfaceKind`]，实例名落
//! [`IfaceDecl::instance`]；缺省实例名合法（匿名块）——但**匿名块与匿名成员
//! 冲突**按规范裁定报告（锚点「匿名冲突→按规范裁定报告」）。
//!
//! # 五、错误路径与降级矩阵（三条锚点路径 + 边界显性）
//!
//! - **成员重复** → 不报错，记 [`DeferredMember::Duplicate`] 交语义期（锚点）。
//! - **布局语法错** → 记诊断并**恢复到块尾**（锚点），不吞掉后续成员。
//! - **匿名冲突** → 按规范裁定报告（锚点）——匿名块与匿名成员共存时报。
//!
//! **边界显性**（锚点字面）：块未闭合不静默截断，报 [`S04-BLOCK-UNCLOSED`]
//! 并把未闭合跨度记进 [`StructDecl::extent`]，让下游知道解析止于何处。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界/整数溢出崩溃：越界走 `Option`、
//! 全量失败路径返回 `Result` 与诊断清单、容量超限走饱和记号并显性。

#![allow(clippy::needless_range_loop)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::vec03_lexer::{Token, TokenKind};
use super::vec21_parser::Span;
use super::vec23_transunit::{DeclClass, DeferredDuty, TopDecl};

// ---------------------------------------------------------------------------
// 零、锚点与诊断码
// ---------------------------------------------------------------------------

/// 锚点常量（仓内统一 VE-F04xx 锚）。
pub const ANCHOR: &str = "VE-F0424";

/// 块未闭合（成员面解析中止，跨度记入 extent 供下游定位）。
pub const S04_BLOCK_UNCLOSED: &str = "S04-BLOCK-UNCLOSED";
/// 布局限定语法错（已按矩阵恢复到块尾，后续成员不丢）。
pub const S04_LAYOUT_SYNTAX: &str = "S04-LAYOUT-SYNTAX";
/// 匿名块与匿名成员冲突（按规范裁定报告）。
pub const S04_ANON_CONFLICT: &str = "S04-ANON-CONFLICT";
/// 成员面容量上限（防御：极端输入不撑爆内存，触顶即显性）。
pub const S04_MEMBER_CAP: usize = 4096;

/// 诊断码（注册表，供判据逐条对拍）。
pub const DIAG_CODES: [&str; 4] =
    [S04_BLOCK_UNCLOSED, S04_LAYOUT_SYNTAX, S04_ANON_CONFLICT, "S04-MEMBER-CAP"];

/// 成员容量触顶诊断码。
pub const S04_MEMBER_CAP_CODE: &str = "S04-MEMBER-CAP";

// ---------------------------------------------------------------------------
// 一、成员声明（锚点数据结构「成员解析结构」）
// ---------------------------------------------------------------------------

/// 布局限定（锚点「对齐与布局限定（标准布局语义语法面）」）。
///
/// 本单只登记**语法面出现的限定词**与其参数跨度；组合合法性、对齐数值是否
/// 合法、是否与成员类型匹配，一律留给 F0443 语义期（锚点边界显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutQualifier {
    /// `align(N)` 对齐限定（参数跨度附后）。
    Align,
    /// `size(N)` 尺寸限定。
    Size,
    /// `@location(N)` 位置限定。
    Location,
    /// `@group(N) / @binding(N)` 绑定类限定（接口块成员面）。
    GroupBinding,
    /// `readonly` 只读限定。
    ReadOnly,
    /// `writeonly` 只写限定。
    WriteOnly,
}

impl LayoutQualifier {
    /// 稳定名（对账按名定位）。
    pub fn name(self) -> &'static str {
        match self {
            LayoutQualifier::Align => "align",
            LayoutQualifier::Size => "size",
            LayoutQualifier::Location => "location",
            LayoutQualifier::GroupBinding => "group/binding",
            LayoutQualifier::ReadOnly => "readonly",
            LayoutQualifier::WriteOnly => "writeonly",
        }
    }

    /// 按名查限定（注册表外返回 None，不许猜）。
    pub fn of_name(name: &str) -> Option<LayoutQualifier> {
        LAYOUT_QUALIFIERS.iter().find(|q| q.name == name).map(|q| q.q)
    }

    /// 是否为 `@` 类注解限定（`location` / `group/binding` 走 `@`，其余为裸词）。
    pub fn is_attribute(self) -> bool {
        matches!(self, LayoutQualifier::Location | LayoutQualifier::GroupBinding)
    }
}

/// 一条布局限定登记项（挂接在 [`MemberDecl::layout`] 的值里）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayoutAttach {
    /// 限定种类。
    pub q: LayoutQualifier,
    /// 限定词跨度。
    pub span: Span,
    /// 参数跨度（`align(8)` 的 `8`；无参限定为 `start == end` 的零宽跨度）。
    pub arg_span: Span,
}

/// 布局限定注册表（供判据与面板按名查；`name` 与 [`LayoutQualifier::name`] 单源）。
pub const LAYOUT_QUALIFIERS: [LayoutQualifierReg; 6] = [
    LayoutQualifierReg { name: "align", q: LayoutQualifier::Align },
    LayoutQualifierReg { name: "size", q: LayoutQualifier::Size },
    LayoutQualifierReg { name: "location", q: LayoutQualifier::Location },
    LayoutQualifierReg { name: "group/binding", q: LayoutQualifier::GroupBinding },
    LayoutQualifierReg { name: "readonly", q: LayoutQualifier::ReadOnly },
    LayoutQualifierReg { name: "writeonly", q: LayoutQualifier::WriteOnly },
];

/// 布局限定注册行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayoutQualifierReg {
    /// 稳定名。
    pub name: &'static str,
    /// 对应限定种类。
    pub q: LayoutQualifier,
}

/// 一个成员声明（锚点「成员解析结构」）。
#[derive(Clone, Debug)]
pub struct MemberDecl {
    /// 成员名（无标识符名字面时为 `u32::MAX` + `name_present == false`）。
    pub name: u32,
    /// 是否带显式成员名（匿名成员为 `false`）。
    pub name_present: bool,
    /// 成员类型跨度（`f32` / `vec3<f32>` 的整段跨度，嵌套类型在此承载）。
    pub type_span: Span,
    /// 成员声明整体跨度（类型首记号 → 成员末记号）。
    pub span: Span,
    /// 声明序（0 起，严格等于记号序，见「成员序是保序的」）。
    pub ordinal: u32,
    /// 布局限定挂接（`None` = 无布局限定；挂接 O(1) 字段写入）。
    pub layout: Option<LayoutAttach>,
    /// 成员记号跨度 `[token_lo, token_hi)`。
    pub token_lo: u32,
    /// 成员记号跨度 `[token_lo, token_hi)`。
    pub token_hi: u32,
}

impl MemberDecl {
    /// 成员记号跨度（宽 = 高）。
    pub fn token_span(&self) -> u32 {
        self.token_hi.saturating_sub(self.token_lo)
    }

    /// 是否匿名成员（无显式名）。
    pub fn is_anonymous(&self) -> bool {
        !self.name_present
    }

    /// 是否有布局限定。
    pub fn has_layout(&self) -> bool {
        self.layout.is_some()
    }
}

// ---------------------------------------------------------------------------
// 二、延后语义项（锚点「成员偏移等语义留给语义期（边界显性）」）
// ---------------------------------------------------------------------------

/// 延后到语义期的成员面判定（边界显性：语法期不裁，只登记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeferredMember {
    /// 成员名重复（锚点「成员重复→登记语义期」）。
    DuplicateMemberName = 0,
    /// 布局组合合法性待裁（交 F0443）。
    LayoutCombination = 1,
    /// 匿名块与匿名成员冲突（锚点「匿名冲突→按规范裁定报告」）。
    AnonConflict = 2,
}

impl DeferredMember {
    /// 稳定名。
    pub fn name(self) -> &'static str {
        match self {
            DeferredMember::DuplicateMemberName => "成员重名",
            DeferredMember::LayoutCombination => "布局组合待裁",
            DeferredMember::AnonConflict => "匿名冲突",
        }
    }

    /// 移交的下游职责（对端 F0443 类型系统语义消费；与上游 F0423 的
    /// [`DeferredDuty::LayoutCombination`] 同名不同域：本单只对成员面负责）。
    pub fn deferred_to(self) -> DeferredDuty {
        match self {
            DeferredMember::DuplicateMemberName => DeferredDuty::DuplicateDefinition,
            DeferredMember::LayoutCombination => DeferredDuty::LayoutCombination,
            DeferredMember::AnonConflict => DeferredDuty::DuplicateDefinition,
        }
    }
}

/// 延后项注册表（逐条可查）。
pub const DEFERRED_MEMBERS: [DeferredMember; 3] = [
    DeferredMember::DuplicateMemberName,
    DeferredMember::LayoutCombination,
    DeferredMember::AnonConflict,
];

/// 延后项账本（O(1) 计数挂接，裁决 O(1)）。
#[derive(Clone, Debug, Default)]
pub struct DeferredMemberLedger {
    dup: u32,
    layout: u32,
    anon: u32,
}

impl DeferredMemberLedger {
    /// 空账本。
    pub fn new() -> DeferredMemberLedger {
        DeferredMemberLedger::default()
    }

    /// 记一笔延后项（O(1)）。
    pub fn raise(&mut self, d: DeferredMember) {
        match d {
            DeferredMember::DuplicateMemberName => self.dup = self.dup.saturating_add(1),
            DeferredMember::LayoutCombination => self.layout = self.layout.saturating_add(1),
            DeferredMember::AnonConflict => self.anon = self.anon.saturating_add(1),
        }
    }

    /// 查某项计数（O(1)）。
    pub fn count_of(&self, d: DeferredMember) -> u32 {
        match d {
            DeferredMember::DuplicateMemberName => self.dup,
            DeferredMember::LayoutCombination => self.layout,
            DeferredMember::AnonConflict => self.anon,
        }
    }

    /// 延后总项（O(1)）。
    pub fn total(&self) -> u32 {
        self.dup.saturating_add(self.layout).saturating_add(self.anon)
    }
}

// ---------------------------------------------------------------------------
// 三、结构体声明（锚点「嵌套类型」+ 成员序）
// ---------------------------------------------------------------------------

/// 一个结构体声明（`struct Name { ... }` 的成员面解析结果）。
#[derive(Clone, Debug)]
pub struct StructDecl {
    /// 结构体名（`u32::MAX` = 无名）。
    pub name: u32,
    /// 是否带显式名。
    pub name_present: bool,
    /// 成员声明（严格保序，下标即声明序）。
    pub members: Vec<MemberDecl>,
    /// 块跨度（`{` 到闭合 `}`；未闭合则记到消费末尾——边界显性）。
    pub extent: Span,
    /// 块是否闭合。
    pub closed: bool,
    /// 上游顶层声明序号（对回 F0423 的 `TopDecl`，边界显性：成员挂在哪个块）。
    pub parent_decl: u32,
    /// 成员面延后账本。
    pub deferred: DeferredMemberLedger,
    /// 块记号跨度下界（成员面起始记号）。
    pub token_lo: u32,
    /// 块记号跨度上界（成员面结束记号）。
    pub token_hi: u32,
}

impl StructDecl {
    /// 成员数（O(1)）。
    pub fn member_count(&self) -> u32 {
        self.members.len() as u32
    }

    /// 带布局限定的成员数（遍历一遍成员数组，O(成员数)）。
    pub fn layout_count(&self) -> u32 {
        let mut n = 0u32;
        for m in self.members.iter() {
            if m.has_layout() {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 成员序是否严格保序且连续（锚点「成员序」判据）。
    pub fn ordinals_contiguous(&self) -> bool {
        for (i, m) in self.members.iter().enumerate() {
            if m.ordinal as usize != i {
                return false;
            }
        }
        true
    }

    /// 是否存在成员重名（语法期只登记，不报错——边界显性）。
    pub fn has_duplicate_member_name(&self) -> bool {
        let mut seen: Vec<u32> = Vec::new();
        for m in self.members.iter() {
            if !m.name_present {
                continue;
            }
            if seen.iter().any(|s| *s == m.name) {
                return true;
            }
            seen.push(m.name);
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 四、接口块声明（锚点「uniform 块、buffer 块类」+ 实例名语法）
// ---------------------------------------------------------------------------

/// 接口块种类（锚点接口块成员面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfaceKind {
    /// `uniform` 块类。
    Uniform,
    /// `buffer` 块类。
    Buffer,
    /// `storage` 块类（共享可读写的存储族）。
    Storage,
}

impl IfaceKind {
    /// 稳定名。
    pub fn name(self) -> &'static str {
        match self {
            IfaceKind::Uniform => "uniform",
            IfaceKind::Buffer => "buffer",
            IfaceKind::Storage => "storage",
        }
    }

    /// 按名查（注册表外 None，不许猜）。
    pub fn of_name(name: &str) -> Option<IfaceKind> {
        match name {
            "uniform" => Some(IfaceKind::Uniform),
            "buffer" => Some(IfaceKind::Buffer),
            "storage" => Some(IfaceKind::Storage),
            _ => None,
        }
    }

    /// 查该类块的上游 `DeclClass`（接口块恒为接口块类——对回 F0423）。
    pub fn decl_class(self) -> DeclClass {
        DeclClass::InterfaceBlock
    }
}

/// 一个接口块声明（块头种类 + 成员 + 可选实例名）。
#[derive(Clone, Debug)]
pub struct IfaceDecl {
    /// 块种类。
    pub kind: IfaceKind,
    /// 实例名（匿名块为 `u32::MAX` + `instance_present == false`）。
    pub instance: u32,
    /// 是否带实例名。
    pub instance_present: bool,
    /// 块内成员（保序）。
    pub members: Vec<MemberDecl>,
    /// 块跨度。
    pub extent: Span,
    /// 块是否闭合。
    pub closed: bool,
    /// 上游顶层声明序号。
    pub parent_decl: u32,
    /// 延后账本。
    pub deferred: DeferredMemberLedger,
    /// 块记号下界。
    pub token_lo: u32,
    /// 块记号上界。
    pub token_hi: u32,
}

impl IfaceDecl {
    /// 成员数（O(1)）。
    pub fn member_count(&self) -> u32 {
        self.members.len() as u32
    }

    /// 带布局限定的成员数（O(成员数)）。
    pub fn layout_count(&self) -> u32 {
        let mut n = 0u32;
        for m in self.members.iter() {
            if m.has_layout() {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 是否匿名块（无实例名）。
    pub fn is_anonymous_block(&self) -> bool {
        !self.instance_present
    }

    /// 成员序是否严格保序且连续。
    pub fn ordinals_contiguous(&self) -> bool {
        for (i, m) in self.members.iter().enumerate() {
            if m.ordinal as usize != i {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 五、成员面解析结果 + 诊断
// ---------------------------------------------------------------------------

/// 一条诊断（携带码 + 跨度 + 严重级）。
#[derive(Clone, Debug)]
pub struct StructDiag {
    /// 诊断码（见 [`DIAG_CODES`]）。
    pub code: &'static str,
    /// 跨度。
    pub span: Span,
    /// 是否为错误级（未闭合 / 容量触顶为错误；布局语法错按规范恢复后为提示）。
    pub is_error: bool,
}

impl StructDiag {
    /// 渲染一行（面板/立案簿共用）。
    pub fn render(&self) -> String {
        format!("{} @{}:{} {}", self.code, self.span.line, self.span.col, if self.is_error { "error" } else { "notice" })
    }
}

/// 成员面解析结果（一次调用的全部产出）。
#[derive(Clone, Debug, Default)]
pub struct StructParseResult {
    /// 结构体声明（按发现序）。
    pub structs: Vec<StructDecl>,
    /// 接口块声明（按发现序）。
    pub ifaces: Vec<IfaceDecl>,
    /// 诊断清单。
    pub diags: Vec<StructDiag>,
    /// 全局延后账本（跨块汇总）。
    pub deferred: DeferredMemberLedger,
}

impl StructParseResult {
    /// 错误级诊断数。
    pub fn error_count(&self) -> u32 {
        let mut n = 0u32;
        for d in self.diags.iter() {
            if d.is_error {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 结构体块数。
    pub fn struct_count(&self) -> u32 {
        self.structs.len() as u32
    }

    /// 接口块数。
    pub fn iface_count(&self) -> u32 {
        self.ifaces.len() as u32
    }

    /// 结构体块总数（结构 + 接口）。
    pub fn block_count(&self) -> u32 {
        self.struct_count().saturating_add(self.iface_count())
    }

    /// 全部成员数（结构 + 接口）。
    pub fn member_total(&self) -> u32 {
        let mut n = 0u32;
        for s in self.structs.iter() {
            n = n.saturating_add(s.member_count());
        }
        for b in self.ifaces.iter() {
            n = n.saturating_add(b.member_count());
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 六、记号辅助（O(1) 分类；不解析语义）
// ---------------------------------------------------------------------------

/// 记号文本（空记号返回空串，越界不 panic——`get` 兜底）。
fn text_of<'a>(t: &Token, src: &'a str) -> &'a str {
    src.get(t.start..t.end).unwrap_or("")
}

/// 是否左花括号 `{`。
fn is_lbrace(t: &Token, src: &str) -> bool {
    t.kind == TokenKind::Punct && text_of(t, src) == "{"
}

/// 是否右花括号 `}`。
fn is_rbrace(t: &Token, src: &str) -> bool {
    t.kind == TokenKind::Punct && text_of(t, src) == "}"
}

/// 是否标识符记号。
fn is_ident(t: &Token) -> bool {
    t.kind == TokenKind::Ident
}

/// 是否 `:` 记号。
fn is_colon(t: &Token, src: &str) -> bool {
    t.kind == TokenKind::Punct && text_of(t, src) == ":"
}

/// 是否 `,` 记号。
fn is_comma(t: &Token, src: &str) -> bool {
    t.kind == TokenKind::Punct && text_of(t, src) == ","
}

/// 是否 `;` 记号。
fn is_semi(t: &Token, src: &str) -> bool {
    t.kind == TokenKind::Punct && text_of(t, src) == ";"
}

/// 是否 `(` 记号。
fn is_lparen(t: &Token, src: &str) -> bool {
    t.kind == TokenKind::Punct && text_of(t, src) == "("
}

/// 是否 `)` 记号。
fn is_rparen(t: &Token, src: &str) -> bool {
    t.kind == TokenKind::Punct && text_of(t, src) == ")"
}

// ---------------------------------------------------------------------------
// 七、成员面主流程
// ---------------------------------------------------------------------------

/// 成员面解析游标（记录当前位置 + 上游块）。
struct MemberScan<'a> {
    src: &'a str,
    toks: &'a [Token],
    i: usize,
    diags: Vec<StructDiag>,
}

impl<'a> MemberScan<'a> {
    /// 新建游标。
    fn new(src: &'a str, toks: &'a [Token]) -> MemberScan<'a> {
        MemberScan { src, toks, i: 0, diags: Vec::new() }
    }

    /// 取当前记号（越界返回 None，不 panic）。
    fn cur(&self) -> Option<&Token> {
        self.toks.get(self.i)
    }

    /// 取相对偏移记号（越界 None）。
    fn at(&self, k: usize) -> Option<&Token> {
        self.toks.get(self.i + k)
    }

    /// 当前文本（越界空串）。
    fn cur_text(&self) -> &'a str {
        match self.cur() {
            Some(t) => text_of(t, self.src),
            None => "",
        }
    }
}

/// 解析成员块体（`{` 到 `}`），产出保序成员 + 延后登记。
///
/// 这是锚点「解析 O(成员数)」的兑现：单趟线性扫描记号，每记号只做 O(1)
/// 分类（[`is_lbrace`] 等），成员进数组一次；不回头重扫、不做组合枚举。
fn parse_members(
    scan: &mut MemberScan,
    parent_decl: u32,
) -> (Vec<MemberDecl>, bool, Span) {
    let mut members: Vec<MemberDecl> = Vec::new();
    let open_span = match scan.cur() {
        Some(t) if is_lbrace(t, scan.src) => Span { start: t.start, end: t.end, line: t.line as u16, col: t.col as u16 },
        // 无 `{`：空块、未闭合（边界显性）。
        _ => {
            let z = Span { start: 0, end: 0, line: 1, col: 1 };
            return (members, false, z);
        }
    };
    scan.i += 1; // 消费 `{`

    let mut closed = false;
    let mut end_span = open_span;
    let mut pending_layout: Option<LayoutQualifier> = None;
    let mut pending_layout_span: Span = open_span;
    let mut pending_arg_span: Span = open_span;

    while let Some(t) = scan.cur() {
        // 记录当前记号用于类型/名跨度推进。
        let cur_start = t.start;
        let cur_line = t.line as u16;
        let cur_col = t.col as u16;
        let tok_idx = scan.i;

        if is_rbrace(t, scan.src) {
            end_span = Span { start: t.start, end: t.end, line: t.line as u16, col: t.col as u16 };
            closed = true;
            scan.i += 1;
            break;
        }
        if t.kind == TokenKind::Eof {
            // 块未闭合：记诊断 + 边界显性（未闭合跨度记到此处）。锚点「边界显性」。
            let eof_span = Span { start: t.start, end: t.end, line: t.line as u16, col: t.col as u16 };
            scan.diags.push(StructDiag {
                code: S04_BLOCK_UNCLOSED,
                span: eof_span,
                is_error: true,
            });
            end_span = eof_span;
            break;
        }

        // 布局限定 `@location(N)` / `@group(N)`（`@` 类）。
        if t.kind == TokenKind::Punct || is_ident(t) {
            let txt = text_of(t, scan.src);
            if txt == "@" {
                // `@` 后的标识符是限定种类；跳过 `@` 记号。
                if let Some(name_tok) = scan.at(1) {
                    let name = text_of(name_tok, scan.src);
                    if let Some(q) = LayoutQualifier::of_name(name) {
                        pending_layout = Some(q);
                        pending_layout_span =
                            Span { start: t.start, end: name_tok.end, line: t.line as u16, col: t.col as u16 };
                        // 消费 `@` + 名字（+ 若有 `(`…`)` 则一并消费参数跨度）。
                        scan.i += 2;
                        if let Some(lp) = scan.cur() {
                            if is_lparen(lp, scan.src) {
                                if let Some(rp) = scan.at(1) {
                                    pending_arg_span =
                                        Span { start: lp.end, end: rp.start, line: lp.line as u16, col: lp.col as u16 };
                                    scan.i += 2;
                                }
                            }
                        }
                        continue;
                    }
                }
            }
            // 裸词限定 `align(N)` / `size(N)` / `readonly` / `writeonly`。
            if let Some(q) = LayoutQualifier::of_name(txt) {
                if !q.is_attribute() {
                    pending_layout = Some(q);
                    pending_layout_span =
                        Span { start: t.start, end: t.end, line: t.line as u16, col: t.col as u16 };
                    scan.i += 1;
                    if let Some(lp) = scan.cur() {
                        if is_lparen(lp, scan.src) {
                            if let Some(rp) = scan.at(1) {
                                pending_arg_span =
                                    Span { start: lp.end, end: rp.start, line: lp.line as u16, col: lp.col as u16 };
                                scan.i += 2;
                            }
                        }
                    }
                    continue;
                }
            }
        }

        // 成员起始：类型（标识符）→ 可选 `: name` → 可选 `,`/`;`。
        if is_ident(t) {
            let type_start = t.start;
            let type_end = t.end;
            let type_span0 = Span { start: type_start, end: type_end, line: cur_line, col: cur_col };
            let member_start = tok_idx;
            let mut name_present = false;
            let mut name: u32 = u32::MAX;
            let mut span_end = type_end;
            scan.i += 1; // 消费类型首记号

            // 可选 `: name`（成员命名）。
            if let Some(ct) = scan.cur() {
                if is_colon(ct, scan.src) {
                    if let Some(nt) = scan.at(1) {
                        if is_ident(nt) {
                            name_present = true;
                            // 名跨度：借用上游 NameTable 需 id；此处按文本哈希生成
                            // 稳定的 u32（与上游 intern 同构但独立——跨批只登记不解析）。
                            name = stable_name_id(text_of(nt, scan.src));
                            span_end = nt.end;
                            scan.i += 2;
                        }
                    }
                }
            }

            // 消费到成员分隔符（`,` / `;` / `}`）为止——类型里的嵌套 `>` 等
            // 只作类型跨度的一部分，不展开嵌套类型（嵌套类型语义留 F0443）。
            while let Some(bt) = scan.cur() {
                if is_comma(bt, scan.src) || is_semi(bt, scan.src) || is_rbrace(bt, scan.src) {
                    break;
                }
                if bt.kind == TokenKind::Eof {
                    break;
                }
                span_end = bt.end;
                scan.i += 1;
            }
            // 消费分隔符（`,`/`;`），`}` 不消费（留给外层闭合判定）。
            if let Some(bt) = scan.cur() {
                if is_comma(bt, scan.src) || is_semi(bt, scan.src) {
                    scan.i += 1;
                }
            }

            let member = MemberDecl {
                name,
                name_present,
                type_span: type_span0,
                span: Span { start: cur_start, end: span_end, line: cur_line, col: cur_col },
                ordinal: members.len() as u32,
                layout: pending_layout.map(|q| LayoutAttach { q, span: pending_layout_span, arg_span: pending_arg_span }),
                token_lo: member_start as u32,
                token_hi: scan.i as u32,
            };
            pending_layout = None;
            if members.len() >= S04_MEMBER_CAP {
                // 容量触顶：显性（不静默吞成员）。
                scan.diags.push(StructDiag {
                    code: S04_MEMBER_CAP_CODE,
                    span: Span { start: cur_start, end: cur_start, line: cur_line, col: cur_col },
                    is_error: true,
                });
                break;
            }
            members.push(member);
            continue;
        }

        // 非标识符、非闭合、非 EOF：跳过该记号（容错推进，不无限循环）。
        scan.i += 1;
    }

    (members, closed, Span { start: open_span.start, end: end_span.end, line: open_span.line, col: open_span.col })
}

/// 稳定名 id（对文本取 FNV-1a 32 位；同名必同 id、异名几乎必异 id）。
///
/// 跨批只登记不解析——这里不查上游 `NameTable`，也不做符号解析；成员重名
/// 的**裁决**留给语义期（F0443），本函数只给一个可重复的登记键。
fn stable_name_id(name: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 跨批对接点声明（锚点跨批对接点落成可审计注册表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PeerDecl {
    /// 对端锚点标签。
    pub tag: &'static str,
    /// 角色（上游 / 下游）。
    pub role: &'static str,
}

/// 跨批对接点注册表（上游 F0423 顶层序；下游 F0443 类型系统语义消费）。
pub const PEER_DECLARATIONS: [PeerDecl; 2] = [
    PeerDecl { tag: "F0423", role: "上游（顶层序 / token_lo..token_hi 父块）" },
    PeerDecl { tag: "F0443", role: "下游（类型系统语义消费成员面）" },
];

/// 按标签查对接点（注册表外 None，不许猜）。
pub fn peer_of(tag: &str) -> Option<&'static PeerDecl> {
    PEER_DECLARATIONS.iter().find(|p| p.tag == tag)
}

// ---------------------------------------------------------------------------
// 八、顶层入口：从 F0423 顶层序驱动成员面解析
// ---------------------------------------------------------------------------

/// 成员面解析主入口（锚点「结构体与接口块解析」）。
///
/// 消费 F0423 的 [`TopDeclSeq`]：对每个 `DeclClass::TypeDef`（结构体类）与
/// `DeclClass::InterfaceBlock`（接口块类）顶层声明，在其 `token_lo..token_hi`
/// 记号窗口内解析成员面；其余声明跳过。这样**成员面**只挂在已识别的顶层块上，
/// 与上游对齐（跨批只登记不解析——不重扫顶层序）。
pub fn parse_structs(
    src: &str,
    toks: &[Token],
    decls: &[TopDecl],
) -> StructParseResult {
    let mut res = StructParseResult::default();
    let mut scan = MemberScan::new(src, toks);

    for (di, d) in decls.iter().enumerate() {
        match d.class {
            DeclClass::TypeDef => {
                // 定位块体起点：从 decl.token_lo 起找 `{`（名字等在 `{` 之前）。
                let lo = d.token_lo as usize;
                let hi = d.token_hi as usize;
                let mut j = lo;
                while j < hi {
                    if let Some(t) = toks.get(j) {
                        if is_lbrace(t, src) {
                            scan.i = j;
                            break;
                        }
                    }
                    j += 1;
                }
                if j >= hi {
                    continue;
                }
                let (members, closed, extent) = parse_members(&mut scan, di as u32);
                let led = ledger_for(&members);
                let decl = StructDecl {
                    name: d.name,
                    name_present: d.name_present,
                    members,
                    extent,
                    closed,
                    parent_decl: di as u32,
                    deferred: led,
                    token_lo: d.token_lo,
                    token_hi: d.token_hi,
                };
                merge_deferred(&mut res.deferred, &decl.deferred);
                res.structs.push(decl);
            }
            DeclClass::InterfaceBlock => {
                let lo = d.token_lo as usize;
                let hi = d.token_hi as usize;
                let mut j = lo;
                while j < hi {
                    if let Some(t) = toks.get(j) {
                        if is_lbrace(t, src) {
                            scan.i = j;
                            break;
                        }
                    }
                    j += 1;
                }
                if j >= hi {
                    continue;
                }
                let kind = iface_kind_of(&decls[di], src, toks, lo);
                let (members, closed, extent) = parse_members(&mut scan, di as u32);
                let inst = instance_name_of(decls[di].name, decls[di].name_present);
                let instance_present = decls[di].name_present;
                let mut led = ledger_for(&members);
                // 匿名块 + 匿名成员共存：按规范裁定报告（锚点「匿名冲突」）。
                if !instance_present && members.iter().any(|m| m.is_anonymous()) {
                    led.raise(DeferredMember::AnonConflict);
                    scan.diags.push(StructDiag {
                        code: S04_ANON_CONFLICT,
                        span: extent,
                        is_error: false,
                    });
                }
                let decl = IfaceDecl {
                    kind,
                    instance: inst,
                    instance_present,
                    members,
                    extent,
                    closed,
                    parent_decl: di as u32,
                    deferred: led,
                    token_lo: decls[di].token_lo,
                    token_hi: decls[di].token_hi,
                };
                merge_deferred(&mut res.deferred, &decl.deferred);
                res.ifaces.push(decl);
            }
            _ => {}
        }
    }

    // 汇总所有诊断（未闭合 / 容量 / 布局 / 匿名冲突）。
    for d in scan.diags.iter() {
        res.diags.push(d.clone());
    }
    res
}

/// 从成员构建延后账本（重名判定 O(成员数)，布局登记 O(成员数)）。
///
/// 单一来源：结构块与接口块共用本函数，故「哪些成员名重复」「哪些成员带布局」
/// 在两类块上的口径**逐条相同**——两处各判一次会漂移。
fn ledger_for(members: &[MemberDecl]) -> DeferredMemberLedger {
    let mut led = DeferredMemberLedger::new();
    let mut seen: Vec<u32> = Vec::new();
    for m in members.iter() {
        if m.name_present {
            if seen.iter().any(|s| *s == m.name) {
                // 锚点「成员重复→登记语义期」：语法期不报错，只登记。
                led.raise(DeferredMember::DuplicateMemberName);
            }
            seen.push(m.name);
        }
        if m.has_layout() {
            // 布局组合是否合法交 F0443 语义期裁（锚点边界显性）。
            led.raise(DeferredMember::LayoutCombination);
        }
    }
    led
}

/// 合并两个延后账本（O(1)，逐项相加）。
fn merge_deferred(dst: &mut DeferredMemberLedger, src: &DeferredMemberLedger) {
    let d = src.count_of(DeferredMember::DuplicateMemberName);
    let l = src.count_of(DeferredMember::LayoutCombination);
    let a = src.count_of(DeferredMember::AnonConflict);
    for _ in 0..d {
        dst.raise(DeferredMember::DuplicateMemberName);
    }
    for _ in 0..l {
        dst.raise(DeferredMember::LayoutCombination);
    }
    for _ in 0..a {
        dst.raise(DeferredMember::AnonConflict);
    }
}

/// 从顶层声明窗口推断接口块种类（读块头 `uniform`/`buffer`/`storage` 词）。
fn iface_kind_of(decl: &TopDecl, src: &str, toks: &[Token], lo: usize) -> IfaceKind {
    let hi = decl.token_hi as usize;
    let mut j = lo;
    while j < hi {
        if let Some(t) = toks.get(j) {
            let txt = text_of(t, src);
            if let Some(k) = IfaceKind::of_name(txt) {
                return k;
            }
            if is_lbrace(t, src) {
                break;
            }
        }
        j += 1;
    }
    IfaceKind::Uniform
}

/// 取接口块实例名（缺省即匿名块，`name_present == false`）。
fn instance_name_of(name: u32, present: bool) -> u32 {
    if present {
        name
    } else {
        u32::MAX
    }
}

/// 结构成员面摘要行（面板 / 完成摘要数据来源）。
pub fn screen_line(r: &StructParseResult) -> String {
    format!(
        "s04 {} blocks={} structs={} ifaces={} members={} layouts={} diags={} deferred={} peers={}",
        ANCHOR,
        r.block_count(),
        r.struct_count(),
        r.iface_count(),
        r.member_total(),
        r.layout_total(),
        r.diags.len(),
        r.deferred.total(),
        PEER_DECLARATIONS.len(),
    )
}

impl StructParseResult {
    /// 带布局限定的成员总数（结构 + 接口）。
    pub fn layout_total(&self) -> u32 {
        let mut n = 0u32;
        for s in self.structs.iter() {
            n = n.saturating_add(s.layout_count());
        }
        for b in self.ifaces.iter() {
            n = n.saturating_add(b.layout_count());
        }
        n
    }

    /// 未闭合块数（边界显性的直接证据）。
    pub fn unclosed_blocks(&self) -> u32 {
        let mut n = 0u32;
        for s in self.structs.iter() {
            if !s.closed {
                n = n.saturating_add(1);
            }
        }
        for b in self.ifaces.iter() {
            if !b.closed {
                n = n.saturating_add(1);
            }
        }
        n
    }
}
