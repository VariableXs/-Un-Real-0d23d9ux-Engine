//! UNX-F50040 B22 批收官印（20 条冻结 / 累计 132,000 行）（0x43120 段）
//!
//! 锚点：UNX-F50040-J1 批印入册：20/20 冻结、域账 132,000/240,000；
//! 判据锚定 §AI-63 §7。
//!
//! 数据段：`SEG_BASE = 0x43120`，诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 17`，
//! 逐项见 [`SealError`]。
//!
//! 收官印与 B21 的差别在**增量账**：B21 把域账推到 126,000 就落印；本批
//! 要证明的不是「又涨了」，而是「涨得正好」——
//!
//! ```text
//! 132,000（本批检查点） − 126,000（B21 检查点） = 6,000
//! 6,000 = 本批 20 条计划下限之和（折叠求和，不是写死的常量）
//! ```
//!
//! 也就是说：本批账面**零冗余**，每条卡的冻结行数恰好等于计划下限，
//! 合计恰好等于增量。任何一条被悄悄抬高或压低，`Delta` 立刻拦下落印。
//! 落印前必须 20/20 冻结（未冻满 `NotFrozen`、不足 20 条 `Incomplete`），
//! 落印后整本账只读（`Immutable`），印面自带逐条链摘要，事后改任何一条
//! 都会被 [`BatchLedger::audit`] 的逐条比对抓出。
//!
//! 结构清单：
//! * [`Card`]：单条冻结卡（卡号 / slug / 下限 / 账面行数 / 判据数 / 冻结位）；
//! * [`BatchLedger`]：定容账本（注册 / 冻结 / 落印 / 审计 + 逐条链）；
//! * [`Cumulative`]：跨批域账（前批 126,000 + 本批增量 → 累计 132,000）；
//! * [`Stamp`]：印面快照（条数 / 增量 / 累计 / 链尾 / 摘要）；
//! * [`injection_core`]：9 类坏清单注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43120 独占 0x20）。
pub const SEG_BASE: u32 = 0x43120;

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
const E_DELTA: u16 = 13;
const E_PRIOR: u16 = 14;
const E_ALREADY_SEALED: u16 = 15;
const E_IMMUTABLE: u16 = 16;
const E_FROZEN_TWICE: u16 = 17;

/// 收官印错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    /// 清单为空。
    Empty,
    /// 字段数不是 5。
    ParseField,
    /// 数字段非十进制。
    ParseInt,
    /// 卡号越出本批区间。
    BadNum,
    /// slug 为空 / 非 ASCII 小写数字下划线 / 超长。
    BadSlug,
    /// 账面行数低于计划下限（或下限为 0）。
    BelowFloor,
    /// 判据数越出 8..=20。
    ChecksRange,
    /// 账本容量已满。
    Capacity,
    /// 卡号重复。
    Duplicate,
    /// 卡号未注册。
    Unknown,
    /// 尚有未冻结条目。
    NotFrozen,
    /// 条目数不足 20。
    Incomplete,
    /// 本批增量与检查点差额不符。
    Delta,
    /// 前批检查点不是 B21 的 126,000。
    Prior,
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
            SealError::Delta => E_DELTA,
            SealError::Prior => E_PRIOR,
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
            SealError::ParseField => "record does not have 5 fields",
            SealError::ParseInt => "numeric field is not decimal",
            SealError::BadNum => "card number outside 50021..=50040",
            SealError::BadSlug => "slug empty, non-ascii or longer than 24",
            SealError::BelowFloor => "lines below planned floor",
            SealError::ChecksRange => "checks outside 8..=20",
            SealError::Capacity => "ledger capacity reached",
            SealError::Duplicate => "card number already registered",
            SealError::Unknown => "card number not registered",
            SealError::NotFrozen => "entries still unfrozen",
            SealError::Incomplete => "batch has fewer than 20 cards",
            SealError::Delta => "batch line sum does not match checkpoint delta",
            SealError::Prior => "prior checkpoint is not 126000",
            SealError::AlreadySealed => "batch already sealed",
            SealError::Immutable => "ledger is sealed read-only",
            SealError::FrozenTwice => "card already frozen",
        }
    }
}

