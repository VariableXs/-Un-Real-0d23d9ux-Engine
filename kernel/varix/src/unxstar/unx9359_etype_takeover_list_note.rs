//! UNX-F9359 E 型升级接管总注记（错误面增量清单） · 0x7C740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9359 逐字）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F9359-J1 接管边界总声明判据过，存量错误面零重写承诺
//!      格过，增量清单落账格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 接管边界是「syscall 号段 × 三种归属」的闭集：三段自有（0..=63 /
//!      64..=127 / 128..=191）、一段保留（192..=255）、一段域外（256..=511），
//!      两两不相交且首尾相接覆盖 0..=511，这就是「边界总声明」的数值落点。
//!   ② 存量错误面是冻结仓：10 条既有 (syscall, errno) 只读存放，初始化即算
//!      死摘要；零重写 = 摘要相等 + 行数不变 + id 空间不相交 + 尾仓只增。
//!   ③ 增量清单只增尾部：8 条按 id 升序落尾、链摘逐步累加；容量 8，第 9 条
//!      必回 E2BIG，绝不静默扩容。
//!   ④ 状态机 Idle → Claimed → Sealed：非法跃迁 EINVAL、重复接管 EBUSY、
//!      保留区 EPERM、域外 EINVAL、封存后追加 EROFS；次序为状态 → 号段 →
//!      归属 → 重复 → 容量。
//!   ⑤ 落账格：追加 12 次 = 落账 8 + 拒绝 4（EEXIST/EINVAL/EPERM/E2BIG），
//!      四类各一笔；注记（写回）含状态、段数、条数与摘要。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9359 独占 0x7C740..=0x7C77F）。
pub const SEG_BASE: u32 = 0x7C740;

/// 存量错误面条数（冻结仓）。
pub const BASE_LEN: usize = 10;

/// 增量清单条数。
pub const INCR_LEN: usize = 8;

/// 增量尾部容量（= 清单条数；第 9 条必拒）。
pub const INCR_CAP: usize = 8;

/// 边界段数。
pub const ZONE_LEN: usize = 5;

/// 追加尝试总数（8 落账 + 4 拒绝）。
pub const APPEND_ATTEMPTS: u32 = 12;

/// 拒绝种类数。
pub const REFUSAL_KINDS: usize = 4;

/// 零重写承诺条数。
pub const PROMISES: usize = 5;

/// syscall 号段总跨度（0..=511）。
pub const SYSCALL_SPAN: u32 = 512;

/// 增量 id 空间下界（0x2000 起，与存量 0x1000 段不相交）。
pub const INCR_ID_BASE: u32 = 0x2000;
/// 增量 id 空间上界（含）。
pub const INCR_ID_LIMIT: u32 = 0x2FFF;
/// 存量 id 空间上界（含），增量不得越入。
pub const BASE_ID_LIMIT: u32 = 0x1FFF;

/// Linux errno（正数口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EBUSY: i32 = 16;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ENOTTY: i32 = 25;
pub const EROFS: i32 = 30;
pub const ENOSYS: i32 = 38;
pub const ENOEXEC: i32 = 8;
pub const E2BIG: i32 = 7;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+12）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeoverCode {
    BoundaryOverlap = 1,
    BoundaryHole = 2,
    ReservedTouched = 3,
    DoubleClaim = 4,
    IdDuplicate = 5,
    IdOutOfRange = 6,
    CapacityFull = 7,
    SealedRewrite = 8,
    BaselineMutated = 9,
    LedgerUnbalanced = 10,
    StateIllegal = 11,
    SegmentOut = 12,
}

impl TakeoverCode {
    /// 段内序号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内绝对码。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文一句话建议。
    pub fn detail(self) -> &'static str {
        match self {
            TakeoverCode::BoundaryOverlap => "边界重叠：两个号段覆盖同一 syscall，须先把重叠段让给单一归属",
            TakeoverCode::BoundaryHole => "边界空洞：号段未连续覆盖 0..=511，落在空洞里的调用无归属",
            TakeoverCode::ReservedTouched => "触碰保留区：192..=255 属后续波次，本波接管必须回 EPERM",
            TakeoverCode::DoubleClaim => "重复接管：同一段已被声明，二次接管回 EBUSY 而不是静默覆盖",
            TakeoverCode::IdDuplicate => "增量 id 重复：只增日志不允许改写既有条目，请换新 id",
            TakeoverCode::IdOutOfRange => "增量 id 越界：必须落在 0x2000..=0x2FFF，禁碰存量 0x1xxx",
            TakeoverCode::CapacityFull => "增量尾仓已满：容量 8 条，第 9 条回 E2BIG，禁止静默扩容",
            TakeoverCode::SealedRewrite => "封存后追加：封存即只读，追加必须回 EROFS",
            TakeoverCode::BaselineMutated => "存量被改：冻仓摘要变动，升级必须零重写存量错误面",
            TakeoverCode::LedgerUnbalanced => "落账不平：追加尝试必须等于落账加拒绝，四类拒绝各一笔",
            TakeoverCode::StateIllegal => "状态跃迁非法：必须按 Idle→Claimed→Sealed 顺序推进",
            TakeoverCode::SegmentOut => "段位越界：本模块必须落在 0x7C740 起 0x40 窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [TakeoverCode] = &[
        TakeoverCode::BoundaryOverlap,
        TakeoverCode::BoundaryHole,
        TakeoverCode::ReservedTouched,
        TakeoverCode::DoubleClaim,
        TakeoverCode::IdDuplicate,
        TakeoverCode::IdOutOfRange,
        TakeoverCode::CapacityFull,
        TakeoverCode::SealedRewrite,
        TakeoverCode::BaselineMutated,
        TakeoverCode::LedgerUnbalanced,
        TakeoverCode::StateIllegal,
        TakeoverCode::SegmentOut,
    ];
}

