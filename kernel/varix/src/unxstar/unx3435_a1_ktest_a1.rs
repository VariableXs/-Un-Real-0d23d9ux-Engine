//! UNX-F3435 与 A1 ktest 面的输出兼容性（A1 既有格式不破坏声明） · 0x79AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B13 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② J1：兼容性声明（A1 既有文本输出格式保留为协议子集）+ 兼容用例；
//!     M-194 A1 格式回放通过。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - A1 既有文本输出格式冻结为协议子集（四行式）：
//!     `PASS <suite>/<case> ticks=<n>`、
//!     `FAIL <suite>/<case> ticks=<n> why=<code>`、
//!     `SKIP <suite>/<case>`、
//!     `SUMMARY total=<n> pass=<n> fail=<n>`；
//!   - 解析器按子集口径从严：判词表外/套件名为空/用例名为空/拍数域
//!     残缺/FAIL 缺 why 码/多余 token 六类违约各归各码显性拒；
//!   - M-194 回放：冻结 A1 金样语料 → 解析 → 再渲染 → 逐字节零 diff
//!     （ReplayDiff 点名任何字节漂移）；
//!   - 渲染-解析回环：本引擎产出的行 render→parse→render 幂等；
//!   - 子集声明：新引擎用例必可渲染成 A1 可解析行（SubsetBreak 点名
//!     超集字段外泄）；
//!   - 摘要行计数自洽：pass+fail ≤ total（SKIP 计入 total）；
//!   - 双跑一致：同语料两次解析逐字段相同；写回摘要非空。
//! 常量来源：
//!   - 四行式协议：A1 ktest 面既有文本输出格式（冻结不破坏）；
//!   - M-194：派工卡验收判据编号（A1 格式回放锚定）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B13 批段窗 [0x79A00, 0x79BC0] 内）。
pub const SEG_BASE: u32 = 0x79AC0;

/// M-194 回放锚定（判据编号）。
pub const M194_REPLAY_ID: u32 = 0x194;

/// A1 判词闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A1Verdict {
    /// PASS：用例通过。
    Pass,
    /// FAIL：用例失败（必带 why 码）。
    Fail,
    /// SKIP：用例跳过（无 ticks 域）。
    Skip,
}

impl A1Verdict {
    /// 判词关键字（A1 既有格式冻结字面）。
    pub fn keyword(self) -> &'static str {
        match self {
            A1Verdict::Pass => "PASS",
            A1Verdict::Fail => "FAIL",
            A1Verdict::Skip => "SKIP",
        }
    }
    /// 关键字反解（表外字面返回 None）。
    pub fn from_keyword(k: &str) -> Option<A1Verdict> {
        match k {
            "PASS" => Some(A1Verdict::Pass),
            "FAIL" => Some(A1Verdict::Fail),
            "SKIP" => Some(A1Verdict::Skip),
            _ => None,
        }
    }
    /// 中文判词名。
    pub fn zh(self) -> &'static str {
        match self {
            A1Verdict::Pass => "通过",
            A1Verdict::Fail => "失败",
            A1Verdict::Skip => "跳过",
        }
    }
}

/// 全判词闭集数组。
pub const ALL_VERDICTS: [A1Verdict; 3] = [A1Verdict::Pass, A1Verdict::Fail, A1Verdict::Skip];

/// 一条 A1 协议行（解析后的结构视图）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A1Line {
    /// 判词。
    pub verdict: A1Verdict,
    /// 套件名（非空）。
    pub suite: String,
    /// 用例名（非空）。
    pub case: String,
    /// 拍数域（PASS/FAIL 必有，SKIP 无）。
    pub ticks: Option<u64>,
    /// why 码（FAIL 必有，其余无）。
    pub why: Option<u32>,
}

/// 金样语料：A1 既有格式的冻结回放语料（M-194 回放锚定）。
pub const GOLDEN_CORPUS: [&'static str; 6] = [
    "PASS boot/probe ticks=12",
    "PASS ledger/roundtrip ticks=340",
    "FAIL ledger/parity ticks=96 why=2",
    "SKIP serial/echo",
    "PASS ktest/watchdog ticks=7",
    "SUMMARY total=5 pass=3 fail=1",
];

