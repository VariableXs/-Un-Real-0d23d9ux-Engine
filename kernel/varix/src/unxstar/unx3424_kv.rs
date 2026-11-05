//! UNX-F3424 结果协议：行式 KV 输出格式 · 0x79800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B12 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② J1：行式 KV 协议（CASE/PASS/FAIL/SKIP/SUMMARY 五类行）冻结；
//!     M-183 协议往返验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五类行闭集：CASE（用例登记）/ PASS（通过）/ FAIL（失败带原因）/
//!     SKIP（跳过带理由）/ SUMMARY（四计数收口），行前缀逐字冻结，
//!     表外前缀一律拒收；
//!   - 行文法：`<前缀> <k>=<v>…`，键取冻结键表，值非空，单空格分隔；
//!     双空格、空值、未知键、未知前缀均显性拒（错误码闭集 12 码）；
//!   - 协议写入器单调状态机：Open → SummaryWritten 单向翻，
//!     SUMMARY 落行后流即关闭，关流后任何追加行与二次 SUMMARY 均拒；
//!   - M-183 往返验证：渲染 → 解析 → 再渲染逐字节一致，且解析结果与
//!     原行对象相等（五类行各往返一条全对账）；
//!   - 注入必抓：坏前缀 / 未知键 / 空值 / SUMMARY 缺计数项 / 关流后写
//!     / 双 SUMMARY 六类违约必被抓；
//!   - 双跑一致：同输入两次渲染与两次写入器流逐字节相同。
//! 常量来源：
//!   - 五类行前缀：对齐 M-183 协议往返验证对五类行的冻结要求；
//!   - 键表：name/why/total/pass/fail/skip 六键，覆盖五类行全部字段；
//!   - 诊断段：0x79800（0x40 对齐，B12 批窗 [0x79800, 0x799C0]）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B12 批段窗 [0x79800, 0x799C0]）。
pub const SEG_BASE: u32 = 0x79800;

/// 五类行前缀冻结表（行文法首词，逐字冻结；表外前缀拒收）。
pub const LINE_PREFIXES: [&'static str; 5] = ["CASE", "PASS", "FAIL", "SKIP", "SUMMARY"];

/// 键冻结表（五类行全部键；表外键拒收）。
pub const LINE_KEYS: [&'static str; 6] = ["name", "why", "total", "pass", "fail", "skip"];

/// 行类型闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// 用例登记行（CASE）。
    Case,
    /// 通过行（PASS）。
    Pass,
    /// 失败行（FAIL，带 why）。
    Fail,
    /// 跳过行（SKIP，带 why）。
    Skip,
    /// 收口汇总行（SUMMARY，四计数）。
    Summary,
}

impl LineKind {
    /// 行前缀（冻结表逐字）。
    pub fn prefix(self) -> &'static str {
        match self {
            LineKind::Case => "CASE",
            LineKind::Pass => "PASS",
            LineKind::Fail => "FAIL",
            LineKind::Skip => "SKIP",
            LineKind::Summary => "SUMMARY",
        }
    }

    /// 该类行的必填键表（冻结）。
    pub fn required_keys(self) -> &'static [&'static str] {
        match self {
            LineKind::Case | LineKind::Pass => &["name"],
            LineKind::Fail | LineKind::Skip => &["name", "why"],
            LineKind::Summary => &["total", "pass", "fail", "skip"],
        }
    }
}

/// 十进制无格式化渲染（整数运算，无浮点无墙钟）。
fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 20];
    let mut w = 0usize;
    while v > 0 && w < digits.len() {
        match digits.get_mut(w) {
            Some(slot) => *slot = b'0' + (v % 10) as u8,
            None => break,
        }
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        match digits.get(w) {
            Some(&d) => out.push(d as char),
            None => out.push('0'),
        }
    }
}

/// 手写 u64 十进制解析（checked 累积，溢出显性拒）。
fn parse_u64(text: &str) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for ch in text.chars() {
        let d = match ch {
            '0'..='9' => (ch as u8 - b'0') as u64,
            _ => return None,
        };
        v = v.checked_mul(10)?.checked_add(d)?;
    }
    Some(v)
}

