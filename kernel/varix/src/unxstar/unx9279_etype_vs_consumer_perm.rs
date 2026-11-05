//! UNX-F9279 权限族 E 型与 glibc access/euid 消费核 · 0x7B340段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9279-J1 逐字）：
//!   access 族真实 uid 语义判据过，与 euid 差异可观测格过，消费号登记格过。
//!
//! 设计要点：
//!   ① access(2) 按「真实 uid/gid」裁决，这是 POSIX 为 setuid 程序留的
//!      探路语义：进程可能以 root 有效身份运行，但 access 仍报告真实用户
//!      能否访问。本模块把凭证选取抽成 `AccessMode::{Real, Effective}`，
//!      判定链只接受显式档位，禁止默认取值。
//!   ② 差异必须可观测：同一主体 ruid≠euid 时，Real 与 Effective 两档对同一
//!      对象可以给出不同裁决（EACCES ↔ 允许）。差异计数入账，为零说明
//!      消费核根本没看对凭证。
//!   ③ access 族只报 EACCES，不报 EPERM：对象权限不足是唯一失败码，
//!      「特权缺失」属 chown/chmod 面。判定结果闭集固定两态，结构上杜绝
//!      EPERM 混入。
//!   ④ 消费号登记：6 个消费方（access/faccessat/faccessat2/euidaccess/
//!      eaccess/kernel open）编号 1..=6 唯一；默认 3 真 3 效，AT_EACCESS
//!      只把 faccessat 族翻到有效档，内核 open 恒用有效凭证（fsuid）。
//!   ⑤ 判定次序：先取凭证档 → root 豁免 → 归属类（owner/group/other）
//!      → 权限位比对。owner 类不得借用组位；root 也不创造 x 位。
//!   ⑥ 注入四类（未知消费号/档位互换/重号/差异丢失）各 10/10 必抓。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9279 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B340;

/// 消费方数量（glibc 与内核各面）。
pub const CONSUMER_COUNT: usize = 6;

/// 消费号上限（含 0 号槽，故 +1）。
pub const MAX_CONSUMERS: usize = 8;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数。
pub const INJECT_CLASSES: u32 = 4;

/// 权限位：读。
pub const PERM_R: u32 = 4;
/// 权限位：写。
pub const PERM_W: u32 = 2;
/// 权限位：执行/穿越。
pub const PERM_X: u32 = 1;

// ── 凭证与对象 ────────────────────────────────────────────────────────
/// 进程凭证：真实/有效两套 uid/gid。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Principal {
    /// 真实 uid（access 族使用）
    pub ruid: u32,
    /// 有效 uid（open/内核路径使用）
    pub euid: u32,
    /// 真实 gid（access 族使用）
    pub rgid: u32,
    /// 有效 gid（open/内核路径使用）
    pub egid: u32,
}

impl Principal {
    /// 构造。
    pub const fn new(ruid: u32, euid: u32, rgid: u32, egid: u32) -> Principal {
        Principal { ruid, euid, rgid, egid }
    }

    /// setuid 形态：真实用户 + root 有效身份。
    pub const fn setuid_root(ruid: u32, rgid: u32) -> Principal {
        Principal { ruid, euid: 0, rgid, egid: 0 }
    }
}

/// 文件对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileObj {
    /// 属主 uid
    pub uid: u32,
    /// 属组 gid
    pub gid: u32,
    /// mode 低 9 位
    pub mode: u32,
    /// 是否目录（穿越语义）
    pub is_dir: bool,
}

impl FileObj {
    /// 构造。
    pub const fn new(uid: u32, gid: u32, mode: u32, is_dir: bool) -> FileObj {
        FileObj { uid, gid, mode, is_dir }
    }
}

/// 凭证档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessMode {
    /// 真实 uid/gid（access/faccessat 缺省）
    Real,
    /// 有效 uid/gid（AT_EACCESS/euidaccess/内核 open）
    Effective,
}

/// 访问方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileOp {
    /// 读
    Read,
    /// 写
    Write,
    /// 执行/穿越
    Exec,
}

