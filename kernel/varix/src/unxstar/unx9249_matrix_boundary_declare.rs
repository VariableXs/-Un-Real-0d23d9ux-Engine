//! UNX-F9249 mlock/mprotect 权限边界矩阵（与 B24 分界声明） · 0x7ABC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9249 逐字）：
//!   UNX-F9249-J1 锁页越限全 EPERM 判据过，与 B24 权限矩阵分界声明格过，
//!   零重复判据格过。
//!
//! 设计要点：
//!   ① 锁页预算按 RLIMIT_MEMLOCK 建模：8 页语料区、预算 4 页。mlock 先按页
//!      取整（起点下取整、末端上取整），再算「已锁 ∪ 本次」的总页数；超过
//!      4 页一律 EPERM，且状态一位都不改——越限是权限面，不是部分成功。
//!   ② 判定次序：区间越界 → 未映射 → 预算。未映射页即使同时越限也必须先回
//!      ENOMEM，不能让权限错误掩盖地址错误。
//!   ③ mprotect 是位面校验：prot 只接受 PROT_NONE/READ/WRITE/EXEC 的按位或，
//!      任何表外位（如 PROT_GROWSDOWN 的 0x01000000）回 EINVAL；起点必须页
//!      对齐，末端上取整；未映射回 ENOMEM。
//!   ④ 访问判定与保护位联动：PROT_NONE 上读写、无 EXEC 位上取指都回 EPERM，
//!      与锁页越限共用同一权限错误面；mprotect 提权后同一访问立即放行。
//!   ⑤ 与 B24 分界：B24 拥有 fd/文件权限位、进程状态权限与能力门，F9249 只
//!      拥有页级锁页预算、mprotect 位面与页访问判定；surface 两两互斥。
//!   ⑥ 诊断码段内闭集 12 个（+0x01..=+0x0C），码值唯一，每码带中文建议。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9249 独占 0x7ABC0..=0x7ABFF）。
pub const SEG_BASE: u32 = 0x7ABC0;
/// 4K 页大小。
pub const PAGE: u64 = 4096;
/// 语料区间基址（8 页）。
pub const REGION_BASE: u64 = 0x0000_0000_5000_0000;
/// 语料区间页数。
pub const REGION_PAGES: usize = 8;
/// 语料区间上界（不含）。
pub const REGION_END: u64 = REGION_BASE + PAGE * REGION_PAGES as u64;
/// RLIMIT_MEMLOCK 预算（页）。
pub const MEMLOCK_LIMIT_PAGES: u32 = 4;
/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 越限注入类别数。
pub const OVER_CLASSES: u32 = 4;

/// PROT_NONE。
pub const PROT_NONE: u8 = 0;
/// PROT_READ。
pub const PROT_READ: u8 = 1;
/// PROT_WRITE。
pub const PROT_WRITE: u8 = 2;
/// PROT_EXEC。
pub const PROT_EXEC: u8 = 4;
/// 合法 prot 位掩码闭集。
pub const PROT_MASK: u8 = 7;

/// Linux errno 数值（正数口径）。
pub const EPERM: i32 = 1;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemPermCode {
    LockOverLimit,
    UnmappedRange,
    UnalignedStart,
    BadProtBits,
    AccessDenied,
    ExecDenied,
    LockUnderflow,
    BoundaryOverlap,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
    CapacityExceeded,
}

impl MemPermCode {
    /// 码值（段基址 + 1 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MemPermCode::LockOverLimit => "锁页越限被拒：已锁与本次请求合计超过 RLIMIT_MEMLOCK 4 页；请先 munlock 或提高限额，回 EPERM",
            MemPermCode::UnmappedRange => "区间未映射被拒：范围内至少一页不在映射表；请核对地址后再锁页或改保护，回 ENOMEM",
            MemPermCode::UnalignedStart => "起点未对齐被拒：mprotect 要求 start 按 4K 页对齐；请先 page_align_down 再提交",
            MemPermCode::BadProtBits => "非法保护位被拒：prot 只接受 NONE/READ/WRITE/EXEC 的按位或；请清除 0x8 及以上的表外位",
            MemPermCode::AccessDenied => "页访问被拒：目标页处于 PROT_NONE 或缺少所需权限位；请先 mprotect 提权",
            MemPermCode::ExecDenied => "取指被拒：目标页没有 PROT_EXEC；请补 EXEC 位后再执行",
            MemPermCode::LockUnderflow => "解锁下溢被拒：munlock 试图清空未锁页；请按锁定记录成对解锁",
            MemPermCode::BoundaryOverlap => "B24 分界重复：同一权限面被两个域认领；请把重复项归还唯一属主",
            MemPermCode::LedgerRegress => "锁页账倒退被拒：计数只增不减；请检查回滚路径是否误改累计值",
            MemPermCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            MemPermCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
            MemPermCode::CapacityExceeded => "语料区容量不足：页下标越过 8 页窗口；请缩小范围或扩充语料区",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: [MemPermCode; 12] = [
        MemPermCode::LockOverLimit,
        MemPermCode::UnmappedRange,
        MemPermCode::UnalignedStart,
        MemPermCode::BadProtBits,
        MemPermCode::AccessDenied,
        MemPermCode::ExecDenied,
        MemPermCode::LockUnderflow,
        MemPermCode::BoundaryOverlap,
        MemPermCode::LedgerRegress,
        MemPermCode::NotReproducible,
        MemPermCode::SegmentViolation,
        MemPermCode::CapacityExceeded,
    ];
}

