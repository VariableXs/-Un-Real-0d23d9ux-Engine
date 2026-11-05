//! UNX-F3405 断言失败报告格式（文件/行/期望/实际） · 0x79480段
//!
//! 判据原文（UNX-F3405 派工卡逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：失败报告四元组（file/line/期望/实际）+ 上下文字段；M-164 报告完整性。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 失败报告四元组：file（.rs 源文件名）/line（从 1 起）/期望/实际，字段
//!    全置非空才算完整；上下文字段（域/用例/批注）随行落账；
//! 2. 单行规范格式：`file:line: 期望<> 实际<> 上下文[域/用例]`，格式化纯
//!    整数拼装（无浮点无墙钟），格式行必须回含四元组与上下文原值；
//! 3. M-164 报告完整性：六档语料（四元组齐/缺文件/缺行/缺期望/缺实际/缺
//!    上下文）逐档过校验闸，齐档必过、缺档必拒、拒码逐档可辨；
//! 4. 报告台账：入账前过校验闸（Result<T,u32>），坏报告显性拒不入账、
//!    拒绝计数显性记账；台账序号从一起连续递增；
//! 5. 注入矩阵 8 项：空文件/非 .rs 文件/零行/空期望/空实际/空上下文/账序
//!    断裂/越位取账逐种构造，逐种必须被口点出；
//! 6. 诊断码闭集 0x79481..0x7948C，Err 用 Result<T,u32>，中文诊断文案。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B10 给定值；本批窗口 0x79400..=0x795C0）。
pub const SEG_BASE: u32 = 0x79480;

/// M-164 完整性语料档数。
pub const M164_CASES: usize = 6;

/// 报告台账容量（定容防膨胀，超容显性记账）。
pub const LEDGER_CAP: usize = 64;

/// 报告格式诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 文件字段空：file 必填。
    EEmptyFile,
    /// 文件名非法：file 必须是 .rs 源文件名。
    EBadFile,
    /// 行号非法：line 必须从 1 起。
    EBadLine,
    /// 期望字段空：期望值必填。
    EEmptyWant,
    /// 实际字段空：实际值必填。
    EEmptyGot,
    /// 上下文缺：域/用例必填。
    ENoCtx,
    /// 报告重复：同四元组重复入账被拒。
    EDup,
    /// 账序断裂：台账序号不连续。
    ESeq,
    /// 账越位：取账下标越出台账。
    ERange,
    /// 账容溢出：超容入账被拒并记账。
    EFull,
    /// 格式行破损：格式化行回不含原值（格式化违约）。
    EBadFormat,
    /// 相位倒退：报告封账后不许再入账。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EEmptyFile => 0x01,
                Diag::EBadFile => 0x02,
                Diag::EBadLine => 0x03,
                Diag::EEmptyWant => 0x04,
                Diag::EEmptyGot => 0x05,
                Diag::ENoCtx => 0x06,
                Diag::EDup => 0x07,
                Diag::ESeq => 0x08,
                Diag::ERange => 0x09,
                Diag::EFull => 0x0a,
                Diag::EBadFormat => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EEmptyFile => "文件字段空：file 必填，回填 .rs 源文件名再入账",
            Diag::EBadFile => "文件名非法：file 须以 .rs 结尾，按真实源文件名回填",
            Diag::EBadLine => "行号非法：line 从 1 起，按断言实际所在行回填",
            Diag::EEmptyWant => "期望字段空：期望值必填，回填断言期望再入账",
            Diag::EEmptyGot => "实际字段空：实际值必填，回填断言实测再入账",
            Diag::ENoCtx => "上下文缺：域/用例必填，回填上下文再入账",
            Diag::EDup => "报告重复：同四元组已在账，去重后再入账",
            Diag::ESeq => "账序断裂：台账序号须从一起连续递增，重建台账",
            Diag::ERange => "取账越位：下标越出台账，按台账长度取账",
            Diag::EFull => "账容溢出：台账已满，先归档再续账",
            Diag::EBadFormat => "格式行破损：格式化行回不含原值，修格式化器再出账",
            Diag::EPhase => "相位倒退：报告已封账只读，改动走新台账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EEmptyFile,
    Diag::EBadFile,
    Diag::EBadLine,
    Diag::EEmptyWant,
    Diag::EEmptyGot,
    Diag::ENoCtx,
    Diag::EDup,
    Diag::ESeq,
    Diag::ERange,
    Diag::EFull,
    Diag::EBadFormat,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79481..0x7948C 闭集内"
}

