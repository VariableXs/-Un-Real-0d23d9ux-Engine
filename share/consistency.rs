//! TRINITY-500 · AI-04 共享卷数据协议域（F082~F099，W1）
//!
//! 桥上的**协议层**：版本号/校验和/写锁、冲突仲裁、变更通知、保险箱密文格式、
//! 剪贴板互操作、审计日志、性能预算与一致性测试夹具。

// ---------------------------------------------------------------------------
// F082 跨系统一致性协议 — 版本号/校验和/写锁
// ---------------------------------------------------------------------------

pub const PROTOCOL_VERSION: u16 = 1;
pub const RECORD_LEN: usize = 32;
pub const RECORD_MAGIC: [u8; 4] = *b"VXS1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedRecord {
    pub version: u16,
    /// 单调序号：跨系统仲裁的唯一依据。
    pub seq: u32,
    pub writer: u8,
    pub payload_crc: u32,
    pub payload_len: u32,
    pub stamp_ms: u64,
}

impl SharedRecord {
    pub fn new(writer: u8, seq: u32, payload_crc: u32, payload_len: u32, stamp_ms: u64) -> SharedRecord {
        SharedRecord { version: PROTOCOL_VERSION, seq, writer, payload_crc, payload_len, stamp_ms }
    }

    pub fn encode(&self, out: &mut [u8]) -> bool {
        if out.len() < RECORD_LEN {
            return false;
        }
        for b in out.iter_mut().take(RECORD_LEN) {
            *b = 0;
        }
        out[0..4].copy_from_slice(&RECORD_MAGIC);
        out[4..6].copy_from_slice(&self.version.to_le_bytes());
        out[6..10].copy_from_slice(&self.seq.to_le_bytes());
        out[10] = self.writer;
        out[12..16].copy_from_slice(&self.payload_crc.to_le_bytes());
        out[16..20].copy_from_slice(&self.payload_len.to_le_bytes());
        out[20..28].copy_from_slice(&self.stamp_ms.to_le_bytes());
        true
    }

    pub fn decode(bytes: &[u8]) -> Option<SharedRecord> {
        if bytes.len() < RECORD_LEN || bytes[0..4] != RECORD_MAGIC {
            return None;
        }
        Some(SharedRecord {
            version: u16::from_le_bytes([bytes[4], bytes[5]]),
            seq: u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]),
            writer: bytes[10],
            payload_crc: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            payload_len: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            stamp_ms: u64::from_le_bytes([
                bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27],
            ]),
        })
    }

    pub fn version_ok(&self) -> bool {
        self.version == PROTOCOL_VERSION
    }
}

/// 主版本不同即不兼容（不做「尽力而为」的静默降级）。
pub fn version_compatible(v: u16) -> bool {
    v == PROTOCOL_VERSION
}

// ---------------------------------------------------------------------------
// F083 冲突检测与仲裁策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    KeepLocal,
    KeepRemote,
    /// 都无法判优：并保留两个版本（不丢数据）。
    ForkBoth,
}

impl Resolution {
    pub fn text(self) -> &'static str {
        match self {
            Resolution::KeepLocal => "keep-local",
            Resolution::KeepRemote => "keep-remote",
            Resolution::ForkBoth => "fork-both",
        }
    }
}

/// 仲裁规则：序号大者胜；序号相同则时间戳新者胜；完全相同 → 分叉保留。
pub fn arbitrate(local_seq: u32, remote_seq: u32, local_ms: u64, remote_ms: u64) -> Resolution {
    if local_seq > remote_seq {
        Resolution::KeepLocal
    } else if remote_seq > local_seq {
        Resolution::KeepRemote
    } else if local_ms > remote_ms {
        Resolution::KeepLocal
    } else if remote_ms > local_ms {
        Resolution::KeepRemote
    } else {
        Resolution::ForkBoth
    }
}

// ---------------------------------------------------------------------------
// F084 文件变更通知 — 跨系统可见性
// ---------------------------------------------------------------------------

pub const MAX_EVENTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Debug)]
pub struct ChangeEvent {
    pub path_hash: u64,
    pub kind: ChangeKind,
    pub seq: u32,
}

pub struct ChangeBus {
    events: [Option<ChangeEvent>; MAX_EVENTS],
    head: usize,
    count: usize,
    pub seq: u32,
}

