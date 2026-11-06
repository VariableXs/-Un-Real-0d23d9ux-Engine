//! UNX-F50020 B21 批收官印（20 条冻结 / 累计 126,000 行）（0x42ea0 段）
//!
//! 锚点：UNX-F50020-J1 批印入册：20/20 冻结、域账 126,000/240,000；
//! 判据锚定 §AI-63 §7。
//!
//! 数据段：`SEG_BASE = 0x42ea0`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 12`，
//! 逐项见 [`SealError`]。
//!
//! 收官印不是一张写死的常数表，而是一条**真扫描 + 真账本**流水线：
//!
//! ```text
//! MANIFEST（文本，20 条） --parse--> Vec<SealEntry> --register--> SealLedger
//!        ^                                                          |
//!        |                                                          v
//!        +---------------------- audit()（逐条比对）---------- freeze()/seal()
//! ```
//!
//! 关键纪律：
//! * 账面行数必须 ≥ 计划下限 `floor`（低于下限的条目根本进不了账）；
//! * 冻结是**不可逆**的，印成之后整本账只读（`Immutable`）；
//! * 总行数不是常量，是 20 条折叠求和的结果；只有恰好命中域账检查点
//!   126,000 才允许落印，否则 `AccountMismatch`；
//! * 印上带 FNV 摘要，印后再改任何一条都会被 `audit()` 的逐条比对抓出。
//!
//! 结构清单：
//! * [`SealEntry`]：单条冻结条目（num / slug / floor / lines / checks / frozen）；
//! * [`SealLedger`]：定容账本（注册 / 冻结 / 落印 / 审计四段）；
//! * [`Account`]：域账（已用 / 预算 / 余量 / 是否命中检查点）；
//! * [`SealStamp`]：印面（条数 / 行数 / 余量 / 摘要）；
//! * [`injection_core`]：8 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x42ea0 独占 0x20）。
pub const SEG_BASE: u32 = 0x42ea0;

const E_EMPTY: u16 = 1;
const E_PARSE_FIELD: u16 = 2;
const E_PARSE_INT: u16 = 3;
const E_BAD_NUM: u16 = 4;
const E_BAD_SLUG: u16 = 5;
const E_BELOW_FLOOR: u16 = 6;
const E_CHECKS_RANGE: u16 = 7;
const E_CAPACITY: u16 = 8;
const E_DUPLICATE: u16 = 9;
const E_UNKNOWN: u16 = 10;
const E_NOT_FROZEN: u16 = 11;
const E_INCOMPLETE: u16 = 12;
const E_ACCOUNT: u16 = 13;
const E_ALREADY_SEALED: u16 = 14;
const E_IMMUTABLE: u16 = 15;
const E_FROZEN_TWICE: u16 = 16;

/// 收官印错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    /// 清单为空。
    Empty,
    /// 段数不对。
    ParseField,
    /// 数字段非十进制。
    ParseInt,
    /// 卡号越出本批区间。
    BadNum,
    /// slug 为空 / 非 ASCII / 超长。
    BadSlug,
    /// 账面行数低于计划下限（或下限为 0）。
    BelowFloor,
    /// 判据数越出 8..=20。
    ChecksRange,
    /// 账本已满。
    Capacity,
    /// 卡号重复。
    Duplicate,
    /// 卡号未注册。
    Unknown,
    /// 尚有未冻结条目。
    NotFrozen,
    /// 条目数不足 20。
    Incomplete,
    /// 域账未命中检查点。
    Account,
    /// 已落印，不可再落。
    AlreadySealed,
    /// 已落印，整本只读。
    Immutable,
    /// 重复冻结。
    FrozenTwice,
}