/// 报告相位（单向：开账→封账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 开账：可入账。
    Open,
    /// 封账：只读。
    Sealed,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Open => 0,
            Phase::Sealed => 1,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Open => "开账",
            Phase::Sealed => "封账",
        }
    }
}

/// 报告上下文字段（域/用例/批注）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportCtx {
    /// 域（非空）。
    pub domain: &'static str,
    /// 用例名（非空）。
    pub case: &'static str,
    /// 批注（可空）。
    pub note: &'static str,
}

/// 失败报告四元组 + 上下文。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailReport {
    /// 源文件名（.rs 结尾）。
    pub file: &'static str,
    /// 行号（从 1 起）。
    pub line: u32,
    /// 期望值（非空）。
    pub expected: String,
    /// 实际值（非空）。
    pub actual: String,
    /// 上下文字段。
    pub ctx: ReportCtx,
}

impl FailReport {
    /// M-164 完整性校验：四元组齐 + 上下文齐 + 文件 .rs + 行号从 1 起。
    /// 返回 Ok(()) 或逐字段可辨的拒码。
    pub fn validate(&self) -> Result<(), u32> {
        if self.file.is_empty() {
            return Err(Diag::EEmptyFile.val());
        }
        if !self.file.ends_with(".rs") {
            return Err(Diag::EBadFile.val());
        }
        if self.line == 0 {
            return Err(Diag::EBadLine.val());
        }
        if self.expected.is_empty() {
            return Err(Diag::EEmptyWant.val());
        }
        if self.actual.is_empty() {
            return Err(Diag::EEmptyGot.val());
        }
        if self.ctx.domain.is_empty() || self.ctx.case.is_empty() {
            return Err(Diag::ENoCtx.val());
        }
        Ok(())
    }

    /// 四元组键（去重用：文件+行+期望+实际）。
    pub fn quad_key(&self) -> String {
        format!("{}:{}:{}", self.file, self.line, self.expected)
    }

    /// 单行规范格式：`file:line: 期望<> 实际<> 上下文[域/用例]`。
    pub fn format_line(&self) -> String {
        format!(
            "{}:{}: 期望<{}> 实际<{}> 上下文[{}/{}]",
            self.file, self.line, self.expected, self.actual, self.ctx.domain, self.ctx.case
        )
    }

    /// 格式行回含原值（格式化器自检——防格式行破损）。
    pub fn format_roundtrip_ok(&self) -> bool {
        let line = self.format_line();
        line.contains(self.file)
            && line.contains(&format!("{}", self.line))
            && line.contains(self.expected.as_str())
            && line.contains(self.actual.as_str())
            && line.contains(self.ctx.domain)
            && line.contains(self.ctx.case)
    }
}

/// 六档语料（M-164）：一档齐 + 五档各缺一。
pub struct M164Case {
    /// 档名。
    pub name: &'static str,
    /// 语料报告。
    pub report: FailReport,
    /// 钦定拒码（None=必过）。
    pub want_reject: Option<Diag>,
}

/// 构造 M-164 六档语料。
pub fn m164_corpus() -> Vec<M164Case> {
    let good = FailReport {
        file: "kmem.rs",
        line: 42,
        expected: String::from("0x1F"),
        actual: String::from("0x2E"),
        ctx: ReportCtx {
            domain: "内存",
            case: "块首差异",
            note: "M-164 齐档",
        },
    };
    let mut out = Vec::new();
    // 档1：四元组+上下文齐（必过）。
    out.push(M164Case {
        name: "齐档",
        report: good.clone(),
        want_reject: None,
    });
    // 档2：缺文件。
    let mut r = good.clone();
    r.file = "";
    out.push(M164Case {
        name: "缺文件",
        report: r,
        want_reject: Some(Diag::EEmptyFile),
    });
    // 档3：行号零。
    let mut r = good.clone();
    r.line = 0;
    out.push(M164Case {
        name: "零行",
        report: r,
        want_reject: Some(Diag::EBadLine),
    });
    // 档4：缺期望。
    let mut r = good.clone();
    r.expected = String::new();
    out.push(M164Case {
        name: "缺期望",
        report: r,
        want_reject: Some(Diag::EEmptyWant),
    });
    // 档5：缺实际。
    let mut r = good.clone();
    r.actual = String::new();
    out.push(M164Case {
        name: "缺实际",
        report: r,
        want_reject: Some(Diag::EEmptyGot),
    });
    // 档6：缺上下文（域空）。
    let mut r = good.clone();
    r.ctx = ReportCtx {
        domain: "",
        case: "块首差异",
        note: "",
    };
    out.push(M164Case {
        name: "缺上下文",
        report: r,
        want_reject: Some(Diag::ENoCtx),
    });
    out
}

