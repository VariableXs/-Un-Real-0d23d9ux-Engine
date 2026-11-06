//! UNX-F50149 发现与 B12 IPP 发现链联签（单源零改写）（0x43ec0 段）
//!
//! 锚点：UNX-F50149-J1 联签登记，B12 条目零改写；判据锚定 §AI-63 §1 防重声明。
//!
//! 「单源」在这个模块里是**可以验证的事实**，不是一句口号：
//!
//! 1. 发现链 [`DiscoverChain`] 把一行行通告解析成端点候选。候选的键是
//!    **端点身份**（地址 + 端口 + 资源路径），**不含显示名**：同一台打印机
//!    用 `OfficeJet` 和 `HP-OfficeJet-2F` 两个实例名各通告一次，仍然是同一个
//!    端点，只能有一条候选。键里带名字就会产生两条指向同一端点的候选，
//!    下游联签会对着同一台设备签两次——这正是防重声明要防的事。
//! 2. B12 快照表 [`B12Table`] 是发现链的一次性投影：`plant` 只接受
//!    (键, 摘要)，冻结之后**同摘要重放是幂等**（`Ok(false)`），异摘要才是
//!    `EntryFrozen`。冻结禁的是「改」，不是「再说一遍」。
//! 3. 联签账 [`SealLedger`] 用前向哈希链串起每条签：`self_digest` 覆盖
//!    (prev, key, b12_digest, chain_digest, signer)。链一旦被改写，
//!    [`SealLedger::verify_chain`] 立刻 `Tamper`；B12 摘要被换掉，
//!    [`SealLedger::verify_seal`] 立刻 `DigestMismatch`。
//!
//! 「零改写」的度量：签完之后 B12 表摘要必须与签之前逐位相同。这条判据
//! 不依赖任何日志，只依赖表本身——日志可以被写成任何样子，表不会。
//!
//! 错误闭集：`SEG_BASE + 1 ..= SEG_BASE + 16`，逐项见 [`IppChainError`]。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43ec0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43ec0;

/// 候选上限。同网段可发现的 IPP 端点数量级远小于此值。
pub const CHAIN_CAP: usize = 16;
/// B12 快照上限。
pub const B12_CAP: usize = 16;
/// 联签账上限。
pub const SEAL_CAP: usize = 8;
/// 实例名上限（字节）。
pub const NAME_MAX: usize = 48;
/// 资源路径上限（字节）。RFC 8010 的 rp 实测都远短于此。
pub const RP_MAX: usize = 64;

pub const E_EMPTY_RECORD: u16 = 1;
pub const E_EMPTY_NAME: u16 = 2;
pub const E_NAME_LONG: u16 = 3;
pub const E_FIELD_COUNT: u16 = 4;
pub const E_BAD_ADDR: u16 = 5;
pub const E_ZERO_PORT: u16 = 6;
pub const E_BAD_NUMBER: u16 = 7;
pub const E_BAD_PATH: u16 = 8;
pub const E_TABLE_FULL: u16 = 9;
pub const E_DUPLICATE_KEY: u16 = 10;
pub const E_ENTRY_FROZEN: u16 = 11;
pub const E_KEY_MISSING: u16 = 12;
pub const E_DIGEST_MISMATCH: u16 = 13;
pub const E_SEAL_FULL: u16 = 14;
pub const E_TAMPER: u16 = 15;
pub const E_NOT_FROZEN: u16 = 16;

/// IPP 发现链域错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IppChainError {
    EmptyRecord,
    EmptyName,
    NameTooLong,
    BadFieldCount,
    BadAddr,
    ZeroPort,
    BadNumber,
    BadPath,
    TableFull,
    DuplicateKey,
    EntryFrozen,
    KeyMissing,
    DigestMismatch,
    SealFull,
    Tamper,
    NotFrozen,
}

impl IppChainError {
    pub fn offset(self) -> u16 {
        match self {
            IppChainError::EmptyRecord => E_EMPTY_RECORD,
            IppChainError::EmptyName => E_EMPTY_NAME,
            IppChainError::NameTooLong => E_NAME_LONG,
            IppChainError::BadFieldCount => E_FIELD_COUNT,
            IppChainError::BadAddr => E_BAD_ADDR,
            IppChainError::ZeroPort => E_ZERO_PORT,
            IppChainError::BadNumber => E_BAD_NUMBER,
            IppChainError::BadPath => E_BAD_PATH,
            IppChainError::TableFull => E_TABLE_FULL,
            IppChainError::DuplicateKey => E_DUPLICATE_KEY,
            IppChainError::EntryFrozen => E_ENTRY_FROZEN,
            IppChainError::KeyMissing => E_KEY_MISSING,
            IppChainError::DigestMismatch => E_DIGEST_MISMATCH,
            IppChainError::SealFull => E_SEAL_FULL,
            IppChainError::Tamper => E_TAMPER,
            IppChainError::NotFrozen => E_NOT_FROZEN,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.offset() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            IppChainError::EmptyRecord => "empty discovery record",
            IppChainError::EmptyName => "instance name empty",
            IppChainError::NameTooLong => "instance name over limit",
            IppChainError::BadFieldCount => "record does not have six fields",
            IppChainError::BadAddr => "endpoint address malformed",
            IppChainError::ZeroPort => "endpoint port zero",
            IppChainError::BadNumber => "numeric field malformed",
            IppChainError::BadPath => "ipp resource path malformed",
            IppChainError::TableFull => "endpoint table full",
            IppChainError::DuplicateKey => "endpoint key already registered",
            IppChainError::EntryFrozen => "b12 entry frozen against change",
            IppChainError::KeyMissing => "b12 entry not present",
            IppChainError::DigestMismatch => "b12 digest no longer matches seal",
            IppChainError::SealFull => "seal ledger full",
            IppChainError::Tamper => "seal hash chain broken",
            IppChainError::NotFrozen => "b12 snapshot not frozen yet",
        }
    }
}