// ── 页权限模型 ────────────────────────────────────────────────────────
/// 访问类型（与 prot 位对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Read,
    Write,
    Exec,
}

impl Access {
    /// 对应的 prot 位。
    pub fn bit(self) -> u8 {
        match self {
            Access::Read => PROT_READ,
            Access::Write => PROT_WRITE,
            Access::Exec => PROT_EXEC,
        }
    }
}

/// 定容页权限表：映射位 + 锁定位 + 逐页 prot + 四组单调计数。
#[derive(Clone, Copy, Debug)]
pub struct MemSpace {
    mapped: u8,
    locked: u8,
    prot: [u8; REGION_PAGES],
    lock_events: u32,
    unlock_events: u32,
    denied: u32,
}

impl MemSpace {
    /// 全部映射、读写保护、零锁定。
    pub const fn new() -> MemSpace {
        MemSpace {
            mapped: 0xFF,
            locked: 0,
            prot: [PROT_READ | PROT_WRITE; REGION_PAGES],
            lock_events: 0,
            unlock_events: 0,
            denied: 0,
        }
    }

    pub fn mapped_mask(&self) -> u8 {
        self.mapped
    }
    pub fn locked_mask(&self) -> u8 {
        self.locked
    }
    pub fn lock_events(&self) -> u32 {
        self.lock_events
    }
    pub fn unlock_events(&self) -> u32 {
        self.unlock_events
    }
    pub fn denied(&self) -> u32 {
        self.denied
    }
    /// 当前预算占用页数。
    pub fn budget_used(&self) -> u32 {
        self.locked.count_ones()
    }
    /// 某页保护位。
    pub fn prot_at(&self, idx: usize) -> Option<u8> {
        self.prot.get(idx).copied()
    }
    /// 地址 → 页下标（必须页对齐）。
    pub fn page_of(&self, addr: u64) -> Option<usize> {
        if addr < REGION_BASE || addr >= REGION_END || addr % PAGE != 0 {
            return None;
        }
        Some(((addr - REGION_BASE) / PAGE) as usize)
    }

    /// 页区间解析：`round_start` 决定起点是否下取整；末端总是上取整。
    /// 返回 (起始页下标, 页数, 位掩码)。
    fn span(&self, start: u64, len: u64, round_start: bool) -> Result<(usize, usize, u8), i32> {
        if !round_start && start % PAGE != 0 {
            return Err(EINVAL);
        }
        let mask = PAGE - 1;
        let aligned_start = if round_start {
            start & !mask
        } else {
            start
        };
        let raw_end = match start.checked_add(len) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        let end = match raw_end.checked_add(mask) {
            Some(v) => v & !mask,
            None => return Err(ENOMEM),
        };
        if aligned_start < REGION_BASE || end > REGION_END {
            return Err(ENOMEM);
        }
        let first = ((aligned_start - REGION_BASE) / PAGE) as usize;
        let count = ((end - aligned_start) / PAGE) as usize;
        if count == 0 {
            return Ok((first, 0, 0));
        }
        let mut bits = 0u8;
        let mut i = 0usize;
        while i < count {
            let idx = match first.checked_add(i) {
                Some(v) => v,
                None => return Err(ENOMEM),
            };
            let bit = match 1u8.checked_shl(idx as u32) {
                Some(v) => v,
                None => return Err(ENOMEM),
            };
            bits |= bit;
            i += 1;
        }
        Ok((first, count, bits))
    }