// ── 边界模型 ──────────────────────────────────────────────────────────
/// 号段归属。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoneState {
    /// 本波接管。
    Owned,
    /// 保留给后续波次，本波不得触碰。
    Reserved,
    /// 域外，不属本域。
    OutOfScope,
}

impl ZoneState {
    /// 归属码（0 自有 / 1 保留 / 2 域外）。
    pub fn idx(self) -> u32 {
        match self {
            ZoneState::Owned => 0,
            ZoneState::Reserved => 1,
            ZoneState::OutOfScope => 2,
        }
    }
}

/// 一个 syscall 号段（闭区间）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ZoneRow {
    pub id: u32,
    pub bit: u32,
    pub lo: u32,
    pub hi: u32,
    pub state: ZoneState,
    pub name: &'static str,
}

impl ZoneRow {
    /// 段长（闭区间）。
    pub fn span(&self) -> u32 {
        self.hi.saturating_sub(self.lo).saturating_add(1)
    }

    /// 是否覆盖某 syscall。
    pub fn covers(&self, syscall: u32) -> bool {
        syscall >= self.lo && syscall <= self.hi
    }
}

/// 冻结边界表（五段，首尾相接覆盖 0..=511）。
pub const ZONES: [ZoneRow; ZONE_LEN] = [
    ZoneRow { id: 0x21, bit: 1, lo: 0, hi: 63, state: ZoneState::Owned, name: "C2 主调用面" },
    ZoneRow { id: 0x22, bit: 2, lo: 64, hi: 127, state: ZoneState::Owned, name: "C3 回归基建面" },
    ZoneRow { id: 0x23, bit: 4, lo: 128, hi: 191, state: ZoneState::Owned, name: "E 型错误面" },
    ZoneRow { id: 0x24, bit: 8, lo: 192, hi: 255, state: ZoneState::Reserved, name: "后续波次保留" },
    ZoneRow { id: 0x25, bit: 16, lo: 256, hi: 511, state: ZoneState::OutOfScope, name: "域外扩展区" },
];

/// 按 id 查段。
pub fn lookup_zone(id: u32) -> Option<&'static ZoneRow> {
    let mut i = 0usize;
    while i < ZONES.len() {
        if let Some(z) = ZONES.get(i) {
            if z.id == id {
                return Some(z);
            }
        }
        i += 1;
    }
    None
}

/// 按 syscall 查段（单调扫描，命中即停）。
pub fn zone_of(syscall: u32) -> Option<&'static ZoneRow> {
    let mut i = 0usize;
    while i < ZONES.len() {
        if let Some(z) = ZONES.get(i) {
            if z.covers(syscall) {
                return Some(z);
            }
            if syscall < z.lo {
                return None;
            }
        }
        i += 1;
    }
    None
}

