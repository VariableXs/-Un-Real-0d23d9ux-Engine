//! UNX-F9247 mremap 错误矩阵（EINVAL/ENOMEM/EPERM 联 B17） · 0x7AB40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9247 逐字）：
//!   UNX-F9247-J1 三错误码注入矩阵判据过，非对齐 old_addr 全 EINVAL 格
//!   过，与 B17 矩阵零重复分层声明格过。
//!
//! 设计要点：
//!   ① 判定次序冻结：非对齐 → 零尺寸 → 非法 flags → FIXED 缺 MAYMOVE →
//!      区间未映射 → 锁页搬移 → 容量。前四步 EINVAL，第五步 EFAULT，第六步
//!      EPERM，第七步 ENOMEM。次序是判据本体：非对齐地址即使在未映射区间也
//!      必须先报 EINVAL，不得被 EFAULT 抢先。
//!   ② 非对齐 old_addr 是十格注入面：10 个互不相同的页内偏移 × 每格 10 轮，
//!      共 100 次，全部必须 EINVAL；任何一格逃逸即矩阵红。
//!   ③ 三错误码各自独立注入：EINVAL 走参数面、ENOMEM 走容量/溢出面、EPERM
//!      走「锁页 + MAYMOVE」权限面，账本按类分桶，互不串账。
//!   ④ 搬移状态机：原地扩缩优先；原地放不下时只有 MAYMOVE 才搬到空洞，且
//!      旧区间必须先摘除再落位，搬移前后区间计数守恒，失败不留半搬移残影。
//!   ⑤ 与 B17 分层声明：B17 拥有 mmap/munmap 基本映射与容量账，F9247 只拥有
//!      mremap 三错误码面、非对齐格与搬移返回地址面；surface 两两互斥。
//!   ⑥ 诊断码段内闭集 13 个（+0x01..=+0x0D），码值唯一，每码带中文建议。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9247 独占 0x7AB40..=0x7AB7F，紧邻 F9246 之后）。
pub const SEG_BASE: u32 = 0x7AB40;
/// 4K 页大小。
pub const PAGE: u64 = 4096;
/// 映射表容量（定容 8 槽）。
pub const MAX_REGIONS: usize = 8;
/// 竞技场基址（64 页，页对齐）。
pub const ARENA_BASE: u64 = 0x0000_0000_3000_0000;
/// 竞技场页数。
pub const ARENA_PAGES: u64 = 64;
/// 竞技场上界（不含）。
pub const ARENA_END: u64 = ARENA_BASE + PAGE * ARENA_PAGES;
/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;
/// 非对齐 old_addr 的注入格数。
pub const UNALIGNED_CELLS: usize = 10;
/// 三错误码类别数。
pub const ERROR_CLASSES: u32 = 3;
/// MREMAP_MAYMOVE：允许搬移。
pub const MAYMOVE: u32 = 1;
/// MREMAP_FIXED：要求指定落点（必须与 MAYMOVE 同用）。
pub const FIXED: u32 = 2;
/// Linux errno 数值（正数口径）。
pub const EPERM: i32 = 1;
pub const ENOMEM: i32 = 12;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0D 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MremapCode {
    UnalignedOldAddr,
    ZeroSize,
    BadFlags,
    FixedNoMove,
    NotMapped,
    NoSpace,
    LockedMove,
    LayeringOverlap,
    CountMismatch,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
    CapacityFull,
}

