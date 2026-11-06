//! UNX-F9298 进程族 E 型升级接管注记 · 0x7B800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9298 逐字）：
//!   UNX-F9298-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单
//!   落账格过。
//!
//! 设计要点：
//!   ① 边界用 32 位 id 空间上的**三张掩码**表达，而不是一张名单：ACTIVE（在役
//!      30 个进程族系统调用）、RESERVED（30/31 两个保留位）、TAKEOVER（E 型
//!      升级接管的 16 个）。存量 LEGACY = ACTIVE & !TAKEOVER 恒等于 14 个——
//!      这个等式是判据，不是注释：任何把保留位或非进程调用塞进接管集的写法，
//!      都会让三条掩码关系同时炸掉。
//!   ② 声明是逐位的，不是整体的：declare(id) 一次只落一位，顺序被记账（ids
//!      数组留下声明次序），重复位 EEXIST、保留位 EINVAL、存量位 EPERM。
//!      三者次序冻结：在役判定 → 重复判定 → 白名单判定，错序会把「重复声明
//!      一个越界 id」误判成 EPERM 而不是 EEXIST。
//!   ③ 零重写承诺格：封边界时对 LEGACY 全量拍一次 FNV 快照（逐位喂 id 与三条
//!      代表性 errno 的阶梯值）。此后每一次 try_rewrite 都先问「这一位是不是
//!      存量」：存量回 EPERM 且快照必须逐位不变；接管位才允许改。承诺因此是
//!      **可复算的等式**（legacy_digest() == digest_at_freeze），而非口头声明。
//!   ④ 增量清单是追加式账本：seq 从 1 起严格递增无空洞；(id, kind) 组合唯一；
//!      只允许登记 TAKEOVER 位（登记存量位 EPERM，等于偷偷改写存量路径）；
//!      边界未闭合前落账 EINVAL；封存后任何落账 EROFS。状态机
//!      Blank→BoundaryClosed→ManifestPosted→Verified→Sealed，封存后一切变更
//!      EROFS；增量按 Added/Reused/Retired 三类计数，三类之和必须等于条数。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（F9298 独占 0x7B800..=0x7B83F）。
pub const SEG_BASE: u32 = 0x7B800;
/// 段窗口宽度（下一张卡 F9299 从 0x7B840 起）。
pub const SEG_SPAN: u32 = 0x40;
/// 进程族 id 空间。
pub const ID_SPACE: usize = 32;
/// 增量清单容量。
pub const MAX_MANIFEST: usize = 16;
/// 接管集规模（白名单位数）。
pub const TAKEOVER_COUNT: u32 = 16;
/// 存量集规模（ACTIVE 30 位减去接管 16 位）。
pub const LEGACY_COUNT: u32 = 14;

// ── Linux errno 数值 ─────────────────────────────────────────────────
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EBUSY: i32 = 16;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EROFS: i32 = 30;

/// id → 位。越界（>=32）回 0，绝不回绕成别的位。
pub const fn bit_of(id: u8) -> u32 {
    if id < 32 { 1u32 << (id as u32) } else { 0 }
}

/// 在役位图：id 0..=29 为进程族在役接口。
pub const ACTIVE_MASK: u32 = 0x3FFF_FFFF;
/// 保留位图：id 30/31 未分配，任何声明都必须 EINVAL。
pub const RESERVED_MASK: u32 = 0xC000_0000;

/// E 型升级接管的 16 位：进程创建（fork/vfork/clone/clone3）、装载
/// （execve/execveat）、回收（wait4/waitid）、退出（exit_group）、
/// 信号发送（kill/tkill/tgkill）、资源与属性（prctl/prlimit64）、
/// 命名空间（unshare/setns）。语义上成组，不是随手挑的 16 个。
pub const TAKEOVER_MASK: u32 = bit_of(0) | bit_of(1) | bit_of(2) | bit_of(3)
    | bit_of(4) | bit_of(5) | bit_of(6) | bit_of(7) | bit_of(8)
    | bit_of(10) | bit_of(11) | bit_of(12)
    | bit_of(18) | bit_of(19)
    | bit_of(22) | bit_of(23);

/// 存量位图：在役但**不在**接管集内 —— 这 14 位的错误路径承诺零重写。
pub const LEGACY_MASK: u32 = ACTIVE_MASK & !TAKEOVER_MASK;

// ── 进程族系统调用表 ─────────────────────────────────────────────────
/// 一行系统调用：id 与名字（名字只用于诊断文案，不参与判定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcSys { pub id: u8, pub name: &'static str }

/// 32 槽冻结表（30 在役 + 2 保留）。
pub const SYS_TABLE: [ProcSys; ID_SPACE] = [
    ProcSys { id: 0, name: "fork" }, ProcSys { id: 1, name: "vfork" },
    ProcSys { id: 2, name: "clone" }, ProcSys { id: 3, name: "clone3" },
    ProcSys { id: 4, name: "execve" }, ProcSys { id: 5, name: "execveat" },
    ProcSys { id: 6, name: "wait4" }, ProcSys { id: 7, name: "waitid" },
    ProcSys { id: 8, name: "exit" }, ProcSys { id: 9, name: "exit_group" },
    ProcSys { id: 10, name: "kill" }, ProcSys { id: 11, name: "tkill" },
    ProcSys { id: 12, name: "tgkill" }, ProcSys { id: 13, name: "getpid" },
    ProcSys { id: 14, name: "getppid" }, ProcSys { id: 15, name: "getpgid" },
    ProcSys { id: 16, name: "setpgid" }, ProcSys { id: 17, name: "setsid" },
    ProcSys { id: 18, name: "prctl" }, ProcSys { id: 19, name: "prlimit64" },
    ProcSys { id: 20, name: "arch_prctl" }, ProcSys { id: 21, name: "personality" },
    ProcSys { id: 22, name: "unshare" }, ProcSys { id: 23, name: "setns" },
    ProcSys { id: 24, name: "pidfd_open" }, ProcSys { id: 25, name: "pidfd_send_signal" },
    ProcSys { id: 26, name: "rt_sigqueueinfo" }, ProcSys { id: 27, name: "process_vm_readv" },
    ProcSys { id: 28, name: "process_vm_writev" }, ProcSys { id: 29, name: "membarrier" },
    ProcSys { id: 30, name: "reserved-30" }, ProcSys { id: 31, name: "reserved-31" },
];

