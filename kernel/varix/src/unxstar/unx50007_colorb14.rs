//! UNX-F50007 色彩与 B14 基础档联签（单源零改写）（0x42d00 段）
//!
//! 锚点：UNX-F50007-J1 联签登记；判据锚定 §AI-63 §1 防重声明。
//!
//! 数据段 SEG_BASE = 0x42d00，诊断码闭集为 Base+1..Base+12，越界即实现缺陷。
//! 本模块解决的是「同一份色彩源同时被色彩管线与 B14 基础档引用」时的防重问题：
//! 任何联签都必须登记到唯一指纹上，签发前后不得出现第二次改写（single source,
//! zero rewrite）。指纹取自 profile 的量化数值（1e-6 定点），不依赖浮点比较，
//! 因此同一源在任意次重读下逐位一致。
//!
//! 结构清单：
//! * `SourceSnapshot` 色彩源快照 + FNV-1a 指纹（单源零改写的度量口径）；
//! * `BaseTier` B14 三档登记表（法定签署人数随档位单调不减）；
//! * `CoSignState` 六态状态机（Draft/Reviewed/Signed/Published/Revoked/Expired）；
//! * `CoSignLedger` 联签账本（容量、票号单调、重复登记拒收、超时过期、源漂移审计）；
//! * 注入核 `injection_core()`：六类坏样本逐类断言显式拒绝。
//!
//! 错误纪律：内核路径零 panic——所有查询走 `get()`，所有可失败入口返回
//! `Result<_, CoSignError>`；账本不持有 profile 本体，只持有不可变量。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::unx50001_gamutmap::{IccProfile, Trc};

/// 诊断段（0x42d00 独占 0x20）。
pub const SEG_BASE: u32 = 0x42d00;

pub const E_EMPTY: u16 = 1;
pub const E_CAPACITY: u16 = 2;
pub const E_DUPLICATE: u16 = 3;
pub const E_TRANSITION: u16 = 4;
pub const E_DRIFT: u16 = 5;
pub const E_TIER: u16 = 6;
pub const E_ORDER: u16 = 7;
pub const E_TIMEOUT: u16 = 8;
pub const E_QUORUM: u16 = 9;
pub const E_REVOKED: u16 = 10;
pub const E_RANGE: u16 = 11;
pub const E_UNKNOWN: u16 = 12;

/// 联签错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoSignError {
    Empty,
    Capacity,
    Duplicate,
    Transition,
    Drift,
    Tier,
    Order,
    Timeout,
    Quorum,
    Revoked,
    Range,
    Unknown,
}

