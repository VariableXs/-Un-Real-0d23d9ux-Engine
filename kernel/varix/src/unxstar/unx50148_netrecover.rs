//! UNX-F50148 网络分区恢复账（断网→恢复重发现幂等）（0x43ea0 段）
//!
//! 锚点：UNX-F50148-J1 分区 ×10 恢复零重复表项；判据锚定 §AI-63 §2 专题五。
//!
//! 分区恢复的难点不在「重连」，而在**重连之后还不能多出一行**。断网期间发现
//! 链停摆、心跳计满、状态翻到 `Partitioned`；网络回来的一瞬间，交换机会把同一
//! 批 announce 重放两遍（多播 + 单播兜底），打印机会带着新 epoch 重新声明自己，
//! 也可能已经换了 IP。三种情况必须落到同一张表上：
//!
//! * 同 id 同 epoch 的重复包 —— 幂等重放，只记 `replays`，表内容不变；
//! * 同 id 更高 epoch —— 合并进原行（更新地址/端口/epoch），绝不再插一行；
//! * 表里已无此 id —— 才允许新增，且受 [`PEER_CAP`] 约束。
//!
//! 状态机 [`LinkState`]：`Up → Degraded → Partitioned → Resyncing → Up`。
//! 合法转移写成显式表（[`LinkState::legal`]），非法转移一律 `BadState`——
//! 恢复期收到的普通心跳如果被当成「已恢复」，`Resyncing` 阶段的重发现就永远
//! 不会跑，表里会留下一批已经消失的邻居。
//!
//! 批处理是**先校验后应用**（validate-then-apply）：一批里只要有一个坏项，
//! 整批拒绝且表内容逐位不变。半写比整批失败危险得多——半写的表会让下一次
//! 恢复把「已合并的行」当成「新行」再插一遍。
//!
//! 错误闭集：`SEG_BASE + 1 ..= SEG_BASE + 12`，逐项见 [`NetError`]。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43ea0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43ea0;

/// 邻居表硬顶。一个 /24 网段内的打印机 + 扫描仪远小于此值。
pub const PEER_CAP: usize = 32;

/// 分区账本硬顶（历史保留 16 次断网，够定位「每次断网都多一行」这类回归）。
pub const EPISODE_CAP: usize = 16;

/// 连续丢 3 次心跳才判分区：1~2 次是链路抖动，过早分区会让账本被噪声灌满。
pub const MISS_LIMIT: u32 = 3;

pub const E_BAD_PEER_ID: u16 = 1;
pub const E_SELF_PEER: u16 = 2;
pub const E_TABLE_FULL: u16 = 3;
pub const E_DUP_IN_BATCH: u16 = 4;
pub const E_STALE_EPOCH: u16 = 5;
pub const E_BAD_EPOCH: u16 = 6;
pub const E_NOT_PARTITIONED: u16 = 7;
pub const E_ALREADY_PARTITIONED: u16 = 8;
pub const E_NOT_RESYNCING: u16 = 9;
pub const E_EMPTY_BATCH: u16 = 10;
pub const E_BAD_STATE: u16 = 11;
pub const E_LEDGER_FULL: u16 = 12;

/// 分区恢复域错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    BadPeerId,
    SelfPeer,
    TableFull,
    DuplicateInBatch,
    StaleEpoch,
    BadEpoch,
    NotPartitioned,
    AlreadyPartitioned,
    NotResyncing,
    EmptyBatch,
    BadState,
    LedgerFull,
}

impl NetError {
    pub fn offset(self) -> u16 {
        match self {
            NetError::BadPeerId => E_BAD_PEER_ID,
            NetError::SelfPeer => E_SELF_PEER,
            NetError::TableFull => E_TABLE_FULL,
            NetError::DuplicateInBatch => E_DUP_IN_BATCH,
            NetError::StaleEpoch => E_STALE_EPOCH,
            NetError::BadEpoch => E_BAD_EPOCH,
            NetError::NotPartitioned => E_NOT_PARTITIONED,
            NetError::AlreadyPartitioned => E_ALREADY_PARTITIONED,
            NetError::NotResyncing => E_NOT_RESYNCING,
            NetError::EmptyBatch => E_EMPTY_BATCH,
            NetError::BadState => E_BAD_STATE,
            NetError::LedgerFull => E_LEDGER_FULL,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.offset() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            NetError::BadPeerId => "peer id zero or broadcast",
            NetError::SelfPeer => "announce carries our own id",
            NetError::TableFull => "peer table full",
            NetError::DuplicateInBatch => "peer id repeated inside batch",
            NetError::StaleEpoch => "announce epoch older than cached",
            NetError::BadEpoch => "epoch zero is not a valid generation",
            NetError::NotPartitioned => "link is not partitioned",
            NetError::AlreadyPartitioned => "link already partitioned",
            NetError::NotResyncing => "link is not resyncing",
            NetError::EmptyBatch => "empty recovery batch",
            NetError::BadState => "illegal link state transition",
            NetError::LedgerFull => "partition episode ledger full",
        }
    }
}