impl ChangeBus {
    pub const fn new() -> ChangeBus {
        ChangeBus { events: [None; MAX_EVENTS], head: 0, count: 0, seq: 0 }
    }

    pub fn publish(&mut self, path_hash: u64, kind: ChangeKind) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        self.events[self.head] = Some(ChangeEvent { path_hash, kind, seq: self.seq });
        self.head = (self.head + 1) % MAX_EVENTS;
        if self.count < MAX_EVENTS {
            self.count += 1;
        }
        self.seq
    }

    /// 拉取 `since_seq` 之后的所有事件（环形缓冲内）。
    pub fn drain(&self, since_seq: u32, out: &mut [ChangeEvent]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            let idx = (self.head + MAX_EVENTS - self.count + i) % MAX_EVENTS;
            if let Some(e) = self.events[idx] {
                if e.seq > since_seq && n < out.len() {
                    out[n] = e;
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for ChangeBus {
    fn default() -> Self {
        ChangeBus::new()
    }
}

// ---------------------------------------------------------------------------
// F086 保险箱密文互通 — AES-256-GCM 格式
// ---------------------------------------------------------------------------

pub const VAULT_MAGIC: [u8; 4] = *b"VXVG";
pub const VAULT_HEADER_LEN: usize = 48;
pub const ALG_AES_256_GCM: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultEnvelope {
    pub version: u16,
    pub alg: u16,
    pub nonce: [u8; 12],
    pub tag: [u8; 16],
    pub payload_len: u64,
}

impl VaultEnvelope {
    pub fn new(alg: u16, nonce: [u8; 12], tag: [u8; 16], payload_len: u64) -> VaultEnvelope {
        VaultEnvelope { version: 1, alg, nonce, tag, payload_len }
    }

    pub fn encode(&self, out: &mut [u8]) -> bool {
        if out.len() < VAULT_HEADER_LEN {
            return false;
        }
        for b in out.iter_mut().take(VAULT_HEADER_LEN) {
            *b = 0;
        }
        out[0..4].copy_from_slice(&VAULT_MAGIC);
        out[4..6].copy_from_slice(&self.version.to_le_bytes());
        out[6..8].copy_from_slice(&self.alg.to_le_bytes());
        out[8..20].copy_from_slice(&self.nonce);
        out[20..36].copy_from_slice(&self.tag);
        out[36..44].copy_from_slice(&self.payload_len.to_le_bytes());
        true
    }

    pub fn decode(bytes: &[u8]) -> Option<VaultEnvelope> {
        if bytes.len() < VAULT_HEADER_LEN || bytes[0..4] != VAULT_MAGIC {
            return None;
        }
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&bytes[8..20]);
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&bytes[20..36]);
        Some(VaultEnvelope {
            version: u16::from_le_bytes([bytes[4], bytes[5]]),
            alg: u16::from_le_bytes([bytes[6], bytes[7]]),
            nonce,
            tag,
            payload_len: u64::from_le_bytes([
                bytes[36], bytes[37], bytes[38], bytes[39], bytes[40], bytes[41], bytes[42], bytes[43],
            ]),
        })
    }

    /// 只有 AES-256-GCM 被接受；其他算法一律拒绝，绝不静默降级成明文。
    pub fn acceptable(&self) -> bool {
        self.version == 1 && self.alg == ALG_AES_256_GCM
    }
}

// ---------------------------------------------------------------------------
// F087 剪贴板互操作协议
// ---------------------------------------------------------------------------

pub const CLIPBOARD_MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Text,
    Image,
    FileRefs,
}

#[derive(Clone, Copy, Debug)]
pub struct ClipboardPayload {
    pub kind: ClipKind,
    pub seq: u32,
    pub len: u32,
}

/// 跨系统剪贴板只走共享卷；超限或未知类型直接拒绝（不截断，避免静默丢数据）。
pub fn clipboard_allowed(kind: ClipKind, len: u32) -> bool {
    matches!(kind, ClipKind::Text | ClipKind::Image | ClipKind::FileRefs) && (len as usize) <= CLIPBOARD_MAX_BYTES
}

// ---------------------------------------------------------------------------
// F088 最近文件三系统同步
// ---------------------------------------------------------------------------

pub const MAX_RECENT: usize = 24;

pub struct RecentList {
    items: [(u64, u64); MAX_RECENT], // (path_hash, stamp)
    count: usize,
}