impl CoSignError {
    pub fn code(self) -> u32 {
        let n = match self {
            CoSignError::Empty => E_EMPTY,
            CoSignError::Capacity => E_CAPACITY,
            CoSignError::Duplicate => E_DUPLICATE,
            CoSignError::Transition => E_TRANSITION,
            CoSignError::Drift => E_DRIFT,
            CoSignError::Tier => E_TIER,
            CoSignError::Order => E_ORDER,
            CoSignError::Timeout => E_TIMEOUT,
            CoSignError::Quorum => E_QUORUM,
            CoSignError::Revoked => E_REVOKED,
            CoSignError::Range => E_RANGE,
            CoSignError::Unknown => E_UNKNOWN,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            CoSignError::Empty => "ledger empty or source missing",
            CoSignError::Capacity => "ledger capacity reached",
            CoSignError::Duplicate => "same tier+fingerprint already registered",
            CoSignError::Transition => "illegal co-sign state transition",
            CoSignError::Drift => "source fingerprint drifted after registration",
            CoSignError::Tier => "B14 base tier unknown",
            CoSignError::Order => "ticket out of monotonic order",
            CoSignError::Timeout => "co-sign deadline exceeded",
            CoSignError::Quorum => "signer quorum not reached",
            CoSignError::Revoked => "entry already revoked",
            CoSignError::Range => "index outside legal range",
            CoSignError::Unknown => "ticket not registered",
        }
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 1e-6 定点量化。NaN 归一到 i32::MIN（不会与合法值碰撞），
/// 超界值夹到 ±1e7，保证 `as i32` 转换本身不产生 UB 且结果可复算。
fn quantize(v: f32) -> i32 {
    if v != v {
        return i32::MIN;
    }
    if v > 1.0e4 {
        return 10_000_000;
    }
    if v < -1.0e4 {
        return -10_000_000;
    }
    let s = v * 1.0e6;
    let r = if s >= 0.0 { s + 0.5 } else { s - 0.5 };
    r as i32
}

fn feed_u64(h: &mut u64, v: u64) {
    let mut i = 0u32;
    while i < 8 {
        *h ^= (v >> (i * 8)) & 0xff;
        *h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
}

/// 色彩源快照：只保留参与指纹的标量与矩阵，不持有 profile 本体。
/// 这是「单源」的载体——两份快照指纹相同即视为同一份源。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceSnapshot {
    pub white: [f32; 3],
    pub colorant: [[f32; 3]; 3],
    pub trc_kind: u8,
    pub trc_param: f32,
    pub trc_len: u16,
    pub trc_hash: u64,
    pub intent: u8,
    pub tag_count: u16,
    pub size_bytes: u32,
}

impl SourceSnapshot {
    /// 从已解析 profile 抽取快照。表型 TRC 只折叠长度与 ≤256 点的量化哈希，
    /// 避免大表把快照撑大（纪律：定长结构不驻留大缓冲）。
    pub fn from_profile(p: &IccProfile) -> SourceSnapshot {
        let (kind, param, len, hash) = match &p.trc {
            Trc::Srgb => (0u8, 0.0f32, 0u16, 0u64),
            Trc::Gamma(g) => (1u8, *g, 0u16, 0u64),
            Trc::Table(v) => {
                let mut h = FNV_OFFSET;
                let n = if v.len() > 256 { 256 } else { v.len() };
                let mut i = 0usize;
                while i < n {
                    match v.get(i) {
                        Some(x) => feed_u64(&mut h, quantize(*x) as u32 as u64),
                        None => break,
                    }
                    i += 1;
                }
                let l = if v.len() > 65535 { 65535u16 } else { v.len() as u16 };
                (2u8, 0.0f32, l, h)
            }
        };
        SourceSnapshot {
            white: [p.white.x, p.white.y, p.white.z],
            colorant: p.rgb_to_xyz.m,
            trc_kind: kind,
            trc_param: param,
            trc_len: len,
            trc_hash: hash,
            intent: p.intent.code(),
            tag_count: p.tag_count,
            size_bytes: p.size_bytes,
        }
    }

    /// 单源指纹。量化后逐字段 FNV-1a，字段顺序固定 → 同源同值、异源异值。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        feed_u64(&mut h, self.trc_kind as u64);
        feed_u64(&mut h, quantize(self.trc_param) as u32 as u64);
        feed_u64(&mut h, self.trc_len as u64);
        feed_u64(&mut h, self.trc_hash);
        feed_u64(&mut h, self.intent as u64);
        feed_u64(&mut h, self.tag_count as u64);
        feed_u64(&mut h, self.size_bytes as u64);
        let mut i = 0usize;
        while i < 3 {
            match self.white.get(i) {
                Some(v) => feed_u64(&mut h, quantize(*v) as u32 as u64),
                None => break,
            }
            i += 1;
        }
        i = 0;
        while i < 3 {
            match self.colorant.get(i) {
                Some(row) => {
                    let mut j = 0usize;
                    while j < 3 {
                        match row.get(j) {
                            Some(v) => feed_u64(&mut h, quantize(*v) as u32 as u64),
                            None => break,
                        }
                        j += 1;
                    }
                }
                None => break,
            }
            i += 1;
        }
        h
    }

    /// 指纹相等即同一份源。留成具名函数，调用点无需关心哈希实现。
    pub fn same_source(&self, other: &SourceSnapshot) -> bool {
        self.fingerprint() == other.fingerprint()
    }

    /// 构造一个「被动过」的快照用于漂移审计：idx 0..8 打色料矩阵，
    /// 9..11 打白点，其余索引返回原位（不引入随机性）。
    pub fn tapped(&self, idx: usize, delta: f32) -> SourceSnapshot {
        let mut out = *self;
        let mut i = 0usize;
        while i < 3 {
            let mut j = 0usize;
            while j < 3 {
                if idx == i * 3 + j {
                    if let Some(row) = out.colorant.get_mut(i) {
                        if let Some(cell) = row.get_mut(j) {
                            *cell += delta;
                        }
                    }
                }
                j += 1;
            }
            i += 1;
        }
        if idx >= 9 && idx <= 11 {
            if let Some(cell) = out.white.get_mut(idx - 9) {
                *cell += delta;
            }
        }
        out
    }
}

/// B14 基础档三档。quorum 为该档签发所需的独立签署方数量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseTier {
    B14Core,
    B14Standard,
    B14Extended,
}