/// 闭集枚举（顺序即偏移序）。
pub const ALL_ERRORS: [NetError; 12] = [
    NetError::BadPeerId,
    NetError::SelfPeer,
    NetError::TableFull,
    NetError::DuplicateInBatch,
    NetError::StaleEpoch,
    NetError::BadEpoch,
    NetError::NotPartitioned,
    NetError::AlreadyPartitioned,
    NetError::NotResyncing,
    NetError::EmptyBatch,
    NetError::BadState,
    NetError::LedgerFull,
];

/// 链路四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Degraded,
    Partitioned,
    Resyncing,
}

impl LinkState {
    pub fn rank(self) -> u8 {
        match self {
            LinkState::Up => 0,
            LinkState::Degraded => 1,
            LinkState::Partitioned => 2,
            LinkState::Resyncing => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LinkState::Up => "up",
            LinkState::Degraded => "degraded",
            LinkState::Partitioned => "partitioned",
            LinkState::Resyncing => "resyncing",
        }
    }

    /// 合法转移表。写成显式匹配而不是「rank 比较」，是因为
    /// `Resyncing(3) → Degraded(1)` 这种"降级"其实非法，而
    /// `Partitioned(2) → Up(0)` 也非法（必须经过重发现）。
    pub fn legal(from: LinkState, to: LinkState) -> bool {
        match (from, to) {
            (LinkState::Up, LinkState::Degraded) => true,
            (LinkState::Degraded, LinkState::Up) => true,
            (LinkState::Degraded, LinkState::Partitioned) => true,
            (LinkState::Partitioned, LinkState::Resyncing) => true,
            (LinkState::Resyncing, LinkState::Up) => true,
            _ => false,
        }
    }
}

/// 一条 announce（等价于 mDNS 记录里「我还在、我是第几代」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Announce {
    pub id: u16,
    pub epoch: u32,
    pub addr: u32,
    pub port: u16,
}

/// 邻居行。`merges` 记录这一行被合并过几次，用来分辨「一直很稳」与
/// 「每次断网都在重新自我声明」——后者往往意味着设备在反复重启。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerEntry {
    pub id: u16,
    pub epoch: u32,
    pub addr: u32,
    pub port: u16,
    pub first_seen: u32,
    pub last_seen: u32,
    pub merges: u16,
}

/// 邻居表：ids 唯一是硬不变量。
#[derive(Clone, Debug)]
pub struct PeerTable {
    peers: Vec<PeerEntry>,
}