/// 取名字；越界回 "?"。
pub fn sys_name(id: u8) -> &'static str {
    match SYS_TABLE.get(id as usize) {
        Some(row) => row.name,
        None => "?",
    }
}

/// 表内 id 是否连续 0..=31。
pub fn sys_ids_contiguous() -> bool {
    let mut i = 0usize;
    while i < SYS_TABLE.len() {
        match SYS_TABLE.get(i) {
            Some(row) => if row.id as usize != i { return false; },
            None => return false,
        }
        i += 1;
    }
    true
}

/// 位图是否只含在役位。
pub fn all_active(mask: u32) -> bool {
    mask & !ACTIVE_MASK == 0 && mask != 0
}

// ── 零重写承诺：阶梯值与快照 ─────────────────────────────────────────
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 单字节喂入的 FNV-1a。
pub fn fnv1a_step(h: u32, byte: u8) -> u32 {
    (h ^ byte as u32).wrapping_mul(FNV_PRIME)
}

/// 一位系统调用的错误路径阶梯值：id 与三条代表性 errno 混合。
/// 这是「错误路径指纹」的最小可复算单元——只依赖 id，不依赖运行期状态。
pub fn errno_ladder(id: u8, errno: i32) -> u32 {
    let mut h = FNV_OFFSET;
    h = fnv1a_step(h, id);
    h = fnv1a_step(h, (errno & 0xFF) as u8);
    h = fnv1a_step(h, (((errno >> 8) & 0x7F) as u8) ^ 0x5A);
    h
}

/// 存量错误路径全量快照：逐位喂 id、家族标签与三条 errno 阶梯值。
pub fn legacy_digest() -> u32 {
    let mut h = FNV_OFFSET;
    let mut id = 0u8;
    while id < 32 {
        if bit_of(id) & LEGACY_MASK != 0 {
            h = fnv1a_step(h, id);
            h = fnv1a_step(h, 0x40);
            h = fnv1a_step(h, (errno_ladder(id, EPERM) & 0xFF) as u8);
            h = fnv1a_step(h, (errno_ladder(id, EINVAL) & 0xFF) as u8);
            h = fnv1a_step(h, (errno_ladder(id, ENOMEM) & 0xFF) as u8);
        }
        id += 1;
    }
    h
}

/// 零重写守卫：封边界时拍快照，此后只允许改写接管位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RewriteGuard {
    frozen: bool,
    digest_at_freeze: u32,
    attempts: u32,
    refused: u32,
}

impl RewriteGuard {
    pub const fn new() -> RewriteGuard {
        RewriteGuard { frozen: false, digest_at_freeze: 0, attempts: 0, refused: 0 }
    }

    /// 定格快照。二次冻结回 EBUSY——快照只有第一次是「升级前」。
    pub fn freeze(&mut self) -> Result<(), i32> {
        if self.frozen { return Err(EBUSY); }
        self.digest_at_freeze = legacy_digest();
        self.frozen = true;
        Ok(())
    }

    /// 改写请求。未冻结即改回 EPERM（承诺尚未生效就不许动）；保留位 EINVAL；
    /// 存量位 EPERM 并记账；接管位放行。
    pub fn try_rewrite(&mut self, id: u8) -> Result<(), i32> {
        if !self.frozen { return Err(EPERM); }
        let bit = bit_of(id);
        if bit & ACTIVE_MASK == 0 { return Err(EINVAL); }
        if bit & LEGACY_MASK != 0 {
            self.refused = self.refused.saturating_add(1);
            return Err(EPERM);
        }
        self.attempts = self.attempts.saturating_add(1);
        Ok(())
    }

    /// 承诺是否仍然成立：快照存在且当前存量指纹逐位相等。
    pub fn intact(&self) -> bool {
        self.frozen && legacy_digest() == self.digest_at_freeze
    }

    pub fn frozen(&self) -> bool { self.frozen }
    pub fn snapshot(&self) -> u32 { self.digest_at_freeze }
    pub fn attempts(&self) -> u32 { self.attempts }
    pub fn refused(&self) -> u32 { self.refused }
}

// ── 接管边界声明 ──────────────────────────────────────────────────────
/// 逐位声明器：记录声明次序与已声明位图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryDecl {
    ids: [u8; TAKEOVER_COUNT as usize],
    n: usize,
    declared: u32,
    refused: u32,
}

impl BoundaryDecl {
    pub const fn new() -> BoundaryDecl {
        BoundaryDecl { ids: [0u8; TAKEOVER_COUNT as usize], n: 0, declared: 0, refused: 0 }
    }

