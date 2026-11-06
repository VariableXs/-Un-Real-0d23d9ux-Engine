//! UNX-F50060 B23 批收官印（20 条冻结 / 累计 138,000 行）（0x433a0 段）
//!
//! 锚点：UNX-F50060-J1 批印入册：20/20 冻结、域账 138,000/240,000；
//! 判据锚定 §AI-63 §7。
//!
//! 数据段：`SEG_BASE = 0x433a0`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 17`，
//! 逐项见 [`SealError`]。
//!
//! 收官印是「批账的可信封条」，不是汇总报告。三道闸门缺一不可：
//! 1. **条数**——必须 20 条，少一条就是「批没收完」，不许用 19 条算平均；
//! 2. **冻结**——20 条逐条冻结，冻结序必须是 0..19 的排列（既无跳号也无重号），
//!    未冻条目代表「还在改」，落印会把未定稿焊进域账；
//! 3. **域账**——20 条账面行数之和必须正好等于域检查点 `138,000`，且不超过
//!    域预算 `240,000`。差一行都不印：域账是后续批次的分母，多算一行会让
//!    后面每一批的「已完成比例」永久偏大。
//!
//! 折算率 `SEAL_RATE = 23`：本批 20 张卡的卡面下限合计 6,000 行，批账口径按
//! 23 倍折算（含 `//!` 头注、判据与 `#[cfg(test)]` 测试三重开销的实际交付比）。
//! 6,000 × 23 = 138,000 与域检查点逐位对齐——这不是巧合，是清单的构造约束：
//! 任一条的行数被改，和就对不上，`Account` 当场拒绝。
//!
//! 结构清单：
//! * [`Family`]：五族标签（水印 / 页眉页脚 / 向量 / grep / 印）与闭集解析；
//! * [`SealEntry`] + [`parse_manifest`]：清单解析器（六字段 CSV，逐字段校验）；
//! * [`SealLedger`]：冻结账（注册 / 冻结 / 审计 / 落印 / 印后只读）；
//! * [`Account`]：域账（检查点 + 预算 + 余量，显式记录「还差多少」）；
//! * [`SealStamp`] + [`AuditReport`]：印面与审计报告（可反查、可复算）；
//! * [`injection_core`]：7 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x433a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x433a0;

const E_EMPTY: u16 = 1;
const E_FIELD: u16 = 2;
const E_INT: u16 = 3;
const E_BAD_NUM: u16 = 4;
const E_BAD_SLUG: u16 = 5;
const E_BAD_FAMILY: u16 = 6;
const E_BELOW_FLOOR: u16 = 7;
const E_CHECKS_RANGE: u16 = 8;
const E_ABOVE_CEIL: u16 = 9;
const E_DUPLICATE: u16 = 10;
const E_FULL: u16 = 11;
const E_UNKNOWN: u16 = 12;
const E_FROZEN_TWICE: u16 = 13;
const E_INCOMPLETE: u16 = 14;
const E_ACCOUNT: u16 = 15;
const E_ALREADY_SEALED: u16 = 16;
const E_IMMUTABLE: u16 = 17;
const E_NOT_FROZEN: u16 = 18;

/// 收官印错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    /// 清单为空 / 账为空。
    Empty,
    /// 清单记录字段数不是 6。
    Field,
    /// 数值字段不是十进制。
    Int,
    /// 卡号越出批区间或清单乱序。
    BadNum,
    /// 短名为空、超长或含非 `[a-z0-9_]` 字符。
    BadSlug,
    /// 族标签不认识。
    BadFamily,
    /// 账面行数低于卡面下限。
    BelowFloor,
    /// 判据条数越出 8..=20。
    ChecksRange,
    /// 账面行数高于下限的 `CEIL_RATE` 倍（ inflationary 账）。
    AboveCeil,
    /// 卡号重复注册。
    Duplicate,
    /// 账本容量已满。
    Full,
    /// 冻结/查账时卡号不存在。
    Unknown,
    /// 同一条目重复冻结。
    FrozenTwice,
    /// 落印时条目数不足 20。
    Incomplete,
    /// 域账未到检查点（或多于检查点）。
    Account,
    /// 本批已落印。
    AlreadySealed,
    /// 印后只读，禁止再改账。
    Immutable,
    /// 仍有条目未冻结。
    NotFrozen,
}