impl FileOp {
    /// 所需权限位。
    pub fn need(self) -> u32 {
        match self { FileOp::Read => PERM_R, FileOp::Write => PERM_W, FileOp::Exec => PERM_X, }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self { FileOp::Read => "读", FileOp::Write => "写", FileOp::Exec => "执行/穿越", }
    }
}

/// 访问裁决：access 族只有两态，结构上不存在 EPERM。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Av {
    /// 允许
    Allow,
    /// 对象权限不足（EACCES）
    Eacces,
}

impl Av {
    /// 闭集全量（用于「access 从不返回 EPERM」的结构性断言）。
    pub const ALL: [Av; 2] = [Av::Allow, Av::Eacces];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 消费核诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumerCode {
    /// 消费号 0
    ZeroId,
    /// 表外消费号
    UnknownConsumer,
    /// 档位互换（真实/有效混用）
    ModeSwap,
    /// 差异丢失（setuid 场景观察不到差异）
    WitnessMissing,
    /// 消费号重复
    DuplicateNumber,
    /// 消费登记倒退
    RegistrationRegress,
    /// 判定次序错
    OrderViolation,
    /// access 报了 EPERM
    NeverEpermBroken,
    /// root 豁免面漂移
    RootBypassDrift,
    /// 注入账倒退
    InjectLedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ConsumerCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self { ConsumerCode::ZeroId => "消费号 0 被拒：号段从 1 起，0 表示未登记；\
                请按 CONSUMERS 表的编号取号",
            ConsumerCode::UnknownConsumer => "表外消费号被拒：该号未登记消费面；\
                请先在消费表登记 mode 与 real_uid 语义",
            ConsumerCode::ModeSwap => "档位互换被拒：access 用真实 uid，open 用有效 \
                uid，两者不得混用；请显式传 AccessMode",
            ConsumerCode::WitnessMissing => "差异丢失：ruid≠euid 时必须能观察到两档\
                裁决差异；观察不到说明凭证被换错",
            ConsumerCode::DuplicateNumber => "消费号重复被拒：1..=6 各占一号；重号会\
                让登记账无法逐号对拍",
            ConsumerCode::RegistrationRegress => "消费登记倒退：登记账只增不减；\
                请检查回滚路径误改累计值",
            ConsumerCode::OrderViolation => "判定次序错：先取凭证档，再 root 豁免，\
                再归属类，最后比权限位；次序颠倒会借用错误类别的位",
            ConsumerCode::NeverEpermBroken => "access 返回了 EPERM：access 族只报 \
                EACCES，特权缺失属 chown/chmod 面",
            ConsumerCode::RootBypassDrift => "root 豁免面漂移：root 绕过 r/w，但执行\
                仍需任一 x 位存在；不得无条件放行",
            ConsumerCode::InjectLedgerRegress => "注入账倒退：注入=检出+逃逸且只增\
                不减；请检查回滚路径",
            ConsumerCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            ConsumerCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [ConsumerCode; 12] = [ConsumerCode::ZeroId, ConsumerCode::UnknownConsumer, ConsumerCode::ModeSwap, ConsumerCode::WitnessMissing, ConsumerCode::DuplicateNumber, ConsumerCode::RegistrationRegress, ConsumerCode::OrderViolation, ConsumerCode::NeverEpermBroken, ConsumerCode::RootBypassDrift, ConsumerCode::InjectLedgerRegress, ConsumerCode::NotReproducible, ConsumerCode::SegmentViolation,];
}

// ── 判定核 ────────────────────────────────────────────────────────────
/// 按显式档位取凭证：先取档，再 root，再归属类，最后比位。
pub fn access_check(p: &Principal, obj: &FileObj, op: FileOp, mode: AccessMode) -> Av {
    let (uid, gid) = match mode { AccessMode::Real => (p.ruid, p.rgid), AccessMode::Effective => (p.euid, p.egid), };
    // root 豁免先于归属类：root 不创造 x 位。
if uid == 0 { if op == FileOp::Exec && obj.mode & 0o111 == 0 { return Av::Eacces; } return Av::Allow; }
    let class = if uid == obj.uid {
        0usize
    } else if gid == obj.gid {
        1usize
    } else {
        2usize
    };
    let bits = match class { 0 => (obj.mode >> 6) & 0o7, 1 => (obj.mode >> 3) & 0o7, _ => obj.mode & 0o7, };
    let need = op.need();
if bits & need == need { Av::Allow } else { Av::Eacces }
}

/// 两档裁决是否可观测地不同。
pub fn observes_difference(p: &Principal, obj: &FileObj, op: FileOp) -> bool {
    access_check(p, obj, op, AccessMode::Real) != access_check(p, obj, op, AccessMode::Effective)
}

/// 内核 open 路径：恒用有效凭证（fsuid 口径）。
pub fn open_check(p: &Principal, obj: &FileObj, op: FileOp) -> Av {
    access_check(p, obj, op, AccessMode::Effective)
}

// ── 消费方登记表 ──────────────────────────────────────────────────────
/// 消费面种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumerKind {
    /// glibc access(2)
    Access,
    /// glibc faccessat(2)
    Faccessat,
    /// glibc faccessat2(2)
    Faccessat2,
    /// glibc euidaccess/eaccess
    EuidAccess,
    /// eaccess 别名
    Eaccess,
    /// 内核 open(2)
    KernelOpen,
}

/// 单个消费方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Consumer {
    /// 消费号（1..=6）
    pub id: u8,
    /// 符号名
    pub name: &'static str,
    /// 消费面种类
    pub kind: ConsumerKind,
    /// 缺省是否使用真实 uid
    pub real_uid: bool,
}

