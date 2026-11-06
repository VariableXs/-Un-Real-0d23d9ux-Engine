//! UNX-F50147 发现缓存与 TTL 失效账（0x43e80 段）
//!
//! 锚点：UNX-F50147-J1 TTL ×100 轮失效零误判；判据锚定 §AI-63 §2 专题五。
//!
//! 判据锚定 §AI-63 §2 专题五。
//!
//! 发现链的缓存只有一个真问题：**什么时候可以忘掉一条服务记录**。mDNS 把
//! 这件事交给每条记录自带的 TTL——在 `[learned_at, learned_at + ttl)` 这个
//! 半开区间里记录有效，右端点一到立刻作废。半开区间不是随手挑的：闭区间会在
//! `now == expires_at` 这一拍上同时存在「已过期」与「未过期」两种说法，而发现
//! 链下游要跟 B12 联签，判据必须逐拍可复算，不允许同一时刻两种答案。
//!
//! 三段结构：
//! * [`DiscoveryCache`] —— 定容服务记录表。键 = 名字（ASCII 不区分大小写）+
//!   地址 + 端口；插入逐项校验，重复/满表显式拒绝，刷新只前移过期点而不改键；
//! * [`RecState`] 状态机 —— `Fresh → Aging → Expired → Evicted`，`rank()` 单调。
//!   状态只由 `(now, expires_at, ttl)` 现场求值，**不缓存「上次状态」**：一旦
//!   存了，时钟回退或刷新之后就必然与事实脱节；
//! * [`TtlLedger`] —— 失效账。每次失效（过期 / 内容替换 / 容量淘汰）落一条
//!   账，带原因、时刻、存活时长、失效后状态。账本总量必须等于分因计数之和。
//!
//! 为什么账要记「存活时长」而不只是计数：发现缓存的经典故障是「还没到期就被
//! 丢掉」。这类误判在纯计数账面上与正常过期长得一模一样，只有把
//! `now - learned_at` 与 `ttl` 摆在一起才分辨得出来。
//!
//! 错误路径：段内闭集 `SEG_BASE + 1 ..= SEG_BASE + 12`，逐项见 [`TtlError`]；
//! 12 个码值全部在 [`injection_core`] 里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43e80 独占 0x20）。
pub const SEG_BASE: u32 = 0x43e80;

/// 缓存表硬顶。24 条足够覆盖同网段打印/扫描/传真三类实例的常见规模，
/// 又不至于让定长扫描的代价随网络规模失控。
pub const CACHE_CAP: usize = 24;

/// 账本采样硬顶。超出后滚动丢弃最早的采样并计入 `overwritten`——
/// 丢弃是显式的，绝不静默。
pub const LEDGER_CAP: usize = 32;

/// 实例名上限（字节）。DNS-SD 实例名常见 ≤63，这里留出 UTF-8 余量取 48，
/// 超过即判 `NameTooLong`，不做截断（截断会让两条记录撞键）。
pub const NAME_MAX: usize = 48;

/// TTL 上限 86400 秒（24h）。超过的不是缓存语义而是「永久」，应走静态配置。
pub const TTL_MAX: u32 = 86_400;

pub const E_EMPTY_NAME: u16 = 1;
pub const E_NAME_LONG: u16 = 2;
pub const E_CACHE_FULL: u16 = 3;
pub const E_DUPLICATE: u16 = 4;
pub const E_NOT_FOUND: u16 = 5;
pub const E_ZERO_TTL: u16 = 6;
pub const E_TTL_LONG: u16 = 7;
pub const E_ZERO_PORT: u16 = 8;
pub const E_BAD_ADDR: u16 = 9;
pub const E_CLOCK_BACK: u16 = 10;
pub const E_BAD_DIGEST: u16 = 11;
pub const E_OVERFLOW: u16 = 12;

/// 发现缓存域错误。`code()` 返回 `SEG_BASE + n`，与全仓诊断码闭集一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TtlError {
    EmptyName,
    NameTooLong,
    CacheFull,
    Duplicate,
    NotFound,
    ZeroTtl,
    TtlTooLong,
    ZeroPort,
    BadAddr,
    ClockBack,
    BadDigest,
    Overflow,
}

impl TtlError {
    pub fn code(self) -> u32 {
        SEG_BASE + self.offset() as u32
    }