    /// 声明一位。次序冻结：保留/越界 EINVAL → 重复 EEXIST → 存量 EPERM →
    /// 容量 ENOMEM。容量检查放最后，因为前三条决定了「这一位有没有资格占坑」。
    pub fn declare(&mut self, id: u8) -> Result<(), i32> {
        let bit = bit_of(id);
        if bit & ACTIVE_MASK == 0 {
            self.refused = self.refused.saturating_add(1); return Err(EINVAL);
        }
        if bit & self.declared != 0 {
            self.refused = self.refused.saturating_add(1); return Err(EEXIST);
        }
        if bit & TAKEOVER_MASK == 0 {
            self.refused = self.refused.saturating_add(1); return Err(EPERM);
        }
        if self.n >= self.ids.len() {
            self.refused = self.refused.saturating_add(1); return Err(ENOMEM);
        }
        if let Some(slot) = self.ids.get_mut(self.n) {
            *slot = id;
        }
        self.n = self.n.saturating_add(1);
        self.declared |= bit;
        Ok(())
    }

    /// 边界是否已声明完整：已声明位图与白名单逐位相等。
    pub fn is_complete(&self) -> bool {
        self.declared == TAKEOVER_MASK
    }

    /// 还差几位。
    pub fn missing(&self) -> u32 {
        (TAKEOVER_MASK & !self.declared).count_ones()
    }

    /// 已声明几位。
    pub fn declared_count(&self) -> u32 {
        self.declared.count_ones()
    }

    pub fn declared_mask(&self) -> u32 { self.declared }
    pub fn declared_len(&self) -> usize { self.n }
    pub fn refused(&self) -> u32 { self.refused }

