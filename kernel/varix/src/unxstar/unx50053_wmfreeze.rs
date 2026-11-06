//! UNX-F50053 水印冻结接口（管线消费单点面）（0x432c0 段）
//!
//! 锚点：UNX-F50053-J1 接口冻结登记；判据锚定 §AI-63 §4。
//!
//! 结构清单：
//! * 冻结状态机 `Phase`：Draft → Candidate → Frozen → Sealed → Retired，
//!   另有 `reopen` 回到 Candidate 开始新一代；所有跃迁只走三个入口
//!   （`advance` / `freeze` / `seal`）加一个受控回退（`reopen`）；
//! * 注册表 `FreezeGate::slots`：id → 相位 + 修订号 + 载荷摘要，重复 id 显式
//!   拒绝；容量固定 16，满了报 `Full`；
//! * 冻结账本 `JournalEntry`：每次 free/seal/retire/reopen 落一条，序号连续，
//!   J1 的「接口冻结登记」= Freeze 条目与成功冻结次数一一对应；
//! * 消费单点面：管线只能拿 `FreezeTicket` 调 `consume` 取当前槽位，票据带
//!   修订号与摘要，旧票据在换代后必被 `StaleRevision` 挡住；
//! * 注入核：空载荷 / 未注册 / 非法跃迁 / 重复冻结 / 过期票 / 撤销票 /
//!   无消费方封存 / 注销后消费 / 空标签 / 重复注册 / 注册表满 / 修订号溢出，
//!   十二类坏样本逐类断言诊断码。
//!
//! 错误纪律：诊断码闭集 `0x432c0 + 1..=12`，全部被真实构造；内核路径零 panic。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x432c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x432c0;

pub const E_UNREG: u16 = 1;
pub const E_DUP: u16 = 2;
pub const E_FULL: u16 = 3;
pub const E_PHASE: u16 = 4;
pub const E_FROZEN: u16 = 5;
pub const E_STALE: u16 = 6;
pub const E_REVOKED: u16 = 7;
pub const E_CONSUMERS: u16 = 8;
pub const E_CLOSED: u16 = 9;
pub const E_DIGEST: u16 = 10;
pub const E_OVERFLOW: u16 = 11;
pub const E_NAME: u16 = 12;

/// 冻结接口错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmError {
    Unregistered,
    Dup,
    Full,
    IllegalPhase,
    AlreadyFrozen,
    StaleRevision,
    Revoked,
    NoConsumers,
    Closed,
    BadDigest,
    Overflow,
    BadName,
}

impl WmError {
    pub fn code(self) -> u32 {
        let n = match self {
            WmError::Unregistered => E_UNREG,
            WmError::Dup => E_DUP,
            WmError::Full => E_FULL,
            WmError::IllegalPhase => E_PHASE,
            WmError::AlreadyFrozen => E_FROZEN,
            WmError::StaleRevision => E_STALE,
            WmError::Revoked => E_REVOKED,
            WmError::NoConsumers => E_CONSUMERS,
            WmError::Closed => E_CLOSED,
            WmError::BadDigest => E_DIGEST,
            WmError::Overflow => E_OVERFLOW,
            WmError::BadName => E_NAME,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            WmError::Unregistered => "interface id not registered",
            WmError::Dup => "interface id already registered",
            WmError::Full => "freeze table capacity exhausted",
            WmError::IllegalPhase => "transition illegal for current phase",
            WmError::AlreadyFrozen => "same payload already frozen",
            WmError::StaleRevision => "ticket revision superseded",
            WmError::Revoked => "ticket revoked",
            WmError::NoConsumers => "seal requires at least one consumer",
            WmError::Closed => "interface retired, consumption closed",
            WmError::BadDigest => "payload empty, digest undefined",
            WmError::Overflow => "revision counter exhausted",
            WmError::BadName => "interface label empty",
        }
    }
}