    /// 段内偏移（1..=12）。账本与判据都按偏移做闭集校验。
    pub fn offset(self) -> u16 {
        match self {
            TtlError::EmptyName => E_EMPTY_NAME,
            TtlError::NameTooLong => E_NAME_LONG,
            TtlError::CacheFull => E_CACHE_FULL,
            TtlError::Duplicate => E_DUPLICATE,
            TtlError::NotFound => E_NOT_FOUND,
            TtlError::ZeroTtl => E_ZERO_TTL,
            TtlError::TtlTooLong => E_TTL_LONG,
            TtlError::ZeroPort => E_ZERO_PORT,
            TtlError::BadAddr => E_BAD_ADDR,
            TtlError::ClockBack => E_CLOCK_BACK,
            TtlError::BadDigest => E_BAD_DIGEST,
            TtlError::Overflow => E_OVERFLOW,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            TtlError::EmptyName => "service instance name empty",
            TtlError::NameTooLong => "service instance name over limit",
            TtlError::CacheFull => "discovery cache full",
            TtlError::Duplicate => "service record already cached",
            TtlError::NotFound => "service record not cached",
            TtlError::ZeroTtl => "ttl zero means not cacheable",
            TtlError::TtlTooLong => "ttl beyond cache ceiling",
            TtlError::ZeroPort => "service port zero",
            TtlError::BadAddr => "service address unusable",
            TtlError::ClockBack => "monotonic clock went backwards",
            TtlError::BadDigest => "payload digest zero (unset)",
            TtlError::Overflow => "expiry tick overflow",
        }
    }
}

/// 闭集枚举（顺序即偏移序），供判据与注入核遍历。
pub const ALL_ERRORS: [TtlError; 12] = [
    TtlError::EmptyName,
    TtlError::NameTooLong,
    TtlError::CacheFull,
    TtlError::Duplicate,
    TtlError::NotFound,
    TtlError::ZeroTtl,
    TtlError::TtlTooLong,
    TtlError::ZeroPort,
    TtlError::BadAddr,
    TtlError::ClockBack,
    TtlError::BadDigest,
    TtlError::Overflow,
];

/// 记录生命周期四态。`Evicted` 只能由容量淘汰产生，求值函数永远不会返回它
/// ——淘汰是一次动作，不是时间的函数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecState {
    Fresh,
    Aging,
    Expired,
    Evicted,
}

impl RecState {
    /// 单调序号：同一条记录随 `now` 前进只能非降。
    pub fn rank(self) -> u8 {
        match self {
            RecState::Fresh => 0,
            RecState::Aging => 1,
            RecState::Expired => 2,
            RecState::Evicted => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RecState::Fresh => "fresh",
            RecState::Aging => "aging",
            RecState::Expired => "expired",
            RecState::Evicted => "evicted",
        }
    }
}

/// 失效原因三类。分因计数必须与账本总量守恒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidReason {
    Expired,
    Replaced,
    Evicted,
}

impl InvalidReason {
    pub fn label(self) -> &'static str {
        match self {
            InvalidReason::Expired => "expired",
            InvalidReason::Replaced => "replaced",
            InvalidReason::Evicted => "evicted",
        }
    }
}

/// 一条缓存记录。`expires_at` 是冗余字段（= `learned_at + ttl`），保留它是
/// 为了让「刷新只前移过期点」这件事在数据结构层面可见，也避免每拍重复做加法。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvcRecord {
    pub name: String,
    pub addr: u32,
    pub port: u16,
    /// TXT/属性摘要，0 保留为「未设置」，因此 0 必须拒绝。
    pub payload: u64,
    pub ttl: u32,
    pub learned_at: u32,
    pub expires_at: u32,
    pub refresh_count: u16,
    pub hits: u32,
}

impl SvcRecord {
    /// 现场求值生命周期状态。半开区间：`now < expires_at` 才算活。
    pub fn state(&self, now: u32) -> RecState {
        if self.expires_at <= now {
            return RecState::Expired;
        }
        let remaining = self.expires_at - now;
        // 剩余不足半个 TTL 即进入 Aging：这是「该重新探测了」的信号，
        // 阈值取 ttl/2 而不是固定秒数，才能同时适配 120s 与 4500s 的记录。
        if remaining.saturating_mul(2) <= self.ttl {
            RecState::Aging
        } else {
            RecState::Fresh
        }
    }

    /// 记录键（名字小写化 + 地址 + 端口）的 FNV-1a 低 32 位，账本用它定位。
    pub fn key32(&self) -> u32 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let bytes = self.name.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                let low = if *b >= b'A' && *b <= b'Z' { *b + 32 } else { *b };
                h = fnv_mix(h, low as u64);
            }
            i += 1;
        }
        h = fnv_mix(h, self.addr as u64);
        h = fnv_mix(h, self.port as u64);
        (h & 0xffff_ffff) as u32
    }

    /// 同一键的判定口径（与 `find` 完全一致，避免两处口径漂移）。
    pub fn same_key(&self, name: &str, addr: u32, port: u16) -> bool {
        self.addr == addr && self.port == port && self.name.eq_ignore_ascii_case(name)
    }
}

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 账本采样。存活时长 `age` 是误判分析的关键量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    pub reason: InvalidReason,
    pub key: u32,
    pub at: u32,
    pub age: u32,
    pub state: RecState,
}