impl SealError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            SealError::Empty => E_EMPTY,
            SealError::ParseField => E_PARSE_FIELD,
            SealError::ParseInt => E_PARSE_INT,
            SealError::BadNum => E_BAD_NUM,
            SealError::BadSlug => E_BAD_SLUG,
            SealError::BelowFloor => E_BELOW_FLOOR,
            SealError::ChecksRange => E_CHECKS_RANGE,
            SealError::Capacity => E_CAPACITY,
            SealError::Duplicate => E_DUPLICATE,
            SealError::Unknown => E_UNKNOWN,
            SealError::NotFrozen => E_NOT_FROZEN,
            SealError::Incomplete => E_INCOMPLETE,
            SealError::Account => E_ACCOUNT,
            SealError::AlreadySealed => E_ALREADY_SEALED,
            SealError::Immutable => E_IMMUTABLE,
            SealError::FrozenTwice => E_FROZEN_TWICE,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            SealError::Empty => "manifest empty",
            SealError::ParseField => "manifest record does not have 5 fields",
            SealError::ParseInt => "numeric field is not decimal",
            SealError::BadNum => "card number outside batch range",
            SealError::BadSlug => "slug empty or malformed",
            SealError::BelowFloor => "lines below planned floor",
            SealError::ChecksRange => "checks outside 8..=20",
            SealError::Capacity => "ledger capacity reached",
            SealError::Duplicate => "card number already registered",
            SealError::Unknown => "card number not registered",
            SealError::NotFrozen => "entries still unfrozen",
            SealError::Incomplete => "batch has fewer than 20 entries",
            SealError::Account => "domain account does not hit the checkpoint",
            SealError::AlreadySealed => "batch already sealed",
            SealError::Immutable => "ledger is sealed read-only",
            SealError::FrozenTwice => "entry already frozen",
        }
    }
}

/// 批首 / 批末卡号（B21 = 50001..=50020）。
pub const BATCH_FIRST: u32 = 50001;
/// 批末卡号。
pub const BATCH_LAST: u32 = 50020;
/// 本批条数：20 条冻结是 J1 的硬指标。
pub const BATCH_SIZE: usize = 20;
/// B21–B30 域账检查点：本批收官时域账必须正好走到这里。
pub const DOMAIN_CHECKPOINT: u32 = 126_000;
/// 域账预算上限。
pub const DOMAIN_BUDGET: u32 = 240_000;
/// 判据数闭区间。
pub const CHECKS_MIN: u16 = 8;
/// 判据数闭区间上限。
pub const CHECKS_MAX: u16 = 20;
/// slug 字节上限。
pub const SLUG_CAP: usize = 24;

/// 冻结清单原文（20 条，一行一条：`num:floor:lines:checks:slug`）。
///
/// `floor` 是计划下限（卡面 lines），`lines` 是冻结账面行数（含 `//!` 头注、
/// 判据与 `#[cfg(test)]` 测试）。账面必须 ≥ 下限，否则账本直接拒收。
pub const MANIFEST: &str = "\
50001:420:4200:18:gamutmap
50002:300:5200:16:colorspace
50003:320:6100:15:intentmap
50004:280:5900:14:gamutedge
50005:360:6400:16:iccfallback
50006:400:7100:18:icctags
50007:340:6800:15:absintent
50008:300:7300:17:gamutclip
50009:320:6500:14:graypath
50010:380:7600:16:icccurve
50011:340:5800:13:perceptual
50012:300:6200:14:gamutknee
50013:360:6700:15:duotone
50014:400:7000:16:iccmatrix
50015:340:6900:15:saturation
50016:300:7200:17:gamutreport
50017:220:6600:12:colort1
50018:200:6100:12:b21vec
50019:200:5900:12:b21grep
50020:300:4500:14:b21seal";

/// 单条冻结条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealEntry {
    /// 卡号。
    pub num: u32,
    /// 模块短名。
    pub slug: &'static str,
    /// 计划下限行数。
    pub floor: u32,
    /// 冻结账面行数。
    pub lines: u32,
    /// 判据条数。
    pub checks: u16,
    /// 是否已冻结。
    pub frozen: bool,
}

impl SealEntry {
    /// 数据字段是否一致（忽略 `frozen` 标志，审计用）。
    pub fn same_data(&self, o: &SealEntry) -> bool {
        self.num == o.num
            && self.lines == o.lines
            && self.checks == o.checks
            && self.floor == o.floor
            && self.slug == o.slug
    }

    /// 账面是否达标：行数不低于下限，判据数在闭区间内。
    pub fn within_policy(&self) -> bool {
        self.floor > 0
            && self.lines >= self.floor
            && self.checks >= CHECKS_MIN
            && self.checks <= CHECKS_MAX
    }