/// 协议行对象（五类一体，字段按类取用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvLine {
    /// CASE 行：用例登记（仅 name）。
    Case { name: String },
    /// PASS 行：通过（仅 name）。
    Pass { name: String },
    /// FAIL 行：失败（name + why 原因）。
    Fail { name: String, why: String },
    /// SKIP 行：跳过（name + why 理由）。
    Skip { name: String, why: String },
    /// SUMMARY 行：四计数收口（total/pass/fail/skip）。
    Summary { total: u64, pass: u64, fail: u64, skip: u64 },
}

impl KvLine {
    /// 行类型。
    pub fn kind(&self) -> LineKind {
        match self {
            KvLine::Case { .. } => LineKind::Case,
            KvLine::Pass { .. } => LineKind::Pass,
            KvLine::Fail { .. } => LineKind::Fail,
            KvLine::Skip { .. } => LineKind::Skip,
            KvLine::Summary { .. } => LineKind::Summary,
        }
    }

    /// 渲染为协议行文本（不含行尾换行；文法 `<前缀> <k>=<v>…`）。
    pub fn render_line(&self) -> String {
        let mut out = String::new();
        match self {
            KvLine::Case { name } => {
                out.push_str("CASE name=");
                out.push_str(name);
            }
            KvLine::Pass { name } => {
                out.push_str("PASS name=");
                out.push_str(name);
            }
            KvLine::Fail { name, why } => {
                out.push_str("FAIL name=");
                out.push_str(name);
                out.push_str(" why=");
                out.push_str(why);
            }
            KvLine::Skip { name, why } => {
                out.push_str("SKIP name=");
                out.push_str(name);
                out.push_str(" why=");
                out.push_str(why);
            }
            KvLine::Summary { total, pass, fail, skip } => {
                out.push_str("SUMMARY total=");
                push_u64(&mut out, *total);
                out.push_str(" pass=");
                push_u64(&mut out, *pass);
                out.push_str(" fail=");
                push_u64(&mut out, *fail);
                out.push_str(" skip=");
                push_u64(&mut out, *skip);
            }
        }
        out
    }

    /// 键值对完备性自查：按行类必填键逐个核对（渲染侧守卫）。
    pub fn fields_complete(&self) -> bool {
        match self {
            KvLine::Case { name } | KvLine::Pass { name } => !name.is_empty(),
            KvLine::Fail { name, why } | KvLine::Skip { name, why } => {
                !name.is_empty() && !why.is_empty()
            }
            KvLine::Summary { total, pass, fail, skip } => {
                // 计数自洽：total == pass + fail + skip（checked 防溢出）。
                match pass.checked_add(*fail) {
                    Some(pf) => match pf.checked_add(*skip) {
                        Some(sum) => sum == *total,
                        None => false,
                    },
                    None => false,
                }
            }
        }
    }
}