/// 闭集枚举（顺序即偏移序）。
pub const ALL_ERRORS: [IppChainError; 16] = [
    IppChainError::EmptyRecord,
    IppChainError::EmptyName,
    IppChainError::NameTooLong,
    IppChainError::BadFieldCount,
    IppChainError::BadAddr,
    IppChainError::ZeroPort,
    IppChainError::BadNumber,
    IppChainError::BadPath,
    IppChainError::TableFull,
    IppChainError::DuplicateKey,
    IppChainError::EntryFrozen,
    IppChainError::KeyMissing,
    IppChainError::DigestMismatch,
    IppChainError::SealFull,
    IppChainError::Tamper,
    IppChainError::NotFrozen,
];

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

fn fnv_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 一条发现通告解析出的端点候选。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub addr: u32,
    pub port: u16,
    pub rp: String,
    pub priority: u16,
    pub weight: u16,
}

impl Candidate {
    /// 端点身份键：地址 + 端口 + rp。**刻意不含显示名**——显示名只是别名。
    pub fn key32(&self) -> u32 {
        let mut h = fnv_init();
        h = fnv_mix(h, self.addr as u64);
        h = fnv_mix(h, self.port as u64);
        let bytes = self.rp.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                h = fnv_mix(h, *b as u64);
            }
            i += 1;
        }
        (h & 0xffff_ffff) as u32
    }

    /// 候选摘要：可比较的「内容」。不含显示名，理由同 `key32`。
    pub fn digest(&self) -> u64 {
        let mut h = fnv_init();
        h = fnv_mix(h, self.key32() as u64);
        h = fnv_mix(h, self.priority as u64);
        h = fnv_mix(h, self.weight as u64);
        h
    }
}

fn split_six(text: &str) -> Result<[&str; 6], IppChainError> {
    let b = text.as_bytes();
    let mut fields: [&str; 6] = [""; 6];
    let mut n = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= b.len() {
        let at_sep = match b.get(i) {
            Some(v) => *v == b'|',
            // i == len：收尾
            None => true,
        };
        if at_sep {
            if n >= 6 {
                // 第 7 个字段：记录里多了一个 '|'，一定是别的东西被塞进来了。
                return Err(IppChainError::BadFieldCount);
            }
            let piece = match text.get(start..i) {
                Some(v) => v,
                None => return Err(IppChainError::BadFieldCount),
            };
            fields[n] = piece;
            n += 1;
            start = i + 1;
        }
        i += 1;
    }
    if n != 6 {
        return Err(IppChainError::BadFieldCount);
    }
    Ok(fields)
}

fn parse_u16(s: &str) -> Result<u16, IppChainError> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 5 {
        return Err(IppChainError::BadNumber);
    }
    // 前导零一律拒绝：`0631` 在八进制口径下是 409，宁可显式报错。
    if b.len() > 1 {
        if let Some(first) = b.first() {
            if *first == b'0' {
                return Err(IppChainError::BadNumber);
            }
        }
    }
    let mut val: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(IppChainError::BadNumber),
        };
        if !(b'0'..=b'9').contains(&c) {
            return Err(IppChainError::BadNumber);
        }
        val = val * 10 + (c - b'0') as u32;
        i += 1;
    }
    if val > 65535 {
        return Err(IppChainError::BadNumber);
    }
    Ok(val as u16)
}

fn parse_ipv4(s: &str) -> Result<u32, IppChainError> {
    let b = s.as_bytes();
    if b.is_empty() {
        return Err(IppChainError::BadAddr);
    }
    let mut octets = [0u8; 4];
    let mut parts = 0usize;
    let mut val: u32 = 0;
    let mut digits = 0usize;
    let mut i = 0usize;
    while i <= b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => b'.',
        };
        if c == b'.' {
            if digits == 0 || digits > 3 || parts >= 4 {
                return Err(IppChainError::BadAddr);
            }
            if val > 255 {
                return Err(IppChainError::BadAddr);
            }
            match octets.get_mut(parts) {
                Some(slot) => *slot = val as u8,
                None => return Err(IppChainError::BadAddr),
            }
            parts += 1;
            val = 0;
            digits = 0;
        } else if (b'0'..=b'9').contains(&c) {
            if digits == 0 && c == b'0' {
                // 前导零（含 "01"）拒绝：与 `parse_u16` 同一口径。
                let next_is_dot = match b.get(i + 1) {
                    Some(v) => *v == b'.',
                    None => true,
                };
                if !next_is_dot {
                    return Err(IppChainError::BadAddr);
                }
            }
            val = val * 10 + (c - b'0') as u32;
            digits += 1;
        } else {
            return Err(IppChainError::BadAddr);
        }
        i += 1;
    }
    if parts != 4 {
        return Err(IppChainError::BadAddr);
    }
    let addr = ((octets[0] as u32) << 24)
        | ((octets[1] as u32) << 16)
        | ((octets[2] as u32) << 8)
        | octets[3] as u32;
    if addr == 0 || addr == 0xffff_ffff {
        return Err(IppChainError::BadAddr);
    }
    Ok(addr)
}