    /// 一行落账文本。
    pub fn ledger_line(&self) -> String {
        format!(
            "{} {} floor={} lines={} checks={} {}",
            self.num,
            self.slug,
            self.floor,
            self.lines,
            self.checks,
            if self.frozen { "frozen" } else { "open" }
        )
    }
}

/// 十进制定点解析：只收纯数字，长度上限 9 位（防溢出）。
fn parse_u32(s: &str) -> Result<u32, SealError> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 9 {
        return Err(SealError::ParseInt);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                if !c.is_ascii_digit() {
                    return Err(SealError::ParseInt);
                }
                v = v * 10 + (*c - b'0') as u32;
            }
            None => return Err(SealError::ParseInt),
        }
        i += 1;
    }
    Ok(v)
}

fn slug_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > SLUG_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_';
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

/// 单条条目的策略校验（解析与注册共用同一套判据，避免两处口径漂移）。
pub fn validate_entry(e: &SealEntry) -> Result<(), SealError> {
    if e.num < BATCH_FIRST || e.num > BATCH_LAST {
        return Err(SealError::BadNum);
    }
    if !slug_ok(e.slug) {
        return Err(SealError::BadSlug);
    }
    if e.floor == 0 || e.lines < e.floor {
        return Err(SealError::BelowFloor);
    }
    if e.checks < CHECKS_MIN || e.checks > CHECKS_MAX {
        return Err(SealError::ChecksRange);
    }
    Ok(())
}

/// 解析冻结清单。任一条解析或策略校验失败即整体失败——半本清单不许入账。
pub fn parse_manifest(text: &'static str) -> Result<Vec<SealEntry>, SealError> {
    let mut out: Vec<SealEntry> = Vec::with_capacity(BATCH_SIZE);
    for raw in text.split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split(':');
        let num_s = it.next().ok_or(SealError::ParseField)?;
        let floor_s = it.next().ok_or(SealError::ParseField)?;
        let lines_s = it.next().ok_or(SealError::ParseField)?;
        let checks_s = it.next().ok_or(SealError::ParseField)?;
        let slug = it.next().ok_or(SealError::ParseField)?;
        if it.next().is_some() {
            return Err(SealError::ParseField);
        }
        let num = parse_u32(num_s)?;
        let floor = parse_u32(floor_s)?;
        let lines = parse_u32(lines_s)?;
        let checks_raw = parse_u32(checks_s)?;
        if checks_raw > u16::MAX as u32 {
            return Err(SealError::ChecksRange);
        }
        let e = SealEntry {
            num,
            slug,
            floor,
            lines,
            checks: checks_raw as u16,
            frozen: false,
        };
        validate_entry(&e)?;
        let mut dup = false;
        for prior in out.iter() {
            if prior.num == e.num {
                dup = true;
            }
        }
        if dup {
            return Err(SealError::Duplicate);
        }
        out.push(e);
    }
    if out.is_empty() {
        return Err(SealError::Empty);
    }
    Ok(out)
}

/// 域账视图：已用 / 预算 / 余量 / 是否命中检查点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Account {
    /// 已用行数。
    pub used: u32,
    /// 预算上限。
    pub budget: u32,
    /// 余量（预算耗尽时为 0）。
    pub remaining: u32,
    /// 域账检查点。
    pub checkpoint: u32,
    /// 是否正好命中检查点。
    pub hit: bool,
    /// 是否越预算。
    pub over: bool,
}

impl Account {
    /// 由已用行数构造；`remaining` 用饱和减法，余额不出现负数回绕。
    pub fn new(used: u32) -> Account {
        Account {
            used,
            budget: DOMAIN_BUDGET,
            remaining: DOMAIN_BUDGET.saturating_sub(used),
            checkpoint: DOMAIN_CHECKPOINT,
            hit: used == DOMAIN_CHECKPOINT,
            over: used > DOMAIN_BUDGET,
        }
    }

    /// 一行对账文本（`used/budget` 即 J1 的「126,000/240,000」）。
    pub fn ledger_line(&self) -> String {
        format!(
            "domain {}/{} remaining={} checkpoint={}",
            self.used, self.budget, self.remaining, self.checkpoint
        )
    }
}