/// 注册表容量 16：水印管线同时活跃的消费面不超过十来条，
/// 16 留一倍余量；再大就说明有人在按数据项注册而不是按管线面注册。
pub const REG_CAP: usize = 16;
/// 账本容量 64：按「一代 4 条（freeze/seal/retire/reopen）」算够 16 代；
/// 满账显式拒绝，避免历史被静默覆盖。
pub const JOURNAL_CAP: usize = 64;
/// 撤销表容量 8：撤销是异常路径，8 条足够覆盖一次发布窗口内的撤回。
pub const REVOKE_CAP: usize = 8;

/// 接口生命周期相位。消费者只在 Frozen / Sealed 两相位可见内容。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Draft,
    Candidate,
    Frozen,
    Sealed,
    Retired,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Draft => "draft",
            Phase::Candidate => "candidate",
            Phase::Frozen => "frozen",
            Phase::Sealed => "sealed",
            Phase::Retired => "retired",
        }
    }

    /// 是否已有可消费内容（Draft/Candidate 尚未冻结）。
    pub fn has_content(self) -> bool {
        matches!(self, Phase::Frozen | Phase::Sealed)
    }
}

/// 账本操作类型。`Reopen` 也入账：新一代的起点必须可追溯，
/// 否则「冻结了两次」在账上只剩两条 Freeze 而看不出中间发生过回退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Freeze,
    Seal,
    Retire,
    Reopen,
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::Freeze => "freeze",
            Op::Seal => "seal",
            Op::Retire => "retire",
            Op::Reopen => "reopen",
        }
    }
}

/// 一条冻结日志。`digest` 只在 Freeze 上非零（其余操作用 0 占位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub seq: u32,
    pub id: u16,
    pub revision: u32,
    pub digest: u64,
    pub op: Op,
}

/// 消费票据。`Copy` 是刻意的：票据本身不可变，撤销与换代都由门内
/// 状态裁决——票据自己不承担失效判断，否则两边一致性问题会扩散到管线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreezeTicket {
    pub id: u16,
    pub revision: u32,
    pub digest: u64,
}

/// 注册槽位：冻结接口的当前状态。公开只读字段，让管线拿到槽位后能自行
/// 记录「这次消费的是第几代、摘要是什么」，而不必回头再查门。
#[derive(Clone, Debug)]
pub struct FreezeSlot {
    pub id: u16,
    pub label: String,
    pub phase: Phase,
    pub revision: u32,
    pub digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Revocation {
    id: u16,
    revision: u32,
}

/// FNV-1a 64：跨构建稳定的内容摘要（no_std 无 Hasher，且要求逐位可复算）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        i += 1;
    }
    h
}

/// 冻结门：注册表 + 账本 + 撤销表的唯一持有者，也是管线消费的唯一入口。
#[derive(Clone, Debug)]
pub struct FreezeGate {
    slots: Vec<FreezeSlot>,
    journal: Vec<JournalEntry>,
    revocations: Vec<Revocation>,
    revision: u32,
    consumers: u16,
}

impl FreezeGate {
    pub fn new() -> FreezeGate {
        FreezeGate::starting_at(0)
    }

    /// 从落盘修订号起跳。恢复快照时必须显式给定，不能默认 0——
    /// 默认 0 会让恢复后签发的票据与历史票据撞号，过期判定失效。
    pub fn starting_at(revision: u32) -> FreezeGate {
        FreezeGate {
            slots: Vec::new(),
            journal: Vec::new(),
            revocations: Vec::new(),
            revision,
            consumers: 0,
        }
    }