impl BaseTier {
    pub fn code(self) -> u8 {
        match self {
            BaseTier::B14Core => 0,
            BaseTier::B14Standard => 1,
            BaseTier::B14Extended => 2,
        }
    }

    pub fn from_code(c: u8) -> Result<BaseTier, CoSignError> {
        match c {
            0 => Ok(BaseTier::B14Core),
            1 => Ok(BaseTier::B14Standard),
            2 => Ok(BaseTier::B14Extended),
            _ => Err(CoSignError::Tier),
        }
    }

    /// B14 基础档法定人数：核心档单签、标准档双签、扩展档三签。
    pub fn quorum(self) -> u8 {
        match self {
            BaseTier::B14Core => 1,
            BaseTier::B14Standard => 2,
            BaseTier::B14Extended => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BaseTier::B14Core => "B14-core",
            BaseTier::B14Standard => "B14-standard",
            BaseTier::B14Extended => "B14-extended",
        }
    }
}

/// 三档登记表。判据用它验证档位表本身没有缺档或重码。
pub const TIER_TABLE: [BaseTier; 3] = [
    BaseTier::B14Core,
    BaseTier::B14Standard,
    BaseTier::B14Extended,
];

/// 联签六态。终态 Revoked / Expired 不再接受签发。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoSignState {
    Draft,
    Reviewed,
    Signed,
    Published,
    Revoked,
    Expired,
}

impl CoSignState {
    pub fn label(self) -> &'static str {
        match self {
            CoSignState::Draft => "draft",
            CoSignState::Reviewed => "reviewed",
            CoSignState::Signed => "signed",
            CoSignState::Published => "published",
            CoSignState::Revoked => "revoked",
            CoSignState::Expired => "expired",
        }
    }

    /// 显式转移表。不在这张表里的组合一律拒绝——防的是「跳档签发」
    /// 这类静默通过：草稿绝不允许直接进入已发布。
    pub fn allows(self, next: CoSignState) -> bool {
        match (self, next) {
            (CoSignState::Draft, CoSignState::Reviewed) => true,
            (CoSignState::Draft, CoSignState::Revoked) => true,
            (CoSignState::Draft, CoSignState::Expired) => true,
            (CoSignState::Reviewed, CoSignState::Signed) => true,
            (CoSignState::Reviewed, CoSignState::Revoked) => true,
            (CoSignState::Reviewed, CoSignState::Expired) => true,
            (CoSignState::Signed, CoSignState::Published) => true,
            (CoSignState::Signed, CoSignState::Revoked) => true,
            (CoSignState::Signed, CoSignState::Expired) => true,
            (CoSignState::Published, CoSignState::Revoked) => true,
            _ => false,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, CoSignState::Revoked | CoSignState::Expired)
    }
}

/// 一条联签登记。seq 即票号，由调用方给出以便乱序可被检出。
#[derive(Clone, Debug, PartialEq)]
pub struct SignEntry {
    pub seq: u64,
    pub tier: BaseTier,
    pub fingerprint: u64,
    pub state: CoSignState,
    pub signers: u8,
    pub quorum: u8,
    pub deadline: u64,
    pub label: String,
}

impl SignEntry {
    pub fn new(seq: u64, tier: BaseTier, fingerprint: u64, deadline: u64) -> SignEntry {
        SignEntry {
            seq,
            tier,
            fingerprint,
            state: CoSignState::Draft,
            signers: 0,
            quorum: tier.quorum(),
            deadline,
            label: tier.label().to_string(),
        }
    }

