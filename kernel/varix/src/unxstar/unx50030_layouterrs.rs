//! UNX-F50030 布局错误码族（15 条：页数超限/纸张过小等）（0x42fe0 段）
//!
//! 锚点：UNX-F50030-J1 错误码 ×15 零漂移；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x42fe0`，诊断码闭集 `0x42fe0 + 1..=20`：
//! - 1..=15：布局故障族（用户可见的 15 条错误）；
//! - 16..=20：码表审计故障（表损坏时的自诊断，不与用户错误混码）。
//!
//! 本模块结构：
//! - 错误码族：`LayoutFault` 十五型，`code()` 恒为 `SEG_BASE + n`；`TABLE`
//!   是只读规范表（码值/名称/严重度三元组），`audit_table()` 对任意表做
//!   逐项审计（重复码、越界码、空名、严重度漂移、条数不足）；
//! - 分类器：`LayoutProbe`（页数/纸张/N-up/双面/边距/出血/缩放/折标/张数/
//!   方向/书脊）与 `collect_faults()`，各规则相互独立，一次可收集多条故障；
//! - 账本：`FaultLedger` 十五槽计数（饱和加法），`FaultReport` 携带码值 +
//!   严重度 + 补救建议，供 UI 三要素（是什么/为什么/怎么办）直读；
//! - 注入核：`injection_core()` 5 类坏表（重复码、越界码、空名、严重度
//!   漂移、条数不足）逐类断言被审计拒绝；
//! - 判据：15 条码值零漂移、名称/补救非空、分类器可达性、良性探针零故障
//!   （非恒绿对照）、账本守恒、表指纹可复算。
//!
//! 零 panic 纪律：无索引裸访问，全部走 `get()`/区间校验；无 std。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x42fe0 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x42fe0;

/// 页数上限（与拼版卡同口径：4000 页是办公作业上限）。
pub const MAX_PAGES: u32 = 4000;
/// 纸张短边下限：1000 点 @600dpi ≈ 42mm，再小无法容纳任何 2-up 格。
pub const MIN_SIDE: u32 = 1000;
/// 缩放上下限（千分比）：10% ~ 500%，打印机驱动的通用安全区。
pub const SCALE_MIN: u32 = 100;
pub const SCALE_MAX: u32 = 5000;
/// 书脊下限：40 mil ≈ 1mm，薄于此的折标在装订侧不可靠。
pub const SPINE_MIN_MILS: u32 = 40;

const N_FAULT: u16 = 15;
const E_TABLE_COUNT: u16 = 16;
const E_TABLE_DUP: u16 = 17;
const E_TABLE_RANGE: u16 = 18;
const E_TABLE_NAME: u16 = 19;
const E_TABLE_SEVERITY: u16 = 20;

/// 布局故障十五型。变体顺序即码序——插入新变体必须同步扩段并重算指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutFault {
    PageOverflow,
    PaperTooSmall,
    NUpUnsupported,
    GridOverflow,
    DuplexUnsupported,
    MarginNegative,
    MarginTooLarge,
    CellCollision,
    BleedExceedsMargin,
    OrientationMismatch,
    PageZero,
    ScalingOutOfRange,
    SpineTooThin,
    SignatureNotMultiple,
    SheetCountMismatch,
}

impl LayoutFault {
    pub const ALL: [LayoutFault; 15] = [
        LayoutFault::PageOverflow,
        LayoutFault::PaperTooSmall,
        LayoutFault::NUpUnsupported,
        LayoutFault::GridOverflow,
        LayoutFault::DuplexUnsupported,
        LayoutFault::MarginNegative,
        LayoutFault::MarginTooLarge,
        LayoutFault::CellCollision,
        LayoutFault::BleedExceedsMargin,
        LayoutFault::OrientationMismatch,
        LayoutFault::PageZero,
        LayoutFault::ScalingOutOfRange,
        LayoutFault::SpineTooThin,
        LayoutFault::SignatureNotMultiple,
        LayoutFault::SheetCountMismatch,
    ];