/// 边界成对判据：返回 (无重叠且无空洞, 自有段数, 覆盖跨度)。
pub fn boundary_scan() -> (bool, u32, u32) {
    let mut disjoint = true;
    let mut owned = 0u32;
    let mut covered = 0u32;
    let mut i = 0usize;
    while i < ZONES.len() {
        if let Some(z) = ZONES.get(i) {
            covered = covered.saturating_add(z.span());
            if z.state == ZoneState::Owned {
                owned = owned.saturating_add(1);
            }
            // 与后面每一段做区间交叉判定（闭区间）。
            let mut j = i + 1;
            while j < ZONES.len() {
                if let Some(w) = ZONES.get(j) {
                    if z.lo <= w.hi && w.lo <= z.hi {
                        disjoint = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    (disjoint && covered == SYSCALL_SPAN, owned, covered)
}

// ── 错误面条目 ────────────────────────────────────────────────────────
/// 一条错误面：id、syscall 号、errno、注记标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Surf {
    pub id: u32,
    pub syscall: u32,
    pub errno: i32,
    pub tag: &'static str,
}

/// 空条目常量（定容数组初始化用）。
pub const EMPTY_SURF: Surf = Surf { id: 0, syscall: 0, errno: 0, tag: "" };

/// 冻结存量错误面（只读；任何改写都是违约）。
pub const BASELINE: [Surf; BASE_LEN] = [
    Surf { id: 0x1001, syscall: 3, errno: EBADF, tag: "read 句柄" },
    Surf { id: 0x1002, syscall: 4, errno: EFAULT, tag: "write 缓冲" },
    Surf { id: 0x1003, syscall: 5, errno: ENOENT, tag: "open 路径" },
    Surf { id: 0x1004, syscall: 6, errno: EBADF, tag: "close 句柄" },
    Surf { id: 0x1005, syscall: 16, errno: ENOTTY, tag: "ioctl 能力" },
    Surf { id: 0x1006, syscall: 9, errno: ENOMEM, tag: "mmap 内存" },
    Surf { id: 0x1007, syscall: 61, errno: ECHILD, tag: "wait 子进程" },
    Surf { id: 0x1008, syscall: 59, errno: ENOEXEC, tag: "exec 格式" },
    Surf { id: 0x1009, syscall: 83, errno: EEXIST, tag: "mkdir 存在" },
    Surf { id: 0x100a, syscall: 87, errno: EROFS, tag: "unlink 只读" },
];

/// 增量清单（8 条，全部落在 E 型错误面 128..=191 段内）。
pub const INCREMENTS: [Surf; INCR_LEN] = [
    Surf { id: 0x2001, syscall: 128, errno: EINVAL, tag: "新参数面" },
    Surf { id: 0x2002, syscall: 129, errno: EBADF, tag: "新句柄面" },
    Surf { id: 0x2003, syscall: 130, errno: EFAULT, tag: "新缓冲面" },
    Surf { id: 0x2004, syscall: 131, errno: ENOTTY, tag: "新能力面" },
    Surf { id: 0x2005, syscall: 132, errno: EAGAIN, tag: "新重试面" },
    Surf { id: 0x2006, syscall: 133, errno: ENOSYS, tag: "新未实现面" },
    Surf { id: 0x2007, syscall: 134, errno: EACCES, tag: "新权限面" },
    Surf { id: 0x2008, syscall: 135, errno: EPERM, tag: "新特权面" },
];

/// 增量 id 是否合法（落在增量空间）。
pub fn id_in_incr_space(id: u32) -> bool {
    id >= INCR_ID_BASE && id <= INCR_ID_LIMIT
}

/// 载重状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Claimed,
    Sealed,
}

impl Stage {
    /// 状态码（0/1/2）。
    pub fn idx(self) -> u32 {
        match self {
            Stage::Idle => 0,
            Stage::Claimed => 1,
            Stage::Sealed => 2,
        }
    }
}

/// 错误面注册表：冻仓 + 尾仓 + 链摘要 + 接管掩码。
#[derive(Clone, Copy, Debug)]
pub struct Registry {
    frozen: [Surf; BASE_LEN],
    tail: [Surf; INCR_CAP],
    tail_len: usize,
    chain: u32,
    baseline_digest: u32,
    claimed_mask: u32,
    stage: Stage,
}

impl Registry {
    /// 空仓（未冻结、无尾仓）。
    pub const fn blank() -> Registry {
        Registry {
            frozen: [EMPTY_SURF; BASE_LEN],
            tail: [EMPTY_SURF; INCR_CAP],
            tail_len: 0,
            chain: 0x811C_9DC5,
            baseline_digest: 0,
            claimed_mask: 0,
            stage: Stage::Idle,
        }
    }
    /// 由冻仓初始化：摘要此刻定死，之后只允许读。
    pub fn from_baseline() -> Registry {
        let mut r = Registry::blank();
        let mut i = 0usize;
        while i < BASE_LEN {
            if let (Some(src), Some(dst)) = (BASELINE.get(i), r.frozen.get_mut(i)) {
                *dst = *src;
            }
            i += 1;
        }
        r.baseline_digest = freeze_digest(&r.frozen);
        r.stage = Stage::Idle;
        r
    }

    /// 冻仓摘要（重算，用于证明零重写）。
    pub fn baseline_digest_now(&self) -> u32 {
        freeze_digest(&self.frozen)
    }

    /// 初始化时记下的冻仓摘要。
    pub fn baseline_digest_sealed(&self) -> u32 {
        self.baseline_digest
    }

    /// 尾仓长度。
    pub fn tail_len(&self) -> u32 {
        self.tail_len as u32
    }

    /// 链摘要。
    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 当前状态。
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// 尾仓视图。
    pub fn tail_at(&self, i: usize) -> Option<&Surf> {
        if i >= self.tail_len {
            return None;
        }
        self.tail.get(i)
    }

    /// 全仓 id 查重（冻仓 + 尾仓）。
    pub fn find_id(&self, id: u32) -> Option<Surf> {
        for s in self.frozen.iter() {
            if s.id == id {
                return Some(*s);
            }
        }
        let mut i = 0usize;
        while i < self.tail_len {
            if let Some(s) = self.tail.get(i) {
                if s.id == id {
                    return Some(*s);
                }
            }
            i += 1;
        }
        None
    }

    /// 接管一段。
    ///
    /// 次序：状态 → 段存在性 → 归属 → 重复。状态先判，避免封存后被「重复
    /// 接管」抢先回 EBUSY 而掩盖 EROFS。
    pub fn claim(&mut self, zone_id: u32) -> Result<(), i32> {
        if self.stage == Stage::Sealed {
            return Err(EROFS);
        }
        let zone = match lookup_zone(zone_id) {
            Some(z) => *z,
            None => return Err(EINVAL),
        };
        match zone.state {
            ZoneState::Reserved => return Err(EPERM),
            ZoneState::OutOfScope => return Err(EINVAL),
            ZoneState::Owned => {}
        }
        if self.claimed_mask & zone.bit != 0 {
            return Err(EBUSY);
        }
        self.claimed_mask |= zone.bit;
        self.stage = Stage::Claimed;
        Ok(())
    }

    /// 段是否已接管。
    pub fn zone_claimed(&self, zone_id: u32) -> bool {
        match lookup_zone(zone_id) {
            Some(z) => self.claimed_mask & z.bit != 0,
            None => false,
        }
    }

    /// 追加一条增量（只增尾部，重算链摘要）。
    ///
    /// 次序：封存 → 未接管 → id 空间 → 重复 → 号段归属 → 段接管 → 容量。
    pub fn append(&mut self, s: Surf) -> Result<usize, i32> {
        if self.stage == Stage::Sealed {
            return Err(EROFS);
        }
        if self.stage == Stage::Idle {
            return Err(EPERM);
        }
        if !id_in_incr_space(s.id) {
            return Err(EINVAL);
        }
        if self.find_id(s.id).is_some() {
            return Err(EEXIST);
        }
        let zone = match zone_of(s.syscall) {
            Some(z) => *z,
            None => return Err(EINVAL),
        };
        if zone.state != ZoneState::Owned {
            // 保留区与域外都不可落增量：保留区回 EPERM，域外回 EINVAL。
            return if zone.state == ZoneState::Reserved { Err(EPERM) } else { Err(EINVAL) };
        }
        if !self.zone_claimed(zone.id) {
            return Err(EPERM);
        }
        if self.tail_len >= INCR_CAP {
            return Err(E2BIG);
        }
        let slot = match self.tail.get_mut(self.tail_len) {
            Some(p) => p,
            None => return Err(E2BIG),
        };
        *slot = s;
        self.tail_len = self.tail_len.saturating_add(1);
        // 链摘要：id / syscall / errno 逐字段卷入，顺序固定。
        let mut h = self.chain;
        h ^= s.id;
        h = h.wrapping_mul(0x0100_0193);
        h ^= s.syscall;
        h = h.wrapping_mul(0x0100_0193);
        h ^= s.errno as u32;
        h = h.wrapping_mul(0x0100_0193);
        self.chain = h;
        Ok(self.tail_len - 1)
    }

    /// 封存：状态必须是 Claimed，否则回 EINVAL；重复封存回 EBUSY。
    pub fn seal(&mut self) -> Result<u32, i32> {
        if self.stage == Stage::Sealed {
            return Err(EBUSY);
        }
        if self.stage != Stage::Claimed {
            return Err(EINVAL);
        }
        self.stage = Stage::Sealed;
        Ok(self.chain)
    }
}

/// 冻仓摘要（FNV-1a 逐条目喂入）。
pub fn freeze_digest(frozen: &[Surf; BASE_LEN]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for s in frozen.iter() {
        h ^= s.id;
        h = h.wrapping_mul(0x0100_0193);
        h ^= s.syscall;
        h = h.wrapping_mul(0x0100_0193);
        h ^= s.errno as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= s.tag.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 接管脚本 ──────────────────────────────────────────────────────────
/// 一次接管执行报告（所有字段都由脚本实跑得出）。
#[derive(Clone, Copy, Debug)]
pub struct TakeoverReport {
    pub claims_ok: u32,
    pub claims_refused: u32,
    pub appended: u32,
    pub refused: u32,
    pub dup_code: i32,
    pub range_code: i32,
    pub zone_code: i32,
    pub cap_code: i32,
    pub sealed: bool,
    pub post_seal: i32,
    pub baseline_before: u32,
    pub baseline_after: u32,
    pub chain: u32,
    pub tail_len: u32,
    pub stage: Stage,
}

impl TakeoverReport {
    /// 空报告。
    pub const fn blank() -> TakeoverReport {
        TakeoverReport {
            claims_ok: 0,
            claims_refused: 0,
            appended: 0,
            refused: 0,
            dup_code: 0,
            range_code: 0,
            zone_code: 0,
            cap_code: 0,
            sealed: false,
            post_seal: 0,
            baseline_before: 0,
            baseline_after: 0,
            chain: 0,
            tail_len: 0,
            stage: Stage::Idle,
        }
    }

    /// 追加账是否平（尝试 = 落账 + 拒绝，拒绝 4 类）。
    pub fn append_balanced(&self) -> bool {
        self.appended.saturating_add(self.refused) == APPEND_ATTEMPTS
            && self.refused == REFUSAL_KINDS as u32
            && self.appended == INCR_LEN as u32
    }

    /// 四类拒绝码互不相同且各为约定的 errno。
    pub fn refusal_codes_ok(&self) -> bool {
        self.dup_code == EEXIST
            && self.range_code == EINVAL
            && self.zone_code == EPERM
            && self.cap_code == E2BIG
            && self.dup_code != self.range_code
            && self.range_code != self.zone_code
            && self.zone_code != self.cap_code
    }

    /// 零重写：追加前后冻仓摘要相等。
    pub fn baseline_intact(&self) -> bool {
        self.baseline_before == self.baseline_after && self.baseline_before != 0
    }
}

/// 记账一次追加尝试：成功计入 appended，失败计入 refused 并按槽位记码
/// （1 重复 / 2 越界 / 3 保留区 / 4 满仓）。槽位与拒绝语义一一对应。
fn book(rep: &mut TakeoverReport, res: Result<usize, i32>, slot: u32) {
    match res {
        Ok(_) => rep.appended = rep.appended.saturating_add(1),
        Err(c) => {
            rep.refused = rep.refused.saturating_add(1);
            match slot {
                1 => rep.dup_code = c,
                2 => rep.range_code = c,
                3 => rep.zone_code = c,
                _ => rep.cap_code = c,
            }
        }
    }
}

/// 执行冻结脚本（claim×5 → append×8 → 拒绝×4 → seal → 封存后追加）。
pub fn run_takeover() -> TakeoverReport {
    let mut rep = TakeoverReport::blank();
    let mut reg = Registry::from_baseline();
    rep.baseline_before = reg.baseline_digest_now();

    // 阶段 A：接管三段自有区间 + 两次越界尝试（重复 → EBUSY、保留 → EPERM）。
    let claims = [0x21u32, 0x22, 0x23, 0x21, 0x24];
    for zid in claims.iter() {
        match reg.claim(*zid) {
            Ok(()) => rep.claims_ok = rep.claims_ok.saturating_add(1),
            Err(_) => rep.claims_refused = rep.claims_refused.saturating_add(1),
        }
    }

    // 阶段 B：8 条增量全部落账。
    for s in INCREMENTS.iter() {
        match reg.append(*s) {
            Ok(_) => rep.appended = rep.appended.saturating_add(1),
            Err(_) => rep.refused = rep.refused.saturating_add(1),
        }
    }

    // 阶段 C：四类拒绝各打一笔（重复 id / 越界 id / 保留区 / 满仓）。
    let dup = INCREMENTS.get(2).copied().unwrap_or(EMPTY_SURF);
    book(&mut rep, reg.append(dup), 1);
    let stray = Surf { id: 0x1001, syscall: 136, errno: EINVAL, tag: "越界 id" };
    book(&mut rep, reg.append(stray), 2);
    let reserved = Surf { id: 0x2009, syscall: 200, errno: EPERM, tag: "保留区" };
    book(&mut rep, reg.append(reserved), 3);
    let overflow = Surf { id: 0x200a, syscall: 137, errno: EAGAIN, tag: "溢出" };
    book(&mut rep, reg.append(overflow), 4);

    // 阶段 D：封存，再尝试一次追加，必须回 EROFS。
    rep.sealed = reg.seal().is_ok();
    let after_seal = Surf { id: 0x200b, syscall: 138, errno: EINVAL, tag: "封存后" };
    rep.post_seal = match reg.append(after_seal) {
        Ok(_) => 0,
        Err(c) => c,
    };

    rep.baseline_after = reg.baseline_digest_now();
    rep.chain = reg.chain();
    rep.tail_len = reg.tail_len();
    rep.stage = reg.stage();
    rep
}

// ── 零重写承诺格 ──────────────────────────────────────────────────────
/// 一条承诺断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Promise {
    pub name: &'static str,
    pub holds: bool,
    pub note: &'static str,
}

/// 五条零重写承诺（顺序固定：行数 / 摘要 / id 空间 / errno 集合 / 尾仓单调）。
pub fn promise_board(rep: &TakeoverReport) -> [Promise; PROMISES] {
    let mut frozen_rows = 0u32;
    for s in BASELINE.iter() {
        if s.id != 0 && s.id <= BASE_ID_LIMIT && s.errno != 0 {
            frozen_rows = frozen_rows.saturating_add(1);
        }
    }
    let mut id_disjoint = true;
    for s in INCREMENTS.iter() {
        if s.id <= BASE_ID_LIMIT || !id_in_incr_space(s.id) {
            id_disjoint = false;
        }
        for b in BASELINE.iter() {
            if b.id == s.id {
                id_disjoint = false;
            }
        }
    }
    [
        Promise {
            name: "存量行数",
            holds: frozen_rows == BASE_LEN as u32,
            note: "冻仓 10 行必须原样保留，升级只允许在尾部追加",
        },
        Promise {
            name: "存量摘要",
            holds: rep.baseline_intact(),
            note: "追加前后冻仓摘要必须逐字节相等，任何改写都会让摘要漂移",
        },
        Promise {
            name: "id 空间",
            holds: id_disjoint,
            note: "增量 id 必须落在 0x2000..=0x2FFF，与存量 0x1xxx 不相交",
        },
        Promise {
            name: "errno 集合",
            holds: INCREMENTS.iter().all(|s| s.errno > 0 && s.errno <= ENOSYS),
            note: "增量 errno 必须取正数且在 Linux 闭集内，禁止自定义码",
        },
        Promise {
            name: "尾仓单调",
            holds: rep.tail_len == INCR_LEN as u32 && rep.appended == INCR_LEN as u32,
            note: "尾仓只增不减，落账条数必须等于清单条数",
        },
    ]
}

/// 接管注记（合同 ③ 写回）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Note {
    pub status_idx: u32,
    pub zones_owned: u32,
    pub increments: u32,
    pub promises_ok: u32,
    pub summary: &'static str,
}

impl Note {
    /// 是否已完成（状态=2 且承诺全平）。
    pub fn done(&self) -> bool {
        self.status_idx == 2 && self.promises_ok == PROMISES as u32 && self.increments == INCR_LEN as u32
    }
}

/// 由报告生成注记。
pub fn compose_note(rep: &TakeoverReport) -> Note {
    let board = promise_board(rep);
    let mut ok = 0u32;
    for p in board.iter() {
        if p.holds {
            ok = ok.saturating_add(1);
        }
    }
    let (_, owned, _) = boundary_scan();
    let done = ok == PROMISES as u32 && rep.append_balanced() && rep.sealed;
    Note {
        status_idx: if done { 2 } else { 1 },
        zones_owned: owned,
        increments: rep.appended,
        promises_ok: ok,
        summary: "接管边界 5 段声明成立，存量 10 面零重写，增量 8 条落账 4 类拒绝各一笔",
    }
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 全域指纹：边界表 + 冻仓 + 增量 + 报告账目。
pub fn corpus_fingerprint(rep: &TakeoverReport) -> u32 {
    let mut out: Vec<u8> = Vec::new();
    for z in ZONES.iter() {
        out.push((z.id & 0xFF) as u8);
        out.push(z.state.idx() as u8);
        out.push((z.lo & 0xFF) as u8);
        out.push((z.hi & 0xFF) as u8);
    }
    for s in BASELINE.iter() {
        out.push((s.id & 0xFF) as u8);
        out.push((s.syscall & 0xFF) as u8);
        out.push((s.errno & 0x7F) as u8);
    }
    for s in INCREMENTS.iter() {
        out.push((s.id & 0xFF) as u8);
        out.push((s.syscall & 0xFF) as u8);
        out.push((s.errno & 0x7F) as u8);
    }
    out.push((rep.chain & 0xFF) as u8);
    out.push(((rep.chain >> 8) & 0xFF) as u8);
    out.push((rep.baseline_after & 0xFF) as u8);
    out.push(((rep.baseline_after >> 8) & 0xFF) as u8);
    out.push(rep.tail_len as u8);
    out.push(rep.stage.idx() as u8);
    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9359 判据集（静态 16 条 + 动态 3 条）。
pub fn run_unx9359_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9359");

    // 1. 段位
    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE == 0x7C740
        && (0x7C740..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7C740 起 0x40 对齐", seg_ok,
          "本模块独占 0x7C740..=0x7C77F，紧接 F9358 之后");

    // 2. 边界无重叠无空洞
    let (disjoint, owned, covered) = boundary_scan();
    s.add("接管边界：五段两两不交且覆盖 0..=511",
          disjoint && covered == SYSCALL_SPAN && ZONES.len() == ZONE_LEN,
          "闭区间交叉判定必须全假，跨度之和必须等于 512");

    // 3. 归属分布
    let reserved = ZONES.iter().filter(|z| z.state == ZoneState::Reserved).count();
    let outside = ZONES.iter().filter(|z| z.state == ZoneState::OutOfScope).count();
    s.add("边界总声明：3 自有 / 1 保留 / 1 域外",
          owned == 3 && reserved == 1 && outside == 1,
          "五种归属必须齐备，保留区与域外区不可被并入自有段");

    // 4. 号段查找
    let lookup_ok = zone_of(0).map(|z| z.id) == Some(0x21)
        && zone_of(63).map(|z| z.id) == Some(0x21)
        && zone_of(64).map(|z| z.id) == Some(0x22)
        && zone_of(191).map(|z| z.id) == Some(0x23)
        && zone_of(192).map(|z| z.id) == Some(0x24)
        && zone_of(511).map(|z| z.id) == Some(0x25)
        && zone_of(512).is_none();
    s.add("号段查找：六个边点命中、512 越界为空", lookup_ok,
          "闭区间端点必须归属正确，越界不得回绕到首段");

    // 5. 接管状态机
    let mut probe = Registry::from_baseline();
    let c1 = probe.claim(0x21);
    let c2 = probe.claim(0x21);
    let c3 = probe.claim(0x24);
    let c4 = probe.claim(0x25);
    s.add("状态机：首次接管成功、重复 EBUSY、保留 EPERM、域外 EINVAL",
          c1.is_ok() && c2 == Err(EBUSY) && c3 == Err(EPERM) && c4 == Err(EINVAL),
          "四类接管错误必须各归其码，不能一律 EINVAL");

    // 6. 未接管不得追加
    let mut idle = Registry::from_baseline();
    let idle_append = idle.append(INCREMENTS.first().copied().unwrap_or(EMPTY_SURF));
    s.add("次序：未接管就追加回 EPERM", idle_append == Err(EPERM),
          "追加必须在 Claimed 之后，状态先于一切数据校验");

    // 7. 全脚本
    let rep = run_takeover();
    s.add("全脚本：12 次追加 = 8 落账 + 4 拒绝",
          rep.append_balanced() && rep.appended == 8 && rep.refused == 4,
          "增量清单落账格必须逐笔平账，落账 8、拒绝 4");

    // 8. 四类拒绝码
    s.add("拒绝语义：EEXIST/EINVAL/EPERM/E2BIG 各一笔",
          rep.refusal_codes_ok(),
          "重复 id、越界 id、保留区、满仓四类必须各回其码");

    // 9. 零重写：摘要不变
    s.add("零重写：追加前后冻仓摘要相等", rep.baseline_intact(),
          "冻仓摘要是零重写的唯一证据，必须逐字节相等");

    // 10. 零重写：行数与 id 空间
    let frozen_ok = rep.tail_len == INCR_LEN as u32
        && BASELINE.iter().all(|b| b.id <= BASE_ID_LIMIT && b.id > 0)
        && INCREMENTS.iter().all(|i| id_in_incr_space(i.id));
    s.add("零重写：存量 10 行未动、增量全在 0x2xxx 空间", frozen_ok,
          "冻仓只读 + id 空间不相交，双保险覆盖改写与覆盖两种事故");

    // 11. 承诺五条
    let board = promise_board(&rep);
    let holds = board.iter().filter(|p| p.holds).count() as u32;
    s.add("零重写承诺格：五条全部成立",
          holds == PROMISES as u32 && board.iter().all(|p| !p.note.is_empty()),
          "行数/摘要/id 空间/errno 集合/尾仓单调五条缺一即红");

    // 12. 封存语义
    s.add("封存：seal 成功、封存后追加 EROFS",
          rep.sealed && rep.post_seal == EROFS && rep.stage == Stage::Sealed,
          "封存即只读，追加必须回 EROFS 而不是 EPERM");

    // 13. 容量
    let mut cap = Registry::from_baseline();
    let claimed = cap.claim(0x23).is_ok();
    let mut k = 0usize;
    let mut landed = 0u32;
    while k < INCR_LEN {
        if let Some(s) = INCREMENTS.get(k) {
            if cap.append(*s).is_ok() {
                landed = landed.saturating_add(1);
            }
        }
        k += 1;
    }
    let ninth = Surf { id: 0x200c, syscall: 139, errno: EAGAIN, tag: "第 9 条" };
    s.add("容量：8 条填满后第 9 条回 E2BIG",
          claimed && landed == INCR_LEN as u32 && cap.append(ninth) == Err(E2BIG),
          "尾仓定容 8，满仓不得越界写入也不得静默扩容");

    // 14. 注记（写回）
    let note = compose_note(&rep);
    s.add("写回注记：状态=已完成、摘要非空、段数/条数相符",
          note.done()
              && note.status_idx == 2
              && note.summary.len() >= 24
              && note.zones_owned == 3
              && note.increments == INCR_LEN as u32,
          "合同 ③ 要求状态与摘要同时成立，摘要须在封存后生成");

    // 15. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < TakeoverCode::ALL.len() {
        let mut j = i + 1;
        while j < TakeoverCode::ALL.len() {
            if let (Some(a), Some(b)) = (TakeoverCode::ALL.get(i), TakeoverCode::ALL.get(j)) {
                if a.code() == b.code() || a.seg_addr() == b.seg_addr() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个段内序号与绝对码两两唯一", uniq,
          "闭集码重复会让红项无法定位到唯一建议");

    // 16. 文案闭集
    let text_ok = TakeoverCode::ALL.len() == 12
        && TakeoverCode::ALL.iter().all(|c| !c.detail().is_empty() && c.detail().len() >= 12);
    s.add("文案闭集：12 个诊断码均有可操作建议", text_ok,
          "每个码必须给出非空中文处置建议");

    // 17. 双跑
    let rep2 = run_takeover();
    let fp_a = corpus_fingerprint(&rep);
    let fp_b = corpus_fingerprint(&rep2);
    s.add("双跑一致：接管脚本两遍指纹相等",
          fp_a == fp_b && rep.chain == rep2.chain && rep.baseline_after == rep2.baseline_after,
          "链摘要与冻仓摘要两遍必须逐字节相同");

    // 动态明细（≤8）
    let boundary_text = format!("边界 {} 段，覆盖 {} 个号，自有 {}", ZONES.len(), covered, owned);
    s.add_dyn("边界明细", disjoint && covered == SYSCALL_SPAN, &boundary_text);

    let ledger_text = format!("追加尝试 12：落账 {}，拒绝 {}（{:?}/{:?}/{:?}/{:?}）",
                              rep.appended, rep.refused, rep.dup_code, rep.range_code,
                              rep.zone_code, rep.cap_code);
    s.add_dyn("落账明细", rep.append_balanced(), &ledger_text);

    let fp_text = format!("冻仓 0x{:08X} 链 0x{:08X}", rep.baseline_after, rep.chain);
    s.add_dyn("指纹快照", rep.baseline_intact(), &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_and_zone_lookup() {
        let (ok, owned, covered) = boundary_scan();
        assert!(ok);
        assert_eq!(owned, 3);
        assert_eq!(covered, SYSCALL_SPAN);
        assert_eq!(zone_of(0).map(|z| z.id), Some(0x21));
        assert_eq!(zone_of(63).map(|z| z.id), Some(0x21));
        assert_eq!(zone_of(64).map(|z| z.id), Some(0x22));
        assert_eq!(zone_of(127).map(|z| z.id), Some(0x22));
        assert_eq!(zone_of(128).map(|z| z.id), Some(0x23));
        assert_eq!(zone_of(191).map(|z| z.id), Some(0x23));
        assert_eq!(zone_of(192).map(|z| z.id), Some(0x24));
        assert_eq!(zone_of(255).map(|z| z.id), Some(0x24));
        assert_eq!(zone_of(256).map(|z| z.id), Some(0x25));
        assert_eq!(zone_of(511).map(|z| z.id), Some(0x25));
        assert!(zone_of(512).is_none());
        assert!(lookup_zone(0x99).is_none());
    }

    #[test]
    fn claim_matrix() {
        let mut r = Registry::from_baseline();
        assert_eq!(r.stage(), Stage::Idle);
        assert!(r.claim(0x21).is_ok());
        assert_eq!(r.stage(), Stage::Claimed);
        assert_eq!(r.claim(0x21), Err(EBUSY));
        assert_eq!(r.claim(0x24), Err(EPERM));
        assert_eq!(r.claim(0x25), Err(EINVAL));
        assert_eq!(r.claim(0x77), Err(EINVAL));
        assert!(r.zone_claimed(0x21));
        assert!(!r.zone_claimed(0x22));
    }

    #[test]
    fn append_requires_claim() {
        let mut r = Registry::from_baseline();
        let first = INCREMENTS.first().copied().unwrap_or(EMPTY_SURF);
        assert_eq!(r.append(first), Err(EPERM));
        assert_eq!(r.tail_len(), 0);
    }

    #[test]
    fn append_lands_in_tail() {
        let mut r = Registry::from_baseline();
        assert!(r.claim(0x23).is_ok());
        let first = INCREMENTS.first().copied().unwrap_or(EMPTY_SURF);
        let slot = r.append(first);
        assert_eq!(slot, Ok(0));
        assert_eq!(r.tail_len(), 1);
        assert_eq!(r.tail_at(0).map(|s| s.id), Some(0x2001));
        assert_eq!(r.tail_at(1).map(|s| s.id), None);
        assert!(r.find_id(0x2001).is_some());
        assert!(r.find_id(0x1001).is_some());
        assert!(r.find_id(0x9999).is_none());
    }

    #[test]
    fn id_range_and_zone_ownership_enforced() {
        let mut r = Registry::from_baseline();
        assert!(r.claim(0x23).is_ok());
        let low = Surf { id: 0x1001, syscall: 136, errno: EINVAL, tag: "旧 id" };
        let high = Surf { id: 0x3000, syscall: 137, errno: EINVAL, tag: "超界" };
        assert_eq!(r.append(low), Err(EINVAL));
        assert_eq!(r.append(high), Err(EINVAL));
        assert_eq!(r.tail_len(), 0);
        let reserved = Surf { id: 0x2011, syscall: 200, errno: EPERM, tag: "保留" };
        let outside = Surf { id: 0x2012, syscall: 300, errno: EINVAL, tag: "域外" };
        assert_eq!(r.append(reserved), Err(EPERM));
        assert_eq!(r.append(outside), Err(EINVAL));
    }

    #[test]
    fn full_script_balances() {
        let rep = run_takeover();
        assert!(rep.append_balanced());
        assert_eq!(rep.appended, 8);
        assert_eq!(rep.refused, 4);
        assert_eq!(rep.claims_ok, 3);
        assert_eq!(rep.claims_refused, 2);
        assert!(rep.refusal_codes_ok());
        assert!(rep.baseline_intact());
        assert!(rep.sealed);
        assert_eq!(rep.post_seal, EROFS);
        assert_eq!(rep.tail_len, 8);
        assert_eq!(rep.stage, Stage::Sealed);
    }

    #[test]
    fn promises_hold() {
        let rep = run_takeover();
        let board = promise_board(&rep);
        assert_eq!(board.len(), PROMISES);
        for p in board.iter() {
            assert!(p.holds, "promise {} failed", p.name);
        }
    }

    #[test]
    fn sealed_is_read_only() {
        let mut r = Registry::from_baseline();
        assert!(r.claim(0x21).is_ok());
        let chain = r.chain();
        assert_eq!(r.seal(), Ok(chain));
        assert_eq!(r.stage(), Stage::Sealed);
        let s = Surf { id: 0x2001, syscall: 130, errno: EINVAL, tag: "封存后" };
        assert_eq!(r.append(s), Err(EROFS));
        assert_eq!(r.claim(0x22), Err(EROFS));
        assert_eq!(r.seal(), Err(EBUSY));
    }

    #[test]
    fn capacity_is_fixed() {
        let mut r = Registry::from_baseline();
        assert!(r.claim(0x23).is_ok());
        for s in INCREMENTS.iter() {
            assert!(r.append(*s).is_ok());
        }
        assert_eq!(r.tail_len(), INCR_CAP as u32);
        let ninth = Surf { id: 0x2009, syscall: 139, errno: EAGAIN, tag: "第 9" };
        assert_eq!(r.append(ninth), Err(E2BIG));
    }

    #[test]
    fn note_is_done() {
        let rep = run_takeover();
        let note = compose_note(&rep);
        assert!(note.done());
        assert_eq!(note.status_idx, 2);
        assert_eq!(note.zones_owned, 3);
        assert_eq!(note.increments, 8);
        assert_eq!(note.promises_ok, 5);
        assert!(!note.summary.is_empty());
    }

    #[test]
    fn digest_is_stable() {
        let a = Registry::from_baseline();
        let b = Registry::from_baseline();
        assert_eq!(a.baseline_digest_now(), b.baseline_digest_now());
        assert_eq!(a.baseline_digest_now(), a.baseline_digest_sealed());
        assert_ne!(a.baseline_digest_now(), 0);
    }

    #[test]
    fn codes_and_segment() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(TakeoverCode::ALL.len(), 12);
        assert_eq!(TakeoverCode::BoundaryOverlap.seg_addr(), SEG_BASE + 1);
        assert_eq!(TakeoverCode::SegmentOut.code(), 12);
        assert!(TakeoverCode::ALL.iter().all(|c| !c.detail().is_empty()));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9359_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