    pub fn signed_by(&self, role: u8) -> bool {
        role != 0 && (self.signers & role) == role
    }

    pub fn signer_count(&self) -> u32 {
        self.signers.count_ones()
    }
}

/// 联签账本。所有状态改写集中在这里，外部拿不到可变的 entry。
#[derive(Clone, Debug)]
pub struct CoSignLedger {
    cap: usize,
    last_ticket: u64,
    entries: Vec<SignEntry>,
    rejections: u32,
    rewrites: u32,
}

impl CoSignLedger {
    pub fn new(cap: usize) -> CoSignLedger {
        CoSignLedger {
            cap,
            last_ticket: 0,
            entries: Vec::new(),
            rejections: 0,
            rewrites: 0,
        }
    }

    fn index_of(&self, seq: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.seq == seq {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条草稿。检查顺序：容量 → 票号单调 → 期限 → 重复（档位+指纹）。
    /// 顺序固定是为了让每个坏样本只命中一个确定诊断码，便于零漂移断言。
    pub fn draft(
        &mut self,
        ticket: u64,
        tier: BaseTier,
        src: &SourceSnapshot,
        now: u64,
        deadline: u64,
    ) -> Result<u64, CoSignError> {
        if self.entries.len() >= self.cap {
            self.rejections += 1;
            return Err(CoSignError::Capacity);
        }
        if ticket <= self.last_ticket {
            self.rejections += 1;
            return Err(CoSignError::Order);
        }
        if deadline <= now {
            self.rejections += 1;
            return Err(CoSignError::Timeout);
        }
        let fp = src.fingerprint();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.tier == tier && e.fingerprint == fp && e.state != CoSignState::Revoked {
                    self.rejections += 1;
                    return Err(CoSignError::Duplicate);
                }
            }
            i += 1;
        }
        self.entries.push(SignEntry::new(ticket, tier, fp, deadline));
        self.last_ticket = ticket;
        Ok(ticket)
    }

    fn require(&self, seq: u64, want: CoSignState) -> Result<usize, CoSignError> {
        let i = match self.index_of(seq) {
            Some(i) => i,
            None => return Err(CoSignError::Unknown),
        };
        match self.entries.get(i) {
            Some(e) => {
                if e.state != want {
                    if e.state.is_terminal() {
                        return Err(CoSignError::Revoked);
                    }
                    return Err(CoSignError::Transition);
                }
                Ok(i)
            }
            None => Err(CoSignError::Unknown),
        }
    }

    fn move_to(&mut self, seq: u64, next: CoSignState) -> Result<(), CoSignError> {
        let i = match self.index_of(seq) {
            Some(i) => i,
            None => return Err(CoSignError::Unknown),
        };
        let cur = match self.entries.get(i) {
            Some(e) => e.state,
            None => return Err(CoSignError::Unknown),
        };
        if !cur.allows(next) {
            self.rejections += 1;
            return Err(CoSignError::Transition);
        }
        if let Some(e) = self.entries.get_mut(i) {
            e.state = next;
        }
        Ok(())
    }

    /// 草稿复核。超期直接转 Expired 并报 Timeout——过期不是静默事实。
    pub fn review(&mut self, seq: u64, now: u64) -> Result<(), CoSignError> {
        let i = self.require(seq, CoSignState::Draft)?;
        let deadline = match self.entries.get(i) {
            Some(e) => e.deadline,
            None => return Err(CoSignError::Unknown),
        };
        if now > deadline {
            if let Some(e) = self.entries.get_mut(i) {
                e.state = CoSignState::Expired;
            }
            self.rejections += 1;
            return Err(CoSignError::Timeout);
        }
        self.move_to(seq, CoSignState::Reviewed)
    }

