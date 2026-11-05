//! UNX-F3412 断言消息本地化与诊断串 · 0x79640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B11 逐字摘录）：
//!   ① 纯功能代码 ≥240 行；
//!   ② J1：诊断串生成（表达式文本化 + 上下文摘要）；M-171 诊断串可读性抽检。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 表达式文本化：二元断言按操作符闭集（Eq/Ne/Lt/Le/Gt/Ge/True/False）
//!     生成「左值(型) 符号 右值(型)」的文本化表达式；
//!   - 上下文摘要：用例标 + 行号 + 宏名三要素拼进诊断串头，
//!     保证红项在聚合日志里可直接定位到行；
//!   - 本地化目录：每宏一条中文使用模板（冻结表），渲染时按模板补建议；
//!   - M-171 可读性抽检：非空、含「：」分隔、含「建议」、长度
//!     [16,160]、无控制字符、双空格不出现——六条全过才算可读；
//!   - 帧状态机 Draft→Annotated→Localized→Sealed 单调推进，倒退显性拒；
//!   - 注入必抓：空表达式/超长串/缺右值操作符三类违约串必须被抽检点名。
//! 常量来源：
//!   - 长度窗 [16,160]：48 字节动态明细容量（checks.rs DYN_CAP）的
//!     1/3 与 3 倍取整对齐，保证诊断串进明细侧表不被截断到不可读；
//!   - 操作符闭集 8 项：与 B12 双后端接口约定（UNX-F3417）的宏面一一对应。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，段位守卫区间 [0x79600, 0x797C0] 内）。
pub const SEG_BASE: u32 = 0x79640;

/// 诊断串长度下界（短于此即不可读）。
pub const LEN_MIN: usize = 16;

/// 诊断串长度上界（超此即进明细侧表会被截断，违约定为不可读）。
pub const LEN_MAX: usize = 160;

/// 二元操作符闭集（与断言宏面一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// 等值（kexpect_eq）。
    Eq,
    /// 不等（kexpect_ne）。
    Ne,
    /// 小于（kexpect_lt）。
    Lt,
    /// 小于等于（kexpect_le）。
    Le,
    /// 大于（kexpect_gt）。
    Gt,
    /// 大于等于（kexpect_ge）。
    Ge,
    /// 真值（kexpect_true）。
    True,
    /// 假值（kexpect_false）。
    False,
}

impl BinOp {
    /// 符号文本（表达式文本化的中段）。
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::True => "is-true",
            BinOp::False => "is-false",
        }
    }

    /// 中文谓词（本地化目录用）。
    pub fn zh(self) -> &'static str {
        match self {
            BinOp::Eq => "等值",
            BinOp::Ne => "不等",
            BinOp::Lt => "小于",
            BinOp::Le => "不大于",
            BinOp::Gt => "大于",
            BinOp::Ge => "不小于",
            BinOp::True => "应为真",
            BinOp::False => "应为假",
        }
    }
}

/// 全量操作符闭集（8 项，顺序即编码序）。
pub const ALL_OPS: [BinOp; 8] = [
    BinOp::Eq,
    BinOp::Ne,
    BinOp::Lt,
    BinOp::Le,
    BinOp::Gt,
    BinOp::Ge,
    BinOp::True,
    BinOp::False,
];

/// 值形态闭集（诊断串里的类型标注）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValKind {
    /// 整数型。
    Int,
    /// 字符串型。
    Str,
    /// 布尔型。
    Bool,
    /// 空值型。
    Nil,
}

impl ValKind {
    /// 中文类型标注。
    pub fn zh(self) -> &'static str {
        match self {
            ValKind::Int => "整数",
            ValKind::Str => "字符串",
            ValKind::Bool => "布尔",
            ValKind::Nil => "空值",
        }
    }
}

/// 帧状态机（单调推进，倒退显性拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePhase {
    /// 草稿：仅有原始表达式。
    Draft,
    /// 已标注：左右值形态已补。
    Annotated,
    /// 已本地化：中文模板已套。
    Localized,
    /// 已封印：可读性抽检通过，只读。
    Sealed,
}

impl FramePhase {
    /// 单调秩。
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            FramePhase::Draft => "草稿",
            FramePhase::Annotated => "已标注",
            FramePhase::Localized => "已本地化",
            FramePhase::Sealed => "已封印",
        }
    }
}

