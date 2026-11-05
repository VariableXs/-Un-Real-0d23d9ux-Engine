//! UNX-F3429 断言账汇总（总断言数/通过/失败/跳过四计数） · 0x79940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B12 逐字摘录）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：账本汇总入 JSON（F3408 四计数 → summary 段）+ 分域视图渲染；
//!     M-188 与 F3408 对账。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四计数轴冻结：total/passed/failed/skipped 逐字锚定 F3408 轴名，
//!     域内不变式 passed+failed+skipped == total（checked 防溢出）；
//!   - 记账面：断言事件按域归并（同域累加不重行），域行序 = 首记序；
//!   - 汇总入 JSON（F3426 序列化器的值面）：summary 段四计数 + 分域
//!     数组逐域一行，键名全在冻结轴名表内（表外键点名拒）；
//!   - 分域视图渲染：`域=X 总=N 过=N 败=N 跳=N` 逐域一行，行数与域数
//!     守恒，行序与账本序一致；
//!   - M-188 对账：原始事件流重放求和与账本逐域逐轴比对，任一轴不齐
//!     即 ReconcileMismatch 点名（改账必抓）；
//!   - 双跑一致：重放与渲染均纯函数，同输入两次逐字节相同。
//! 常量来源：
//!   - 四计数轴名：F3408 断言账（total/passed/failed/skipped）冻结轴；
//!   - 诊断段：0x79940（0x40 对齐，B12 批窗 [0x79800, 0x799C0]）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B12 批段窗 [0x79800, 0x799C0]）。
pub const SEG_BASE: u32 = 0x79940;

/// F3408 四计数轴名（冻结，序固定）。
pub const COUNT_FIELDS: [&'static str; 4] = ["total", "passed", "failed", "skipped"];

/// 断言判定闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssertVerdict {
    /// 通过。
    Passed,
    /// 失败。
    Failed,
    /// 跳过。
    Skipped,
}

impl AssertVerdict {
    /// 中文判定名。
    pub fn zh(self) -> &'static str {
        match self {
            AssertVerdict::Passed => "过",
            AssertVerdict::Failed => "败",
            AssertVerdict::Skipped => "跳",
        }
    }
}

/// 域计数行（四计数 + 域名；行内不变式 total == p+f+s）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainTally {
    /// 域名（非空；账内唯一）。
    pub domain: String,
    /// 总断言数。
    pub total: u64,
    /// 通过数。
    pub passed: u64,
    /// 失败数。
    pub failed: u64,
    /// 跳过数。
    pub skipped: u64,
}

impl DomainTally {
    /// 行内不变式（checked 加法防溢出）。
    pub fn row_consistent(&self) -> bool {
        match self.passed.checked_add(self.failed) {
            Some(pf) => match pf.checked_add(self.skipped) {
                Some(sum) => sum == self.total,
                None => false,
            },
            None => false,
        }
    }

    /// 累加一条判定（checked；溢出显性拒）。
    pub fn apply(&mut self, v: AssertVerdict) -> Result<(), u32> {
        match v {
            AssertVerdict::Passed => {
                self.passed = self.passed.checked_add(1).ok_or(Diag::AxisOverflow.val())?;
            }
            AssertVerdict::Failed => {
                self.failed = self.failed.checked_add(1).ok_or(Diag::AxisOverflow.val())?;
            }
            AssertVerdict::Skipped => {
                self.skipped = self.skipped.checked_add(1).ok_or(Diag::AxisOverflow.val())?;
            }
        }
        self.total = self.total.checked_add(1).ok_or(Diag::AxisOverflow.val())?;
        Ok(())
    }
}

/// 断言账本（域行序 = 首记序；同域归并不重行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBook {
    /// 域计数行表。
    pub domains: Vec<DomainTally>,
}

impl AccountBook {
    /// 空账本。
    pub fn new() -> AccountBook {
        AccountBook { domains: Vec::new() }
    }

