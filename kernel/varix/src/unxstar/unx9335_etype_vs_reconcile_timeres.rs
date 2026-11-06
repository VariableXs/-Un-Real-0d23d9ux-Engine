//! UNX-F9335 时间/资源族 E 型与 B06/B19 深化册对账格 · 0x7C140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9335 逐字）：
//!   UNX-F9335-J1 对账判据过，与时间族/资源族深化册逐条零冲突格过，
//!   正反分层声明格过。
//!
//! 设计要点：
//!   ① 对账口径不是「再跑一遍探针」，而是「把探针产出的字段报告逐行并回
//!      深化册」：册（BOOK）是期望值，报告（Report）是观测值，二者按行
//!      标识一一配对。配对只认标识，不认位置，因此册与报告可以不同序。
//!   ② 册共 22 行：B06 时间族 12 行（标识 0x0601..=0x060C）、B19 资源族
//!      10 行（标识 0x1301..=0x130A）。族=标识高字节，行=低字节，行标识
//!      天然携带族信息，杜绝跨族串行。
//!   ③ 报告卡六项症状分开记账：missing（册有报告无）、dup（报告多出同名
//!      行）、mismatch（观测码与期望码不等）、polarity（成功/失败极性被
//!      翻转）、order（报告未按标识严格升序）、unknown（报告含册外标识）。
//!      冲突总数 = missing + dup + mismatch + order + unknown，polarity 是
//!      mismatch 的子类标签，不重复计入总数。
//!   ④ 注入账十场景 ×10 轮 = 100 次：重复行、缺行、六个邻位交换位、判定
//!      篡改、极性翻转。每个场景都必须在 10/10 轮被检出；账本恒满足
//!      injected == detected + escaped 且 escaped == 0。
//!   ⑤ 正反分层声明：正层=期望成功（errno 0）的 6 行，负层=期望失败的
//!      16 行。两层位图必须互斥、并集恰为 22 位全掩码、逐位与册的极性
//!      一致；这是「正反分层声明格」的硬约束。
//!   ⑥ 深化册对账：BOOK 与 DEEP 是两份独立手抄账（版本 6 与 19，版次和
//!      25 冻结），逐行比较 errno/nargs/指针槽/版次，失配数必须为 0；
//!      DEEP 故意按资源族在前、时间族在后的顺序排列，证明按标识配对。
//!   ⑦ 阶段机 Collect→Validate→Compare→Declare→Sealed 只许前进；后退
//!      跳转必须被拒并计入 rejected，防止对账流程在阶段间回摆。
//!   ⑧ 指纹自写 FNV-1a 32 位，逐字节喂入册与报告，双跑必须相等；诊断码
//!      码值 = SEG_BASE + 1 + 段内序号，全闭集两两唯一。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9335 独占 0x7C140..=0x7C17F）。
pub const SEG_BASE: u32 = 0x7C140;

/// 册总行数。
pub const BOOK_ROWS: usize = 22;
/// 时间族 B06 行数。
pub const TIME_ROWS: usize = 12;
/// 资源族 B19 行数。
pub const RESOURCE_ROWS: usize = 10;
/// 单场景注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 邻位交换位个数。
pub const SWAP_SLOTS: usize = 6;
/// 册内 nargs 合计（冻结值）。
pub const NARGS_SUM: u32 = 62;
/// 正层行数。
pub const POSITIVE_ROWS: u32 = 6;
/// 负层行数。
pub const NEGATIVE_ROWS: u32 = 16;
/// 22 位全掩码。
pub const FULL_MASK: u32 = 0x003F_FFFF;
/// 正层位图（行 0/4/6/10/14/20）。
pub const POSITIVE_MASK: u32 = 0x0010_4451;
/// 负层位图（全掩码去掉正层）。
pub const NEGATIVE_MASK: u32 = 0x002F_BBAE;
/// B06 深化册版次。
pub const DEEP_B06_EDITION: u16 = 6;
/// B19 深化册版次。
pub const DEEP_B19_EDITION: u16 = 19;
/// 两册版次之和（冻结）。
pub const DEEP_PAIR_SUM: u16 = 25;
/// 可空参数行数。
pub const NULLABLE_ROWS: usize = 6;
/// 容量门行数。
pub const CAPACITY_ROWS: usize = 1;

pub const EPERM: i32 = 1;
pub const EAGAIN: i32 = 11;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;

/// 本模块诊断码（段内 +0x01..=+0x10 闭集，共 16 码）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconCode {
    SegmentViolation, BookShapeDrift, BookIdDup, BookIdOrder,
    ErrnoOutside, DeepConflict, DeepEditionDrift, PointerSlotBad,
    LayerImbalance, LayerOverlap, CleanReconcileFail, InjectionLeak,
    FingerprintDrift, StageRegression, NargsDrift, CodeValueDup,
}