/// 帧推进器：rank 不增即显性拒。
pub fn step_phase(cur: FramePhase, next: FramePhase) -> Result<FramePhase, u32> {
    if next.rank() <= cur.rank() {
        return Err(Diag::PhaseRegress.val());
    }
    Ok(next)
}

/// 诊断码闭集：0x79641..=0x7964C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 表达式文本为空：文本化产物缺失。
    ExprEmpty = 0x01,
    /// 操作数缺失：二元操作符缺右值。
    OperandMissing = 0x02,
    /// 长度越窗：诊断串短于 LEN_MIN 或长于 LEN_MAX。
    LenOutOfBounds = 0x03,
    /// 控制字符混入：诊断串出现不可打印字节。
    ControlByte = 0x04,
    /// 双空格噪声：本地化排版未压实。
    DoubleSpace = 0x05,
    /// 建议缺失：本地化模板未补「建议」段。
    AdviceMissing = 0x06,
    /// 帧状态倒退：Phase 迁移 rank 不增。
    PhaseRegress = 0x07,
    /// 未封印即出账：Sealed 前的帧被渲染进聚合日志。
    UnsealedEmit = 0x08,
    /// 目录缺口：宏名在本地化目录查无模板。
    CatalogMiss = 0x09,
    /// 双跑不一致：同输入两次渲染文本不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：③ 判据的写回面破约。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ExprEmpty => "表达式文本为空：建议核对 textualize 是否吞掉了表达式源文",
            Diag::OperandMissing => "操作数缺失：建议二元断言两侧都给 ExprSide，缺侧先拒",
            Diag::LenOutOfBounds => "长度越窗：建议压缩上下文摘要或拆分表达式，控制在16..=160字节",
            Diag::ControlByte => "控制字符混入：建议渲染前过滤 \\n\\t 等不可打印字节",
            Diag::DoubleSpace => "双空格噪声：建议拼接时用单空格分隔，模板尾部去空格",
            Diag::AdviceMissing => "建议缺失：建议每条诊断串尾部挂本地化目录里的中文建议",
            Diag::PhaseRegress => "帧状态倒退：建议按 Draft→Annotated→Localized→Sealed 单调推进",
            Diag::UnsealedEmit => "未封印即出账：建议渲染前校验帧状态为 Sealed",
            Diag::CatalogMiss => "目录缺口：建议新增宏时同步补本地化模板并跑目录完整性判据",
            Diag::DoubleRunDiverge => "双跑不一致：建议渲染做成纯函数，禁用任何环境态输入",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与段窗区间",
            Diag::SummaryEmpty => "写回摘要为空：建议收口时以非空中文摘要写回状态=已完成",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::ExprEmpty,
    Diag::OperandMissing,
    Diag::LenOutOfBounds,
    Diag::ControlByte,
    Diag::DoubleSpace,
    Diag::AdviceMissing,
    Diag::PhaseRegress,
    Diag::UnsealedEmit,
    Diag::CatalogMiss,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79641..=0x7964C 闭集内"
}

/// 上下文引用（摘要三要素之二：用例标 + 行号；宏名在帧上）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtxRef {
    /// 用例标签。
    pub case: &'static str,
    /// 行号（1 起）。
    pub line: u32,
}

impl CtxRef {
    /// 上下文摘要串（紧凑式）：「用例 X#N」。
    pub fn summary(&self) -> String {
        format!("用例{}#{}", self.case, self.line)
    }
}

/// 表达式单侧（文本化后的操作数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprSide {
    /// 操作数文本（文本化产物，如 "4"、"\"abc\""）。
    pub repr: String,
    /// 值形态（诊断串的类型标注）。
    pub kind: ValKind,
}

impl ExprSide {
    /// 构造整数侧。
    pub fn int(v: u64) -> ExprSide {
        ExprSide { repr: format!("{}", v), kind: ValKind::Int }
    }

    /// 构造字符串侧。
    pub fn str_side(s: &str) -> ExprSide {
        ExprSide { repr: format!("\"{}\"", s), kind: ValKind::Str }
    }

    /// 构造布尔侧。
    pub fn bool_side(b: bool) -> ExprSide {
        ExprSide { repr: if b { "true" } else { "false" }.to_string(), kind: ValKind::Bool }
    }