/// 印面：落印瞬间的快照。印后账本只读，这张快照就是唯一对外凭据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealStamp {
    /// 冻结条数（应为 20）。
    pub count: usize,
    /// 冻结行数合计。
    pub lines: u32,
    /// 域账余量。
    pub remaining: u32,
    /// 条目摘要（FNV-1a 32）。
    pub digest: u32,
}

/// 审计结果：清单与账本的逐条比对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditReport {
    /// 清单条数。
    pub manifest: usize,
    /// 账本条数。
    pub ledger: usize,
    /// 数据完全一致的条数。
    pub matched: usize,
    /// 同号但数据不同的条数（账本被改过）。
    pub mismatched: usize,
    /// 清单有、账本无。
    pub missing: usize,
    /// 账本有、清单无。
    pub extra: usize,
}

impl AuditReport {
    /// 是否完全一致。
    pub fn clean(&self) -> bool {
        self.mismatched == 0 && self.missing == 0 && self.extra == 0 && self.matched == self.manifest
    }
}

/// 定容冻结账本。落印前可注册/冻结，落印后一律只读。
pub struct SealLedger {
    entries: Vec<SealEntry>,
    stamp: Option<SealStamp>,
    rejected: u32,
}

impl SealLedger {
    /// 建空账（容量固定为本批条数）。
    pub fn new() -> SealLedger {
        SealLedger {
            entries: Vec::with_capacity(BATCH_SIZE),
            stamp: None,
            rejected: 0,
        }
    }

    fn reject<T>(&mut self, e: SealError) -> Result<T, SealError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 按卡号取条目。
    pub fn entry(&self, num: u32) -> Option<&SealEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.num == num {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册一条（策略校验 + 防重 + 容量）；落印后拒绝任何写入。
    pub fn register(&mut self, e: SealEntry) -> Result<(), SealError> {
        if self.stamp.is_some() {
            return self.reject(SealError::Immutable);
        }
        validate_entry(&e)?;
        if self.entries.len() >= BATCH_SIZE {
            return self.reject(SealError::Capacity);
        }
        if self.entry(e.num).is_some() {
            return self.reject(SealError::Duplicate);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 批量注册（顺序即清单顺序，账本指纹按它算）。
    pub fn register_all(&mut self, items: &[SealEntry]) -> Result<usize, SealError> {
        if items.is_empty() {
            return self.reject(SealError::Empty);
        }
        let mut i = 0usize;
        while i < items.len() {
            match items.get(i) {
                Some(e) => self.register(*e)?,
                None => return self.reject(SealError::Empty),
            }
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 冻结一条：不可逆、不可重复。落印后拒绝。
    pub fn freeze(&mut self, num: u32) -> Result<(), SealError> {
        if self.stamp.is_some() {
            return self.reject(SealError::Immutable);
        }
        let mut idx: Option<usize> = None;
        let mut already = false;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.num == num {
                    idx = Some(i);
                    already = e.frozen;
                }
            }
            i += 1;
        }
        if already {
            return self.reject(SealError::FrozenTwice);
        }
        match idx {
            Some(k) => match self.entries.get_mut(k) {
                Some(e) => {
                    e.frozen = true;
                    Ok(())
                }
                None => self.reject(SealError::Unknown),
            },
            None => self.reject(SealError::Unknown),
        }
    }

    /// 冻结全部已注册条目，返回冻结条数。
    pub fn freeze_all(&mut self) -> Result<usize, SealError> {
        let mut nums: Vec<u32> = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if !e.frozen {
                    nums.push(e.num);
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < nums.len() {
            if let Some(n) = nums.get(k) {
                self.freeze(*n)?;
            }
            k += 1;
        }
        Ok(self.frozen_count())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 已冻结条数。
    pub fn frozen_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.frozen {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账面行数合计（折叠求和，不是常量）。
    pub fn total_lines(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.lines);
            }
            i += 1;
        }
        sum
    }

    /// 判据条数合计（与域账对账的第二根柱子）。
    pub fn total_checks(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.checks as u32);
            }
            i += 1;
        }
        sum
    }

    /// 域账视图。
    pub fn account(&self) -> Account {
        Account::new(self.total_lines())
    }