/// 解析一行发现通告：`名字|IPv4|端口|rp|priority|weight`。
pub fn parse_candidate(text: &str) -> Result<Candidate, IppChainError> {
    if text.trim().is_empty() {
        return Err(IppChainError::EmptyRecord);
    }
    let f = split_six(text)?;
    let name = f[0].trim();
    if name.is_empty() {
        return Err(IppChainError::EmptyName);
    }
    if name.as_bytes().len() > NAME_MAX {
        return Err(IppChainError::NameTooLong);
    }
    let addr = parse_ipv4(f[1].trim())?;
    let port = parse_u16(f[2].trim())?;
    if port == 0 {
        return Err(IppChainError::ZeroPort);
    }
    let rp_raw = f[3].trim();
    if rp_raw.is_empty() || rp_raw.as_bytes().len() > RP_MAX {
        return Err(IppChainError::BadPath);
    }
    // 控制字符/空白与 ".." 段一律拒绝：rp 会被拼进 HTTP 请求行。
    let rb = rp_raw.as_bytes();
    let mut i = 0usize;
    while i < rb.len() {
        if let Some(c) = rb.get(i) {
            if *c <= 0x20 || *c == 0x7f {
                return Err(IppChainError::BadPath);
            }
        }
        i += 1;
    }
    if rp_raw.contains("..") {
        return Err(IppChainError::BadPath);
    }
    let mut rp = String::with_capacity(rp_raw.len() + 1);
    // DNS-SD 的 rp 按惯例不带前导 '/'（如 `ipp/print`），这里统一补上，
    // 让键与下游 HTTP 路径只存在一种写法。
    if !rp_raw.starts_with('/') {
        rp.push('/');
    }
    rp.push_str(rp_raw);
    let priority = parse_u16(f[4].trim())?;
    let weight = parse_u16(f[5].trim())?;
    Ok(Candidate {
        name: String::from(name),
        addr,
        port,
        rp,
        priority,
        weight,
    })
}

/// 链上的入表结果。`Refreshed` 表示同内容重复通告，表未变。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngestOutcome {
    Accepted,
    Refreshed,
}

/// 发现链：端点身份唯一的候选表。
#[derive(Clone, Debug)]
pub struct DiscoverChain {
    cands: Vec<Candidate>,
    accepted: u32,
    refreshed: u32,
}