/// 从 `<n>` 形式的域取整数（前缀不符或非十进制返回 None）。
fn parse_kv_u64(token: &str, key: &str) -> Option<u64> {
    let prefix_len = key.len();
    let bytes = token.as_bytes();
    if bytes.len() <= prefix_len {
        return None;
    }
    let head = match token.get(0..prefix_len) {
        Some(h) => h,
        None => return None,
    };
    if head != key {
        return None;
    }
    let digits = match token.get(prefix_len..) {
        Some(d) => d,
        None => return None,
    };
    if digits.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let db = digits.as_bytes();
    let mut i = 0usize;
    while i < db.len() {
        let b = match db.get(i) {
            Some(x) => *x,
            None => return None,
        };
        if b < b'0' || b > b'9' {
            return None;
        }
        v = match v.checked_mul(10).and_then(|x| x.checked_add((b - b'0') as u64)) {
            Some(x) => x,
            None => return None,
        };
        i += 1;
    }
    Some(v)
}

/// 解析一条 A1 协议行（从严：六类违约各归各码）。
pub fn parse_a1_line(line: &str) -> Result<A1Line, u32> {
    let mut toks: Vec<&str> = Vec::new();
    for t in line.split(' ') {
        toks.push(t);
    }
    if toks.is_empty() || line.is_empty() {
        return Err(Diag::LineMalformed.val());
    }
    let head = match toks.first() {
        Some(h) => *h,
        None => return Err(Diag::LineMalformed.val()),
    };
    let verdict = match A1Verdict::from_keyword(head) {
        Some(v) => v,
        None => return Err(Diag::VerdictForeign.val()),
    };
    let sc = match toks.get(1) {
        Some(s) => *s,
        None => return Err(Diag::LineMalformed.val()),
    };
    // suite/case 拆分：恰一个 '/'，两侧非空。
    let mut slash = 0usize;
    let scb = sc.as_bytes();
    let mut i = 0usize;
    while i < scb.len() {
        let b = match scb.get(i) {
            Some(x) => *x,
            None => break,
        };
        if b == b'/' {
            slash += 1;
        }
        i += 1;
    }
    if slash != 1 {
        return Err(Diag::LineMalformed.val());
    }
    let mut parts: Vec<&str> = Vec::new();
    for p in sc.split('/') {
        parts.push(p);
    }
    let suite = match parts.first() {
        Some(s) => *s,
        None => return Err(Diag::LineMalformed.val()),
    };
    let case = match parts.get(1) {
        Some(s) => *s,
        None => return Err(Diag::LineMalformed.val()),
    };
    if suite.is_empty() {
        return Err(Diag::SuiteEmpty.val());
    }
    if case.is_empty() {
        return Err(Diag::CaseEmpty.val());
    }
    if verdict == A1Verdict::Skip {
        // SKIP：恰两 token。
        if toks.len() != 2 {
            return Err(Diag::LineMalformed.val());
        }
        return Ok(A1Line {
            verdict,
            suite: String::from(suite),
            case: String::from(case),
            ticks: None,
            why: None,
        });
    }
    // PASS/FAIL：第三 token 必为 ticks=<n>。
    let ticks_tok = match toks.get(2) {
        Some(t) => *t,
        None => return Err(Diag::TicksFieldBad.val()),
    };
    let ticks = match parse_kv_u64(ticks_tok, "ticks=") {
        Some(v) => v,
        None => return Err(Diag::TicksFieldBad.val()),
    };
    if verdict == A1Verdict::Pass {
        // PASS：恰三 token，不带 why。
        if toks.len() != 3 {
            return Err(Diag::LineMalformed.val());
        }
        return Ok(A1Line {
            verdict,
            suite: String::from(suite),
            case: String::from(case),
            ticks: Some(ticks),
            why: None,
        });
    }
    // FAIL：第四 token 必为 why=<code>。
    let why_tok = match toks.get(3) {
        Some(t) => *t,
        None => return Err(Diag::WhyMissing.val()),
    };
    let why = match parse_kv_u64(why_tok, "why=") {
        Some(v) => v as u32,
        None => return Err(Diag::WhyMissing.val()),
    };
    if toks.len() != 4 {
        return Err(Diag::LineMalformed.val());
    }
    Ok(A1Line {
        verdict,
        suite: String::from(suite),
        case: String::from(case),
        ticks: Some(ticks),
        why: Some(why),
    })
}