    /// mlock：按页取整并检查预算。成功返回本次新增锁定页数。
    pub fn mlock(&mut self, start: u64, len: u64) -> Result<u32, i32> {
        if len == 0 {
            return Ok(0);
        }
        let (_first, _count, bits) = self.span(start, len, true)?;
        if bits & !self.mapped != 0 {
            return Err(ENOMEM);
        }
        let merged = self.locked | bits;
        if merged.count_ones() > MEMLOCK_LIMIT_PAGES {
            return Err(EPERM);
        }
        let added = (bits & !self.locked).count_ones();
        self.locked |= bits;
        self.lock_events = self.lock_events.saturating_add(added);
        Ok(added)
    }

    /// munlock：成对解锁；未映射回 ENOMEM。
    pub fn munlock(&mut self, start: u64, len: u64) -> Result<u32, i32> {
        if len == 0 {
            return Ok(0);
        }
        let (_first, _count, bits) = self.span(start, len, true)?;
        if bits & !self.mapped != 0 {
            return Err(ENOMEM);
        }
        let removed = (bits & self.locked).count_ones();
        self.locked &= !bits;
        self.unlock_events = self.unlock_events.saturating_add(removed);
        Ok(removed)
    }

    /// mprotect：起点必须对齐，prot 位面闭集校验。
    pub fn mprotect(&mut self, start: u64, len: u64, prot: u8) -> Result<u32, i32> {
        if start % PAGE != 0 {
            return Err(EINVAL);
        }
        if prot & !PROT_MASK != 0 {
            return Err(EINVAL);
        }
        if len == 0 {
            return Ok(0);
        }
        let (_first, count, bits) = self.span(start, len, false)?;
        if bits & !self.mapped != 0 {
            return Err(ENOMEM);
        }
        let mut i = 0usize;
        while i < REGION_PAGES {
            if bits & (1u8 << i) != 0 {
                if let Some(slot) = self.prot.get_mut(i) {
                    *slot = prot;
                }
            }
            i += 1;
        }
        Ok(count as u32)
    }

    /// 页访问判定：未映射 ENOMEM，权限不足 EPERM。
    pub fn access(&mut self, idx: usize, kind: Access) -> Result<(), i32> {
        let bit = match 1u8.checked_shl(idx as u32) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        if self.mapped & bit == 0 {
            return Err(ENOMEM);
        }
        let prot = match self.prot.get(idx) {
            Some(v) => *v,
            None => return Err(ENOMEM),
        };
        if prot & kind.bit() == 0 {
            self.denied = self.denied.saturating_add(1);
            return Err(EPERM);
        }
        Ok(())
    }