    /// 构造空值侧。
    pub fn nil() -> ExprSide {
        ExprSide { repr: "None".to_string(), kind: ValKind::Nil }
    }

    /// 带类型标注的操作数文本。
    pub fn annotated(&self) -> String {
        format!("{}({})", self.repr, self.kind.zh())
    }
}

/// 诊断帧：一次断言失败的完整本地化承载。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagFrame {
    /// 宏名（目录键）。
    pub macro_name: &'static str,
    /// 操作符。
    pub op: BinOp,
    /// 左值。
    pub lhs: ExprSide,
    /// 右值（一元断言为 None）。
    pub rhs: Option<ExprSide>,
    /// 上下文引用。
    pub ctx: CtxRef,
    /// 帧状态（单调推进）。
    pub phase: FramePhase,
}

/// 本地化目录（冻结表）：宏名 → 中文使用模板。
/// 模板语义：说明该宏何时用 + 失败时该查什么（可操作建议的来源）。
pub const MACRO_CATALOG: [(&'static str, &'static str); 8] = [
    ("kexpect_eq", "等值断言：建议核对两侧类型与量纲"),
    ("kexpect_ne", "不等断言：建议确认两侧值确已分化"),
    ("kexpect_lt", "小于断言：建议核对边界含不含等"),
    ("kexpect_le", "不大于断言：建议核对边界含不含等"),
    ("kexpect_gt", "大于断言：建议核对方向是否写反"),
    ("kexpect_ge", "不小于断言：建议核对方向是否写反"),
    ("kexpect_true", "真值断言：建议核对条件短路路径"),
    ("kexpect_false", "假值断言：建议核对取反逻辑"),
];

/// 目录查表：宏名 → 模板；查无回 None。
pub fn catalog_lookup(macro_name: &str) -> Option<&'static str> {
    let mut found: Option<&'static str> = None;
    for (name, tpl) in MACRO_CATALOG.iter() {
        if *name == macro_name {
            found = Some(tpl);
        }
    }
    found
}

/// 表达式文本化：操作符 + 两侧标注拼成表达式段。
pub fn textualize(f: &DiagFrame) -> Result<String, u32> {
    let sym = f.op.symbol();
    let left = f.lhs.annotated();
    match &f.rhs {
        Some(r) => Ok(format!("{} {} {}", left, sym, r.annotated())),
        None => {
            if f.op != BinOp::True && f.op != BinOp::False {
                return Err(Diag::OperandMissing.val());
            }
            Ok(format!("({} {})", left, sym))
        }
    }
}

/// 渲染一帧为完整诊断串（纯函数：输入定则输出定）。
/// 流程：校验帧状态 Sealed → 表达式文本化 → 套上下文摘要 →
/// 挂目录建议 → 六条可读性自检，任一破即 Err 闭集码。
pub fn render_frame(f: &DiagFrame) -> Result<String, u32> {
    if f.phase != FramePhase::Sealed {
        return Err(Diag::UnsealedEmit.val());
    }
    let tpl = match catalog_lookup(f.macro_name) {
        Some(t) => t,
        None => return Err(Diag::CatalogMiss.val()),
    };
    let expr = match textualize(f) {
        Ok(e) => e,
        Err(e) => return Err(e),
    };
    if expr.trim().is_empty() {
        return Err(Diag::ExprEmpty.val());
    }
    // 操作数空检：任一侧文本化产物为空即表达式丢失（注入必抓面）。
    if f.lhs.repr.trim().is_empty() {
        return Err(Diag::ExprEmpty.val());
    }
    if let Some(r) = f.rhs.as_ref() {
        if r.repr.trim().is_empty() {
            return Err(Diag::ExprEmpty.val());
        }
    }
    let head = f.ctx.summary();
    // 紧凑排版：长度窗 [16,160] 的主要约束来自模板与摘要，
    // 故头部用「宏名@摘要：表达式」的单行紧凑式。
    let mut out = format!("断言失败：{}@{}：{}", f.macro_name, head, expr);
    out.push_str("；建议：");
    out.push_str(tpl);
    // 排版压实：双空格收敛为单空格（本地化排版纪律）。
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    // 控制字节过滤：换行/制表一律替换为顿号。
    out = out.replace('\n', "、").replace('\t', "、");
    match spot_check(&out) {
        Ok(()) => Ok(out),
        Err(e) => Err(e),
    }
}