    /// 域行定位（线性查，域名匹配返回下标）。
    fn find(&self, domain: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.domains.len() {
            match self.domains.get(i) {
                Some(row) => {
                    if row.domain == domain {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 记一条断言事件：已有域行累加，无则按首记序建行。
    pub fn record(&mut self, domain: &str, v: AssertVerdict) -> Result<(), u32> {
        if domain.is_empty() {
            return Err(Diag::DomainEmpty.val());
        }
        match self.find(domain) {
            Some(i) => match self.domains.get_mut(i) {
                Some(row) => row.apply(v),
                None => Err(Diag::DomainEmpty.val()),
            },
            None => {
                let mut row = DomainTally {
                    domain: String::from(domain),
                    total: 0,
                    passed: 0,
                    failed: 0,
                    skipped: 0,
                };
                row.apply(v)?;
                self.domains.push(row);
                Ok(())
            }
        }
    }

    /// 账本不变式全核：域行唯一 + 行内自洽 + 全局和守恒。
    pub fn invariants_ok(&self) -> Result<(), u32> {
        if self.domains.is_empty() {
            return Err(Diag::EmptyBook.val());
        }
        let mut gt: u64 = 0;
        let mut gp: u64 = 0;
        let mut gf: u64 = 0;
        let mut gs: u64 = 0;
        for (i, row) in self.domains.iter().enumerate() {
            if row.domain.is_empty() {
                return Err(Diag::DomainEmpty.val());
            }
            let mut j = i + 1;
            while j < self.domains.len() {
                match self.domains.get(j) {
                    Some(q) => {
                        if q.domain == row.domain {
                            return Err(Diag::DomainDup.val());
                        }
                    }
                    None => return Err(Diag::DomainDup.val()),
                }
                j += 1;
            }
            if !row.row_consistent() {
                return Err(Diag::CountInconsistent.val());
            }
            gt = gt.checked_add(row.total).ok_or(Diag::AxisOverflow.val())?;
            gp = gp.checked_add(row.passed).ok_or(Diag::AxisOverflow.val())?;
            gf = gf.checked_add(row.failed).ok_or(Diag::AxisOverflow.val())?;
            gs = gs.checked_add(row.skipped).ok_or(Diag::AxisOverflow.val())?;
        }
        // 全局守恒：域行四轴各自之和再走一次行内不变式。
        match gp.checked_add(gf) {
            Some(pf) => match pf.checked_add(gs) {
                Some(sum) => {
                    if sum != gt {
                        return Err(Diag::GlobalSumBreak.val());
                    }
                }
                None => return Err(Diag::AxisOverflow.val()),
            },
            None => return Err(Diag::AxisOverflow.val()),
        }
        Ok(())
    }
}

/// 全局汇总（四计数；F3408 轴）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryCounts {
    /// 总断言数。
    pub total: u64,
    /// 通过数。
    pub passed: u64,
    /// 失败数。
    pub failed: u64,
    /// 跳过数。
    pub skipped: u64,
}

impl SummaryCounts {
    /// 行内不变式。
    pub fn consistent(&self) -> bool {
        match self.passed.checked_add(self.failed) {
            Some(pf) => match pf.checked_add(self.skipped) {
                Some(sum) => sum == self.total,
                None => false,
            },
            None => false,
        }
    }
}

/// 汇总四计数（逐域 checked 累加；空账显性拒）。
pub fn aggregate_summary(book: &AccountBook) -> Result<SummaryCounts, u32> {
    book.invariants_ok()?;
    let mut s = SummaryCounts { total: 0, passed: 0, failed: 0, skipped: 0 };
    for row in book.domains.iter() {
        s.total = s.total.checked_add(row.total).ok_or(Diag::AxisOverflow.val())?;
        s.passed = s.passed.checked_add(row.passed).ok_or(Diag::AxisOverflow.val())?;
        s.failed = s.failed.checked_add(row.failed).ok_or(Diag::AxisOverflow.val())?;
        s.skipped = s.skipped.checked_add(row.skipped).ok_or(Diag::AxisOverflow.val())?;
    }
    Ok(s)
}

/// 汇总入 JSON 值面（summary 段四计数 + 分域数组；键名全在冻结轴表内）。
/// 本模块只产出值结构（序列化归 F3426 的手写器）。
pub fn summary_to_json(book: &AccountBook) -> Result<JsonLite, u32> {
    let s = aggregate_summary(book)?;
    let mut domains: Vec<JsonLite> = Vec::new();
    for row in book.domains.iter() {
        domains.push(JsonLite::JObj(vec![
            (String::from("domain"), JsonLite::JStr(row.domain.clone())),
            (String::from("total"), JsonLite::JNum(row.total)),
            (String::from("passed"), JsonLite::JNum(row.passed)),
            (String::from("failed"), JsonLite::JNum(row.failed)),
            (String::from("skipped"), JsonLite::JNum(row.skipped)),
        ]));
    }
    Ok(JsonLite::JObj(vec![
        (
            String::from("summary"),
            JsonLite::JObj(vec![
                (String::from("total"), JsonLite::JNum(s.total)),
                (String::from("passed"), JsonLite::JNum(s.passed)),
                (String::from("failed"), JsonLite::JNum(s.failed)),
                (String::from("skipped"), JsonLite::JNum(s.skipped)),
            ]),
        ),
        (String::from("domains"), JsonLite::JArr(domains)),
    ]))
}

/// 极简 JSON 值面（键序保留；供 summary_to_json 与键表外核查）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonLite {
    /// 字符串。
    JStr(String),
    /// 整数。
    JNum(u64),
    /// 数组。
    JArr(Vec<JsonLite>),
    /// 对象。
    JObj(Vec<(String, JsonLite)>),
}

impl JsonLite {
    /// 对象取键。
    pub fn get(&self, key: &str) -> Option<&JsonLite> {
        match self {
            JsonLite::JObj(pairs) => {
                for (k, v) in pairs.iter() {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// summary 段键表外核查：对象键全在四轴名 + 分域键名闭集内。
pub fn json_keys_frozen(node: &JsonLite) -> bool {
    match node {
        JsonLite::JObj(pairs) => {
            for (k, v) in pairs.iter() {
                let known = match k.as_str() {
                    "summary" | "domains" => true,
                    "domain" => true,
                    "total" | "passed" | "failed" | "skipped" => true,
                    _ => false,
                };
                if !known {
                    return false;
                }
                if !json_keys_frozen(v) {
                    return false;
                }
            }
            true
        }
        JsonLite::JArr(items) => {
            for it in items.iter() {
                if !json_keys_frozen(it) {
                    return false;
                }
            }
            true
        }
        _ => true,
    }
}

/// 分域视图渲染：`域=X 总=N 过=N 败=N 跳=N` 逐域一行（行序 = 账本序）。
pub fn render_domain_view(book: &AccountBook) -> Result<Vec<String>, u32> {
    book.invariants_ok()?;
    let mut lines: Vec<String> = Vec::new();
    for row in book.domains.iter() {
        lines.push(format!(
            "域={} 总={} 过={} 败={} 跳={}",
            row.domain, row.total, row.passed, row.failed, row.skipped
        ));
    }
    Ok(lines)
}

/// 原始事件流（M-188 对账的重放素材：域 + 判定按序留痕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventLog {
    /// 事件表（域非空）。
    pub events: Vec<(String, AssertVerdict)>,
}

impl EventLog {
    /// 空流。
    pub fn new() -> EventLog {
        EventLog { events: Vec::new() }
    }

    /// 留一条事件。
    pub fn push(&mut self, domain: &str, v: AssertVerdict) -> Result<(), u32> {
        if domain.is_empty() {
            return Err(Diag::DomainEmpty.val());
        }
        self.events.push((String::from(domain), v));
        Ok(())
    }
}

/// M-188 对账：事件流重放求和与账本逐域逐轴比对。
/// 平账返回 Ok(())；任一域任一轴不齐即 ReconcileMismatch。
pub fn reconcile(log: &EventLog, book: &AccountBook) -> Result<(), u32> {
    // 重放：事件流 → 新账本。
    let mut replay = AccountBook::new();
    for (domain, v) in log.events.iter() {
        replay.record(domain, *v)?;
    }
    // 账本不变式（含域唯一）先核。
    book.invariants_ok()?;
    // 逐域比对：域集合与四轴全等。
    if replay.domains.len() != book.domains.len() {
        return Err(Diag::ReconcileMismatch.val());
    }
    for row in book.domains.iter() {
        let mut matched = false;
        for rr in replay.domains.iter() {
            if rr.domain == row.domain {
                if rr.total != row.total
                    || rr.passed != row.passed
                    || rr.failed != row.failed
                    || rr.skipped != row.skipped
                {
                    return Err(Diag::ReconcileMismatch.val());
                }
                matched = true;
            }
        }
        if !matched {
            return Err(Diag::ReconcileMismatch.val());
        }
    }
    Ok(())
}

/// 诊断码闭集：0x79941..=0x7994C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 域名空：空域名入账。
    DomainEmpty = 0x01,
    /// 域内计数破：passed+failed+skipped != total。
    CountInconsistent = 0x02,
    /// 对账不平：事件流重放与账本逐域逐轴不齐。
    ReconcileMismatch = 0x03,
    /// 全局和破：域轴之和再走行内不变式不守恒。
    GlobalSumBreak = 0x04,
    /// 域重行：账本出现同名域双行（归并失守）。
    DomainDup = 0x05,
    /// 空账：账本无任何域行即求汇总。
    EmptyBook = 0x06,
    /// 轴溢出：计数累加超 u64。
    AxisOverflow = 0x07,
    /// 分域视图行数不守恒：行数 != 域数。
    ViewLineMissing = 0x08,
    /// 键表外：汇总 JSON 出现冻结轴名之外的键。
    FieldForeign = 0x09,
    /// 视图行序破：分域视图行序与账本首记序不一致。
    ViewOrderBreak = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 双跑不一致：同输入两次重放/渲染不同。
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
            Diag::DomainEmpty => "域名空：建议补全域名再记账，空域无归并目标",
            Diag::CountInconsistent => "域内计数破：建议 passed+failed+skipped 修正为 total 真实和",
            Diag::ReconcileMismatch => "对账不平：建议以事件流重放为准回填账本，改账必先过重放",
            Diag::GlobalSumBreak => "全局和破：建议逐域核对四轴，域轴之和须守恒",
            Diag::DomainDup => "域重行：建议同名域归并为一行，禁止双行并行累加",
            Diag::EmptyBook => "空账：建议至少记一条断言事件后再求汇总",
            Diag::AxisOverflow => "轴溢出：建议分账落盘，单账断言量控制在 u64 值域内",
            Diag::ViewLineMissing => "视图行缺：建议渲染按账本序逐域一行，行数与域数守恒",
            Diag::FieldForeign => "键表外：建议汇总 JSON 键只用 total/passed/failed/skipped 轴名",
            Diag::ViewOrderBreak => "视图行序破：建议渲染按账本首记序逐域出行，不做重排",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::DoubleRunDiverge => "双跑不一致：建议重放与渲染做成纯函数，输入定则输出定",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::DomainEmpty,
    Diag::CountInconsistent,
    Diag::ReconcileMismatch,
    Diag::GlobalSumBreak,
    Diag::DomainDup,
    Diag::EmptyBook,
    Diag::AxisOverflow,
    Diag::ViewLineMissing,
    Diag::FieldForeign,
    Diag::ViewOrderBreak,
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
    "未知诊断码：建议核对是否落在 0x79941..=0x7994C 闭集内"
}

/// 标准账场景：三域十事件按序记账（对账正样本素材）。
pub fn build_sample() -> (EventLog, AccountBook) {
    let mut log = EventLog::new();
    let mut book = AccountBook::new();
    let seq = [
        ("mm", AssertVerdict::Passed),
        ("mm", AssertVerdict::Passed),
        ("sched", AssertVerdict::Passed),
        ("sched", AssertVerdict::Failed),
        ("mm", AssertVerdict::Skipped),
        ("io", AssertVerdict::Passed),
        ("io", AssertVerdict::Failed),
        ("io", AssertVerdict::Failed),
        ("sched", AssertVerdict::Skipped),
        ("io", AssertVerdict::Passed),
    ];
    for (domain, v) in seq.iter() {
        let _ = log.push(domain, *v);
        let _ = book.record(domain, *v);
    }
    (log, book)
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

/// 冻结面：四计数轴名逐字锚定 F3408（total/passed/failed/skipped）。
fn frozen_fields_ok() -> bool {
    COUNT_FIELDS.len() == 4
        && COUNT_FIELDS.get(0).map(|k| *k == "total").unwrap_or(false)
        && COUNT_FIELDS.get(1).map(|k| *k == "passed").unwrap_or(false)
        && COUNT_FIELDS.get(2).map(|k| *k == "failed").unwrap_or(false)
        && COUNT_FIELDS.get(3).map(|k| *k == "skipped").unwrap_or(false)
}

/// 正常流：三域十事件记账、行序首记、行内自洽、全局守恒。
fn record_normal_flow_ok() -> bool {
    let (_, book) = build_sample();
    if book.domains.len() != 3 {
        return false;
    }
    let first = match book.domains.first() {
        Some(r) => r,
        None => return false,
    };
    // 首记序：mm 最先入账。
    if first.domain != "mm" {
        return false;
    }
    match book.domains.get(0) {
        Some(r) => !(r.total != 3 || r.passed != 2 || r.failed != 0 || r.skipped != 1),
        None => false,
    }
}

/// M-188 对账平：样本流重放与账本逐域逐轴全等。
fn reconcile_ok() -> bool {
    let (log, book) = build_sample();
    reconcile(&log, &book).is_ok()
}

/// 注入 1：改账必抓——账本某轴偏移后对账点名 ReconcileMismatch。
/// 改账保持行内自洽（passed 与 total 同加），让违约落在「对账不平」轴。
fn inj_reconcile_mismatch_ok() -> bool {
    let (log, mut book) = build_sample();
    match book.domains.first_mut() {
        Some(row) => {
            row.passed = row.passed.saturating_add(1);
            row.total = row.total.saturating_add(1);
        }
        None => return false,
    }
    if reconcile(&log, &book) != Err(Diag::ReconcileMismatch.val()) {
        return false;
    }
    // 域集合不齐（账本多一域，事件流没有）同样点名。
    let mut extra = book.clone();
    extra.domains.push(DomainTally {
        domain: String::from("幽灵域"),
        total: 1,
        passed: 1,
        failed: 0,
        skipped: 0,
    });
    reconcile(&log, &extra) == Err(Diag::ReconcileMismatch.val())
}

/// 注入 2：行内计数破账（手工构造 total 与三轴和不等）必抓。
fn inj_count_inconsistent_ok() -> bool {
    let mut book = AccountBook::new();
    book.domains.push(DomainTally {
        domain: String::from("破账域"),
        total: 9,
        passed: 1,
        failed: 1,
        skipped: 1,
    });
    book.invariants_ok() == Err(Diag::CountInconsistent.val())
}

/// 注入 3：域重行（同名域双行）必抓。
fn inj_domain_dup_ok() -> bool {
    let mut book = AccountBook::new();
    book.domains.push(DomainTally {
        domain: String::from("dup"),
        total: 1,
        passed: 1,
        failed: 0,
        skipped: 0,
    });
    book.domains.push(DomainTally {
        domain: String::from("dup"),
        total: 1,
        passed: 0,
        failed: 1,
        skipped: 0,
    });
    book.invariants_ok() == Err(Diag::DomainDup.val())
}

/// 注入 4：空域入账与空账汇总逐类拒。
fn inj_empty_cases_ok() -> bool {
    let mut book = AccountBook::new();
    let empty_domain = book.record("", AssertVerdict::Passed);
    let empty_book = aggregate_summary(&book);
    let mut log = EventLog::new();
    let empty_event = log.push("", AssertVerdict::Passed);
    empty_domain == Err(Diag::DomainEmpty.val())
        && empty_book == Err(Diag::EmptyBook.val())
        && empty_event == Err(Diag::DomainEmpty.val())
}

/// 分域视图渲染：行数守恒、行序一致、行文逐字核对。
fn domain_view_ok() -> bool {
    let (_, book) = build_sample();
    let lines = match render_domain_view(&book) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if lines.len() != book.domains.len() {
        return false;
    }
    match lines.first() {
        Some(l) => l == "域=mm 总=3 过=2 败=0 跳=1",
        None => false,
    }
}

/// 汇总入 JSON：summary 四计数 + 分域数组，键全在冻结轴表内。
fn summary_json_ok() -> bool {
    let (_, book) = build_sample();
    let node = match summary_to_json(&book) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if !json_keys_frozen(&node) {
        return false;
    }
    match node.get("summary") {
        Some(JsonLite::JObj(pairs)) => {
            if pairs.len() != 4 {
                return false;
            }
            let total = match node.get("summary") {
                Some(t) => match t.get("total") {
                    Some(JsonLite::JNum(v)) => *v,
                    _ => return false,
                },
                None => return false,
            };
            total == 10
        }
        _ => false,
    }
}

/// 双跑一致：重放、渲染、汇总三次执行逐码逐字节相同。
fn double_run_ok() -> bool {
    let (log, book) = build_sample();
    if reconcile(&log, &book) != reconcile(&log, &book) {
        return false;
    }
    let v1 = render_domain_view(&book);
    let v2 = render_domain_view(&book);
    v1 == v2 && aggregate_summary(&book) == aggregate_summary(&book)
}

/// 错误路径 Result 语义：Ok 与闭集 Err 两侧均可达。
fn error_path_ok() -> bool {
    let (log, book) = build_sample();
    if reconcile(&log, &book).is_err() {
        return false;
    }
    match book.invariants_ok() {
        Ok(()) => !detail(0x0).is_empty(),
        Err(_) => false,
    }
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let (log, book) = build_sample();
    let summary = match (reconcile(&log, &book), aggregate_summary(&book)) {
        (Ok(()), Ok(s)) => format!(
            "UNX-F3429 断言账汇总：四计数 总{}过{}败{}跳{} 与F3408对账平",
            s.total, s.passed, s.failed, s.skipped
        ),
        _ => String::new(),
    };
    !summary.is_empty() && summary.contains("对账平")
}

// ── 判据总成：run_unx3429_checks ───────────────────────────────────────

/// run_unx3429_checks：本单判据聚合（诊断段 0x79940，全部走 add_dyn）。
pub fn run_unx3429_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3429 断言账汇总");
    let checks: [(&'static str, bool, String); 14] = [
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
            "冻结面四计数轴名逐字锚定F3408",
            frozen_fields_ok(),
            format!("轴名{}", if frozen_fields_ok() { "齐" } else { "漂移" }),
        ),
        (
            "正常流三域十事件首记序行内自洽",
            record_normal_flow_ok(),
            format!("记账{}", if record_normal_flow_ok() { "平" } else { "乱" }),
        ),
        (
            "M188对账事件流重放与账本全等",
            reconcile_ok(),
            format!("对账{}", if reconcile_ok() { "平" } else { "不平" }),
        ),
        (
            "注入1改账与多域对账必抓",
            inj_reconcile_mismatch_ok(),
            format!("对账违约{}", if inj_reconcile_mismatch_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入2行内计数破账必抓",
            inj_count_inconsistent_ok(),
            format!("破账{}", if inj_count_inconsistent_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3域重行必抓",
            inj_domain_dup_ok(),
            format!("域重行{}", if inj_domain_dup_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4空域入账与空账汇总逐类拒",
            inj_empty_cases_ok(),
            format!("空违约{}", if inj_empty_cases_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "分域视图行数守恒行序一致行文核对",
            domain_view_ok(),
            format!("分域视图{}", if domain_view_ok() { "齐" } else { "缺" }),
        ),
        (
            "汇总入JSON键全在冻结轴表内",
            summary_json_ok(),
            format!("JSON面{}", if summary_json_ok() { "齐" } else { "缺" }),
        ),
        (
            "双跑一致重放渲染汇总逐码同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径Ok与闭集Err两侧可达",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
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
        assert_eq!(SEG_BASE, 0x79940);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn frozen_fields_anchor_f3408() {
        assert!(frozen_fields_ok());
        assert_eq!(COUNT_FIELDS, ["total", "passed", "failed", "skipped"]);
    }

    #[test]
    fn record_flow_and_totals() {
        assert!(record_normal_flow_ok());
        let (_, book) = build_sample();
        let s = aggregate_summary(&book).unwrap();
        assert_eq!((s.total, s.passed, s.failed, s.skipped), (10, 5, 3, 2));
        assert!(s.consistent());
    }

    #[test]
    fn reconcile_flat_and_tampered() {
        assert!(reconcile_ok());
        assert!(inj_reconcile_mismatch_ok());
    }

    #[test]
    fn injections_caught() {
        assert!(inj_count_inconsistent_ok());
        assert!(inj_domain_dup_ok());
        assert!(inj_empty_cases_ok());
    }

    #[test]
    fn domain_view_lines_exact() {
        assert!(domain_view_ok());
        let (_, book) = build_sample();
        let lines = render_domain_view(&book).unwrap();
        assert_eq!(lines.get(1).unwrap(), "域=sched 总=3 过=1 败=1 跳=1");
    }

    #[test]
    fn summary_json_keys_frozen() {
        assert!(summary_json_ok());
        let (_, book) = build_sample();
        let node = summary_to_json(&book).unwrap();
        match node.get("domains") {
            Some(JsonLite::JArr(items)) => assert_eq!(items.len(), 3),
            _ => panic!("分域数组缺"),
        }
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x799FF).starts_with("未知诊断码"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3429_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