    /// 声明次序无重复。
    pub fn unique_ids(&self) -> bool {
        let mut i = 0usize;
        while i < self.n {
            let a = match self.ids.get(i).copied() { Some(v) => v, None => return false };
            let mut j = i + 1;
            while j < self.n {
                if let Some(b) = self.ids.get(j) {
                    if *b == a { return false; }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 声明全部 16 位并返回声明器（检查与测试共用；顺序取自表内升序）。
pub fn declare_all() -> BoundaryDecl {
    let mut d = BoundaryDecl::new();
    let mut id = 0u8;
    while id < 32 {
        if bit_of(id) & TAKEOVER_MASK != 0 {
            // 白名单位必然在役，declare 不会失败；失败则交由调用方从 is_complete 看出。
            let _ = d.declare(id);
        }
        id += 1;
    }
    d
}

// ── 增量清单 ──────────────────────────────────────────────────────────
/// 增量类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeltaKind {
    /// 新增一条 E 型错误路径
    Added,
    /// 复用既有错误路径（不新增分支）
    Reused,
    /// 退役旧错误路径
    Retired,
}

impl DeltaKind {
    pub fn code(self) -> u8 {
        match self {
            DeltaKind::Added => 1, DeltaKind::Reused => 2, DeltaKind::Retired => 3,
        }
    }
}

/// 一条增量记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ManifestEntry { pub seq: u32, pub id: u8, pub kind: DeltaKind }

/// 空条目（`[INIT; N]` 要求的常量）。
const ENTRY_INIT: ManifestEntry = ManifestEntry { seq: 0, id: 0, kind: DeltaKind::Added };

/// 追加式清单账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Manifest {
    entries: [ManifestEntry; MAX_MANIFEST],
    n: usize,
    next_seq: u32,
    added: u32,
    reused: u32,
    retired: u32,
    refused: u32,
}

impl Manifest {
    pub const fn new() -> Manifest {
        Manifest {
            entries: [ENTRY_INIT; MAX_MANIFEST], n: 0, next_seq: 1,
            added: 0, reused: 0, retired: 0, refused: 0,
        }
    }

    pub fn len(&self) -> usize { self.n }
    pub fn is_empty(&self) -> bool { self.n == 0 }
    pub fn refused(&self) -> u32 { self.refused }
    pub fn added(&self) -> u32 { self.added }
    pub fn reused(&self) -> u32 { self.reused }
    pub fn retired(&self) -> u32 { self.retired }

    /// 第 i 条（写入序）。
    pub fn entry(&self, i: usize) -> Option<ManifestEntry> {
        if i < self.n { self.entries.get(i).copied() } else { None }
    }

    /// 三类计数之和是否等于条数（增量账守恒）。
    pub fn kinds_sum(&self) -> bool {
        self.added.saturating_add(self.reused).saturating_add(self.retired) == self.n as u32
    }

    /// seq 是否 1..=n 严格递增无空洞。
    pub fn monotonic(&self) -> bool {
        let mut i = 0usize;
        while i < self.n {
            match self.entries.get(i) {
                Some(e) => if e.seq != i as u32 + 1 { return false; },
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// (id, kind) 是否已在账上。
    fn has_pair(&self, id: u8, kind: DeltaKind) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.entries.get(i) {
                if e.id == id && e.kind == kind { return true; }
            }
            i += 1;
        }
        false
    }

    /// 落账（闸门判定已由调用方完成，这里只做账本自身的不变量）。
    /// 顺序：重复 EEXIST → 容量 ENOMEM → 追加并计数。
    fn post_raw(
        &mut self,
        id: u8,
        kind: DeltaKind,
        phase_ok: bool,
        complete: bool,
        active: bool,
        takeover: bool,
    ) -> Result<u32, i32> {
        if !phase_ok || !complete {
            self.refused = self.refused.saturating_add(1); return Err(EINVAL);
        }
        if !active {
            self.refused = self.refused.saturating_add(1); return Err(EINVAL);
        }
        if !takeover {
            self.refused = self.refused.saturating_add(1); return Err(EPERM);
        }
        if self.has_pair(id, kind) {
            self.refused = self.refused.saturating_add(1); return Err(EEXIST);
        }
        if self.n >= MAX_MANIFEST {
            self.refused = self.refused.saturating_add(1); return Err(ENOMEM);
        }
        let seq = self.next_seq;
        if let Some(slot) = self.entries.get_mut(self.n) {
            *slot = ManifestEntry { seq, id, kind };
        }
        self.n = self.n.saturating_add(1);
        self.next_seq = self.next_seq.saturating_add(1);
        match kind {
            DeltaKind::Added => self.added = self.added.saturating_add(1),
            DeltaKind::Reused => self.reused = self.reused.saturating_add(1),
            DeltaKind::Retired => self.retired = self.retired.saturating_add(1),
        }
        Ok(seq)
    }
}

// ── 注记状态机 ────────────────────────────────────────────────────────
/// 接管注记阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotePhase {
    Blank,
    BoundaryClosed,
    ManifestPosted,
    Verified,
    Sealed,
}

impl NotePhase {
    pub fn code(self) -> u8 {
        match self {
            NotePhase::Blank => 0, NotePhase::BoundaryClosed => 1, NotePhase::ManifestPosted => 2,
            NotePhase::Verified => 3, NotePhase::Sealed => 4,
        }
    }
}

/// 接管注记：边界声明 + 零重写守卫 + 增量清单 + 阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TakeoverNote {
    phase: NotePhase,
    boundary: BoundaryDecl,
    manifest: Manifest,
    guard: RewriteGuard,
    refusals: u32,
}

impl TakeoverNote {
    pub const fn new() -> TakeoverNote {
        TakeoverNote {
            phase: NotePhase::Blank, boundary: BoundaryDecl::new(),
            manifest: Manifest::new(), guard: RewriteGuard::new(), refusals: 0,
        }
    }

    pub fn phase(&self) -> NotePhase { self.phase }
    pub fn refusals(&self) -> u32 { self.refusals }
    pub fn boundary(&self) -> &BoundaryDecl { &self.boundary }
    pub fn manifest(&self) -> &Manifest { &self.manifest }
    pub fn guard(&self) -> &RewriteGuard { &self.guard }

    /// 统一拒绝路径：记账 + 回码。闸门语义不变，只是不再每条写三行。
    fn refuse(&mut self, e: i32) -> Result<(), i32> {
        self.refusals = self.refusals.saturating_add(1);
        Err(e)
    }

    /// 逐位声明。封存后 EROFS；边界已闭合后 EBUSY（不允许边关边加）。
    pub fn declare(&mut self, id: u8) -> Result<(), i32> {
        if self.phase == NotePhase::Sealed { return self.refuse(EROFS); }
        if self.phase != NotePhase::Blank { return self.refuse(EBUSY); }
        let r = self.boundary.declare(id);
        if r.is_err() { self.refusals = self.refusals.saturating_add(1); }
        r
    }

    /// 闭合边界：必须声明完整；同时拍零重写快照。二次闭合 EBUSY。
    pub fn close_boundary(&mut self) -> Result<(), i32> {
        if self.phase == NotePhase::Sealed { return self.refuse(EROFS); }
        if self.phase != NotePhase::Blank { return self.refuse(EBUSY); }
        if !self.boundary.is_complete() { return self.refuse(EINVAL); }
        if self.guard.freeze().is_err() { return self.refuse(EBUSY); }
        self.phase = NotePhase::BoundaryClosed;
        Ok(())
    }

    /// 落一笔增量。封存后 EROFS，边界未闭合 EINVAL（由 post_raw 判）。
    pub fn post(&mut self, id: u8, kind: DeltaKind) -> Result<u32, i32> {
        if self.phase == NotePhase::Sealed {
            self.refusals = self.refusals.saturating_add(1);
            return Err(EROFS);
        }
        let phase_ok = matches!(self.phase, NotePhase::BoundaryClosed | NotePhase::ManifestPosted);
        let complete = self.boundary.is_complete();
        let bit = bit_of(id);
        let active = bit & ACTIVE_MASK != 0;
        let takeover = bit & TAKEOVER_MASK != 0;
        match self.manifest.post_raw(id, kind, phase_ok, complete, active, takeover) {
            Ok(seq) => { self.phase = NotePhase::ManifestPosted; Ok(seq) }
            Err(e) => { self.refusals = self.refusals.saturating_add(1); Err(e) }
        }
    }

    /// 改写请求：转交守卫。封存后 EROFS（封存即冻结全部路径）。
    pub fn rewrite(&mut self, id: u8) -> Result<(), i32> {
        if self.phase == NotePhase::Sealed { return self.refuse(EROFS); }
        let r = self.guard.try_rewrite(id);
        if r.is_err() { self.refusals = self.refusals.saturating_add(1); }
        r
    }

    /// 核验：清单非空、边界完整、零重写承诺成立。
    pub fn verify(&mut self) -> Result<(), i32> {
        if self.phase == NotePhase::Sealed { return self.refuse(EROFS); }
        if self.phase != NotePhase::ManifestPosted { return self.refuse(EINVAL); }
        if !self.boundary.is_complete() || self.manifest.is_empty() || !self.guard.intact() {
            return self.refuse(EINVAL);
        }
        self.phase = NotePhase::Verified;
        Ok(())
    }

    /// 封存：仅已核验可封存；封存后一切变更 EROFS。
    pub fn seal(&mut self) -> Result<(), i32> {
        if self.phase == NotePhase::Sealed { return self.refuse(EROFS); }
        if self.phase != NotePhase::Verified { return self.refuse(EINVAL); }
        self.phase = NotePhase::Sealed;
        Ok(())
    }

    /// 增量账守恒：三类之和等于条数，且 seq 单调无洞。
    pub fn manifest_balanced(&self) -> bool {
        self.manifest.kinds_sum() && self.manifest.monotonic()
    }
}
pub fn build_reference_note() -> TakeoverNote {
    let mut note = TakeoverNote::new();
    let mut id = 0u8;
    while id < 32 {
        if bit_of(id) & TAKEOVER_MASK != 0 {
            let _ = note.declare(id);
        }
        id += 1;
    }
    let _ = note.close_boundary();
    let _ = note.post(0, DeltaKind::Added);
    let _ = note.post(4, DeltaKind::Reused);
    let _ = note.post(6, DeltaKind::Retired);
    note
}

// ── 注记指纹 ──────────────────────────────────────────────────────────
/// 注记指纹：阶段 + 边界位图 + 清单(seq,id,kind) + 零重写快照。
pub fn note_digest(note: &TakeoverNote) -> u32 {
    let mut h = FNV_OFFSET;
    h = fnv1a_step(h, note.phase().code());
    let mask = note.boundary().declared_mask();
    h = fnv1a_step(h, (mask & 0xFF) as u8);
    h = fnv1a_step(h, ((mask >> 8) & 0xFF) as u8);
    h = fnv1a_step(h, ((mask >> 16) & 0xFF) as u8);
    h = fnv1a_step(h, ((mask >> 24) & 0xFF) as u8);
    h = fnv1a_step(h, note.manifest().len() as u8);
    let mut i = 0usize;
    while i < note.manifest().len() {
        if let Some(e) = note.manifest().entry(i) {
            h = fnv1a_step(h, (e.seq & 0xFF) as u8);
            h = fnv1a_step(h, e.id);
            h = fnv1a_step(h, e.kind.code());
        }
        i += 1;
    }
    let snap = note.guard().snapshot();
    h = fnv1a_step(h, (snap & 0xFF) as u8);
    h = fnv1a_step(h, ((snap >> 8) & 0xFF) as u8);
    h = fnv1a_step(h, ((snap >> 16) & 0xFF) as u8);
    h = fnv1a_step(h, ((snap >> 24) & 0xFF) as u8);
    h
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（序号 1..=15，段内码 = SEG_BASE + 序号）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeoverCode {
    BoundaryIncomplete = 1,
    ReservedId = 2,
    DuplicateId = 3,
    LegacyTakeover = 4,
    ManifestBeforeBoundary = 5,
    ManifestLegacy = 6,
    ManifestDuplicate = 7,
    ManifestFull = 8,
    SeqGap = 9,
    RewriteLeak = 10,
    PhaseSkip = 11,
    SealedMutation = 12,
    SegmentViolation = 13,
    DigestDrift = 14,
    MaskRelation = 15,
}

impl TakeoverCode {
    pub fn code(self) -> u16 { self as u16 }

    pub fn seg_code(self) -> u32 { SEG_BASE + self as u32 }

    pub fn detail(self) -> &'static str {
        match self {
            TakeoverCode::BoundaryIncomplete => "接管边界未闭合：16 位白名单未声明齐；请先补齐再闭合",
            TakeoverCode::ReservedId => "保留 id 被声明：30/31 未分配；请改用 0..=29 的在役位",
            TakeoverCode::DuplicateId => "重复声明：该位已在边界内；请勿重放同一 id",
            TakeoverCode::LegacyTakeover => "越界接管被拒：该位属存量错误路径；请留在 LEGACY 集内",
            TakeoverCode::ManifestBeforeBoundary => "清单先于边界：边界未闭合即落账；请先闭合边界",
            TakeoverCode::ManifestLegacy => "清单登记存量位：等于偷改存量错误路径；请只登记接管位",
            TakeoverCode::ManifestDuplicate => "清单重复：(id, kind) 已落账；请换类别或换 id",
            TakeoverCode::ManifestFull => "清单已满：16 条槽位耗尽；请先合并增量再落账",
            TakeoverCode::SeqGap => "序号断档：seq 必须从 1 起严格递增；请查回滚是否吞了序号",
            TakeoverCode::RewriteLeak => "零重写承诺被破：存量指纹在冻结后变化；请回滚对存量路径的改写",
            TakeoverCode::PhaseSkip => "阶段越级：未闭合/未落账/未核验就推进；请按冻结次序走",
            TakeoverCode::SealedMutation => "封存后变更被拒：注记已封存，一切写入只读；请新开一张注记",
            TakeoverCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7B800 且 0x40 对齐",
            TakeoverCode::DigestDrift => "指纹漂移：两次注记指纹不等；请排查隐藏状态",
            TakeoverCode::MaskRelation => "掩码关系被破：TAKEOVER 必须是 ACTIVE 子集且与 RESERVED 不交",
        }
    }

    pub const ALL: &'static [TakeoverCode] = &[
        TakeoverCode::BoundaryIncomplete, TakeoverCode::ReservedId, TakeoverCode::DuplicateId,
        TakeoverCode::LegacyTakeover, TakeoverCode::ManifestBeforeBoundary,
        TakeoverCode::ManifestLegacy, TakeoverCode::ManifestDuplicate, TakeoverCode::ManifestFull,
        TakeoverCode::SeqGap, TakeoverCode::RewriteLeak, TakeoverCode::PhaseSkip,
        TakeoverCode::SealedMutation, TakeoverCode::SegmentViolation,
        TakeoverCode::DigestDrift, TakeoverCode::MaskRelation,
    ];
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9298 判据集（静态 19 条 + 动态 3 条）。
pub fn run_unx9298_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9298");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && SEG_BASE == 0x7B800
        && SEG_BASE + SEG_SPAN == 0x7B840;
    s.add("段位守卫：0x7B800 独占 0x40 窗口", seg_ok,
          "段基址须 0x40 对齐且紧邻 F9299 窗口，不得重叠");

    s.add("系统调用表：32 槽、id 连续 0..=31",
          SYS_TABLE.len() == ID_SPACE && sys_ids_contiguous() && sys_name(0) == "fork"
              && sys_name(31) == "reserved-31",
          "表是边界判据的坐标系：id 必须与位序一一对应，否则位图会指错系统调用");

    s.add("掩码关系：TAKEOVER ⊆ ACTIVE 且与 RESERVED 不交",
          TAKEOVER_MASK & !ACTIVE_MASK == 0 && TAKEOVER_MASK & RESERVED_MASK == 0
              && TAKEOVER_MASK != 0 && all_active(ACTIVE_MASK),
          "接管集若含保留位或越界位，边界声明从一开始就不可能闭合");

    s.add("掩码计数：接管 16 位、存量 14 位、互不重叠",
          TAKEOVER_MASK.count_ones() == TAKEOVER_COUNT
              && LEGACY_MASK.count_ones() == LEGACY_COUNT
              && LEGACY_MASK == ACTIVE_MASK & !TAKEOVER_MASK
              && TAKEOVER_MASK & LEGACY_MASK == 0,
          "14 = 30 - 16 是硬等式：任何一位挪动都会同时破坏两张表的规模");

    let full = declare_all();
    s.add("边界声明：16 位逐位声明后可闭合",
          full.is_complete() && full.declared_count() == TAKEOVER_COUNT
              && full.missing() == 0 && full.declared_len() == TAKEOVER_COUNT as usize
              && full.unique_ids(),
          "声明必须逐位落账、无重复、无遗漏，闭合条件只看位图等式");

    let mut reserved = BoundaryDecl::new();
    let (r30, r31, r32) = (reserved.declare(30), reserved.declare(31), reserved.declare(32));
    let mut legacy = BoundaryDecl::new();
    let (l9, l13) = (legacy.declare(9), legacy.declare(13));
    s.add("保留位与存量位：EINVAL / EPERM 且边界不变",
          r30 == Err(EINVAL) && r31 == Err(EINVAL) && r32 == Err(EINVAL)
              && reserved.declared_count() == 0 && reserved.declared_len() == 0
              && reserved.refused() == 3 && l9 == Err(EPERM) && l13 == Err(EPERM)
              && legacy.declared_mask() == 0 && legacy.declared_len() == 0 && legacy.refused() == 2,
          "保留位没有语义、存量位受零重写承诺保护；两类都必须拒且不占位");

    let mut dup = BoundaryDecl::new();
    let d0 = dup.declare(0);
    let d0_again = dup.declare(0);
    s.add("重复声明：EEXIST 且位图不重复计数",
          d0.is_ok() && d0_again == Err(EEXIST)
              && dup.declared_mask() == bit_of(0) && dup.declared_count() == 1
              && dup.declared_len() == 1,
          "重复位若被计两次，闭合判定会提前为真，边界就会被少声明一半");

    let mut guess = RewriteGuard::new();
    let early = guess.try_rewrite(13);
    let frozen = guess.freeze();
    let freeze_again = guess.freeze();
    s.add("零重写：未冻结拒改、二次冻结 EBUSY",
          early == Err(EPERM) && frozen.is_ok() && freeze_again == Err(EBUSY)
              && guess.frozen() && guess.intact(),
          "快照只有第一次是「升级前」，二次冻结会把已改过的状态当成基线");

    let mut guard = RewriteGuard::new();
    let _ = guard.freeze();
    let before = legacy_digest();
    let leak = guard.try_rewrite(13);
    let allowed = guard.try_rewrite(4);
    let reserved_rw = guard.try_rewrite(30);
    s.add("零重写承诺格：存量 EPERM、接管放行、指纹逐位不变",
          leak == Err(EPERM) && allowed.is_ok() && reserved_rw == Err(EINVAL)
              && guard.refused() == 1 && guard.attempts() == 1
              && legacy_digest() == before && guard.intact(),
          "承诺必须是可复算等式：拒绝改写后存量指纹仍与冻结值逐位相等");

    let note = build_reference_note();
    s.add("清单落账：三条增量 seq 1..3 且三类计数守恒",
          note.manifest().len() == 3 && note.manifest().monotonic()
              && note.manifest().added() == 1 && note.manifest().reused() == 1
              && note.manifest().retired() == 1 && note.manifest_balanced()
              && note.manifest().entry(0).map(|e| e.seq) == Some(1)
              && note.manifest().entry(2).map(|e| e.id) == Some(6),
          "追加式账本的三个不变量：序号无洞、组合唯一、三类之和对条数");

    let mut early_note = TakeoverNote::new();
    let post_before = early_note.post(0, DeltaKind::Added);
    s.add("次序守卫：边界未闭合时落清单回 EINVAL",
          post_before == Err(EINVAL) && early_note.phase() == NotePhase::Blank
              && early_note.manifest().is_empty() && early_note.refusals() == 1,
          "清单先于边界等于先斩后奏：接管范围还没定就登记增量，无法审计");

    let mut bad_kind = build_reference_note();
    let legacy_post = bad_kind.post(13, DeltaKind::Added);
    let reserved_post = bad_kind.post(30, DeltaKind::Added);
    let dup_post = bad_kind.post(0, DeltaKind::Added);
    s.add("清单拒绝面：存量 EPERM、保留 EINVAL、重复组合 EEXIST",
          legacy_post == Err(EPERM) && reserved_post == Err(EINVAL)
              && dup_post == Err(EEXIST) && bad_kind.manifest().len() == 3,
          "拒绝不得占用序号：三条拒绝之后清单仍只有最初三条");

    let mut cap_note = build_reference_note();
    let mut posted = 3u32;
    let mut id = 0u8;
    while id < 32 {
        if bit_of(id) & TAKEOVER_MASK != 0 {
            let r = cap_note.post(id, DeltaKind::Reused);
            if r.is_ok() { posted = posted.saturating_add(1); }
        }
        id += 1;
    }
    let overflow = cap_note.post(0, DeltaKind::Retired);
    s.add("清单容量：16 条满后第 17 条回 ENOMEM",
          cap_note.manifest().len() == MAX_MANIFEST && overflow == Err(ENOMEM)
              && posted >= MAX_MANIFEST as u32 && cap_note.manifest().monotonic(),
          "定容账本不得越界写入；满账必须显性回 ENOMEM 而不是静默丢弃");

    let mut sealed = build_reference_note();
    let verified = sealed.verify();
    let sealed_ok = sealed.seal();
    let after_seal = sealed.post(1, DeltaKind::Added);
    let declare_after = sealed.declare(1);
    let rewrite_after = sealed.rewrite(4);
    s.add("封存：核验后可封存，封存后一切变更 EROFS",
          verified.is_ok() && sealed_ok.is_ok() && sealed.phase() == NotePhase::Sealed
              && after_seal == Err(EROFS) && declare_after == Err(EROFS)
              && rewrite_after == Err(EROFS),
          "封存必须是硬闸门：落账/声明/改写三条路径统一只读");

    let mut unverified = build_reference_note();
    let seal_too_early = unverified.seal();
    let mut empty_manifest = TakeoverNote::new();
    let mut id2 = 0u8;
    while id2 < 32 {
        if bit_of(id2) & TAKEOVER_MASK != 0 { let _ = empty_manifest.declare(id2); }
        id2 += 1;
    }
    let _ = empty_manifest.close_boundary();
    let verify_empty = empty_manifest.verify();
    s.add("阶段越级：未核验不得封存、空清单不得核验",
          seal_too_early == Err(EINVAL) && verify_empty == Err(EINVAL)
              && empty_manifest.phase() == NotePhase::BoundaryClosed,
          "阶段判据的价值在拒绝：跳过核验就封存，等于把未审的增量钉成既成事实");

    let mut half = TakeoverNote::new();
    let _ = half.declare(0);
    let half_close = half.close_boundary();
    s.add("闭合条件：少声明一位即不得闭合",
          half_close == Err(EINVAL) && half.phase() == NotePhase::Blank
              && half.boundary().missing() == TAKEOVER_COUNT - 1,
          "闭合只看位图等式的完整成立，不做「差不多就放行」的宽容");

    let fp_a = note_digest(&build_reference_note());
    let fp_b = note_digest(&build_reference_note());
    let mut sealed_note = build_reference_note();
    let _ = sealed_note.verify();
    let _ = sealed_note.seal();
    s.add("双跑一致：同构造路径注记指纹相等且封存后变化",
          fp_a == fp_b && note_digest(&sealed_note) != fp_a,
          "指纹既要可复现（双跑相等），又要能区分阶段（封存后必须变）");

    s.add("errno 数值：与 Linux 头文件逐一相等",
          EPERM == 1 && EINVAL == 22 && EEXIST == 17 && ENOMEM == 12
              && EROFS == 30 && EBUSY == 16 && ENOENT == 2 && ECHILD == 10,
          "拒绝面用到的每个码都必须是内核码值，否则审计工具无法归并");

    let mut uniq = true;
    let mut i = 0usize;
    while i < TakeoverCode::ALL.len() {
        let mut j = i + 1;
        while j < TakeoverCode::ALL.len() {
            if let (Some(a), Some(b)) = (TakeoverCode::ALL.get(i), TakeoverCode::ALL.get(j)) {
                if a.seg_code() == b.seg_code() || a.code() == b.code() { uniq = false; }
            }
            j += 1;
        }
        i += 1;
    }
    let codes_ok = uniq && TakeoverCode::ALL.len() == 15
        && TakeoverCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：15 码段内唯一且文案非空", codes_ok,
          "码值唯一是红项定位的前提；文案为空等于把诊断丢回给读码人");