    /// 联合签署一方（role 必须是单一位）。达到法定人数即自动转 Signed。
    /// 重复签同一 role 报 Duplicate，不静默覆盖。
    pub fn co_sign(&mut self, seq: u64, role: u8, now: u64) -> Result<CoSignState, CoSignError> {
        let i = self.require(seq, CoSignState::Reviewed)?;
        if role == 0 || (role & (role - 1)) != 0 {
            self.rejections += 1;
            return Err(CoSignError::Range);
        }
        let (deadline, quorum, mut mask) = match self.entries.get(i) {
            Some(e) => (e.deadline, e.quorum, e.signers),
            None => return Err(CoSignError::Unknown),
        };
        if now > deadline {
            if let Some(e) = self.entries.get_mut(i) {
                e.state = CoSignState::Expired;
            }
            self.rejections += 1;
            return Err(CoSignError::Timeout);
        }
        if (mask & role) == role {
            self.rejections += 1;
            return Err(CoSignError::Duplicate);
        }
        mask |= role;
        let reached = mask.count_ones() >= quorum as u32;
        if let Some(e) = self.entries.get_mut(i) {
            e.signers = mask;
        }
        if reached {
            self.move_to(seq, CoSignState::Signed)?;
        }
        match self.entries.get(i) {
            Some(e) => Ok(e.state),
            None => Err(CoSignError::Unknown),
        }
    }

    /// 发布：必须已 Signed、人数达标、未超期、且源指纹与登记值逐位相同。
    /// 指纹不一致即视为「二次改写」，返回 Drift 并计入 rewrites 审计计数。
    pub fn publish(
        &mut self,
        seq: u64,
        now: u64,
        src: &SourceSnapshot,
    ) -> Result<(), CoSignError> {
        let i = self.require(seq, CoSignState::Signed)?;
        let (deadline, quorum, signers, fp) = match self.entries.get(i) {
            Some(e) => (e.deadline, e.quorum, e.signers, e.fingerprint),
            None => return Err(CoSignError::Unknown),
        };
        if now > deadline {
            if let Some(e) = self.entries.get_mut(i) {
                e.state = CoSignState::Expired;
            }
            self.rejections += 1;
            return Err(CoSignError::Timeout);
        }
        if signers.count_ones() < quorum as u32 {
            self.rejections += 1;
            return Err(CoSignError::Quorum);
        }
        if src.fingerprint() != fp {
            self.rewrites += 1;
            self.rejections += 1;
            return Err(CoSignError::Drift);
        }
        self.move_to(seq, CoSignState::Published)
    }

    /// 撤销：已发布条目可撤销，终态条目不可再撤销。
    pub fn revoke(&mut self, seq: u64) -> Result<(), CoSignError> {
        self.move_to(seq, CoSignState::Revoked)
    }