    /// 段内序号（1 起），`const` 以便规范表在编译期定值。
    pub const fn ordinal(self) -> u16 {
        match self {
            LayoutFault::PageOverflow => 1,
            LayoutFault::PaperTooSmall => 2,
            LayoutFault::NUpUnsupported => 3,
            LayoutFault::GridOverflow => 4,
            LayoutFault::DuplexUnsupported => 5,
            LayoutFault::MarginNegative => 6,
            LayoutFault::MarginTooLarge => 7,
            LayoutFault::CellCollision => 8,
            LayoutFault::BleedExceedsMargin => 9,
            LayoutFault::OrientationMismatch => 10,
            LayoutFault::PageZero => 11,
            LayoutFault::ScalingOutOfRange => 12,
            LayoutFault::SpineTooThin => 13,
            LayoutFault::SignatureNotMultiple => 14,
            LayoutFault::SheetCountMismatch => 15,
        }
    }

    /// 诊断码：段基址 + 序号，闭集内唯一且位置固定（零漂移的锚点）。
    pub const fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub const fn name(self) -> &'static str {
        match self {
            LayoutFault::PageOverflow => "page-overflow",
            LayoutFault::PaperTooSmall => "paper-too-small",
            LayoutFault::NUpUnsupported => "nup-unsupported",
            LayoutFault::GridOverflow => "grid-overflow",
            LayoutFault::DuplexUnsupported => "duplex-unsupported",
            LayoutFault::MarginNegative => "margin-negative",
            LayoutFault::MarginTooLarge => "margin-too-large",
            LayoutFault::CellCollision => "cell-collision",
            LayoutFault::BleedExceedsMargin => "bleed-exceeds-margin",
            LayoutFault::OrientationMismatch => "orientation-mismatch",
            LayoutFault::PageZero => "page-zero",
            LayoutFault::ScalingOutOfRange => "scaling-out-of-range",
            LayoutFault::SpineTooThin => "spine-too-thin",
            LayoutFault::SignatureNotMultiple => "signature-not-multiple",
            LayoutFault::SheetCountMismatch => "sheet-count-mismatch",
        }
    }

    /// 严重度归口：用户可纠正 / 系统配置 / 介质装订。表审计按此比对，
    /// 任何严重度调换都会被 `BadSeverity` 抓出（错误三要素不许改写）。
    pub const fn severity(self) -> Severity {
        match self {
            LayoutFault::PageOverflow
            | LayoutFault::PageZero
            | LayoutFault::NUpUnsupported
            | LayoutFault::DuplexUnsupported
            | LayoutFault::MarginNegative
            | LayoutFault::MarginTooLarge
            | LayoutFault::ScalingOutOfRange => Severity::User,
            LayoutFault::GridOverflow
            | LayoutFault::CellCollision
            | LayoutFault::OrientationMismatch
            | LayoutFault::SheetCountMismatch
            | LayoutFault::SignatureNotMultiple => Severity::System,
            LayoutFault::PaperTooSmall
            | LayoutFault::BleedExceedsMargin
            | LayoutFault::SpineTooThin => Severity::Media,
        }
    }

    /// 一行「是什么」。
    pub fn message(self) -> &'static str {
        match self {
            LayoutFault::PageOverflow => "page count exceeds job limit",
            LayoutFault::PaperTooSmall => "paper smaller than minimum sheet",
            LayoutFault::NUpUnsupported => "n-up not in driver set",
            LayoutFault::GridOverflow => "grid produced zero sheets",
            LayoutFault::DuplexUnsupported => "duplex mode code invalid",
            LayoutFault::MarginNegative => "margin below zero",
            LayoutFault::MarginTooLarge => "margin consumes content box",
            LayoutFault::CellCollision => "negative gutter makes cells collide",
            LayoutFault::BleedExceedsMargin => "bleed larger than margin",
            LayoutFault::OrientationMismatch => "orientation contradicts paper aspect",
            LayoutFault::PageZero => "job has no pages",
            LayoutFault::ScalingOutOfRange => "scale outside 10%..500%",
            LayoutFault::SpineTooThin => "spine below 1mm binding floor",
            LayoutFault::SignatureNotMultiple => "pages not a signature multiple",
            LayoutFault::SheetCountMismatch => "sheet count disagrees with grid",
        }
    }

    /// 一行「怎么办」。
    pub fn remedy(self) -> &'static str {
        match self {
            LayoutFault::PageOverflow => "split job under 4000 pages",
            LayoutFault::PaperTooSmall => "pick A5 or larger media",
            LayoutFault::NUpUnsupported => "choose 1/2/4/6/8/9/16-up",
            LayoutFault::GridOverflow => "recompute grid with sides=1..2",
            LayoutFault::DuplexUnsupported => "use simplex/long/short edge",
            LayoutFault::MarginNegative => "set margin >= 0",
            LayoutFault::MarginTooLarge => "reduce margin below half width",
            LayoutFault::CellCollision => "set gutter >= 0",
            LayoutFault::BleedExceedsMargin => "cap bleed at margin",
            LayoutFault::OrientationMismatch => "rotate media or flip orientation",
            LayoutFault::PageZero => "add at least one page",
            LayoutFault::ScalingOutOfRange => "scale within 10%..500%",
            LayoutFault::SpineTooThin => "add sheets or disable saddle stitch",
            LayoutFault::SignatureNotMultiple => "pad pages to signature size",
            LayoutFault::SheetCountMismatch => "re-derive sheets from page count",
        }
    }

    /// 从码值反查；段内非法码返回 `None`（不猜测、不就近映射）。
    pub fn from_code(code: u32) -> Option<LayoutFault> {
        if code < SEG_BASE + 1 || code > SEG_BASE + N_FAULT as u32 {
            return None;
        }
        let idx = (code - SEG_BASE) as usize;
        LayoutFault::ALL.get(idx - 1).copied()
    }
}