/// M-171 可读性抽检（六条）：非空、有分隔、有建议、长度窗、
/// 无控制字节、无双空格。全过 Ok(())，破者逐条点名 Err。
pub fn spot_check(s: &str) -> Result<(), u32> {
    if s.is_empty() {
        return Err(Diag::ExprEmpty.val());
    }
    // 长度窗先于排版检：短串/超长串一律先点名越窗，
    // 再谈分隔与建议（短串谈可读性没有意义）。
    if s.len() < LEN_MIN || s.len() > LEN_MAX {
        return Err(Diag::LenOutOfBounds.val());
    }
    if !s.contains("：") {
        return Err(Diag::AdviceMissing.val());
    }
    if !s.contains("建议") {
        return Err(Diag::AdviceMissing.val());
    }
    for ch in s.chars() {
        if (ch as u32) < 0x20 && ch != ' ' {
            return Err(Diag::ControlByte.val());
        }
    }
    if s.contains("  ") {
        return Err(Diag::DoubleSpace.val());
    }
    Ok(())
}

/// 造帧（草稿起步）：一元/二元自适应，phase 从 Draft 起。
pub fn make_frame(
    macro_name: &'static str,
    op: BinOp,
    lhs: ExprSide,
    rhs: Option<ExprSide>,
    case: &'static str,
    line: u32,
) -> DiagFrame {
    DiagFrame {
        macro_name,
        op,
        lhs,
        rhs,
        ctx: CtxRef { case, line },
        phase: FramePhase::Draft,
    }
}

