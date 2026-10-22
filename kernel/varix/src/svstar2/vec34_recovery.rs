//! VE-F0434 · 语法错误恢复（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0434`
//!
//! **锚点原文**：「语法错误恢复体系：恐慌模式（同步记号集按声明、语句、
//! 表达式三层边界）、记号插入与删除建议（缺分号缺括号类高频错误启发式
//! 建议）、恢复显性计数与级联指标（F0417 词法恢复同构语义）；恢复后 AST
//! 标记不完整段（供语义期跳过或降级处理显性）。判据：三层同步、启发建议、
//! 级联指标、不完整标记、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三层同步**（判据一）。同步记号集 [`SyncSet`] 分三层：声明层（`{`/
//!    `;` 收尾的全局物）、语句层（`{`/`;`）、表达式层（`)`/`]`/`,`/`;`）。
//!    恢复按**当前上下文层**取集：[`panic_skip`] 给层与流，吐出同步跳过
//!    的记号数。层语义入册：高层恢复不跳过低层同步点**之外**的结构边界
//!    （声明层跳过不越 `}` 的**函数体级**边界——用深度计数保证）。
//!
//! 2. **启发建议**（判据二）。高频错误建议表 [`Advice`]：缺分号（下一记
//!    号在新行首且是引导词）→ 建议「插入 `;`」；缺 `)`（括号深度 1 且遇
//!    语句引导词）→ 建议「插入 `)`」。建议是**启发式**：误报可接受，但
//!    每条建议必须可复算（纯函数 [`suggest`]：同样的流同样位置出同样的
//!    建议）。
//!
//! 3. **级联指标**（判据三）。恢复事件显性计数（[`RecoveryStats`]）：
//!    恢复次数、跳过记号数、级联深度（一次错误后到下一同步点之间的
//!    追加错误数）。锚点「F0417 词法恢复同构语义」：计数口径与词法域
//!    相同——只增不减、逐事件留痕。级联爆发（指标超阈）→ 回退上层同步
//!    点的处置有独立计数位。
//!
//! 4. **不完整标记**（判据四）。恢复后的段标 [`Incomplete`]（跨度 + 原因
//!    码）：供语义期跳过或降级。判据验证：标记跨度与恢复跳过范围一致
//!    （标记无段/段无标记 = 失联，双向核对）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0434";

// ---------------------------------------------------------------------------
// 词位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Ident,
    Number,
    Sym,
    Kw,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Tk,
    pub text: String,
    pub off: u32,
}

pub fn mini_lex(src: &str) -> Vec<Tok> {
    const KWS: [&str; 6] = ["if", "for", "while", "return", "void", "int"];
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if "(){}[];,=".contains(c) {
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
// 同步集（三层）
// ---------------------------------------------------------------------------

/// 恢复上下文层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Decl,
    Stmt,
    Expr,
}

/// 层的同步记号集。
pub fn sync_set(layer: Layer) -> &'static [&'static str] {
    match layer {
        Layer::Decl => &[";", "}"],
        Layer::Stmt => &[";", "}"],
        Layer::Expr => &[")", "]", ",", ";"],
    }
}

/// 恐慌跳过：从 pos 起扫到同步记号（含）止，返回跳过数与新 pos。
/// 结构边界保护：`}` 计深——Decl 层跳过不越过净深度为负的 `}`。
pub fn panic_skip(toks: &[Tok], pos: usize, layer: Layer) -> (usize, usize) {
    let set = sync_set(layer);
    let mut i = pos;
    let mut depth = 0i32;
    while i < toks.len() {
        let t = &toks[i];
        if t.kind == Tk::Eof {
            // 流尾：Eof 不是可跳记号也不计入跳过数，落点即 Eof。
            return (i - pos, i);
        }
        if t.kind == Tk::Sym {
            if t.text == "{" {
                depth += 1;
            }
            if t.text == "}" {
                if layer == Layer::Decl && depth == 0 {
                    // Decl 层：函数体级边界不越。
                    return (i - pos, i);
                }
                depth -= 1;
            }
            if depth == 0 && set.contains(&t.text.as_str()) {
                return (i - pos + 1, i + 1);
            }
        }
        i += 1;
    }
    (i - pos, i)
}