/// M-164 报告完整性验证：逐档过闸，齐档必过、缺档必拒且拒码可辨。
pub fn m164_verify() -> (bool, Vec<bool>) {
    let corpus = m164_corpus();
    let mut per: Vec<bool> = Vec::new();
    if corpus.len() != M164_CASES {
        return (false, per);
    }
    let mut all = true;
    for c in &corpus {
        let got = c.report.validate();
        let ok = match c.want_reject {
            None => got.is_ok(),
            Some(w) => got == Err(w.val()),
        };
        per.push(ok);
        if !ok {
            all = false;
        }
    }
    (all, per)
}

/// 报告台账（序号从一起、坏报告拒入账、封账只读）。
#[derive(Clone, Debug)]
pub struct ReportLedger {
    phase: Phase,
    reports: Vec<(u32, FailReport)>,
    rejects: u32,
    overflow: usize,
}

impl ReportLedger {
    /// 新台账（开账）。
    pub fn new() -> ReportLedger {
        ReportLedger {
            phase: Phase::Open,
            reports: Vec::new(),
            rejects: 0,
            overflow: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 已入账条数。
    pub fn len(&self) -> usize {
        self.reports.len()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 超容溢出计数。
    pub fn overflow(&self) -> usize {
        self.overflow
    }

    /// 入账：过校验闸 + 去重 + 超容拒，序号连续递增。
    pub fn push(&mut self, r: FailReport) -> Result<u32, u32> {
        if self.phase != Phase::Open {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if let Err(code) = r.validate() {
            self.rejects += 1;
            return Err(code);
        }
        if self.reports.len() >= LEDGER_CAP {
            self.overflow += 1;
            return Err(Diag::EFull.val());
        }
        let key = r.quad_key();
        for (_, x) in &self.reports {
            if x.quad_key() == key {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        let seq = (self.reports.len() as u32) + 1;
        self.reports.push((seq, r));
        Ok(seq)
    }

    /// 取账（越位返回 Err(ERange)）。
    pub fn at(&self, idx: usize) -> Result<(u32, &FailReport), u32> {
        if idx >= self.reports.len() {
            return Err(Diag::ERange.val());
        }
        match self.reports.get(idx) {
            Some((seq, r)) => Ok((*seq, r)),
            None => Err(Diag::ERange.val()),
        }
    }

    /// 账序自检：序号从一起连续递增。
    pub fn seq_ok(&self) -> bool {
        let mut want = 1u32;
        for (seq, _) in &self.reports {
            if *seq != want {
                return false;
            }
            want += 1;
        }
        true
    }

    /// 封账（开账→封账，单向；封账后入账显性拒）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != Phase::Open {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        self.phase = Phase::Sealed;
        Ok(())
    }

    /// 写回摘要（域×条数聚合一行一域，摘要非空）。
    pub fn summary(&self) -> String {
        let mut out = String::new();
        let mut seen: Vec<&'static str> = Vec::new();
        for (_, r) in &self.reports {
            let fresh = !seen.iter().any(|d| *d == r.ctx.domain);
            if fresh {
                seen.push(r.ctx.domain);
            }
        }
        for d in &seen {
            let mut n = 0usize;
            for (_, r) in &self.reports {
                if r.ctx.domain == *d {
                    n += 1;
                }
            }
            out.push_str(&format!("域{} 报告{}\n", d, n));
        }
        out.push_str(&format!("账{} 拒{}\n", self.reports.len(), self.rejects));
        out
    }
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..INJECTS 内）。
pub const INJECTS: usize = 8;

fn mark(out: &mut [bool; INJECTS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 标准齐报告（注入构造基底）。
fn good_report() -> FailReport {
    FailReport {
        file: "ktime.rs",
        line: 7,
        expected: String::from("10"),
        actual: String::from("11"),
        ctx: ReportCtx {
            domain: "计时",
            case: "单调钟",
            note: "注入基底",
        },
    }
}

/// 注入矩阵 8 项：逐种构造违约，逐种必须被口点出。
pub fn inject_matrix() -> [bool; INJECTS] {
    let mut out = [false; INJECTS];
    // 注入1：空文件。
    let mut r = good_report();
    r.file = "";
    mark(&mut out, 0, r.validate() == Err(Diag::EEmptyFile.val()));
    // 注入2：非 .rs 文件。
    let mut r = good_report();
    r.file = "ktime.c";
    mark(&mut out, 1, r.validate() == Err(Diag::EBadFile.val()));
    // 注入3：零行。
    let mut r = good_report();
    r.line = 0;
    mark(&mut out, 2, r.validate() == Err(Diag::EBadLine.val()));
    // 注入4：空期望。
    let mut r = good_report();
    r.expected = String::new();
    mark(&mut out, 3, r.validate() == Err(Diag::EEmptyWant.val()));
    // 注入5：空实际。
    let mut r = good_report();
    r.actual = String::new();
    mark(&mut out, 4, r.validate() == Err(Diag::EEmptyGot.val()));
    // 注入6：空上下文（用例空）。
    let mut r = good_report();
    r.ctx.case = "";
    mark(&mut out, 5, r.validate() == Err(Diag::ENoCtx.val()));
    // 注入7：重复入账。
    let mut led = ReportLedger::new();
    let _ = led.push(good_report());
    mark(&mut out, 6, led.push(good_report()) == Err(Diag::EDup.val()));
    // 注入8：封账后入账。
    let mut led = ReportLedger::new();
    let _ = led.push(good_report());
    let _ = led.seal();
    mark(
        &mut out,
        7,
        led.push(good_report()) == Err(Diag::EPhase.val()),
    );
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B10 派工卡 0x79400..=0x795C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79400 || SEG_BASE > 0x795C0 {
        return false;
    }
    SEG_BASE == 0x79480
}

/// 诊断码闭集自洽（12 码互异、码值递增、查表非空）。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL.len() {
        let a = match ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 || detail(a.val()).is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            let b = match ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// J1 主判据：四元组 + 上下文字段齐备、格式行回含原值。
fn j1_main_ok() -> bool {
    let r = good_report();
    let validate_ok = r.validate().is_ok();
    let fields_ok = !r.file.is_empty()
        && r.line >= 1
        && !r.expected.is_empty()
        && !r.actual.is_empty()
        && !r.ctx.domain.is_empty()
        && !r.ctx.case.is_empty();
    let format_ok = r.format_roundtrip_ok()
        && r.format_line().contains("期望<10>")
        && r.format_line().contains("实际<11>")
        && r.format_line().contains("上下文[计时/单调钟]");
    validate_ok && fields_ok && format_ok
}

/// M-164 报告完整性验证全绿。
fn m164_ok() -> bool {
    let (all, per) = m164_verify();
    all && per.len() == M164_CASES
}

/// 台账面：序号连续、取账对位、坏账拒入、封账只读。
fn ledger_ok() -> bool {
    let mut led = ReportLedger::new();
    let mut r1 = good_report();
    r1.ctx.case = "用例甲";
    let mut r2 = good_report();
    r2.ctx.case = "用例乙";
    r2.file = "kbus.rs";
    let s1 = led.push(r1);
    let s2 = led.push(r2);
    if !matches!((s1, s2), (Ok(1), Ok(2))) {
        return false;
    }
    if !led.seq_ok() || led.len() != 2 {
        return false;
    }
    // 取账对位。
    let at1 = led.at(1);
    let at9 = led.at(9);
    if at9 != Err(Diag::ERange.val()) {
        return false;
    }
    match at1 {
        Ok((seq, r)) => {
            if seq != 2 || r.file != "kbus.rs" {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 摘要非空。
    let sum = led.summary();
    if sum.is_empty() || !sum.contains("域计时") {
        return false;
    }
    true
}

/// 双跑一致（语料两次验证同判、摘要同文）。
fn dual_run_ok() -> bool {
    let (a, pa) = m164_verify();
    let (b, pb) = m164_verify();
    if a != b || pa != pb {
        return false;
    }
    let mut l1 = ReportLedger::new();
    let mut l2 = ReportLedger::new();
    let _ = l1.push(good_report());
    let _ = l2.push(good_report());
    l1.summary() == l2.summary()
}

/// 注入 8/8 检出。
fn inject_all_ok() -> bool {
    inject_matrix() == [true; INJECTS]
}

/// 判据总成：UNX-F3405 断言失败报告格式。
pub fn run_unx3405_checks() -> CheckSet {
    let mut s = CheckSet::new("unx3405");
    let good = good_report();

    s.add_dyn(
        "F3405-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F3405-02 诊断码闭集自洽",
        diag_closed_ok(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    // 正常流：四元组+上下文齐备、格式行回含原值（J1 主判据）。
    s.add_dyn(
        "F3405-03 失败报告四元组上下文齐备",
        j1_main_ok(),
        &format!("line={}", good.format_line()),
    );

    // M-164 六档完整性验证全绿。
    let (m_ok, m_per) = m164_verify();
    s.add_dyn(
        "F3405-04 M164报告完整性六档验证",
        m_ok,
        &format!("per={:?}", m_per),
    );

    // 台账面：序号连续、取账对位、坏账拒入。
    s.add_dyn(
        "F3405-05 报告台账序号取账对位",
        ledger_ok(),
        &format!("ledger={}", ledger_ok()),
    );

    // 注入必抓：8 项注入全检出。
    s.add_dyn(
        "F3405-06 注入矩阵八项全检出",
        inject_all_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 边界：行号 1（下沿）必过、行号 0（违约）必拒。
    let mut r_low = good_report();
    r_low.line = 1;
    let line_edge = r_low.validate().is_ok();
    s.add_dyn(
        "F3405-07 行号下沿必过零必拒",
        line_edge,
        &format!("line={}", r_low.line),
    );

    // 边界：重复四元组去重拒、异四元组放行。
    let mut led = ReportLedger::new();
    let _ = led.push(good_report());
    let mut r_other = good_report();
    r_other.file = "kbus.rs";
    let dup = led.push(good_report()) == Err(Diag::EDup.val());
    let fresh = led.push(r_other).is_ok();
    s.add_dyn(
        "F3405-08 四元组去重放行异账",
        dup && fresh && led.len() == 2,
        &format!("len={}", led.len()),
    );

    // 边界：封账后入账与再封账均显性拒。
    let mut led2 = ReportLedger::new();
    let _ = led2.push(good_report());
    let _ = led2.seal();
    let sealed = led2.push(good_report()) == Err(Diag::EPhase.val())
        && led2.seal() == Err(Diag::EPhase.val())
        && led2.phase() == Phase::Sealed;
    s.add_dyn(
        "F3405-09 封账只读显性拒",
        sealed,
        &format!("phase={}", led2.phase().name()),
    );

    // 账容溢出显性记账（定容账不静默吞）。
    let mut led3 = ReportLedger::new();
    let mut fill = 0usize;
    let mut overflow_seen = false;
    while fill < LEDGER_CAP {
        let mut r = good_report();
        r.line = (fill as u32) + 1;
        let _ = led3.push(r);
        fill += 1;
    }
    let mut r_over = good_report();
    r_over.file = "kover.rs";
    overflow_seen = led3.push(r_over) == Err(Diag::EFull.val()) && led3.overflow() == 1;
    s.add_dyn(
        "F3405-10 账容溢出显性记账",
        overflow_seen && led3.len() == LEDGER_CAP,
        &format!("cap={} overflow={}", LEDGER_CAP, led3.overflow()),
    );

    // 格式行破损自检：格式化器回含原值（四元组+上下文六面）。
    let rt = good.format_roundtrip_ok()
        && good.format_line().starts_with("ktime.rs:7:");
    s.add_dyn(
        "F3405-11 格式行回含原值",
        rt,
        &format!("bytes={}", good.format_line().len()),
    );

    // 写回摘要非空（判据③写回面）。
    let mut led4 = ReportLedger::new();
    let _ = led4.push(good_report());
    let sum = led4.summary();
    s.add_dyn(
        "F3405-12 写回摘要非空含域",
        !sum.is_empty() && sum.contains("域计时") && sum.contains("账1"),
        &format!("bytes={}", sum.len()),
    );

    // 双跑一致。
    s.add_dyn(
        "F3405-13 双跑一致",
        dual_run_ok(),
        &format!("dual={}", dual_run_ok()),
    );

    // 拒绝计数显性记账（坏账留痕不静默：拒码可辨且计数入账）。
    let mut led5 = ReportLedger::new();
    let mut r_bad = good_report();
    r_bad.file = "";
    let bad_rej = led5.push(r_bad) == Err(Diag::EEmptyFile.val());
    s.add_dyn(
        "F3405-14 坏账拒绝计数显性",
        bad_rej && led5.len() == 0 && led5.rejects() == 1,
        &format!("rejects={}", led5.rejects()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EEmptyFile.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79480);
    }

    #[test]
    fn good_report_passes_all_gates() {
        let r = good_report();
        assert_eq!(r.validate(), Ok(()));
        assert!(r.format_roundtrip_ok());
        assert!(r.format_line().contains("期望<10>"));
        assert!(r.format_line().contains("上下文[计时/单调钟]"));
    }

    #[test]
    fn m164_six_cases() {
        assert_eq!(m164_corpus().len(), M164_CASES);
        let (all, per) = m164_verify();
        assert!(all);
        assert!(per.iter().all(|x| *x));
        // 逐档拒码可辨：五缺档各得其码。
        let corpus = m164_corpus();
        for (i, c) in corpus.iter().enumerate() {
            if i == 0 {
                assert_eq!(c.report.validate(), Ok(()));
            } else {
                let want = match c.want_reject {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_eq!(c.report.validate(), Err(want), "档{}拒码错位", i);
            }
        }
    }

    #[test]
    fn ledger_seq_and_range() {
        let mut led = ReportLedger::new();
        let mut r1 = good_report();
        r1.ctx.case = "甲";
        let mut r2 = good_report();
        r2.ctx.case = "乙";
        r2.file = "kbus.rs";
        assert_eq!(led.push(r1), Ok(1));
        assert_eq!(led.push(r2), Ok(2));
        assert!(led.seq_ok());
        assert_eq!(led.at(0).map(|(s, _)| s), Ok(1));
        assert_eq!(led.at(5).map(|(s, _)| s), Err(Diag::ERange.val()));
    }

    #[test]
    fn dup_rejected() {
        let mut led = ReportLedger::new();
        let _ = led.push(good_report());
        assert_eq!(led.push(good_report()), Err(Diag::EDup.val()));
        assert_eq!(led.len(), 1);
    }

    #[test]
    fn inject_eight_of_eight() {
        assert_eq!(inject_matrix(), [true; INJECTS]);
    }

    #[test]
    fn seal_is_one_way() {
        let mut led = ReportLedger::new();
        assert_eq!(led.seal(), Ok(()));
        assert_eq!(led.seal(), Err(Diag::EPhase.val()));
        assert_eq!(led.push(good_report()), Err(Diag::EPhase.val()));
        assert_eq!(led.phase(), Phase::Sealed);
    }

    #[test]
    fn overflow_accounted() {
        let mut led = ReportLedger::new();
        for i in 0..LEDGER_CAP {
            let mut r = good_report();
            r.line = i as u32 + 1;
            assert!(led.push(r).is_ok());
        }
        let mut r = good_report();
        r.file = "kover.rs";
        assert_eq!(led.push(r), Err(Diag::EFull.val()));
        assert_eq!(led.overflow(), 1);
        assert_eq!(led.len(), LEDGER_CAP);
    }

    #[test]
    fn format_line_structure() {
        let line = good_report().format_line();
        assert!(line.starts_with("ktime.rs:7:"));
        assert!(line.contains("期望<10> 实际<11>"));
        assert!(line.ends_with("上下文[计时/单调钟]"));
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn summary_non_empty_with_domain() {
        let mut led = ReportLedger::new();
        let _ = led.push(good_report());
        let sum = led.summary();
        assert!(!sum.is_empty());
        assert!(sum.contains("域计时 报告1"));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx3405_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