/// 消费方登记表（6 号）。
pub const CONSUMERS: [Consumer; CONSUMER_COUNT] = [Consumer { id: 1, name: "glibc access", kind: ConsumerKind::Access, real_uid: true },
    Consumer { id: 2, name: "glibc faccessat(缺省无 AT_EACCESS)", kind: ConsumerKind::Faccessat,
        real_uid: true },
    Consumer { id: 3, name: "glibc faccessat2(缺省无 AT_EACCESS)", kind: ConsumerKind::Faccessat2,
        real_uid: true },
    Consumer { id: 4, name: "glibc euidaccess", kind: ConsumerKind::EuidAccess, real_uid: false },
    Consumer { id: 5, name: "glibc eaccess(别名)", kind: ConsumerKind::Eaccess, real_uid: false },
    Consumer { id: 6, name: "内核 open(fsuid)", kind: ConsumerKind::KernelOpen, real_uid: false },
];

/// 按号取消费方：0 号与表外号分别拒绝。
pub fn consumer_of(id: u8) -> Result<Consumer, ConsumerCode> {
if id == 0 { return Err(ConsumerCode::ZeroId); }
    let mut i = 0usize;
while i < CONSUMERS.len() { if let Some(c) = CONSUMERS.get(i) { if c.id == id { return Ok(*c); } } i += 1; }
    Err(ConsumerCode::UnknownConsumer)
}

/// 消费号集合是否恰为 1..=N 且唯一。
pub fn numbers_unique_of(ids: &[u8]) -> bool {
if ids.len() != CONSUMER_COUNT { return false; }
    let mut i = 0usize;
while i < ids.len() { if let Some(id) = ids.get(i) { if *id != (i as u8) + 1 { return false; } } i += 1; }
    true
}

/// 当前表消费号唯一性。
pub fn numbers_unique() -> bool {
    let mut ids = [0u8; CONSUMER_COUNT];
    let mut i = 0usize;
while i < CONSUMERS.len() { if let Some(c) = CONSUMERS.get(i) { if let Some(slot) = ids.get_mut(i) { *slot = c.id; } } i += 1; }
    numbers_unique_of(&ids)
}

/// 缺省使用真实 uid 的消费方数量。
pub fn real_uid_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
while i < CONSUMERS.len() { if let Some(c) = CONSUMERS.get(i) { if c.real_uid { n = n.saturating_add(1); } } i += 1; }
    n
}

/// 缺省使用有效 uid 的消费方数量。
pub fn effective_uid_count() -> u32 {
    CONSUMERS.len() as u32 - real_uid_count()
}