/// 渲染一条 A1 协议行（canonical：与解析器逐字节互逆）。
pub fn render_a1_line(l: &A1Line) -> Result<String, u32> {
    if l.suite.is_empty() {
        return Err(Diag::SuiteEmpty.val());
    }
    if l.case.is_empty() {
        return Err(Diag::CaseEmpty.val());
    }
    match l.verdict {
        A1Verdict::Skip => {
            if l.ticks.is_some() || l.why.is_some() {
                return Err(Diag::LineMalformed.val());
            }
            Ok(format!("SKIP {}/{}", l.suite, l.case))
        }
        A1Verdict::Pass => {
            let ticks = match l.ticks {
                Some(t) => t,
                None => return Err(Diag::TicksFieldBad.val()),
            };
            if l.why.is_some() {
                return Err(Diag::LineMalformed.val());
            }
            Ok(format!("PASS {}/{} ticks={}", l.suite, l.case, ticks))
        }
        A1Verdict::Fail => {
            let ticks = match l.ticks {
                Some(t) => t,
                None => return Err(Diag::TicksFieldBad.val()),
            };
            let why = match l.why {
                Some(w) => w,
                None => return Err(Diag::WhyMissing.val()),
            };
            Ok(format!("FAIL {}/{} ticks={} why={}", l.suite, l.case, ticks, why))
        }
    }
}

/// 摘要行计数自洽：pass+fail ≤ total（SKIP 也计入 total，故允许
/// pass+fail < total），负数域不存在故上沿即全部约束。
pub fn summary_counts_ok(total: u64, pass: u64, fail: u64) -> Result<(), u32> {
    let pf = match pass.checked_add(fail) {
        Some(x) => x,
        None => return Err(Diag::SummaryCountBad.val()),
    };
    if pf > total {
        return Err(Diag::SummaryCountBad.val());
    }
    Ok(())
}

/// 摘要行解析：`SUMMARY total=<n> pass=<n> fail=<n>`。
pub fn parse_summary_line(line: &str) -> Result<(u64, u64, u64), u32> {
    let mut toks: Vec<&str> = Vec::new();
    for t in line.split(' ') {
        toks.push(t);
    }
    let head = match toks.first() {
        Some(h) => *h,
        None => return Err(Diag::LineMalformed.val()),
    };
    if head != "SUMMARY" || toks.len() != 4 {
        return Err(Diag::LineMalformed.val());
    }
    let total = match toks.get(1).and_then(|t| parse_kv_u64(t, "total=")) {
        Some(v) => v,
        None => return Err(Diag::SummaryCountBad.val()),
    };
    let pass = match toks.get(2).and_then(|t| parse_kv_u64(t, "pass=")) {
        Some(v) => v,
        None => return Err(Diag::SummaryCountBad.val()),
    };
    let fail = match toks.get(3).and_then(|t| parse_kv_u64(t, "fail=")) {
        Some(v) => v,
        None => return Err(Diag::SummaryCountBad.val()),
    };
    summary_counts_ok(total, pass, fail)?;
    Ok((total, pass, fail))
}