impl ReconCode {
    /// 段内序号。
    pub fn code(self) -> u16 { self as u16 }
    /// 段内码值：SEG_BASE + 1 + 序号。
    pub fn val(self) -> u32 { SEG_BASE + 1 + self as u32 }
    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            ReconCode::SegmentViolation => "段位越界：0x7C140 必须 0x40 对齐且落在 0x7C000..=0x7D1C0",
            ReconCode::BookShapeDrift => "册形漂移：时间族必须 12 行、资源族必须 10 行，合计 22 行",
            ReconCode::BookIdDup => "行标识重复：22 行标识必须两两唯一，重复会被对账重复计数",
            ReconCode::BookIdOrder => "行标识失序：册内标识必须严格升序，族切换处也不许回退",
            ReconCode::ErrnoOutside => "期望码越域：册内只许出现 0/EPERM/EAGAIN/EFAULT/EINVAL 五个闭集值",
            ReconCode::DeepConflict => "深化册冲突：BOOK 与 B06/B19 的 errno/nargs/槽位必须逐行相等",
            ReconCode::DeepEditionDrift => "深化册版次漂移：B06=6、B19=19、和为 25，三值都不许改",
            ReconCode::PointerSlotBad => "指针槽越界：每行指针实参序号必须小于 nargs",
            ReconCode::LayerImbalance => "分层失衡：正层必须 6 行、负层必须 16 行，合计 22",
            ReconCode::LayerOverlap => "分层重叠：正层与负层位图必须互斥且并集为全掩码",
            ReconCode::CleanReconcileFail => "空报表不干净：无注入的字段报告对账后冲突必须为 0",
            ReconCode::InjectionLeak => "注入账开裂：注入数必须等于检出数加逃逸数，逃逸恒 0",
            ReconCode::FingerprintDrift => "指纹漂移：同一报表两次 FNV-1a 必须逐位相等",
            ReconCode::StageRegression => "阶段回摆：对账阶段机只许前进，后退跳转必须被拒",
            ReconCode::NargsDrift => "nargs 合计漂移：22 行 nargs 之和冻结为 62",
            ReconCode::CodeValueDup => "诊断码重复：段内 16 个码值必须两两唯一",
        }
    }
    /// 闭集全量。
    pub const ALL: &'static [ReconCode] = &[
        ReconCode::SegmentViolation, ReconCode::BookShapeDrift, ReconCode::BookIdDup, ReconCode::BookIdOrder,
        ReconCode::ErrnoOutside, ReconCode::DeepConflict, ReconCode::DeepEditionDrift, ReconCode::PointerSlotBad,
        ReconCode::LayerImbalance, ReconCode::LayerOverlap, ReconCode::CleanReconcileFail, ReconCode::InjectionLeak,
        ReconCode::FingerprintDrift, ReconCode::StageRegression, ReconCode::NargsDrift, ReconCode::CodeValueDup,
    ];
}

/// 册所属族。族编号写进行标识高字节（0x06 / 0x13）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Time,
    Resource,
}

impl Family {
    pub fn code(self) -> u8 {
        match self { Family::Time => 0x06, Family::Resource => 0x13 }
    }
    pub fn name(self) -> &'static str {
        match self { Family::Time => "B06-time", Family::Resource => "B19-resource" }
    }
}

/// 行标志位：正层、可空、写方向、容量门。
pub const FLAG_POSITIVE: u8 = 0x01;
pub const FLAG_NULLABLE: u8 = 0x02;
pub const FLAG_WRITE: u8 = 0x04;
pub const FLAG_CAPACITY: u8 = 0x08;

/// 册行：期望码、实参个数、指针槽位、标志位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    pub id: u16, pub family: Family, pub name: &'static str, pub expected: i32,
    pub nargs: u8, pub slot: u8, pub flags: u8,
}

impl Row {
    pub const fn new(id: u16, family: Family, name: &'static str, expected: i32,
                     nargs: u8, slot: u8, flags: u8) -> Row {
        Row { id, family, name, expected, nargs, slot, flags }
    }
    pub fn is_positive(self) -> bool { self.expected == 0 }
    pub fn nullable(self) -> bool { self.flags & FLAG_NULLABLE != 0 }
    pub fn capacity_gate(self) -> bool { self.flags & FLAG_CAPACITY != 0 }
}

/// 22 行册（B06 十二行在前，B19 十行在后）。
pub const BOOK: [Row; BOOK_ROWS] = [
    Row::new(0x0601, Family::Time, "clock_gettime(CLOCK_REALTIME, tp)", 0, 2, 1, FLAG_POSITIVE | FLAG_WRITE),
    Row::new(0x0602, Family::Time, "clock_gettime(CLOCK_REALTIME, NULL)", EFAULT, 2, 1, FLAG_WRITE),
    Row::new(0x0603, Family::Time, "clock_settime(CLOCK_MONOTONIC, tp)", EPERM, 2, 1, FLAG_WRITE),
    Row::new(0x0604, Family::Time, "clock_settime(CLOCK_REALTIME, bad)", EFAULT, 2, 1, FLAG_WRITE),
    Row::new(0x0605, Family::Time, "clock_nanosleep(rem=NULL)", 0, 4, 3, FLAG_POSITIVE | FLAG_NULLABLE | FLAG_WRITE),
    Row::new(0x0606, Family::Time, "clock_nanosleep(rem=bad)", EFAULT, 4, 3, FLAG_WRITE),
    Row::new(0x0607, Family::Time, "timer_create(sevp=NULL)", 0, 3, 2, FLAG_POSITIVE | FLAG_NULLABLE | FLAG_WRITE),
    Row::new(0x0608, Family::Time, "timer_create(ninth timer)", EAGAIN, 3, 2, FLAG_NULLABLE | FLAG_WRITE | FLAG_CAPACITY),
    Row::new(0x0609, Family::Time, "timer_settime(new=bad)", EFAULT, 4, 2, FLAG_WRITE),
    Row::new(0x060A, Family::Time, "timer_gettime(curr=NULL)", EFAULT, 2, 1, FLAG_WRITE),
    Row::new(0x060B, Family::Time, "nanosleep(rem=NULL)", 0, 2, 1, FLAG_POSITIVE | FLAG_NULLABLE | FLAG_WRITE),
    Row::new(0x060C, Family::Time, "getitimer(curr=NULL)", EFAULT, 2, 1, FLAG_WRITE),
    Row::new(0x1301, Family::Resource, "getrlimit(rlim=bad)", EFAULT, 2, 1, FLAG_WRITE),
    Row::new(0x1302, Family::Resource, "setrlimit(rlim=bad)", EFAULT, 2, 1, FLAG_WRITE),
    Row::new(0x1303, Family::Resource, "prlimit64(new=NULL)", 0, 4, 2, FLAG_POSITIVE | FLAG_NULLABLE | FLAG_WRITE),
    Row::new(0x1304, Family::Resource, "prlimit64(old=bad)", EFAULT, 4, 3, FLAG_WRITE),
    Row::new(0x1305, Family::Resource, "getrusage(usage=bad)", EFAULT, 2, 1, FLAG_WRITE),
    Row::new(0x1306, Family::Resource, "getrusage(who=-2)", EINVAL, 2, 0, 0),
    Row::new(0x1307, Family::Resource, "sched_getaffinity(mask=bad)", EFAULT, 3, 2, FLAG_WRITE),
    Row::new(0x1308, Family::Resource, "sched_setaffinity(mask=bad)", EFAULT, 3, 2, 0),
    Row::new(0x1309, Family::Resource, "wait4(rusage=NULL)", 0, 4, 3, FLAG_POSITIVE | FLAG_NULLABLE | FLAG_WRITE),
    Row::new(0x130A, Family::Resource, "wait4(rusage=bad)", EFAULT, 4, 3, FLAG_WRITE),
];