/// 严重度三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    User,
    System,
    Media,
}

impl Severity {
    pub fn code(self) -> u8 {
        match self {
            Severity::User => 0,
            Severity::System => 1,
            Severity::Media => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::User => "user",
            Severity::System => "system",
            Severity::Media => "media",
        }
    }
}

/// 规范表一行：码值 + 故障 + 名称 + 严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultEntry {
    pub code: u32,
    pub fault: LayoutFault,
    pub name: &'static str,
    pub severity: Severity,
}

const fn entry(fault: LayoutFault) -> FaultEntry {
    FaultEntry {
        code: fault.code(),
        fault,
        name: fault.name(),
        severity: fault.severity(),
    }
}

/// 只读规范表（15 行）。判据把它当作「零漂移」的基准快照。
pub const TABLE: [FaultEntry; 15] = [
    entry(LayoutFault::PageOverflow),
    entry(LayoutFault::PaperTooSmall),
    entry(LayoutFault::NUpUnsupported),
    entry(LayoutFault::GridOverflow),
    entry(LayoutFault::DuplexUnsupported),
    entry(LayoutFault::MarginNegative),
    entry(LayoutFault::MarginTooLarge),
    entry(LayoutFault::CellCollision),
    entry(LayoutFault::BleedExceedsMargin),
    entry(LayoutFault::OrientationMismatch),
    entry(LayoutFault::PageZero),
    entry(LayoutFault::ScalingOutOfRange),
    entry(LayoutFault::SpineTooThin),
    entry(LayoutFault::SignatureNotMultiple),
    entry(LayoutFault::SheetCountMismatch),
];

/// 表审计故障（16..=20）。与用户错误族分码：表坏了是系统自诊断，不是
/// 用户作业错误，混码会让用户看到「无法修复」的内部码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableFault {
    Count,
    DuplicateCode,
    OutOfRange,
    BadName,
    BadSeverity,
}

impl TableFault {
    pub fn code(self) -> u32 {
        let n = match self {
            TableFault::Count => E_TABLE_COUNT,
            TableFault::DuplicateCode => E_TABLE_DUP,
            TableFault::OutOfRange => E_TABLE_RANGE,
            TableFault::BadName => E_TABLE_NAME,
            TableFault::BadSeverity => E_TABLE_SEVERITY,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            TableFault::Count => "fault table must hold 15 rows",
            TableFault::DuplicateCode => "two rows share one code",
            TableFault::OutOfRange => "row code outside 1..=15",
            TableFault::BadName => "row name empty or drifted",
            TableFault::BadSeverity => "row severity drifted",
        }
    }
}

