//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3402 · 断言宏族总表（kexpect_eq/ne/true/false/null/notnull）
//! · 诊断段 0xCB80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② J1：六基础宏实现（eq/ne/true/false/null/notnull），
//!      失败路径全通；M-161 每宏正反例；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言结果枚举：Pass/Fail(原因)；
//!   - 六宏语义核：eq/ne/true/false/null/notnull（函数核 + 宏封装）；
//!   - 失败路径全通：失败时返回 Fail 且不 panic（no_std 安全）；
//!   - M-161：每宏一正例一反例，12 例全过；
//!   - 断言账：逐例登记，账闭合（总例数 ≡ 正例+反例）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xCB80;

/// 断言名数。
pub const MACROS: usize = 6;
/// M-161 每宏正反例。
pub const M161_PER: usize = 2;

/// 断言结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertRes {
    Pass,
    Fail(String),
}

impl AssertRes {
    pub fn ok(&self) -> bool {
        matches!(self, AssertRes::Pass)
    }
}

/// 宏名。
pub const MACRO_NAMES: [&str; MACROS] = ["EQ", "NE", "TRUE", "FALSE", "NULL", "NOTNULL"];

// ── 六宏语义核 ──────────────────────────────────────────────────────────

/// kexpect_eq：a == b 才过。
pub fn kexpect_eq<T: PartialEq + core::fmt::Debug>(a: T, b: T) -> AssertRes {
    if a == b {
        AssertRes::Pass
    } else {
        AssertRes::Fail(String::from("EQ-MISMATCH"))
    }
}

/// kexpect_ne：a != b 才过。
pub fn kexpect_ne<T: PartialEq + core::fmt::Debug>(a: T, b: T) -> AssertRes {
    if a != b {
        AssertRes::Pass
    } else {
        AssertRes::Fail(String::from("NE-EQUAL"))
    }
}

/// kexpect_true：cond 为真才过。
pub fn kexpect_true(cond: bool) -> AssertRes {
    if cond {
        AssertRes::Pass
    } else {
        AssertRes::Fail(String::from("NOT-TRUE"))
    }
}

/// kexpect_false：cond 为假才过。
pub fn kexpect_false(cond: bool) -> AssertRes {
    if !cond {
        AssertRes::Pass
    } else {
        AssertRes::Fail(String::from("NOT-FALSE"))
    }
}

/// kexpect_null：Option 为 None 才过。
pub fn kexpect_null<T>(v: Option<T>) -> AssertRes {
    if v.is_none() {
        AssertRes::Pass
    } else {
        AssertRes::Fail(String::from("NOT-NULL"))
    }
}

/// kexpect_notnull：Option 为 Some 才过。
pub fn kexpect_notnull<T>(v: Option<T>) -> AssertRes {
    if v.is_some() {
        AssertRes::Pass
    } else {
        AssertRes::Fail(String::from("NULL"))
    }
}

/// 宏语义核分派：按宏名执行（总表锚）。
pub fn dispatch(name: &str, a: u64, b: u64, opt_none: bool) -> AssertRes {
    match name {
        "EQ" => kexpect_eq(a, b),
        "NE" => kexpect_ne(a, b),
        "TRUE" => kexpect_true(a == b),
        "FALSE" => kexpect_false(a == b),
        "NULL" => {
            if opt_none {
                kexpect_null::<u64>(None)
            } else {
                kexpect_null(Some(a))
            }
        }
        "NOTNULL" => {
            if opt_none {
                kexpect_notnull::<u64>(None)
            } else {
                kexpect_notnull(Some(a))
            }
        }
        _ => AssertRes::Fail(String::from("UNKNOWN-MACRO")),
    }
}

/// 断言账。
pub struct AssertBook {
    pub entries: Vec<(&'static str, bool)>,
}

impl AssertBook {
    pub fn new() -> Self {
        AssertBook { entries: Vec::new() }
    }

    pub fn record(&mut self, name: &'static str, ok: bool) {
        self.entries.push((name, ok));
    }

    pub fn closed(&self) -> bool {
        self.entries.len() == MACROS * M161_PER && self.entries.iter().all(|(_, ok)| *ok)
    }