impl DiscoverChain {
    pub fn new() -> DiscoverChain {
        DiscoverChain {
            cands: Vec::with_capacity(CHAIN_CAP),
            accepted: 0,
            refreshed: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.cands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cands.is_empty()
    }

    pub fn at(&self, idx: usize) -> Option<&Candidate> {
        self.cands.get(idx)
    }

    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    pub fn refreshed(&self) -> u32 {
        self.refreshed
    }

    fn index_of_key(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.cands.len() {
            if let Some(c) = self.cands.get(i) {
                if c.key32() == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按端点身份查候选。
    pub fn find(&self, key: u32) -> Option<&Candidate> {
        match self.index_of_key(key) {
            Some(i) => self.cands.get(i),
            None => None,
        }
    }

    /// 摄入一行通告。同键同内容幂等（别名收敛到先到的那条），
    /// 同键异内容（优先级/权重不同）显式冲突。
    pub fn ingest(&mut self, line: &str) -> Result<IngestOutcome, IppChainError> {
        let c = parse_candidate(line)?;
        let key = c.key32();
        match self.index_of_key(key) {
            Some(idx) => {
                let same = match self.cands.get(idx) {
                    // 显示名不参与判定：同一端点的别名不是冲突，而是同一条
                    // 事实的第二次陈述。优先级/权重变了才是真冲突。
                    Some(old) => old.digest() == c.digest(),
                    None => false,
                };
                if same {
                    self.refreshed = self.refreshed.saturating_add(1);
                    Ok(IngestOutcome::Refreshed)
                } else {
                    // 同一端点被通告成不同的优先级/权重：单源原则要求
                    // 由调用方裁决，模块自己绝不悄悄覆盖。
                    Err(IppChainError::DuplicateKey)
                }
            }
            None => {
                if self.cands.len() >= CHAIN_CAP {
                    return Err(IppChainError::TableFull);
                }
                self.cands.push(c);
                self.accepted = self.accepted.saturating_add(1);
                Ok(IngestOutcome::Accepted)
            }
        }
    }

    /// 确定性排序视图：priority 升序、weight 降序、端点键升序。
    ///
    /// 插入排序在这里是刻意的：比较器只依赖三个整型字段，
    /// 结果与元素原始顺序无关，因此「同样的集合」必然得到同样的序列。
    pub fn ordered(&self) -> Vec<Candidate> {
        let mut v: Vec<Candidate> = Vec::with_capacity(self.cands.len());
        let mut i = 0usize;
        while i < self.cands.len() {
            if let Some(c) = self.cands.get(i) {
                v.push(c.clone());
            }
            i += 1;
        }
        let mut a = 1usize;
        while a < v.len() {
            let mut b = a;
            while b > 0 {
                let prev = match v.get(b - 1) {
                    Some(x) => x,
                    None => break,
                };
                let cur = match v.get(b) {
                    Some(x) => x,
                    None => break,
                };
                let prev_first = prev.priority < cur.priority
                    || (prev.priority == cur.priority && prev.weight > cur.weight)
                    || (prev.priority == cur.priority
                        && prev.weight == cur.weight
                        && prev.key32() <= cur.key32());
                if prev_first {
                    break;
                }
                v.swap(b - 1, b);
                b -= 1;
            }
            a += 1;
        }
        v
    }

    /// 链摘要（按排序视图）。与插入顺序无关，只反映端点集合本身。
    pub fn digest(&self) -> u64 {
        let v = self.ordered();
        let mut h = fnv_init();
        h = fnv_mix(h, v.len() as u64);
        let mut i = 0usize;
        while i < v.len() {
            if let Some(c) = v.get(i) {
                h = fnv_mix(h, c.key32() as u64);
                h = fnv_mix(h, c.priority as u64);
                h = fnv_mix(h, c.weight as u64);
            }
            i += 1;
        }
        h
    }
}

/// B12 快照条目：键 + 摘要。摘要就是候选摘要，联签只引用它。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B12Entry {
    pub key: u32,
    pub digest: u64,
}

/// B12 快照表（可冻结）。冻结后同摘要重放幂等，异摘要 `EntryFrozen`。
#[derive(Clone, Debug)]
pub struct B12Table {
    entries: Vec<B12Entry>,
    frozen: bool,
}

impl B12Table {
    pub fn new() -> B12Table {
        B12Table {
            entries: Vec::with_capacity(B12_CAP),
            frozen: false,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    pub fn at(&self, idx: usize) -> Option<B12Entry> {
        self.entries.get(idx).copied()
    }

    fn index_of(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按键取摘要。
    pub fn get(&self, key: u32) -> Option<u64> {
        match self.index_of(key) {
            Some(i) => match self.entries.get(i) {
                Some(e) => Some(e.digest),
                None => None,
            },
            None => None,
        }
    }

    /// 登记一条快照。返回是否真的写入了新内容。
    pub fn plant(&mut self, key: u32, digest: u64) -> Result<bool, IppChainError> {
        match self.index_of(key) {
            Some(idx) => {
                let same = match self.entries.get(idx) {
                    Some(e) => e.digest == digest,
                    None => false,
                };
                if same {
                    // 冻结与否都不算写入：内容一模一样，写它没有意义。
                    return Ok(false);
                }
                if self.frozen {
                    return Err(IppChainError::EntryFrozen);
                }
                if let Some(e) = self.entries.get_mut(idx) {
                    e.digest = digest;
                }
                Ok(true)
            }
            None => {
                if self.frozen {
                    return Err(IppChainError::EntryFrozen);
                }
                if self.entries.len() >= B12_CAP {
                    return Err(IppChainError::TableFull);
                }
                self.entries.push(B12Entry { key, digest });
                Ok(true)
            }
        }
    }

    /// 冻结：之后只允许同摘要重放。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 表摘要（表序）。签前签后必须逐位相同。
    pub fn digest(&self) -> u64 {
        let mut h = fnv_init();
        h = fnv_mix(h, self.entries.len() as u64);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_mix(h, e.key as u64);
                h = fnv_mix(h, e.digest);
            }
            i += 1;
        }
        h
    }
}

/// 一条联签记录。字段全部只读可见，便于「从日志恢复后再校验」这条真实路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealRecord {
    pub key: u32,
    pub b12_digest: u64,
    pub chain_digest: u64,
    pub signer: u32,
    pub prev: u64,
    pub self_digest: u64,
}

/// 联签账：前向哈希链。
#[derive(Clone, Debug)]
pub struct SealLedger {
    seals: Vec<SealRecord>,
}

impl SealLedger {
    pub fn new() -> SealLedger {
        SealLedger {
            seals: Vec::with_capacity(SEAL_CAP),
        }
    }

    pub fn len(&self) -> usize {
        self.seals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seals.is_empty()
    }

    pub fn at(&self, idx: usize) -> Option<SealRecord> {
        self.seals.get(idx).copied()
    }

    /// 链头（最后一条签的自摘要；空账为 0）。
    pub fn head(&self) -> u64 {
        match self.seals.last() {
            Some(r) => r.self_digest,
            None => 0,
        }
    }

    /// 从持久化日志恢复。恢复后必须再跑 `verify_chain`——恢复本身不做信任。
    pub fn restore(recs: &[SealRecord]) -> Result<SealLedger, IppChainError> {
        if recs.len() > SEAL_CAP {
            return Err(IppChainError::SealFull);
        }
        let mut seals: Vec<SealRecord> = Vec::with_capacity(recs.len());
        let mut i = 0usize;
        while i < recs.len() {
            if let Some(r) = recs.get(i) {
                seals.push(*r);
            }
            i += 1;
        }
        Ok(SealLedger { seals })
    }

    /// 对某个 B12 条目联签。表未冻结、键缺失、账满都显式拒绝。
    pub fn seal(
        &mut self,
        key: u32,
        table: &B12Table,
        chain: &DiscoverChain,
        signer: u32,
    ) -> Result<SealRecord, IppChainError> {
        if signer == 0 {
            return Err(IppChainError::BadNumber);
        }
        if !table.frozen() {
            return Err(IppChainError::NotFrozen);
        }
        let b12_digest = match table.get(key) {
            Some(d) => d,
            None => return Err(IppChainError::KeyMissing),
        };
        if self.seals.len() >= SEAL_CAP {
            return Err(IppChainError::SealFull);
        }
        let prev = self.head();
        let chain_digest = chain.digest();
        let mut h = fnv_init();
        h = fnv_mix(h, prev);
        h = fnv_mix(h, key as u64);
        h = fnv_mix(h, b12_digest);
        h = fnv_mix(h, chain_digest);
        h = fnv_mix(h, signer as u64);
        let rec = SealRecord {
            key,
            b12_digest,
            chain_digest,
            signer,
            prev,
            self_digest: h,
        };
        self.seals.push(rec);
        Ok(rec)
    }

    /// 校验整条哈希链。任何一处被改写都返回 `Tamper`。
    pub fn verify_chain(&self) -> Result<(), IppChainError> {
        let mut expected_prev = 0u64;
        let mut i = 0usize;
        while i < self.seals.len() {
            let r = match self.seals.get(i) {
                Some(v) => *v,
                None => break,
            };
            if r.prev != expected_prev {
                return Err(IppChainError::Tamper);
            }
            let mut h = fnv_init();
            h = fnv_mix(h, r.prev);
            h = fnv_mix(h, r.key as u64);
            h = fnv_mix(h, r.b12_digest);
            h = fnv_mix(h, r.chain_digest);
            h = fnv_mix(h, r.signer as u64);
            if h != r.self_digest {
                return Err(IppChainError::Tamper);
            }
            expected_prev = r.self_digest;
            i += 1;
        }
        Ok(())
    }

    /// 校验单条签：链完整 + B12 摘要未被改写。
    pub fn verify_seal(&self, idx: usize, table: &B12Table) -> Result<(), IppChainError> {
        let r = match self.seals.get(idx) {
            Some(v) => *v,
            None => return Err(IppChainError::KeyMissing),
        };
        let cur = match table.get(r.key) {
            Some(d) => d,
            None => return Err(IppChainError::KeyMissing),
        };
        if cur != r.b12_digest {
            return Err(IppChainError::DigestMismatch);
        }
        self.verify_chain()
    }

    /// 账本摘要（供日志对账）。
    pub fn digest(&self) -> u64 {
        let mut h = fnv_init();
        h = fnv_mix(h, self.seals.len() as u64);
        let mut i = 0usize;
        while i < self.seals.len() {
            if let Some(r) = self.seals.get(i) {
                h = fnv_mix(h, r.self_digest);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 联签审计：J1 的度量本体
// ---------------------------------------------------------------------------

/// 一次联签审计的结果。`digest_stable` 就是「B12 条目零改写」的正面度量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointAudit {
    pub candidates: usize,
    pub b12_entries: usize,
    pub seals: usize,
    pub digest_stable: bool,
    pub verified: usize,
    pub replays: usize,
}

/// 从若干行通告走完「发现 → 快照 → 冻结 → 联签 → 复核 → 重放」全链。
pub fn joint_sign_audit(lines: &[&str]) -> Result<JointAudit, IppChainError> {
    let mut chain = DiscoverChain::new();
    let mut i = 0usize;
    while i < lines.len() {
        if let Some(l) = lines.get(i) {
            chain.ingest(l)?;
        }
        i += 1;
    }
    let mut table = B12Table::new();
    let ordered = chain.ordered();
    let mut k = 0usize;
    while k < ordered.len() {
        if let Some(c) = ordered.get(k) {
            table.plant(c.key32(), c.digest())?;
        }
        k += 1;
    }
    table.freeze();
    let before = table.digest();

    let mut ledger = SealLedger::new();
    let mut s = 0usize;
    while s < ordered.len() {
        if let Some(c) = ordered.get(s) {
            ledger.seal(c.key32(), &table, &chain, 0x5e_a1)?;
        }
        s += 1;
    }

    // 重放：同一批通告再来一遍，发现链只记 Refreshed，B12 只回 Ok(false)。
    let mut replays = 0usize;
    let mut j = 0usize;
    while j < lines.len() {
        if let Some(l) = lines.get(j) {
            if chain.ingest(l)? == IngestOutcome::Refreshed {
                replays += 1;
            }
        }
        j += 1;
    }
    let ordered2 = chain.ordered();
    let mut m = 0usize;
    while m < ordered2.len() {
        if let Some(c) = ordered2.get(m) {
            if !table.plant(c.key32(), c.digest())? {
                replays += 1;
            }
        }
        m += 1;
    }

    let mut verified = 0usize;
    let mut v = 0usize;
    while v < ledger.len() {
        ledger.verify_seal(v, &table)?;
        verified += 1;
        v += 1;
    }
    let after = table.digest();
    Ok(JointAudit {
        candidates: chain.len(),
        b12_entries: table.len(),
        seals: ledger.len(),
        digest_stable: before == after,
        verified,
        replays,
    })
}

// ---------------------------------------------------------------------------
// 注入核：16 类坏样本
// ---------------------------------------------------------------------------

/// 造一张已冻结、装好 `n` 个条目的 B12 表（键 1..=n）。
fn frozen_table(n: usize) -> Result<B12Table, IppChainError> {
    let mut t = B12Table::new();
    let mut i = 1usize;
    while i <= n {
        t.plant(i as u32, 0x1000 + i as u64)?;
        i += 1;
    }
    t.freeze();
    Ok(t)
}

/// 注入核：16 类坏样本 → 16 个具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 16usize;

    if parse_candidate("   ").err() == Some(IppChainError::EmptyRecord) {
        caught += 1;
    }
    if parse_candidate("|10.0.0.1|631|ipp/print|10|5").err() == Some(IppChainError::EmptyName) {
        caught += 1;
    }
    let long_name = "n".repeat(NAME_MAX + 1);
    let long_line = format!("{}|10.0.0.1|631|ipp/print|10|5", long_name);
    if parse_candidate(&long_line).err() == Some(IppChainError::NameTooLong) {
        caught += 1;
    }
    if parse_candidate("a|10.0.0.1|631").err() == Some(IppChainError::BadFieldCount) {
        caught += 1;
    }
    if parse_candidate("a|10.0.0.256|631|ipp/print|10|5").err() == Some(IppChainError::BadAddr) {
        caught += 1;
    }
    if parse_candidate("a|10.0.0.1|0|ipp/print|10|5").err() == Some(IppChainError::ZeroPort) {
        caught += 1;
    }
    if parse_candidate("a|10.0.0.1|631|ipp/print|x|5").err() == Some(IppChainError::BadNumber) {
        caught += 1;
    }
    if parse_candidate("a|10.0.0.1|631|ip p/print|10|5").err() == Some(IppChainError::BadPath) {
        caught += 1;
    }

    // 发现链满
    let mut chain = DiscoverChain::new();
    let mut i = 0usize;
    while i < CHAIN_CAP {
        let line = format!("p{}|10.0.1.{}|631|ipp/print|10|5", i, i + 1);
        let _ = chain.ingest(&line);
        i += 1;
    }
    let overflow_line = "last|10.0.2.1|631|ipp/print|10|5";
    if chain.ingest(overflow_line).err() == Some(IppChainError::TableFull) {
        caught += 1;
    }
    // 同端点异内容（权重不同）：单源冲突
    let mut dup = DiscoverChain::new();
    let ok = match dup.ingest("A|10.0.3.1|631|ipp/print|10|5") {
        Ok(_) => dup.ingest("B|10.0.3.1|631|ipp/print|10|6").err() == Some(IppChainError::DuplicateKey),
        Err(_) => false,
    };
    if ok {
        caught += 1;
    }
    // 冻结后改摘要
    let mut frozen = match frozen_table(2) {
        Ok(v) => v,
        Err(_) => B12Table::new(),
    };
    if frozen.plant(1, 0xdead).err() == Some(IppChainError::EntryFrozen) {
        caught += 1;
    }
    // 未冻结就联签
    let mut raw = B12Table::new();
    let _ = raw.plant(9, 0x99);
    let mut l0 = SealLedger::new();
    let c0 = DiscoverChain::new();
    if l0.seal(9, &raw, &c0, 1).err() == Some(IppChainError::NotFrozen) {
        caught += 1;
    }
    // 键缺失
    let mut l1 = SealLedger::new();
    if l1.seal(77, &frozen, &c0, 1).err() == Some(IppChainError::KeyMissing) {
        caught += 1;
    }
    // 摘要替换
    let mut l2 = SealLedger::new();
    let _ = l2.seal(1, &frozen, &c0, 1);
    let mut other = B12Table::new();
    let _ = other.plant(1, 0xbeef);
    if l2.verify_seal(0, &other).err() == Some(IppChainError::DigestMismatch) {
        caught += 1;
    }
    // 联签账满：8 条已满，第 9 条（键仍取已存在的 1，确保是「满」而不是「缺」）
    let big = match frozen_table(SEAL_CAP + 1) {
        Ok(v) => v,
        Err(_) => B12Table::new(),
    };
    let mut l3 = SealLedger::new();
    let mut s = 1usize;
    while s <= SEAL_CAP {
        let _ = l3.seal(s as u32, &big, &c0, 1);
        s += 1;
    }
    if l3.seal(1, &big, &c0, 1).err() == Some(IppChainError::SealFull) {
        caught += 1;
    }
    // 篡改：从日志恢复时改动一条
    let mut recs: Vec<SealRecord> = Vec::new();
    let mut t = 0usize;
    while t < l3.len() {
        if let Some(r) = l3.at(t) {
            recs.push(r);
        }
        t += 1;
    }
    if let Some(first) = recs.first_mut() {
        first.b12_digest ^= 0x1;
    }
    let broken = SealLedger::restore(&recs);
    if match broken {
        Ok(v) => v.verify_chain().err() == Some(IppChainError::Tamper),
        Err(_) => false,
    } {
        caught += 1;
    }
    (caught.min(total), total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 一组固定的联签发现在场样本（覆盖三种优先级 / 一段别名重复）。
pub const AUDIT_LINES: [&str; 5] = [
    "OfficeJet|10.0.5.11|631|ipp/print|10|5",
    "OfficeJet-2F|10.0.5.11|631|ipp/print|10|5",
    "ScanPro|10.0.5.12|631|ipp/scan|20|8",
    "LaserMFP|10.0.5.13|631|ipp/print|5|1",
    "FaxGate|10.0.5.14|80|ipp/fax|30|2",
];

pub fn run_unx50149_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50149");

    // J1-1 联签登记，B12 条目零改写（判据原文）
    let audit = joint_sign_audit(&AUDIT_LINES);
    let ok = match audit {
        Ok(a) => a.digest_stable && a.verified == a.seals && a.seals == a.b12_entries && a.b12_entries > 0,
        Err(_) => false,
    };
    let det = match audit {
        Ok(a) => format!(
            "cand={} b12={} seal={} verify={} stable={}",
            a.candidates, a.b12_entries, a.seals, a.verified, a.digest_stable
        ),
        Err(e) => format!("audit err {}", e.code()),
    };
    s.add_dyn("50149-J1-联签零改写", ok, &det);

    // J1-2 别名去重：两行不同实例名指向同一端点，只出一条候选
    let ok = match joint_sign_audit(&AUDIT_LINES) {
        Ok(a) => a.candidates == 4 && a.b12_entries == 4,
        Err(_) => false,
    };
    s.add("50149-J1-端点唯一", ok, if ok { "" } else { "别名产生了重复候选" });

    // J1-3 解析往返：rp 归一化补前导 '/'，各字段逐位还原
    let p = parse_candidate("Edge|192.168.1.9|631|ipp/print|10|5");
    let ok = match p {
        Ok(c) => {
            c.addr == 0xc0a8_0109
                && c.port == 631
                && c.rp == "/ipp/print"
                && c.priority == 10
                && c.weight == 5
                && c.name == "Edge"
        }
        Err(_) => false,
    };
    s.add("50149-J1-解析往返", ok, if ok { "" } else { "字段还原漂移" });

    // J1-4 同内容重复通告幂等：链长不变、摘要不变
    let mut idem = DiscoverChain::new();
    let line = "One|10.0.6.1|631|ipp/print|10|5";
    let a = idem.ingest(line);
    let d1 = idem.digest();
    let b = idem.ingest(line);
    let ok = a == Ok(IngestOutcome::Accepted)
        && b == Ok(IngestOutcome::Refreshed)
        && idem.len() == 1
        && idem.digest() == d1;
    s.add("50149-J1-通告幂等", ok, if ok { "" } else { "重复通告改写了链" });

    // J1-5 排序确定性：与插入顺序无关，只由优先级/权重/键决定
    let build = |rev: bool| -> u64 {
        let mut c = DiscoverChain::new();
        if rev {
            let mut i = AUDIT_LINES.len();
            while i > 0 {
                i -= 1;
                if let Some(l) = AUDIT_LINES.get(i) {
                    let _ = c.ingest(l);
                }
            }
        } else {
            let mut i = 0usize;
            while i < AUDIT_LINES.len() {
                if let Some(l) = AUDIT_LINES.get(i) {
                    let _ = c.ingest(l);
                }
                i += 1;
            }
        }
        c.digest()
    };
    let ok = build(false) == build(true);
    s.add("50149-J1-摘要序无关", ok, if ok { "" } else { "摘要依赖插入顺序" });

    // J1-6 显示名不进摘要：同端点不同别名链路摘要相同（单源口径）
    let mut c1 = DiscoverChain::new();
    let _ = c1.ingest("Alpha|10.0.7.1|631|ipp/print|10|5");
    let mut c2 = DiscoverChain::new();
    let _ = c2.ingest("Beta|10.0.7.1|631|ipp/print|10|5");
    let ok = c1.digest() == c2.digest() && c1.at(0).is_some();
    s.add("50149-J1-别名不入摘要", ok, if ok { "" } else { "摘要被显示名污染" });

    // J1-7 冻结语义：同摘要回 Ok(false)，异摘要 EntryFrozen
    let mut ft = B12Table::new();
    let _ = ft.plant(5, 0x1111);
    ft.freeze();
    let again = ft.plant(5, 0x1111);
    let change = ft.plant(5, 0x2222);
    let ok = again == Ok(false) && change.err() == Some(IppChainError::EntryFrozen) && ft.len() == 1;
    s.add("50149-J1-冻结语义", ok, if ok { "" } else { "冻结语义错误" });

    // J1-8 联签链完整：篡改一条即 Tamper，不改则 Ok
    let tbl = match frozen_table(2) {
        Ok(v) => v,
        Err(_) => B12Table::new(),
    };
    let mut chain = DiscoverChain::new();
    let _ = chain.ingest("X|10.0.8.1|631|ipp/print|1|1");
    let mut led = SealLedger::new();
    let _ = led.seal(1, &tbl, &chain, 9);
    let _ = led.seal(2, &tbl, &chain, 9);
    let clean = led.verify_chain();
    let mut recs: Vec<SealRecord> = Vec::new();
    if let Some(r) = led.at(1) {
        recs.push(r);
    }
    if let Some(r) = led.at(1) {
        let mut bad = r;
        bad.prev ^= 0x2;
        let mut restored: Vec<SealRecord> = Vec::new();
        if let Some(r0) = led.at(0) {
            restored.push(r0);
        }
        restored.push(bad);
        let broken = match SealLedger::restore(&restored) {
            Ok(v) => v.verify_chain().err() == Some(IppChainError::Tamper),
            Err(_) => false,
        };
        let ok = clean.is_ok() && broken && recs.len() == 1;
        s.add("50149-J1-链篡改必抓", ok, if ok { "" } else { "篡改未被发现" });
    } else {
        s.add("50149-J1-链篡改必抓", false, "seal missing");
    }

    // J1-9 摘要替换必抓
    let mut l2 = SealLedger::new();
    let _ = l2.seal(1, &tbl, &chain, 9);
    let mut swapped = B12Table::new();
    let _ = swapped.plant(1, 0x7777);
    let ok = l2.verify_seal(0, &swapped).err() == Some(IppChainError::DigestMismatch)
        && l2.verify_seal(0, &tbl).is_ok();
    s.add("50149-J1-摘要替换必抓", ok, if ok { "" } else { "摘要替换未被发现" });

    // J1-10 三种容量拒绝互相独立
    let mut cap = DiscoverChain::new();
    let mut i = 0usize;
    while i < CHAIN_CAP {
        let line = format!("c{}|10.0.9.{}|631|ipp/print|1|1", i, i + 1);
        let _ = cap.ingest(&line);
        i += 1;
    }
    let chain_full = cap.ingest("cx|10.0.10.1|631|ipp/print|1|1").err() == Some(IppChainError::TableFull);
    let mut bt = B12Table::new();
    let mut k = 0usize;
    while k < B12_CAP {
        let _ = bt.plant(k as u32 + 1, 0x10 + k as u64);
        k += 1;
    }
    let b12_full = bt.plant(999, 1).err() == Some(IppChainError::TableFull);
    let mut lg = SealLedger::new();
    bt.freeze();
    let mut s2 = 1usize;
    while s2 <= SEAL_CAP {
        let _ = lg.seal(s2 as u32, &bt, &cap, 3);
        s2 += 1;
    }
    let seal_full = lg.seal(1, &bt, &cap, 3).err() == Some(IppChainError::SealFull);
    let ok = chain_full && b12_full && seal_full;
    s.add("50149-J1-容量三分", ok, if ok { "" } else { "容量拒绝互相串味" });

    // J1-11 未冻结联签必拒（不允许对着活表签）
    let mut live = B12Table::new();
    let _ = live.plant(3, 0x33);
    let mut lg2 = SealLedger::new();
    let ok = lg2.seal(3, &live, &chain, 1).err() == Some(IppChainError::NotFrozen)
        && lg2.is_empty();
    s.add("50149-J1-未冻结必拒", ok, if ok { "" } else { "活表被签" });

    // J1-12 非法解析族：前导零 / 广播地址 / 路径穿越
    let ok = parse_candidate("a|10.0.0.01|631|ipp/print|1|1").err() == Some(IppChainError::BadAddr)
        && parse_candidate("a|255.255.255.255|631|ipp/print|1|1").err() == Some(IppChainError::BadAddr)
        && parse_candidate("a|10.0.0.1|631|ipp/../etc|1|1").err() == Some(IppChainError::BadPath)
        && parse_candidate("a|10.0.0.1|0631|ipp/print|1|1").err() == Some(IppChainError::BadNumber);
    s.add("50149-J1-非法必拒", ok, if ok { "" } else { "坏记录被接受" });

    // J1-13 注入核全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50149-J1-注入必抓", ok, &det);

    // J1-14 码值闭集连续
    let mut seen = [false; 17];
    let mut ok = true;
    let mut i = 0usize;
    while i < ALL_ERRORS.len() {
        let off = ALL_ERRORS[i].offset();
        if off == 0 || off as usize >= seen.len() || seen[off as usize] {
            ok = false;
        } else {
            seen[off as usize] = true;
        }
        if ALL_ERRORS[i].code() != SEG_BASE + off as u32 {
            ok = false;
        }
        i += 1;
    }
    s.add("50149-J1-码值闭集", ok, if ok { "" } else { "码值重复或断裂" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50149_checks_green() {
        let set = run_unx50149_checks();
        assert!(set.all_passed(), "UNX-F50149 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50149_alias_is_one_endpoint() {
        let mut c = DiscoverChain::new();
        assert!(c.ingest("A|10.0.0.1|631|ipp/print|1|1").is_ok());
        // 别名同内容：收敛到同一条候选，不新增行。
        assert_eq!(
            c.ingest("B|10.0.0.1|631|ipp/print|1|1"),
            Ok(IngestOutcome::Refreshed)
        );
        assert_eq!(c.len(), 1);
        // 同端点但权重不同：单源冲突。
        assert_eq!(
            c.ingest("C|10.0.0.1|631|ipp/print|1|2").err(),
            Some(IppChainError::DuplicateKey)
        );
    }

    #[test]
    fn unx50149_b12_digest_untouched_by_sealing() {
        let mut t = B12Table::new();
        assert!(t.plant(1, 0xabc).is_ok());
        t.freeze();
        let before = t.digest();
        let mut l = SealLedger::new();
        let c = DiscoverChain::new();
        assert!(l.seal(1, &t, &c, 1).is_ok());
        assert_eq!(before, t.digest());
    }
}