/// 失效账。定容采样 + 分因计数；总量守恒是硬不变量。
#[derive(Clone, Debug)]
pub struct TtlLedger {
    samples: Vec<LedgerEntry>,
    expired: u32,
    replaced: u32,
    evicted: u32,
    total: u32,
    overwritten: u32,
}

impl TtlLedger {
    pub fn new() -> TtlLedger {
        TtlLedger {
            samples: Vec::with_capacity(LEDGER_CAP),
            expired: 0,
            replaced: 0,
            evicted: 0,
            total: 0,
            overwritten: 0,
        }
    }

    /// 落一条账。采样满则丢最早一条，丢弃量显式累计。
    pub fn record(&mut self, reason: InvalidReason, key: u32, at: u32, age: u32, state: RecState) {
        match reason {
            InvalidReason::Expired => self.expired = self.expired.saturating_add(1),
            InvalidReason::Replaced => self.replaced = self.replaced.saturating_add(1),
            InvalidReason::Evicted => self.evicted = self.evicted.saturating_add(1),
        }
        self.total = self.total.saturating_add(1);
        let e = LedgerEntry {
            reason,
            key,
            at,
            age,
            state,
        };
        if self.samples.len() >= LEDGER_CAP {
            if !self.samples.is_empty() {
                self.samples.remove(0);
            }
            self.overwritten = self.overwritten.saturating_add(1);
        }
        self.samples.push(e);
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn expired(&self) -> u32 {
        self.expired
    }

    pub fn replaced(&self) -> u32 {
        self.replaced
    }

    pub fn evicted(&self) -> u32 {
        self.evicted
    }

    pub fn overwritten(&self) -> u32 {
        self.overwritten
    }

    pub fn samples_len(&self) -> usize {
        self.samples.len()
    }

    pub fn sample(&self, idx: usize) -> Option<LedgerEntry> {
        self.samples.get(idx).copied()
    }

    /// 分因守恒：总量 == 过期 + 替换 + 淘汰。
    pub fn balanced(&self) -> bool {
        self.total == self.expired.saturating_add(self.replaced).saturating_add(self.evicted)
    }

    /// 采样守恒：留存采样 + 被覆盖数 == 总量。
    pub fn samples_balanced(&self) -> bool {
        (self.samples.len() as u32).saturating_add(self.overwritten) == self.total
    }
}

/// 一次清扫的回报。`scanned`/`kept`/`expired` 三者相加关系是判据的一部分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SweepReport {
    pub now: u32,
    pub scanned: usize,
    pub expired: usize,
    pub kept: usize,
}

/// 刷新结果两态：只续期，或内容被替换（替换要落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshOutcome {
    Extended,
    Replaced,
}

/// 发现缓存。插入序即表序，摘要按表序计算（同序列逐位可复算）。
#[derive(Clone, Debug)]
pub struct DiscoveryCache {
    recs: Vec<SvcRecord>,
    ledger: TtlLedger,
    inserts: u32,
    refreshes: u32,
    sweeps: u32,
}