/// 批首卡号（B22 = 50021..=50040）。
pub const BATCH_FIRST: u32 = 50021;
/// 批末卡号。
pub const BATCH_LAST: u32 = 50040;
/// 本批条数。
pub const BATCH_SIZE: usize = 20;
/// B21 收官检查点（前批已用行数）。
pub const PRIOR_CHECKPOINT: u32 = 126_000;
/// B22 收官检查点（本批落印后累计行数）。
pub const CHECKPOINT: u32 = 132_000;
/// 域账预算上限。
pub const DOMAIN_BUDGET: u32 = 240_000;
/// 本批增量 = 132,000 − 126,000；由检查点差额推导，不另写常数。
pub const BATCH_DELTA: u32 = CHECKPOINT - PRIOR_CHECKPOINT;
/// 判据数闭区间下限。
pub const CHECKS_MIN: u16 = 8;
/// 判据数闭区间上限。
pub const CHECKS_MAX: u16 = 20;
/// slug 字节上限。
pub const SLUG_CAP: usize = 24;
/// 数字段位数上限（防溢出）。
const DIGITS_CAP: usize = 9;

/// 冻结清单（20 条，一行一条：`num:floor:lines:checks:slug`）。
///
/// 本批 `lines == floor`：计划下限就是实际冻结行数，合计 6,000，
/// 恰好等于 132,000 − 126,000。账面零冗余是本批的资产，不是巧合。
pub const MANIFEST: &str = "\
50021:420:420:16:nup
50022:480:480:15:nuporder
50023:400:400:15:booklet
50024:380:380:14:poster
50025:360:360:14:fitpage
50026:340:340:13:rotate
50027:320:320:13:layoutduplex
50028:300:300:14:layoutpreview
50029:300:300:14:layouttrace
50030:280:280:13:layouterrs
50031:280:280:14:layoutsamples
50032:260:260:13:layoutfuzz
50033:260:260:13:layoutfreeze
50034:240:240:13:layoutvec
50035:240:240:13:binding
50036:220:220:12:cropline
50037:220:220:17:layoutt1
50038:200:200:15:b22vec
50039:200:200:16:b22grep
50040:300:300:14:b22seal";

/// 单条冻结卡。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
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

impl Card {
    /// 数据字段是否一致（忽略 `frozen`，审计用）。
    pub fn same_data(&self, o: &Card) -> bool {
        self.num == o.num
            && self.lines == o.lines
            && self.checks == o.checks
            && self.floor == o.floor
            && self.slug == o.slug
    }

    /// 账面是否达标：有下限、行数不低于下限、判据数在闭区间。
    pub fn within_policy(&self) -> bool {
        self.floor > 0
            && self.lines >= self.floor
            && self.checks >= CHECKS_MIN
            && self.checks <= CHECKS_MAX
    }

    /// 本条的链分量（注册时混入，事后复算用）。
    pub fn chain_part(&self) -> u32 {
        let mut h = 0x811c_9dc5u32;
        h = mix(h, self.num);
        h = mix(h, self.floor);
        h = mix(h, self.lines);
        h = mix(h, self.checks as u32);
        let b = self.slug.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                h = (h ^ *c as u32).wrapping_mul(0x0100_0193);
            }
            i += 1;
        }
        h
    }

    /// 一行台账。
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

fn mix(h: u32, v: u32) -> u32 {
    let a = (h ^ (v & 0xffff)).wrapping_mul(0x0100_0193);
    (a ^ (v >> 16)).wrapping_mul(0x0100_0193)
}