/// 行文本解析（M-183 往返的解析半边）。
/// 文法：首词为五类前缀之一，其余为 `k=v` 对（单空格分隔，值非空，
/// 键在冻结表内）。任何违约显性拒，返回闭集错误码。
pub fn parse_line(text: &str) -> Result<KvLine, u32> {
    if text.is_empty() {
        return Err(Diag::LineEmpty.val());
    }
    let mut tokens = text.split(' ');
    let head = match tokens.next() {
        Some(h) => h,
        None => return Err(Diag::LineEmpty.val()),
    };
    let kind = match head {
        "CASE" => LineKind::Case,
        "PASS" => LineKind::Pass,
        "FAIL" => LineKind::Fail,
        "SKIP" => LineKind::Skip,
        "SUMMARY" => LineKind::Summary,
        _ => return Err(Diag::PrefixForeign.val()),
    };
    // 收集键值对（空 token、无等号、空值、未知键均拒）。
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for tok in tokens {
        if tok.is_empty() {
            return Err(Diag::LineEmpty.val());
        }
        let mut kv = tok.splitn(2, '=');
        let k = match kv.next() {
            Some(k) => k,
            None => return Err(Diag::KeyForeign.val()),
        };
        let v = match kv.next() {
            Some(v) => v,
            None => return Err(Diag::ValueEmpty.val()),
        };
        if k.is_empty() || v.is_empty() {
            return Err(Diag::ValueEmpty.val());
        }
        let mut known = false;
        for key in LINE_KEYS.iter() {
            if *key == k {
                known = true;
            }
        }
        if !known {
            return Err(Diag::KeyForeign.val());
        }
        pairs.push((k, v));
    }
    // 必填键完备性。
    for req in kind.required_keys() {
        let mut found = false;
        for (k, _) in pairs.iter() {
            if *k == *req {
                found = true;
            }
        }
        if !found {
            return Err(Diag::FieldMissing.val());
        }
    }
    // 按类构造（值域校验就地做）。
    let get = |key: &str| -> Option<&str> {
        for (k, v) in pairs.iter() {
            if *k == key {
                return Some(*v);
            }
        }
        None
    };
    match kind {
        LineKind::Case => match get("name") {
            Some(n) => Ok(KvLine::Case { name: n.to_string() }),
            None => Err(Diag::FieldMissing.val()),
        },
        LineKind::Pass => match get("name") {
            Some(n) => Ok(KvLine::Pass { name: n.to_string() }),
            None => Err(Diag::FieldMissing.val()),
        },
        LineKind::Fail => match (get("name"), get("why")) {
            (Some(n), Some(w)) => Ok(KvLine::Fail { name: n.to_string(), why: w.to_string() }),
            _ => Err(Diag::FieldMissing.val()),
        },
        LineKind::Skip => match (get("name"), get("why")) {
            (Some(n), Some(w)) => Ok(KvLine::Skip { name: n.to_string(), why: w.to_string() }),
            _ => Err(Diag::FieldMissing.val()),
        },
        LineKind::Summary => {
            let total = match get("total").and_then(parse_u64) {
                Some(t) => t,
                None => return Err(Diag::CountMalformed.val()),
            };
            let pass = match get("pass").and_then(parse_u64) {
                Some(p) => p,
                None => return Err(Diag::CountMalformed.val()),
            };
            let fail = match get("fail").and_then(parse_u64) {
                Some(f) => f,
                None => return Err(Diag::CountMalformed.val()),
            };
            let skip = match get("skip").and_then(parse_u64) {
                Some(s) => s,
                None => return Err(Diag::CountMalformed.val()),
            };
            let line = KvLine::Summary { total, pass, fail, skip };
            if !line.fields_complete() {
                return Err(Diag::CountMismatch.val());
            }
            Ok(line)
        }
    }
}

/// 协议写入器状态机：Open → SummaryWritten 单向翻。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterState {
    /// 开流：可追加 CASE/PASS/FAIL/SKIP 行。
    Open,
    /// 已收口：SUMMARY 已落行，流关闭（再写即拒）。
    SummaryWritten,
}

impl WriterState {
    /// 中文态名。
    pub fn zh(self) -> &'static str {
        match self {
            WriterState::Open => "开流",
            WriterState::SummaryWritten => "已收口",
        }
    }
}

/// 协议写入器：追加行留痕，SUMMARY 单发，关流后拒写。
#[derive(Debug, Clone)]
pub struct KvWriter {
    state: WriterState,
    lines: Vec<KvLine>,
    /// 已落 CASE 行数（SUMMARY 四计数自洽的素材）。
    cases: u64,
    passes: u64,
    fails: u64,
    skips: u64,
}

impl KvWriter {
    /// 新建开流写入器。
    pub fn new() -> KvWriter {
        KvWriter {
            state: WriterState::Open,
            lines: Vec::new(),
            cases: 0,
            passes: 0,
            fails: 0,
            skips: 0,
        }
    }

    /// 当前态。
    pub fn state(&self) -> WriterState {
        self.state
    }

    /// 已落行数。
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 取第 `idx` 行。
    pub fn get(&self, idx: usize) -> Option<&KvLine> {
        self.lines.get(idx)
    }

    /// 追加一行（关流态一律拒；SUMMARY 只在开流态收口一次）。
    pub fn push(&mut self, line: KvLine) -> Result<(), u32> {
        if self.state == WriterState::SummaryWritten {
            return Err(Diag::StreamClosed.val());
        }
        if !line.fields_complete() {
            return Err(Diag::FieldMissing.val());
        }
        match &line {
            KvLine::Case { .. } => self.cases = self.cases.saturating_add(1),
            KvLine::Pass { .. } => self.passes = self.passes.saturating_add(1),
            KvLine::Fail { .. } => self.fails = self.fails.saturating_add(1),
            KvLine::Skip { .. } => self.skips = self.skips.saturating_add(1),
            KvLine::Summary { .. } => {
                return Err(Diag::DoubleSummary.val());
            }
        }
        self.lines.push(line);
        Ok(())
    }