/// 深化册行：字段顺序与册不同，且不带名称，强调只按标识/数值对账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeepRow {
    pub expected: i32,
    pub id: u16,
    pub nargs: u8,
    pub slot: u8,
    pub edition: u16,
}

impl DeepRow {
    pub const fn new(id: u16, expected: i32, nargs: u8, slot: u8, edition: u16) -> DeepRow {
        DeepRow { expected, id, nargs, slot, edition }
    }
}

/// 深化册（资源族在前、时间族在后：顺序故意错开，验证按标识配对）。
pub const DEEP: [DeepRow; BOOK_ROWS] = [
    DeepRow::new(0x1301, EFAULT, 2, 1, DEEP_B19_EDITION),
    DeepRow::new(0x1302, EFAULT, 2, 1, DEEP_B19_EDITION),
    DeepRow::new(0x1303, 0, 4, 2, DEEP_B19_EDITION),
    DeepRow::new(0x1304, EFAULT, 4, 3, DEEP_B19_EDITION),
    DeepRow::new(0x1305, EFAULT, 2, 1, DEEP_B19_EDITION),
    DeepRow::new(0x1306, EINVAL, 2, 0, DEEP_B19_EDITION),
    DeepRow::new(0x1307, EFAULT, 3, 2, DEEP_B19_EDITION),
    DeepRow::new(0x1308, EFAULT, 3, 2, DEEP_B19_EDITION),
    DeepRow::new(0x1309, 0, 4, 3, DEEP_B19_EDITION),
    DeepRow::new(0x130A, EFAULT, 4, 3, DEEP_B19_EDITION),
    DeepRow::new(0x0601, 0, 2, 1, DEEP_B06_EDITION),
    DeepRow::new(0x0602, EFAULT, 2, 1, DEEP_B06_EDITION),
    DeepRow::new(0x0603, EPERM, 2, 1, DEEP_B06_EDITION),
    DeepRow::new(0x0604, EFAULT, 2, 1, DEEP_B06_EDITION),
    DeepRow::new(0x0605, 0, 4, 3, DEEP_B06_EDITION),
    DeepRow::new(0x0606, EFAULT, 4, 3, DEEP_B06_EDITION),
    DeepRow::new(0x0607, 0, 3, 2, DEEP_B06_EDITION),
    DeepRow::new(0x0608, EAGAIN, 3, 2, DEEP_B06_EDITION),
    DeepRow::new(0x0609, EFAULT, 4, 2, DEEP_B06_EDITION),
    DeepRow::new(0x060A, EFAULT, 2, 1, DEEP_B06_EDITION),
    DeepRow::new(0x060B, 0, 2, 1, DEEP_B06_EDITION),
    DeepRow::new(0x060C, EFAULT, 2, 1, DEEP_B06_EDITION),
];

/// 按标识查册行（线性扫描，不依赖位置）。
pub fn book_by_id(id: u16) -> Option<&'static Row> {
    BOOK.iter().find(|row| row.id == id)
}

/// 按标识查深化册行。
pub fn deep_by_id(id: u16) -> Option<&'static DeepRow> {
    DEEP.iter().find(|row| row.id == id)
}

/// 两族行数。
pub fn family_counts() -> (usize, usize) {
    let mut time = 0usize;
    let mut resource = 0usize;
    for row in BOOK.iter() {
        match row.family {
            Family::Time => time += 1,
            Family::Resource => resource += 1,
        }
    }
    (time, resource)
}

/// 带某标志位的行数。
pub fn count_flags(flag: u8) -> usize {
    BOOK.iter().filter(|row| row.flags & flag != 0).count()
}

/// nargs 合计。
pub fn nargs_total() -> u32 {
    let mut sum = 0u32;
    for row in BOOK.iter() { sum = sum.saturating_add(row.nargs as u32); }
    sum
}

/// 重复行标识个数。
pub fn duplicate_ids() -> u32 {
    let mut dup = 0u32;
    for (i, a) in BOOK.iter().enumerate() {
        for (j, b) in BOOK.iter().enumerate() {
            if i < j && a.id == b.id { dup = dup.saturating_add(1); }
        }
    }
    dup
}