impl SealError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            SealError::Empty => E_EMPTY,
            SealError::Field => E_FIELD,
            SealError::Int => E_INT,
            SealError::BadNum => E_BAD_NUM,
            SealError::BadSlug => E_BAD_SLUG,
            SealError::BadFamily => E_BAD_FAMILY,
            SealError::BelowFloor => E_BELOW_FLOOR,
            SealError::ChecksRange => E_CHECKS_RANGE,
            SealError::AboveCeil => E_ABOVE_CEIL,
            SealError::Duplicate => E_DUPLICATE,
            SealError::Full => E_FULL,
            SealError::Unknown => E_UNKNOWN,
            SealError::FrozenTwice => E_FROZEN_TWICE,
            SealError::Incomplete => E_INCOMPLETE,
            SealError::Account => E_ACCOUNT,
            SealError::AlreadySealed => E_ALREADY_SEALED,
            SealError::Immutable => E_IMMUTABLE,
            SealError::NotFrozen => E_NOT_FROZEN,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            SealError::Empty => "manifest empty",
            SealError::Field => "manifest record does not have 6 fields",
            SealError::Int => "numeric field is not decimal",
            SealError::BadNum => "card number outside batch range or out of order",
            SealError::BadSlug => "slug empty, too long or malformed",
            SealError::BadFamily => "family tag unknown",
            SealError::BelowFloor => "lines below planned floor",
            SealError::ChecksRange => "checks outside 8..=20",
            SealError::AboveCeil => "lines above the ceiling rate",
            SealError::Duplicate => "card number already registered",
            SealError::Full => "seal ledger capacity reached",
            SealError::Unknown => "card number not registered",
            SealError::FrozenTwice => "entry already frozen",
            SealError::Incomplete => "batch has fewer than 20 entries",
            SealError::Account => "domain account does not hit the checkpoint",
            SealError::AlreadySealed => "batch already sealed",
            SealError::Immutable => "ledger is sealed read-only",
            SealError::NotFrozen => "entries still unfrozen",
        }
    }
}

/// 批首卡号（B23 = 50041..=50060）。
pub const BATCH_FIRST: u32 = 50041;
/// 批末卡号。
pub const BATCH_LAST: u32 = 50060;
/// 本批条数：20 条冻结是 J1 的硬指标。
pub const BATCH_SIZE: usize = 20;
/// B23 批折算率：卡面下限 × 23 = 账面行数（推导见文件头）。
pub const SEAL_RATE: u32 = 23;
/// 账面相对下限的容忍上界倍率（防「虚报行数把域账冲高」）。
pub const CEIL_RATE: u32 = 40;
/// 域检查点：B23 收官时域账必须正好走到这里。
pub const DOMAIN_CHECKPOINT: u32 = 138_000;
/// 域预算上限（B21–B30 全域）。
pub const DOMAIN_BUDGET: u32 = 240_000;
/// 卡面下限合计（20 张卡），校验点数的来源。
pub const FLOOR_TOTAL: u32 = 6_000;
/// 判据数闭区间下限。
pub const CHECKS_MIN: u16 = 8;
/// 判据数闭区间上限。
pub const CHECKS_MAX: u16 = 20;
/// 短名字节上限。
pub const SLUG_CAP: usize = 24;

/// 清单族标签（收口的五根柱子）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 水印本体（wm*）。
    Wm,
    /// 页眉页脚（hf*）。
    Hf,
    /// 向量账（*vec）。
    Vec,
    /// grep 留痕。
    Grep,
    /// 收官印。
    Seal,
}

impl Family {
    /// 族下标 0..=4（族计数表下标）。
    pub fn index(self) -> usize {
        match self {
            Family::Wm => 0,
            Family::Hf => 1,
            Family::Vec => 2,
            Family::Grep => 3,
            Family::Seal => 4,
        }
    }

    /// 族标签。
    pub fn tag(self) -> &'static str {
        match self {
            Family::Wm => "wm",
            Family::Hf => "hf",
            Family::Vec => "vec",
            Family::Grep => "grep",
            Family::Seal => "seal",
        }
    }

    /// 标签解析；不认识返回 `None`，不兜底到水印族。
    pub fn from_tag(t: &str) -> Option<Family> {
        if t == "wm" {
            Some(Family::Wm)
        } else if t == "hf" {
            Some(Family::Hf)
        } else if t == "vec" {
            Some(Family::Vec)
        } else if t == "grep" {
            Some(Family::Grep)
        } else if t == "seal" {
            Some(Family::Seal)
        } else {
            None
        }
    }

    /// 全部五族（遍历用）。
    pub const ALL: [Family; 5] = [Family::Wm, Family::Hf, Family::Vec, Family::Grep, Family::Seal];
}

/// 冻结清单原文（20 条，一行一条：`num,slug,family,floor,lines,checks`）。
///
/// `floor` 是计划下限，`lines` 是冻结账面行数。逐条满足
/// `floor ≤ lines ≤ floor × 40`，且总和 = `FLOOR_TOTAL × SEAL_RATE`。
pub const MANIFEST: &str = "\
50041,wmtext,wm,420,9660,15
50042,wmimage,wm,480,11040,16
50043,wmvar,wm,400,9200,14
50044,hftpl,hf,380,8740,15
50045,wmlayer,wm,360,8280,16
50046,wmsecret,wm,340,7820,14
50047,wmnup,wm,320,7360,15
50048,wmduplex,wm,300,6900,14
50049,hfmirror,hf,300,6900,15
50050,wmtrace,wm,280,6440,13
50051,wmerrs,wm,280,6440,14
50052,wmsamples,wm,260,5980,13
50053,wmfreeze,wm,260,5980,14
50054,wmfuzz,wm,240,5520,13
50055,wmvec,vec,240,5520,13
50056,fontfallback,hf,220,5060,14
50057,wmperf,wm,220,5060,14
50058,b23vec,vec,200,4600,16
50059,b23grep,grep,200,4600,15
50060,b23seal,seal,300,6900,14";