    /// 到期清扫：把所有未终态且已过期的条目转 Expired，返回条数。
    pub fn expire(&mut self, now: u64) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            let mut hit = false;
            if let Some(e) = self.entries.get(i) {
                if !e.state.is_terminal() && e.state != CoSignState::Published && now > e.deadline
                {
                    hit = true;
                }
            }
            if hit {
                if let Some(e) = self.entries.get_mut(i) {
                    e.state = CoSignState::Expired;
                }
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn get(&self, seq: u64) -> Option<&SignEntry> {
        match self.index_of(seq) {
            Some(i) => self.entries.get(i),
            None => None,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn rejections(&self) -> u32 {
        self.rejections
    }

    pub fn rewrites(&self) -> u32 {
        self.rewrites
    }

    /// 单源审计：返回（与给定源指纹一致的 Signed/Published 条数，漂移条数）。
    /// 零改写的判据就是 drifted == 0 且 matched > 0。
    pub fn audit_source(&self, src: &SourceSnapshot) -> (usize, usize) {
        let fp = src.fingerprint();
        let mut matched = 0usize;
        let mut drifted = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.state == CoSignState::Signed || e.state == CoSignState::Published {
                    if e.fingerprint == fp {
                        matched += 1;
                    } else {
                        drifted += 1;
                    }
                }
            }
            i += 1;
        }
        (matched, drifted)
    }

    /// 状态计数摘要（非空）。供上层写回「结果摘要」字段。
    pub fn summary(&self) -> String {
        let mut published = 0usize;
        let mut signed = 0usize;
        let mut revoked = 0usize;
        let mut expired = 0usize;
        let mut draft = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                match e.state {
                    CoSignState::Published => published += 1,
                    CoSignState::Signed => signed += 1,
                    CoSignState::Revoked => revoked += 1,
                    CoSignState::Expired => expired += 1,
                    _ => draft += 1,
                }
            }
            i += 1;
        }
        format!(
            "B14联签 entries={} published={} signed={} draft={} revoked={} expired={} rej={} rewrite={}",
            self.entries.len(),
            published,
            signed,
            draft,
            revoked,
            expired,
            self.rejections,
            self.rewrites
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核：六类坏样本必须逐类显式拒绝
// ---------------------------------------------------------------------------

/// 六个坏样本 → 命中数。每类都对应一个不同的诊断码，命中即证明拒绝路径真实存在。
pub fn injection_core() -> (usize, usize) {
    let src = SourceSnapshot::from_profile(&IccProfile::builtin_srgb());
    let mut caught = 0usize;
    let total = 6usize;

    // ① 未登记票号发布
    let mut a = CoSignLedger::new(4);
    if a.publish(9, 0, &src).err() == Some(CoSignError::Unknown) {
        caught += 1;
    }

    // ② 草稿直接发布（跳档）
    let mut b = CoSignLedger::new(4);
    if b.draft(1, BaseTier::B14Core, &src, 0, 100).is_ok()
        && b.publish(1, 10, &src).err() == Some(CoSignError::Transition)
    {
        caught += 1;
    }

    // ③ 同一签署方重复签
    let mut c = CoSignLedger::new(4);
    let ok = c.draft(1, BaseTier::B14Standard, &src, 0, 100).is_ok()
        && c.review(1, 5).is_ok()
        && c.co_sign(1, 0b001, 6).is_ok()
        && c.co_sign(1, 0b001, 7).err() == Some(CoSignError::Duplicate);
    if ok {
        caught += 1;
    }

    // ④ 票号乱序
    let mut d = CoSignLedger::new(4);
    if d.draft(5, BaseTier::B14Core, &src, 0, 100).is_ok()
        && d.draft(4, BaseTier::B14Core, &src, 0, 100).err() == Some(CoSignError::Order)
    {
        caught += 1;
    }

    // ⑤ 源漂移（签发后改了色料矩阵）
    let mut e = CoSignLedger::new(4);
    let ok = e.draft(1, BaseTier::B14Core, &src, 0, 100).is_ok()
        && e.review(1, 5).is_ok()
        && e.co_sign(1, 0b001, 6).is_ok()
        && e.publish(1, 7, &src.tapped(0, 0.001)).err() == Some(CoSignError::Drift);
    if ok {
        caught += 1;
    }

    // ⑥ 容量溢出
    let mut f = CoSignLedger::new(1);
    if f.draft(1, BaseTier::B14Core, &src, 0, 100).is_ok()
        && f.draft(2, BaseTier::B14Standard, &src, 0, 100).err() == Some(CoSignError::Capacity)
    {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50007_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50007");
    let src = SourceSnapshot::from_profile(&IccProfile::builtin_srgb());

    // J1-1 指纹确定：两次调用逐位一致且非平凡
    let f1 = src.fingerprint();
    let f2 = SourceSnapshot::from_profile(&IccProfile::builtin_srgb()).fingerprint();
    let ok = f1 == f2 && f1 != 0 && f1 != FNV_OFFSET;
    let det = format!("fp={:016x}", f1);
    s.add_dyn("50007-J1-指纹确定", ok, &det);

    // J1-2 指纹灵敏：色料任一格动 1e-3 必变
    let mut changed = 0usize;
    let mut i = 0usize;
    while i < 9 {
        if src.tapped(i, 1e-3).fingerprint() != f1 {
            changed += 1;
        }
        i += 1;
    }
    let ok = changed == 9;
    let det = format!("changed={}/9", changed);
    s.add_dyn("50007-J1-指纹灵敏", ok, &det);

    // J1-3 指纹覆盖白点：白点三通道各自可分辨
    let ok = src.tapped(9, 1e-3).fingerprint() != f1
        && src.tapped(10, 1e-3).fingerprint() != f1
        && src.tapped(11, 1e-3).fingerprint() != f1;
    s.add("50007-J1-指纹白点", ok, if ok { "" } else { "白点未入指纹" });

    // J1-4 档位表：码互异、法定人数单调不减
    let mut codes = [false; 3];
    let mut quorum_ok = true;
    let mut prev = 0u8;
    let mut k = 0usize;
    while k < TIER_TABLE.len() {
        if let Some(t) = TIER_TABLE.get(k) {
            let c = t.code() as usize;
            if c < 3 {
                codes[c] = true;
            }
            if k > 0 && t.quorum() < prev {
                quorum_ok = false;
            }
            prev = t.quorum();
            if BaseTier::from_code(t.code()) != Ok(*t) {
                quorum_ok = false;
            }
        }
        k += 1;
    }
    let ok = codes.iter().all(|v| *v) && quorum_ok;
    s.add("50007-J1-档位登记", ok, if ok { "" } else { "档位表缺档或人数倒挂" });

    // J1-5 合法链路：草稿→复核→单签→发布
    let mut led = CoSignLedger::new(8);
    let mut ok = led.draft(1, BaseTier::B14Core, &src, 100, 1000) == Ok(1);
    ok &= led.review(1, 200).is_ok();
    ok &= led.co_sign(1, 0b001, 300) == Ok(CoSignState::Signed);
    ok &= led.publish(1, 400, &src).is_ok();
    let st = match led.get(1) {
        Some(e) => e.state,
        None => CoSignState::Draft,
    };
    let ok = ok && st == CoSignState::Published && led.len() == 1;
    let det = format!("state={} len={}", st.label(), led.len());
    s.add_dyn("50007-J1-合法联签链", ok, &det);

    // J1-6 非法跃迁：草稿→发布必须拒绝
    let mut led = CoSignLedger::new(4);
    let ok = led.draft(1, BaseTier::B14Core, &src, 0, 100).is_ok()
        && led.publish(1, 10, &src).err() == Some(CoSignError::Transition);
    s.add("50007-J1-跳档必拒", ok, if ok { "" } else { "草稿被直接发布" });

    // J1-7 重复登记：同档同指纹第二票必须拒
    let mut led = CoSignLedger::new(8);
    let ok = led.draft(1, BaseTier::B14Standard, &src, 0, 100).is_ok()
        && led.draft(2, BaseTier::B14Standard, &src, 0, 100).err()
            == Some(CoSignError::Duplicate);
    s.add("50007-J1-重复登记拒收", ok, if ok { "" } else { "重复登记未拦" });

    // J1-8 容量满：cap=1 第二票报容量
    let mut led = CoSignLedger::new(1);
    let ok = led.draft(1, BaseTier::B14Core, &src, 0, 100).is_ok()
        && led.draft(2, BaseTier::B14Extended, &src, 0, 100).err() == Some(CoSignError::Capacity);
    s.add("50007-J1-容量边界", ok, if ok { "" } else { "容量未生效" });

    // J1-9 票号乱序：回退票号必须拒
    let mut led = CoSignLedger::new(8);
    let ok = led.draft(10, BaseTier::B14Core, &src, 0, 100).is_ok()
        && led.draft(9, BaseTier::B14Standard, &src, 0, 100).err() == Some(CoSignError::Order);
    s.add("50007-J1-票号单调", ok, if ok { "" } else { "乱序票号被接受" });

    // J1-10 单源零改写：发布后审计 matched=1 drifted=0 且指纹未变
    let (matched, drifted) = led.audit_source(&src);
    let mut led2 = CoSignLedger::new(4);
    let mut ok = led2.draft(1, BaseTier::B14Core, &src, 0, 100).is_ok();
    ok &= led2.review(1, 10).is_ok();
    ok &= led2.co_sign(1, 0b001, 20) == Ok(CoSignState::Signed);
    ok &= led2.publish(1, 30, &src).is_ok();
    let (m2, d2) = led2.audit_source(&src);
    let fp_now = SourceSnapshot::from_profile(&IccProfile::builtin_srgb()).fingerprint();
    let ok = ok && m2 == 1 && d2 == 0 && fp_now == f1;
    let det = format!("prev({},{}) now({},{})", matched, drifted, m2, d2);
    s.add_dyn("50007-J1-单源零改写", ok, &det);

    // J1-11 源漂移：签发后改源，发布必报 Drift 且改写计数 +1
    let mut led = CoSignLedger::new(4);
    let ok = led.draft(1, BaseTier::B14Core, &src, 0, 100).is_ok()
        && led.review(1, 5).is_ok()
        && led.co_sign(1, 0b001, 6) == Ok(CoSignState::Signed);
    let drifted_src = src.tapped(4, 1e-3);
    let rejected = led.publish(1, 7, &drifted_src).err() == Some(CoSignError::Drift);
    let (m, d) = led.audit_source(&drifted_src);
    let ok = ok && rejected && led.rewrites() == 1 && m == 0 && d == 1;
    let det = format!("reject={} rewrites={} match={} drift={}", rejected, led.rewrites(), m, d);
    s.add_dyn("50007-J1-源漂移必拒", ok, &det);

    // J1-12 超时：过期发布报 Timeout，状态落 Expired；expire 清扫计数一致
    let mut led = CoSignLedger::new(4);
    let ok = led.draft(1, BaseTier::B14Core, &src, 10, 50).is_ok()
        && led.review(1, 20).is_ok()
        && led.co_sign(1, 0b001, 30).is_ok();
    let late = led.publish(1, 60, &src).err() == Some(CoSignError::Timeout);
    let st = match led.get(1) {
        Some(e) => e.state,
        None => CoSignState::Draft,
    };
    let expired = led.expire(70);
    let ok = ok && late && st == CoSignState::Expired && expired == 0;
    let det = format!("late={} state={} swept={}", late, st.label(), expired);
    s.add_dyn("50007-J1-超时显式", ok, &det);

    // J1-13 法定人数：标准档双签未满不得签发
    let mut led = CoSignLedger::new(4);
    let mut ok = led.draft(1, BaseTier::B14Standard, &src, 0, 100).is_ok();
    ok &= led.review(1, 5).is_ok();
    ok &= led.co_sign(1, 0b001, 6) == Ok(CoSignState::Reviewed);
    let st = led.co_sign(1, 0b010, 7);
    let ok = ok && st == Ok(CoSignState::Signed) && led.publish(1, 8, &src).is_ok();
    let det = format!("two={:?} quorum={}", st, BaseTier::B14Standard.quorum());
    s.add_dyn("50007-J1-法定人数", ok, &det);

    // J1-14 撤销后不可复活
    let mut led = CoSignLedger::new(4);
    let ok = led.draft(1, BaseTier::B14Core, &src, 0, 100).is_ok()
        && led.review(1, 5).is_ok()
        && led.revoke(1).is_ok()
        && led.review(1, 6).err() == Some(CoSignError::Revoked)
        && led.get(1).map(|e| e.state) == Some(CoSignState::Revoked);
    s.add("50007-J1-撤销终态", ok, if ok { "" } else { "撤销后仍可流转" });

    // J1-15 摘要非空且字段齐备（写回要求）
    let text = led.summary();
    let ok = !text.is_empty() && text.contains("entries=") && text.contains("rewrite=");
    s.add("50007-J1-摘要非空", ok, if ok { "" } else { "摘要缺失" });

    // J1-16 六类注入全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50007-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50007_checks_green() {
        let set = run_unx50007_checks();
        assert!(set.all_passed(), "UNX-F50007 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50007_fingerprint_stable() {
        let a = SourceSnapshot::from_profile(&IccProfile::builtin_srgb());
        let b = SourceSnapshot::from_profile(&IccProfile::simulated_printer(0.7));
        assert!(a.same_source(&a));
        assert!(!a.same_source(&b));
        assert!(a.fingerprint() != b.fingerprint());
    }

    #[test]
    fn unx50007_state_machine_table() {
        assert!(CoSignState::Draft.allows(CoSignState::Reviewed));
        assert!(!CoSignState::Draft.allows(CoSignState::Published));
        assert!(CoSignState::Signed.allows(CoSignState::Published));
        assert!(!CoSignState::Published.allows(CoSignState::Draft));
    }

    #[test]
    fn unx50007_error_codes_in_segment() {
        assert_eq!(CoSignError::Empty.code(), SEG_BASE + 1);
        assert_eq!(CoSignError::Unknown.code(), SEG_BASE + 12);
        assert!(CoSignError::Unknown.code() > CoSignError::Empty.code());
    }
}