impl MremapCode {
    /// 码值（段基址 + 1 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MremapCode::UnalignedOldAddr => "old_addr 未对齐被拒：mremap 要求按 4K 页对齐；请先 page_align_down 再提交，未对齐一律 EINVAL",
            MremapCode::ZeroSize => "零尺寸被拒：old_size 与 new_size 都必须大于 0；请缩短到合法页数后再调用",
            MremapCode::BadFlags => "非法 flags 被拒：只接受 MAYMOVE|FIXED；请清除未知位后再提交，越界标志按 EINVAL 处理",
            MremapCode::FixedNoMove => "FIXED 缺 MAYMOVE 被拒：指定落点必须同时允许搬移；请补上 MAYMOVE 或去掉 FIXED",
            MremapCode::NotMapped => "区间未映射被拒：old_addr..old_addr+old_size 未被单块映射完整覆盖；请核对边界后重试，回 EFAULT",
            MremapCode::NoSpace => "地址空间不足：原地扩展越界且无可用空洞；请缩小 new_size 或先 munmap 腾出连续区间，回 ENOMEM",
            MremapCode::LockedMove => "锁页搬移被拒：区间处于 mlock 状态且请求搬移；请先 munlock 再 mremap，回 EPERM",
            MremapCode::LayeringOverlap => "分层声明重复：同一 surface 被两个域同时认领；请把重复项归还唯一属主后再注册",
            MremapCode::CountMismatch => "区间计数不守恒：搬移前后映射槽数量不一致；请检查失败路径是否遗留半搬移残影",
            MremapCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查回滚路径是否误改累计值",
            MremapCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            MremapCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
            MremapCode::CapacityFull => "映射表已满：8 个槽位全部占用；请先 munmap 再申请，表满回 ENOMEM",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: [MremapCode; 13] = [
        MremapCode::UnalignedOldAddr,
        MremapCode::ZeroSize,
        MremapCode::BadFlags,
        MremapCode::FixedNoMove,
        MremapCode::NotMapped,
        MremapCode::NoSpace,
        MremapCode::LockedMove,
        MremapCode::LayeringOverlap,
        MremapCode::CountMismatch,
        MremapCode::LedgerRegress,
        MremapCode::NotReproducible,
        MremapCode::SegmentViolation,
        MremapCode::CapacityFull,
    ];
}

// ── 区间表模型 ────────────────────────────────────────────────────────
/// 单个映射区间（定容槽，Copy 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub used: bool,
    pub base: u64,
    pub len: u64,
    pub prot: u32,
}

impl Region {
    pub const fn empty() -> Region {
        Region { used: false, base: 0, len: 0, prot: 3 }
    }
    /// 区间末端（不含），饱和加法防回绕。
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.len)
    }
}

/// 定容映射表：8 槽 + 三个单调计数。
#[derive(Clone, Copy, Debug)]
pub struct RegionTable {
    regions: [Region; MAX_REGIONS],
    count: u32,
    maps: u32,
    moves: u32,
}