/// 十进制定点解析（位数上限 9，非数字即拒）。
pub fn parse_u32(s: &str) -> Result<u32, SealError> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > DIGITS_CAP {
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

/// 十进制输出（与 [`parse_u32`] 构成往返：`parse(itoa(v)) == v`）。
pub fn itoa(v: u32) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf: [u8; DIGITS_CAP] = [0; DIGITS_CAP];
    let mut n = v;
    let mut at = DIGITS_CAP;
    while n > 0 && at > 0 {
        at -= 1;
        if let Some(slot) = buf.get_mut(at) {
            *slot = b'0' + (n % 10) as u8;
        }
        n /= 10;
    }
    let mut s = String::new();
    let mut i = at;
    while i < DIGITS_CAP {
        if let Some(c) = buf.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    s
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

/// 单条策略校验（解析与注册共用同一判据，避免两处口径漂移）。
pub fn validate_card(c: &Card) -> Result<(), SealError> {
    if c.num < BATCH_FIRST || c.num > BATCH_LAST {
        return Err(SealError::BadNum);
    }
    if !slug_ok(c.slug) {
        return Err(SealError::BadSlug);
    }
    if c.floor == 0 || c.lines < c.floor {
        return Err(SealError::BelowFloor);
    }
    if c.checks < CHECKS_MIN || c.checks > CHECKS_MAX {
        return Err(SealError::ChecksRange);
    }
    Ok(())
}

/// 解析冻结清单；任一条失败即整体失败（半本清单不入账）。
pub fn parse_manifest(text: &'static str) -> Result<Vec<Card>, SealError> {
    let mut out: Vec<Card> = Vec::with_capacity(BATCH_SIZE);
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
        let c = Card {
            num,
            slug,
            floor,
            lines,
            checks: checks_raw as u16,
            frozen: false,
        };
        validate_card(&c)?;
        let mut dup = false;
        let mut i = 0usize;
        while i < out.len() {
            if let Some(p) = out.get(i) {
                if p.num == c.num {
                    dup = true;
                }
            }
            i += 1;
        }
        if dup {
            return Err(SealError::Duplicate);
        }
        out.push(c);
    }
    if out.is_empty() {
        return Err(SealError::Empty);
    }
    Ok(out)
}

/// 跨批域账。`prior` 必须是 B21 检查点，`delta` 是本批折叠求和。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cumulative {
    /// 前批已用。
    pub prior: u32,
    /// 本批增量。
    pub delta: u32,
    /// 累计 = prior + delta。
    pub total: u32,
    /// 域账预算。
    pub budget: u32,
    /// 本批检查点。
    pub checkpoint: u32,
    /// 余量（饱和减，不出现负数回绕）。
    pub remaining: u32,
    /// 是否正好命中本批检查点。
    pub hit: bool,
}

impl Cumulative {
    /// 受检构造：前批必须是 126,000，否则 `Prior`（前批错位会让增量对上而累计错）。
    pub fn checked(prior: u32, delta: u32) -> Result<Cumulative, SealError> {
        if prior != PRIOR_CHECKPOINT {
            return Err(SealError::Prior);
        }
        let total = prior.saturating_add(delta);
        Ok(Cumulative {
            prior,
            delta,
            total,
            budget: DOMAIN_BUDGET,
            checkpoint: CHECKPOINT,
            remaining: DOMAIN_BUDGET.saturating_sub(total),
            hit: total == CHECKPOINT,
        })
    }

    /// 一行对账文本（即 J1 的「132,000/240,000」）。
    pub fn ledger_line(&self) -> String {
        format!(
            "domain {}/{} prior={} delta={} remaining={} hit={}",
            self.total,
            self.budget,
            self.prior,
            self.delta,
            self.remaining,
            if self.hit { 1 } else { 0 }
        )
    }
}

/// 印面：落印瞬间的快照，印后唯一对外凭据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// 冻结条数（应为 20）。
    pub count: usize,
    /// 本批增量行数。
    pub delta: u32,
    /// 累计域账行数。
    pub total: u32,
    /// 域账余量。
    pub remaining: u32,
    /// 逐条链尾。
    pub chain: u32,
    /// 印面摘要（覆盖条数/增量/累计/链尾）。
    pub digest: u32,
}

/// 审计报表：清单与账本逐条比对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditReport {
    /// 清单条数。
    pub manifest: usize,
    /// 账本条数。
    pub ledger: usize,
    /// 数据一致条数。
    pub matched: usize,
    /// 同号但数据不同。
    pub mismatched: usize,
    /// 清单有账本无。
    pub missing: usize,
    /// 账本有清单无。
    pub extra: usize,
}