/// 审计任意码表：重复码 -> 越界/错位 -> 名称 -> 严重度 -> 条数。
/// 顺序固定，注入核据此断言唯一错误码。
pub fn audit_table(rows: &[FaultEntry]) -> Result<(), TableFault> {
    if rows.len() != N_FAULT as usize {
        return Err(TableFault::Count);
    }
    // 先查重复：重复码与错位码必须分码，否则「抄错一行」会被误报成越界。
    let mut i = 0usize;
    while i < rows.len() {
        let a = match rows.get(i) {
            Some(v) => v,
            None => return Err(TableFault::Count),
        };
        let mut j = i + 1;
        while j < rows.len() {
            if let Some(b) = rows.get(j) {
                if a.code == b.code {
                    return Err(TableFault::DuplicateCode);
                }
            }
            j += 1;
        }
        i += 1;
    }
    i = 0;
    while i < rows.len() {
        let e = match rows.get(i) {
            Some(v) => v,
            None => return Err(TableFault::Count),
        };
        if e.code < SEG_BASE + 1 || e.code > SEG_BASE + N_FAULT as u32 {
            return Err(TableFault::OutOfRange);
        }
        if e.code != SEG_BASE + (i as u32) + 1 {
            return Err(TableFault::OutOfRange);
        }
        if e.fault.ordinal() as usize != i + 1 {
            return Err(TableFault::OutOfRange);
        }
        if e.name.is_empty() || e.name != e.fault.name() {
            return Err(TableFault::BadName);
        }
        if e.severity != e.fault.severity() {
            return Err(TableFault::BadSeverity);
        }
        i += 1;
    }
    Ok(())
}

/// 表指纹（FNV-1a over code/name/severity）。判据比对硬编码常量：
/// 任何一行改动都会让指纹变红——这是「×15 零漂移」的机器锚点。
/// 常量由规范表推出：15 行、码 0x42fe0+1..15、名称/严重度见 `TABLE`。
pub const TABLE_HASH: u64 = 0x4ad9_9f66_e895_2990;

fn hash_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 复算表指纹。与 `TABLE_HASH` 不一致即视为码表漂移。
pub fn table_hash() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(e) = TABLE.get(i) {
            h = hash_mix(h, e.code as u64);
            let bytes = e.name.as_bytes();
            let mut k = 0usize;
            while k < bytes.len() {
                if let Some(b) = bytes.get(k) {
                    h = hash_mix(h, *b as u64);
                }
                k += 1;
            }
            h = hash_mix(h, e.severity.code() as u64);
        }
        i += 1;
    }
    h
}

/// 一次布局探测的全部输入。字段刻意扁平，分类规则可独立测试。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutProbe {
    pub pages: u32,
    pub paper_w: u32,
    pub paper_h: u32,
    pub n_up: u8,
    pub duplex: u8,
    pub margin: i32,
    pub gutter: i32,
    pub bleed: u32,
    pub scale_permille: u32,
    pub signature_pages: u32,
    pub sheets: u32,
    pub orientation: u8,
    pub spine_mils: u32,
}

impl LayoutProbe {
    /// 良性基线：A4 600dpi、4-up 双面、8 页、4 页折标、1 张、竖版。
    pub const fn good() -> LayoutProbe {
        LayoutProbe {
            pages: 8,
            paper_w: 4960,
            paper_h: 7040,
            n_up: 4,
            duplex: 2,
            margin: 240,
            gutter: 64,
            bleed: 0,
            scale_permille: 1000,
            signature_pages: 4,
            sheets: 1,
            orientation: 0,
            spine_mils: 50,
        }
    }
}