    /// 摘除一页映射（制造页洞的测试原语）。
    pub fn unmap_page(&mut self, idx: usize) -> Result<(), i32> {
        let bit = match 1u8.checked_shl(idx as u32) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        if self.mapped & bit == 0 {
            return Err(EINVAL);
        }
        self.mapped &= !bit;
        self.locked &= !bit;
        Ok(())
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 越限注入四类：请求 5/6/8 页，以及先锁 2 页再追加 3 页。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverCase {
    Five,
    Six,
    Eight,
    SplitTwoPlusThree,
}

impl OverCase {
    /// 闭集全量。
    pub const ALL: [OverCase; 4] = [
        OverCase::Five,
        OverCase::Six,
        OverCase::Eight,
        OverCase::SplitTwoPlusThree,
    ];
}

/// 越限注入：每类 10 轮，全部 EPERM 且锁定状态不变。
pub fn run_over_limit_matrix() -> ([u32; 4], InjectLedger) {
    let mut sp = MemSpace::new();
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < OverCase::ALL.len() {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            if ci == 3 {
                let _ = sp.mlock(REGION_BASE, PAGE * 2);
            }
            let before = sp.locked_mask();
            let got = match ci {
                0 => sp.mlock(REGION_BASE, PAGE * 5),
                1 => sp.mlock(REGION_BASE, PAGE * 6),
                2 => sp.mlock(REGION_BASE, PAGE * 8),
                _ => sp.mlock(REGION_BASE + PAGE * 2, PAGE * 3),
            };
            let caught = got == Err(EPERM) && sp.locked_mask() == before;
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (per_class, ledger)
}

// ── 与 B24 的分界声明 ────────────────────────────────────────────────
/// 一条分界声明：属主 + 独占面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryClaim {
    pub owner: &'static str,
    pub surface: &'static str,
}

/// 冻结声明表：B24 管非页权限，F9249 只管页级权限。
pub const BOUNDARY: &[BoundaryClaim] = &[
    BoundaryClaim { owner: "B24", surface: "fd-file-perm-bits" },
    BoundaryClaim { owner: "B24", surface: "proc-status-perm" },
    BoundaryClaim { owner: "B24", surface: "capability-gate" },
    BoundaryClaim { owner: "F9249", surface: "page-mlock-budget" },
    BoundaryClaim { owner: "F9249", surface: "mprotect-bit-set" },
    BoundaryClaim { owner: "F9249", surface: "page-access-check" },
];

/// 分界表零重复：surface 两两不同，属主只能是 B24/F9249。
pub fn boundary_disjoint() -> bool {
    let mut i = 0usize;
    while i < BOUNDARY.len() {
        if let Some(a) = BOUNDARY.get(i) {
            if a.owner != "B24" && a.owner != "F9249" {
                return false;
            }
            if a.surface.is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < BOUNDARY.len() {
                if let Some(b) = BOUNDARY.get(j) {
                    if a.surface == b.surface {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

/// 统计某属主的声明条数。
pub fn claims_owned_by(owner: &str) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BOUNDARY.len() {
        if let Some(c) = BOUNDARY.get(i) {
            if c.owner == owner {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；权限族"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；内存不足族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；访问拒绝族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
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

/// 编码一次页操作结果。
fn encode(out: &mut Vec<u8>, r: Result<u32, i32>) {
    match r {
        Ok(v) => {
            out.push(1);
            out.push((v & 0xFF) as u8);
        }
        Err(e) => {
            out.push(2);
            out.push((e & 0x7F) as u8);
        }
    }
}

/// 规范语料双跑指纹：锁 4 页、越限、解锁、改保护、访问拒绝与提权。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut sp = MemSpace::new();
    encode(&mut bytes, sp.mlock(REGION_BASE, PAGE * 4));
    encode(&mut bytes, sp.mlock(REGION_BASE + PAGE * 4, PAGE));
    encode(&mut bytes, sp.munlock(REGION_BASE, PAGE * 2));
    encode(&mut bytes, sp.mlock(REGION_BASE + PAGE * 4, PAGE * 2));
    encode(&mut bytes, sp.mprotect(REGION_BASE, PAGE, PROT_READ));
    encode(&mut bytes, sp.mprotect(REGION_BASE + 1, PAGE, PROT_READ));
    encode(&mut bytes, sp.mprotect(REGION_BASE, PAGE, 0x0F));
    encode(&mut bytes, sp.mprotect(REGION_BASE, PAGE, PROT_READ | PROT_EXEC));
    let mut acc = 0u8;
    let mut i = 0usize;
    while i < REGION_PAGES {
        let r = sp.access(i, Access::Read);
        if r == Err(EPERM) {
            acc = acc.saturating_add(1);
        }
        i += 1;
    }
    bytes.push(3);
    bytes.push(acc);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9249 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9249_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9249");

    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7A000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE > 0x7AB80;
    s.add("段位守卫：0x40 对齐且不与 F9248 重叠", seg_ok,
          "段基址唯一且落在本批窗口，段间不得互相覆盖");

    let mut sp = MemSpace::new();
    let exact = sp.mlock(REGION_BASE, PAGE * 4);
    s.add("正常流：恰好锁满 4 页预算", exact == Ok(4) && sp.budget_used() == 4,
          "预算刚好用满必须成功，不得多报一页 EPERM");

    let (per_class, ledger) = run_over_limit_matrix();
    s.add("注入：锁页越限 4 类各 10/10 全 EPERM",
          per_class.iter().all(|n| *n == INJECT_ROUNDS),
          "5/6/8 页与 2+3 分段越限必须全部回 EPERM");
    s.add("注入账：越限面 40 次全检出且逃逸为 0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == OVER_CLASSES * INJECT_ROUNDS,
          "40 次注入必须 40 次检出，状态不得被部分修改");

    let mut budget = MemSpace::new();
    let _ = budget.mlock(REGION_BASE, PAGE * 4);
    let deny5 = budget.mlock(REGION_BASE + PAGE * 4, PAGE);
    let budget_ok = deny5 == Err(EPERM)
        && budget.budget_used() == 4
        && budget.locked_mask() == 0x0F;
    s.add("容量：预算满后再锁一页 EPERM 且位图不变", budget_ok,
          "越限检查必须在写位图之前完成，位图保持原值");

    let mut release = MemSpace::new();
    let _ = release.mlock(REGION_BASE, PAGE * 4);
    let un = release.munlock(REGION_BASE, PAGE * 2);
    let again = release.mlock(REGION_BASE + PAGE * 4, PAGE * 2);
    s.add("状态机：解锁 2 页后可再锁 2 页",
          un == Ok(2) && again == Ok(2) && release.budget_used() == 4
              && release.lock_events() == 6 && release.unlock_events() == 2,
          "锁/解锁事件计数必须单调，预算随位图实时复算");

    let mut holes = MemSpace::new();
    let _ = holes.unmap_page(6);
    let hole_ok = holes.mlock(REGION_BASE, PAGE * 8) == Err(ENOMEM)
        && holes.mprotect(REGION_BASE, PAGE * 8, PROT_READ) == Err(ENOMEM);
    s.add("边界：含未映射页的整段锁页/改保护回 ENOMEM", hole_ok,
          "未映射检查先于预算与位面写入，整段不得部分生效");

    let mut ro = MemSpace::new();
    let mp = ro.mprotect(REGION_BASE, PAGE, PROT_READ);
    let ro_ok = mp == Ok(1)
        && ro.access(0, Access::Read) == Ok(())
        && ro.access(0, Access::Write) == Err(EPERM)
        && ro.denied() == 1;
    s.add("权限面：只读页写访问 EPERM", ro_ok,
          "mprotect 降权后写访问必须被拒，读访问保持放行");

    let mut nx = MemSpace::new();
    let _ = nx.mprotect(REGION_BASE, PAGE, PROT_READ);
    let nx_denied = nx.access(0, Access::Exec) == Err(EPERM);
    let _ = nx.mprotect(REGION_BASE, PAGE, PROT_READ | PROT_EXEC);
    let nx_ok = nx_denied && nx.access(0, Access::Exec) == Ok(());
    s.add("权限面：无 EXEC 取指 EPERM、补位后放行", nx_ok,
          "EXEC 位必须独立校验，提权后同一访问立即通过");

    let mut none = MemSpace::new();
    let _ = none.mprotect(REGION_BASE, PAGE, PROT_NONE);
    s.add("权限面：PROT_NONE 读写均 EPERM",
          none.access(0, Access::Read) == Err(EPERM)
              && none.access(0, Access::Write) == Err(EPERM),
          "零保护位是拒绝面，不是默认放行");

    let mut meta = MemSpace::new();
    let meta_mp = meta.mprotect(REGION_BASE + PAGE, PAGE * 2, PROT_READ | PROT_EXEC);
    let meta_ok = meta_mp == Ok(2)
        && meta.prot_at(1) == Some(PROT_READ | PROT_EXEC)
        && meta.prot_at(3) == Some(PROT_READ | PROT_WRITE)
        && meta.page_of(REGION_BASE + PAGE) == Some(1)
        && meta.page_of(REGION_BASE + 1).is_none()
        && meta.mapped_mask() == 0xFF;
    s.add("位面：只改目标页且地址解析可复算", meta_ok,
          "mprotect 不得波及区间外页，未对齐地址不产生页下标");

    let mut param = MemSpace::new();
    let param_ok = param.mprotect(REGION_BASE + 1, PAGE, PROT_READ) == Err(EINVAL)
        && param.mprotect(REGION_BASE, PAGE, 0x0F) == Err(EINVAL)
        && param.mprotect(REGION_BASE, PAGE, 0x80) == Err(EINVAL);
    s.add("参数面：未对齐起点与表外保护位全 EINVAL", param_ok,
          "对齐与位面闭集校验必须在任何状态写入之前");

    let mut order = MemSpace::new();
    let _ = order.unmap_page(4);
    let order_ok = order.mlock(REGION_BASE, PAGE * 8) == Err(ENOMEM)
        && order.mlock(REGION_BASE, PAGE * 5) == Err(ENOMEM);
    s.add("判定次序：未映射 ENOMEM 先于越限 EPERM", order_ok,
          "地址面错误必须先于权限面错误返回");

    let bound_ok = boundary_disjoint()
        && claims_owned_by("B24") == 3
        && claims_owned_by("F9249") == 3
        && claims_owned_by("B24") + claims_owned_by("F9249") == BOUNDARY.len() as u32;
    s.add("对账：B24 分界声明零重复且属主完整", bound_ok,
          "每个 surface 只能有一个属主，两域条数之和等于总表");

    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOMEM)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(EINVAL)
        && linux_neg(EPERM) == -EPERM
        && !agrees_with_linux(0);
    s.add("Linux 逐码一致：EPERM/ENOMEM/EINVAL 表内可复算", codes_ok,
          "四码必须都在 errno 表内，表外码一律拒绝");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    s.add("文案闭集：12 个诊断码均有中文建议",
          MemPermCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut i = 0usize;
    while i < MemPermCode::ALL.len() {
        let mut j = i + 1;
        while j < MemPermCode::ALL.len() {
            if let (Some(a), Some(b)) = (MemPermCode::ALL.get(i), MemPermCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    let budget_text = format!(
        "越限注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("越限账目", ledger.conserved(), &budget_text);
    let lock_text = format!(
        "锁事件 {} / 解锁事件 {} / 拒绝 {}",
        release.lock_events(), release.unlock_events(), release.denied()
    );
    s.add_dyn("锁页快照", release.budget_used() == 4, &lock_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_exact_budget() {
        let mut sp = MemSpace::new();
        assert_eq!(sp.mlock(REGION_BASE, PAGE * 4), Ok(4));
        assert_eq!(sp.budget_used(), 4);
        assert_eq!(sp.locked_mask(), 0x0F);
    }

    #[test]
    fn over_limit_eperm_keeps_state() {
        let mut sp = MemSpace::new();
        assert_eq!(sp.mlock(REGION_BASE, PAGE * 5), Err(EPERM));
        assert_eq!(sp.locked_mask(), 0);
        assert_eq!(sp.lock_events(), 0);
    }

    #[test]
    fn unlock_restores_budget() {
        let mut sp = MemSpace::new();
        assert_eq!(sp.mlock(REGION_BASE, PAGE * 4), Ok(4));
        assert_eq!(sp.munlock(REGION_BASE, PAGE * 2), Ok(2));
        assert_eq!(sp.mlock(REGION_BASE + PAGE * 4, PAGE * 2), Ok(2));
        assert_eq!(sp.budget_used(), 4);
    }

    #[test]
    fn mprotect_gates_access() {
        let mut sp = MemSpace::new();
        assert_eq!(sp.mprotect(REGION_BASE, PAGE, PROT_READ), Ok(1));
        assert_eq!(sp.access(0, Access::Read), Ok(()));
        assert_eq!(sp.access(0, Access::Write), Err(EPERM));
    }

    #[test]
    fn exec_bit_independent() {
        let mut sp = MemSpace::new();
        let _ = sp.mprotect(REGION_BASE, PAGE, PROT_READ);
        assert_eq!(sp.access(0, Access::Exec), Err(EPERM));
        let _ = sp.mprotect(REGION_BASE, PAGE, PROT_READ | PROT_EXEC);
        assert_eq!(sp.access(0, Access::Exec), Ok(()));
    }

    #[test]
    fn unaligned_and_bad_bits() {
        let mut sp = MemSpace::new();
        assert_eq!(sp.mprotect(REGION_BASE + 1, PAGE, PROT_READ), Err(EINVAL));
        assert_eq!(sp.mprotect(REGION_BASE, PAGE, 0x0F), Err(EINVAL));
        assert_eq!(sp.mprotect(REGION_BASE, PAGE, 0x80), Err(EINVAL));
    }

    #[test]
    fn unmapped_range_enomem() {
        let mut sp = MemSpace::new();
        assert_eq!(sp.unmap_page(6), Ok(()));
        assert_eq!(sp.mlock(REGION_BASE, PAGE * 8), Err(ENOMEM));
        assert_eq!(sp.mprotect(REGION_BASE, PAGE * 8, PROT_READ), Err(ENOMEM));
    }

    #[test]
    fn prot_none_denies_all() {
        let mut sp = MemSpace::new();
        assert_eq!(sp.mprotect(REGION_BASE, PAGE, PROT_NONE), Ok(1));
        assert_eq!(sp.access(0, Access::Read), Err(EPERM));
        assert_eq!(sp.access(0, Access::Write), Err(EPERM));
    }

    #[test]
    fn over_matrix_conserved() {
        let (per, l) = run_over_limit_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 40);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn boundary_zero_overlap() {
        assert!(boundary_disjoint());
        assert_eq!(claims_owned_by("B24"), 3);
        assert_eq!(claims_owned_by("F9249"), 3);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9249_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