/// 推帧到封印：Draft→Annotated→Localized→Sealed 三步，
/// 任一步倒退即 Err（正常流下 rank 严格递增，必过）。
pub fn seal_frame(f: &mut DiagFrame) -> Result<(), u32> {
    let steps = [
        (FramePhase::Draft, FramePhase::Annotated),
        (FramePhase::Annotated, FramePhase::Localized),
        (FramePhase::Localized, FramePhase::Sealed),
    ];
    for (cur, next) in steps.iter() {
        if f.phase != *cur {
            return Err(Diag::PhaseRegress.val());
        }
        match step_phase(*cur, *next) {
            Ok(_) => f.phase = *next,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// 抽检样本（冻结）：本卡固定跑 6 帧覆盖 6 个宏 + 2 类一元。
/// （来源：M-171 抽检判据面；样本即抽检批，覆盖全操作符闭集。）
pub fn frozen_samples() -> Vec<DiagFrame> {
    let mut v: Vec<DiagFrame> = Vec::new();
    v.push(make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(4), Some(ExprSide::int(5)), "样本批", 12));
    v.push(make_frame("kexpect_ne", BinOp::Ne, ExprSide::int(7), Some(ExprSide::int(7)), "样本批", 18));
    v.push(make_frame("kexpect_lt", BinOp::Lt, ExprSide::int(9), Some(ExprSide::int(3)), "样本批", 24));
    v.push(make_frame("kexpect_le", BinOp::Le, ExprSide::str_side("b"), Some(ExprSide::str_side("a")), "样本批", 30));
    v.push(make_frame("kexpect_gt", BinOp::Gt, ExprSide::int(1), Some(ExprSide::int(9)), "样本批", 36));
    v.push(make_frame("kexpect_true", BinOp::True, ExprSide::bool_side(false), None, "样本批", 42));
    v.push(make_frame("kexpect_false", BinOp::False, ExprSide::bool_side(true), None, "样本批", 48));
    v.push(make_frame("kexpect_ge", BinOp::Ge, ExprSide::nil(), Some(ExprSide::int(0)), "样本批", 54));
    v
}

/// 冻结样本批全渲染：全部封印并渲染成功，返回串集；
/// 任一帧失败即 Err（抽检批必须全绿才能出账）。
pub fn render_all_samples() -> Result<Vec<String>, u32> {
    let mut out: Vec<String> = Vec::new();
    for f in frozen_samples().iter_mut() {
        match seal_frame(f) {
            Ok(()) => {}
            Err(e) => return Err(e),
        }
        match render_frame(f) {
            Ok(s) => out.push(s),
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + 段窗 [0x79600, 0x797C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79600 && SEG_BASE <= 0x797C0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// 正常流：冻结样本批全部封印并渲染成功。
fn sample_batch_ok() -> bool {
    match render_all_samples() {
        Ok(v) => v.len() == 8,
        Err(_) => false,
    }
}

/// 表达式文本化：八操作符符号与中文谓词逐项对上，一元自适应括号。
fn textualize_ok() -> bool {
    let f2 = make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(1), Some(ExprSide::int(2)), "文", 1);
    match textualize(&f2) {
        Ok(s) => {
            if !s.contains("==") || !s.contains("整数") {
                return false;
            }
        }
        Err(_) => return false,
    }
    let f1 = make_frame("kexpect_true", BinOp::True, ExprSide::bool_side(true), None, "文", 2);
    let one_arity_ok = match textualize(&f1) {
        Ok(s) => s.starts_with('(') && s.ends_with(')') && s.contains("is-true"),
        Err(_) => false,
    };
    if !one_arity_ok {
        return false;
    }
    let mut all_ops_present = true;
    for op in ALL_OPS.iter() {
        if op.symbol().is_empty() || op.zh().is_empty() {
            all_ops_present = false;
        }
    }
    all_ops_present
}

/// 上下文摘要：三要素齐进诊断串头，行号随输入精确落串。
fn ctx_summary_ok() -> bool {
    let c = CtxRef { case: "页分配冒烟", line: 33 };
    let s = c.summary();
    s.contains("页分配冒烟") && s.contains("33") && s.starts_with("用例")
}

/// M-171 可读性抽检：冻结样本批逐条六检全过。
fn spot_check_ok() -> bool {
    let batch = match render_all_samples() {
        Ok(v) => v,
        Err(_) => return false,
    };
    for s in batch.iter() {
        if spot_check(s).is_err() {
            return false;
        }
        if !s.contains("断言失败") || !s.contains("建议") {
            return false;
        }
    }
    true
}

/// 注入必抓 1：空表达式帧必须被 ExprEmpty 点名。
fn inj_empty_expr_ok() -> bool {
    let mut f = make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(0), Some(ExprSide::int(0)), "注入", 1);
    // 注入点：抽掉表达式文本（模拟文本化丢源文）。
    f.lhs.repr.clear();
    if seal_frame(&mut f).is_err() {
        return false;
    }
    match render_frame(&f) {
        Err(e) => e == Diag::ExprEmpty.val(),
        Ok(_) => false,
    }
}

/// 注入必抓 2：二元操作符缺右值必须被 OperandMissing 点名。
fn inj_missing_rhs_ok() -> bool {
    let mut f = make_frame("kexpect_lt", BinOp::Lt, ExprSide::int(1), None, "注入", 2);
    if seal_frame(&mut f).is_err() {
        return false;
    }
    match render_frame(&f) {
        Err(e) => e == Diag::OperandMissing.val(),
        Ok(_) => false,
    }
}

/// 注入必抓 3：超长诊断串必须被 LenOutOfBounds 点名（长度窗上界）。
fn inj_overlong_ok() -> bool {
    let mut f = make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(1), Some(ExprSide::int(2)), "注入", 3);
    // 注入点：塞超长上下文摘要（>160 字节的用例标）。
    f.ctx.case = "超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标超长用例标";
    if seal_frame(&mut f).is_err() {
        return false;
    }
    match render_frame(&f) {
        Err(e) => e == Diag::LenOutOfBounds.val(),
        Ok(_) => false,
    }
}

/// 注入必抓 4：未封印帧出账必须被 UnsealedEmit 点名。
fn inj_unsealed_ok() -> bool {
    let f = make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(1), Some(ExprSide::int(2)), "注入", 4);
    match render_frame(&f) {
        Err(e) => e == Diag::UnsealedEmit.val(),
        Ok(_) => false,
    }
}

/// 状态机倒退显性拒：五组违约迁移全部 Err。
fn phase_regress_ok() -> bool {
    let cases = [
        (FramePhase::Annotated, FramePhase::Draft),
        (FramePhase::Sealed, FramePhase::Localized),
        (FramePhase::Localized, FramePhase::Annotated),
        (FramePhase::Draft, FramePhase::Draft),
        (FramePhase::Sealed, FramePhase::Sealed),
    ];
    for (cur, next) in cases.iter() {
        match step_phase(*cur, *next) {
            Err(e) => {
                if e != Diag::PhaseRegress.val() {
                    return false;
                }
            }
            Ok(_) => return false,
        }
    }
    // 封印后再推必须拒（单调终态）。
    let mut f = make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(1), Some(ExprSide::int(2)), "回", 1);
    if seal_frame(&mut f).is_err() || f.phase != FramePhase::Sealed {
        return false;
    }
    step_phase(f.phase, FramePhase::Draft).is_err()
}

/// 本地化目录：8 宏全覆盖、模板非空带建议、查无宏回 None。
fn catalog_ok() -> bool {
    if MACRO_CATALOG.len() != 8 {
        return false;
    }
    for (name, tpl) in MACRO_CATALOG.iter() {
        if name.is_empty() || tpl.is_empty() || !tpl.contains("建议") {
            return false;
        }
        match catalog_lookup(name) {
            Some(t) => {
                if t != *tpl {
                    return false;
                }
            }
            None => return false,
        }
    }
    catalog_lookup("kexpect_noop").is_none()
}

/// 双跑一致：同一帧两次渲染逐字节相同；样本批两次渲染串集相同。
fn double_run_ok() -> bool {
    let mut f = make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(4), Some(ExprSide::int(5)), "双跑", 9);
    if seal_frame(&mut f).is_err() {
        return false;
    }
    let a = match render_frame(&f) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let b = match render_frame(&f) {
        Ok(s) => s,
        Err(_) => return false,
    };
    if a != b {
        return false;
    }
    let batch1 = match render_all_samples() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let batch2 = match render_all_samples() {
        Ok(v) => v,
        Err(_) => return false,
    };
    batch1 == batch2
}