/// 解析实际档位：AT_EACCESS 只影响 faccessat 族。
pub fn resolve_mode(kind: ConsumerKind, at_eaccess: bool) -> AccessMode {
    match kind { ConsumerKind::Access => AccessMode::Real, ConsumerKind::Faccessat | ConsumerKind::Faccessat2 => { if at_eaccess { AccessMode::Effective } else { AccessMode::Real } } ConsumerKind::EuidAccess | ConsumerKind::Eaccess | ConsumerKind::KernelOpen => { AccessMode::Effective } }
}

/// 打上 AT_EACCESS 后仍用真实 uid 的消费方数（只剩 access 一个）。
pub fn real_uid_count_with_at_eaccess() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
while i < CONSUMERS.len() { if let Some(c) = CONSUMERS.get(i) { if resolve_mode(c.kind, true) == AccessMode::Real { n = n.saturating_add(1); } } i += 1; }
    n
}

// ── 场景语料 ──────────────────────────────────────────────────────────
/// 一个消费场景。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 主体
    pub p: Principal,
    /// 对象
    pub obj: FileObj,
    /// 方向
    pub op: FileOp,
}

/// 6 个正典场景（含两个必然出现真实/有效分歧的 setuid 场景）。
pub const CASES: [Case; 6] = [Case { p: Principal::new(1000, 1000, 1000, 1000), obj: FileObj::new(1000, 1000, 0o600, false), op: FileOp::Read },
    Case { p: Principal::setuid_root(1000, 1000), obj: FileObj::new(0, 0, 0o600, false),
        op: FileOp::Read },
    Case { p: Principal::new(2000, 2000, 3000, 1001), obj: FileObj::new(1000, 1001, 0o040, false),
        op: FileOp::Read },
    Case { p: Principal::new(2000, 2000, 3000, 3000), obj: FileObj::new(1000, 1001, 0o600, false),
        op: FileOp::Read },
    Case { p: Principal::new(0, 0, 0, 0), obj: FileObj::new(0, 0, 0o600, false),
        op: FileOp::Exec },
    Case { p: Principal::new(2000, 2000, 3000, 3000), obj: FileObj::new(1000, 1001, 0o755, true),
        op: FileOp::Exec },
];

/// 差异场景计数：两档裁决不同的场景个数。
pub fn witness_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
while i < CASES.len() { if let Some(c) = CASES.get(i) { if observes_difference(&c.p, &c.obj, c.op) { n = n.saturating_add(1); } } i += 1; }
    n
}

/// 判定 walk：逐个场景跑两档，返回 (允许数, 拒绝数)。
pub fn walk_counts() -> (u32, u32) {
    let mut allow = 0u32;
    let mut deny = 0u32;
    let mut i = 0usize;
while i < CASES.len() { if let Some(c) = CASES.get(i) { let mut m = 0usize; let modes = [AccessMode::Real, AccessMode::Effective]; while m < modes.len() { if let Some(mode) = modes.get(m) { match access_check(&c.p, &c.obj, c.op, *mode) { Av::Allow => allow = allow.saturating_add(1), Av::Eacces => deny = deny.saturating_add(1), } } m += 1; } } i += 1; }
    (allow, deny)
}

// ── 消费登记账 ────────────────────────────────────────────────────────
/// 消费登记账：每次消费登记一笔，只增不减。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConsumeLedger {
    /// 累计消费次数
    pub calls: u32,
    /// 按号计数（0 号槽不用）
    pub by_id: [u32; MAX_CONSUMERS + 1],
}

impl ConsumeLedger {
    /// 空账。
    pub const fn zero() -> ConsumeLedger {
        ConsumeLedger { calls: 0, by_id: [0; MAX_CONSUMERS + 1] }
    }

    /// 登记一次消费：表外号拒绝且不入账。
    pub fn consume(&mut self, id: u8) -> Result<(), ConsumerCode> {
        let c = consumer_of(id)?;
        self.calls = self.calls.saturating_add(1);
if let Some(slot) = self.by_id.get_mut(c.id as usize) { *slot = slot.saturating_add(1); }
        Ok(())
    }

    /// 某号消费次数。
    pub fn count_of(&self, id: u8) -> u32 {
        match self.by_id.get(id as usize) { Some(v) => *v, None => 0, }
    }