impl DiscoveryCache {
    pub fn new() -> DiscoveryCache {
        DiscoveryCache {
            recs: Vec::with_capacity(CACHE_CAP),
            ledger: TtlLedger::new(),
            inserts: 0,
            refreshes: 0,
            sweeps: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn at(&self, idx: usize) -> Option<&SvcRecord> {
        self.recs.get(idx)
    }

    pub fn ledger(&self) -> &TtlLedger {
        &self.ledger
    }

    pub fn inserts(&self) -> u32 {
        self.inserts
    }

    pub fn refreshes(&self) -> u32 {
        self.refreshes
    }

    pub fn sweeps(&self) -> u32 {
        self.sweeps
    }

    /// 按三元键定位下标；名字比较不区分 ASCII 大小写（DNS 语义）。
    fn find(&self, name: &str, addr: u32, port: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.same_key(name, addr, port) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 三元键是否仍在有效期内（不计数命中）。
    pub fn contains(&self, name: &str, addr: u32, port: u16, now: u32) -> bool {
        match self.find(name, addr, port) {
            Some(i) => match self.recs.get(i) {
                Some(r) => r.expires_at > now,
                None => false,
            },
            None => false,
        }
    }

    /// 计数命中并取记录；已过期的记录视为不存在（返回 None）。
    pub fn lookup(&mut self, name: &str, addr: u32, port: u16, now: u32) -> Option<&SvcRecord> {
        let idx = self.find(name, addr, port)?;
        let alive = match self.recs.get(idx) {
            Some(r) => r.expires_at > now,
            None => false,
        };
        if !alive {
            return None;
        }
        if let Some(r) = self.recs.get_mut(idx) {
            r.hits = r.hits.saturating_add(1);
        }
        self.recs.get(idx)
    }

    /// 插入一条记录，返回其 32 位键。重复键与满表都显式拒绝。
    pub fn insert(
        &mut self,
        name: &str,
        addr: u32,
        port: u16,
        payload: u64,
        ttl: u32,
        now: u32,
    ) -> Result<u32, TtlError> {
        validate_name(name)?;
        if addr == 0 || addr == 0xffff_ffff {
            return Err(TtlError::BadAddr);
        }
        if port == 0 {
            return Err(TtlError::ZeroPort);
        }
        if payload == 0 {
            return Err(TtlError::BadDigest);
        }
        let expires_at = validate_ttl(ttl, now)?;
        if self.find(name, addr, port).is_some() {
            return Err(TtlError::Duplicate);
        }
        if self.recs.len() >= CACHE_CAP {
            return Err(TtlError::CacheFull);
        }
        let rec = SvcRecord {
            name: String::from(name),
            addr,
            port,
            payload,
            ttl,
            learned_at: now,
            expires_at,
            refresh_count: 0,
            hits: 0,
        };
        let key = rec.key32();
        self.recs.push(rec);
        self.inserts = self.inserts.saturating_add(1);
        Ok(key)
    }

    /// 先清扫再插入：到期的位置被回收，避免「表里全是尸体但插不进新人」。
    pub fn insert_reclaiming(
        &mut self,
        name: &str,
        addr: u32,
        port: u16,
        payload: u64,
        ttl: u32,
        now: u32,
    ) -> Result<SweepReport, TtlError> {
        let rep = self.sweep(now);
        self.insert(name, addr, port, payload, ttl, now)?;
        Ok(rep)
    }

    /// 淘汰最早学习到的一条（容量压力下的显式动作），落 `Evicted` 账。
    pub fn drop_oldest(&mut self, now: u32) -> Result<SvcRecord, TtlError> {
        if self.recs.is_empty() {
            return Err(TtlError::NotFound);
        }
        let mut oldest = 0usize;
        let mut i = 1usize;
        while i < self.recs.len() {
            let a = match self.recs.get(i) {
                Some(v) => v,
                None => break,
            };
            let b = match self.recs.get(oldest) {
                Some(v) => v,
                None => break,
            };
            if a.learned_at < b.learned_at {
                oldest = i;
            }
            i += 1;
        }
        let rec = match self.recs.get(oldest) {
            Some(v) => v.clone(),
            None => return Err(TtlError::NotFound),
        };
        self.recs.remove(oldest);
        self.ledger.record(
            InvalidReason::Evicted,
            rec.key32(),
            now,
            now.saturating_sub(rec.learned_at),
            RecState::Evicted,
        );
        Ok(rec)
    }

    /// 刷新：同键只前移过期点。内容变化要走替换路径并落 `Replaced` 账。
    pub fn refresh(
        &mut self,
        name: &str,
        addr: u32,
        port: u16,
        payload: u64,
        ttl: u32,
        now: u32,
    ) -> Result<RefreshOutcome, TtlError> {
        let idx = match self.find(name, addr, port) {
            Some(i) => i,
            None => return Err(TtlError::NotFound),
        };
        if payload == 0 {
            return Err(TtlError::BadDigest);
        }
        let expires_at = validate_ttl(ttl, now)?;
        let (learned_at, old_payload, key) = match self.recs.get(idx) {
            Some(r) => (r.learned_at, r.payload, r.key32()),
            None => return Err(TtlError::NotFound),
        };
        // 单调时钟回归必须先拒：否则 `now - learned_at` 会下溢成一笔巨额账。
        if now < learned_at {
            return Err(TtlError::ClockBack);
        }
        let replaced = old_payload != payload;
        if let Some(r) = self.recs.get_mut(idx) {
            r.payload = payload;
            r.ttl = ttl;
            r.learned_at = now;
            r.expires_at = expires_at;
            r.refresh_count = r.refresh_count.saturating_add(1);
        }
        let age = now.saturating_sub(learned_at);
        if replaced {
            self.ledger
                .record(InvalidReason::Replaced, key, now, age, RecState::Expired);
        }
        self.refreshes = self.refreshes.saturating_add(1);
        Ok(if replaced {
            RefreshOutcome::Replaced
        } else {
            RefreshOutcome::Extended
        })
    }

    /// 清扫：`expires_at <= now` 的记录全部移出并逐条落账。
    ///
    /// 先收集死键再改账本，是为了避开「同时借 `recs` 与 `ledger`」的借用冲突
    /// ——也是把「先判定、后记账」两阶段在代码里钉死，防止半写。
    pub fn sweep(&mut self, now: u32) -> SweepReport {
        let scanned = self.recs.len();
        let mut kept: Vec<SvcRecord> = Vec::with_capacity(scanned);
        let mut dead: Vec<(u32, u32)> = Vec::new();
        let mut i = 0usize;
        while i < scanned {
            if let Some(r) = self.recs.get(i) {
                if r.expires_at <= now {
                    dead.push((r.key32(), now.saturating_sub(r.learned_at)));
                } else {
                    kept.push(r.clone());
                }
            }
            i += 1;
        }
        self.recs = kept;
        let expired = dead.len();
        let mut d = 0usize;
        while d < expired {
            if let Some((key, age)) = dead.get(d).copied() {
                self.ledger
                    .record(InvalidReason::Expired, key, now, age, RecState::Expired);
            }
            d += 1;
        }
        self.sweeps = self.sweeps.saturating_add(1);
        SweepReport {
            now,
            scanned,
            expired,
            kept: scanned - expired,
        }
    }

    /// 当前有效记录数（不清扫也能算准）。
    pub fn live_len(&self, now: u32) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.expires_at > now {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 表摘要（表序）。两次同序列构造必须逐位相等。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                let bytes = r.name.as_bytes();
                let mut k = 0usize;
                while k < bytes.len() {
                    if let Some(b) = bytes.get(k) {
                        h = fnv_mix(h, *b as u64);
                    }
                    k += 1;
                }
                h = fnv_mix(h, r.addr as u64);
                h = fnv_mix(h, r.port as u64);
                h = fnv_mix(h, r.payload);
                h = fnv_mix(h, r.ttl as u64);
                h = fnv_mix(h, r.expires_at as u64);
                h = fnv_mix(h, r.refresh_count as u64);
            }
            i += 1;
        }
        h
    }
}

fn validate_name(name: &str) -> Result<(), TtlError> {
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return Err(TtlError::EmptyName);
    }
    let mut i = 0usize;
    let mut printable = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            // 纯空白名的哈希与空名无法区分，必须先拒；控制字符会破坏控制台日志。
            if *b > 0x20 && *b != 0x7f {
                printable += 1;
            }
        }
        i += 1;
    }
    if printable == 0 {
        return Err(TtlError::EmptyName);
    }
    if bytes.len() > NAME_MAX {
        return Err(TtlError::NameTooLong);
    }
    Ok(())
}