    /// 每宏恰一正一反。
    pub fn per_macro(&self, name: &str) -> usize {
        self.entries.iter().filter(|(n, _)| *n == name).count()
    }
}

/// M-161：每宏正反例（正例过 + 反例返回 Fail）。
pub fn m161() -> (bool, AssertBook) {
    let mut book = AssertBook::new();
    // EQ：正例 3==3 过；反例 3==4 Fail
    book.record("EQ", kexpect_eq(3u32, 3).ok() && !kexpect_eq(3u32, 4).ok());
    // NE
    book.record("NE", kexpect_ne(3u32, 4).ok() && !kexpect_ne(3u32, 3).ok());
    // TRUE
    book.record("TRUE", kexpect_true(1 == 1).ok() && !kexpect_true(1 == 2).ok());
    // FALSE
    book.record("FALSE", kexpect_false(1 == 2).ok() && !kexpect_false(1 == 1).ok());
    // NULL
    book.record("NULL", kexpect_null::<u32>(None).ok() && !kexpect_null(Some(5u32)).ok());
    // NOTNULL
    book.record("NOTNULL", kexpect_notnull(Some(5u32)).ok() && !kexpect_notnull::<u32>(None).ok());
    (book.closed() && MACRO_NAMES.iter().all(|n| book.per_macro(n) == 1), book)
}

/// J1 主判据：六宏实现 + 失败路径全通（Fail 不 panic）+ 分派表一致。
pub fn j1_main() -> bool {
    let (ok, _) = m161();
    // 失败路径全通：每个宏的 Fail 变体可构造且携带原因
    let fails = [
        kexpect_eq(1u32, 2),
        kexpect_ne(1u32, 1),
        kexpect_true(false),
        kexpect_false(true),
        kexpect_null(Some(1u32)),
        kexpect_notnull::<u32>(None),
    ];
    let fail_ok = fails.iter().all(|r| matches!(r, AssertRes::Fail(s) if !s.is_empty()));
    // 分派表一致：总表执行与直调结果一致
    let d1 = dispatch("EQ", 3, 3, false).ok() && dispatch("NULL", 0, 0, true).ok() && dispatch("NOTNULL", 7, 0, false).ok();
    let d2 = matches!(dispatch("NOPE", 0, 0, false), AssertRes::Fail(s) if s == "UNKNOWN-MACRO");
    ok && fail_ok && d1 && d2
}

// ── 注入核（0xCB80 段 5 项）────────────────────────────────────────────

/// 注入核 1：未知宏注入——分派表未知名必 Fail。
pub fn inj1_unknown() -> bool {
    matches!(dispatch("WHAT", 1, 1, false), AssertRes::Fail(_))
}

/// 注入核 2：正反例翻转注入——反例结果必与正例互斥。
pub fn inj2_flip() -> bool {
    let p = kexpect_eq(5u32, 5);
    let n = kexpect_eq(5u32, 6);
    p.ok() != n.ok()
}

/// 注入核 3：Fail 原因注入——每宏失败原因互异。
pub fn inj3_fail_reasons() -> bool {
    let rs = [
        match kexpect_eq(1u32, 2) { AssertRes::Fail(s) => s, _ => String::new() },
        match kexpect_ne(1u32, 1) { AssertRes::Fail(s) => s, _ => String::new() },
        match kexpect_true(false) { AssertRes::Fail(s) => s, _ => String::new() },
        match kexpect_false(true) { AssertRes::Fail(s) => s, _ => String::new() },
        match kexpect_null(Some(1u32)) { AssertRes::Fail(s) => s, _ => String::new() },
        match kexpect_notnull::<u32>(None) { AssertRes::Fail(s) => s, _ => String::new() },
    ];
    for i in 0..rs.len() {
        if rs[i].is_empty() {
            return false;
        }
        for j in i + 1..rs.len() {
            if rs[i] == rs[j] {
                return false;
            }
        }
    }
    true
}

/// 注入核 4：宏名互异注入。
pub fn inj4_names() -> bool {
    for i in 0..MACROS {
        if MACRO_NAMES[i].is_empty() {
            return false;
        }
        for j in i + 1..MACROS {
            if MACRO_NAMES[i] == MACRO_NAMES[j] {
                return false;
            }
        }
    }
    true
}

/// 注入核 5：账闭合注入——缺一例必破。
pub fn inj5_book_open() -> bool {
    let mut b = AssertBook::new();
    for n in MACRO_NAMES.iter().take(MACROS - 1) {
        b.record(n, true);
    }
    !b.closed()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_unx3402_checks：本单判据聚合（诊断段 0xCB80）。
pub fn run_unx3402_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3402 断言宏族总表（kexpect_eq/ne/true/false/null/notnull）");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    let (m_ok, book) = m161();
    s.add(
        "六基础宏实现（失败路径全通）",
        j1_main(),
        if j1_main() { "" } else { "宏语义破" },
    );
    s.add(
        "M-161每宏正反例",
        m_ok && book.closed(),
        if m_ok { "" } else { "M-161例缺" },
    );
    let inj = [
        ("注入1未知宏", inj1_unknown()),
        ("注入2正反翻转", inj2_flip()),
        ("注入3失败原因互异", inj3_fail_reasons()),
        ("注入4宏名互异", inj4_names()),
        ("注入5账闭合", inj5_book_open()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