impl RegionTable {
    pub const fn new() -> RegionTable {
        RegionTable { regions: [Region::empty(); MAX_REGIONS], count: 0, maps: 0, moves: 0 }
    }
    pub fn count(&self) -> u32 {
        self.count
    }
    pub fn maps(&self) -> u32 {
        self.maps
    }
    pub fn moves(&self) -> u32 {
        self.moves
    }
    /// 查找包含 addr 的槽下标。
    pub fn find(&self, addr: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_REGIONS {
            if let Some(r) = self.regions.get(i) {
                if r.used && addr >= r.base && addr < r.end() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
    /// 取槽视图。
    pub fn slot(&self, i: usize) -> Option<&Region> {
        self.regions.get(i)
    }
    /// 区间 [base, base+len) 是否与除 skip 外的槽重叠。
    pub fn overlaps_except(&self, base: u64, len: u64, skip: Option<usize>) -> bool {
        let end = match base.checked_add(len) {
            Some(v) => v,
            None => return true,
        };
        let mut i = 0usize;
        while i < MAX_REGIONS {
            let skipped = match skip {
                Some(k) => k == i,
                None => false,
            };
            if !skipped {
                if let Some(r) = self.regions.get(i) {
                    if r.used && base < r.end() && r.base < end {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }
    /// 区间 [addr, addr+len) 是否被单个已映射区间完整覆盖。
    pub fn covers(&self, addr: u64, len: u64) -> bool {
        let end = match addr.checked_add(len) {
            Some(v) => v,
            None => return false,
        };
        let mut i = 0usize;
        while i < MAX_REGIONS {
            if let Some(r) = self.regions.get(i) {
                if r.used && addr >= r.base && end <= r.end() {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
    /// 插入新区间，成功返回槽下标；表满回 ENOMEM。
    pub fn insert(&mut self, base: u64, len: u64) -> Result<usize, i32> {
        if len == 0 {
            return Err(EINVAL);
        }
        let end = match base.checked_add(len) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        if end > ARENA_END || base < ARENA_BASE {
            return Err(ENOMEM);
        }
        let mut i = 0usize;
        while i < MAX_REGIONS {
            let free = match self.regions.get(i) {
                Some(r) => !r.used,
                None => false,
            };
            if free {
                if let Some(slot) = self.regions.get_mut(i) {
                    slot.used = true;
                    slot.base = base;
                    slot.len = len;
                }
                self.count = self.count.saturating_add(1);
                self.maps = self.maps.saturating_add(1);
                return Ok(i);
            }
            i += 1;
        }
        Err(ENOMEM)
    }
    /// 摘除下标 i 的区间。
    pub fn remove(&mut self, i: usize) -> Result<(), i32> {
        let used = match self.regions.get(i) {
            Some(r) => r.used,
            None => false,
        };
        if !used {
            return Err(EINVAL);
        }
        if let Some(slot) = self.regions.get_mut(i) {
            slot.used = false;
            slot.base = 0;
            slot.len = 0;
        }
        self.count = self.count.saturating_sub(1);
        Ok(())
    }
    /// 在竞技场内寻找能容纳 len 的空洞（页步进扫描）。
    pub fn free_gap(&self, len: u64) -> Option<u64> {
        if len == 0 {
            return None;
        }
        let mut base = ARENA_BASE;
        while base < ARENA_END {
            let end = match base.checked_add(len) {
                Some(v) => v,
                None => return None,
            };
            if end > ARENA_END {
                return None;
            }
            if !self.overlaps_except(base, len, None) {
                return Some(base);
            }
            base = base.saturating_add(PAGE);
        }
        None
    }
    /// 缩放到指定长度（仅限已映射槽）。
    fn resize(&mut self, i: usize, len: u64) {
        if let Some(slot) = self.regions.get_mut(i) {
            slot.len = len;
        }
    }
}

// ── mremap 判定器 ────────────────────────────────────────────────────
/// Linux mremap 判据模型。`locked` 表示区间处于 mlock 状态，
/// 带 MAYMOVE 的搬移必须 EPERM。返回新地址或 errno 正值。
pub fn mremap(
    table: &mut RegionTable,
    old_addr: u64,
    old_size: u64,
    new_size: u64,
    flags: u32,
    locked: bool,
) -> Result<u64, i32> {
    if old_addr % PAGE != 0 {
        return Err(EINVAL);
    }
    if old_size == 0 || new_size == 0 {
        return Err(EINVAL);
    }
    if flags & !(MAYMOVE | FIXED) != 0 {
        return Err(EINVAL);
    }
    if flags & FIXED != 0 && flags & MAYMOVE == 0 {
        return Err(EINVAL);
    }
    if !table.covers(old_addr, old_size) {
        return Err(EFAULT);
    }
    if locked && flags & MAYMOVE != 0 {
        return Err(EPERM);
    }
    let idx = match table.find(old_addr) {
        Some(i) => i,
        None => return Err(EFAULT),
    };
    let new_end = match old_addr.checked_add(new_size) {
        Some(v) => v,
        None => return Err(ENOMEM),
    };
    if new_size <= old_size {
        table.resize(idx, new_size);
        return Ok(old_addr);
    }
    if new_end > ARENA_END {
        return Err(ENOMEM);
    }
    if !table.overlaps_except(old_addr, new_size, Some(idx)) {
        table.resize(idx, new_size);
        return Ok(old_addr);
    }
    if flags & MAYMOVE == 0 {
        return Err(ENOMEM);
    }
    let gap = match table.free_gap(new_size) {
        Some(v) => v,
        None => return Err(ENOMEM),
    };
    if table.remove(idx).is_err() {
        return Err(ENOMEM);
    }
    match table.insert(gap, new_size) {
        Ok(_) => {
            table.moves = table.moves.saturating_add(1);
            Ok(gap)
        }
        Err(e) => Err(e),
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
    /// 记一次注入结果；守恒被破坏回 EINVAL。
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

/// 非对齐偏移格（全部满足 offset % PAGE != 0）。
pub const UNALIGNED_OFFSETS: [u64; UNALIGNED_CELLS] =
    [1, 2, 63, 128, 2048, 4095, 4097, 8193, 12345, 65_535];

/// 非对齐 old_addr 十格矩阵：每格 10 轮，全部 EINVAL。
/// 返回 (逐格命中数, 注入账)。
pub fn run_unaligned_matrix() -> ([u32; UNALIGNED_CELLS], InjectLedger) {
    let mut table = RegionTable::new();
    let _ = table.insert(ARENA_BASE, PAGE * 2);
    let mut ledger = InjectLedger::zero();
    let mut per_cell = [0u32; UNALIGNED_CELLS];
    let mut ci = 0usize;
    while ci < UNALIGNED_OFFSETS.len() {
        if let Some(off) = UNALIGNED_OFFSETS.get(ci).copied() {
            let addr = ARENA_BASE + off;
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match mremap(&mut table, addr, PAGE, PAGE * 2, MAYMOVE, false) {
                    Ok(_) => false,
                    Err(e) => e == EINVAL,
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_cell.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (per_cell, ledger)
}

/// 三错误码注入：EINVAL（零尺寸）、ENOMEM（超竞技场）、EPERM（锁页搬移）。
/// 返回 (逐类命中数, 注入账)。
pub fn run_error_class_matrix() -> ([u32; 3], InjectLedger) {
    let mut table = RegionTable::new();
    let _ = table.insert(ARENA_BASE, PAGE * 4);
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < 3 {
        let expect = match ci {
            0 => EINVAL,
            1 => ENOMEM,
            _ => EPERM,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let got = match ci {
                0 => mremap(&mut table, ARENA_BASE, 0, PAGE, 0, false),
                1 => mremap(&mut table, ARENA_BASE, PAGE, 1u64 << 40, MAYMOVE, false),
                _ => mremap(&mut table, ARENA_BASE, PAGE, PAGE * 2, MAYMOVE, true),
            };
            let caught = match got {
                Ok(_) => false,
                Err(e) => e == expect,
            };
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

// ── 与 B17 的分层声明 ────────────────────────────────────────────────
/// 一条分层声明：属主 + 独占面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayeringClaim {
    pub owner: &'static str,
    pub surface: &'static str,
}

/// 冻结声明表：B17 管基本映射与容量，F9247 管 mremap 错误码面。
pub const LAYERING: &[LayeringClaim] = &[
    LayeringClaim { owner: "B17", surface: "mmap-munmap-basic" },
    LayeringClaim { owner: "B17", surface: "mapping-capacity-ledger" },
    LayeringClaim { owner: "B17", surface: "munmap-partial-range" },
    LayeringClaim { owner: "F9247", surface: "mremap-three-errno" },
    LayeringClaim { owner: "F9247", surface: "mremap-unaligned-old-addr" },
    LayeringClaim { owner: "F9247", surface: "mremap-move-return-addr" },
];

/// 声明表零重复：surface 两两不同，属主只能是 B17/F9247。
pub fn layering_disjoint() -> bool {
    let mut i = 0usize;
    while i < LAYERING.len() {
        if let Some(a) = LAYERING.get(i) {
            if a.owner != "B17" && a.owner != "F9247" {
                return false;
            }
            if a.surface.is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < LAYERING.len() {
                if let Some(b) = LAYERING.get(j) {
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
    while i < LAYERING.len() {
        if let Some(c) = LAYERING.get(i) {
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
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
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

/// 编码一次 mremap 结果（成功 1+低字节，失败 2+errno）。
fn encode(out: &mut Vec<u8>, r: Result<u64, i32>) {
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

/// 规范语料双跑指纹：等长、收缩、原地增长、搬移、非对齐、未映射、超大。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut table = RegionTable::new();
    let _ = table.insert(ARENA_BASE, PAGE * 2);
    let _ = table.insert(ARENA_BASE + PAGE * 4, PAGE * 2);
    encode(&mut bytes, mremap(&mut table, ARENA_BASE, PAGE * 2, PAGE * 2, 0, false));
    encode(&mut bytes, mremap(&mut table, ARENA_BASE, PAGE * 2, PAGE, 0, false));
    encode(&mut bytes, mremap(&mut table, ARENA_BASE, PAGE, PAGE * 2, 0, false));
    encode(&mut bytes, mremap(&mut table, ARENA_BASE, PAGE * 2, PAGE * 8, MAYMOVE, false));
    encode(&mut bytes, mremap(&mut table, ARENA_BASE + 3, PAGE, PAGE, 0, false));
    encode(&mut bytes, mremap(&mut table, ARENA_END - PAGE, PAGE * 2, PAGE * 2, 0, false));
    encode(&mut bytes, mremap(&mut table, ARENA_BASE, PAGE, 1u64 << 40, MAYMOVE, false));
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9247 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9247_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9247");

    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7A000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE > 0x7AB00;
    s.add("段位守卫：0x40 对齐且不与 F9246 重叠", seg_ok,
          "段基址唯一且落在本批窗口，段间不得互相覆盖");

    let mut t = RegionTable::new();
    let _ = t.insert(ARENA_BASE, PAGE * 2);
    let same = mremap(&mut t, ARENA_BASE, PAGE * 2, PAGE * 2, 0, false);
    s.add("正常流：等长 mremap 返回原地址", same == Ok(ARENA_BASE),
          "等长调用不改落点，也不得触发搬移计数");

    let mut flow = RegionTable::new();
    let _ = flow.insert(ARENA_BASE, PAGE * 4);
    let sh = mremap(&mut flow, ARENA_BASE, PAGE * 4, PAGE, 0, false);
    let sh_len = match flow.find(ARENA_BASE) {
        Some(i) => flow.slot(i).map(|r| r.len),
        None => None,
    };
    let gr = mremap(&mut flow, ARENA_BASE, PAGE, PAGE * 4, 0, false);
    s.add("正常流：收缩 4→1 与原地增长 1→4 页",
          sh == Ok(ARENA_BASE) && sh_len == Some(PAGE) && gr == Ok(ARENA_BASE)
              && flow.covers(ARENA_BASE, PAGE * 4) && flow.moves() == 0,
          "原地扩缩只改长度，不搬移、不换地址");

    let mut grow = RegionTable::new();
    let _ = grow.insert(ARENA_BASE, PAGE * 2);
    let _ = grow.insert(ARENA_BASE + PAGE * 4, PAGE * 2);
    let moved = mremap(&mut grow, ARENA_BASE, PAGE * 2, PAGE * 8, MAYMOVE, false);
    let move_ok = match moved {
        Ok(addr) => addr != ARENA_BASE && grow.covers(addr, PAGE * 8) && grow.moves == 1,
        Err(_) => false,
    };
    s.add("正常流：MAYMOVE 搬到空洞且计数为 1", move_ok,
          "搬移必须落到空闲区间，旧区间摘除，moves 计数加一");

    let (per_cell, u_ledger) = run_unaligned_matrix();
    s.add("注入：非对齐 old_addr 10 格各 10/10 全 EINVAL",
          per_cell.iter().all(|n| *n == INJECT_ROUNDS),
          "页内偏移 1..65535 的十格必须全部先报 EINVAL");
    s.add("注入账：非对齐面 100 次全检出且逃逸为 0",
          u_ledger.conserved() && u_ledger.escaped == 0
              && u_ledger.injected == UNALIGNED_CELLS as u32 * INJECT_ROUNDS,
          "100 次注入必须 100 次检出，不得串到其它面");

    let (per_class, e_ledger) = run_error_class_matrix();
    s.add("注入：EINVAL/ENOMEM/EPERM 各 10/10",
          per_class.iter().all(|n| *n == INJECT_ROUNDS),
          "三类错误码必须分别命中，任何一类逃逸即红");
    s.add("注入账：三错误码面 30 次全检出且逃逸为 0",
          e_ledger.conserved() && e_ledger.escaped == 0
              && e_ledger.injected == ERROR_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出，分桶计数与总账一致");

    let mut order = RegionTable::new();
    let _ = order.insert(ARENA_BASE, PAGE);
    let order_ok = mremap(&mut order, ARENA_BASE + 1, PAGE, PAGE, 0, false) == Err(EINVAL)
        && mremap(&mut order, ARENA_BASE + PAGE * 8, PAGE, PAGE, 0, false) == Err(EFAULT);
    s.add("判定次序：非对齐 EINVAL 先于未映射 EFAULT", order_ok,
          "参数面错误必须先于区间面错误返回");

    let mut params = RegionTable::new();
    let _ = params.insert(ARENA_BASE, PAGE * 4);
    let param_ok = mremap(&mut params, ARENA_BASE, 0, PAGE, 0, false) == Err(EINVAL)
        && mremap(&mut params, ARENA_BASE, PAGE, 0, 0, false) == Err(EINVAL)
        && mremap(&mut params, ARENA_BASE, PAGE, PAGE, 8, false) == Err(EINVAL)
        && mremap(&mut params, ARENA_BASE, PAGE, PAGE, FIXED, false) == Err(EINVAL);
    s.add("参数面：零尺寸/坏 flags/FIXED 缺 MAYMOVE 全 EINVAL", param_ok,
          "四类参数错误必须统一回 EINVAL，不得落到区间判定");

    let mut perm = RegionTable::new();
    let _ = perm.insert(ARENA_BASE, PAGE * 2);
    let perm_ok = mremap(&mut perm, ARENA_BASE, PAGE * 2, PAGE, 0, true) == Ok(ARENA_BASE)
        && mremap(&mut perm, ARENA_BASE, PAGE, PAGE * 2, MAYMOVE, true) == Err(EPERM);
    s.add("权限面：锁页搬移 EPERM、锁页收缩放行", perm_ok,
          "EPERM 只在请求搬移时触发，原地收缩不受锁页影响");

    let mut cap = RegionTable::new();
    let mut filled = 0usize;
    while filled < MAX_REGIONS {
        let base = ARENA_BASE + PAGE * (filled as u64) * 2;
        if cap.insert(base, PAGE).is_ok() {
            filled += 1;
        } else {
            break;
        }
    }
    let ninth = cap.insert(ARENA_BASE + PAGE * 32, PAGE);
    s.add("容量：8 槽填满后第 9 块回 ENOMEM",
          filled == MAX_REGIONS && ninth == Err(ENOMEM) && cap.count() == 8
              && cap.maps() == MAX_REGIONS as u32,
          "定容表不得越界写入，满表必须显性回 ENOMEM");

    let mut sm = RegionTable::new();
    let _ = sm.insert(ARENA_BASE, PAGE * 2);
    let _ = sm.insert(ARENA_BASE + PAGE * 4, PAGE * 2);
    let before_count = sm.count();
    let dest = mremap(&mut sm, ARENA_BASE, PAGE * 2, PAGE * 8, MAYMOVE, false);
    let sm_ok = match dest {
        Ok(addr) => {
            sm.count() == before_count
                && sm.find(ARENA_BASE).is_none()
                && sm.find(addr).is_some()
                && sm.moves() == 1
        }
        Err(_) => false,
    };
    s.add("状态机：搬移后区间数守恒且旧址失效", sm_ok,
          "remove+insert 必须成对，失败不得留下双份区间");

    let lay_ok = layering_disjoint()
        && claims_owned_by("B17") == 3
        && claims_owned_by("F9247") == 3
        && claims_owned_by("F9247") + claims_owned_by("B17") == LAYERING.len() as u32;
    s.add("对账：B17 分层声明零重复且属主完整", lay_ok,
          "每个 surface 只能有一个属主，两域条数之和等于总表");

    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOMEM)
        && agrees_with_linux(EPERM)
        && linux_neg(EINVAL) == -EINVAL
        && !agrees_with_linux(0);
    s.add("Linux 逐码一致：EINVAL/ENOMEM/EPERM 表内可复算", codes_ok,
          "三码必须都在 errno 表内，表外码一律拒绝");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    s.add("文案闭集：13 个诊断码均有中文建议",
          MremapCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut i = 0usize;
    while i < MremapCode::ALL.len() {
        let mut j = i + 1;
        while j < MremapCode::ALL.len() {
            if let (Some(a), Some(b)) = (MremapCode::ALL.get(i), MremapCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：13 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    let mut mono = e_ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    let u_text = format!(
        "非对齐注入 {} / 检出 {} / 逃逸 {}",
        u_ledger.injected, u_ledger.detected, u_ledger.escaped
    );
    s.add_dyn("非对齐账目", u_ledger.conserved(), &u_text);
    let e_text = format!(
        "三码注入 {} / 检出 {} / 逃逸 {}",
        e_ledger.injected, e_ledger.detected, e_ledger.escaped
    );
    s.add_dyn("三码账目", e_ledger.conserved(), &e_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_and_grow_in_place() {
        let mut t = RegionTable::new();
        let _ = t.insert(ARENA_BASE, PAGE * 4);
        assert_eq!(mremap(&mut t, ARENA_BASE, PAGE * 4, PAGE, 0, false), Ok(ARENA_BASE));
        let i = t.find(ARENA_BASE).unwrap_or(0);
        assert_eq!(t.slot(i).map(|r| r.len), Some(PAGE));
        assert_eq!(mremap(&mut t, ARENA_BASE, PAGE, PAGE * 4, 0, false), Ok(ARENA_BASE));
        assert!(t.covers(ARENA_BASE, PAGE * 4));
        assert_eq!(t.moves(), 0);
    }

    #[test]
    fn grow_moves_when_blocked() {
        let mut t = RegionTable::new();
        let _ = t.insert(ARENA_BASE, PAGE * 2);
        let _ = t.insert(ARENA_BASE + PAGE * 4, PAGE * 2);
        match mremap(&mut t, ARENA_BASE, PAGE * 2, PAGE * 8, MAYMOVE, false) {
            Ok(addr) => {
                assert_ne!(addr, ARENA_BASE);
                assert!(t.covers(addr, PAGE * 8));
                assert!(t.find(ARENA_BASE).is_none());
                assert_eq!(t.moves(), 1);
            }
            Err(_) => assert!(false, "move should succeed"),
        }
    }

    #[test]
    fn unaligned_always_einval() {
        let mut t = RegionTable::new();
        let _ = t.insert(ARENA_BASE, PAGE * 2);
        let mut i = 0usize;
        while i < UNALIGNED_OFFSETS.len() {
            if let Some(off) = UNALIGNED_OFFSETS.get(i).copied() {
                assert_eq!(
                    mremap(&mut t, ARENA_BASE + off, PAGE, PAGE, MAYMOVE, false),
                    Err(EINVAL)
                );
            }
            i += 1;
        }
    }

    #[test]
    fn parameter_errors() {
        let mut t = RegionTable::new();
        let _ = t.insert(ARENA_BASE, PAGE * 4);
        assert_eq!(mremap(&mut t, ARENA_BASE, 0, PAGE, 0, false), Err(EINVAL));
        assert_eq!(mremap(&mut t, ARENA_BASE, PAGE, PAGE, 8, false), Err(EINVAL));
        assert_eq!(mremap(&mut t, ARENA_BASE, PAGE, PAGE, FIXED, false), Err(EINVAL));
        assert_eq!(
            mremap(&mut t, ARENA_BASE + PAGE * 8, PAGE, PAGE, 0, false),
            Err(EFAULT)
        );
    }

    #[test]
    fn locked_move_eperm() {
        let mut t = RegionTable::new();
        let _ = t.insert(ARENA_BASE, PAGE * 2);
        assert_eq!(
            mremap(&mut t, ARENA_BASE, PAGE * 2, PAGE * 4, MAYMOVE, true),
            Err(EPERM)
        );
        assert_eq!(mremap(&mut t, ARENA_BASE, PAGE * 2, PAGE, 0, true), Ok(ARENA_BASE));
    }

    #[test]
    fn capacity_and_huge_enomem() {
        let mut t = RegionTable::new();
        let _ = t.insert(ARENA_BASE, PAGE);
        assert_eq!(
            mremap(&mut t, ARENA_BASE, PAGE, 1u64 << 40, MAYMOVE, false),
            Err(ENOMEM)
        );
        let mut c = RegionTable::new();
        let mut i = 0u64;
        while i < 8 {
            assert!(c.insert(ARENA_BASE + PAGE * 2 * i, PAGE).is_ok());
            i += 1;
        }
        assert_eq!(c.insert(ARENA_BASE + PAGE * 32, PAGE), Err(ENOMEM));
        assert_eq!(c.count(), 8);
    }

    #[test]
    fn matrices_conserved() {
        let (cells, l1) = run_unaligned_matrix();
        assert!(l1.conserved());
        assert_eq!(l1.escaped, 0);
        assert_eq!(l1.injected, 100);
        for n in cells.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        let (classes, l2) = run_error_class_matrix();
        assert!(l2.conserved());
        assert_eq!(l2.escaped, 0);
        for n in classes.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9247_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