/// 标识严格升序。
pub fn ids_ascending() -> bool {
    let mut prev: Option<u16> = None;
    for row in BOOK.iter() {
        if let Some(p) = prev { if row.id <= p { return false; } }
        prev = Some(row.id);
    }
    true
}

/// 期望码是否在闭集 {0, EPERM, EAGAIN, EFAULT, EINVAL} 内。
pub fn errno_domain_ok() -> bool {
    BOOK.iter().all(|row| {
        row.expected == 0 || row.expected == EPERM || row.expected == EAGAIN
            || row.expected == EFAULT || row.expected == EINVAL
    })
}

/// 每行指针槽位必须严格小于 nargs。
pub fn pointer_slots_ok() -> bool {
    BOOK.iter().all(|row| row.slot < row.nargs)
}

/// 正层位图：期望成功的行为 1。
pub fn positive_mask() -> u32 {
    let mut mask = 0u32;
    for (i, row) in BOOK.iter().enumerate() {
        if row.is_positive() { mask |= 1u32 << (i as u32); }
    }
    mask
}

/// 负层位图。
pub fn negative_mask() -> u32 {
    let mut mask = 0u32;
    for (i, row) in BOOK.iter().enumerate() {
        if !row.is_positive() { mask |= 1u32 << (i as u32); }
    }
    mask
}

/// 正反分层声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayerDecl {
    pub positive: u32, pub negative: u32,
}

impl LayerDecl {
    pub fn from_book() -> LayerDecl {
        LayerDecl { positive: positive_mask(), negative: negative_mask() }
    }
    /// 声明自检：互斥、并集满、层数正确、与冻结位图一致。
    pub fn validate(&self) -> bool {
        let disjoint = (self.positive & self.negative) == 0;
        let full = (self.positive | self.negative) == FULL_MASK;
        let pos_ok = self.positive.count_ones() == POSITIVE_ROWS;
        let neg_ok = self.negative.count_ones() == NEGATIVE_ROWS;
        let frozen = self.positive == POSITIVE_MASK && self.negative == NEGATIVE_MASK;
        disjoint && full && pos_ok && neg_ok && frozen
    }
    /// 与册逐位对账，返回失配位数（干净值 0）。
    pub fn conflicts_with_book(&self) -> u32 {
        let (want_pos, want_neg) = (positive_mask(), negative_mask());
        let mut diff = 0u32;
        for (i, row) in BOOK.iter().enumerate() {
            let bit = 1u32 << (i as u32);
            let declared_pos = self.positive & bit != 0;
            let declared_neg = self.negative & bit != 0;
            if declared_pos != row.is_positive() || declared_neg == row.is_positive() {
                diff = diff.saturating_add(1);
            }
            if (want_pos & bit != 0) != declared_pos || (want_neg & bit != 0) != declared_neg {
                diff = diff.saturating_add(1);
            }
        }
        diff
    }
}

/// 分层声明入口。
pub fn layer_decl() -> LayerDecl {
    LayerDecl::from_book()
}

/// 正层行是否全为成功语义（标志位与期望码一致）。
pub fn positive_layer_consistent() -> bool {
    let mut count = 0u8;
    let mut ok = true;
    for row in BOOK.iter() {
        let positive = row.flags & FLAG_POSITIVE != 0;
        if positive != row.is_positive() {
            ok = false;
        }
        if positive { count = count.saturating_add(1); }
    }
    ok && count as u32 == POSITIVE_ROWS
}

/// 深化册逐行失配数（errno/nargs/槽位/版次四列）。干净值 0。
pub fn deep_conflicts() -> u32 {
    let mut diff = 0u32;
    for row in BOOK.iter() {
        let want_edition = match row.family {
            Family::Time => DEEP_B06_EDITION,
            Family::Resource => DEEP_B19_EDITION,
        };
        match deep_by_id(row.id) {
            Some(deep) => {
                if deep.expected != row.expected { diff = diff.saturating_add(1); }
                if deep.nargs != row.nargs { diff = diff.saturating_add(1); }
                if deep.slot != row.slot { diff = diff.saturating_add(1); }
                if deep.edition != want_edition { diff = diff.saturating_add(1); }
            }
            None => diff = diff.saturating_add(1),
        }
    }
    for deep in DEEP.iter() {
        if book_by_id(deep.id).is_none() { diff = diff.saturating_add(1); }
    }
    diff
}

/// 两册版次行数（B06, B19）。
pub fn deep_edition_counts() -> (usize, usize) {
    let mut b06 = 0usize;
    let mut b19 = 0usize;
    for row in DEEP.iter() {
        if row.edition == DEEP_B06_EDITION { b06 += 1; }
        else if row.edition == DEEP_B19_EDITION { b19 += 1; }
    }
    (b06, b19)
}

/// 字段报告行：册行标识、观测码、采集序号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub id: u16, pub obs: i32, pub seq: u16,
}

impl Entry {
    pub const fn new(id: u16, obs: i32, seq: u16) -> Entry { Entry { id, obs, seq } }
}

/// 字段报告：采集顺序即 Vec 顺序。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Report {
    pub entries: Vec<Entry>,
}

impl Report {
    pub fn new() -> Report { Report { entries: Vec::new() } }
    pub fn push(&mut self, entry: Entry) { self.entries.push(entry); }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

/// 由册直接生成一份无注入的字段报告（观测=期望）。
pub fn clean_report() -> Report {
    let mut report = Report::new();
    for (i, row) in BOOK.iter().enumerate() {
        report.push(Entry::new(row.id, row.expected, i as u16));
    }
    report
}

/// 对账结果卡：六项症状分开记账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Card {
    pub compared: u32, pub matched: u32, pub missing: u32, pub dup: u32,
    pub mismatch: u32, pub polarity: u32, pub order: u32, pub unknown: u32,
    pub conflicts: u32, pub digest: u32,
}