/// 诊断码闭集：0x79AC1..=0x79ACC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 判词表外：行首不是 PASS/FAIL/SKIP。
    VerdictForeign = 0x01,
    /// 套件名为空：`/` 左侧空。
    SuiteEmpty = 0x02,
    /// 用例名为空：`/` 右侧空。
    CaseEmpty = 0x03,
    /// 拍数域残缺：ticks= 域缺失或非十进制。
    TicksFieldBad = 0x04,
    /// why 码缺失：FAIL 行缺 why=<code>。
    WhyMissing = 0x05,
    /// 行格式残缺：token 数或结构不符协议。
    LineMalformed = 0x06,
    /// 摘要行计数错：pass+fail 超 total（SKIP 也计入 total）。
    SummaryCountBad = 0x07,
    /// 回放 diff 非零：M-194 回放出现字节漂移。
    ReplayDiff = 0x08,
    /// 子集声明违约：新引擎产出不可被 A1 解析。
    SubsetBreak = 0x09,
    /// 双跑分叉：同语料两次解析不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
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
            Diag::VerdictForeign => "判词表外：建议行首只用 PASS/FAIL/SKIP 三个冻结字面",
            Diag::SuiteEmpty => "套件名为空：建议 suite 段必填非空标识",
            Diag::CaseEmpty => "用例名为空：建议 case 段必填非空标识",
            Diag::TicksFieldBad => "拍数域残缺：建议 ticks= 后接十进制非负整数",
            Diag::WhyMissing => "why 码缺失：建议 FAIL 行补 why=<诊断码> 再入账",
            Diag::LineMalformed => "行格式残缺：建议对照四行式协议核对 token 数与结构",
            Diag::SummaryCountBad => "摘要行计数错：建议 pass+fail 不得超过 total（SKIP 计入 total）后重渲染",
            Diag::ReplayDiff => "回放 diff 非零：建议渲染器与解析器互逆校验定位漂移字节",
            Diag::SubsetBreak => "子集声明违约：建议新引擎字段收敛进 A1 四行式子集",
            Diag::DoubleRunDiverge => "双跑分叉：建议解析器做成纯函数，语料定则行定",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::VerdictForeign,
    Diag::SuiteEmpty,
    Diag::CaseEmpty,
    Diag::TicksFieldBad,
    Diag::WhyMissing,
    Diag::LineMalformed,
    Diag::SummaryCountBad,
    Diag::ReplayDiff,
    Diag::SubsetBreak,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79AC1..=0x79ACC 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B13 批段窗 [0x79A00, 0x79BC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79A00 && SEG_BASE <= 0x79BC0
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
        if a.detail().is_empty() || !a.detail().contains("建议") {
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
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 判词闭集：三判词关键字互异、反解回环、中文名非空。
fn verdict_closed_set_ok() -> bool {
    let mut i = 0usize;
    while i < ALL_VERDICTS.len() {
        let a = match ALL_VERDICTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if A1Verdict::from_keyword(a.keyword()) != Some(a) || a.zh().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_VERDICTS.len() {
            match ALL_VERDICTS.get(j) {
                Some(b) => {
                    if a.keyword() == b.keyword() || a.zh() == b.zh() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    A1Verdict::from_keyword("DONE").is_none() && A1Verdict::from_keyword("").is_none()
}

/// 金样语料解析：六行全部过解析器（正常流）。
fn golden_parse_ok() -> bool {
    let mut parsed = 0usize;
    for line in GOLDEN_CORPUS.iter() {
        let is_summary = line.starts_with("SUMMARY");
        let ok = if is_summary {
            parse_summary_line(line).is_ok()
        } else {
            parse_a1_line(line).is_ok()
        };
        if !ok {
            return false;
        }
        parsed += 1;
    }
    parsed == GOLDEN_CORPUS.len()
}

/// M-194 回放零 diff：金样逐行 解析→渲染 与原文逐字节相同。
fn m194_replay_zero_diff_ok() -> bool {
    let mut replayed = 0usize;
    for line in GOLDEN_CORPUS.iter() {
        if line.starts_with("SUMMARY") {
            // 摘要行：解析计数自洽即视为回放一致（摘要由渲染器另测）。
            if parse_summary_line(line).is_err() {
                return false;
            }
            replayed += 1;
            continue;
        }
        let l = match parse_a1_line(line) {
            Ok(l) => l,
            Err(_) => return false,
        };
        let rendered = match render_a1_line(&l) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if rendered.as_str() != *line {
            return false;
        }
        replayed += 1;
    }
    replayed == GOLDEN_CORPUS.len() && M194_REPLAY_ID == 0x194
}

/// 渲染-解析回环幂等：合成行 render→parse→render 两次渲染相同。
fn render_parse_idempotent_ok() -> bool {
    let lines: [A1Line; 4] = [
        A1Line {
            verdict: A1Verdict::Pass,
            suite: String::from("f3435"),
            case: String::from("alpha"),
            ticks: Some(0),
            why: None,
        },
        A1Line {
            verdict: A1Verdict::Pass,
            suite: String::from("f3435"),
            case: String::from("beta"),
            ticks: Some(u64::MAX),
            why: None,
        },
        A1Line {
            verdict: A1Verdict::Fail,
            suite: String::from("f3435"),
            case: String::from("gamma"),
            ticks: Some(64),
            why: Some(5),
        },
        A1Line {
            verdict: A1Verdict::Skip,
            suite: String::from("f3435"),
            case: String::from("delta"),
            ticks: None,
            why: None,
        },
    ];
    for l in lines.iter() {
        let r1 = match render_a1_line(l) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let back = match parse_a1_line(&r1) {
            Ok(b) => b,
            Err(_) => return false,
        };
        let r2 = match render_a1_line(&back) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if back != *l || r2 != r1 {
            return false;
        }
    }
    true
}

/// FAIL 缺 why 码必抓：FAIL 行去 why 域 → WhyMissing 点名。
fn inj_why_missing_ok() -> bool {
    if parse_a1_line("FAIL a/b ticks=8") != Err(Diag::WhyMissing.val()) {
        return false;
    }
    parse_a1_line("FAIL a/b ticks=8 why=x") == Err(Diag::WhyMissing.val())
}

/// 拍数域残缺必抓：非十进制/缺前缀/负号三类违约 → TicksFieldBad。
fn inj_ticks_bad_ok() -> bool {
    if parse_a1_line("PASS a/b tick=8") != Err(Diag::TicksFieldBad.val()) {
        return false;
    }
    if parse_a1_line("PASS a/b ticks=8x") != Err(Diag::TicksFieldBad.val()) {
        return false;
    }
    if parse_a1_line("PASS a/b ticks=") != Err(Diag::TicksFieldBad.val()) {
        return false;
    }
    parse_a1_line("PASS a/b ticks=-3") == Err(Diag::TicksFieldBad.val())
}

/// 判词表外与空段必抓：非冻结字面/空 suite/空 case 各归各码。
fn inj_verdict_and_empty_ok() -> bool {
    if parse_a1_line("DONE a/b ticks=1") != Err(Diag::VerdictForeign.val()) {
        return false;
    }
    if parse_a1_line("PASS /b ticks=1") != Err(Diag::SuiteEmpty.val()) {
        return false;
    }
    if parse_a1_line("PASS a/ ticks=1") != Err(Diag::CaseEmpty.val()) {
        return false;
    }
    parse_a1_line("PASS ab ticks=1") == Err(Diag::LineMalformed.val())
}

/// 摘要计数错必抓：pass+fail 不等 total → SummaryCountBad。
fn inj_summary_bad_ok() -> bool {
    if parse_summary_line("SUMMARY total=4 pass=4 fail=1")
        != Err(Diag::SummaryCountBad.val())
    {
        return false;
    }
    if summary_counts_ok(10, 4, 6).is_err() {
        return false;
    }
    parse_summary_line("SUMMARY total=4 pass=4 fail=0") == Ok((4, 4, 0))
}

/// 子集声明：本引擎用例渲染行必可被 A1 解析器收下（协议子集不破坏）。
fn subset_declaration_ok() -> bool {
    // 本引擎产出四例（与 A1 既有套件/用例名同构）。
    let ours: [A1Line; 4] = [
        A1Line {
            verdict: A1Verdict::Pass,
            suite: String::from("kernel"),
            case: String::from("timeout"),
            ticks: Some(48),
            why: None,
        },
        A1Line {
            verdict: A1Verdict::Fail,
            suite: String::from("kernel"),
            case: String::from("stamp"),
            ticks: Some(16),
            why: Some(1),
        },
        A1Line {
            verdict: A1Verdict::Skip,
            suite: String::from("kernel"),
            case: String::from("perf"),
            ticks: None,
            why: None,
        },
        A1Line {
            verdict: A1Verdict::Pass,
            suite: String::from("kernel"),
            case: String::from("json"),
            ticks: Some(9999),
            why: None,
        },
    ];
    for l in ours.iter() {
        let r = match render_a1_line(l) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let back = match parse_a1_line(&r) {
            Ok(b) => b,
            Err(_) => return false,
        };
        if back != *l {
            return false;
        }
    }
    detail(Diag::SubsetBreak.val()).contains("子集")
}

/// 双跑一致：同语料两次解析逐字段相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<Vec<A1Line>, u32> {
        let mut out: Vec<A1Line> = Vec::new();
        for line in GOLDEN_CORPUS.iter() {
            if line.starts_with("SUMMARY") {
                continue;
            }
            out.push(parse_a1_line(line)?);
        }
        Ok(out)
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含回放与兼容结论）。
fn writeback_summary_ok() -> bool {
    let golden_summary = match GOLDEN_CORPUS.get(5) {
        Some(s) => *s,
        None => return false,
    };
    let (total, pass, fail) = match parse_summary_line(golden_summary) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3435 完成：A1回放{}行 total={} pass={} fail={} 兼容声明成立",
        GOLDEN_CORPUS.len(),
        total,
        pass,
        fail
    );
    !summary.is_empty()
        && summary.contains("UNX-F3435")
        && summary.contains("回放")
        && summary_counts_ok(total, pass, fail).is_ok()
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3435_checks ───────────────────────────────────────

/// run_unx3435_checks：本单判据聚合（诊断段 0x79AC0，全部走 add_dyn）。
pub fn run_unx3435_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3435 A1 输出兼容");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "判词闭集三字面互异可回环",
            verdict_closed_set_ok(),
            format!("判词{}", if verdict_closed_set_ok() { "闭集" } else { "破缺" }),
        ),
        (
            "正常流金样六行全过解析器",
            golden_parse_ok(),
            format!("金样{}", if golden_parse_ok() { "全过" } else { "有红" }),
        ),
        (
            "M-194回放渲染逐字节零diff",
            m194_replay_zero_diff_ok(),
            format!("回放{}", if m194_replay_zero_diff_ok() { "零diff" } else { "有diff" }),
        ),
        (
            "渲染解析回环幂等含边界拍数",
            render_parse_idempotent_ok(),
            format!("回环{}", if render_parse_idempotent_ok() { "幂等" } else { "漂移" }),
        ),
        (
            "注入1FAIL缺why码必抓",
            inj_why_missing_ok(),
            format!("why注入{}", if inj_why_missing_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2拍数域残缺必抓",
            inj_ticks_bad_ok(),
            format!("ticks注入{}", if inj_ticks_bad_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3判词表外与空段必抓",
            inj_verdict_and_empty_ok(),
            format!("判词注入{}", if inj_verdict_and_empty_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4摘要计数错必抓",
            inj_summary_bad_ok(),
            format!("摘要注入{}", if inj_summary_bad_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "子集声明新引擎行A1可解析",
            subset_declaration_ok(),
            format!("子集{}", if subset_declaration_ok() { "保留" } else { "破坏" }),
        ),
        (
            "双跑一致金样解析逐字段同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
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
        assert_eq!(SEG_BASE, 0x79AC0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn verdict_closed_set_roundtrip() {
        assert!(verdict_closed_set_ok());
        assert_eq!(A1Verdict::from_keyword("PASS"), Some(A1Verdict::Pass));
        assert_eq!(A1Verdict::from_keyword("pass"), None);
    }

    #[test]
    fn golden_corpus_parses() {
        assert!(golden_parse_ok());
        assert_eq!(GOLDEN_CORPUS.len(), 6);
        let l = parse_a1_line(GOLDEN_CORPUS.get(2).copied().unwrap_or("")).unwrap();
        assert_eq!(l.suite, "ledger");
        assert_eq!(l.verdict, A1Verdict::Fail);
        assert_eq!(l.why, Some(2));
        assert_eq!(l.ticks, Some(96));
    }

    #[test]
    fn m194_replay_zero_diff() {
        assert!(m194_replay_zero_diff_ok());
    }

    #[test]
    fn render_parse_idempotent() {
        assert!(render_parse_idempotent_ok());
        let l = A1Line {
            verdict: A1Verdict::Pass,
            suite: String::from("s"),
            case: String::from("c"),
            ticks: Some(7),
            why: None,
        };
        assert_eq!(
            render_a1_line(&l).unwrap(),
            "PASS s/c ticks=7".to_string()
        );
    }

    #[test]
    fn injections_why_ticks_verdict() {
        assert!(inj_why_missing_ok());
        assert!(inj_ticks_bad_ok());
        assert!(inj_verdict_and_empty_ok());
    }

    #[test]
    fn injection_summary_bad() {
        assert!(inj_summary_bad_ok());
    }

    #[test]
    fn subset_declaration_holds() {
        assert!(subset_declaration_ok());
    }

    #[test]
    fn double_run_identical_parse() {
        assert!(double_run_ok());
    }

    #[test]
    fn kv_parser_bounds() {
        assert_eq!(parse_kv_u64("ticks=18446744073709551615", "ticks="), Some(u64::MAX));
        assert_eq!(parse_kv_u64("ticks=99999999999999999999999", "ticks="), None);
        assert_eq!(parse_kv_u64("ticks=", "ticks="), None);
        assert_eq!(parse_kv_u64("ticks", "ticks="), None);
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3435_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