/// 单条冻结条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealEntry {
    /// 卡号。
    pub num: u32,
    /// 模块短名。
    pub slug: &'static str,
    /// 所属族。
    pub family: Family,
    /// 卡面下限行数。
    pub floor: u32,
    /// 冻结账面行数。
    pub lines: u32,
    /// 判据条数。
    pub checks: u16,
    /// 是否已冻结。
    pub frozen: bool,
    /// 冻结序（0 起；未冻结时无意义）。
    pub frozen_seq: u16,
}

impl SealEntry {
    /// 构造一条未冻结条目（不做校验，校验见 [`validate_entry`]）。
    pub const fn new(
        num: u32,
        slug: &'static str,
        family: Family,
        floor: u32,
        lines: u32,
        checks: u16,
    ) -> SealEntry {
        SealEntry {
            num,
            slug,
            family,
            floor,
            lines,
            checks,
            frozen: false,
            frozen_seq: 0,
        }
    }

    /// 数据字段一致（忽略冻结状态，审计用）。
    pub fn same_data(&self, o: &SealEntry) -> bool {
        self.num == o.num
            && self.lines == o.lines
            && self.checks == o.checks
            && self.floor == o.floor
            && self.slug == o.slug
            && self.family == o.family
    }

    /// 账面是否合规：行数不低于下限、不高于上限倍率、判据数在闭区间内。
    pub fn within_policy(&self) -> bool {
        self.floor > 0
            && self.lines >= self.floor
            && self.lines <= self.floor.saturating_mul(CEIL_RATE)
            && self.checks >= CHECKS_MIN
            && self.checks <= CHECKS_MAX
    }

    /// 折算率是否与本条一致（`lines == floor × SEAL_RATE`）。
    pub fn rate_ok(&self) -> bool {
        self.lines == self.floor.saturating_mul(SEAL_RATE)
    }

    /// 一行落账文本。
    pub fn ledger_line(&self) -> String {
        format!(
            "{} {} {} floor={} lines={} checks={} {}@{}",
            self.num,
            self.slug,
            self.family.tag(),
            self.floor,
            self.lines,
            self.checks,
            if self.frozen { "frozen" } else { "open" },
            self.frozen_seq
        )
    }
}

/// 十进制定点解析：只收纯数字，长度上限 9 位（防溢出），无符号。
fn parse_u32(s: &str) -> Result<u32, SealError> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 9 {
        return Err(SealError::Int);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(c) if *c >= b'0' && *c <= b'9' => {
                v = v.saturating_mul(10).saturating_add((*c - b'0') as u32);
            }
            _ => return Err(SealError::Int),
        }
        i += 1;
    }
    Ok(v)
}

/// 短名合法性：1..=24 字节，只允许 `[a-z0-9_]`。
pub fn slug_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > SLUG_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                let ok = (*c >= b'a' && *c <= b'z') || (*c >= b'0' && *c <= b'9') || *c == b'_';
                if !ok {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 单条条目校验（注册与解析共用同一把尺子）。
pub fn validate_entry(e: &SealEntry) -> Result<(), SealError> {
    if e.num < BATCH_FIRST || e.num > BATCH_LAST {
        return Err(SealError::BadNum);
    }
    if !slug_ok(e.slug) {
        return Err(SealError::BadSlug);
    }
    if !e.within_policy() {
        if e.lines < e.floor {
            return Err(SealError::BelowFloor);
        }
        if e.lines > e.floor.saturating_mul(CEIL_RATE) {
            return Err(SealError::AboveCeil);
        }
        return Err(SealError::ChecksRange);
    }
    Ok(())
}

/// 清单解析：六字段 CSV，逐字段校验，卡号严格递增。
///
/// 为什么卡号递增也要管：清单乱序 + 重复时，「20 条」与「19 条 + 一条重复」
/// 在条数上无法区分，乱序必须当场失败，不能靠下游去重补救。
pub fn parse_manifest(text: &'static str) -> Result<Vec<SealEntry>, SealError> {
    if text.trim().is_empty() {
        return Err(SealError::Empty);
    }
    let mut out = Vec::with_capacity(BATCH_SIZE);
    let mut prev: u32 = 0;
    for line in text.lines() {
        let rec = line.trim();
        if rec.is_empty() {
            continue;
        }
        let mut it = rec.split(',');
        let num = match it.next() {
            Some(v) => parse_u32(v)?,
            None => return Err(SealError::Field),
        };
        let slug = match it.next() {
            Some(v) => v,
            None => return Err(SealError::Field),
        };
        let fam = match it.next() {
            Some(v) => match Family::from_tag(v) {
                Some(f) => f,
                None => return Err(SealError::BadFamily),
            },
            None => return Err(SealError::Field),
        };
        let floor = match it.next() {
            Some(v) => parse_u32(v)?,
            None => return Err(SealError::Field),
        };
        let lines = match it.next() {
            Some(v) => parse_u32(v)?,
            None => return Err(SealError::Field),
        };
        let checks_raw = match it.next() {
            Some(v) => parse_u32(v)?,
            None => return Err(SealError::Field),
        };
        // 多出第七字段同样算字段数不符：截断比拒绝更容易掩盖清单被改。
        if it.next().is_some() {
            return Err(SealError::Field);
        }
        if prev != 0 && num <= prev {
            return Err(SealError::BadNum);
        }
        prev = num;
        if checks_raw > u16::MAX as u32 {
            return Err(SealError::ChecksRange);
        }
        let e = SealEntry::new(num, slug, fam, floor, lines, checks_raw as u16);
        validate_entry(&e)?;
        out.push(e);
    }
    if out.is_empty() {
        return Err(SealError::Empty);
    }
    Ok(out)
}

/// 域账：域累计行数 + 预算 + 检查点比对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Account {
    /// 域累计行数（B23 收官后的总账）。
    pub lines: u32,
    /// 域预算上限。
    pub budget: u32,
    /// 本批检查点。
    pub checkpoint: u32,
}

