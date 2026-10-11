//! unx741_domain_journal ↔ UNX-F0741 域经登记簿建档（全域条目汇总入簿）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0741，UNX-A1-B37）：
//! 「域经登记簿建档（全域条目汇总入簿）| 570 | 已深化 |
//!  UNX-F0741-J1 登记簿五列齐（编号/主题/来源批/证据指针/价值档）
//!  且全域条目 ≥40 条汇总入簿、B01–B37 逐批有源或零沉淀声明」
//!
//! 设计要点：
//! 1. **五列登记**：`JournalRow` 五列（编号/主题/来源批/证据指针/
//!     价值档）逐条入簿；缺列即 `ColumnMissing` 点名列名——
//!     缺证据指针的条目 = 不可核验的宣称（最常見假账源）。
//! 2. **≥40 条硬线**：`completeness_check()` 核「登记簿条数 ≥40」
//!     且「B01–B37 每批都有登记或有零沉淀声明」——逐批核，
//!     禁「总量够就过」（某批整批漏登会被总量掩盖）。
//! 3. **零沉淀声明**：无沉淀的批次必须显式登记
//!     `BarrenDeclaration`（本批无沉淀是合法结论，但必须声明），
//!     未声明即 `BatchUnaccounted`。
//! 4. **价值档闭集**：`ValueGrade` 四档（金/银/铜/登记），
//!     档位排序可用于「经验复用检索」。
//! 5. **编号唯一**：域经编号全域唯一（重号 = 两条经验抢同一
//!     编号，引用即错位）。
//!
//! 诊断码：`JournalCode` 0x8F 段（0x8F00..=0x8F0F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX741_JOURNAL_VERSION: &str = "unx741-domain-journal-v1";

/// 条目硬线（≥40）。
pub const ENTRY_HARDLINE: usize = 40;

/// 批次数（B01–B37）。
pub const BATCH_SPAN: usize = 37;

/// 簿容量。
pub const JOURNAL_CAP: usize = 256;

/// 价值档（四档闭集，序 = 价值降序）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ValueGrade {
    /// 金（方法论级，跨域可复用）
    Gold,
    /// 银（域内可复用）
    Silver,
    /// 铜（单点参考）
    Bronze,
    /// 登记（仅留痕）
    Record,
}

impl ValueGrade {
    /// 全部（降序）。
    pub const ALL: [ValueGrade; 4] = [
        ValueGrade::Gold,
        ValueGrade::Silver,
        ValueGrade::Bronze,
        ValueGrade::Record,
    ];

    /// 中文档名。
    pub const fn zh(self) -> &'static str {
        match self {
            ValueGrade::Gold => "金",
            ValueGrade::Silver => "银",
            ValueGrade::Bronze => "铜",
            ValueGrade::Record => "登记",
        }
    }

    /// 线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            ValueGrade::Gold => "GOLD",
            ValueGrade::Silver => "SILVER",
            ValueGrade::Bronze => "BRONZE",
            ValueGrade::Record => "RECORD",
        }
    }
}

/// 登记簿一行（五列）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JournalRow {
    /// 域经编号（全域唯一）
    pub no: String,
    /// 主题
    pub theme: String,
    /// 来源批（B01..B37）
    pub batch: String,
    /// 证据指针（可解引，禁空）
    pub pointer: String,
    /// 价值档
    pub grade: ValueGrade,
}

/// 域经登记簿。
pub struct DomainJournal {
    rows: Vec<JournalRow>,
    /// 零沉淀声明（批号）
    barren: Vec<String>,
}

/// 登记错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JournalError {
    /// 缺列（点名列）
    ColumnMissing { column: &'static str },
    /// 编号重复
    DuplicateNo { no: String },
    /// 批号格式非法
    BatchBad { batch: String },
    /// 条目硬线不足
    EntryShort { got: usize, hardline: usize },
    /// 批次无着落（无登记也无零沉淀声明）
    BatchUnaccounted { batch: String },
    /// 簿满
    JournalFull { cap: usize },
    /// 指针格式非法
    PointerBad { no: String },
}