impl Card {
    pub fn is_clean(&self) -> bool {
        self.compared == BOOK_ROWS as u32 && self.matched == BOOK_ROWS as u32
            && self.conflicts == 0
    }
}

/// 核心对账：册行逐条找报告行，再独立扫报告顺序与册外标识。
pub fn reconcile(report: &Report) -> Card {
    let mut matched = 0u32;
    let mut missing = 0u32;
    let mut dup = 0u32;
    let mut mismatch = 0u32;
    let mut polarity = 0u32;
    for row in BOOK.iter() {
        let mut seen = 0u32;
        let mut first_obs: Option<i32> = None;
        for entry in report.entries.iter() {
            if entry.id == row.id {
                seen = seen.saturating_add(1);
                if first_obs.is_none() {
                    first_obs = Some(entry.obs);
                }
            }
        }
        if seen == 0 { missing = missing.saturating_add(1); continue; }
        if seen > 1 { dup = dup.saturating_add(seen - 1); }
        match first_obs {
            Some(obs) => {
                if obs == row.expected {
                    matched = matched.saturating_add(1);
                } else {
                    mismatch = mismatch.saturating_add(1);
                    if (obs == 0) != (row.expected == 0) {
                        polarity = polarity.saturating_add(1);
                    }
                }
            }
            None => missing = missing.saturating_add(1),
        }
    }
    let mut order = 0u32;
    let mut prev: Option<u16> = None;
    for entry in report.entries.iter() {
        if let Some(p) = prev { if entry.id <= p { order = order.saturating_add(1); } }
        prev = Some(entry.id);
    }
    let mut unknown = 0u32;
    for entry in report.entries.iter() {
        if book_by_id(entry.id).is_none() { unknown = unknown.saturating_add(1); }
    }
    let conflicts = missing.saturating_add(dup).saturating_add(mismatch)
        .saturating_add(order).saturating_add(unknown);
    Card {
        compared: BOOK_ROWS as u32, matched, missing, dup, mismatch, polarity,
        order, unknown, conflicts, digest: report_digest(report),
    }
}

/// 重复行注入：把第 3 条（下标 2）复制插到其后。
pub fn inject_dup(base: &Report) -> Report {
    let mut report = base.clone();
    if report.entries.len() >= 4 {
        if let Some(entry) = report.entries.get(2) {
            let copy = *entry;
            report.entries.insert(3, copy);
        }
    }
    report
}

/// 缺行注入：抽掉下标 7（0x0608 容量行）。
pub fn inject_missing(base: &Report) -> Report {
    let mut out = Report::new();
    let mut i = 0u16;
    for entry in base.entries.iter() {
        if i != 7 { out.push(*entry); }
        i = i.saturating_add(1);
    }
    out
}

/// 六个邻位交换位（覆盖两族，且都不与缺行/重复位重合）。
pub const SWAP_SLOTS_AT: [usize; SWAP_SLOTS] = [1, 5, 9, 12, 16, 20];

/// 邻位交换注入：交换 pos 与 pos+1 两条。
pub fn inject_swap(base: &Report, pos: usize) -> Report {
    let mut report = base.clone();
    let next = pos.saturating_add(1);
    if next < report.entries.len() { report.entries.swap(pos, next); }
    report
}

/// 判定篡改：把 0x0602（期望 EFAULT）的观测码改成 EPERM。
pub fn inject_verdict(base: &Report) -> Report {
    let mut report = base.clone();
    if let Some(entry) = report.entries.get_mut(1) { entry.obs = EPERM; }
    report
}

/// 极性翻转：把 0x0601（期望成功）的观测码改成 EFAULT。
pub fn inject_polarity(base: &Report) -> Report {
    let mut report = base.clone();
    if let Some(entry) = report.entries.get_mut(0) { entry.obs = EFAULT; }
    report
}

/// 注入账本：每次注入记一笔，检出/逃逸必须配平。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct InjectLedger {
    pub injected: u32, pub detected: u32, pub escaped: u32,
}

impl InjectLedger {
    pub fn charge(&mut self, detected: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if detected { self.detected = self.detected.saturating_add(1); }
        else { self.escaped = self.escaped.saturating_add(1); }
        self.balanced()
    }
    pub fn balanced(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
    }
}

/// 注入战役总账。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Campaign {
    pub dup_hits: u32, pub missing_hits: u32, pub swap_hits: u32,
    pub verdict_hits: u32, pub polarity_hits: u32, pub total: u32,
    pub leaks: u32, pub ledger: InjectLedger,
}

impl Campaign {
    pub fn hits(&self) -> u32 {
        self.dup_hits.saturating_add(self.missing_hits).saturating_add(self.swap_hits)
            .saturating_add(self.verdict_hits).saturating_add(self.polarity_hits)
    }
}