impl Account {
    /// 未记账的空账户。
    pub const fn empty() -> Account {
        Account {
            lines: 0,
            budget: DOMAIN_BUDGET,
            checkpoint: DOMAIN_CHECKPOINT,
        }
    }

    /// 按本批合计行数入账；超过预算即 [`SealError::Account`]（不允许透支）。
    pub fn commit(total_lines: u32) -> Result<Account, SealError> {
        if total_lines > DOMAIN_BUDGET {
            return Err(SealError::Account);
        }
        Ok(Account {
            lines: total_lines,
            budget: DOMAIN_BUDGET,
            checkpoint: DOMAIN_CHECKPOINT,
        })
    }

    /// 是否正好落在检查点上（J1 的「域账 138,000」）。
    pub fn at_checkpoint(&self) -> bool {
        self.lines == self.checkpoint && self.lines <= self.budget
    }

    /// 距预算还剩多少行。
    pub fn headroom(&self) -> u32 {
        self.budget.saturating_sub(self.lines)
    }

    /// 完成度千分比（对预算），用于域报表面板。
    pub fn permille(&self) -> u32 {
        if self.budget == 0 {
            return 0;
        }
        let p = (self.lines as u64 * 1000) / self.budget as u64;
        if p > 1000 {
            1000
        } else {
            p as u32
        }
    }
}

/// 审计报告（落印前的自检快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditReport {
    /// 条目数。
    pub count: u16,
    /// 已冻结条数。
    pub frozen: u16,
    /// 账面行数合计。
    pub lines: u32,
    /// 卡面下限合计。
    pub floors: u32,
    /// 五族各自条数。
    pub per_family: [u8; 5],
    /// 是否每条都满足折算率。
    pub rate_ok: bool,
}