/// 分类器：每条规则相互独立，命中即入列（顺序固定 = 变体序）。
pub fn collect_faults(p: &LayoutProbe) -> Vec<LayoutFault> {
    let mut out = Vec::new();
    if p.pages == 0 {
        out.push(LayoutFault::PageZero);
    }
    if p.pages > MAX_PAGES {
        out.push(LayoutFault::PageOverflow);
    }
    if !matches!(p.n_up, 1 | 2 | 4 | 6 | 8 | 9 | 16) {
        out.push(LayoutFault::NUpUnsupported);
    }
    if p.duplex > 2 {
        out.push(LayoutFault::DuplexUnsupported);
    }
    if p.margin < 0 {
        out.push(LayoutFault::MarginNegative);
    }
    if p.gutter < 0 {
        out.push(LayoutFault::CellCollision);
    }
    if p.paper_w < MIN_SIDE || p.paper_h < MIN_SIDE {
        out.push(LayoutFault::PaperTooSmall);
    }
    if p.margin > 0 && (p.margin as u32).saturating_mul(2) >= p.paper_w {
        out.push(LayoutFault::MarginTooLarge);
    }
    if p.bleed > 0 && p.margin >= 0 && p.bleed > p.margin as u32 {
        out.push(LayoutFault::BleedExceedsMargin);
    }
    if p.scale_permille < SCALE_MIN || p.scale_permille > SCALE_MAX {
        out.push(LayoutFault::ScalingOutOfRange);
    }
    let portrait = p.paper_h >= p.paper_w;
    if (p.orientation == 1 && portrait) || (p.orientation == 0 && !portrait) {
        out.push(LayoutFault::OrientationMismatch);
    }
    if p.signature_pages > 0 && p.pages > 0 && p.pages % p.signature_pages != 0 {
        out.push(LayoutFault::SignatureNotMultiple);
    }
    if p.pages > 0 && p.sheets == 0 {
        out.push(LayoutFault::GridOverflow);
    }
    if p.pages > 0 && p.sheets > 0 {
        let per = (p.n_up as u32).saturating_mul(if p.duplex == 0 { 1 } else { 2 });
        if per == 0 {
            out.push(LayoutFault::GridOverflow);
        } else if (p.pages + per - 1) / per != p.sheets {
            out.push(LayoutFault::SheetCountMismatch);
        }
    }
    if p.spine_mils > 0 && p.spine_mils < SPINE_MIN_MILS && p.sheets > 1 {
        out.push(LayoutFault::SpineTooThin);
    }
    out
}

/// 首个故障（UI 先展示一条，其余进详情）。无故障返回 `None`。
pub fn first_fault(p: &LayoutProbe) -> Option<LayoutFault> {
    collect_faults(p).into_iter().next()
}

/// 故障回执（错误三要素的载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultReport {
    pub code: u32,
    pub fault: LayoutFault,
    pub severity: Severity,
    pub message: &'static str,
    pub remedy: &'static str,
}

/// 由故障构造回执；码值/严重度/文案全部取自族定义，不允许调用方覆写。
pub fn report(fault: LayoutFault) -> FaultReport {
    FaultReport {
        code: fault.code(),
        fault,
        severity: fault.severity(),
        message: fault.message(),
        remedy: fault.remedy(),
    }
}

/// 故障账本：十五槽计数（饱和），总量单独累计，避免槽溢出丢总量。
#[derive(Clone, Debug)]
pub struct FaultLedger {
    pub counts: [u16; 15],
    pub total: u64,
}

impl FaultLedger {
    pub fn new() -> FaultLedger {
        FaultLedger {
            counts: [0; 15],
            total: 0,
        }
    }

    /// 记一次故障；槽位下标由码值导出，越界（码漂移）直接不计入。
    pub fn latch(&mut self, fault: LayoutFault) -> bool {
        let idx = fault.ordinal() as usize;
        if idx == 0 || idx > 15 {
            return false;
        }
        if let Some(c) = self.counts.get_mut(idx - 1) {
            *c = c.saturating_add(1);
            self.total = self.total.saturating_add(1);
            return true;
        }
        false
    }