fn validate_ttl(ttl: u32, now: u32) -> Result<u32, TtlError> {
    if ttl == 0 {
        return Err(TtlError::ZeroTtl);
    }
    if ttl > TTL_MAX {
        return Err(TtlError::TtlTooLong);
    }
    match now.checked_add(ttl) {
        Some(v) => Ok(v),
        None => Err(TtlError::Overflow),
    }
}

// ---------------------------------------------------------------------------
// 百轮审计：J1 的度量本体
// ---------------------------------------------------------------------------

/// 审计回报。`false_positive` = 缓存说活但按 TTL 应死；`false_negative` 反之。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TtlAudit {
    pub rounds: u32,
    pub false_positive: u32,
    pub false_negative: u32,
    pub expiries: u32,
    pub refreshed: u32,
    pub peak_live: usize,
}

#[derive(Clone, Debug)]
struct Tracked {
    name: String,
    addr: u32,
    port: u16,
    payload: u64,
    expires: u32,
}

/// 跑 `rounds` 轮「插入 → 续期 → 清扫 → 对账」。
///
/// 每轮只插入一条新记录并可能续期最近一条：这样任意时刻的期望存活集都能用
/// 一个短列表算准，不需要在审计器里复制一份缓存——审计器一旦复用被测逻辑，
/// 误判就被原样复制进期望值，判据等于恒真。
pub fn ttl_expiry_audit(rounds: u32) -> Result<TtlAudit, TtlError> {
    let mut cache = DiscoveryCache::new();
    let mut live: Vec<Tracked> = Vec::with_capacity(CACHE_CAP);
    let mut audit = TtlAudit {
        rounds: 0,
        false_positive: 0,
        false_negative: 0,
        expiries: 0,
        refreshed: 0,
        peak_live: 0,
    };
    let mut r = 0u32;
    while r < rounds {
        // 步长 3 秒、TTL 4..=16 秒：每条记录活 1~5 轮，表压始终远低于容量，
        // 于是「所有消失」都只能归因于 TTL，而不是被容量挤掉。
        let now = r.saturating_mul(3);
        let ttl = 4 + (r % 13);
        let name = format!("prn-{}", r);
        let addr = 0x0a00_0000u32.wrapping_add(r);
        let payload = 0x1000u64 + r as u64;
        cache.insert(&name, addr, 631, payload, ttl, now)?;
        live.push(Tracked {
            name: String::from(name.as_str()),
            addr,
            port: 631,
            payload,
            expires: now + ttl,
        });

        // 每 3 轮续期最近一条：续期必须只把过期点前移到 now + ttl。
        if r % 3 == 0 {
            if let Some(last) = live.last() {
                let (n, a, p, pl) = (last.name.clone(), last.addr, last.port, last.payload);
                cache.refresh(&n, a, p, pl, ttl, now)?;
                let idx = live.len() - 1;
                if let Some(t) = live.get_mut(idx) {
                    t.expires = now + ttl;
                }
                audit.refreshed = audit.refreshed.saturating_add(1);
            }
        }

        let rep = cache.sweep(now);
        audit.expiries = audit.expiries.saturating_add(rep.expired as u32);

        let mut i = 0usize;
        while i < live.len() {
            let tracked = match live.get(i) {
                Some(t) => t.clone(),
                None => break,
            };
            // 期望值直接由 TTL 定义算出，不经过缓存。
            let expect = now < tracked.expires;
            let actual = cache.contains(&tracked.name, tracked.addr, tracked.port, now);
            if actual && !expect {
                audit.false_positive = audit.false_positive.saturating_add(1);
            }
            if !actual && expect {
                audit.false_negative = audit.false_negative.saturating_add(1);
            }
            if expect {
                i += 1;
            } else {
                live.remove(i);
            }
        }
        let alive = cache.live_len(now);
        if alive > audit.peak_live {
            audit.peak_live = alive;
        }
        audit.rounds = audit.rounds.saturating_add(1);
        r = r.saturating_add(1);
    }
    Ok(audit)
}