    // ── 动态明细 ─────────────────────────────────────────────────────
    // 首末位用位运算取，不构造中间列表——边界快照只需要两个端点。
    let first = match TAKEOVER_MASK.trailing_zeros() {
        n if n < 32 => n as u8,
        _ => 0,
    };
    let last = match 31u32.checked_sub(TAKEOVER_MASK.leading_zeros()) {
        Some(n) => n as u8,
        None => 0,
    };
    let boundary_text = format!(
        "边界 {}/{} 位，首个 id {} ({})，末位 {} ({})，缺失 {}",
        full.declared_count(), TAKEOVER_COUNT, first, sys_name(first), last,
        sys_name(last), full.missing()
    );
    s.add_dyn("边界快照", full.is_complete(), &boundary_text);

    let manifest_text = format!(
        "清单 {} 条：新增 {} / 复用 {} / 退役 {}，拒绝 {}，守恒 {}",
        note.manifest().len(), note.manifest().added(), note.manifest().reused(),
        note.manifest().retired(), note.manifest().refused(),
        note.manifest_balanced()
    );
    s.add_dyn("清单快照", note.manifest_balanced(), &manifest_text);

    let guard_text = format!(
        "存量指纹 0x{:08X}，冻结 0x{:08X}，放行 {}，拒绝 {}",
        legacy_digest(), guard.snapshot(), guard.attempts(), guard.refused()
    );
    s.add_dyn("零重写快照", guard.intact(), &guard_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_and_masks_are_consistent() {
        assert_eq!(SYS_TABLE.len(), 32);
        assert_eq!(sys_ids_contiguous(), true);
        assert_eq!(TAKEOVER_MASK & !ACTIVE_MASK, 0);
        assert_eq!(TAKEOVER_MASK & RESERVED_MASK, 0);
        assert_eq!(TAKEOVER_MASK.count_ones(), 16);
        assert_eq!(LEGACY_MASK.count_ones(), 14);
        assert_eq!(TAKEOVER_MASK & LEGACY_MASK, 0);
        assert_eq!(bit_of(0), 1);
        assert_eq!(bit_of(31), 0x8000_0000);
        assert_eq!(bit_of(32), 0);
        assert_eq!(bit_of(255), 0);
    }

    #[test]
    fn boundary_accepts_exactly_the_whitelist() {
        let full = declare_all();
        assert_eq!(full.is_complete(), true);
        assert_eq!(full.declared_count(), 16);
        assert_eq!(full.missing(), 0);
        assert_eq!(full.unique_ids(), true);
        let mut d = BoundaryDecl::new();
        assert_eq!(d.declare(9), Err(EPERM));
        assert_eq!(d.declare(30), Err(EINVAL));
        assert_eq!(d.declare(0), Ok(()));
        assert_eq!(d.declare(0), Err(EEXIST));
    }

    #[test]
    fn legacy_digest_is_stable_and_guarded() {
        let before = legacy_digest();
        assert_eq!(legacy_digest(), before);
        let mut g = RewriteGuard::new();
        assert_eq!(g.try_rewrite(13), Err(EPERM));
        assert_eq!(g.freeze(), Ok(()));
        assert_eq!(g.freeze(), Err(EBUSY));
        assert_eq!(g.try_rewrite(13), Err(EPERM));
        assert_eq!(g.try_rewrite(4), Ok(()));
        assert_eq!(g.try_rewrite(30), Err(EINVAL));
        assert_eq!(g.intact(), true);
        assert_eq!(legacy_digest(), before);
    }

    #[test]
    fn manifest_appends_in_order() {
        let note = build_reference_note();
        assert_eq!(note.manifest().len(), 3);
        assert_eq!(note.manifest().monotonic(), true);
        assert_eq!(note.manifest().kinds_sum(), true);
        assert_eq!(note.manifest().entry(0).map(|e| e.seq), Some(1));
        assert_eq!(note.manifest().entry(1).map(|e| e.id), Some(4));
        assert_eq!(note.manifest().entry(2).map(|e| e.kind), Some(DeltaKind::Retired));
        let a = build_reference_note();
        let b = build_reference_note();
        assert_eq!(note_digest(&a), note_digest(&b));
        let mut c = build_reference_note();
        let _ = c.verify();
        let _ = c.seal();
        assert_eq!(note_digest(&c) != note_digest(&a), true);
    }

    #[test]
    fn manifest_rejections_do_not_consume_slots() {
        let mut note = build_reference_note();
        assert_eq!(note.post(13, DeltaKind::Added), Err(EPERM));
        assert_eq!(note.post(30, DeltaKind::Added), Err(EINVAL));
        assert_eq!(note.post(0, DeltaKind::Added), Err(EEXIST));
        assert_eq!(note.manifest().len(), 3);
        assert_eq!(note.manifest().monotonic(), true);
    }

    #[test]
    fn manifest_capacity_is_hard() {
        let mut note = build_reference_note();
        let mut id = 0u8;
        while id < 32 {
            if bit_of(id) & TAKEOVER_MASK != 0 {
                let _ = note.post(id, DeltaKind::Reused);
            }
            id += 1;
        }
        assert_eq!(note.manifest().len(), MAX_MANIFEST);
        assert_eq!(note.post(0, DeltaKind::Retired), Err(ENOMEM));
        assert_eq!(note.manifest().monotonic(), true);
    }

    #[test]
    fn phase_machine_ends_sealed() {
        let mut note = build_reference_note();
        assert_eq!(note.verify(), Ok(()));
        assert_eq!(note.seal(), Ok(()));
        assert_eq!(note.phase(), NotePhase::Sealed);
        assert_eq!(note.post(1, DeltaKind::Added), Err(EROFS));
        assert_eq!(note.declare(1), Err(EROFS));
        assert_eq!(note.verify(), Err(EROFS));
        let mut early = TakeoverNote::new();
        assert_eq!(early.post(0, DeltaKind::Added), Err(EINVAL));
        assert_eq!(early.verify(), Err(EINVAL));
        assert_eq!(early.seal(), Err(EINVAL));
        let _ = early.declare(0);
        assert_eq!(early.close_boundary(), Err(EINVAL));
    }

    #[test]
    fn digest_is_reproducible() {
        let a = build_reference_note();
        let b = build_reference_note();
        assert_eq!(note_digest(&a), note_digest(&b));
        let mut c = build_reference_note();
        let _ = c.verify();
        let _ = c.seal();
        assert_eq!(note_digest(&c) != note_digest(&a), true);
    }

    #[test]
    fn codes_are_unique() {
        assert_eq!(TakeoverCode::ALL.len(), 15);
        assert_eq!(TakeoverCode::BoundaryIncomplete.code(), 1);
        assert_eq!(TakeoverCode::MaskRelation.seg_code(), SEG_BASE + 15);
        for c in TakeoverCode::ALL.iter() {
            assert_eq!(c.detail().is_empty(), false);
        }
        let s = run_unx9298_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