impl PeerTable {
    pub fn new() -> PeerTable {
        PeerTable {
            peers: Vec::with_capacity(PEER_CAP),
        }
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn at(&self, idx: usize) -> Option<&PeerEntry> {
        self.peers.get(idx)
    }

    /// 按 id 取行。
    pub fn get(&self, id: u16) -> Option<&PeerEntry> {
        match self.index_of(id) {
            Some(i) => self.peers.get(i),
            None => None,
        }
    }

    fn index_of(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.peers.len() {
            if let Some(p) = self.peers.get(i) {
                if p.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 插入新行（调用方保证 id 不在表中且容量足够）。
    fn push_new(&mut self, a: Announce, now: u32) {
        self.peers.push(PeerEntry {
            id: a.id,
            epoch: a.epoch,
            addr: a.addr,
            port: a.port,
            first_seen: now,
            last_seen: now,
            merges: 0,
        });
    }

    /// 合并进已有行：更新世代与地址，绝不新增行。
    fn merge_at(&mut self, idx: usize, a: Announce, now: u32) {
        if let Some(p) = self.peers.get_mut(idx) {
            p.epoch = a.epoch;
            p.addr = a.addr;
            p.port = a.port;
            p.last_seen = now;
            p.merges = p.merges.saturating_add(1);
        }
    }

    fn touch(&mut self, idx: usize, now: u32) {
        if let Some(p) = self.peers.get_mut(idx) {
            p.last_seen = now;
        }
    }

    /// 摘除 `last_seen < tick` 的行（恢复期没再声明的邻居），返回摘除数。
    pub fn prune_before(&mut self, tick: u32) -> usize {
        let mut kept: Vec<PeerEntry> = Vec::with_capacity(self.peers.len());
        let mut dropped = 0usize;
        let mut i = 0usize;
        while i < self.peers.len() {
            if let Some(p) = self.peers.get(i) {
                if p.last_seen >= tick {
                    kept.push(p.clone());
                } else {
                    dropped += 1;
                }
            }
            i += 1;
        }
        self.peers = kept;
        dropped
    }

    /// 重复 id 数（不变量检查；正常路径应恒为 0）。
    pub fn duplicate_ids(&self) -> usize {
        let mut dup = 0usize;
        let mut i = 0usize;
        while i < self.peers.len() {
            let id = match self.peers.get(i) {
                Some(p) => p.id,
                None => break,
            };
            let mut j = 0usize;
            while j < i {
                if let Some(q) = self.peers.get(j) {
                    if q.id == id {
                        dup += 1;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        dup
    }

    /// 表摘要（表序）。恢复前后摘要相同 = 零改写。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.peers.len() {
            if let Some(p) = self.peers.get(i) {
                h = fnv_mix(h, p.id as u64);
                h = fnv_mix(h, p.epoch as u64);
                h = fnv_mix(h, p.addr as u64);
                h = fnv_mix(h, p.port as u64);
            }
            i += 1;
        }
        h
    }

    /// 当前全部邻居的 announce 快照（epoch 原样），供恢复批次构造。
    pub fn snapshot(&self) -> Vec<Announce> {
        let mut out: Vec<Announce> = Vec::with_capacity(self.peers.len());
        let mut i = 0usize;
        while i < self.peers.len() {
            if let Some(p) = self.peers.get(i) {
                out.push(Announce {
                    id: p.id,
                    epoch: p.epoch,
                    addr: p.addr,
                    port: p.port,
                });
            }
            i += 1;
        }
        out
    }
}

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 一次分区 episode 的账。`merged + added` 是本次恢复对表的全部写入，
/// `replays` 是被幂等吞掉的重复包——它必须显式记账，否则「重放」与「真的
/// 没收到」在日志里无法区分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartitionEpisode {
    pub index: u32,
    pub started: u32,
    pub ended: u32,
    pub announced: u32,
    pub merged: u32,
    pub added: u32,
    pub replays: u32,
    pub pruned: u32,
}

/// 分区账本。`begin` 只占位，`commit` 才落账——恢复中途失败时宁可没有账，
/// 也不要一条 `ended == 0` 的半截记录。
#[derive(Clone, Debug)]
pub struct PartitionLedger {
    eps: Vec<PartitionEpisode>,
    merged: u32,
    added: u32,
    pruned: u32,
    replays: u32,
}

impl PartitionLedger {
    pub fn new() -> PartitionLedger {
        PartitionLedger {
            eps: Vec::with_capacity(EPISODE_CAP),
            merged: 0,
            added: 0,
            pruned: 0,
            replays: 0,
        }
    }

    /// 占一个新 episode 位（返回其序号）。账本满即拒。
    pub fn begin(&self, _now: u32) -> Result<u32, NetError> {
        if self.eps.len() >= EPISODE_CAP {
            return Err(NetError::LedgerFull);
        }
        Ok(self.eps.len() as u32)
    }

    fn commit(&mut self, ep: PartitionEpisode) {
        self.merged = self.merged.saturating_add(ep.merged);
        self.added = self.added.saturating_add(ep.added);
        self.pruned = self.pruned.saturating_add(ep.pruned);
        self.replays = self.replays.saturating_add(ep.replays);
        self.eps.push(ep);
    }

    pub fn len(&self) -> usize {
        self.eps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.eps.is_empty()
    }

    pub fn at(&self, idx: usize) -> Option<PartitionEpisode> {
        self.eps.get(idx).copied()
    }

    pub fn merged(&self) -> u32 {
        self.merged
    }

    pub fn added(&self) -> u32 {
        self.added
    }

    pub fn pruned(&self) -> u32 {
        self.pruned
    }

    pub fn replays(&self) -> u32 {
        self.replays
    }

    /// 账本自洽：总量恒等于逐 episode 求和（这里用分项计数器间接校验）。
    pub fn balanced(&self) -> bool {
        let mut m = 0u32;
        let mut a = 0u32;
        let mut p = 0u32;
        let mut r = 0u32;
        let mut i = 0usize;
        while i < self.eps.len() {
            if let Some(e) = self.eps.get(i) {
                m = m.saturating_add(e.merged);
                a = a.saturating_add(e.added);
                p = p.saturating_add(e.pruned);
                r = r.saturating_add(e.replays);
            }
            i += 1;
        }
        m == self.merged && a == self.added && p == self.pruned && r == self.replays
    }
}

/// 一批 announce 的应用结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchReport {
    pub merged: u32,
    pub added: u32,
    pub replays: u32,
}

/// 分区恢复引擎：链路状态 + 邻居表 + 分区账。
#[derive(Clone, Debug)]
pub struct RecoveryEngine {
    own_id: u16,
    state: LinkState,
    misses: u32,
    table: PeerTable,
    ledger: PartitionLedger,
    open: Option<PartitionEpisode>,
    replays: u32,
}

impl RecoveryEngine {
    /// `own_id` 必须是单播 id（0 与 0xffff 保留）。
    pub fn new(own_id: u16) -> Result<RecoveryEngine, NetError> {
        if own_id == 0 || own_id == u16::MAX {
            return Err(NetError::BadPeerId);
        }
        Ok(RecoveryEngine {
            own_id,
            state: LinkState::Up,
            misses: 0,
            table: PeerTable::new(),
            ledger: PartitionLedger::new(),
            open: None,
            replays: 0,
        })
    }

    pub fn state(&self) -> LinkState {
        self.state
    }

    pub fn misses(&self) -> u32 {
        self.misses
    }

    pub fn peer_count(&self) -> usize {
        self.table.len()
    }

    pub fn table(&self) -> &PeerTable {
        &self.table
    }

    pub fn ledger(&self) -> &PartitionLedger {
        &self.ledger
    }

    pub fn replays(&self) -> u32 {
        self.replays
    }

    pub fn table_digest(&self) -> u64 {
        self.table.digest()
    }

    pub fn snapshot(&self) -> Vec<Announce> {
        self.table.snapshot()
    }

    /// 显式状态转移，非法即拒（`BadState`）。
    pub fn transition_to(&mut self, next: LinkState) -> Result<(), NetError> {
        if !LinkState::legal(self.state, next) {
            return Err(NetError::BadState);
        }
        self.state = next;
        Ok(())
    }

    fn check_announce(&self, a: Announce) -> Result<(), NetError> {
        if a.id == 0 || a.id == u16::MAX {
            return Err(NetError::BadPeerId);
        }
        if a.id == self.own_id {
            return Err(NetError::SelfPeer);
        }
        if a.epoch == 0 {
            return Err(NetError::BadEpoch);
        }
        Ok(())
    }

    /// 正常运行期的发现入表（非恢复期）。返回是否改动了表。
    pub fn learn(&mut self, a: Announce, now: u32) -> Result<bool, NetError> {
        self.check_announce(a)?;
        match self.table.index_of(a.id) {
            Some(idx) => {
                let cur = match self.table.get(a.id) {
                    Some(p) => p.epoch,
                    None => 0,
                };
                if a.epoch <= cur {
                    // 同代重放或更老的代：都不算改动（同代是重放，老是陈旧）。
                    self.table.touch(idx, now);
                    return Ok(false);
                }
                self.table.merge_at(idx, a, now);
                Ok(true)
            }
            None => {
                if self.table.len() >= PEER_CAP {
                    return Err(NetError::TableFull);
                }
                self.table.push_new(a, now);
                Ok(true)
            }
        }
    }

    /// 丢一次心跳：1~2 次降级，第 3 次判分区并开 episode。
    pub fn on_miss(&mut self, now: u32) -> Result<LinkState, NetError> {
        if self.state == LinkState::Resyncing {
            return Err(NetError::BadState);
        }
        if self.state == LinkState::Partitioned {
            return Err(NetError::AlreadyPartitioned);
        }
        self.misses = self.misses.saturating_add(1);
        if self.misses >= MISS_LIMIT {
            // 先占账本位再翻状态：账本满时链路仍停在 Degraded，
            // 不会出现「已分区但无账可查」的黑洞。
            let index = self.ledger.begin(now)?;
            self.transition_to(LinkState::Partitioned)?;
            self.open = Some(PartitionEpisode {
                index,
                started: now,
                ended: 0,
                announced: 0,
                merged: 0,
                added: 0,
                replays: 0,
                pruned: 0,
            });
            return Ok(LinkState::Partitioned);
        }
        if self.state == LinkState::Up {
            self.transition_to(LinkState::Degraded)?;
        }
        Ok(self.state)
    }

    /// 抖动恢复（未到分区阈值）：Degraded → Up。
    pub fn on_heartbeat(&mut self, _now: u32) -> Result<(), NetError> {
        match self.state {
            LinkState::Up => Ok(()),
            LinkState::Degraded => {
                self.misses = 0;
                self.transition_to(LinkState::Up)
            }
            _ => Err(NetError::BadState),
        }
    }

    /// 链路回来：Partitioned → Resyncing（重发现只在 Resyncing 里允许）。
    pub fn on_link_up(&mut self, _now: u32) -> Result<(), NetError> {
        if self.state != LinkState::Partitioned {
            return Err(NetError::NotPartitioned);
        }
        self.misses = 0;
        self.transition_to(LinkState::Resyncing)
    }

    /// 应用一批恢复期 announce。先整批校验，再整批应用。
    pub fn recover_batch(&mut self, batch: &[Announce], now: u32) -> Result<BatchReport, NetError> {
        if self.state != LinkState::Resyncing {
            return Err(NetError::NotResyncing);
        }
        if batch.is_empty() {
            return Err(NetError::EmptyBatch);
        }
        let mut fresh = 0usize;
        let mut i = 0usize;
        while i < batch.len() {
            let a = match batch.get(i) {
                Some(v) => *v,
                None => break,
            };
            self.check_announce(a)?;
            let mut j = 0usize;
            while j < i {
                if let Some(b) = batch.get(j) {
                    if b.id == a.id {
                        return Err(NetError::DuplicateInBatch);
                    }
                }
                j += 1;
            }
            match self.table.get(a.id) {
                Some(cur) => {
                    if a.epoch < cur.epoch {
                        return Err(NetError::StaleEpoch);
                    }
                }
                None => fresh += 1,
            }
            i += 1;
        }
        if self.table.len().saturating_add(fresh) > PEER_CAP {
            return Err(NetError::TableFull);
        }

        let mut merged = 0u32;
        let mut added = 0u32;
        let mut replays = 0u32;
        let mut k = 0usize;
        while k < batch.len() {
            let a = match batch.get(k) {
                Some(v) => *v,
                None => break,
            };
            match self.table.index_of(a.id) {
                Some(idx) => {
                    let cur = match self.table.get(a.id) {
                        Some(p) => p.epoch,
                        None => 0,
                    };
                    if a.epoch == cur {
                        self.table.touch(idx, now);
                        replays = replays.saturating_add(1);
                    } else {
                        self.table.merge_at(idx, a, now);
                        merged = merged.saturating_add(1);
                    }
                }
                None => {
                    self.table.push_new(a, now);
                    added = added.saturating_add(1);
                }
            }
            k += 1;
        }
        self.replays = self.replays.saturating_add(replays);
        if let Some(open) = self.open.as_mut() {
            open.announced = open.announced.saturating_add(batch.len() as u32);
            open.merged = open.merged.saturating_add(merged);
            open.added = open.added.saturating_add(added);
            open.replays = open.replays.saturating_add(replays);
        }
        Ok(BatchReport {
            merged,
            added,
            replays,
        })
    }

    /// 结束重发现：Resyncing → Up，落账；`prune` 时摘掉本次没再声明的邻居。
    pub fn finish_recovery(&mut self, now: u32, prune: bool) -> Result<PartitionEpisode, NetError> {
        if self.state != LinkState::Resyncing {
            return Err(NetError::NotResyncing);
        }
        let mut ep = match self.open.take() {
            Some(v) => v,
            None => return Err(NetError::BadState),
        };
        if prune {
            ep.pruned = self.table.prune_before(ep.started) as u32;
        }
        ep.ended = now;
        self.ledger.commit(ep);
        self.misses = 0;
        self.transition_to(LinkState::Up)?;
        Ok(ep)
    }
}

// ---------------------------------------------------------------------------
// 十轮分区审计：J1 的度量本体
// ---------------------------------------------------------------------------

/// 审计回报。`idempotent_violations` 统计「重放批次改动了表」的次数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryAudit {
    pub episodes: u32,
    pub duplicates: usize,
    pub applied_merged: u32,
    pub applied_added: u32,
    pub ledger_merged: u32,
    pub ledger_added: u32,
    pub replays: u32,
    pub idempotent_violations: u32,
    pub final_peers: usize,
}

/// 跑 `episodes` 次「分区 → 重发现 → 重放 → 收尾」。
///
/// 每次恢复都会把表里所有邻居带新 epoch 重新声明一遍，并额外挂一台新设备；
/// 紧接着把同一批原样重放一次——重放必须零改动，这是「零重复表项」的正面度量。
pub fn recovery_audit(episodes: u32) -> Result<RecoveryAudit, NetError> {
    let mut e = RecoveryEngine::new(1)?;
    let mut id = 2u16;
    while id <= 7 {
        let a = Announce {
            id,
            epoch: 1,
            addr: 0x0a00_0100u32 + id as u32,
            port: 631,
        };
        e.learn(a, 0)?;
        id += 1;
    }
    let mut audit = RecoveryAudit {
        episodes: 0,
        duplicates: 0,
        applied_merged: 0,
        applied_added: 0,
        ledger_merged: 0,
        ledger_added: 0,
        replays: 0,
        idempotent_violations: 0,
        final_peers: 0,
    };
    let mut ep = 0u32;
    while ep < episodes {
        let t = 1_000u32.saturating_add(ep.saturating_mul(100));
        let mut m = 0u32;
        while m < MISS_LIMIT {
            e.on_miss(t.saturating_add(m))?;
            m += 1;
        }
        e.on_link_up(t.saturating_add(10))?;

        let snap = e.snapshot();
        let existed = snap.len();
        let mut batch: Vec<Announce> = Vec::with_capacity(existed + 1);
        let mut i = 0usize;
        while i < snap.len() {
            if let Some(a) = snap.get(i).copied() {
                batch.push(Announce {
                    id: a.id,
                    epoch: a.epoch.saturating_add(1),
                    addr: a.addr,
                    port: a.port,
                });
            }
            i += 1;
        }
        batch.push(Announce {
            id: 100u16.saturating_add(ep as u16),
            epoch: 1,
            addr: 0x0a00_1000u32.saturating_add(ep),
            port: 631,
        });

        let first = e.recover_batch(&batch, t.saturating_add(20))?;
        audit.applied_merged = audit.applied_merged.saturating_add(first.merged);
        audit.applied_added = audit.applied_added.saturating_add(first.added);
        audit.replays = audit.replays.saturating_add(first.replays);

        let before = e.table_digest();
        let replay = e.recover_batch(&batch, t.saturating_add(21))?;
        let after = e.table_digest();
        if before != after || replay.merged != 0 || replay.added != 0 {
            audit.idempotent_violations = audit.idempotent_violations.saturating_add(1);
        }
        audit.replays = audit.replays.saturating_add(replay.replays);
        e.finish_recovery(t.saturating_add(30), true)?;
        audit.episodes = audit.episodes.saturating_add(1);
        ep = ep.saturating_add(1);
    }
    audit.duplicates = e.table().duplicate_ids();
    audit.final_peers = e.peer_count();
    audit.ledger_merged = e.ledger().merged();
    audit.ledger_added = e.ledger().added();
    Ok(audit)
}

// ---------------------------------------------------------------------------
// 注入核：12 类坏样本
// ---------------------------------------------------------------------------

fn ok_engine() -> RecoveryEngine {
    match RecoveryEngine::new(7) {
        Ok(e) => e,
        Err(_) => RecoveryEngine {
            own_id: 7,
            state: LinkState::Up,
            misses: 0,
            table: PeerTable::new(),
            ledger: PartitionLedger::new(),
            open: None,
            replays: 0,
        },
    }
}

/// 走完一次「分区 → 恢复」的前半程，返回处于 Resyncing 的引擎。
fn resyncing_engine() -> RecoveryEngine {
    let mut e = ok_engine();
    let mut i = 0u32;
    while i < MISS_LIMIT {
        let _ = e.on_miss(i);
        i += 1;
    }
    let _ = e.on_link_up(10);
    e
}

/// 注入核：12 类坏样本 → 12 个具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    if RecoveryEngine::new(0).err() == Some(NetError::BadPeerId) {
        caught += 1;
    }
    let mut e = ok_engine();
    let self_ann = Announce {
        id: 7,
        epoch: 1,
        addr: 0x0a00_0007,
        port: 631,
    };
    if e.learn(self_ann, 0).err() == Some(NetError::SelfPeer) {
        caught += 1;
    }
    if e.learn(Announce { id: 9, epoch: 0, addr: 0x0a00_0009, port: 631 }, 0).err()
        == Some(NetError::BadEpoch)
    {
        caught += 1;
    }
    if e.on_link_up(0).err() == Some(NetError::NotPartitioned) {
        caught += 1;
    }
    // 已分区再丢心跳
    let mut p = ok_engine();
    let mut i = 0u32;
    while i < MISS_LIMIT {
        let _ = p.on_miss(i);
        i += 1;
    }
    if p.on_miss(5).err() == Some(NetError::AlreadyPartitioned) {
        caught += 1;
    }
    // 非 Resyncing 收批次
    if p.recover_batch(&[], 6).err() == Some(NetError::NotResyncing) {
        caught += 1;
    }
    // 空批次
    let mut r = resyncing_engine();
    if r.recover_batch(&[], 11).err() == Some(NetError::EmptyBatch) {
        caught += 1;
    }
    // 批内重复
    let dup = [
        Announce { id: 20, epoch: 1, addr: 0x0a00_0020, port: 631 },
        Announce { id: 20, epoch: 2, addr: 0x0a00_0021, port: 631 },
    ];
    if r.recover_batch(&dup, 11).err() == Some(NetError::DuplicateInBatch) {
        caught += 1;
    }
    // 陈旧 epoch：先学 epoch 5，再在恢复期收到 epoch 4
    let mut s = resyncing_engine();
    let _ = s.learn(Announce { id: 30, epoch: 5, addr: 0x0a00_0030, port: 631 }, 5);
    if s.recover_batch(&[Announce { id: 30, epoch: 4, addr: 0x0a00_0030, port: 631 }], 12).err()
        == Some(NetError::StaleEpoch)
    {
        caught += 1;
    }
    // 容量满：先把表灌满，再恢复期加一台新设备
    let mut full = resyncing_engine();
    let mut k = 0u16;
    while k < PEER_CAP as u16 {
        let id = 200 + k;
        let _ = full.learn(Announce { id, epoch: 1, addr: 0x0a00_0200 + id as u32, port: 631 }, 1);
        k += 1;
    }
    if full.recover_batch(&[Announce { id: 400, epoch: 1, addr: 0x0a00_0400, port: 631 }], 12).err()
        == Some(NetError::TableFull)
    {
        caught += 1;
    }
    // 非法状态转移：Up → Resyncing
    let mut bad = ok_engine();
    if bad.transition_to(LinkState::Resyncing).err() == Some(NetError::BadState) {
        caught += 1;
    }
    // 账本满：连续 16 次完整分区后，第 17 次开不了账
    let mut led = ok_engine();
    let mut n = 0u32;
    while n < EPISODE_CAP as u32 {
        let t = 100 + n * 10;
        let mut m = 0u32;
        while m < MISS_LIMIT {
            let _ = led.on_miss(t + m);
            m += 1;
        }
        let _ = led.on_link_up(t + 5);
        let _ = led.finish_recovery(t + 6, false);
        n += 1;
    }
    let t = 100 + EPISODE_CAP as u32 * 10;
    let mut m = 0u32;
    let mut last = Ok(LinkState::Up);
    while m < MISS_LIMIT {
        last = led.on_miss(t + m);
        m += 1;
    }
    if last.err() == Some(NetError::LedgerFull) {
        caught += 1;
    }

    (caught.min(total), total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50148_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50148");

    // J1-1 分区 ×10 恢复零重复表项（判据原文）
    let audit = recovery_audit(10);
    let ok = match audit {
        Ok(a) => {
            a.episodes == 10
                && a.duplicates == 0
                && a.idempotent_violations == 0
                && a.final_peers == 16
                && a.applied_added == 10
                && a.applied_merged == a.ledger_merged
                && a.applied_added == a.ledger_added
        }
        Err(_) => false,
    };
    let det = match audit {
        Ok(a) => format!(
            "eps={} dup={} merged={} added={} rep={} peers={}",
            a.episodes, a.duplicates, a.applied_merged, a.applied_added, a.replays, a.final_peers
        ),
        Err(e) => format!("audit err {}", e.code()),
    };
    s.add_dyn("50148-J1-十字恢复零重复", ok, &det);

    // J1-2 状态机合法链：Up→Degraded→Partitioned→Resyncing→Up
    let mut e = ok_engine();
    let ok = e.on_miss(1).is_ok()
        && e.state() == LinkState::Degraded
        && e.on_miss(2).is_ok()
        && e.on_miss(3).is_ok()
        && e.state() == LinkState::Partitioned
        && e.on_link_up(4).is_ok()
        && e.state() == LinkState::Resyncing
        && e.finish_recovery(5, false).is_ok()
        && e.state() == LinkState::Up;
    s.add("50148-J1-状态链合法", ok, if ok { "" } else { "状态转移断裂" });

    // J1-3 抖动不分区：1~2 次丢包停在 Degraded，心跳回来即 Up
    let mut j = ok_engine();
    let _ = j.learn(Announce { id: 11, epoch: 1, addr: 0x0a00_0011, port: 631 }, 0);
    let _ = j.on_miss(1);
    let _ = j.on_miss(2);
    let ok = j.state() == LinkState::Degraded
        && j.peer_count() == 1
        && j.on_heartbeat(3).is_ok()
        && j.state() == LinkState::Up
        && j.ledger().is_empty();
    s.add("50148-J1-抖动不分区", ok, if ok { "" } else { "抖动被误判为分区" });

    // J1-4 恢复期重放幂等：同批两次，表摘要不变、合并/新增均为 0
    let mut r = resyncing_engine();
    let _ = r.learn(Announce { id: 12, epoch: 3, addr: 0x0a00_0012, port: 631 }, 1);
    let batch = [Announce { id: 12, epoch: 4, addr: 0x0a00_0012, port: 631 }];
    let first = r.recover_batch(&batch, 11);
    let mid = r.table_digest();
    let second = r.recover_batch(&batch, 12);
    let ok = first == Ok(BatchReport { merged: 1, added: 0, replays: 0 })
        && second == Ok(BatchReport { merged: 0, added: 0, replays: 1 })
        && mid == r.table_digest()
        && r.peer_count() == 1;
    s.add("50148-J1-重放幂等", ok, if ok { "" } else { "重放改动了表" });

    // J1-5 陈旧 epoch 必拒且零半写（整批原子）
    let mut st = resyncing_engine();
    let _ = st.learn(Announce { id: 13, epoch: 9, addr: 0x0a00_0013, port: 631 }, 1);
    let before = st.table_digest();
    let bad = [
        Announce { id: 14, epoch: 1, addr: 0x0a00_0014, port: 631 },
        Announce { id: 13, epoch: 8, addr: 0x0a00_0013, port: 631 },
    ];
    let ok = st.recover_batch(&bad, 12).err() == Some(NetError::StaleEpoch)
        && st.table_digest() == before
        && st.peer_count() == 1;
    s.add("50148-J1-陈旧epoch拒绝", ok, if ok { "" } else { "陈旧批次半写" });

    // J1-6 容量满必拒且表不变
    let mut f = ok_engine();
    let mut i = 0u16;
    while i < PEER_CAP as u16 {
        let id = 40 + i;
        let _ = f.learn(Announce { id, epoch: 1, addr: 0x0a00_0030 + id as u32, port: 631 }, 0);
        i += 1;
    }
    let before = f.table_digest();
    let ok = f.learn(Announce { id: 900, epoch: 1, addr: 0x0a00_0900, port: 631 }, 1).err()
        == Some(NetError::TableFull)
        && f.table_digest() == before;
    s.add("50148-J1-满表必拒", ok, if ok { "" } else { "满表插入半写" });

    // J1-7 自 id / 广播 id 必拒
    let mut sid = ok_engine();
    let ok = sid.learn(Announce { id: 7, epoch: 1, addr: 0x0a00_0007, port: 631 }, 0).err()
        == Some(NetError::SelfPeer)
        && sid.learn(Announce { id: u16::MAX, epoch: 1, addr: 0x0a00_00ff, port: 631 }, 0).err()
            == Some(NetError::BadPeerId)
        && sid.peer_count() == 0;
    s.add("50148-J1-自id拒绝", ok, if ok { "" } else { "自 id 入表" });

    // J1-8 未分区不得恢复、非 Resyncing 不得收批次
    let mut up = ok_engine();
    let a = [Announce { id: 50, epoch: 1, addr: 0x0a00_0050, port: 631 }];
    let ok = up.on_link_up(1).err() == Some(NetError::NotPartitioned)
        && up.recover_batch(&a, 1).err() == Some(NetError::NotResyncing)
        && up.peer_count() == 0;
    s.add("50148-J1-序外拒绝", ok, if ok { "" } else { "序外调用被接受" });

    // J1-9 修剪：恢复期没再声明的邻居被摘掉并记入账
    let mut pr = resyncing_engine();
    let old = Announce { id: 60, epoch: 1, addr: 0x0a00_0060, port: 631 };
    let _ = pr.learn(old, 1);
    let keep = Announce { id: 61, epoch: 2, addr: 0x0a00_0061, port: 631 };
    let _ = pr.recover_batch(&[keep], 11);
    let ep = pr.finish_recovery(12, true);
    let ok = match ep {
        Ok(v) => v.pruned == 1 && v.merged == 0 && v.added == 1 && pr.peer_count() == 1 && pr.ledger().pruned() == 1,
        Err(_) => false,
    };
    s.add("50148-J1-修剪记账", ok, if ok { "" } else { "离线邻居未摘除" });

    // J1-10 账本守恒：分项之和 == 逐 episode 之和
    let mut lb = ok_engine();
    let mut n = 0u32;
    while n < 4 {
        let t = 10 + n * 10;
        let mut m = 0u32;
        while m < MISS_LIMIT {
            let _ = lb.on_miss(t + m);
            m += 1;
        }
        let _ = lb.on_link_up(t + 5);
        let _ = lb.recover_batch(&[Announce { id: 70 + n as u16, epoch: 1, addr: 0x0a00_0070 + n, port: 631 }], t + 6);
        let _ = lb.finish_recovery(t + 7, false);
        n += 1;
    }
    let ok = lb.ledger().len() == 4 && lb.ledger().balanced() && lb.ledger().added() == 4;
    s.add("50148-J1-账本守恒", ok, if ok { "" } else { "账本分项不守恒" });

    // J1-11 注入核全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50148-J1-注入必抓", ok, &det);

    // J1-12 错误码闭集连续
    let mut seen = [false; 13];
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
    s.add("50148-J1-码值闭集", ok, if ok { "" } else { "码值重复或断裂" });

    // J1-13 摘要确定性：同序列两引擎逐位相同，epoch 变化则不同
    let build = |bump: bool| -> u64 {
        let mut e = ok_engine();
        let _ = e.learn(Announce { id: 80, epoch: 1, addr: 0x0a00_0080, port: 631 }, 0);
        let _ = e.learn(Announce { id: 81, epoch: 1, addr: 0x0a00_0081, port: 631 }, 1);
        if bump {
            let _ = e.learn(Announce { id: 81, epoch: 2, addr: 0x0a00_0081, port: 631 }, 2);
        }
        e.table_digest()
    };
    let ok = build(false) == build(false) && build(true) != build(false);
    s.add("50148-J1-摘要确定", ok, if ok { "" } else { "表摘要不确定" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50148_checks_green() {
        let set = run_unx50148_checks();
        assert!(set.all_passed(), "UNX-F50148 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50148_no_duplicate_after_ten_episodes() {
        match recovery_audit(10) {
            Ok(a) => {
                assert_eq!(a.duplicates, 0);
                assert_eq!(a.episodes, 10);
                assert_eq!(a.idempotent_violations, 0);
            }
            Err(_) => panic!("recovery audit failed"),
        }
    }

    #[test]
    fn unx50148_merge_keeps_single_row() {
        let mut e = ok_engine();
        assert!(e.learn(Announce { id: 5, epoch: 1, addr: 1, port: 631 }, 0).is_ok());
        assert!(e.learn(Announce { id: 5, epoch: 2, addr: 2, port: 631 }, 1).is_ok());
        assert_eq!(e.peer_count(), 1);
    }
}