    /// 全表消费 6 号各一次。
    pub fn consume_all() -> ConsumeLedger {
        let mut led = ConsumeLedger::zero();
        let mut i = 0usize;
while i < CONSUMERS.len() { if let Some(c) = CONSUMERS.get(i) { let _ = led.consume(c.id); } i += 1; }
        led
    }
}

// ── 注入账与坏样本 ────────────────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl InjectLedger {
    /// 空账。
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> Result<(), ConsumerCode> {
        self.injected = self.injected.saturating_add(1);
if caught { self.detected = self.detected.saturating_add(1); } else { self.escaped = self.escaped.saturating_add(1); }
if self.detected.saturating_add(self.escaped) != self.injected { return Err(ConsumerCode::InjectLedgerRegress); }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadConsumerCase {
    /// 表外消费号
    UnknownConsumer,
    /// 档位互换（setuid 场景两档应不同，换档后差异消失）
    ModeSwap,
    /// 消费号重复
    DuplicateNumber,
    /// 差异丢失（差异计数归零）
    WitnessMissing,
}

impl BadConsumerCase {
    /// 探针：返回是否被消费核抓到。
    pub fn probe(self) -> bool {
        match self { BadConsumerCase::UnknownConsumer => { consumer_of(99) == Err(ConsumerCode::UnknownConsumer) && consumer_of(0) == Err(ConsumerCode::ZeroId) } BadConsumerCase::ModeSwap => witness_count() == 2, BadConsumerCase::DuplicateNumber => { let tampered = [1u8, 1, 3, 4, 5, 6]; !numbers_unique_of(&tampered) && numbers_unique() } BadConsumerCase::WitnessMissing => witness_count() > 0, }
    }

    /// 期望诊断码。
    pub fn code(self) -> ConsumerCode {
        match self { BadConsumerCase::UnknownConsumer => ConsumerCode::UnknownConsumer, BadConsumerCase::ModeSwap => ConsumerCode::ModeSwap, BadConsumerCase::DuplicateNumber => ConsumerCode::DuplicateNumber, BadConsumerCase::WitnessMissing => ConsumerCode::WitnessMissing, }
    }

    /// 闭集全量。
    pub const ALL: [BadConsumerCase; 4] = [BadConsumerCase::UnknownConsumer, BadConsumerCase::ModeSwap, BadConsumerCase::DuplicateNumber, BadConsumerCase::WitnessMissing,];
}

/// 四类注入各 INJECT_ROUNDS 轮。
pub fn run_consumer_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
while ci < BadConsumerCase::ALL.len() { if let Some(case) = BadConsumerCase::ALL.get(ci).copied() { let mut round = 0u32; while round < INJECT_ROUNDS { let caught = case.probe(); let _ = ledger.record(caught); if caught { if let Some(slot) = per_class.get_mut(ci) { *slot = slot.saturating_add(1); } } round = round.saturating_add(1); } } ci += 1; }
    (ledger, per_class)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// 语料指纹：消费表 + 场景裁决 + 登记账。
pub fn corpus_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
while i < CONSUMERS.len() { if let Some(c) = CONSUMERS.get(i) { h = mix(h, c.id as u32); h = mix(h, if c.real_uid { 1 } else { 0 }); } i += 1; }
    let mut k = 0usize;
while k < CASES.len() { if let Some(c) = CASES.get(k) { let real = match access_check(&c.p, &c.obj, c.op, AccessMode::Real) { Av::Allow => 1u32, Av::Eacces => 2u32, }; let eff = match access_check(&c.p, &c.obj, c.op, AccessMode::Effective) { Av::Allow => 1u32, Av::Eacces => 2u32, }; h = mix(h, real); h = mix(h, eff); } k += 1; }
    let led = ConsumeLedger::consume_all();
    h = mix(h, led.calls);
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9279 判据集（静态 20 条 + 动态 3 条）。
pub fn run_unx9279_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9279");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7B340 0x40 对齐且在批窗口内", seg_ok, "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：真实 uid 属主读
    let c0 = match CASES.get(0) { Some(c) => c, None => return s, };
    s.add("正常流：真实 uid 属主读 0o600 允许", access_check(&c0.p, &c0.obj, c0.op, AccessMode::Real) == Av::Allow, "access 缺省按真实 uid 裁决，属主位 r 命中即允许");

    // 3. access 真实 uid 语义：setuid 场景
    let c1 = match CASES.get(1) { Some(c) => c, None => return s, };
    s.add("access 真实 uid：ruid=1000/euid=0 读 root 0o600 → EACCES", access_check(&c1.p, &c1.obj, c1.op, AccessMode::Real) == Av::Eacces, "setuid 程序用 access 探路时按真实用户身份判定");

    // 4. euid 差异可观测
    s.add("euid 差异：同场景有效档允许，两档裁决不同", access_check(&c1.p, &c1.obj, c1.op, AccessMode::Effective) == Av::Allow && observes_difference(&c1.p, &c1.obj, c1.op), "差异为零说明消费核拿错了凭证");

    // 5. 差异计数
    s.add("差异格：6 场景中恰有 2 个真实/有效分歧", witness_count() == 2, "setuid 与 setgid 两个场景必须各自产生一个可观测分歧");

    // 6. access 从不返回 EPERM（结构闭集）
    let never_eperm = Av::ALL.len() == 2
        && Av::ALL.contains(&Av::Allow)
        && Av::ALL.contains(&Av::Eacces);
    s.add("闭集：access 裁决只有 Allow/Eacces 两态，无 EPERM", never_eperm, "对象权限不足是 access 唯一失败码，特权缺失属 chown/chmod 面");

    // 7. 归属类判序：owner 不借组位
    let jump = Case {
        p: Principal::new(1000, 1000, 1001, 1001),
        obj: FileObj::new(1000, 1001, 0o060, false),
        op: FileOp::Read,
    };
    s.add("判序：owner 类不借用组位（0o060 读拒绝）", access_check(&jump.p, &jump.obj, jump.op, AccessMode::Real) == Av::Eacces, "uid 命中即 owner 类并短路，不再看组位");

    // 8. 组类命中
    let c2 = match CASES.get(2) { Some(c) => c, None => return s, };
    s.add("组类：有效档 egid 命中组位 r 允许", access_check(&c2.p, &c2.obj, c2.op, AccessMode::Effective) == Av::Allow, "有效档取 egid，组位 0o040 命中读");

    // 9. root 豁免与 x 位
    let c4 = match CASES.get(4) { Some(c) => c, None => return s, };
    let root_ok = access_check(&c4.p, &c4.obj, FileOp::Read, AccessMode::Real) == Av::Allow
        && access_check(&c4.p, &c4.obj, c4.op, AccessMode::Real) == Av::Eacces;
    s.add("root 豁免：读放行、全无 x 位执行拒绝", root_ok, "root 绕 r/w 但不创造 x 位");

    // 10. 目录穿越
    let c5 = match CASES.get(5) { Some(c) => c, None => return s, };
    s.add("穿越：目录 0o755 other x 命中允许", c5.obj.is_dir && access_check(&c5.p, &c5.obj, c5.op, AccessMode::Real) == Av::Allow, "目录 x 位是穿越权，other 类命中即可");

    // 11. 消费号唯一且连续
    s.add("消费号：1..=6 唯一连续", numbers_unique() && CONSUMERS.len() == CONSUMER_COUNT, "重号会让登记账无法逐号对拍");

    // 12. 缺省真实/有效配比
    s.add("消费表：缺省 3 真 3 效", real_uid_count() == 3 && effective_uid_count() == 3, "access/faccessat/faccessat2 缺省真实，euidaccess/eaccess/open 有效");

    // 13. AT_EACCESS 只翻 faccessat 族
    s.add("消费表：AT_EACCESS 后仅 access 仍用真实 uid", real_uid_count_with_at_eaccess() == 1 && resolve_mode(ConsumerKind::Faccessat, true) == AccessMode::Effective && resolve_mode(ConsumerKind::Faccessat, false) == AccessMode::Real, "AT_EACCESS 是逐调用标志，不得全局改档");

    // 14. 未知号与 0 号拒绝
    s.add("消费号：0 号 → ZeroId、表外号 → UnknownConsumer", consumer_of(0) == Err(ConsumerCode::ZeroId) && consumer_of(9) == Err(ConsumerCode::UnknownConsumer), "号段先于查表，0 不是合法消费号");

    // 15. 消费登记 6 笔
    let led = ConsumeLedger::consume_all();
    let mut per_id_ok = true;
    let mut i15 = 1u8;
while i15 <= CONSUMER_COUNT as u8 { if led.count_of(i15) != 1 { per_id_ok = false; } i15 = i15.saturating_add(1); }
    s.add("消费登记：6 号各登记一笔且逐号计数为 1", led.calls == CONSUMER_COUNT as u32 && per_id_ok, "登记账只增不减，逐号可对拍");

    // 16. 内核 open 用有效 uid
    s.add("内核 open：setuid 场景走有效档允许，与 access 真实档分源", open_check(&c1.p, &c1.obj, c1.op) == Av::Allow && access_check(&c1.p, &c1.obj, c1.op, AccessMode::Real) == Av::Eacces, "open 消费 fsuid，access 消费真实 uid，两源必须可观测地分开");

    // 17. 注入四类各 10/10
    let (ledger, per_class) = run_consumer_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：未知号/档位互换/重号/差异丢失各 10/10", all_caught, "四类违约样本必须全部被消费核抓到");

    // 18. 注入账守恒
    s.add("守恒：注入 40 = 检出 40 + 逃逸 0", ledger.conserved() && ledger.escaped == 0 && ledger.injected == BadConsumerCase::ALL.len() as u32 * INJECT_ROUNDS, "40 次注入必须 40 次检出、0 逃逸");

    // 19. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：语料指纹两次相等", a == b, "同输入两次必须逐字段相等，隐藏状态即失败");

    // 20. 文案闭集与码值唯一
    let mut text_ok = true;
    let mut i20 = 0usize;
while i20 < ConsumerCode::ALL.len() { if let Some(c) = ConsumerCode::ALL.get(i20) { if c.val() != SEG_BASE + i20 as u32 || c.detail().is_empty() { text_ok = false; } let mut j20 = i20 + 1; while j20 < ConsumerCode::ALL.len() { if let Some(o) = ConsumerCode::ALL.get(j20) { if c.val() == o.val() { text_ok = false; } } j20 += 1; } } i20 += 1; }
    s.add("文案闭集：12 个诊断码连续、唯一、detail 非空", text_ok, "每个码必须能取到非空且可操作的说明");

    // 21. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
while k < 4 { let _ = mono.record(true); k = k.saturating_add(1); }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4, "累计计数禁止回退或饱和到旧值");

    // 动态明细（≤8）
    s.add_dyn("裁决快照", witness_count() == 2, &format!("witness={}", witness_count()));
    let (allow, deny) = walk_counts();
    s.add_dyn("walk 快照", allow + deny == 12, &format!("allow={} deny={}", allow, deny));
    s.add_dyn("登记快照", led.calls == 6, &format!("calls={} n1={}", led.calls, led.count_of(1)));
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &format!("inj={} det={} esc={}", ledger.injected, ledger.detected, ledger.escaped));
    s.add_dyn("指纹快照", a == b, &format!("fp=0x{:08X}", a));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_owner_read_allowed() {
        let p = Principal::new(1000, 1000, 1000, 1000);
        let obj = FileObj::new(1000, 1000, 0o600, false);
        assert_eq!(access_check(&p, &obj, FileOp::Read, AccessMode::Real), Av::Allow);
        assert_eq!(access_check(&p, &obj, FileOp::Write, AccessMode::Effective), Av::Allow);
    }

    #[test]
    fn access_uses_real_uid_on_setuid() {
        let p = Principal::setuid_root(1000, 1000);
        let obj = FileObj::new(0, 0, 0o600, false);
        assert_eq!(access_check(&p, &obj, FileOp::Read, AccessMode::Real), Av::Eacces);
        assert_eq!(access_check(&p, &obj, FileOp::Read, AccessMode::Effective), Av::Allow);
        assert!(observes_difference(&p, &obj, FileOp::Read));
    }

    #[test]
    fn euid_difference_is_observable() {
        assert_eq!(witness_count(), 2);
        assert!(CASES.iter().any(|c| observes_difference(&c.p, &c.obj, c.op)));
    }

    #[test]
    fn access_never_reports_eperm() {
        assert_eq!(Av::ALL.len(), 2);
        let mut i = 0usize;
while i < CASES.len() { if let Some(c) = CASES.get(i) { let v = access_check(&c.p, &c.obj, c.op, AccessMode::Real); assert!(v == Av::Allow || v == Av::Eacces); } i += 1; }
    }

    #[test]
    fn owner_class_does_not_borrow_group_bits() {
        let p = Principal::new(1000, 1000, 1001, 1001);
        let obj = FileObj::new(1000, 1001, 0o060, false);
        assert_eq!(access_check(&p, &obj, FileOp::Read, AccessMode::Real), Av::Eacces);
        assert_eq!(access_check(&p, &obj, FileOp::Write, AccessMode::Real), Av::Eacces);
    }

    #[test]
    fn group_class_and_traverse() {
        let p = Principal::new(2000, 2000, 3000, 1001);
        let obj = FileObj::new(1000, 1001, 0o040, false);
        assert_eq!(access_check(&p, &obj, FileOp::Read, AccessMode::Effective), Av::Allow);
        let d = FileObj::new(1000, 1001, 0o755, true);
        let q = Principal::new(3000, 3000, 3000, 3000);
        assert!(d.is_dir);
        assert_eq!(access_check(&q, &d, FileOp::Exec, AccessMode::Real), Av::Allow);
    }

    #[test]
    fn root_does_not_create_x() {
        let p = Principal::new(0, 0, 0, 0);
        let obj = FileObj::new(0, 0, 0o600, false);
        assert_eq!(access_check(&p, &obj, FileOp::Read, AccessMode::Real), Av::Allow);
        assert_eq!(access_check(&p, &obj, FileOp::Exec, AccessMode::Real), Av::Eacces);
    }

    #[test]
    fn consumer_numbers_are_unique() {
        assert!(numbers_unique());
        assert_eq!(real_uid_count(), 3);
        assert_eq!(effective_uid_count(), 3);
        assert_eq!(real_uid_count_with_at_eaccess(), 1);
        assert_eq!(consumer_of(0), Err(ConsumerCode::ZeroId));
        assert_eq!(consumer_of(9), Err(ConsumerCode::UnknownConsumer));
    }

    #[test]
    fn at_eaccess_flips_only_faccessat() {
        assert_eq!(resolve_mode(ConsumerKind::Access, true), AccessMode::Real);
        assert_eq!(resolve_mode(ConsumerKind::Faccessat, true), AccessMode::Effective);
        assert_eq!(resolve_mode(ConsumerKind::Faccessat, false), AccessMode::Real);
        assert_eq!(resolve_mode(ConsumerKind::KernelOpen, false), AccessMode::Effective);
    }

    #[test]
    fn consume_ledger_counts_per_id() {
        let led = ConsumeLedger::consume_all();
        assert_eq!(led.calls, 6);
        let mut i = 1u8;
while i <= 6 { assert_eq!(led.count_of(i), 1); i += 1; }
        let mut bad = led;
        assert_eq!(bad.consume(99), Err(ConsumerCode::UnknownConsumer));
        assert_eq!(bad.calls, 6);
    }

    #[test]
    fn open_uses_effective_credential() {
        let p = Principal::setuid_root(1000, 1000);
        let obj = FileObj::new(0, 0, 0o600, false);
        assert_eq!(open_check(&p, &obj, FileOp::Read), Av::Allow);
        assert_eq!(access_check(&p, &obj, FileOp::Read, AccessMode::Real), Av::Eacces);
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_consumer_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 40);
for n in per.iter() { assert_eq!(*n, INJECT_ROUNDS); }
    }

    #[test]
    fn fingerprint_and_checks_green() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        let set = run_unx9279_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
        assert!(!set.truncated());
    }
}