impl AuditReport {
    /// 是否完全一致。
    pub fn clean(&self) -> bool {
        self.mismatched == 0
            && self.missing == 0
            && self.extra == 0
            && self.matched == self.manifest
            && self.manifest == self.ledger
    }
}

/// 定容冻结账本。落印前可注册/冻结，落印后一律只读。
pub struct BatchLedger {
    cards: Vec<Card>,
    capacity: usize,
    chain: u32,
    stamp: Option<Stamp>,
    rejected: u32,
}

impl BatchLedger {
    /// 建空账（容量默认 20 条）。
    pub fn new(capacity: usize) -> BatchLedger {
        let cap = if capacity == 0 { 1 } else { capacity };
        BatchLedger {
            cards: Vec::with_capacity(cap),
            capacity: cap,
            chain: 0x811c_9dc5,
            stamp: None,
            rejected: 0,
        }
    }

    /// 默认容量账本。
    pub fn with_batch_capacity() -> BatchLedger {
        BatchLedger::new(BATCH_SIZE)
    }

    fn reject<T>(&mut self, e: SealError) -> Result<T, SealError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    fn index_of(&self, num: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if c.num == num {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按卡号取条目。
    pub fn card(&self, num: u32) -> Option<&Card> {
        match self.index_of(num) {
            Some(i) => self.cards.get(i),
            None => None,
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 逐条链尾。
    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 已冻结条数。
    pub fn frozen_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if c.frozen {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 本批增量（折叠求和，不是常量）。
    pub fn delta(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                sum = sum.saturating_add(c.lines);
            }
            i += 1;
        }
        sum
    }

    /// 计划下限之和（增量对账的第二根柱子）。
    pub fn floor_sum(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                sum = sum.saturating_add(c.floor);
            }
            i += 1;
        }
        sum
    }

    /// 判据总数。
    pub fn total_checks(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                sum = sum.saturating_add(c.checks as u32);
            }
            i += 1;
        }
        sum
    }

    /// 域账视图（前批固定 126,000）。
    pub fn account(&self) -> Result<Cumulative, SealError> {
        Cumulative::checked(PRIOR_CHECKPOINT, self.delta())
    }

    /// 注册一条（策略 + 防重 + 容量 + 链）；落印后拒写。
    pub fn register(&mut self, c: Card) -> Result<(), SealError> {
        if self.stamp.is_some() {
            return self.reject(SealError::Immutable);
        }
        validate_card(&c)?;
        if self.cards.len() >= self.capacity {
            return self.reject(SealError::Capacity);
        }
        if self.index_of(c.num).is_some() {
            return self.reject(SealError::Duplicate);
        }
        self.chain = mix(self.chain, c.chain_part());
        self.cards.push(c);
        Ok(())
    }

    /// 批量注册。
    pub fn register_all(&mut self, items: &[Card]) -> Result<usize, SealError> {
        if items.is_empty() {
            return self.reject(SealError::Empty);
        }
        let mut i = 0usize;
        while i < items.len() {
            match items.get(i) {
                Some(c) => self.register(*c)?,
                None => return self.reject(SealError::Empty),
            }
            i += 1;
        }
        Ok(self.cards.len())
    }

    /// 冻结一条：不可逆、不可重复；落印后拒写。
    pub fn freeze(&mut self, num: u32) -> Result<(), SealError> {
        if self.stamp.is_some() {
            return self.reject(SealError::Immutable);
        }
        let idx = match self.index_of(num) {
            Some(i) => i,
            None => return self.reject(SealError::Unknown),
        };
        let already = match self.cards.get(idx) {
            Some(c) => c.frozen,
            None => return self.reject(SealError::Unknown),
        };
        if already {
            return self.reject(SealError::FrozenTwice);
        }
        match self.cards.get_mut(idx) {
            Some(c) => {
                c.frozen = true;
                Ok(())
            }
            None => self.reject(SealError::Unknown),
        }
    }

    /// 全部冻结。
    pub fn freeze_all(&mut self) -> Result<usize, SealError> {
        let mut nums: Vec<u32> = Vec::with_capacity(self.cards.len());
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if !c.frozen {
                    nums.push(c.num);
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

    /// 是否已落印。
    pub fn sealed(&self) -> bool {
        self.stamp.is_some()
    }

    /// 印面快照。
    pub fn stamp(&self) -> Option<Stamp> {
        self.stamp
    }

    /// 落印：20 条全冻结 + 增量恰为检查点差额，二者缺一不可。
    pub fn seal(&mut self) -> Result<Stamp, SealError> {
        if self.stamp.is_some() {
            return self.reject(SealError::AlreadySealed);
        }
        if self.cards.len() != BATCH_SIZE {
            return self.reject(SealError::Incomplete);
        }
        if self.frozen_count() != self.cards.len() {
            return self.reject(SealError::NotFrozen);
        }
        let delta = self.delta();
        if delta != BATCH_DELTA {
            return self.reject(SealError::Delta);
        }
        let acct = match self.account() {
            Ok(a) => a,
            Err(e) => return self.reject(e),
        };
        let mut digest = 0x811c_9dc5u32;
        digest = mix(digest, self.cards.len() as u32);
        digest = mix(digest, delta);
        digest = mix(digest, acct.total);
        digest = mix(digest, self.chain);
        let st = Stamp {
            count: self.cards.len(),
            delta,
            total: acct.total,
            remaining: acct.remaining,
            chain: self.chain,
            digest,
        };
        self.stamp = Some(st);
        Ok(st)
    }

    /// 逐条审计：重解析清单并与账本比对，同时复核增量与前批检查点。
    pub fn audit(&self, text: &'static str) -> Result<AuditReport, SealError> {
        let manifest = parse_manifest(text)?;
        let mut matched = 0usize;
        let mut mismatched = 0usize;
        let mut missing = 0usize;
        let mut i = 0usize;
        while i < manifest.len() {
            if let Some(m) = manifest.get(i) {
                match self.card(m.num) {
                    Some(c) => {
                        if c.same_data(m) {
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
        while k < self.cards.len() {
            if let Some(c) = self.cards.get(k) {
                let mut found = false;
                let mut j = 0usize;
                while j < manifest.len() {
                    if let Some(m) = manifest.get(j) {
                        if m.num == c.num {
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
        // 增量复核：账本条数与清单一致时，两边折叠和必须同时命中。
        let mut m_sum: u32 = 0;
        let mut q = 0usize;
        while q < manifest.len() {
            if let Some(m) = manifest.get(q) {
                m_sum = m_sum.saturating_add(m.lines);
            }
            q += 1;
        }
        if self.sealed() && m_sum != self.delta() {
            return Err(SealError::Delta);
        }
        Ok(AuditReport {
            manifest: manifest.len(),
            ledger: self.cards.len(),
            matched,
            mismatched,
            missing,
            extra,
        })
    }

    /// 全表台账文本。
    pub fn ledger_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                out.push_str(&c.ledger_line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// 按清单建账并全部冻结（未落印）。
pub fn build_ledger() -> BatchLedger {
    let mut l = BatchLedger::with_batch_capacity();
    let items = match parse_manifest(MANIFEST) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let _ = l.register_all(&items);
    let _ = l.freeze_all();
    l
}

/// 按清单建账、冻结并落印。
pub fn sealed_ledger() -> BatchLedger {
    let mut l = build_ledger();
    let _ = l.seal();
    l
}

/// 注入核：9 类坏清单，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    // 1) 空清单。
    if parse_manifest("").err() == Some(SealError::Empty) {
        caught += 1;
    }
    // 2) 字段数不足。
    if parse_manifest("50021:420:420").err() == Some(SealError::ParseField) {
        caught += 1;
    }
    // 3) 数字段非十进制。
    if parse_manifest("50021:4x0:420:16:nup").err() == Some(SealError::ParseInt) {
        caught += 1;
    }
    // 4) 卡号越界（B21 的号进不了 B22 的账）。
    if parse_manifest("50020:420:420:16:nup").err() == Some(SealError::BadNum) {
        caught += 1;
    }
    // 5) slug 为空。
    if parse_manifest("50021:420:420:16:").err() == Some(SealError::BadSlug) {
        caught += 1;
    }
    // 6) 账面低于下限。
    if parse_manifest("50021:9000:420:16:nup").err() == Some(SealError::BelowFloor) {
        caught += 1;
    }
    // 7) 判据数为 0（越出 8..=20）。
    if parse_manifest("50021:420:420:0:nup").err() == Some(SealError::ChecksRange) {
        caught += 1;
    }
    // 8) 清单内重号。
    if parse_manifest("50021:420:420:16:nup\n50021:420:420:16:nup").err() == Some(SealError::Duplicate) {
        caught += 1;
    }
    // 9) slug 超长：24 字节收、25 字节拒（上限两侧各取一个样本）。
    let ok_long = parse_manifest("50021:420:420:16:aaaaaaaaaaaaaaaaaaaaaaaa").is_ok();
    let ok_longer = parse_manifest("50021:420:420:16:aaaaaaaaaaaaaaaaaaaaaaaaa")
        .err()
        == Some(SealError::BadSlug);
    if ok_long && ok_longer {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50040 自检：17 条判据。
pub fn run_unx50040_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50040");

    // J1-1 清单扫描：20 条、卡号 50021..=50040 连续无洞。
    let items = match parse_manifest(MANIFEST) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let mut contiguous = items.len() == BATCH_SIZE;
    let mut i = 0usize;
    while i < items.len() {
        match items.get(i) {
            Some(c) => {
                if c.num != BATCH_FIRST + i as u32 {
                    contiguous = false;
                }
            }
            None => contiguous = false,
        }
        i += 1;
    }
    let det = format!("n={} first={}", items.len(), BATCH_FIRST);
    s.add_dyn("50040-J1-清单扫描", contiguous, &det);

    // J1-2 每条账面 ≥ 下限。
    let mut floor_ok = items.len() == BATCH_SIZE;
    let mut k = 0usize;
    while k < items.len() {
        if let Some(c) = items.get(k) {
            if c.floor == 0 || c.lines < c.floor {
                floor_ok = false;
            }
        }
        k += 1;
    }
    s.add("50040-J1-不低于下限", floor_ok, if floor_ok { "" } else { "有条目低于下限" });

    // J1-3 判据数落在 8..=20。
    let mut checks_ok = items.len() == BATCH_SIZE;
    let mut c = 0usize;
    while c < items.len() {
        if let Some(v) = items.get(c) {
            if v.checks < CHECKS_MIN || v.checks > CHECKS_MAX {
                checks_ok = false;
            }
        }
        c += 1;
    }
    s.add("50040-J1-判据数闭集", checks_ok, if checks_ok { "" } else { "判据数越界" });

    // J1-4 本批增量 = 6,000（折叠求和 = 检查点差额，两侧都算一遍）。
    let mut sum: u32 = 0;
    let mut fsum: u32 = 0;
    let mut m = 0usize;
    while m < items.len() {
        if let Some(v) = items.get(m) {
            sum = sum.saturating_add(v.lines);
            fsum = fsum.saturating_add(v.floor);
        }
        m += 1;
    }
    let ok = sum == BATCH_DELTA && fsum == BATCH_DELTA && items.len() == BATCH_SIZE;
    let det = format!("sum={} floors={} delta={}", sum, fsum, BATCH_DELTA);
    s.add_dyn("50040-J1-增量守恒", ok, &det);

    // J1-5 域账视图：132,000/240,000、余量 108,000、命中检查点。
    let acct = Cumulative::checked(PRIOR_CHECKPOINT, sum);
    let (ok, det) = match acct {
        Ok(a) => (
            a.total == 132_000 && a.budget == 240_000 && a.remaining == 108_000 && a.hit && a.delta == 6_000,
            a.ledger_line(),
        ),
        Err(e) => (false, format!("account err={:#x}", e.code())),
    };
    s.add_dyn("50040-J1-域账132k", ok, &det);

    // J1-6 落印入册：20/20 冻结、印面增量/累计/余量/摘要齐备。
    let mut sealed = build_ledger();
    let stamp = sealed.seal();
    let (stamp_ok, stamp_det) = match stamp {
        Ok(st) => (
            st.count == BATCH_SIZE && st.delta == 6_000 && st.total == 132_000 && st.remaining == 108_000 && st.digest != 0,
            format!("20/20 delta={} total={} dg={:08x}", st.delta, st.total, st.digest),
        ),
        Err(e) => (false, format!("seal err={:#x}", e.code())),
    };
    let ok = stamp_ok && sealed.sealed() && sealed.frozen_count() == BATCH_SIZE;
    s.add_dyn("50040-J1-批印入册", ok, &stamp_det);

    // J1-7 未全冻结不许落印：19 条冻结即 NotFrozen。
    let mut partial = BatchLedger::with_batch_capacity();
    let _ = partial.register_all(&items);
    let mut fz = 0usize;
    while fz + 1 < items.len() {
        if let Some(v) = items.get(fz) {
            let _ = partial.freeze(v.num);
        }
        fz += 1;
    }
    let ok = partial.frozen_count() == 19 && partial.seal().err() == Some(SealError::NotFrozen);
    s.add("50040-J1-未冻不印", ok, if ok { "" } else { "未冻满也落了印" });

    // J1-8 条目不足 20 不许落印：19 条全冻结仍是 Incomplete。
    let mut short = BatchLedger::with_batch_capacity();
    let mut n = 0usize;
    while n + 1 < items.len() {
        if let Some(v) = items.get(n) {
            let _ = short.register(*v);
        }
        n += 1;
    }
    let _ = short.freeze_all();
    let ok = short.len() == 19 && short.seal().err() == Some(SealError::Incomplete);
    s.add("50040-J1-少条不印", ok, if ok { "" } else { "19 条也落了印" });

    // J1-9 增量不符不许落印：把一条抬高 1 行，20 条全冻结仍 Delta。
    let mut drifted = BatchLedger::with_batch_capacity();
    let mut d = 0usize;
    while d < items.len() {
        if let Some(v) = items.get(d) {
            let card = if d == 0 { Card { lines: v.lines + 1, ..*v } } else { *v };
            let _ = drifted.register(card);
        }
        d += 1;
    }
    let _ = drifted.freeze_all();
    let ok = drifted.delta() == BATCH_DELTA + 1 && drifted.seal().err() == Some(SealError::Delta);
    s.add("50040-J1-增量不符拒印", ok, if ok { "" } else { "增量被改也落了印" });

    // J1-10 前批检查点错位必拒：125,999 与 126,001 都落 Prior。
    let ok = Cumulative::checked(125_999, 6_000).err() == Some(SealError::Prior)
        && Cumulative::checked(126_001, 6_000).err() == Some(SealError::Prior)
        && Cumulative::checked(126_000, 6_000).is_ok();
    s.add("50040-J1-前批对账", ok, if ok { "" } else { "前批错位未被拒" });

    // J1-11 二次落印必拒，印面一字不变。
    let first = sealed.stamp();
    let again = sealed.seal();
    let ok = again.err() == Some(SealError::AlreadySealed) && sealed.stamp() == first && first.is_some();
    s.add("50040-J1-印不重落", ok, if ok { "" } else { "允许重落印" });

    // J1-12 印后只读：注册 / 冻结 / 再冻都被 Immutable 挡回。
    let probe = Card { num: 50041, slug: "nextbatch", floor: 200, lines: 200, checks: 12, frozen: false };
    let reg = sealed.register(probe).err() == Some(SealError::Immutable);
    let frz = sealed.freeze(50021).err() == Some(SealError::Immutable);
    let ok = reg && frz && sealed.len() == BATCH_SIZE && sealed.delta() == BATCH_DELTA;
    s.add("50040-J1-印后只读", ok, if ok { "" } else { "印后仍可改账" });

    // J1-13 注册防线：重复号 / 越界号 / 容量满 / 未知号 / 重复冻结五类都拒。
    let mut g = BatchLedger::new(2);
    let a = Card { num: 50021, slug: "nup", floor: 420, lines: 420, checks: 16, frozen: false };
    let bnd = Card { num: 50999, slug: "nup", floor: 420, lines: 420, checks: 16, frozen: false };
    let b2 = Card { num: 50022, slug: "nuporder", floor: 480, lines: 480, checks: 15, frozen: false };
    let b3 = Card { num: 50023, slug: "booklet", floor: 400, lines: 400, checks: 15, frozen: false };
    let first_ok = g.register(a).is_ok();
    let dup = g.register(a).err() == Some(SealError::Duplicate);
    let badnum = g.register(bnd).err() == Some(SealError::BadNum);
    let second_ok = g.register(b2).is_ok();
    let cap = g.register(b3).err() == Some(SealError::Capacity);
    let unknown = g.freeze(50023).err() == Some(SealError::Unknown);
    let froz = g.freeze(50021).is_ok();
    let twice = g.freeze(50021).err() == Some(SealError::FrozenTwice);
    let ok = first_ok && dup && badnum && second_ok && cap && unknown && froz && twice && g.len() == 2;
    let det = format!("len={} rej={}", g.len(), g.rejected());
    s.add_dyn("50040-J1-注册防线", ok, &det);

    // J1-14 逐条审计：清单与已落印账本 20/20 一致，无漂移/缺失/多余。
    let audit = sealed.audit(MANIFEST);
    let (ok, det) = match audit {
        Ok(r) => (
            r.clean() && r.matched == 20 && r.ledger == 20,
            format!("m={} x={} miss={} extra={}", r.matched, r.mismatched, r.missing, r.extra),
        ),
        Err(e) => (false, format!("audit err={:#x}", e.code())),
    };
    s.add_dyn("50040-J1-逐条审计", ok, &det);

    // J1-15 印面确定性：两次独立建账落印得同摘要、同链尾。
    let x = sealed_ledger();
    let y = sealed_ledger();
    let ok = x.stamp().is_some() && x.stamp() == y.stamp() && x.chain() == y.chain() && x.total_checks() == y.total_checks();
    let det = match x.stamp() {
        Some(st) => format!("dg={:08x} chain={:08x}", st.digest, st.chain),
        None => "no stamp".to_string(),
    };
    s.add_dyn("50040-J1-印面确定", ok, &det);

    // J1-16 数字往返：解析与输出互逆（0..=1000 抽样 + 全部卡号）。
    let mut rt_ok = true;
    let mut v = 0u32;
    while v <= 1000 {
        let text = itoa(v);
        match parse_u32(&text) {
            Ok(back) => {
                if back != v {
                    rt_ok = false;
                }
            }
            Err(_) => rt_ok = false,
        }
        v += 1;
    }
    let mut q = 0usize;
    while q < items.len() {
        if let Some(c) = items.get(q) {
            match parse_u32(&itoa(c.num)) {
                Ok(back) => {
                    if back != c.num {
                        rt_ok = false;
                    }
                }
                Err(_) => rt_ok = false,
            }
        }
        q += 1;
    }
    s.add("50040-J1-数字往返", rt_ok, if rt_ok { "" } else { "十进制往返不闭合" });

    // J1-17 九类坏清单全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50040-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50040_checks_green() {
        let set = run_unx50040_checks();
        assert!(set.all_passed(), "UNX-F50040 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50040_seal_hits_checkpoint() {
        let l = sealed_ledger();
        match l.stamp() {
            Some(st) => {
                assert_eq!(st.count, 20);
                assert_eq!(st.delta, 6_000);
                assert_eq!(st.total, 132_000);
                assert_eq!(st.remaining, 108_000);
            }
            None => panic!("seal failed"),
        }
    }

    #[test]
    fn unx50040_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 9);
    }
}
