//! UNX-F50167 Ticket 校验和与防篡改账（0x44100 段）
//!
//! 锚点：UNX-F50167-J1 篡改 ×10 全中；判据锚定 §AI-63 §5。
//!
//! PrintTicket 从渲染进程走到驱动、假脱机、落盘，任何一跳都可能被中间层改写：
//! 多写一个空格、把 `Feature` 顺序换掉、把 `MediaSize` 的值改半格。校验和要回答的
//! 不是「内容对不对」，而是「内容是否与签发时逐字节一致」，所以本模块做的是
//! **链式摘要账**而不是单条 CRC：
//!
//! * 每条记录带三本账——头摘要（seq/id/version）、载荷摘要（字节 + 头摘要）、
//!   链值（上一条链值 + 本条摘要）。删一条、换两条、重放旧条都会破坏链；
//! * 载荷长度单独记账（`declared_len`），截断与追加报 `LengthMismatch`，
//!   与「内容被改」的 `DigestMismatch` 分开——取证时要能区分「少了一截」和
//!   「同一位置换了字节」，两者的修复路径完全不同；
//! * 账本定容（[`MAX_ENTRIES`]）。满时拒绝而不是覆盖：覆盖会让「某页的封条
//!   不见了」变成静默事实，审计无法发现。
//!
//! 摘要算法刻意不用外部 crate：FNV-1a 64 负责字节扩散，位置加权和负责给
//! 「同字节集合换位置」补一刀。两路结果异或进同一个 u64，同样输入两次调用
//! 逐位一致（[`SealLedger::append`] 的判据 50167-J1-确定性 依赖这一点）。
//!
//! 错误闭集：`SEG_BASE + 1 ..= SEG_BASE + 10`，逐项见 [`SealError`]。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x44100 独占 0x20）。
pub const SEG_BASE: u32 = 0x44100;

/// 链首种子。取 FNV-1a 64 的偏移基数：空账的链尾即此值，便于对账。
pub const CHAIN_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单条载荷上限。A4 的 PrintTicket 实测 < 2 KiB，4 KiB 留出一倍余量。
pub const MAX_PAYLOAD: usize = 4096;

/// 账本容量。64 页足够覆盖一次打印任务；超出即要求滚动封存，不允许静默丢弃。
pub const MAX_ENTRIES: usize = 64;

pub const E_EMPTY: u16 = 1;
pub const E_LIMIT: u16 = 2;
pub const E_LENGTH: u16 = 3;
pub const E_DIGEST: u16 = 4;
pub const E_HEADER: u16 = 5;
pub const E_CHAIN: u16 = 6;
pub const E_GAP: u16 = 7;
pub const E_REPLAY: u16 = 8;
pub const E_VERSION: u16 = 9;
pub const E_CAPACITY: u16 = 10;

/// 封条域错误。`code()` 恒等于 `SEG_BASE + 偏移`，没有第二条编码路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    EmptyPayload,
    Limit,
    LengthMismatch,
    DigestMismatch,
    HeaderMismatch,
    ChainBreak,
    SequenceGap,
    Replay,
    BadVersion,
    Capacity,
}

impl SealError {
    pub fn offset(self) -> u16 {
        match self {
            SealError::EmptyPayload => E_EMPTY,
            SealError::Limit => E_LIMIT,
            SealError::LengthMismatch => E_LENGTH,
            SealError::DigestMismatch => E_DIGEST,
            SealError::HeaderMismatch => E_HEADER,
            SealError::ChainBreak => E_CHAIN,
            SealError::SequenceGap => E_GAP,
            SealError::Replay => E_REPLAY,
            SealError::BadVersion => E_VERSION,
            SealError::Capacity => E_CAPACITY,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.offset() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            SealError::EmptyPayload => "sealed payload is empty",
            SealError::Limit => "payload or sequence limit exceeded",
            SealError::LengthMismatch => "payload length differs from seal",
            SealError::DigestMismatch => "payload digest differs from seal",
            SealError::HeaderMismatch => "seq/id/version header differs from seal",
            SealError::ChainBreak => "chain value does not follow predecessor",
            SealError::SequenceGap => "sequence numbers not contiguous",
            SealError::Replay => "duplicate seal digest detected",
            SealError::BadVersion => "ticket schema version outside 1..=2",
            SealError::Capacity => "seal ledger is full",
        }
    }
}