/// 诊断码（0x8F 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JournalCode {
    /// 0x8F00 缺列
    ColumnMissing,
    /// 0x8F01 编号重复
    Duplicate,
    /// 0x8F02 批号非法
    BatchBad,
    /// 0x8F03 条目硬线不足
    EntryShort,
    /// 0x8F04 批次无着落
    BatchUnaccounted,
    /// 0x8F05 簿满
    Full,
    /// 0x8F06 指针非法
    PointerBad,
    /// 0x8F07 建档通过
    Established,
}

impl JournalCode {
    pub const fn code(self) -> u16 {
        match self {
            JournalCode::ColumnMissing => 0x8F00,
            JournalCode::Duplicate => 0x8F01,
            JournalCode::BatchBad => 0x8F02,
            JournalCode::EntryShort => 0x8F03,
            JournalCode::BatchUnaccounted => 0x8F04,
            JournalCode::Full => 0x8F05,
            JournalCode::PointerBad => 0x8F06,
            JournalCode::Established => 0x8F07,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            JournalCode::ColumnMissing => "JOURNAL-COLUMN-MISSING",
            JournalCode::Duplicate => "JOURNAL-DUPLICATE",
            JournalCode::BatchBad => "JOURNAL-BATCH-BAD",
            JournalCode::EntryShort => "JOURNAL-ENTRY-SHORT",
            JournalCode::BatchUnaccounted => "JOURNAL-BATCH-UNACCOUNTED",
            JournalCode::Full => "JOURNAL-FULL",
            JournalCode::PointerBad => "JOURNAL-POINTER-BAD",
            JournalCode::Established => "JOURNAL-ESTABLISHED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            JournalCode::ColumnMissing => "登记簿缺列：五列（编号/主题/来源批/证据指针/价值档）缺一即拒",
            JournalCode::Duplicate => "域经编号重复：全域唯一性被破坏",
            JournalCode::BatchBad => "批号格式非法：必须 B + 两位数字（B01..B37）",
            JournalCode::EntryShort => "登记条目硬线不足：未达 40 条",
            JournalCode::BatchUnaccounted => "批次无着落：该批既无登记也无零沉淀声明",
            JournalCode::Full => "登记簿满：256 条已满",
            JournalCode::PointerBad => "证据指针格式非法：必须 <路径>#<编号> 形态",
            JournalCode::Established => "域经登记簿建档通过：五列齐 + 硬线达 + 逐批有着落",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx741_domain_journal.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8F00 && (c & 0x00FF) < 0x10
    }
}

impl JournalError {
    pub const fn code(&self) -> JournalCode {
        match self {
            JournalError::ColumnMissing { .. } => JournalCode::ColumnMissing,
            JournalError::DuplicateNo { .. } => JournalCode::Duplicate,
            JournalError::BatchBad { .. } => JournalCode::BatchBad,
            JournalError::EntryShort { .. } => JournalCode::EntryShort,
            JournalError::BatchUnaccounted { .. } => JournalCode::BatchUnaccounted,
            JournalError::JournalFull { .. } => JournalCode::Full,
            JournalError::PointerBad { .. } => JournalCode::PointerBad,
        }
    }

    pub fn detail(&self) -> String {
        self.code().detail()
    }
}

/// 批号格式核（B01..B37）。
fn batch_ok(b: &str) -> bool {
    if b.len() != 3 || !b.starts_with('B') {
        return false;
    }
    let n = match b[1..3].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return false,
    };
    n >= 1 && n as usize <= BATCH_SPAN
}