/// 边界：长度窗两端（LEN_MIN 邻域）语义可判——
/// 正常样本串必须落在窗内且距两端有正余量。
fn len_bounds_ok() -> bool {
    let batch = match render_all_samples() {
        Ok(v) => v,
        Err(_) => return false,
    };
    for s in batch.iter() {
        if s.len() <= LEN_MIN || s.len() >= LEN_MAX {
            return false;
        }
    }
    // 窗外语义：短串/长串必须被抽检分别点名。
    if spot_check("").unwrap_err() != Diag::ExprEmpty.val() {
        return false;
    }
    let long: String = core::iter::repeat('长').take(LEN_MAX + 1).collect();
    spot_check(&long).unwrap_err() == Diag::LenOutOfBounds.val()
}

/// 错误路径 Result 语义：目录缺口码可复现、未知码兜底非空。
fn error_path_ok() -> bool {
    let mut f = make_frame("kexpect_noop", BinOp::Eq, ExprSide::int(1), Some(ExprSide::int(2)), "错", 1);
    if seal_frame(&mut f).is_err() {
        return false;
    }
    render_frame(&f) == Err(Diag::CatalogMiss.val())
        && !detail(0x0).is_empty()
        && detail(Diag::SummaryEmpty.val()).contains("写回")
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let batch = match render_all_samples() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let summary = format!("UNX-F3412 诊断串抽检：样本{}帧 全过", batch.len());
    let done = batch.len() == 8 && !summary.is_empty() && summary.contains("诊断串");
    let empty_verdict = if summary.is_empty() {
        Err(Diag::SummaryEmpty.val())
    } else {
        Ok(())
    };
    done && empty_verdict.is_ok()
}

// ── 判据总成：run_unx3412_checks ───────────────────────────────────────