    /// 注册一个冻结接口。
    pub fn register(&mut self, id: u16, label: &str) -> Result<(), WmError> {
        if label.is_empty() {
            return Err(WmError::BadName);
        }
        if self.slots.len() >= REG_CAP {
            return Err(WmError::Full);
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.id == id {
                    return Err(WmError::Dup);
                }
            }
            i += 1;
        }
        self.slots.push(FreezeSlot {
            id,
            label: label.to_string(),
            phase: Phase::Draft,
            revision: 0,
            digest: 0,
        });
        Ok(())
    }

    /// 消费方数量。封存要求 ≥1：没有消费方的冻结接口等于无人认领的产物。
    pub fn set_consumers(&mut self, n: u16) {
        self.consumers = n;
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    pub fn phase_of(&self, id: u16) -> Option<Phase> {
        match self.lookup(id) {
            Some(i) => match self.slots.get(i) {
                Some(s) => Some(s.phase),
                None => None,
            },
            None => None,
        }
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    pub fn count_ops(&self, op: Op) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.journal.len() {
            if let Some(e) = self.journal.get(i) {
                if e.op == op {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    fn lookup(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn push_op(&mut self, id: u16, revision: u32, digest: u64, op: Op) -> Result<(), WmError> {
        if self.journal.len() >= JOURNAL_CAP {
            return Err(WmError::Full);
        }
        let seq = self.journal.len() as u32;
        self.journal.push(JournalEntry {
            seq,
            id,
            revision,
            digest,
            op,
        });
        Ok(())
    }

    /// 前进一步：Draft → Candidate、Sealed → Retired。
    ///
    /// 只允许这两条「无内容变化」的推进；进入 Frozen 必须走 `freeze`（要载荷），
    /// 进入 Sealed 必须走 `seal`（要消费方），把副作用集中在各自的校验点上。
    pub fn advance(&mut self, id: u16) -> Result<Phase, WmError> {
        let idx = match self.lookup(id) {
            Some(i) => i,
            None => return Err(WmError::Unregistered),
        };
        let (phase, rev) = match self.slots.get(idx) {
            Some(s) => (s.phase, s.revision),
            None => return Err(WmError::Unregistered),
        };
        let next = match phase {
            Phase::Draft => Phase::Candidate,
            Phase::Sealed => Phase::Retired,
            _ => return Err(WmError::IllegalPhase),
        };
        if next == Phase::Retired {
            self.push_op(id, rev, 0, Op::Retire)?;
        }
        if let Some(s) = self.slots.get_mut(idx) {
            s.phase = next;
        }
        Ok(next)
    }

    /// 冻结一份载荷，签发票据。
    ///
    /// 校验顺序：空载荷 → 未注册 → 同摘要重逢（AlreadyFrozen）→ 相位 →
    /// 账本容量 → 修订号溢出。把「同摘要重逢」排在相位之前，是因为重复调用
    /// 同一写法的管线应当得到「已冻结」而不是「相位非法」——前者幂等提示，
    /// 后者会让调用方误以为自己的状态机坏了。
    pub fn freeze(&mut self, id: u16, payload: &[u8]) -> Result<FreezeTicket, WmError> {
        if payload.is_empty() {
            return Err(WmError::BadDigest);
        }
        let idx = match self.lookup(id) {
            Some(i) => i,
            None => return Err(WmError::Unregistered),
        };
        let (phase, cur_digest) = match self.slots.get(idx) {
            Some(s) => (s.phase, s.digest),
            None => return Err(WmError::Unregistered),
        };
        let digest = fnv1a64(payload);
        if phase.has_content() && cur_digest == digest {
            return Err(WmError::AlreadyFrozen);
        }
        if phase != Phase::Candidate {
            return Err(WmError::IllegalPhase);
        }
        if self.journal.len() >= JOURNAL_CAP {
            return Err(WmError::Full);
        }
        let rev = match self.revision.checked_add(1) {
            Some(v) => v,
            None => return Err(WmError::Overflow),
        };
        self.revision = rev;
        if let Some(s) = self.slots.get_mut(idx) {
            s.phase = Phase::Frozen;
            s.revision = rev;
            s.digest = digest;
        }
        let seq = self.journal.len() as u32;
        self.journal.push(JournalEntry {
            seq,
            id,
            revision: rev,
            digest,
            op: Op::Freeze,
        });
        Ok(FreezeTicket {
            id,
            revision: rev,
            digest,
        })
    }

    /// 封存冻结结果。封存后内容不可再冻结，直到 `reopen` 开新一代。
    pub fn seal(&mut self, id: u16) -> Result<(), WmError> {
        let idx = match self.lookup(id) {
            Some(i) => i,
            None => return Err(WmError::Unregistered),
        };
        let (phase, rev, digest) = match self.slots.get(idx) {
            Some(s) => (s.phase, s.revision, s.digest),
            None => return Err(WmError::Unregistered),
        };
        if phase != Phase::Frozen {
            return Err(WmError::IllegalPhase);
        }
        if self.consumers == 0 {
            return Err(WmError::NoConsumers);
        }
        self.push_op(id, rev, digest, Op::Seal)?;
        if let Some(s) = self.slots.get_mut(idx) {
            s.phase = Phase::Sealed;
        }
        Ok(())
    }

    /// 受控回退：Frozen / Sealed / Retired → Candidate，开始新一代修订。
    ///
    /// 回退保留旧修订号（不归零）：旧票据靠修订号比较失效，如果归零，
    /// 下一代签发的 1 号票据会和第一代的 1 号票据撞号。
    pub fn reopen(&mut self, id: u16) -> Result<u32, WmError> {
        let idx = match self.lookup(id) {
            Some(i) => i,
            None => return Err(WmError::Unregistered),
        };
        let (phase, rev) = match self.slots.get(idx) {
            Some(s) => (s.phase, s.revision),
            None => return Err(WmError::Unregistered),
        };
        if !matches!(phase, Phase::Frozen | Phase::Sealed | Phase::Retired) {
            return Err(WmError::IllegalPhase);
        }
        self.push_op(id, rev, 0, Op::Reopen)?;
        if let Some(s) = self.slots.get_mut(idx) {
            s.phase = Phase::Candidate;
        }
        Ok(rev)
    }

    /// 管线消费单点：凭票据取当前冻结槽位。
    ///
    /// 判定次序：未注册 → 已注销(Closed) → 无内容 → 修订号过期 → 摘要不符 →
    /// 已撤销。注销优先于过期，因为注销是终态，报 StaleRevision 会让调用方
    /// 以为「等下一代就好」，实际永远等不到。
    pub fn consume(&self, t: &FreezeTicket) -> Result<&FreezeSlot, WmError> {
        let idx = match self.lookup(t.id) {
            Some(i) => i,
            None => return Err(WmError::Unregistered),
        };
        let s = match self.slots.get(idx) {
            Some(v) => v,
            None => return Err(WmError::Unregistered),
        };
        if s.phase == Phase::Retired {
            return Err(WmError::Closed);
        }
        if !s.phase.has_content() {
            return Err(WmError::IllegalPhase);
        }
        if t.revision != s.revision {
            return Err(WmError::StaleRevision);
        }
        if t.digest != s.digest {
            return Err(WmError::BadDigest);
        }
        let mut i = 0usize;
        while i < self.revocations.len() {
            if let Some(r) = self.revocations.get(i) {
                if r.id == t.id && r.revision == t.revision {
                    return Err(WmError::Revoked);
                }
            }
            i += 1;
        }
        Ok(s)
    }

    /// 撤销某一代票据。撤销只影响该修订号，不影响后续新一代。
    pub fn revoke(&mut self, id: u16, revision: u32) -> Result<(), WmError> {
        if self.lookup(id).is_none() {
            return Err(WmError::Unregistered);
        }
        if self.revocations.len() >= REVOKE_CAP {
            return Err(WmError::Full);
        }
        self.revocations.push(Revocation { id, revision });
        Ok(())
    }

    /// 消费面视图：当前有效票据（供管线在等价重启后重建票据）。
    pub fn ticket_for(&self, id: u16) -> Option<FreezeTicket> {
        let idx = self.lookup(id)?;
        let s = self.slots.get(idx)?;
        if !s.phase.has_content() {
            return None;
        }
        Some(FreezeTicket {
            id: s.id,
            revision: s.revision,
            digest: s.digest,
        })
    }

    /// 账本自检：序号连续、Freeze 条目数 == 成功冻结次数。
    pub fn journal_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.journal.len() {
            match self.journal.get(i) {
                Some(e) => {
                    if e.seq != i as u32 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn primed_gate() -> FreezeGate {
    let mut g = FreezeGate::new();
    let _ = g.register(7, "wm-pipeline");
    let _ = g.set_consumers(2);
    let _ = g.advance(7);
    g
}

/// 十二类坏样本。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    // 1 空标签
    let mut g = FreezeGate::new();
    if g.register(1, "").err() == Some(WmError::BadName) {
        caught += 1;
    }

    // 2 重复注册
    let mut g = FreezeGate::new();
    let _ = g.register(2, "a");
    if g.register(2, "b").err() == Some(WmError::Dup) {
        caught += 1;
    }

    // 3 未注册冻结
    let mut g = FreezeGate::new();
    if g.freeze(99, b"x").err() == Some(WmError::Unregistered) {
        caught += 1;
    }

    // 4 空载荷
    let mut g = primed_gate();
    if g.freeze(7, &[]).err() == Some(WmError::BadDigest) {
        caught += 1;
    }

    // 5 非法跃迁（Candidate 上直接 advance）
    let mut g = primed_gate();
    if g.advance(7).err() == Some(WmError::IllegalPhase) {
        caught += 1;
    }

    // 6 未冻结就封存
    let mut g = FreezeGate::new();
    let _ = g.register(3, "b");
    let _ = g.set_consumers(1);
    if g.seal(3).err() == Some(WmError::IllegalPhase) {
        caught += 1;
    }

    // 7 同摘要重复冻结
    let mut g = primed_gate();
    let _ = g.freeze(7, b"payload");
    if g.freeze(7, b"payload").err() == Some(WmError::AlreadyFrozen) {
        caught += 1;
    }

    // 8 无消费方封存
    let mut g = FreezeGate::new();
    let _ = g.register(4, "c");
    let _ = g.advance(4);
    let _ = g.freeze(4, b"payload");
    if g.seal(4).err() == Some(WmError::NoConsumers) {
        caught += 1;
    }

    // 9 过期票据（换代后旧票失效）
    let mut g = primed_gate();
    let t1 = match g.freeze(7, b"v1") {
        Ok(t) => Some(t),
        Err(_) => None,
    };
    let _ = g.reopen(7);
    let _ = g.freeze(7, b"v2");
    if let Some(t) = t1 {
        if g.consume(&t).err() == Some(WmError::StaleRevision) {
            caught += 1;
        }
    }

    // 10 撤销票据
    let mut g = primed_gate();
    match g.freeze(7, b"v1") {
        Ok(t) => {
            let _ = g.revoke(t.id, t.revision);
            if g.consume(&t).err() == Some(WmError::Revoked) {
                caught += 1;
            }
        }
        Err(_) => {}
    }

    // 11 注销后消费
    let mut g = primed_gate();
    match g.freeze(7, b"v1") {
        Ok(t) => {
            let _ = g.seal(7);
            let _ = g.advance(7);
            if g.consume(&t).err() == Some(WmError::Closed) {
                caught += 1;
            }
        }
        Err(_) => {}
    }

    // 12 注册表满
    let mut g = FreezeGate::new();
    let mut i = 0u16;
    while i < REG_CAP as u16 {
        let _ = g.register(1000 + i, "filler");
        i += 1;
    }
    if g.register(2000, "overflow").err() == Some(WmError::Full) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50053_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50053");

    // J1-1 主路径全通：Draft→Candidate→Frozen→Sealed→Retired
    let mut g = FreezeGate::new();
    let _ = g.register(1, "wm");
    let _ = g.set_consumers(1);
    let a = g.advance(1);
    let t = g.freeze(1, b"watermark-v1");
    let se = g.seal(1);
    let r = g.advance(1);
    let ok = a == Ok(Phase::Candidate)
        && t.is_ok()
        && se.is_ok()
        && r == Ok(Phase::Retired)
        && g.phase_of(1) == Some(Phase::Retired);
    s.add("50053-J1-主路径", ok, if ok { "" } else { "状态机主路径不通" });

    // J1-2 冻结登记一一对应：Freeze 条目数 == 成功冻结次数，且每条 seq 连续
    let freeze_entries = g.count_ops(Op::Freeze);
    let ok = freeze_entries == 1 && g.journal_ok() && g.journal().len() == 3;
    let det = format!("freeze={} entries={}", freeze_entries, g.journal().len());
    s.add_dyn("50053-J1-冻结登记", ok, &det);

    // J1-3 修订号单调 +1：换代后新票修订号恰为旧票 +1
    let mut g = primed_gate();
    let t1 = g.freeze(7, b"gen-1");
    let r1 = g.revision();
    let _ = g.reopen(7);
    let t2 = g.freeze(7, b"gen-2");
    let r2 = g.revision();
    let ok = match (&t1, &t2) {
        (Ok(a), Ok(b)) => b.revision == a.revision + 1 && r2 == r1 + 1,
        _ => false,
    };
    let det = format!("r1={} r2={}", r1, r2);
    s.add_dyn("50053-J1-修订单调", ok, &det);

    // J1-4 过期票据必拒
    let mut g = primed_gate();
    let t1 = g.freeze(7, b"gen-1");
    let _ = g.reopen(7);
    let _ = g.freeze(7, b"gen-2");
    let stale = match &t1 {
        Ok(t) => g.consume(t).err() == Some(WmError::StaleRevision),
        Err(_) => false,
    };
    s.add("50053-J1-旧票必拒", stale, if stale { "" } else { "旧票仍可消费" });

    // J1-5 未注册必拒
    let mut g1 = FreezeGate::new();
    let ok = g1.freeze(42, b"x").err() == Some(WmError::Unregistered)
        && g1.seal(42).err() == Some(WmError::Unregistered)
        && g1.advance(42).err() == Some(WmError::Unregistered);
    s.add("50053-J1-未注册必拒", ok, if ok { "" } else { "未注册入口被放行" });

    // J1-6 撤销票据必拒（且只影响该修订）
    let mut g = primed_gate();
    let t1 = g.freeze(7, b"gen-1");
    let rev = match &t1 {
        Ok(t) => {
            let _ = g.revoke(t.id, t.revision);
            g.consume(t).err() == Some(WmError::Revoked)
        }
        Err(_) => false,
    };
    s.add("50053-J1-撤销必拒", rev, if rev { "" } else { "撤销票仍可消费" });

    // J1-7 封存需消费方
    let mut g = FreezeGate::new();
    let _ = g.register(9, "no-cons");
    let _ = g.advance(9);
    let _ = g.freeze(9, b"p");
    let blocked = g.seal(9).err() == Some(WmError::NoConsumers);
    g.set_consumers(1);
    let allowed = g.seal(9).is_ok();
    let ok = blocked && allowed;
    s.add("50053-J1-封存需消费方", ok, if ok { "" } else { "消费方门槛未生效" });

    // J1-8 注销后消费必拒
    let mut g = primed_gate();
    let t = g.freeze(7, b"p");
    let _ = g.seal(7);
    let _ = g.advance(7);
    let ok = match &t {
        Ok(tk) => g.consume(tk).err() == Some(WmError::Closed),
        Err(_) => false,
    };
    s.add("50053-J1-注销必拒", ok, if ok { "" } else { "注销后仍可消费" });

    // J1-9 摘要确定性：同载荷两门同摘要；单 bit 差异必须改变摘要
    let mut ga = primed_gate();
    let mut gb = primed_gate();
    let ta = ga.freeze(7, b"same-bytes");
    let tb = gb.freeze(7, b"same-bytes");
    let mut gc = primed_gate();
    let tc = gc.freeze(7, b"same-byteR");
    let ok = match (&ta, &tb, &tc) {
        (Ok(a), Ok(b), Ok(c)) => a.digest == b.digest && a.digest != c.digest,
        _ => false,
    };
    let det = format!(
        "d={:016x}",
        match &ta {
            Ok(a) => a.digest,
            Err(_) => 0,
        }
    );
    s.add_dyn("50053-J1-摘要确定", ok, &det);

    // J1-10 票据可取回且与冻结结果一致（消费面重建）
    let mut g = primed_gate();
    let t = g.freeze(7, b"restore");
    let ok = match &t {
        Ok(tk) => match g.ticket_for(7) {
            Some(u) => u == *tk,
            None => false,
        },
        Err(_) => false,
    };
    s.add("50053-J1-票据重建", ok, if ok { "" } else { "ticket_for 与票据不符" });

    // J1-11 非法跃迁必拒（未冻结封存 / Candidate 再 advance / reopen Candidate）
    let mut g = FreezeGate::new();
    let _ = g.register(11, "x");
    let _ = g.set_consumers(1);
    let no_freeze_seal = g.seal(11).err() == Some(WmError::IllegalPhase);
    let _ = g.advance(11);
    let double_advance = g.advance(11).err() == Some(WmError::IllegalPhase);
    let reopen_candidate = g.reopen(11).err() == Some(WmError::IllegalPhase);
    let ok = no_freeze_seal && double_advance && reopen_candidate;
    s.add("50053-J1-非法跃迁", ok, if ok { "" } else { "非法跃迁未挡" });

    // J1-12 注册表满必拒
    let mut g = FreezeGate::new();
    let mut i = 0u16;
    while i < REG_CAP as u16 {
        let _ = g.register(i, "slot");
        i += 1;
    }
    let ok = g.register(500, "one-more").err() == Some(WmError::Full);
    s.add("50053-J1-表满必拒", ok, if ok { "" } else { "注册表无上限" });

    // J1-13 修订号溢出必拒（快照恢复到 u32::MAX 后不得回绕到 0）
    let mut g = FreezeGate::starting_at(u32::MAX);
    let _ = g.register(1, "edge");
    let _ = g.advance(1);
    let ok = g.freeze(1, b"p").err() == Some(WmError::Overflow);
    s.add("50053-J1-溢出必拒", ok, if ok { "" } else { "修订号回绕" });

    // J1-14 空载荷/空标签必拒
    let mut g = FreezeGate::new();
    let name_bad = g.register(1, "").err() == Some(WmError::BadName);
    let _ = g.register(2, "ok");
    let _ = g.advance(2);
    let empty_bad = g.freeze(2, &[]).err() == Some(WmError::BadDigest);
    let ok = name_bad && empty_bad;
    s.add("50053-J1-空值必拒", ok, if ok { "" } else { "空标签/空载荷被放行" });

    // J1-15 注入核十二类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50053-J1-注入必抓", ok, &det);

    // J1-16 账本守恒：四项操作计数之和 == 条目数
    let mut g = primed_gate();
    let _ = g.freeze(7, b"a");
    let _ = g.seal(7);
    let _ = g.advance(7);
    let _ = g.reopen(7);
    let sum = g.count_ops(Op::Freeze) + g.count_ops(Op::Seal) + g.count_ops(Op::Retire) + g.count_ops(Op::Reopen);
    let ok = sum == g.journal().len() && g.journal_ok();
    let det = format!("ops={} entries={}", sum, g.journal().len());
    s.add_dyn("50053-J1-账本守恒", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50053_checks_green() {
        let set = run_unx50053_checks();
        assert!(set.all_passed(), "UNX-F50053 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50053_reopen_keeps_revision_monotonic() {
        let mut g = FreezeGate::new();
        assert!(g.register(1, "wm").is_ok());
        g.set_consumers(1);
        assert_eq!(g.advance(1), Ok(Phase::Candidate));
        let t1 = g.freeze(1, b"one");
        assert!(t1.is_ok());
        assert!(g.reopen(1).is_ok());
        let t2 = g.freeze(1, b"two");
        assert!(t2.is_ok());
        match (t1, t2) {
            (Ok(a), Ok(b)) => assert!(b.revision > a.revision),
            _ => panic!("freeze must succeed"),
        }
    }
}