    pub fn count_of(&self, fault: LayoutFault) -> u16 {
        let idx = fault.ordinal() as usize;
        if idx == 0 {
            return 0;
        }
        match self.counts.get(idx - 1) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 按严重度汇总 (user, system, media)。
    pub fn histogram(&self) -> [u32; 3] {
        let mut out = [0u32; 3];
        let mut i = 0usize;
        while i < 15 {
            if let Some(f) = LayoutFault::ALL.get(i) {
                if let Some(c) = self.counts.get(i) {
                    let s = f.severity().code() as usize;
                    if s < 3 {
                        out[s] = out[s].saturating_add(*c as u32);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// 账本审计：总量必须等于十五槽之和（守恒；槽饱和时允许总量大于和，
    /// 但绝不允许小于——那意味着有故障被静默吞掉）。
    pub fn audit(&self) -> bool {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < 15 {
            if let Some(c) = self.counts.get(i) {
                sum += *c as u64;
            }
            i += 1;
        }
        self.total >= sum && sum > 0 || self.total == 0
    }
}

// ---------------------------------------------------------------------------
// 注入核：五类坏表
// ---------------------------------------------------------------------------

fn corrupt_copy() -> [FaultEntry; 15] {
    TABLE
}

pub fn injection_core() -> (usize, usize) {
    let total = 5usize;
    let mut caught = 0usize;

    // 1. 重复码：第 4 行抄成第 1 行的码。
    let mut dup = corrupt_copy();
    let first = dup.get(0).copied();
    if let (Some(a), Some(b)) = (first, dup.get_mut(3)) {
        b.code = a.code;
    }
    if audit_table(&dup).err() == Some(TableFault::DuplicateCode) {
        caught += 1;
    }
    // 2. 越界码：第 8 行码值抬到段外。
    let mut far = corrupt_copy();
    if let Some(e) = far.get_mut(7) {
        e.code = SEG_BASE + 99;
    }
    if audit_table(&far).err() == Some(TableFault::OutOfRange) {
        caught += 1;
    }
    // 3. 空名。
    let mut name = corrupt_copy();
    if let Some(e) = name.get_mut(2) {
        e.name = "";
    }
    if audit_table(&name).err() == Some(TableFault::BadName) {
        caught += 1;
    }
    // 4. 严重度漂移：把 System 档改成 Media。
    let mut sev = corrupt_copy();
    if let Some(e) = sev.get_mut(3) {
        e.severity = Severity::Media;
    }
    if audit_table(&sev).err() == Some(TableFault::BadSeverity) {
        caught += 1;
    }
    // 5. 条数不足：截断到 14 行。
    if audit_table(&TABLE[..14]).err() == Some(TableFault::Count) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 逐条构造单项坏探针，返回 (命中故障, 是否命中)。
fn probe_hits() -> (usize, usize) {
    let base = LayoutProbe::good();
    let mut total = 0usize;
    let mut hit = 0usize;
    let mut probe = |p: LayoutProbe, want: LayoutFault| {
        total += 1;
        let got = collect_faults(&p);
        if got.iter().any(|f| *f == want) {
            hit += 1;
        }
    };

    let mut p = base;
    p.pages = 0;
    probe(p, LayoutFault::PageZero);
    p = base;
    p.pages = MAX_PAGES + 1;
    p.signature_pages = 0;
    probe(p, LayoutFault::PageOverflow);
    p = base;
    p.n_up = 7;
    p.sheets = 0;
    probe(p, LayoutFault::NUpUnsupported);
    p = base;
    p.duplex = 9;
    p.sheets = 0;
    probe(p, LayoutFault::DuplexUnsupported);
    p = base;
    p.margin = -1;
    probe(p, LayoutFault::MarginNegative);
    p = base;
    p.gutter = -1;
    probe(p, LayoutFault::CellCollision);
    p = base;
    p.paper_w = 800;
    p.margin = 100;
    p.sheets = 0;
    probe(p, LayoutFault::PaperTooSmall);
    p = base;
    p.margin = 3000;
    probe(p, LayoutFault::MarginTooLarge);
    p = base;
    p.margin = 200;
    p.bleed = 300;
    probe(p, LayoutFault::BleedExceedsMargin);
    p = base;
    p.scale_permille = 50;
    probe(p, LayoutFault::ScalingOutOfRange);
    p = base;
    p.orientation = 1;
    probe(p, LayoutFault::OrientationMismatch);
    p = base;
    p.pages = 9;
    p.sheets = 0;
    probe(p, LayoutFault::SignatureNotMultiple);
    p = base;
    p.sheets = 0;
    probe(p, LayoutFault::GridOverflow);
    let mut p = base;
    p.pages = 12;
    p.signature_pages = 4;
    p.sheets = 1;
    probe(p, LayoutFault::SheetCountMismatch);
    p = base;
    p.pages = 32;
    p.sheets = 4;
    p.spine_mils = 20;
    probe(p, LayoutFault::SpineTooThin);
    (hit, total)
}

pub fn run_unx50030_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50030");

    // J1-1 ×15 零漂移：变体序 = 码序，码值连续且唯一。
    let mut drift = 0usize;
    let mut i = 0usize;
    while i < LayoutFault::ALL.len() {
        if let Some(f) = LayoutFault::ALL.get(i) {
            if f.code() != SEG_BASE + (i as u32) + 1 {
                drift += 1;
            }
            if f.ordinal() as usize != i + 1 {
                drift += 1;
            }
        }
        i += 1;
    }
    let ok = LayoutFault::ALL.len() == 15 && drift == 0;
    let det = format!("漂移={}", drift);
    s.add_dyn("50030-J1-码值零漂移", ok, &det);

    // J1-2 规范表与枚举互核（顺序、码值、名称、严重度四项）。
    let ok = audit_table(&TABLE).is_ok();
    s.add("50030-J1-规范表自洽", ok, if ok { "" } else { "规范表审计未过" });

    // J1-3 表指纹可复算且非零：任何一行改动都会改变指纹。
    let h1 = table_hash();
    let h2 = table_hash();
    let ok = h1 == h2 && h1 != 0 && h1 == TABLE_HASH;
    let det = format!("hash={:016x}", h1);
    s.add_dyn("50030-J1-表指纹", ok, &det);

    // J1-4 名称/补救/消息非空且名称唯一（UI 三要素不许空转）。
    let mut meta_ok = true;
    let mut names: Vec<&'static str> = Vec::with_capacity(15);
    let mut i = 0usize;
    while i < LayoutFault::ALL.len() {
        if let Some(f) = LayoutFault::ALL.get(i) {
            if f.name().is_empty() || f.message().is_empty() || f.remedy().is_empty() {
                meta_ok = false;
            }
            if names.iter().any(|n| *n == f.name()) {
                meta_ok = false;
            }
            names.push(f.name());
        }
        i += 1;
    }
    let ok = meta_ok && names.len() == 15;
    s.add("50030-J1-三要素完整", ok, if ok { "" } else { "名称/文案缺失或重名" });

    // J1-5 码值反查往返：15 条全往返，段外码返回 None。
    let mut rt_bad = 0usize;
    let mut i = 0usize;
    while i < LayoutFault::ALL.len() {
        if let Some(f) = LayoutFault::ALL.get(i) {
            match LayoutFault::from_code(f.code()) {
                Some(g) => {
                    if g != *f {
                        rt_bad += 1;
                    }
                }
                None => rt_bad += 1,
            }
        }
        i += 1;
    }
    let ok = rt_bad == 0
        && LayoutFault::from_code(SEG_BASE).is_none()
        && LayoutFault::from_code(SEG_BASE + 16).is_none()
        && LayoutFault::from_code(0).is_none();
    let det = format!("往返错={}", rt_bad);
    s.add_dyn("50030-J1-码值反查", ok, &det);

    // J1-6 严重度分布：三档都非空，且直方图与逐条计数一致。
    let mut sev_counts = [0u32; 3];
    let mut i = 0usize;
    while i < LayoutFault::ALL.len() {
        if let Some(f) = LayoutFault::ALL.get(i) {
            let c = f.severity().code() as usize;
            if c < 3 {
                sev_counts[c] += 1;
            }
        }
        i += 1;
    }
    let ok = sev_counts[0] > 0 && sev_counts[1] > 0 && sev_counts[2] > 0;
    let det = format!("user={} sys={} media={}", sev_counts[0], sev_counts[1], sev_counts[2]);
    s.add_dyn("50030-J1-严重度覆盖", ok, &det);

    // J1-7 分类器 15 类单项可达。
    let (hit, total) = probe_hits();
    let ok = hit == total && total == 15;
    let det = format!("命中 {}/{}", hit, total);
    s.add_dyn("50030-J1-分类可达", ok, &det);

    // J1-8 良性探针零故障（非恒绿对照：分类器不是见谁报谁）。
    let good = LayoutProbe::good();
    let faults = collect_faults(&good);
    let ok = faults.is_empty() && first_fault(&good).is_none();
    let det = format!("误报={}", faults.len());
    s.add_dyn("50030-J1-良性零故障", ok, &det);

    // J1-9 回执三要素与族定义逐项一致。
    let mut rep_bad = 0usize;
    let mut i = 0usize;
    while i < LayoutFault::ALL.len() {
        if let Some(f) = LayoutFault::ALL.get(i) {
            let r = report(*f);
            if r.code != f.code()
                || r.fault != *f
                || r.severity != f.severity()
                || r.message != f.message()
                || r.remedy != f.remedy()
            {
                rep_bad += 1;
            }
        }
        i += 1;
    }
    let ok = rep_bad == 0;
    s.add("50030-J1-回执一致", ok, if ok { "" } else { "回执与族定义漂移" });

    // J1-10 账本计数守恒：逐条 latch 与总量/直方图三方相等。
    let mut led = FaultLedger::new();
    let mut i = 0usize;
    while i < 15 {
        if let Some(f) = LayoutFault::ALL.get(i) {
            let n = (i as u16 % 3) + 1;
            let mut k = 0u16;
            while k < n {
                if !led.latch(*f) {
                    break;
                }
                k += 1;
            }
        }
        i += 1;
    }
    let hist = led.histogram();
    let ok = led.total == 30
        && led.count_of(LayoutFault::PageOverflow) == 1
        && led.count_of(LayoutFault::SheetCountMismatch) == 3
        && hist[0] + hist[1] + hist[2] == 30
        && led.audit();
    let det = format!("total={} hist={:?}", led.total, hist);
    s.add_dyn("50030-J1-账本守恒", ok, &det);

    // J1-11 段闭集：用户族 15 + 审计族 5，全部落在 SEG_BASE+1..=20。
    let mut closed = true;
    let mut i = 0usize;
    while i < 15 {
        if let Some(f) = LayoutFault::ALL.get(i) {
            let c = f.code();
            if c < SEG_BASE + 1 || c > SEG_BASE + 20 {
                closed = false;
            }
        }
        i += 1;
    }
    for t in [
        TableFault::Count,
        TableFault::DuplicateCode,
        TableFault::OutOfRange,
        TableFault::BadName,
        TableFault::BadSeverity,
    ]
    .iter()
    {
        let c = t.code();
        if c < SEG_BASE + 16 || c > SEG_BASE + 20 {
            closed = false;
        }
    }
    let ok = closed && TableFault::Count.code() == SEG_BASE + 16;
    s.add("50030-J1-段闭集", ok, if ok { "" } else { "码值越段" });

    // J1-12 审计分码：重复/越界/空名/严重度/条数五类各返回专属码。
    let mut dup = corrupt_copy();
    let head = dup.get(0).copied();
    if let (Some(a), Some(b)) = (head, dup.get_mut(3)) {
        b.code = a.code;
    }
    let mut far = corrupt_copy();
    if let Some(e) = far.get_mut(7) {
        e.code = SEG_BASE + 99;
    }
    let mut nm = corrupt_copy();
    if let Some(e) = nm.get_mut(2) {
        e.name = "";
    }
    let mut sv = corrupt_copy();
    if let Some(e) = sv.get_mut(3) {
        e.severity = Severity::Media;
    }
    let ok = audit_table(&dup).err() == Some(TableFault::DuplicateCode)
        && audit_table(&far).err() == Some(TableFault::OutOfRange)
        && audit_table(&nm).err() == Some(TableFault::BadName)
        && audit_table(&sv).err() == Some(TableFault::BadSeverity)
        && audit_table(&TABLE[..14]).err() == Some(TableFault::Count);
    s.add("50030-J1-审计五类分码", ok, if ok { "" } else { "审计错误码混淆" });

    // J1-13 分类器多条并发：坏探针可一次报出多条（不遮蔽）。
    let mut p = LayoutProbe::good();
    p.pages = 0;
    p.margin = -5;
    p.duplex = 7;
    let many = collect_faults(&p);
    let ok = many.len() >= 3
        && many.iter().any(|f| *f == LayoutFault::PageZero)
        && many.iter().any(|f| *f == LayoutFault::MarginNegative)
        && many.iter().any(|f| *f == LayoutFault::DuplexUnsupported);
    s.add("50030-J1-多故障并发", ok, if ok { "" } else { "多故障被遮蔽" });

    // J1-14 注入核 5 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50030-J1-注入必抓", ok, &det);

    // J1-15 审计错误族自身码值连续（16..=20）且文案非空。
    let audits = [
        TableFault::Count,
        TableFault::DuplicateCode,
        TableFault::OutOfRange,
        TableFault::BadName,
        TableFault::BadSeverity,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < audits.len() {
        if let Some(t) = audits.get(i) {
            if t.code() != SEG_BASE + 16 + i as u32 || t.message().is_empty() {
                ok = false;
            }
        }
        i += 1;
    }
    s.add("50030-J1-审计族连续", ok, if ok { "" } else { "审计族码值断裂" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50030_checks_green() {
        let set = run_unx50030_checks();
        assert!(set.all_passed(), "UNX-F50030 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50030_table_hash_is_stable() {
        assert_eq!(table_hash(), TABLE_HASH);
        assert_eq!(audit_table(&TABLE), Ok(()));
    }
}