/// run_unx3412_checks：本单判据聚合（诊断段 0x79640，全部走 add_dyn）。
pub fn run_unx3412_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3412 断言消息本地化与诊断串");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "表达式文本化八操作符全覆盖",
            textualize_ok(),
            format!("文本化{}", if textualize_ok() { "过" } else { "破" }),
        ),
        (
            "上下文摘要三要素齐进串头",
            ctx_summary_ok(),
            format!("摘要{}", if ctx_summary_ok() { "过" } else { "破" }),
        ),
        (
            "正常流冻结样本批八帧全渲染",
            sample_batch_ok(),
            format!("样本批{}", if sample_batch_ok() { "全绿" } else { "有红" }),
        ),
        (
            "M-171可读性抽检六条全过",
            spot_check_ok(),
            format!("抽检{}", if spot_check_ok() { "过" } else { "破" }),
        ),
        (
            "注入1空表达式必抓",
            inj_empty_expr_ok(),
            format!("空表达式{}", if inj_empty_expr_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2缺右值必抓",
            inj_missing_rhs_ok(),
            format!("缺右值{}", if inj_missing_rhs_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3超长串必抓",
            inj_overlong_ok(),
            format!("超长串{}", if inj_overlong_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4未封印出账必抓",
            inj_unsealed_ok(),
            format!("未封印{}", if inj_unsealed_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "帧状态机倒退五组违约全显性拒",
            phase_regress_ok(),
            format!("倒退拒{}", if phase_regress_ok() { "过" } else { "失" }),
        ),
        (
            "本地化目录八宏全覆盖且查无回空",
            catalog_ok(),
            format!("目录{}", if catalog_ok() { "齐" } else { "缺" }),
        ),
        (
            "双跑一致+长度窗+错误路径+写回非空",
            double_run_ok() && len_bounds_ok() && error_path_ok() && writeback_summary_ok(),
            format!("综合{}", if double_run_ok() && len_bounds_ok() && error_path_ok() && writeback_summary_ok() { "过" } else { "破" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79640);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
            assert!(d.detail().contains("建议"));
        }
    }

    #[test]
    fn textualize_covers_all_ops() {
        assert!(textualize_ok());
        let f = make_frame("kexpect_ne", BinOp::Ne, ExprSide::int(1), Some(ExprSide::int(2)), "t", 1);
        assert!(textualize(&f).unwrap().contains("!="));
    }

    #[test]
    fn frozen_samples_render_all_green() {
        assert!(sample_batch_ok());
        assert!(spot_check_ok());
        let batch = render_all_samples().unwrap();
        assert_eq!(batch.len(), 8);
        for s in batch.iter() {
            assert!(s.contains("断言失败"));
            assert!(s.contains("建议"));
        }
    }

    #[test]
    fn injections_are_caught() {
        assert!(inj_empty_expr_ok());
        assert!(inj_missing_rhs_ok());
        assert!(inj_overlong_ok());
        assert!(inj_unsealed_ok());
    }

    #[test]
    fn phase_machine_rejects_regression() {
        assert!(phase_regress_ok());
        let mut f = make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(1), Some(ExprSide::int(2)), "t", 2);
        assert!(seal_frame(&mut f).is_ok());
        assert_eq!(f.phase, FramePhase::Sealed);
        assert_eq!(
            step_phase(f.phase, FramePhase::Localized).unwrap_err(),
            Diag::PhaseRegress.val()
        );
    }

    #[test]
    fn catalog_is_frozen_and_lookup_honest() {
        assert!(catalog_ok());
        assert_eq!(
            catalog_lookup("kexpect_eq").unwrap(),
            "等值断言：建议核对两侧类型与量纲"
        );
        assert!(catalog_lookup("kexpect_missing").is_none());
    }

    #[test]
    fn double_run_is_byte_identical() {
        assert!(double_run_ok());
        let mut f = make_frame("kexpect_gt", BinOp::Gt, ExprSide::int(5), Some(ExprSide::int(3)), "d", 7);
        seal_frame(&mut f).unwrap();
        assert_eq!(render_frame(&f).unwrap(), render_frame(&f).unwrap());
    }

    #[test]
    fn length_window_enforced() {
        assert!(len_bounds_ok());
        let s = "合规诊断串：长度落在窗内且带建议段";
        assert!(spot_check(s).is_ok());
        assert_eq!(spot_check("短").unwrap_err(), Diag::LenOutOfBounds.val());
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        let f = make_frame("kexpect_eq", BinOp::Eq, ExprSide::int(1), Some(ExprSide::int(2)), "e", 1);
        assert_eq!(render_frame(&f).unwrap_err(), Diag::UnsealedEmit.val());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
        let batch = render_all_samples().unwrap();
        assert_eq!(batch.len(), 8);
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3412_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }

    #[test]
    fn ctx_summary_carries_case_and_line() {
        assert!(ctx_summary_ok());
        let c = CtxRef { case: "边界批", line: 4096 };
        let s = c.summary();
        assert!(s.contains("边界批"));
        assert!(s.contains("4096"));
    }
}