impl DomainJournal {
    /// 空簿。
    pub fn new() -> Self {
        DomainJournal {
            rows: Vec::new(),
            barren: Vec::new(),
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 入簿一条（五列 + 编号唯一 + 指针格式）。
    pub fn enroll(&mut self, r: JournalRow) -> Result<(), JournalError> {
        if r.no.is_empty() {
            return Err(JournalError::ColumnMissing { column: "no" });
        }
        if r.theme.is_empty() {
            return Err(JournalError::ColumnMissing { column: "theme" });
        }
        if r.batch.is_empty() {
            return Err(JournalError::ColumnMissing { column: "batch" });
        }
        if r.pointer.is_empty() {
            return Err(JournalError::ColumnMissing { column: "pointer" });
        }
        if !batch_ok(&r.batch) {
            return Err(JournalError::BatchBad {
                batch: r.batch.clone(),
            });
        }
        // 指针格式（<路径>#<编号>）
        let ok = match r.pointer.find('#') {
            Some(i) => i > 0 && i + 1 < r.pointer.len(),
            None => false,
        };
        if !ok {
            return Err(JournalError::PointerBad { no: r.no.clone() });
        }
        if self.rows.len() >= JOURNAL_CAP {
            return Err(JournalError::JournalFull { cap: JOURNAL_CAP });
        }
        for e in self.rows.iter() {
            if e.no == r.no {
                return Err(JournalError::DuplicateNo { no: r.no.clone() });
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 登记零沉淀声明（批号合法 + 未重复声明）。
    pub fn declare_barren(&mut self, batch: &str) -> Result<(), JournalError> {
        if !batch_ok(batch) {
            return Err(JournalError::BatchBad {
                batch: String::from(batch),
            });
        }
        for b in self.barren.iter() {
            if b == batch {
                return Ok(());
            }
        }
        self.barren.push(String::from(batch));
        Ok(())
    }

    /// 完备性核（条数硬线 + B01–B37 逐批有着落）。
    pub fn completeness_check(&self) -> Result<u32, JournalError> {
        if self.rows.len() < ENTRY_HARDLINE {
            return Err(JournalError::EntryShort {
                got: self.rows.len(),
                hardline: ENTRY_HARDLINE,
            });
        }
        let mut i = 1usize;
        while i <= BATCH_SPAN {
            let b = format!("B{:02}", i);
            let mut has = false;
            for r in self.rows.iter() {
                if r.batch == b {
                    has = true;
                    break;
                }
            }
            if !has {
                let mut declared = false;
                for d in self.barren.iter() {
                    if *d == b {
                        declared = true;
                        break;
                    }
                }
                if !declared {
                    return Err(JournalError::BatchUnaccounted { batch: b });
                }
            }
            i += 1;
        }
        Ok(self.rows.len() as u32)
    }

    /// 价值档直方图。
    pub fn grade_histogram(&self) -> [u32; 4] {
        let mut out = [0u32; 4];
        for r in self.rows.iter() {
            let i = match r.grade {
                ValueGrade::Gold => 0,
                ValueGrade::Silver => 1,
                ValueGrade::Bronze => 2,
                ValueGrade::Record => 3,
            };
            if let Some(v) = out.get_mut(i) {
                *v += 1;
            }
        }
        out
    }

    /// 簿指纹（编号序折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for r in self.rows.iter() {
            for b in r.no.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h ^= r.grade.wire().len() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

impl Default for DomainJournal {
    fn default() -> Self {
        DomainJournal::new()
    }
}

/// 域自检。
pub fn run_unx741_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx741");

    // 灌 45 条（B01..B20）+ B21..B37 零沉淀声明
    let build = || -> DomainJournal {
        let mut j = DomainJournal::new();
        let mut i = 0usize;
        while i < 45 {
            let batch = format!("B{:02}", (i % 20) + 1);
            let grade = ValueGrade::ALL[i % 4];
            let _ = j.enroll(JournalRow {
                no: format!("DJ-{:03}", i),
                theme: format!("theme-{}", i),
                batch,
                pointer: format!("docs/x.md#X-{:03}", i),
                grade,
            });
            i += 1;
        }
        let mut b = 21usize;
        while b <= BATCH_SPAN {
            let _ = j.declare_barren(&format!("B{:02}", b));
            b += 1;
        }
        j
    };

    // C741-01 五列齐 + 硬线达 + 逐批有着落
    {
        let j = build();
        let r = j.completeness_check();
        let good = j.len() == 45 && r == Ok(45);
        s.add("C741-01-建档完备性核", good, if good { "" } else { "完备性失败" });
    }

    // C741-02 编号重复拒收
    {
        let mut j = DomainJournal::new();
        let mk = |n: u32| JournalRow {
            no: format!("DJ-{:03}", n),
            theme: String::from("t"),
            batch: String::from("B01"),
            pointer: format!("docs/x.md#X-{:03}", n),
            grade: ValueGrade::Gold,
        };
        let _ = j.enroll(mk(1));
        let r = j.enroll(mk(1));
        let good = matches!(r, Err(JournalError::DuplicateNo { .. }));
        s.add("C741-02-编号唯一", good, if good { "" } else { "重号被放行" });
    }

    // C741-03 缺列点名
    {
        let mut j = DomainJournal::new();
        let r = j.enroll(JournalRow {
            no: String::from("DJ-001"),
            theme: String::new(),
            batch: String::from("B01"),
            pointer: String::from("docs/x.md#X-1"),
            grade: ValueGrade::Gold,
        });
        let good = matches!(r, Err(JournalError::ColumnMissing { column: "theme" }));
        s.add("C741-03-缺列点名", good, if good { "" } else { "缺列未点名" });
    }

    // C741-04 批号非法拒收
    {
        let mut j = DomainJournal::new();
        let r = j.enroll(JournalRow {
            no: String::from("DJ-001"),
            theme: String::from("t"),
            batch: String::from("B99"),
            pointer: String::from("docs/x.md#X-1"),
            grade: ValueGrade::Gold,
        });
        let good = matches!(r, Err(JournalError::BatchBad { .. }));
        s.add("C741-04-批号 B99 拒收", good, if good { "" } else { "批号非法被放行" });
    }

    // C741-05 批次无着落点名（B05 未登记未声明）
    {
        let j = build();
        // 摘掉 B05 的登记行 + 声明（重建簿只留其它批）
        let mut j2 = DomainJournal::new();
        let mut i = 0usize;
        while i < 45 {
            let batch = format!("B{:02}", (i % 20) + 1);
            if batch == "B05" {
                i += 1;
                continue;
            }
            let _ = j2.enroll(JournalRow {
                no: format!("DJ-{:03}", i),
                theme: format!("theme-{}", i),
                batch,
                pointer: format!("docs/x.md#X-{:03}", i),
                grade: ValueGrade::ALL[i % 4],
            });
            i += 1;
        }
        let mut b = 21usize;
        while b <= BATCH_SPAN {
            if b != 5 {
                let _ = j2.declare_barren(&format!("B{:02}", b));
            }
            b += 1;
        }
        // 若 B05 已被 barren 声明过则先清（簿不可删，改用无 B05 登记的簿）
        let r = j2.completeness_check();
        let good = matches!(r, Err(JournalError::BatchUnaccounted { .. })) && j.len() == 45;
        s.add("C741-05-批次无着落拦截", good, if good { "" } else { "漏批被放过" });
    }

    // C741-06 条目硬线不足（只 39 条）
    {
        let mut j = DomainJournal::new();
        let mut i = 0usize;
        while i < 39 {
            let _ = j.enroll(JournalRow {
                no: format!("DJ-{:03}", i),
                theme: String::from("t"),
                batch: format!("B{:02}", (i % 37) + 1),
                pointer: format!("docs/x.md#X-{:03}", i),
                grade: ValueGrade::Gold,
            });
            i += 1;
        }
        let r = j.completeness_check();
        let good = matches!(r, Err(JournalError::EntryShort { got: 39, hardline: 40 }));
        s.add("C741-06-硬线不足点名", good, if good { "" } else { "硬线不足被放行" });
    }

    // C741-07 价值档直方图
    {
        let j = build();
        let h = j.grade_histogram();
        let total: u32 = h.iter().sum();
        let good = total == 45;
        s.add("C741-07-价值档直方图", good, if good { "" } else { "直方图不平" });
    }

    // C741-08 簿指纹确定
    {
        let a = build();
        let b = build();
        let good = a.fingerprint() == b.fingerprint() && a.fingerprint() != 0;
        s.add("C741-08-簿指纹确定", good, if good { "" } else { "指纹不确定" });
    }

    // C741-09 指针格式非法拒收
    {
        let mut j = DomainJournal::new();
        let r = j.enroll(JournalRow {
            no: String::from("DJ-001"),
            theme: String::from("t"),
            batch: String::from("B01"),
            pointer: String::from("docs/x.md"),
            grade: ValueGrade::Gold,
        });
        let good = matches!(r, Err(JournalError::PointerBad { .. }));
        s.add("C741-09-指针格式核", good, if good { "" } else { "格式非法被放行" });
    }

    // C741-10 诊断码段独占
    {
        let codes = [
            JournalCode::ColumnMissing,
            JournalCode::Duplicate,
            JournalCode::BatchBad,
            JournalCode::EntryShort,
            JournalCode::BatchUnaccounted,
            JournalCode::Full,
            JournalCode::PointerBad,
            JournalCode::Established,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C741-10-诊断码 0x8F 段独占", all, if all { "" } else { "码值越出 0x8F 段" });
    }

    s
}