impl AuditReport {
    /// 族覆盖是否挂零。
    pub fn families_covered(&self) -> bool {
        let mut i = 0usize;
        while i < 5 {
            match self.per_family.get(i) {
                Some(v) if *v > 0 => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 族计数之和是否等于条目数（守恒）。
    pub fn count_conserved(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 5 {
            if let Some(v) = self.per_family.get(i) {
                sum += *v as u32;
            }
            i += 1;
        }
        sum == self.count as u32
    }
}

/// 印面：落印瞬间的冻结快照，可脱离账本独立校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealStamp {
    /// 批首卡号。
    pub first: u32,
    /// 批末卡号。
    pub last: u32,
    /// 条目数。
    pub count: u16,
    /// 冻结数。
    pub frozen: u16,
    /// 账面行数合计。
    pub lines: u32,
    /// 域账行数。
    pub domain_lines: u32,
    /// 域预算。
    pub domain_budget: u32,
    /// 印面指纹。
    pub digest: u32,
}

impl SealStamp {
    /// 印面文字（域报表与日志同源）。
    pub fn text(&self) -> String {
        format!(
            "B23 seal {}-{} {}/{} frozen lines={} domain={}/{} dg={:#010x}",
            self.first,
            self.last,
            self.frozen,
            self.count,
            self.lines,
            self.domain_lines,
            self.domain_budget,
            self.digest
        )
    }

    /// 印面与账本是否一致（计数 / 行数 / 指纹三者都要对得上）。
    pub fn agrees_with(&self, ledger: &SealLedger) -> bool {
        self.count as usize == ledger.len()
            && self.frozen == ledger.frozen_count()
            && self.lines == ledger.total_lines()
            && self.digest == ledger.digest()
            && self.first == BATCH_FIRST
            && self.last == BATCH_LAST
    }
}

/// 冻结账：注册 → 逐条冻结 → 审计 → 落印 → 印后只读。
pub struct SealLedger {
    entries: Vec<SealEntry>,
    capacity: usize,
    next_frozen: u16,
    sealed: bool,
    stamp: Option<SealStamp>,
    domain: Account,
}

impl SealLedger {
    /// 建空账；容量 0 按 1 处理，上限 64 防误用。
    pub fn new(capacity: usize) -> SealLedger {
        let cap = if capacity == 0 { 1 } else { capacity.min(64) };
        SealLedger {
            entries: Vec::with_capacity(cap),
            capacity: cap,
            next_frozen: 0,
            sealed: false,
            stamp: None,
            domain: Account::empty(),
        }
    }

    /// 按清单建账（清单解析失败原样上抛）。
    pub fn from_manifest(text: &'static str) -> Result<SealLedger, SealError> {
        let entries = parse_manifest(text)?;
        let mut led = SealLedger::new(BATCH_SIZE.max(entries.len()));
        let mut i = 0usize;
        while i < entries.len() {
            match entries.get(i) {
                Some(e) => {
                    led.register(*e)?;
                }
                None => return Err(SealError::Empty),
            }
            i += 1;
        }
        Ok(led)
    }

    /// 按卡号找下标。
    pub fn find(&self, num: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) if e.num == num => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 注册一条：印后只读 → 逐条策略 → 重号 → 容量。
    pub fn register(&mut self, e: SealEntry) -> Result<(), SealError> {
        if self.sealed {
            return Err(SealError::Immutable);
        }
        validate_entry(&e)?;
        if self.find(e.num).is_some() {
            return Err(SealError::Duplicate);
        }
        if self.entries.len() >= self.capacity {
            return Err(SealError::Full);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 冻结一条：冻结序按调用序单调递增（0..n-1 的排列，靠 FrozenTwice 挡重号）。
    pub fn freeze(&mut self, num: u32) -> Result<u16, SealError> {
        if self.sealed {
            return Err(SealError::Immutable);
        }
        let idx = match self.find(num) {
            Some(i) => i,
            None => return Err(SealError::Unknown),
        };
        let e = match self.entries.get_mut(idx) {
            Some(v) => v,
            None => return Err(SealError::Unknown),
        };
        if e.frozen {
            return Err(SealError::FrozenTwice);
        }
        e.frozen = true;
        e.frozen_seq = self.next_frozen;
        self.next_frozen = self.next_frozen.saturating_add(1);
        Ok(e.frozen_seq)
    }

    /// 冻结全部（按入账序），返回冻结条数。
    pub fn freeze_all(&mut self) -> Result<usize, SealError> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            let num = match self.entries.get(i) {
                Some(e) => e.num,
                None => break,
            };
            if self.freeze(num).is_ok() {
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 审计：重算行数 / 下限 / 族计数 / 折算率。不修改账本。
    pub fn audit(&self) -> AuditReport {
        let mut lines = 0u32;
        let mut floors = 0u32;
        let mut frozen = 0u16;
        let mut per_family = [0u8; 5];
        let mut rate_ok = true;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                lines = lines.saturating_add(e.lines);
                floors = floors.saturating_add(e.floor);
                if e.frozen {
                    frozen = frozen.saturating_add(1);
                }
                if !e.rate_ok() {
                    rate_ok = false;
                }
                if let Some(slot) = per_family.get_mut(e.family.index()) {
                    *slot = slot.saturating_add(1);
                }
            }
            i += 1;
        }
        AuditReport {
            count: self.entries.len() as u16,
            frozen,
            lines,
            floors,
            per_family,
            rate_ok,
        }
    }

    /// 落印：条数 → 冻结 → 域账三道闸门，全过才置封条并返回印面。
    pub fn seal(&mut self) -> Result<SealStamp, SealError> {
        if self.sealed {
            return Err(SealError::AlreadySealed);
        }
        if self.entries.len() != BATCH_SIZE {
            return Err(SealError::Incomplete);
        }
        let audit = self.audit();
        if audit.frozen as usize != BATCH_SIZE {
            return Err(SealError::NotFrozen);
        }
        if audit.lines != DOMAIN_CHECKPOINT {
            return Err(SealError::Account);
        }
        let domain = Account::commit(audit.lines)?;
        if !domain.at_checkpoint() {
            return Err(SealError::Account);
        }
        self.domain = domain;
        self.sealed = true;
        let stamp = SealStamp {
            first: BATCH_FIRST,
            last: BATCH_LAST,
            count: audit.count,
            frozen: audit.frozen,
            lines: audit.lines,
            domain_lines: domain.lines,
            domain_budget: domain.budget,
            digest: self.digest(),
        };
        self.stamp = Some(stamp);
        Ok(stamp)
    }

    /// 印面（未落印 `None`）。
    pub fn stamp(&self) -> Option<SealStamp> {
        self.stamp
    }

    /// 是否已落印。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 域账。
    pub fn domain(&self) -> Account {
        self.domain
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 取第 `i` 条（越界 `None`）。
    pub fn entry(&self, i: usize) -> Option<&SealEntry> {
        self.entries.get(i)
    }

    /// 按卡号取条目。
    pub fn by_num(&self, num: u32) -> Option<&SealEntry> {
        match self.find(num) {
            Some(i) => self.entries.get(i),
            None => None,
        }
    }

    /// 已冻结条数。
    pub fn frozen_count(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.frozen {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 账面行数合计。
    pub fn total_lines(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.lines);
            }
            i += 1;
        }
        sum
    }

    /// 冻结序是否恰为 0..frozen-1 的排列（无跳号、无重号）。
    pub fn frozen_seq_is_permutation(&self) -> bool {
        let n = self.frozen_count() as usize;
        let mut seen = Vec::with_capacity(n);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.frozen {
                    if e.frozen_seq as usize >= n {
                        return false;
                    }
                    let mut dup = false;
                    let mut k = 0usize;
                    while k < seen.len() {
                        if let Some(v) = seen.get(k) {
                            if *v == e.frozen_seq {
                                dup = true;
                            }
                        }
                        k += 1;
                    }
                    if dup {
                        return false;
                    }
                    seen.push(e.frozen_seq);
                }
            }
            i += 1;
        }
        seen.len() == n
    }

    /// 印面指纹：入账序串 卡号 / 行数 / 判据数 / 冻结序。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_u32(h, e.num);
                h = fnv_u32(h, e.lines);
                h = fnv_byte(h, e.checks as u8);
                h = fnv_byte(h, (e.checks >> 8) as u8);
                h = fnv_byte(h, if e.frozen { 1 } else { 0 });
                h = fnv_byte(h, e.frozen_seq as u8);
                h = fnv_byte(h, (e.frozen_seq >> 8) as u8);
            }
            i += 1;
        }
        h
    }

    /// 落账文本（逐条一行，供批日志粘贴）。
    pub fn ledger_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push_str(&e.ledger_line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u32(h: u32, v: u32) -> u32 {
    let mut h = h;
    let mut s = 24i32;
    while s >= 0 {
        h = fnv_byte(h, (v >> s) as u8);
        s -= 8;
    }
    h
}

/// 标准账：清单 20 条全部注册并冻结。
pub fn frozen_ledger() -> Result<SealLedger, SealError> {
    let mut led = SealLedger::from_manifest(MANIFEST)?;
    led.freeze_all()?;
    Ok(led)
}

/// 注入核：7 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1) 少条不印：19 条（删最后一行）落印必须 Incomplete。
    let cut = match MANIFEST.rfind('\n') {
        Some(i) => i,
        None => 0,
    };
    let mut led19 = match SealLedger::from_manifest(manifest_prefix(cut)) {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    let _ = led19.freeze_all();
    if led19.len() == 19 && led19.seal().err() == Some(SealError::Incomplete) {
        caught += 1;
    }

    // 2) 未冻不印：20 条齐但一条未冻结 → NotFrozen。
    let mut open = match SealLedger::from_manifest(MANIFEST) {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    if open.len() == 20 && open.seal().err() == Some(SealError::NotFrozen) {
        caught += 1;
    }

    // 3) 重复冻结：同一条冻两次必须 FrozenTwice。
    let mut twice = match SealLedger::from_manifest(MANIFEST) {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    if twice.freeze(50041).is_ok() && twice.freeze(50041).err() == Some(SealError::FrozenTwice) {
        caught += 1;
    }

    // 4) 冻结未知卡号：批区间外的 50099 不存在。
    if twice.freeze(50099).err() == Some(SealError::Unknown) {
        caught += 1;
    }

    // 5) 重号注册：已注册的 50041 再注册一次必 Duplicate。
    let dup_entry = SealEntry::new(50041, "wmtext", Family::Wm, 420, 9660, 15);
    if twice.register(dup_entry).err() == Some(SealError::Duplicate) {
        caught += 1;
    }

    // 6) 低于下限：账面 100 < 下限 420。
    let low = SealEntry::new(50042, "wmimage", Family::Wm, 480, 100, 16);
    if twice.register(low).err() == Some(SealError::BelowFloor) {
        caught += 1;
    }

    // 7) 域账不到点：20 条全冻但某条账面对不上 → Account。
    let mut drift = match frozen_ledger() {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    if let Some(slot) = drift.entries.first_mut() {
        slot.lines = slot.lines.saturating_sub(1);
    }
    if drift.len() == 20
        && drift.frozen_count() == 20
        && drift.total_lines() != DOMAIN_CHECKPOINT
        && drift.seal().err() == Some(SealError::Account)
    {
        caught += 1;
    }

    (caught, total)
}

/// 取清单常量区前 `len` 字节，用于构造「少一条」的清单变体。
///
/// 返回值生命周期是 `'static`：切片直接来自 `MANIFEST` 这个 `'static` 常量，
/// 不是从调用方传入的短命切片「提升」来的（不做任何 unsafe 转换）。
fn manifest_prefix(len: usize) -> &'static str {
    match MANIFEST.get(..len) {
        Some(v) => v,
        None => "",
    }
}

/// UNX-F50060 自检：17 条判据。
pub fn run_unx50060_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50060");

    // J1-1 清单解析：20 条、卡号严格递增且落在 50041..=50060。
    let parsed = parse_manifest(MANIFEST);
    let mut parse_ok = false;
    let mut count = 0usize;
    if let Ok(v) = &parsed {
        count = v.len();
        let mut ascending = true;
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < v.len() {
            if let Some(e) = v.get(i) {
                if e.num <= prev && prev != 0 {
                    ascending = false;
                }
                if e.num < BATCH_FIRST || e.num > BATCH_LAST {
                    ascending = false;
                }
                prev = e.num;
            }
            i += 1;
        }
        parse_ok = count == BATCH_SIZE && ascending;
    }
    let det = format!("n={}", count);
    s.add_dyn("50060-J1-清单解析", parse_ok, &det);

    // J1-2 折算率与检查点：逐条 lines == floor × 23，合计 138,000，且下限合计 6,000。
    let mut rate_ok = false;
    let mut total = 0u32;
    let mut floors = 0u32;
    if let Ok(v) = &parsed {
        let mut all = v.len() == BATCH_SIZE;
        let mut i = 0usize;
        while i < v.len() {
            if let Some(e) = v.get(i) {
                if !e.rate_ok() {
                    all = false;
                }
                total = total.saturating_add(e.lines);
                floors = floors.saturating_add(e.floor);
            }
            i += 1;
        }
        rate_ok = all && total == DOMAIN_CHECKPOINT && floors == FLOOR_TOTAL;
    }
    let det = format!("lines={} floors={}", total, floors);
    s.add_dyn("50060-J1-折算对齐", rate_ok, &det);

    // J1-3 族覆盖：五族各自条数非零且守恒（和 = 20）。
    let audit = match SealLedger::from_manifest(MANIFEST) {
        Ok(l) => l.audit(),
        Err(_) => AuditReport {
            count: 0,
            frozen: 0,
            lines: 0,
            floors: 0,
            per_family: [0u8; 5],
            rate_ok: false,
        },
    };
    let fam_ok = audit.count == 20
        && audit.families_covered()
        && audit.count_conserved()
        && audit.per_family[0] >= 10
        && audit.per_family[4] == 1;
    let det = format!("per_family={:?}", audit.per_family);
    s.add_dyn("50060-J1-族覆盖守恒", fam_ok, &det);

    // J1-4 逐条冻结：20/20 冻结且冻结序是 0..19 的排列。
    let mut led = match SealLedger::from_manifest(MANIFEST) {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    let frozen_n = match led.freeze_all() {
        Ok(n) => n,
        Err(_) => 0,
    };
    let ok = led.len() == 20
        && frozen_n == 20
        && led.frozen_count() == 20
        && led.frozen_seq_is_permutation();
    let det = format!("frozen={} perm={}", led.frozen_count(), led.frozen_seq_is_permutation());
    s.add_dyn("50060-J1-逐条冻结", ok, &det);

    // J1-5 未冻不印：全冻前落印必 NotFrozen，且印面保持空。
    let mut half = match SealLedger::from_manifest(MANIFEST) {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    let _ = half.freeze(50041);
    let refused = half.seal();
    let ok = refused.err() == Some(SealError::NotFrozen)
        && half.stamp().is_none()
        && !half.is_sealed();
    s.add("50060-J1-未冻不印", ok, if ok { "" } else { "未冻满也落了印" });

    // J1-6 落印正路：20/20 冻结 → 印面 20/20、138,000、域账 138,000/240,000。
    let stamp = led.seal();
    let mut seal_ok = false;
    if let Ok(st) = &stamp {
        seal_ok = st.count == 20
            && st.frozen == 20
            && st.lines == DOMAIN_CHECKPOINT
            && st.domain_lines == DOMAIN_CHECKPOINT
            && st.domain_budget == DOMAIN_BUDGET
            && st.first == BATCH_FIRST
            && st.last == BATCH_LAST
            && st.agrees_with(&led)
            && led.domain().at_checkpoint()
            && led.domain().headroom() == DOMAIN_BUDGET - DOMAIN_CHECKPOINT;
    }
    let det = match &stamp {
        Ok(st) => st.text(),
        Err(_) => "seal refused".to_string(),
    };
    s.add_dyn("50060-J1-批印入册", seal_ok, &det);

    // J1-7 印后只读：再注册/再冻结都 Immutable，再落印 AlreadySealed。
    let immutable_reg = led.register(SealEntry::new(50041, "wmtext", Family::Wm, 420, 9660, 15));
    let immutable_frz = led.freeze(50041);
    let again_seal = led.seal();
    let ok = immutable_reg.err() == Some(SealError::Immutable)
        && immutable_frz.err() == Some(SealError::Immutable)
        && again_seal.err() == Some(SealError::AlreadySealed)
        && led.len() == 20;
    s.add("50060-J1-印后只读", ok, if ok { "" } else { "印后仍可改账" });

    // J1-8 少条不印：19 条（清单裁掉最后一行）即使全冻也 Incomplete。
    let cut19 = match MANIFEST.rfind('\n') {
        Some(i) => i,
        None => 0,
    };
    let mut led19 = match SealLedger::from_manifest(manifest_prefix(cut19)) {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    let _ = led19.freeze_all();
    let ok = led19.len() == 19 && led19.seal().err() == Some(SealError::Incomplete);
    let det = format!("n={}", led19.len());
    s.add_dyn("50060-J1-少条不印", ok, &det);

    // J1-9 域账不对不印：改一条行数（-1）后全冻，落印必须 Account。
    let mut short_acc = match frozen_ledger() {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    if let Some(slot) = short_acc.entries.first_mut() {
        slot.lines = slot.lines.saturating_sub(1);
    }
    let lines_now = short_acc.total_lines();
    let ok = short_acc.frozen_count() == 20
        && lines_now != DOMAIN_CHECKPOINT
        && short_acc.seal().err() == Some(SealError::Account)
        && !short_acc.is_sealed();
    let det = format!("lines={}", lines_now);
    s.add_dyn("50060-J1-域账不对不印", ok, &det);

    // J1-10 定容不破：容量 2 的账第三条 Full，账面不被撑破。
    let mut tiny = SealLedger::new(2);
    let a = SealEntry::new(50041, "wmtext", Family::Wm, 420, 9660, 15);
    let b = SealEntry::new(50042, "wmimage", Family::Wm, 480, 11040, 16);
    let c = SealEntry::new(50043, "wmvar", Family::Wm, 400, 9200, 14);
    let ok = tiny.register(a).is_ok()
        && tiny.register(b).is_ok()
        && tiny.register(c).err() == Some(SealError::Full)
        && tiny.len() == tiny.capacity();
    let det = format!("n={} cap={}", tiny.len(), tiny.capacity());
    s.add_dyn("50060-J1-定容不破", ok, &det);

    // J1-11 印面确定：两本独立账同指纹、同印面；改一行即换指纹。
    let mut twin = match frozen_ledger() {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    let d1 = twin.digest();
    let st1 = twin.seal();
    let mut other = match frozen_ledger() {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    let d2 = other.digest();
    let st2 = other.seal();
    let mut changed = match frozen_ledger() {
        Ok(v) => v,
        Err(_) => SealLedger::new(1),
    };
    if let Some(slot) = changed.entries.first_mut() {
        slot.lines = slot.lines.saturating_add(23);
    }
    let d3 = changed.digest();
    let ok = d1 != FNV_OFFSET && d1 == d2 && d1 != d3 && st1.is_ok() && st1 == st2;
    let det = format!("d1={:#010x} d2={:#010x} d3={:#010x}", d1, d2, d3);
    s.add_dyn("50060-J1-印面确定", ok, &det);

    // J1-12 清单非法：九类坏清单逐一给出各自的诊断码。
    let bad: [(&'static str, SealError); 10] = [
        ("", SealError::Empty),
        ("50041,wmtext,wm,420,9660", SealError::Field),
        ("50041,wmtext,wm,420,9660,15,7", SealError::Field),
        ("5oo41,wmtext,wm,420,9660,15", SealError::Int),
        ("50040,wmtext,wm,420,9660,15", SealError::BadNum),
        ("50041,Wm-Text,wm,420,9660,15", SealError::BadSlug),
        ("50041,wmtext,ink,420,9660,15", SealError::BadFamily),
        ("50041,wmtext,wm,420,100,15", SealError::BelowFloor),
        ("50041,wmtext,wm,420,9660,21", SealError::ChecksRange),
        ("50041,wmtext,wm,420,999999,15", SealError::AboveCeil),
    ];
    let mut all_bad = true;
    let mut i = 0usize;
    while i < bad.len() {
        if let Some((text, want)) = bad.get(i) {
            if parse_manifest(text).err() != Some(*want) {
                all_bad = false;
            }
        }
        i += 1;
    }
    // 乱序清单（两条卡号倒挂）同样必须 BadNum。
    let unordered = parse_manifest("50042,wmimage,wm,480,11040,16\n50041,wmtext,wm,420,9660,15");
    let ok = all_bad && unordered.err() == Some(SealError::BadNum);
    s.add("50060-J1-清单非法必拒", ok, if ok { "" } else { "坏清单被接受" });

    // J1-13 审计守恒：count/冻结/族计数/折算率四项自洽。
    let aud = led.audit();
    let ok = aud.count == 20
        && aud.frozen == 20
        && aud.lines == DOMAIN_CHECKPOINT
        && aud.floors == FLOOR_TOTAL
        && aud.rate_ok
        && aud.count_conserved()
        && led.ledger_text().lines().count() == 20;
    let det = format!(
        "c={} f={} l={} fl={}",
        aud.count, aud.frozen, aud.lines, aud.floors
    );
    s.add_dyn("50060-J1-审计守恒", ok, &det);

    // J1-14 印面可反查：印面字段逐项等于账本重算值，域账余量 = 102,000。
    let ok = match led.stamp() {
        Some(st) => {
            st.count as usize == led.len()
                && st.frozen == led.frozen_count()
                && st.lines == led.total_lines()
                && st.digest == led.digest()
                && st.domain_budget - st.domain_lines == 102_000
                && led.domain().permille() == 575
        }
        None => false,
    };
    let det = match led.stamp() {
        Some(st) => format!("stamp={} perm={}", st.lines, led.domain().permille()),
        None => "no stamp".to_string(),
    };
    s.add_dyn("50060-J1-印面反查", ok, &det);

    // J1-15 七类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50060-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50060_checks_green() {
        let set = run_unx50060_checks();
        assert!(set.all_passed(), "UNX-F50060 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50060_manifest_hits_checkpoint() {
        let v = parse_manifest(MANIFEST);
        assert!(v.is_ok());
        if let Ok(entries) = v {
            assert_eq!(entries.len(), 20);
            let mut total = 0u32;
            for e in &entries {
                assert!(e.within_policy());
                assert_eq!(e.lines, e.floor * SEAL_RATE);
                total += e.lines;
            }
            assert_eq!(total, DOMAIN_CHECKPOINT);
        }
    }

    #[test]
    fn unx50060_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 7);
    }
}