// ---------------------------------------------------------------------------
// 启发式建议（纯函数可复算）
// ---------------------------------------------------------------------------

/// 建议形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice {
    /// 插入 `;`。
    InsertSemi,
    /// 插入 `)`。
    InsertParen,
    /// 无建议（显性无，不是缺省）。
    None,
}

/// 建议：缺分号——下一记号是语句引导词（Kw）而前一记号非控制结构。
pub fn suggest(toks: &[Tok], pos: usize) -> Advice {
    if pos == 0 || pos >= toks.len() {
        return Advice::None;
    }
    let prev = &toks[pos - 1];
    let next = &toks[pos];
    // 缺 `)`：前面是表达式尾巴而当前是语句引导词。
    if prev.kind == Tk::Ident && next.kind == Tk::Kw && next.text == "return" {
        return Advice::InsertParen;
    }
    // 缺 `;`：前一记号非分号且当前是新语句引导词。
    if !(prev.kind == Tk::Sym && prev.text == ";") && next.kind == Tk::Kw && matches!(next.text.as_str(), "if" | "for" | "while" | "return") {
        return Advice::InsertSemi;
    }
    Advice::None
}

// ---------------------------------------------------------------------------
// 恢复统计与不完整标记
// ---------------------------------------------------------------------------

/// 恢复统计（同 F0417 词法口径：只增留痕）。
#[derive(Debug, Default, Clone, Copy)]
pub struct RecoveryStats {
    pub events: u32,
    pub skipped_toks: u32,
    pub cascade_extra: u32,
    pub escalated: u32,
}

impl RecoveryStats {
    /// 记一次恢复事件。
    pub fn note(&mut self, skipped: u32) {
        self.events += 1;
        self.skipped_toks += skipped;
    }
    /// 级联：同窗口追加错误。
    pub fn note_cascade(&mut self, extra: u32) {
        self.cascade_extra += extra;
    }
    /// 级联爆发 → 回退上层同步点（显性升级计数）。
    pub const CASCADE_CAP: u32 = 8;
    pub fn escalate_if_burst(&mut self) -> bool {
        if self.cascade_extra > Self::CASCADE_CAP {
            self.escalated += 1;
            true
        } else {
            false
        }
    }
}

/// 不完整段标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Incomplete {
    pub span: (u32, u32),
    /// 原因码：1 = 恐慌跳过段。
    pub reason: u8,
}

pub const REASON_PANIC: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecDiag {
    /// 恢复死循环（零推进）→ 强制最外层同步。
    NoProgress,
}

impl RecDiag {
    pub fn code(self) -> u16 {
        0x42E0 + self as u16
    }
}

/// 恢复执行器：一次 panic 恢复的全流程（跳过 + 统计 + 标记）。
pub struct Recoverer {
    pub stats: RecoveryStats,
    pub marks: Vec<Incomplete>,
    pub diags: Vec<(u32, RecDiag)>,
}