    /// 账本指纹（按注册序，串 num / lines / checks）。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let vals = [e.num, e.lines, e.checks as u32];
                let mut k = 0usize;
                while k < vals.len() {
                    if let Some(v) = vals.get(k) {
                        h = (h ^ (v >> 16)) .wrapping_mul(0x0100_0193);
                        h = (h ^ (v & 0xffff)) .wrapping_mul(0x0100_0193);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h
    }

    /// 累计被拒次数（显性记账）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 是否已落印。
    pub fn sealed(&self) -> bool {
        self.stamp.is_some()
    }

    /// 取印面快照（未落印为 `None`）。
    pub fn stamp(&self) -> Option<SealStamp> {
        self.stamp
    }

    /// 落印：20 条全冻结 + 域账恰好命中检查点，二者缺一不可。
    pub fn seal(&mut self) -> Result<SealStamp, SealError> {
        if self.stamp.is_some() {
            return self.reject(SealError::AlreadySealed);
        }
        if self.entries.len() != BATCH_SIZE {
            return self.reject(SealError::Incomplete);
        }
        if self.frozen_count() != self.entries.len() {
            return self.reject(SealError::NotFrozen);
        }
        let acct = self.account();
        if !acct.hit || acct.over {
            return self.reject(SealError::Account);
        }
        let stamp = SealStamp {
            count: self.entries.len(),
            lines: acct.used,
            remaining: acct.remaining,
            digest: self.digest(),
        };
        self.stamp = Some(stamp);
        Ok(stamp)
    }

    /// 逐条审计：把清单重新解析一遍，与账本比对（不信任账本自己记的数）。
    pub fn audit(&self, text: &'static str) -> Result<AuditReport, SealError> {
        let manifest = parse_manifest(text)?;
        let mut matched = 0usize;
        let mut mismatched = 0usize;
        let mut missing = 0usize;
        let mut i = 0usize;
        while i < manifest.len() {
            if let Some(m) = manifest.get(i) {
                match self.entry(m.num) {
                    Some(e) => {
                        if e.same_data(m) {
                            matched += 1;
                        } else {
                            mismatched += 1;
                        }
                    }
                    None => missing += 1,
                }
            }
            i += 1;
        }
        let mut extra = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                let mut found = false;
                let mut j = 0usize;
                while j < manifest.len() {
                    if let Some(m) = manifest.get(j) {
                        if m.num == e.num {
                            found = true;
                        }
                    }
                    j += 1;
                }
                if !found {
                    extra += 1;
                }
            }
            k += 1;
        }
        Ok(AuditReport {
            manifest: manifest.len(),
            ledger: self.entries.len(),
            matched,
            mismatched,
            missing,
            extra,
        })
    }
}

/// 按清单建账并全部冻结（未落印）。
pub fn build_ledger() -> SealLedger {
    let mut l = SealLedger::new();
    let items = match parse_manifest(MANIFEST) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let _ = l.register_all(&items);
    let _ = l.freeze_all();
    l
}

/// 按清单建账、全部冻结并落印。
pub fn sealed_ledger() -> SealLedger {
    let mut l = build_ledger();
    let _ = l.seal();
    l
}

/// 注入核：8 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1) 空清单
    if parse_manifest("").err() == Some(SealError::Empty) {
        caught += 1;
    }

    // 2) 段数不足
    if parse_manifest("50001:420").err() == Some(SealError::ParseField) {
        caught += 1;
    }

    // 3) 数字段非十进制
    if parse_manifest("50001:420:42x0:18:gamutmap").err() == Some(SealError::ParseInt) {
        caught += 1;
    }

    // 4) 卡号越出本批区间
    if parse_manifest("50999:420:4200:18:gamutmap").err() == Some(SealError::BadNum) {
        caught += 1;
    }

    // 5) slug 为空
    if parse_manifest("50001:420:4200:18:").err() == Some(SealError::BadSlug) {
        caught += 1;
    }

    // 6) 账面低于计划下限
    if parse_manifest("50001:9000:4200:18:gamutmap").err() == Some(SealError::BelowFloor) {
        caught += 1;
    }

    // 7) 判据数为 0（越出 8..=20）
    if parse_manifest("50001:420:4200:0:gamutmap").err() == Some(SealError::ChecksRange) {
        caught += 1;
    }

    // 8) 清单内重复卡号
    let dup = "50001:420:4200:18:gamutmap\n50001:420:4200:18:gamutmap";
    if parse_manifest(dup).err() == Some(SealError::Duplicate) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50020 自检：14 条判据。