// ---------------------------------------------------------------------------
// 注入核：12 类坏样本，逐个断言诊断码
// ---------------------------------------------------------------------------

/// 造一张有 `n` 条互不相同记录的缓存。
fn seeded(n: usize) -> Result<DiscoveryCache, TtlError> {
    let mut c = DiscoveryCache::new();
    let mut i = 0usize;
    while i < n {
        let name = format!("seed-{}", i);
        c.insert(&name, 0x0a00_0001 + i as u32, 631, 0x9000 + i as u64, 60, 0)?;
        i += 1;
    }
    Ok(c)
}

/// 注入核：12 类坏样本 → 12 个具体诊断码，全抓才算过。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    // 每个码值恰有一个样本：样本数与码值数一一对应，count 才可信。
    let mut c = DiscoveryCache::new();
    if c.insert("", 0x0a00_0001, 631, 7, 60, 0).err() == Some(TtlError::EmptyName) {
        caught += 1;
    }
    let long = "x".repeat(NAME_MAX + 1);
    if c.insert(&long, 0x0a00_0001, 631, 7, 60, 0).err() == Some(TtlError::NameTooLong) {
        caught += 1;
    }
    if c.insert("a", 0x0a00_0001, 631, 7, 0, 0).err() == Some(TtlError::ZeroTtl) {
        caught += 1;
    }
    if c.insert("a", 0x0a00_0001, 631, 7, TTL_MAX + 1, 0).err() == Some(TtlError::TtlTooLong) {
        caught += 1;
    }
    if c.insert("a", 0x0a00_0001, 0, 7, 60, 0).err() == Some(TtlError::ZeroPort) {
        caught += 1;
    }
    if c.insert("a", 0, 631, 7, 60, 0).err() == Some(TtlError::BadAddr) {
        caught += 1;
    }
    if c.insert("a", 0x0a00_0001, 631, 0, 60, 0).err() == Some(TtlError::BadDigest) {
        caught += 1;
    }
    if c.insert("a", 0x0a00_0001, 631, 7, 10, 0).is_ok()
        && c.insert("A", 0x0a00_0001, 631, 7, 10, 0).err() == Some(TtlError::Duplicate)
    {
        caught += 1;
    }
    let mut full = match seeded(CACHE_CAP) {
        Ok(v) => v,
        Err(_) => DiscoveryCache::new(),
    };
    if full.insert("extra", 0x0b00_0001, 631, 7, 60, 0).err() == Some(TtlError::CacheFull) {
        caught += 1;
    }
    if full.refresh("absent", 0x0b00_0001, 631, 7, 60, 0).err() == Some(TtlError::NotFound) {
        caught += 1;
    }
    // 时钟回归：先插入于 t=100，再以 t=90 刷新。
    let mut back = DiscoveryCache::new();
    let ok = match back.insert("back", 0x0a00_0009, 631, 7, 60, 100) {
        Ok(_) => back.refresh("back", 0x0a00_0009, 631, 7, 60, 90).err() == Some(TtlError::ClockBack),
        Err(_) => false,
    };
    if ok {
        caught += 1;
    }
    // 到期点溢出：now 与 ttl 都在合法域内，但相加越界。
    if c.insert("ov", 0x0a00_0001, 631, 7, 10, u32::MAX - 2).err() == Some(TtlError::Overflow) {
        caught += 1;
    }

    (caught.min(total), total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50147_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50147");

    // J1-1 百轮审计零误判（判据原文的正面度量）
    let audit = ttl_expiry_audit(100);
    let ok = match audit {
        Ok(a) => a.rounds == 100 && a.false_positive == 0 && a.false_negative == 0 && a.expiries > 0,
        Err(_) => false,
    };
    let det = match audit {
        Ok(a) => format!(
            "rounds={} fp={} fn={} exp={}",
            a.rounds, a.false_positive, a.false_negative, a.expiries
        ),
        Err(e) => format!("audit err {}", e.code()),
    };
    s.add_dyn("50147-J1-百轮零误判", ok, &det);

    // J1-2 半开区间：expires_at-1 仍活、expires_at 当拍即死
    let mut edge = DiscoveryCache::new();
    let ok = match edge.insert("edge", 0x0a00_0011, 631, 5, 10, 100) {
        Ok(_) => {
            let live = edge.contains("edge", 0x0a00_0011, 631, 109);
            let dead = !edge.contains("edge", 0x0a00_0011, 631, 110);
            let rep = edge.sweep(110);
            live && dead && rep.expired == 1 && edge.len() == 0
        }
        Err(_) => false,
    };
    s.add("50147-J1-半开区间", ok, if ok { "" } else { "TTL 端点判定漂移" });

    // J1-3 刷新幂等：同一拍刷两次，过期点与表摘要只由最后一次决定
    let mut idem = DiscoveryCache::new();
    let ok = match idem.insert("idem", 0x0a00_0012, 631, 5, 10, 100) {
        Ok(_) => {
            let a = idem.refresh("idem", 0x0a00_0012, 631, 5, 20, 120);
            let exp1 = match idem.at(0) {
                Some(r) => r.expires_at,
                None => 0,
            };
            let b = idem.refresh("idem", 0x0a00_0012, 631, 5, 20, 120);
            let exp2 = match idem.at(0) {
                Some(r) => r.expires_at,
                None => 0,
            };
            a == Ok(RefreshOutcome::Extended)
                && b == Ok(RefreshOutcome::Extended)
                && exp1 == 140
                && exp1 == exp2
                && idem.ledger().total() == 0
        }
        Err(_) => false,
    };
    s.add("50147-J1-刷新幂等", ok, if ok { "" } else { "重复刷新改写了过期点" });

    // J1-4 刷新单调：now 前进时过期点严格前移
    let mut mono = DiscoveryCache::new();
    let ok = match mono.insert("mono", 0x0a00_0013, 631, 5, 30, 100) {
        Ok(_) => {
            let e0 = match mono.at(0) {
                Some(r) => r.expires_at,
                None => 0,
            };
            let _ = mono.refresh("mono", 0x0a00_0013, 631, 5, 30, 110);
            let e1 = match mono.at(0) {
                Some(r) => r.expires_at,
                None => 0,
            };
            let _ = mono.refresh("mono", 0x0a00_0013, 631, 5, 30, 120);
            let e2 = match mono.at(0) {
                Some(r) => r.expires_at,
                None => 0,
            };
            e0 < e1 && e1 < e2 && e1 == 140 && e2 == 150
        }
        Err(_) => false,
    };
    s.add("50147-J1-续期单调", ok, if ok { "" } else { "续期未前移过期点" });

    // J1-5 状态机随 now 单调：Fresh → Aging → Expired 只走一次
    let mut sm = DiscoveryCache::new();
    let ok = match sm.insert("sm", 0x0a00_0014, 631, 5, 10, 0) {
        Ok(_) => {
            let mut prev = 0u8;
            let mut monotone = true;
            let mut now = 0u32;
            while now <= 12 {
                let rank = match sm.at(0) {
                    Some(r) => r.state(now).rank(),
                    None => 3,
                };
                if rank < prev {
                    monotone = false;
                }
                prev = rank;
                now += 1;
            }
            monotone && prev == 2
        }
        Err(_) => false,
    };
    s.add("50147-J1-状态单调", ok, if ok { "" } else { "生命周期回退" });

    // J1-6 老化阈值：剩余 == ttl/2 处恰好进入 Aging
    let mut ag = DiscoveryCache::new();
    let ok = match ag.insert("ag", 0x0a00_0015, 631, 5, 10, 0) {
        Ok(_) => match ag.at(0) {
            Some(r) => {
                r.state(4) == RecState::Fresh
                    && r.state(5) == RecState::Aging
                    && r.state(9) == RecState::Aging
                    && r.state(10) == RecState::Expired
            }
            None => false,
        },
        Err(_) => false,
    };
    s.add("50147-J1-老化阈值", ok, if ok { "" } else { "Aging 边界错位" });

    // J1-7 账本守恒：总量 == 过期 + 替换 + 淘汰；采样也守恒
    let mut led = match seeded(4) {
        Ok(v) => v,
        Err(_) => DiscoveryCache::new(),
    };
    let _ = led.sweep(61);
    let l = led.ledger();
    let ok = l.total() == 4 && l.expired() == 4 && l.balanced() && l.samples_balanced();
    let det = format!(
        "total={} exp={} rep={} evi={} samp={}",
        l.total(),
        l.expired(),
        l.replaced(),
        l.evicted(),
        l.samples_len()
    );
    s.add_dyn("50147-J1-账本守恒", ok, &det);

    // J1-8 内容替换必落账且计数 +1
    let mut rep = match seeded(1) {
        Ok(v) => v,
        Err(_) => DiscoveryCache::new(),
    };
    let ok = match rep.refresh("seed-0", 0x0a00_0001, 631, 0x7777, 30, 10) {
        Ok(o) => o == RefreshOutcome::Replaced && rep.ledger().replaced() == 1 && rep.ledger().total() == 1,
        Err(_) => false,
    };
    s.add("50147-J1-替换落账", ok, if ok { "" } else { "内容替换未记账" });

    // J1-9 容量满必拒，且拒绝后表内容不变
    let mut capc = match seeded(CACHE_CAP) {
        Ok(v) => v,
        Err(_) => DiscoveryCache::new(),
    };
    let before = capc.digest();
    let rejected = capc.insert("over", 0x0c00_0001, 631, 5, 60, 0).err() == Some(TtlError::CacheFull);
    let ok = rejected && capc.len() == CACHE_CAP && capc.digest() == before;
    s.add("50147-J1-满表必拒", ok, if ok { "" } else { "满表插入半写" });

    // J1-10 显式淘汰：落 Evicted 账并释放一个位
    let ok = match capc.drop_oldest(5) {
        Ok(_) => {
            capc.len() == CACHE_CAP - 1
                && capc.ledger().evicted() == 1
                && capc.insert("over", 0x0c00_0001, 631, 5, 60, 5).is_ok()
                && capc.len() == CACHE_CAP
        }
        Err(_) => false,
    };
    s.add("50147-J1-淘汰记账", ok, if ok { "" } else { "淘汰未记账" });

    // J1-11 摘要确定性：同序列两次构造逐位相等，内容不同则不同
    let ok = match (seeded(5), seeded(5)) {
        (Ok(a), Ok(b)) => {
            let mut c = match seeded(5) {
                Ok(v) => v,
                Err(_) => DiscoveryCache::new(),
            };
            let _ = c.refresh("seed-2", 0x0a00_0003, 631, 0x5555, 60, 3);
            a.digest() == b.digest() && c.digest() != a.digest()
        }
        _ => false,
    };
    s.add("50147-J1-摘要确定", ok, if ok { "" } else { "表摘要不确定" });

    // J1-12 注入核全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50147-J1-注入必抓", ok, &det);

    // J1-13 错误码闭集：偏移 1..=12 连续且不重复
    let mut seen = [false; 13];
    let mut i = 0usize;
    let mut contiguous = true;
    while i < ALL_ERRORS.len() {
        let off = ALL_ERRORS[i].offset();
        if off == 0 || off as usize >= seen.len() || seen[off as usize] {
            contiguous = false;
        } else {
            seen[off as usize] = true;
        }
        if ALL_ERRORS[i].code() != SEG_BASE + off as u32 {
            contiguous = false;
        }
        i += 1;
    }
    s.add("50147-J1-码值闭集", contiguous, if contiguous { "" } else { "码值重复或断裂" });

    // J1-14 清扫账面自洽：scanned == expired + kept
    let mut sw = match seeded(3) {
        Ok(v) => v,
        Err(_) => DiscoveryCache::new(),
    };
    let r1 = sw.sweep(30);
    let r2 = sw.sweep(120);
    let ok = r1.scanned == r1.expired + r1.kept
        && r2.scanned == r2.expired + r2.kept
        && r1.expired == 0
        && r2.expired == 3
        && sw.ledger().total() == 3;
    s.add("50147-J1-清扫自洽", ok, if ok { "" } else { "清扫计数不自洽" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50147_checks_green() {
        let set = run_unx50147_checks();
        assert!(set.all_passed(), "UNX-F50147 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50147_ttl_interval_half_open() {
        let mut c = DiscoveryCache::new();
        assert!(c.insert("a", 0x0a00_0001, 631, 1, 5, 0).is_ok());
        assert!(c.contains("a", 0x0a00_0001, 631, 4));
        assert!(!c.contains("a", 0x0a00_0001, 631, 5));
    }

    #[test]
    fn unx50147_audit_is_clean() {
        let a = ttl_expiry_audit(100);
        match a {
            Ok(v) => {
                assert_eq!(v.false_positive, 0);
                assert_eq!(v.false_negative, 0);
            }
            Err(_) => panic!("audit failed"),
        }
    }
}