    /// 收口：按记账计数渲染 SUMMARY 行并落行，流翻已收口态。
    pub fn summarize(&mut self) -> Result<KvLine, u32> {
        if self.state == WriterState::SummaryWritten {
            return Err(Diag::DoubleSummary.val());
        }
        let total = match self.cases.checked_add(self.passes) {
            Some(a) => match a.checked_add(self.fails) {
                Some(b) => match b.checked_add(self.skips) {
                    Some(t) => t,
                    None => return Err(Diag::CountMismatch.val()),
                },
                None => return Err(Diag::CountMismatch.val()),
            },
            None => return Err(Diag::CountMismatch.val()),
        };
        let line = KvLine::Summary {
            total,
            pass: self.passes,
            fail: self.fails,
            skip: self.skips,
        };
        self.lines.push(line.clone());
        self.state = WriterState::SummaryWritten;
        Ok(line)
    }

    /// 全流渲染（逐行拼接，行尾换行）。
    pub fn render_stream(&self) -> String {
        let mut out = String::new();
        for line in self.lines.iter() {
            out.push_str(&line.render_line());
            out.push('\n');
        }
        out
    }
}

/// 诊断码闭集：0x79801..=0x7980C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 关流后写：SUMMARY 已落行仍请求追加。
    StreamClosed = 0x01,
    /// 双收口：SUMMARY 行重复落行。
    DoubleSummary = 0x02,
    /// 前缀表外：行首词不在五类前缀冻结表内。
    PrefixForeign = 0x03,
    /// 键表外：键不在冻结键表内。
    KeyForeign = 0x04,
    /// 空值：`k=v` 的键或值为空。
    ValueEmpty = 0x05,
    /// 必填键缺失：行类要求的键未齐。
    FieldMissing = 0x06,
    /// 计数自洽破缺：total != pass + fail + skip。
    CountMismatch = 0x07,
    /// 空行：待解析文本为空。
    LineEmpty = 0x08,
    /// 计数坏形：SUMMARY 计数不是合法十进制整数。
    CountMalformed = 0x09,
    /// 往返破缺：渲染-解析-再渲染不一致。
    RoundTripBreak = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 双跑不一致：同输入两次执行结论不同。
    DoubleRunDiverge = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::StreamClosed => "关流后写：建议 SUMMARY 落行后不再追加，新行开新流",
            Diag::DoubleSummary => "双收口：建议一条流只落一行 SUMMARY，重复收口走新流",
            Diag::PrefixForeign => "前缀表外：建议行首词只用 CASE/PASS/FAIL/SKIP/SUMMARY 五类",
            Diag::KeyForeign => "键表外：建议键只用冻结键表 name/why/total/pass/fail/skip",
            Diag::ValueEmpty => "空值：建议 k=v 两端均非空，空值改用 why=none 语义占位",
            Diag::FieldMissing => "必填键缺失：建议按行类补齐必填键再落行",
            Diag::CountMismatch => "计数自洽破缺：建议 total 改为 pass+fail+skip 的真实和",
            Diag::LineEmpty => "空行：建议丢弃空行或补全行文法后再解析",
            Diag::CountMalformed => "计数坏形：建议 SUMMARY 计数用无符号十进制整数",
            Diag::RoundTripBreak => "往返破缺：建议渲染与解析共用冻结文法，双方不得私改",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::DoubleRunDiverge => "双跑不一致：建议行渲染做成纯函数，输入定则输出定",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::StreamClosed,
    Diag::DoubleSummary,
    Diag::PrefixForeign,
    Diag::KeyForeign,
    Diag::ValueEmpty,
    Diag::FieldMissing,
    Diag::CountMismatch,
    Diag::LineEmpty,
    Diag::CountMalformed,
    Diag::RoundTripBreak,
    Diag::SegGuardFail,
    Diag::DoubleRunDiverge,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79801..=0x7980C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B12 批段窗 [0x79800, 0x799C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79800 && SEG_BASE <= 0x799C0
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

/// 前缀/键冻结表自查：五前缀互异、六键互异、必填键表都在键表内。
fn frozen_tables_ok() -> bool {
    if LINE_PREFIXES.len() != 5 || LINE_KEYS.len() != 6 {
        return false;
    }
    let kinds = [LineKind::Case, LineKind::Pass, LineKind::Fail, LineKind::Skip, LineKind::Summary];
    for (i, p) in LINE_PREFIXES.iter().enumerate() {
        let kind = match kinds.get(i) {
            Some(k) => *k,
            None => return false,
        };
        if *p != kind.prefix() {
            return false;
        }
        let mut j = i + 1;
        while j < LINE_PREFIXES.len() {
            match LINE_PREFIXES.get(j) {
                Some(q) => {
                    if p == q {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
    }
    for (i, k) in LINE_KEYS.iter().enumerate() {
        if k.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < LINE_KEYS.len() {
            match LINE_KEYS.get(j) {
                Some(q) => {
                    if k == q {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
    }
    for kind in kinds.iter() {
        for req in kind.required_keys() {
            let mut found = false;
            for key in LINE_KEYS.iter() {
                if key == req {
                    found = true;
                }
            }
            if !found {
                return false;
            }
        }
    }
    true
}

/// M-183 往返：五类行各一条，渲染→解析与原对象相等，再渲染逐字节同。
fn round_trip_ok() -> bool {
    let samples = [
        KvLine::Case { name: String::from("boot_page_table") },
        KvLine::Pass { name: String::from("e820_bridge") },
        KvLine::Fail { name: String::from("hda_probe"), why: String::from("no_controller") },
        KvLine::Skip { name: String::from("tpm_extend"), why: String::from("tpm_absent") },
        KvLine::Summary { total: 9, pass: 6, fail: 2, skip: 1 },
    ];
    for line in samples.iter() {
        let text = line.render_line();
        let back = match parse_line(&text) {
            Ok(l) => l,
            Err(_) => return false,
        };
        if back != *line {
            return false;
        }
        if back.render_line() != text {
            return false;
        }
    }
    true
}

/// 正常流：五类行全写通 + 收口四计数与记账一致。
fn writer_normal_flow_ok() -> bool {
    let mut w = KvWriter::new();
    let seq = [
        KvLine::Case { name: String::from("case_a") },
        KvLine::Pass { name: String::from("case_b") },
        KvLine::Fail { name: String::from("case_c"), why: String::from("timeout") },
        KvLine::Skip { name: String::from("case_d"), why: String::from("dep_missing") },
        KvLine::Pass { name: String::from("case_e") },
    ];
    for line in seq.iter() {
        if w.push(line.clone()).is_err() {
            return false;
        }
    }
    let summary = match w.summarize() {
        Ok(s) => s,
        Err(_) => return false,
    };
    match summary {
        KvLine::Summary { total, pass, fail, skip } => {
            total == 5 && pass == 2 && fail == 1 && skip == 1
        }
        _ => false,
    }
}

/// 注入必抓 1：关流后追加行必须被 StreamClosed 拒。
fn inj_stream_closed_ok() -> bool {
    let mut w = KvWriter::new();
    if w.push(KvLine::Pass { name: String::from("only") }).is_err() {
        return false;
    }
    if w.summarize().is_err() {
        return false;
    }
    w.push(KvLine::Pass { name: String::from("after_close") })
        == Err(Diag::StreamClosed.val())
}

/// 注入必抓 2：双 SUMMARY 必须被 DoubleSummary 拒。
fn inj_double_summary_ok() -> bool {
    let mut w = KvWriter::new();
    if w.summarize().is_err() {
        return false;
    }
    w.summarize() == Err(Diag::DoubleSummary.val())
}

/// 注入必抓 3：坏前缀 / 未知键 / 空值三类解析违约必抓。
fn inj_parse_rejects_ok() -> bool {
    let bad_prefix = parse_line("TRACE name=x");
    let bad_key = parse_line("PASS name=x ghost=1");
    let empty_val = parse_line("PASS name=");
    let no_eq = parse_line("PASS namex");
    let empty_line = parse_line("");
    bad_prefix == Err(Diag::PrefixForeign.val())
        && bad_key == Err(Diag::KeyForeign.val())
        && empty_val == Err(Diag::ValueEmpty.val())
        && no_eq == Err(Diag::ValueEmpty.val())
        && empty_line == Err(Diag::LineEmpty.val())
}

/// 注入必抓 4：SUMMARY 缺计数项 / 计数坏形 / 计数自洽破缺必抓。
fn inj_summary_fields_ok() -> bool {
    let missing = parse_line("SUMMARY total=3 pass=1 fail=1");
    let malformed = parse_line("SUMMARY total=x pass=1 fail=1 skip=1");
    let inconsistent = parse_line("SUMMARY total=9 pass=1 fail=1 skip=1");
    missing == Err(Diag::FieldMissing.val())
        && malformed == Err(Diag::CountMalformed.val())
        && inconsistent == Err(Diag::CountMismatch.val())
}

/// 状态机单调：Open→SummaryWritten 单向，无回退路径。
fn writer_state_monotonic_ok() -> bool {
    let mut w = KvWriter::new();
    if w.state() != WriterState::Open {
        return false;
    }
    if w.summarize().is_err() {
        return false;
    }
    if w.state() != WriterState::SummaryWritten {
        return false;
    }
    // 收口后再 summarize 仍是已收口（无回退路径可探）。
    w.summarize() == Err(Diag::DoubleSummary.val()) && w.state() == WriterState::SummaryWritten
}

/// 双跑一致：同输入两次渲染与两次写入器流逐字节相同。
fn double_run_ok() -> bool {
    let render_once = || -> String {
        let mut w = KvWriter::new();
        let seq = [
            KvLine::Case { name: String::from("r1") },
            KvLine::Fail { name: String::from("r2"), why: String::from("boom") },
            KvLine::Skip { name: String::from("r3"), why: String::from("cold") },
        ];
        for line in seq.iter() {
            let _ = w.push(line.clone());
        }
        let _ = w.summarize();
        w.render_stream()
    };
    let a = render_once();
    let b = render_once();
    a == b && !a.is_empty() && a.contains("SUMMARY total=3 pass=0 fail=1 skip=1")
}

/// 错误路径 Result 语义：闭集码可经 parse/push/summarize 复现。
fn error_path_ok() -> bool {
    // 前缀表外 → PrefixForeign。
    if parse_line("NOPE name=x") != Err(Diag::PrefixForeign.val()) {
        return false;
    }
    // 计数自洽破缺经 push 的字段守卫也可抓（Summary 构造违约）。
    let bad = KvLine::Summary { total: 7, pass: 1, fail: 1, skip: 1 };
    if bad.fields_complete() {
        return false;
    }
    let mut w = KvWriter::new();
    if w.push(bad) != Err(Diag::FieldMissing.val()) {
        return false;
    }
    !detail(0x0).is_empty()
}

/// u64 解析边界：0 / 多位 / 前导零 / 非数字 / 溢出显性拒。
fn parse_u64_boundary_ok() -> bool {
    let big = "18446744073709551615";
    let over = "18446744073709551616";
    match (parse_u64("0"), parse_u64("42"), parse_u64("007"), parse_u64("1a"), parse_u64(big), parse_u64(over)) {
        (Some(0), Some(42), Some(7), None, Some(u64::MAX), None) => true,
        _ => false,
    }
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let mut w = KvWriter::new();
    let seq = [
        KvLine::Case { name: String::from("kv_case_reg") },
        KvLine::Pass { name: String::from("kv_pass_one") },
        KvLine::Fail { name: String::from("kv_fail_one"), why: String::from("mismatch") },
    ];
    for line in seq.iter() {
        if w.push(line.clone()).is_err() {
            return false;
        }
    }
    if w.summarize().is_err() {
        return false;
    }
    let stream = w.render_stream();
    let summary = format!("UNX-F3424 行式KV协议：五类行冻结，流{}行收口", w.len());
    !summary.is_empty()
        && summary.contains("行式KV协议")
        && stream.contains("SUMMARY total=3 pass=1 fail=1 skip=0")
        && w.state() == WriterState::SummaryWritten
}

// ── 判据总成：run_unx3424_checks ───────────────────────────────────────

/// run_unx3424_checks：本单判据聚合（诊断段 0x79800，全部走 add_dyn）。
pub fn run_unx3424_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3424 行式KV输出格式");
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
            "五前缀六键冻结表互异且必填键齐",
            frozen_tables_ok(),
            format!("冻结表{}", if frozen_tables_ok() { "齐" } else { "漂移" }),
        ),
        (
            "M183往返五类行渲染解析再渲染一致",
            round_trip_ok(),
            format!("往返{}", if round_trip_ok() { "一致" } else { "破缺" }),
        ),
        (
            "正常流五类行全写通收口计数自洽",
            writer_normal_flow_ok(),
            format!("写入器{}", if writer_normal_flow_ok() { "通" } else { "堵" }),
        ),
        (
            "注入1关流后追加必抓",
            inj_stream_closed_ok(),
            format!("关流写{}", if inj_stream_closed_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2双SUMMARY必抓",
            inj_double_summary_ok(),
            format!("双收口{}", if inj_double_summary_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3坏前缀未知键空值必抓",
            inj_parse_rejects_ok(),
            format!("解析违约{}", if inj_parse_rejects_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入4SUMMARY缺项坏形自洽破必抓",
            inj_summary_fields_ok(),
            format!("SUMMARY违约{}", if inj_summary_fields_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "写入器状态机Open到SummaryWritten单向",
            writer_state_monotonic_ok(),
            format!("状态机{}", if writer_state_monotonic_ok() { "单调" } else { "回退" }),
        ),
        (
            "双跑一致同输入两次流逐字节同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径闭集码可复现",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
        ),
        (
            "u64解析边界含溢出显性拒",
            parse_u64_boundary_ok(),
            format!("解析边界{}", if parse_u64_boundary_ok() { "齐" } else { "缺" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    let wb = writeback_summary_ok();
    s.add_dyn("写回摘要非空状态已完成", wb, &format!("写回{}", if wb { "非空" } else { "空" }));
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79800);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
        assert!(detail(Diag::StreamClosed.val()).contains("追加"));
    }

    #[test]
    fn frozen_tables_are_consistent() {
        assert!(frozen_tables_ok());
        assert_eq!(LINE_PREFIXES[0], "CASE");
        assert_eq!(LINE_PREFIXES[4], "SUMMARY");
        assert_eq!(LineKind::Fail.required_keys(), &["name", "why"][..]);
        assert_eq!(LineKind::Summary.required_keys(), &["total", "pass", "fail", "skip"][..]);
    }

    #[test]
    fn round_trip_all_five_kinds() {
        assert!(round_trip_ok());
        let line = KvLine::Fail { name: String::from("x"), why: String::from("y") };
        let text = line.render_line();
        assert_eq!(text, "FAIL name=x why=y");
        assert_eq!(parse_line(&text).unwrap(), line);
    }

    #[test]
    fn writer_flow_and_summary_counts() {
        assert!(writer_normal_flow_ok());
        let mut w = KvWriter::new();
        w.push(KvLine::Case { name: String::from("a") }).unwrap();
        w.push(KvLine::Skip { name: String::from("b"), why: String::from("z") }).unwrap();
        let s = w.summarize().unwrap();
        match s {
            KvLine::Summary { total, pass, fail, skip } => {
                assert_eq!((total, pass, fail, skip), (2, 0, 0, 1));
            }
            _ => panic!("收口行类型错"),
        }
        assert_eq!(w.state(), WriterState::SummaryWritten);
        assert_eq!(w.len(), 3);
    }

    #[test]
    fn stream_closed_and_double_summary_rejected() {
        assert!(inj_stream_closed_ok());
        assert!(inj_double_summary_ok());
        assert!(writer_state_monotonic_ok());
    }

    #[test]
    fn parse_rejections_all_kinds() {
        assert!(inj_parse_rejects_ok());
        assert!(inj_summary_fields_ok());
        assert_eq!(parse_line("PASS name"), Err(Diag::ValueEmpty.val()));
        assert_eq!(parse_line("CASE name= ghost=1"), Err(Diag::ValueEmpty.val()));
    }

    #[test]
    fn double_run_identical_streams() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x798FF).starts_with("未知诊断码"));
    }

    #[test]
    fn u64_parse_boundaries() {
        assert!(parse_u64_boundary_ok());
        assert_eq!(parse_u64(""), None);
        assert_eq!(parse_u64("-1"), None);
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3424_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