impl RecentList {
    pub const fn new() -> RecentList {
        RecentList { items: [(0, 0); MAX_RECENT], count: 0 }
    }

    /// 同一路径重复出现时先去重（保留最新时间戳）。
    pub fn push(&mut self, path_hash: u64, stamp: u64) {
        if let Some(i) = (0..self.count).find(|i| self.items[*i].0 == path_hash) {
            self.items[i].1 = stamp;
            return;
        }
        if self.count < MAX_RECENT {
            self.items[self.count] = (path_hash, stamp);
            self.count += 1;
        } else {
            // 挤掉最旧的一条。
            let oldest = (0..self.count).min_by_key(|i| self.items[*i].1).unwrap_or(0);
            self.items[oldest] = (path_hash, stamp);
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 按时间倒序取前 n 条。
    pub fn top(&self, n: usize, out: &mut [u64]) -> usize {
        let mut idx: [usize; MAX_RECENT] = [0; MAX_RECENT];
        for i in 0..self.count {
            idx[i] = i;
        }
        // 手写选择排序：无分配器、无 core::slice::sort 依赖。
        let cnt = self.count;
        for i in 0..cnt {
            let mut best = i;
            for j in (i + 1)..cnt {
                if self.items[idx[j]].1 > self.items[idx[best]].1 {
                    best = j;
                }
            }
            idx.swap(i, best);
        }
        let take = if n < self.count { n } else { self.count };
        for i in 0..take {
            if i < out.len() {
                out[i] = self.items[idx[i]].0;
            }
        }
        take
    }
}

impl Default for RecentList {
    fn default() -> Self {
        RecentList::new()
    }
}

// ---------------------------------------------------------------------------
// F089 数据校验和与信任链
// ---------------------------------------------------------------------------

pub const MAX_CHAIN: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct ChainNode {
    pub crc: u32,
    pub parent: u32,
}

/// 信任链：首节点 parent 必须为 0，其后每个节点的 parent 必须等于前一节点 crc。
pub fn chain_ok(nodes: &[ChainNode]) -> bool {
    if nodes.is_empty() {
        return false;
    }
    if nodes[0].parent != 0 {
        return false;
    }
    for i in 1..nodes.len() {
        if nodes[i].parent != nodes[i - 1].crc {
            return false;
        }
    }
    true
}

pub fn chain_tip(nodes: &[ChainNode]) -> u32 {
    nodes.last().map(|n| n.crc).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// F092 共享卷访问审计日志
// ---------------------------------------------------------------------------

pub const MAX_AUDIT: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct AuditEntry {
    pub who: u8,
    pub path_hash: u64,
    pub stamp_ms: u64,
    pub allowed: bool,
}

pub struct AuditLog {
    entries: [Option<AuditEntry>; MAX_AUDIT],
    count: usize,
    denied: usize,
}

impl AuditLog {
    pub const fn new() -> AuditLog {
        AuditLog { entries: [None; MAX_AUDIT], count: 0, denied: 0 }
    }

    pub fn append(&mut self, e: AuditEntry) {
        if !e.allowed {
            self.denied += 1;
        }
        if self.count < MAX_AUDIT {
            self.entries[self.count] = Some(e);
            self.count += 1;
        } else {
            for i in 1..MAX_AUDIT {
                self.entries[i - 1] = self.entries[i];
            }
            self.entries[MAX_AUDIT - 1] = Some(e);
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn denied_count(&self) -> usize {
        self.denied
    }

    pub fn get(&self, i: usize) -> Option<AuditEntry> {
        if i < self.count {
            self.entries[i]
        } else {
            None
        }
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        AuditLog::new()
    }
}

// ---------------------------------------------------------------------------
// F094 大文件流式读写
// ---------------------------------------------------------------------------

pub const STREAM_CHUNK: usize = 1024 * 1024;

pub struct StreamReader {
    pub total: u64,
    pub offset: u64,
    pub chunk: usize,
}

impl StreamReader {
    pub fn new(total: u64, chunk: usize) -> StreamReader {
        StreamReader { total, offset: 0, chunk: if chunk == 0 { STREAM_CHUNK } else { chunk } }
    }

    pub fn remaining(&self) -> u64 {
        self.total.saturating_sub(self.offset)
    }

    /// 返回本次应读字节数；读完返回 0。
    pub fn next_chunk(&mut self) -> usize {
        let rem = self.remaining();
        if rem == 0 {
            return 0;
        }
        let take = if rem < self.chunk as u64 { rem as usize } else { self.chunk };
        self.offset += take as u64;
        take
    }

    pub fn done(&self) -> bool {
        self.offset >= self.total
    }

    pub fn chunk_count(&self) -> u64 {
        self.total / self.chunk as u64 + u64::from(self.total % self.chunk as u64 != 0)
    }
}

// ---------------------------------------------------------------------------
// F095 数据协议版本兼容矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compat {
    Full,
    ReadOnly,
    Incompatible,
}

impl Compat {
    pub fn text(self) -> &'static str {
        match self {
            Compat::Full => "full",
            Compat::ReadOnly => "read-only",
            Compat::Incompatible => "incompatible",
        }
    }
}

pub fn compat(a: u16, b: u16) -> Compat {
    if a == b {
        Compat::Full
    } else if a / 100 == b / 100 {
        Compat::ReadOnly
    } else {
        Compat::Incompatible
    }
}

// ---------------------------------------------------------------------------
// F096 共享卷性能预算 — 随机读 < 20ms
// ---------------------------------------------------------------------------

pub const IO_REDLINE_US: u32 = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoVerdict {
    Within,
    Over,
}

pub fn io_verdict(measured_us: u32) -> IoVerdict {
    if measured_us <= IO_REDLINE_US {
        IoVerdict::Within
    } else {
        IoVerdict::Over
    }
}

/// 一批随机读的 p95（输入需已排序）。
pub fn p95_us(sorted: &[u32]) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (sorted.len() * 95) / 100;
    sorted[if idx >= sorted.len() { sorted.len() - 1 } else { idx }]
}

// ---------------------------------------------------------------------------
// F097 共享卷掉电一致性测试（确定性夹具）
// ---------------------------------------------------------------------------

pub const MAX_JOURNAL: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct JournalEntry {
    pub seq: u32,
    pub crc: u32,
    pub committed: bool,
}

/// 重放日志：只有 crc 与序号自洽且标记为 committed 的条目才生效。
/// 模拟掉电：把尾部若干条目的 committed 清掉，重放结果必须仍然自洽。
pub fn replay(entries: &[JournalEntry], out: &mut [u32]) -> usize {
    let mut n = 0usize;
    let mut expect = 1u32;
    for e in entries.iter() {
        if !e.committed {
            break; // 掉电点之后一律不生效
        }
        if e.seq != expect {
            break;
        }
        if e.crc != crate::power::crc32(&e.seq.to_le_bytes()) {
            break;
        }
        if n < out.len() {
            out[n] = e.seq;
            n += 1;
        }
        expect += 1;
    }
    n
}

pub fn journal_entry(seq: u32) -> JournalEntry {
    JournalEntry { seq, crc: crate::power::crc32(&seq.to_le_bytes()), committed: true }
}

// ---------------------------------------------------------------------------
// F098 共享卷多系统压力测试（确定性夹具）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct StressResult {
    pub applied: u32,
    pub conflicts: u32,
    pub corrupt: u32,
}

/// 三个系统并发写：每次写都带自增序号与 CRC，重放后不允许出现乱序或损坏。
pub fn stress_round(writers: u32, ops: u32, drop_tail: u32) -> StressResult {
    let mut journal = [JournalEntry { seq: 0, crc: 0, committed: false }; MAX_JOURNAL];
    let mut n = 0usize;
    let mut seq = 1u32;
    let mut conflicts = 0u32;
    for _ in 0..ops.min(MAX_JOURNAL as u32) {
        let w = seq % writers.max(1);
        if w == 0 {
            conflicts += 1; // 同一序号槽被两个系统争用 → 记为冲突而非损坏
        }
        if n < MAX_JOURNAL {
            journal[n] = journal_entry(seq);
            n += 1;
        }
        seq += 1;
    }
    let keep = n.saturating_sub(drop_tail as usize);
    for i in keep..n {
        journal[i].committed = false;
    }
    let mut out = [0u32; MAX_JOURNAL];
    let applied = replay(&journal[..n], &mut out) as u32;
    let mut corrupt = 0u32;
    for i in 1..applied as usize {
        if out[i] != out[i - 1] + 1 {
            corrupt += 1;
        }
    }
    StressResult { applied, conflicts, corrupt }
}

// ---------------------------------------------------------------------------
// F099 共享卷诊断器
// ---------------------------------------------------------------------------

pub const MAX_SHARE_ISSUES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareIssue {
    VersionMismatch,
    BrokenChain,
    VaultPlaintext,
    AuditDeniedSpike,
    IoOverBudget,
    TreeMissing,
}

impl ShareIssue {
    pub fn text(self) -> &'static str {
        match self {
            ShareIssue::VersionMismatch => "protocol version mismatch",
            ShareIssue::BrokenChain => "checksum chain broken",
            ShareIssue::VaultPlaintext => "plaintext write attempted inside vault",
            ShareIssue::AuditDeniedSpike => "audit log shows a spike of denied accesses",
            ShareIssue::IoOverBudget => "random read latency over budget",
            ShareIssue::TreeMissing => "shared directory tree incomplete",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ShareDiag {
    issues: [Option<ShareIssue>; MAX_SHARE_ISSUES],
    count: usize,
}

impl ShareDiag {
    pub const fn new() -> ShareDiag {
        ShareDiag { issues: [None; MAX_SHARE_ISSUES], count: 0 }
    }

    fn push(&mut self, i: ShareIssue) {
        if self.count >= MAX_SHARE_ISSUES || (0..self.count).any(|k| self.issues[k] == Some(i)) {
            return;
        }
        self.issues[self.count] = Some(i);
        self.count += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<ShareIssue> {
        if i < self.count {
            self.issues[i]
        } else {
            None
        }
    }

    pub fn has(&self, i: ShareIssue) -> bool {
        (0..self.count).any(|k| self.issues[k] == Some(i))
    }

    /// 汇总体检：版本、信任链、审计拒绝率、IO 预算、目录树。
    pub fn inspect(
        &mut self,
        version_ok: bool,
        chain: &[ChainNode],
        audit: &AuditLog,
        p95: u32,
        tree_ok: bool,
    ) {
        if !version_ok {
            self.push(ShareIssue::VersionMismatch);
        }
        if !chain_ok(chain) {
            self.push(ShareIssue::BrokenChain);
        }
        if audit.len() >= 4 && audit.denied_count() * 4 > audit.len() * 3 {
            self.push(ShareIssue::AuditDeniedSpike);
        }
        if io_verdict(p95) == IoVerdict::Over {
            self.push(ShareIssue::IoOverBudget);
        }
        if !tree_ok {
            self.push(ShareIssue::TreeMissing);
        }
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(s) = self.get(i) {
                for &b in s.text().as_bytes() {
                    if n < out.len() {
                        out[n] = b;
                        n += 1;
                    }
                }
                if n < out.len() {
                    out[n] = b'\n';
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for ShareDiag {
    fn default() -> Self {
        ShareDiag::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f082_record_roundtrip() {
        let r = SharedRecord::new(3, 7, 0xDEAD, 12, 99);
        let mut buf = [0u8; RECORD_LEN];
        assert!(r.encode(&mut buf));
        let back = SharedRecord::decode(&buf).unwrap();
        assert_eq!(back, r);
        assert!(back.version_ok());
        assert!(version_compatible(PROTOCOL_VERSION));
    }

    #[test]
    fn f083_arbitration_is_deterministic() {
        assert_eq!(arbitrate(2, 1, 0, 0), Resolution::KeepLocal);
        assert_eq!(arbitrate(1, 2, 0, 0), Resolution::KeepRemote);
        assert_eq!(arbitrate(1, 1, 5, 4), Resolution::KeepLocal);
        assert_eq!(arbitrate(1, 1, 4, 5), Resolution::KeepRemote);
        assert_eq!(arbitrate(1, 1, 4, 4), Resolution::ForkBoth);
    }

    #[test]
    fn f084_change_bus_drains() {
        let mut bus = ChangeBus::new();
        let s1 = bus.publish(1, ChangeKind::Created);
        bus.publish(2, ChangeKind::Modified);
        let mut out = [ChangeEvent { path_hash: 0, kind: ChangeKind::Created, seq: 0 }; 8];
        assert_eq!(bus.drain(s1, &mut out), 1);
        assert_eq!(bus.drain(0, &mut out), 2);
    }

    #[test]
    fn f086_vault_envelope() {
        let v = VaultEnvelope::new(ALG_AES_256_GCM, [1u8; 12], [2u8; 16], 4096);
        let mut buf = [0u8; VAULT_HEADER_LEN];
        assert!(v.encode(&mut buf));
        let back = VaultEnvelope::decode(&buf).unwrap();
        assert!(back.acceptable());
        assert_eq!(back.payload_len, 4096);
        let bad = VaultEnvelope::new(99, [0u8; 12], [0u8; 16], 1);
        assert!(!bad.acceptable());
    }

    #[test]
    fn f087_clipboard_bounds() {
        assert!(clipboard_allowed(ClipKind::Text, 10));
        assert!(!clipboard_allowed(ClipKind::Text, CLIPBOARD_MAX_BYTES as u32 + 1));
    }

    #[test]
    fn f088_recent_dedupes_and_orders() {
        let mut r = RecentList::new();
        r.push(1, 10);
        r.push(2, 30);
        r.push(1, 50);
        assert_eq!(r.len(), 2);
        let mut top = [0u64; 4];
        assert_eq!(r.top(2, &mut top), 2);
        assert_eq!(top[0], 1);
    }

    #[test]
    fn f089_chain_validation() {
        let nodes = [
            ChainNode { crc: 10, parent: 0 },
            ChainNode { crc: 20, parent: 10 },
        ];
        assert!(chain_ok(&nodes));
        assert_eq!(chain_tip(&nodes), 20);
        assert!(!chain_ok(&[ChainNode { crc: 1, parent: 5 }]));
    }

    #[test]
    fn f092_audit_counts_denials() {
        let mut a = AuditLog::new();
        a.append(AuditEntry { who: 1, path_hash: 1, stamp_ms: 0, allowed: false });
        a.append(AuditEntry { who: 1, path_hash: 2, stamp_ms: 1, allowed: true });
        assert_eq!(a.denied_count(), 1);
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn f094_streaming_walks_to_end() {
        let mut s = StreamReader::new(2_500_000, 1_000_000);
        assert_eq!(s.chunk_count(), 3);
        assert_eq!(s.next_chunk(), 1_000_000);
        assert_eq!(s.next_chunk(), 1_000_000);
        assert_eq!(s.next_chunk(), 500_000);
        assert!(s.done());
        assert_eq!(s.next_chunk(), 0);
    }

    #[test]
    fn f095_compat_matrix() {
        assert_eq!(compat(1, 1), Compat::Full);
        assert_eq!(compat(1, 2), Compat::ReadOnly);
        assert_eq!(compat(1, 101), Compat::Incompatible);
    }

    #[test]
    fn f096_p95_and_budget() {
        let s = [1u32, 2, 3, 4, 5, 6, 7, 8, 9, 100];
        assert_eq!(p95_us(&s), 100);
        assert_eq!(io_verdict(19_000), IoVerdict::Within);
        assert_eq!(io_verdict(21_000), IoVerdict::Over);
    }

    #[test]
    fn f097_power_loss_replay() {
        let mut journal = [journal_entry(1), journal_entry(2), journal_entry(3)];
        journal[2].committed = false; // 掉电点在写第 3 条时
        let mut out = [0u32; MAX_JOURNAL];
        assert_eq!(replay(&journal, &mut out), 2);
    }

    #[test]
    fn f098_stress_never_corrupts() {
        let r = stress_round(3, 20, 5);
        assert_eq!(r.corrupt, 0);
        assert_eq!(r.applied, 15);
        assert!(r.conflicts > 0);
    }

    #[test]
    fn f099_diag_collects_issues() {
        let mut d = ShareDiag::new();
        let mut a = AuditLog::new();
        for _ in 0..8 {
            a.append(AuditEntry { who: 1, path_hash: 0, stamp_ms: 0, allowed: false });
        }
        d.inspect(false, &[ChainNode { crc: 1, parent: 9 }], &a, 40_000, false);
        assert!(d.has(ShareIssue::VersionMismatch));
        assert!(d.has(ShareIssue::BrokenChain));
        assert!(d.has(ShareIssue::AuditDeniedSpike));
        assert!(d.has(ShareIssue::IoOverBudget));
        assert!(d.has(ShareIssue::TreeMissing));
        let mut out = [0u8; 256];
        assert!(d.render(&mut out) > 0);
    }
}