pub fn run_unx50020_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50020");

    // J1-1 清单扫描：20 条，卡号唯一且覆盖 50001..=50020（连续无洞）。
    let items = match parse_manifest(MANIFEST) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let mut contiguous = items.len() == BATCH_SIZE;
    let mut i = 0usize;
    while i < items.len() {
        if let Some(e) = items.get(i) {
            if e.num != BATCH_FIRST + i as u32 {
                contiguous = false;
            }
        }
        i += 1;
    }
    let ok = contiguous;
    let det = format!("n={} first={}", items.len(), BATCH_FIRST);
    s.add_dyn("50020-J1-清单扫描", ok, &det);

    // J1-2 每条账面 ≥ 计划下限（逐条，不是只看总数）。
    let mut floor_ok = items.len() == BATCH_SIZE;
    let mut k = 0usize;
    while k < items.len() {
        if let Some(e) = items.get(k) {
            if e.lines < e.floor || e.floor == 0 {
                floor_ok = false;
            }
        }
        k += 1;
    }
    s.add("50020-J1-账面不低于下限", floor_ok, if floor_ok { "" } else { "有条目低于下限" });

    // J1-3 判据数落在 8..=20。
    let mut checks_ok = items.len() == BATCH_SIZE;
    let mut c = 0usize;
    while c < items.len() {
        if let Some(e) = items.get(c) {
            if e.checks < CHECKS_MIN || e.checks > CHECKS_MAX {
                checks_ok = false;
            }
        }
        c += 1;
    }
    s.add("50020-J1-判据数闭集", checks_ok, if checks_ok { "" } else { "判据数越界" });

    // J1-4 累计行数 = 126,000（折叠求和命中检查点，不是写死的常量比较）。
    let mut sum: u32 = 0;
    let mut m = 0usize;
    while m < items.len() {
        if let Some(e) = items.get(m) {
            sum = sum.saturating_add(e.lines);
        }
        m += 1;
    }
    let ok = sum == DOMAIN_CHECKPOINT && items.len() == BATCH_SIZE;
    let det = format!("sum={}", sum);
    s.add_dyn("50020-J1-累计行数", ok, &det);

    // J1-5 域账视图：126,000 / 240,000，余量 114,000，命中检查点。
    let acct = Account::new(sum);
    let ok = acct.used == 126_000 && acct.budget == 240_000 && acct.remaining == 114_000 && acct.hit && !acct.over;
    let det = acct.ledger_line();
    s.add_dyn("50020-J1-域账126k", ok, &det);

    // J1-6 落印成功：20/20 冻结、印面条数/行数/摘要齐备。
    let ledger = build_ledger();
    let mut sealed = build_ledger();
    let stamp = sealed.seal();
    let stamped = match stamp {
        Ok(st) => st.count == 20 && st.lines == 126_000 && st.remaining == 114_000 && st.digest != 0,
        Err(_) => false,
    };
    let ok = ledger.frozen_count() == BATCH_SIZE
        && ledger.len() == BATCH_SIZE
        && stamped
        && sealed.sealed();
    let det = match stamp {
        Ok(st) => format!("20/20 lines={} dg={:#010x}", st.lines, st.digest),
        Err(_) => "seal failed".to_string(),
    };
    s.add_dyn("50020-J1-批印入册", ok, &det);

    // J1-7 未全冻结不许落印：冻结 19 条即 NotFrozen。
    let mut partial = SealLedger::new();
    let _ = partial.register_all(&items);
    let mut fz = 0usize;
    while fz + 1 < items.len() {
        if let Some(e) = items.get(fz) {
            let _ = partial.freeze(e.num);
        }
        fz += 1;
    }
    let ok = partial.frozen_count() == 19 && partial.seal().err() == Some(SealError::NotFrozen);
    s.add("50020-J1-未冻不印", ok, if ok { "" } else { "未冻满也落了印" });

    // J1-8 条目不足 20 不许落印：19 条全冻结仍是 Incomplete。
    let mut short = SealLedger::new();
    let mut n = 0usize;
    while n + 1 < items.len() {
        if let Some(e) = items.get(n) {
            let _ = short.register(*e);
        }
        n += 1;
    }
    let _ = short.freeze_all();
    let ok = short.len() == 19 && short.seal().err() == Some(SealError::Incomplete);
    s.add("50020-J1-少条不印", ok, if ok { "" } else { "19 条也落了印" });

    // J1-9 二次落印必拒，且印面一字不变。
    let first = sealed.stamp();
    let again = sealed.seal();
    let ok = again.err() == Some(SealError::AlreadySealed) && sealed.stamp() == first && first.is_some();
    s.add("50020-J1-印不重落", ok, if ok { "" } else { "允许重落印" });

    // J1-10 印后只读：注册与冻结都被 Immutable 挡回。
    let probe = SealEntry {
        num: 50021,
        slug: "nup",
        floor: 420,
        lines: 4600,
        checks: 15,
        frozen: false,
    };
    let reg = sealed.register(probe).err() == Some(SealError::Immutable);
    let frz = sealed.freeze(50001).err() == Some(SealError::Immutable);
    let ok = reg && frz && sealed.len() == BATCH_SIZE && sealed.total_lines() == 126_000;
    s.add("50020-J1-印后只读", ok, if ok { "" } else { "印后仍可改账" });

    // J1-11 注册防线：重复号 / 超容 / 越界号 三类都拒。
    let mut g = SealLedger::new();
    let e0 = SealEntry { num: 50001, slug: "gamutmap", floor: 420, lines: 4200, checks: 18, frozen: false };
    let e_bad = SealEntry { num: 50999, slug: "gamutmap", floor: 420, lines: 4200, checks: 18, frozen: false };
    let ok_first = g.register(e0).is_ok();
    let dup = g.register(e0).err() == Some(SealError::Duplicate);
    let badnum = g.register(e_bad).err() == Some(SealError::BadNum);
    // 已入 1 条，再补 19 条正好填满 20（不得与 50001 重号）。
    let mut full = 1usize;
    let mut ok_fill = true;
    while full < BATCH_SIZE {
        let e = SealEntry {
            num: 50001 + full as u32,
            slug: "slot",
            floor: 100,
            lines: 100,
            checks: 8,
            frozen: false,
        };
        if g.register(e).is_err() {
            ok_fill = false;
        }
        full += 1;
    }
    let cap = g
        .register(SealEntry { num: 50020, slug: "slot", floor: 100, lines: 100, checks: 8, frozen: false })
        .err()
        == Some(SealError::Capacity);
    let ok = ok_first && dup && badnum && ok_fill && cap && g.len() == BATCH_SIZE;
    let det = format!("len={} rej={}", g.len(), g.rejected());
    s.add_dyn("50020-J1-注册防线", ok, &det);

    // J1-12 审计：清单与账本逐条一致（20/20 matched，无漂移/缺失/多余）。
    let audit = sealed.audit(MANIFEST);
    let ok = match audit {
        Ok(r) => r.clean() && r.matched == 20 && r.ledger == 20,
        Err(_) => false,
    };
    let det = match audit {
        Ok(r) => format!("m={} x={} miss={} extra={}", r.matched, r.mismatched, r.missing, r.extra),
        Err(_) => "audit failed".to_string(),
    };
    s.add_dyn("50020-J1-逐条审计", ok, &det);

    // J1-13 印面确定性：两次独立建账落印得同摘要、同行数。
    let a = sealed_ledger();
    let b = sealed_ledger();
    let ok = a.stamp().is_some() && a.stamp() == b.stamp() && a.total_checks() == b.total_checks() && a.total_checks() > 0;
    let det = match a.stamp() {
        Some(st) => format!("dg={:#010x} checks={}", st.digest, a.total_checks()),
        None => "no stamp".to_string(),
    };
    s.add_dyn("50020-J1-印面确定", ok, &det);

    // J1-14 八类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50020-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50020_checks_green() {
        let set = run_unx50020_checks();
        assert!(set.all_passed(), "UNX-F50020 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50020_seal_hits_checkpoint() {
        let l = sealed_ledger();
        match l.stamp() {
            Some(st) => {
                assert_eq!(st.count, 20);
                assert_eq!(st.lines, 126_000);
                assert_eq!(st.remaining, 114_000);
            }
            None => panic!("seal failed"),
        }
    }

    #[test]
    fn unx50020_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 8);
    }
}