/// FNV-1a 64：逐字节异或后乘素数，对单字节翻转敏感。
fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => 0,
        };
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 位置加权和：第 i 字节乘 (i+1)，专门抓「同字节集合换位置」这类重排。
fn weighted(bytes: &[u8]) -> u64 {
    let mut sum = 0u64;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => 0,
        };
        sum = sum.wrapping_add((b as u64 + 1).wrapping_mul(i as u64 + 1));
        i += 1;
    }
    sum
}

/// 头摘要：覆盖序号、票号、schema 版本。三项任一改动都必须换摘要。
pub fn header_tag(seq: u32, ticket_id: u32, version: u16) -> u64 {
    let mut h = fnv1a(CHAIN_SEED, &seq.to_be_bytes());
    h = fnv1a(h, &ticket_id.to_be_bytes());
    fnv1a(h, &version.to_be_bytes())
}

/// 载荷摘要：在头摘要之上再吃载荷字节，并用位置加权做二次混合。
pub fn payload_digest(payload: &[u8], tag: u64) -> u64 {
    let h = fnv1a(tag ^ CHAIN_SEED, payload);
    h ^ weighted(payload).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

/// 链值：由前一链值与本条摘要共同决定，保证改任一条都会让后续对账失败。
pub fn chain_of(prev_chain: u64, digest: u64) -> u64 {
    let mut h = fnv1a(prev_chain, &digest.to_be_bytes());
    h = fnv1a(h, &prev_chain.to_be_bytes());
    h
}

/// 一条封条记录。`payload` 与实际字节共存，因此校验时必须同时看长度账与摘要账。
#[derive(Clone, Debug, PartialEq)]
pub struct SealEntry {
    pub seq: u32,
    pub ticket_id: u32,
    pub version: u16,
    pub declared_len: u32,
    pub payload: Vec<u8>,
    pub header_tag: u64,
    pub digest: u64,
    pub chain: u64,
}

impl SealEntry {
    /// 由载荷与前一链值签发一条新封条（不做容量/版本策略，那是账本的职责）。
    pub fn new(seq: u32, ticket_id: u32, version: u16, payload: &[u8], prev_chain: u64) -> SealEntry {
        let tag = header_tag(seq, ticket_id, version);
        let digest = payload_digest(payload, tag);
        SealEntry {
            seq,
            ticket_id,
            version,
            declared_len: payload.len() as u32,
            payload: payload.to_vec(),
            header_tag: tag,
            digest,
            chain: chain_of(prev_chain, digest),
        }
    }

    /// 只验本条的自洽性（头、长度、摘要），链与序号由账本验。
    pub fn verify_shape(&self) -> Result<(), SealError> {
        if header_tag(self.seq, self.ticket_id, self.version) != self.header_tag {
            return Err(SealError::HeaderMismatch);
        }
        if self.payload.len() as u32 != self.declared_len {
            return Err(SealError::LengthMismatch);
        }
        if payload_digest(&self.payload, self.header_tag) != self.digest {
            return Err(SealError::DigestMismatch);
        }
        Ok(())
    }
}

/// 封条账：按签发顺序保存 SealEntry，链尾随每次 append 前移。
#[derive(Clone, Debug)]
pub struct SealLedger {
    entries: Vec<SealEntry>,
    last_chain: u64,
    next_seq: u32,
}

impl SealLedger {
    /// 空账：链尾等于链首种子，序号从 0 起。
    pub const fn new() -> SealLedger {
        SealLedger {
            entries: Vec::new(),
            last_chain: CHAIN_SEED,
            next_seq: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 当前链尾。空账返回 [`CHAIN_SEED`]。
    pub fn chain_tip(&self) -> u64 {
        self.last_chain
    }

    /// 按序号只读一条封条。调用方拿到引用后仍应先 `verify_entry`。
    pub fn inspect(&self, at: usize) -> Option<&SealEntry> {
        self.entries.get(at)
    }

    /// 签发并追加一条封条，返回新的链尾。
    ///
    /// 拒绝顺序固定（空 → 超限 → 版本 → 容量 → 序号耗尽），保证坏样本诊断码
    /// 可复现，不会因为检查顺序漂移而改码。
    pub fn append(&mut self, ticket_id: u32, version: u16, payload: &[u8]) -> Result<u64, SealError> {
        if payload.is_empty() {
            return Err(SealError::EmptyPayload);
        }
        if payload.len() > MAX_PAYLOAD {
            return Err(SealError::Limit);
        }
        if version != 1 && version != 2 {
            return Err(SealError::BadVersion);
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(SealError::Capacity);
        }
        if self.next_seq == u32::MAX {
            return Err(SealError::Limit);
        }
        let e = SealEntry::new(self.next_seq, ticket_id, version, payload, self.last_chain);
        let chain = e.chain;
        self.entries.push(e);
        self.next_seq += 1;
        self.last_chain = chain;
        Ok(chain)
    }

    /// 校验第 `at` 条：序号 → 头 → 长度 → 摘要 → 链，首个不符即返回。
    pub fn verify_entry(&self, at: usize) -> Result<(), SealError> {
        let e = match self.entries.get(at) {
            Some(v) => v,
            None => return Err(SealError::SequenceGap),
        };
        if e.seq != at as u32 {
            return Err(SealError::SequenceGap);
        }
        e.verify_shape()?;
        let prev = if at == 0 {
            CHAIN_SEED
        } else {
            match self.entries.get(at - 1) {
                Some(p) => p.chain,
                None => return Err(SealError::SequenceGap),
            }
        };
        if chain_of(prev, e.digest) != e.chain {
            return Err(SealError::ChainBreak);
        }
        Ok(())
    }

    /// 全账校验：逐条验链，再查重放。空账视为通过（签发前无内容可篡改）。
    pub fn verify_all(&self) -> Result<(), SealError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            self.verify_entry(i)?;
            i += 1;
        }
        self.detect_replay()
    }

    /// 重放检测：两条封条摘要完全相同即视为重放（同一票同一序不可能两次签发）。
    pub fn detect_replay(&self) -> Result<(), SealError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let mut j = i + 1;
            let a = match self.entries.get(i) {
                Some(v) => v,
                None => return Err(SealError::SequenceGap),
            };
            while j < self.entries.len() {
                let b = match self.entries.get(j) {
                    Some(v) => v,
                    None => return Err(SealError::SequenceGap),
                };
                if a.digest == b.digest {
                    return Err(SealError::Replay);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 离线镜像恢复入口：把外部记录原样入账，**不**推进链尾。
    ///
    /// 为什么留这个口子：取证要把磁盘上的旧账拼回内存再逐条验，如果这里顺手
    /// 修正链尾，篡改就被自己洗白了。调用方必须随后 `verify_all`。
    pub fn restore_raw(&mut self, entry: SealEntry) -> Result<(), SealError> {
        if self.entries.len() >= MAX_ENTRIES {
            return Err(SealError::Capacity);
        }
        self.entries.push(entry);
        Ok(())
    }

    // ---------------------------------------------------------------------
    // 篡改模拟：只供注入核与取证演练使用，每一步都是真实的内存改写。
    // ---------------------------------------------------------------------

    /// 翻转载荷某字节的 bit6。
    pub fn flip_payload(&mut self, at: usize, pos: usize) -> Result<(), SealError> {
        let e = match self.entries.get_mut(at) {
            Some(v) => v,
            None => return Err(SealError::SequenceGap),
        };
        let b = match e.payload.get_mut(pos) {
            Some(v) => v,
            None => return Err(SealError::LengthMismatch),
        };
        *b ^= 0x40;
        Ok(())
    }

    /// 把载荷截到 `new_len`（只减不增）。
    pub fn cut_payload(&mut self, at: usize, new_len: usize) -> Result<(), SealError> {
        let e = match self.entries.get_mut(at) {
            Some(v) => v,
            None => return Err(SealError::SequenceGap),
        };
        if new_len > e.payload.len() {
            return Err(SealError::LengthMismatch);
        }
        e.payload.truncate(new_len);
        Ok(())
    }

    /// 在载荷尾部追加一个字节。
    pub fn push_payload(&mut self, at: usize, byte: u8) -> Result<(), SealError> {
        let e = match self.entries.get_mut(at) {
            Some(v) => v,
            None => return Err(SealError::SequenceGap),
        };
        if e.payload.len() >= MAX_PAYLOAD {
            return Err(SealError::Limit);
        }
        e.payload.push(byte);
        Ok(())
    }

    pub fn rewrite_id(&mut self, at: usize, id: u32) -> Result<(), SealError> {
        match self.entries.get_mut(at) {
            Some(e) => {
                e.ticket_id = id;
                Ok(())
            }
            None => Err(SealError::SequenceGap),
        }
    }

    pub fn rewrite_version(&mut self, at: usize, version: u16) -> Result<(), SealError> {
        match self.entries.get_mut(at) {
            Some(e) => {
                e.version = version;
                Ok(())
            }
            None => Err(SealError::SequenceGap),
        }
    }

    pub fn rewrite_seq(&mut self, at: usize, seq: u32) -> Result<(), SealError> {
        match self.entries.get_mut(at) {
            Some(e) => {
                e.seq = seq;
                Ok(())
            }
            None => Err(SealError::SequenceGap),
        }
    }

    /// 直接腐蚀头摘要（模拟落盘时 bit rot）。
    pub fn corrode_tag(&mut self, at: usize) -> Result<(), SealError> {
        match self.entries.get_mut(at) {
            Some(e) => {
                e.header_tag ^= 1;
                Ok(())
            }
            None => Err(SealError::SequenceGap),
        }
    }

    pub fn corrode_digest(&mut self, at: usize) -> Result<(), SealError> {
        match self.entries.get_mut(at) {
            Some(e) => {
                e.digest ^= 0x8000_0000_0000_0000;
                Ok(())
            }
            None => Err(SealError::SequenceGap),
        }
    }

    pub fn corrode_chain(&mut self, at: usize) -> Result<(), SealError> {
        match self.entries.get_mut(at) {
            Some(e) => {
                e.chain ^= 0x0000_0000_0000_0001;
                Ok(())
            }
            None => Err(SealError::SequenceGap),
        }
    }

    /// 删除一条封条（模拟中间层吞页）。
    pub fn remove(&mut self, at: usize) -> Result<SealEntry, SealError> {
        if at >= self.entries.len() {
            return Err(SealError::SequenceGap);
        }
        Ok(self.entries.remove(at))
    }

    /// 交换两条封条（模拟落盘顺序错乱）。
    pub fn swap(&mut self, a: usize, b: usize) -> Result<(), SealError> {
        if a >= self.entries.len() || b >= self.entries.len() {
            return Err(SealError::SequenceGap);
        }
        self.entries.swap(a, b);
        Ok(())
    }
}

/// 造一本 4 条的干净账：内容取自真实 PrintTicket 片段，便于对账时肉眼比对。
fn seed_ledger() -> Result<SealLedger, SealError> {
    let mut l = SealLedger::new();
    l.append(0x50167, 2, b"Feature:PageMediaSize;Option:ISOA4;W=210;H=297")?;
    l.append(0x50167, 2, b"Feature:PageOrientation;Option:Portrait")?;
    l.append(0x50167, 2, b"Feature:PageResolution;Option:600x600")?;
    l.append(0x50167, 2, b"Feature:JobInputBin;Option:AutoSelect")?;
    Ok(l)
}

/// 在干净账上做一次篡改，并断言 `verify_all` 命中指定码。
fn expect_after<F>(f: F, want: SealError) -> bool
where
    F: FnOnce(&mut SealLedger) -> Result<(), SealError>,
{
    match seed_ledger() {
        Ok(mut l) => match f(&mut l) {
            Ok(()) => l.verify_all().err() == Some(want),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// 注入核：10 类真实篡改逐类断言具体诊断码，返回 (命中, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    if expect_after(|l| l.flip_payload(1, 3), SealError::DigestMismatch) {
        caught += 1;
    }
    if expect_after(|l| l.cut_payload(1, 5), SealError::LengthMismatch) {
        caught += 1;
    }
    if expect_after(|l| l.push_payload(1, b'X'), SealError::LengthMismatch) {
        caught += 1;
    }
    if expect_after(|l| l.rewrite_id(1, 0xdead), SealError::HeaderMismatch) {
        caught += 1;
    }
    if expect_after(|l| l.rewrite_version(1, 3), SealError::HeaderMismatch) {
        caught += 1;
    }
    if expect_after(|l| l.rewrite_seq(2, 9), SealError::SequenceGap) {
        caught += 1;
    }
    if expect_after(|l| l.corrode_tag(1), SealError::HeaderMismatch) {
        caught += 1;
    }
    if expect_after(|l| l.corrode_digest(1), SealError::DigestMismatch) {
        caught += 1;
    }
    if expect_after(|l| l.corrode_chain(1), SealError::ChainBreak) {
        caught += 1;
    }
    if expect_after(|l| l.remove(1).map(|_| ()), SealError::SequenceGap) {
        caught += 1;
    }
    (caught, total)
}

/// 判据入口：封条账的守恒/单调/幂等/非法必拒全性质核。
pub fn run_unx50167_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50167");

    // J1 主判据：10 类篡改逐类命中指定码
    let (caught, total) = injection_core();
    let ok = caught == total && total == 10;
    let det = format!("篡改 {}/{}", caught, total);
    s.add_dyn("50167-J1-篡改×10全中", ok, &det);

    // 1 空载荷：签名前就拒，不产生半条记录
    let mut l = SealLedger::new();
    let ok = l.append(1, 2, b"").err() == Some(SealError::EmptyPayload) && l.is_empty();
    s.add("50167-J1-空载荷必拒", ok, if ok { "" } else { "空载荷入账" });

    // 2 超限：MAX_PAYLOAD+1 字节必须被拒
    let mut big = Vec::new();
    let mut i = 0usize;
    while i < MAX_PAYLOAD + 1 {
        big.push(b'a');
        i += 1;
    }
    let ok = l.append(1, 2, big.as_slice()).err() == Some(SealError::Limit)
        && l.is_empty();
    s.add("50167-J1-超限必拒", ok, if ok { "" } else { "超限入账" });

    // 3 版本闭集：0 与 3 都拒，1/2 收
    let mut v = SealLedger::new();
    let bad0 = v.append(1, 0, b"x").err() == Some(SealError::BadVersion);
    let bad3 = v.append(1, 3, b"x").err() == Some(SealError::BadVersion);
    let ok = bad0 && bad3 && v.append(1, 1, b"x").is_ok();
    s.add("50167-J1-版本闭集", ok, if ok { "" } else { "版本闸门漂移" });

    // 4 容量满：MAX_ENTRIES 条后拒绝且账本长度不变
    let mut full = SealLedger::new();
    let mut i = 0u32;
    while i < MAX_ENTRIES as u32 {
        let _ = full.append(i % 7, 2, b"p");
        i += 1;
    }
    let before = full.len();
    let ok = before == MAX_ENTRIES
        && full.append(1, 2, b"p").err() == Some(SealError::Capacity)
        && full.len() == before;
    s.add("50167-J1-容量满必拒", ok, if ok { "" } else { "满后覆盖或静默丢" });

    // 5 链单调：每条链值都由前链与本条摘要重算得出，且逐条不同
    let ok = match seed_ledger() {
        Ok(g) => {
            let mut good = true;
            let mut prev = CHAIN_SEED;
            let mut i = 0usize;
            while i < g.len() {
                match g.inspect(i) {
                    Some(e) => {
                        if chain_of(prev, e.digest) != e.chain {
                            good = false;
                        }
                        prev = e.chain;
                    }
                    None => good = false,
                }
                i += 1;
            }
            good && g.chain_tip() == prev
        }
        Err(_) => false,
    };
    s.add("50167-J1-链单调", ok, if ok { "" } else { "链值未随前驱移动" });

    // 6 干净账全验通过
    let ok = match seed_ledger() {
        Ok(g) => g.verify_all().is_ok() && g.len() == 4,
        Err(_) => false,
    };
    s.add("50167-J1-干净账通过", ok, if ok { "" } else { "干净账误报" });

    // 7 确定性：同输入两次签发，链尾与逐条摘要逐位一致
    let (a, b) = (seed_ledger(), seed_ledger());
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => {
            let mut same = x.chain_tip() == y.chain_tip() && x.len() == y.len();
            let mut i = 0usize;
            while i < x.len() {
                match (x.inspect(i), y.inspect(i)) {
                    (Some(p), Some(q)) => {
                        if p.digest != q.digest || p.chain != q.chain {
                            same = false;
                        }
                    }
                    _ => same = false,
                }
                i += 1;
            }
            same
        }
        _ => false,
    };
    s.add("50167-J1-签发确定", ok, if ok { "" } else { "同输入摘要漂移" });

    // 8 重放：把首条原样拼到账尾，链与序号都会露馅，重放检测单独报 Replay
    let ok = match seed_ledger() {
        Ok(mut g) => match g.inspect(0).cloned() {
            Some(e) => g.restore_raw(e).is_ok() && g.detect_replay().err() == Some(SealError::Replay),
            None => false,
        },
        Err(_) => false,
    };
    s.add("50167-J1-重放必抓", ok, if ok { "" } else { "重复摘要在账" });

    // 9 交换：两条互换后序号错位，必须报 SequenceGap
    let ok = match seed_ledger() {
        Ok(mut g) => g.swap(1, 2).is_ok() && g.verify_all().err() == Some(SealError::SequenceGap),
        Err(_) => false,
    };
    s.add("50167-J1-换序必抓", ok, if ok { "" } else { "换序未被发现" });

    // 10 头字段入摘要：同载荷换 id 或版本，摘要必须不同
    let p = b"Feature:PageMediaSize;Option:ISOA4";
    let d0 = SealEntry::new(0, 1, 2, p, CHAIN_SEED).digest;
    let d1 = SealEntry::new(0, 2, 2, p, CHAIN_SEED).digest;
    let d2 = SealEntry::new(0, 1, 1, p, CHAIN_SEED).digest;
    let d3 = SealEntry::new(1, 1, 2, p, CHAIN_SEED).digest;
    let ok = d0 != d1 && d0 != d2 && d0 != d3;
    s.add("50167-J1-头字段入摘要", ok, if ok { "" } else { "头字段未入摘要" });

    // 11 删中段：剩余序号不连续
    let ok = match seed_ledger() {
        Ok(mut g) => g.remove(1).is_ok() && g.verify_all().err() == Some(SealError::SequenceGap),
        Err(_) => false,
    };
    s.add("50167-J1-删中段必抓", ok, if ok { "" } else { "删条后仍通过" });

    // 12 空账：链尾=链首种子，校验通过
    let e = SealLedger::new();
    let ok = e.chain_tip() == CHAIN_SEED && e.verify_all().is_ok() && e.is_empty();
    s.add("50167-J1-空账基准", ok, if ok { "" } else { "空账链尾漂移" });

    // 13 幂等：同一账连验两次结果一致（校验不得写账）
    let ok = match seed_ledger() {
        Ok(g) => g.verify_all().is_ok() && g.verify_all().is_ok() && g.len() == 4,
        Err(_) => false,
    };
    s.add("50167-J1-校验幂等", ok, if ok { "" } else { "校验修改账本" });

    // 14 单 bit 翻转敏感性：载荷 24 个位置逐一翻转，摘要全部不同
    let base = b"0123456789abcdefghijklmn";
    let tag = header_tag(0, 9, 2);
    let d = payload_digest(base, tag);
    let mut distinct = true;
    let mut i = 0usize;
    while i < base.len() {
        let mut m = base.to_vec();
        if let Some(b) = m.get_mut(i) {
            *b ^= 1;
        }
        if payload_digest(m.as_slice(), tag) == d {
            distinct = false;
        }
        i += 1;
    }
    s.add("50167-J1-翻转敏感", distinct, if distinct { "" } else { "单 bit 碰撞" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50167_checks_green() {
        let set = run_unx50167_checks();
        assert!(set.all_passed(), "UNX-F50167 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50167_tamper_matrix_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
    }

    #[test]
    fn unx50167_clean_ledger_verifies() {
        let g = seed_ledger();
        assert!(g.is_ok());
        match g {
            Ok(l) => {
                assert!(l.verify_all().is_ok());
                assert_eq!(l.len(), 4);
            }
            Err(_) => {}
        }
    }
}