/// 十场景 ×10 轮注入战役。
pub fn run_campaign() -> Campaign {
    let base = clean_report();
    let mut campaign = Campaign::default();
    for _ in 0..INJECT_ROUNDS {
        let hit = reconcile(&inject_dup(&base)).dup >= 1;
        if hit { campaign.dup_hits = campaign.dup_hits.saturating_add(1); }
        if !campaign.ledger.charge(hit) { campaign.leaks = campaign.leaks.saturating_add(1); }
        campaign.total = campaign.total.saturating_add(1);
        let hit = reconcile(&inject_missing(&base)).missing >= 1;
        if hit { campaign.missing_hits = campaign.missing_hits.saturating_add(1); }
        if !campaign.ledger.charge(hit) { campaign.leaks = campaign.leaks.saturating_add(1); }
        campaign.total = campaign.total.saturating_add(1);
        for pos in SWAP_SLOTS_AT.iter() {
            let hit = reconcile(&inject_swap(&base, *pos)).order >= 1;
            if hit { campaign.swap_hits = campaign.swap_hits.saturating_add(1); }
            if !campaign.ledger.charge(hit) { campaign.leaks = campaign.leaks.saturating_add(1); }
            campaign.total = campaign.total.saturating_add(1);
        }
        let verdict = reconcile(&inject_verdict(&base));
        let hit = verdict.mismatch >= 1 && verdict.polarity == 0;
        if hit { campaign.verdict_hits = campaign.verdict_hits.saturating_add(1); }
        if !campaign.ledger.charge(hit) { campaign.leaks = campaign.leaks.saturating_add(1); }
        campaign.total = campaign.total.saturating_add(1);
        let polarity = reconcile(&inject_polarity(&base));
        let hit = polarity.mismatch >= 1 && polarity.polarity >= 1;
        if hit { campaign.polarity_hits = campaign.polarity_hits.saturating_add(1); }
        if !campaign.ledger.charge(hit) { campaign.leaks = campaign.leaks.saturating_add(1); }
        campaign.total = campaign.total.saturating_add(1);
    }
    campaign
}

/// 对账阶段：只许前进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Collect, Validate, Compare, Declare, Sealed,
}

impl Stage {
    pub fn pos(self) -> u8 {
        match self { Stage::Collect => 0, Stage::Validate => 1, Stage::Compare => 2,
                     Stage::Declare => 3, Stage::Sealed => 4 }
    }
    pub fn name(self) -> &'static str {
        match self { Stage::Collect => "collect", Stage::Validate => "validate",
                     Stage::Compare => "compare", Stage::Declare => "declare",
                     Stage::Sealed => "sealed" }
    }
}

/// 阶段机：步进只能走相邻下一级，跳转只许向前。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Survey {
    pub stage: Stage, pub rejected: u32,
}

impl Survey {
    pub fn new() -> Survey { Survey { stage: Stage::Collect, rejected: 0 } }
    pub fn step(&mut self) -> bool {
        match self.stage {
            Stage::Collect => { self.stage = Stage::Validate; true }
            Stage::Validate => { self.stage = Stage::Compare; true }
            Stage::Compare => { self.stage = Stage::Declare; true }
            Stage::Declare => { self.stage = Stage::Sealed; true }
            Stage::Sealed => false,
        }
    }
    pub fn try_jump(&mut self, target: Stage) -> bool {
        if target.pos() > self.stage.pos() { self.stage = target; true }
        else { self.rejected = self.rejected.saturating_add(1); false }
    }
    pub fn stage(&self) -> Stage { self.stage }
    pub fn rejected(&self) -> u32 { self.rejected }
}

/// 跑完整阶段链并尝试一次非法回跳，返回（前进次数, 被拒次数, 终态）。
pub fn survey_report() -> (u32, u32, Stage) {
    let mut survey = Survey::new();
    let mut transitions = 0u32;
    while survey.step() { transitions = transitions.saturating_add(1); }
    if survey.try_jump(Stage::Collect) { transitions = transitions.saturating_add(1); }
    (transitions, survey.rejected(), survey.stage())
}

/// 自写 FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8], seed: u32) -> u32 {
    let mut hash = seed;
    for byte in bytes.iter() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn mix_u16(hash: u32, value: u16) -> u32 { fnv1a(&value.to_le_bytes(), hash) }
fn mix_u32(hash: u32, value: u32) -> u32 { fnv1a(&value.to_le_bytes(), hash) }

/// 报表指纹：册四列 + 报告三列逐项喂入。
pub fn report_digest(report: &Report) -> u32 {
    let mut hash = 0x811C_9DC5u32;
    hash = mix_u32(hash, BOOK.len() as u32);
    for row in BOOK.iter() {
        hash = mix_u16(hash, row.id);
        hash = mix_u32(hash, row.expected as u32);
        hash = mix_u16(hash, row.nargs as u16);
        hash = mix_u16(hash, row.slot as u16);
        hash = mix_u16(hash, row.flags as u16);
    }
    hash = mix_u32(hash, report.entries.len() as u32);
    for entry in report.entries.iter() {
        hash = mix_u16(hash, entry.id);
        hash = mix_u32(hash, entry.obs as u32);
        hash = mix_u16(hash, entry.seq);
    }
    hash
}

/// 语料指纹（由空报表生成，双跑必须相等）。
pub fn corpus_fingerprint() -> u32 {
    report_digest(&clean_report())
}