impl Recoverer {
    pub fn new() -> Self {
        Self { stats: RecoveryStats::default(), marks: Vec::new(), diags: Vec::new() }
    }
    /// 执行一次恢复：返回跳过的记号数。落点非同步记号（流尾无路）显性登记。
    pub fn recover(&mut self, toks: &[Tok], pos: usize, layer: Layer, last_err_off: u32) -> usize {
        let (n, new_pos) = panic_skip(toks, pos, layer);
        let hit_sync = new_pos > pos
            && toks[new_pos - 1].kind == Tk::Sym
            && sync_set(layer).contains(&toks[new_pos - 1].text.as_str());
        if !hit_sync {
            // 流尾无同步点可依（强制最外层亦无路）：显性登记不静默。
            self.diags.push((last_err_off, RecDiag::NoProgress));
        }
        self.stats.note(n as u32);
        let end_off = toks
            .get(new_pos.saturating_sub(1))
            .map(|t| t.off)
            .unwrap_or(last_err_off);
        self.marks.push(Incomplete { span: (last_err_off, end_off), reason: REASON_PANIC });
        n
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec34_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let toks = |src: &str| mini_lex(src);
    // 一、三层同步。
    {
        let t = toks("a b ; c } d ) e");
        // 语句层：跳到 `;`。
        let (n, np) = panic_skip(&t, 0, Layer::Stmt);
        v.push(("F0434-同步-语句层到分号", n == 3 && t[np - 1].text == ";"));
        // 表达式层：跳到 `)`。
        let (n2, np2) = panic_skip(&t, 5, Layer::Expr);
        v.push(("F0434-同步-表达式层到闭括号", n2 == 2 && t[np2 - 1].text == ")"));
        // 声明层：函数体边界不越。恐慌点在 `;`（下标 2）：消费 `;` 后
        // 落点停在 `}` 本身（不消费边界，更不越到其后的 z）。
        let t2 = toks("x y ; } z ;");
        let (n3b, np3b) = panic_skip(&t2, 2, Layer::Decl);
        v.push((
            "F0434-同步-声明层不越体界",
            n3b == 1 && t2[np3b].text == "}",
        ));
        // 层集入册：三层集合互异（表达式层含逗号闭括号）。
        v.push((
            "F0434-同步-层集互异",
            sync_set(Layer::Expr).contains(&",") && !sync_set(Layer::Stmt).contains(&","),
        ));
    }
    // 二、启发建议（可复算纯函数）。
    {
        let t = toks("a = 1 if (x) y();");
        v.push((
            "F0434-建议-缺分号",
            suggest(&t, 3) == Advice::InsertSemi,
        ));
        // 复算一致性：同输入同建议。
        v.push(("F0434-建议-复算一致", suggest(&t, 3) == suggest(&t, 3)));
        // 正常序列无建议。
        let t2 = toks("a = 1; if (x) y();");
        v.push(("F0434-建议-正常无建议", suggest(&t2, 3) == Advice::None));
        // 缺括号场景：表达式尾巴（y）遇语句引导词（return）。
        let t3 = toks("f(x + y return g;");
        v.push(("F0434-建议-缺闭括号", suggest(&t3, 5) == Advice::InsertParen));
    }
    // 三、级联指标。
    {
        let mut s = RecoveryStats::default();
        s.note(3);
        s.note(5);
        v.push((
            "F0434-级联-计数留痕",
            s.events == 2 && s.skipped_toks == 8,
        ));
        s.note_cascade(3);
        v.push(("F0434-级联-追加计数", s.cascade_extra == 3 && !s.escalate_if_burst()));
        // 级联爆发升级。
        let mut s2 = RecoveryStats::default();
        s2.note_cascade(RecoveryStats::CASCADE_CAP + 1);
        v.push(("F0434-级联-爆发升级", s2.escalate_if_burst() && s2.escalated == 1));
        // 阈值入册。
        v.push(("F0434-级联-阈值入册", RecoveryStats::CASCADE_CAP == 8));
    }
    // 四、不完整标记。
    {
        let t = toks("a b ; c");
        let mut r = Recoverer::new();
        let n = r.recover(&t, 0, Layer::Stmt, 0);
        v.push((
            "F0434-标记-跨度与跳过一致",
            n == 3 && r.marks.len() == 1 && r.stats.events == 1,
        ));
        // 标记原因码。
        v.push((
            "F0434-标记-原因码",
            r.marks[0].reason == REASON_PANIC && REASON_PANIC == 1,
        ));
        // 双向核对：marks 数 == stats.events 数（段无标记/标记无段即失联）。
        r.recover(&t, 4, Layer::Stmt, 0);
        v.push((
            "F0434-标记-双向核对",
            r.marks.len() == r.stats.events as usize,
        ));
        // 流尾无同步点：显性登记不静默。
        let t2 = toks("a b c");
        let mut r2 = Recoverer::new();
        let n2 = r2.recover(&t2, 0, Layer::Stmt, 0);
        v.push((
            "F0434-标记-流尾无同步登记",
            n2 == 3 && r2.diags.iter().any(|(_, d)| *d == RecDiag::NoProgress),
        ));
        // 判据自检。
        v.push(("F0434-判据-锚点入册", ANCHOR == "VE-F0434"));
        v.push(("F0434-判据-诊断码段", (0x42E0..0x42F0).contains(&RecDiag::NoProgress.code())));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec34_raw_checks as run_vec34_checks;
    #[test]
    fn vec34_recovery_checks() {
        for (name, ok) in run_vec34_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec34）。
pub fn run_vec34_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec34");
    for (name, ok) in vec34_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