pub fn run_unx9335_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9335");
    let (time_rows, resource_rows) = family_counts();
    let clean = reconcile(&clean_report());
    let decl = layer_decl();
    let campaign = run_campaign();
    let (transitions, rejected, stage) = survey_report();
    let digest_a = corpus_fingerprint();
    let digest_b = corpus_fingerprint();
    let mut code_unique = true;
    for (i, a) in ReconCode::ALL.iter().enumerate() {
        for (j, b) in ReconCode::ALL.iter().enumerate() {
            if i < j && a.val() == b.val() { code_unique = false; }
        }
    }
    s.add("段基址 0x7C140 对齐且落在 0x7C000..=0x7D1C0",
        SEG_BASE == 0x7C140 && SEG_BASE & 0x3F == 0 && SEG_BASE >= 0x7C000 && SEG_BASE <= 0x7D1C0,
        "F9335 独占 0x7C140..=0x7C17F 窗口");
    s.add("册形 22 行 = 时间族 12 + 资源族 10",
        BOOK.len() == BOOK_ROWS && time_rows == TIME_ROWS && resource_rows == RESOURCE_ROWS,
        "B06 十二行、B19 十行，族行数不许互换");
    s.add("22 行标识两两唯一", duplicate_ids() == 0,
        "重复标识会让同一报告行被两册行争用");
    s.add("22 行标识严格升序", ids_ascending(),
        "0x0601..=0x060C 后接 0x1301..=0x130A");
    s.add("期望码均在 0/EPERM/EAGAIN/EFAULT/EINVAL 闭集", errno_domain_ok(),
        "对账只接受这五个码，域外值视为册污染");
    s.add("nargs 合计冻结为 62", nargs_total() == NARGS_SUM,
        "时间族 32 + 资源族 30，任何行改动都会破坏合计");
    s.add("每行指针槽位严格小于 nargs", pointer_slots_ok(),
        "0x1306 的槽 0 是值参数，其余槽位均指向实参");
    s.add("正层 6 行、负层 16 行",
        positive_mask().count_ones() == POSITIVE_ROWS && negative_mask().count_ones() == NEGATIVE_ROWS,
        "正层=期望成功，负层=期望失败");
    s.add("正反位图互斥且并集为 22 位全掩码",
        (decl.positive & decl.negative) == 0 && (decl.positive | decl.negative) == FULL_MASK,
        "分层声明不许重叠，也不许漏行");
    s.add("分层声明与册逐行极性一致",
        decl.validate() && decl.conflicts_with_book() == 0,
        "声明位 = 行期望码是否为零");
    s.add("正层行标志与成功语义一致", positive_layer_consistent(),
        "FLAG_POSITIVE 只挂在 expected==0 的 6 行上");
    s.add("BOOK 与 B06/B19 深化册逐条零冲突", deep_conflicts() == 0,
        "errno/nargs/槽位/版次四列逐行相等");
    s.add("深化册版次冻结（6 + 19 = 25）",
        DEEP_B06_EDITION == 6 && DEEP_B19_EDITION == 19
            && DEEP_B06_EDITION + DEEP_B19_EDITION == DEEP_PAIR_SUM
            && deep_edition_counts() == (TIME_ROWS, RESOURCE_ROWS),
        "B06 十二行版次 6、B19 十行版次 19");
    s.add("可空参数 6 行、容量门 1 行",
        count_flags(FLAG_NULLABLE) == NULLABLE_ROWS && count_flags(FLAG_CAPACITY) == CAPACITY_ROWS,
        "可空槽必须收 NULL，容量门只在指针门之后触发 EAGAIN");
    s.add("空报表对账全绿（22 配对、0 冲突）", clean.is_clean(),
        "无注入字段报告是全部注入场景的基线");
    s.add_dyn("空报表对账账目",
        clean.compared == 22 && clean.matched == 22 && clean.conflicts == 0,
        &format!("compared={} matched={} missing={} dup={} mismatch={} order={} unknown={}",
            clean.compared, clean.matched, clean.missing, clean.dup,
            clean.mismatch, clean.order, clean.unknown));
    s.add("重复行注入 10/10 轮检出", campaign.dup_hits == INJECT_ROUNDS,
        "第 3 条复制后 dup 计数必须为 1");
    s.add("缺行注入 10/10 轮检出", campaign.missing_hits == INJECT_ROUNDS,
        "抽掉 0x0608 后 missing 计数必须为 1");
    s.add("六个邻位交换位 ×10 轮全部检出",
        campaign.swap_hits == (SWAP_SLOTS as u32) * INJECT_ROUNDS,
        "交换相邻标识必产生一处 order 违例");
    s.add("判定篡改 10/10 轮检出", campaign.verdict_hits == INJECT_ROUNDS,
        "0x0602 观测改为 EPERM：mismatch 检出且极性不变");
    s.add("极性翻转 10/10 轮检出", campaign.polarity_hits == INJECT_ROUNDS,
        "0x0601 观测改为 EFAULT：mismatch 与 polarity 同时置位");
    s.add_dyn("十场景注入账闭合",
        campaign.total == 100 && campaign.hits() == 100
            && campaign.ledger.injected == 100 && campaign.ledger.escaped == 0
            && campaign.leaks == 0 && campaign.ledger.balanced(),
        &format!("total={} hits={} injected={} detected={} escaped={} leaks={}",
            campaign.total, campaign.hits(), campaign.ledger.injected,
            campaign.ledger.detected, campaign.ledger.escaped, campaign.leaks));
    s.add("FNV-1a 指纹双跑相等", digest_a == digest_b && digest_a != 0,
        "同一册与空报表两次喂入必须得到同一 32 位值");
    s.add_dyn("阶段机只前进不回摆",
        transitions == 4 && rejected == 1 && stage == Stage::Sealed,
        &format!("transitions={} rejected={} final={}",
            transitions, rejected, stage.name()));
    s.add("诊断码 16 个码值两两唯一", code_unique && ReconCode::ALL.len() == 16,
        "码值 = SEG_BASE + 1 + 段内序号");
    s.add("深化册按标识配对而非按位置",
        deep_by_id(0x0601).is_some() && deep_by_id(0x130A).is_some() && deep_by_id(0x9999).is_none(),
        "DEEP 资源族在前，仍能与时间族标题行正确配对");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_segment() {
        assert_eq!(SEG_BASE, 0x7C140);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE >= 0x7C000 && SEG_BASE <= 0x7D1C0, true);
    }

    #[test]
    fn t02_book_shape() {
        let (time, resource) = family_counts();
        assert_eq!(time, 12);
        assert_eq!(resource, 10);
        assert_eq!(BOOK.len(), 22);
        assert_eq!(nargs_total(), 62);
        assert_eq!(count_flags(FLAG_NULLABLE), 6);
        assert_eq!(count_flags(FLAG_CAPACITY), 1);
        assert_eq!(count_flags(FLAG_POSITIVE), 6);
        assert_eq!(pointer_slots_ok(), true);
    }

    #[test]
    fn t03_ids_and_errno() {
        assert_eq!(duplicate_ids(), 0);
        assert_eq!(ids_ascending(), true);
        assert_eq!(errno_domain_ok(), true);
        assert_eq!(deep_edition_counts(), (12, 10));
    }

    #[test]
    fn t04_deep_zero_conflict() {
        assert_eq!(deep_conflicts(), 0);
        assert_eq!(DEEP.len(), 22);
        assert_eq!(DEEP_B06_EDITION + DEEP_B19_EDITION, DEEP_PAIR_SUM);
        assert_eq!(deep_by_id(0x0601).is_some(), true);
        assert_eq!(deep_by_id(0x130A).is_some(), true);
        assert_eq!(deep_by_id(0x7777).is_none(), true);
    }

    #[test]
    fn t05_layer_masks() {
        let decl = layer_decl();
        assert_eq!(decl.validate(), true);
        assert_eq!(decl.positive, POSITIVE_MASK);
        assert_eq!(decl.negative, NEGATIVE_MASK);
        assert_eq!(decl.positive & decl.negative, 0);
        assert_eq!(decl.positive | decl.negative, FULL_MASK);
        assert_eq!(decl.positive.count_ones(), 6);
        assert_eq!(decl.negative.count_ones(), 16);
        assert_eq!(decl.conflicts_with_book(), 0);
        assert_eq!(positive_layer_consistent(), true);
    }

    #[test]
    fn t06_clean_reconcile() {
        let card = reconcile(&clean_report());
        assert_eq!(card.is_clean(), true);
        assert_eq!(card.compared, 22);
        assert_eq!(card.matched, 22);
        assert_eq!(card.conflicts, 0);
        assert_eq!(card.missing, 0);
        assert_eq!(card.dup, 0);
        assert_eq!(card.mismatch, 0);
        assert_eq!(card.order, 0);
        assert_eq!(card.digest, report_digest(&clean_report()));
    }

    #[test]
    fn t07_dup_missing() {
        let base = clean_report();
        let dup = reconcile(&inject_dup(&base));
        let missing = reconcile(&inject_missing(&base));
        assert_eq!(dup.dup, 1);
        assert_eq!(dup.matched, 22);
        assert_eq!(dup.conflicts >= 1, true);
        assert_eq!(missing.missing, 1);
        assert_eq!(missing.matched, 21);
        assert_eq!(missing.conflicts, 1);
    }

    #[test]
    fn t08_swap_slots() {
        let base = clean_report();
        for pos in SWAP_SLOTS_AT.iter() {
            let card = reconcile(&inject_swap(&base, *pos));
            assert_eq!(card.order >= 1, true);
            assert_eq!(card.matched, 22);
        }
    }

    #[test]
    fn t09_verdict_polarity() {
        let base = clean_report();
        let verdict = reconcile(&inject_verdict(&base));
        let polarity = reconcile(&inject_polarity(&base));
        assert_eq!(verdict.mismatch, 1);
        assert_eq!(verdict.polarity, 0);
        assert_eq!(verdict.conflicts, 1);
        assert_eq!(polarity.mismatch, 1);
        assert_eq!(polarity.polarity, 1);
        assert_eq!(polarity.conflicts, 1);
    }

    #[test]
    fn t10_campaign_ledger() {
        let campaign = run_campaign();
        assert_eq!(campaign.total, 100);
        assert_eq!(campaign.hits(), 100);
        assert_eq!(campaign.dup_hits, 10);
        assert_eq!(campaign.missing_hits, 10);
        assert_eq!(campaign.swap_hits, 60);
        assert_eq!(campaign.verdict_hits, 10);
        assert_eq!(campaign.polarity_hits, 10);
        assert_eq!(campaign.ledger.injected, 100);
        assert_eq!(campaign.ledger.detected, 100);
        assert_eq!(campaign.ledger.escaped, 0);
        assert_eq!(campaign.leaks, 0);
        assert_eq!(campaign.ledger.balanced(), true);
    }

    #[test]
    fn t11_fingerprint() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a != 0, true);
        assert_eq!(fnv1a(&[], 0x811C_9DC5), 0x811C_9DC5);
        assert_eq!(fnv1a(&[0u8], 0x811C_9DC5), 0x811C_9DC5_u32.wrapping_mul(0x0100_0193));
    }

    #[test]
    fn t12_stage_machine() {
        let (transitions, rejected, stage) = survey_report();
        assert_eq!(transitions, 4);
        assert_eq!(rejected, 1);
        assert_eq!(stage, Stage::Sealed);
        let mut survey = Survey::new();
        assert_eq!(survey.stage(), Stage::Collect);
        assert_eq!(survey.try_jump(Stage::Compare), true);
        assert_eq!(survey.stage(), Stage::Compare);
        assert_eq!(survey.try_jump(Stage::Validate), false);
        assert_eq!(survey.rejected(), 1);
        assert_eq!(survey.step(), true);
        assert_eq!(survey.stage(), Stage::Declare);
    }

    #[test]
    fn t13_checks_all_green() {
        let s = run_unx9335_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
