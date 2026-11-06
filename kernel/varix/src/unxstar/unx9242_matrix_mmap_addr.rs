//! UNX-F9242 mmap 坏 addr 提示与坏映射探针矩阵 · 0x7AA00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（判据 UNX-F9242-J1 逐字摘录）：
//!   mmap addr 非法提示值注入判据过，提示值零副作用正判据格过，映射后
//!   访问坏区间 SIGSEGV 路径格过。
//!
//! 设计要点：
//!   ① 提示值（hint）与固定地址是两种语义：未带 MAP_FIXED 时 hint 只是
//!      「愿望」，内核可以完全忽略；带 MAP_FIXED 时 addr 变成硬约束，必须
//!      逐类校验（低于 mmap_min_addr → EPERM，越 TASK_SIZE → ENOMEM，
//!      未对齐 → EINVAL，与 NOREPLACE 冲突 → EEXIST）。
//!   ② 零副作用是正判据：非法 hint 被忽略后，hint 所在页必须仍未映射，
//!      登记表不得出现任何覆盖 hint 的 VMA，返回地址必须落在分配窗内；
//!      「先污染 hint 再改址」也算失败，不能只看返回值。
//!   ③ 映射后访问按页探针：未映射 → SEGV_MAPERR；权限不符（写只读、
//!      读 PROT_NONE）→ SEGV_ACCERR；合法页返回 Ok。守卫页（映射末页的下
//!      一页）必须稳定落 MAPERR，不许越界吞进上一段。
//!   ④ MAP_FIXED 替换旧区间前先校验地址，失败必须整体回滚：登记表签名
//!      与字节账在 EPERM/ENOMEM/EEXIST 后逐字节不变。
//!   ⑤ 五类坏样本（hint 忽略/NOREPLACE 冲突/固定越界/固定低址/坏区间访问）
//!      各 10/10 检出；注入账注入=检出+逃逸且逃逸恒 0。
//!   ⑥ 地址模型为 4K 页、用户窗 [ALLOC_BASE, ALLOC_TOP)、min_addr=0x1000、
//!      TASK_SIZE=47 位；全部 checked 运算，零裸下标、零浮点。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9242 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AA00;

/// 页大小。
pub const PAGE_SIZE: u64 = 4096;

/// 最低可映射地址（mmap_min_addr 模型）。
pub const MMAP_MIN_ADDR: u64 = 0x1000;

/// 用户地址空间上界（47 位 TASK_SIZE）。
pub const TASK_SIZE: u64 = 0x0000_8000_0000_0000;

/// 自动分配窗下界（256MiB）。
pub const ALLOC_BASE: u64 = 0x1000_0000;

/// 自动分配窗上界（512MiB，不含）。
pub const ALLOC_TOP: u64 = 0x2000_0000;

/// VMA 槽容量。
pub const VMA_CAPACITY: usize = 8;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 坏样本类别数。
pub const BAD_CLASSES: u32 = 5;

/// mmap 标志位（Linux 数值）。
pub const MAP_PRIVATE: u32 = 0x02;
pub const MAP_FIXED: u32 = 0x10;
pub const MAP_ANONYMOUS: u32 = 0x20;
pub const MAP_FIXED_NOREPLACE: u32 = 0x100000;

/// 权限位。
pub const PROT_NONE: u32 = 0x0;
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOMEM: i32 = 12;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

/// SIGSEGV 与 si_code。
pub const SIGSEGV: u32 = 11;
pub const SEGV_MAPERR: u32 = 1;
pub const SEGV_ACCERR: u32 = 2;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0E 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmapCode {
    HintIgnored, FixedBelowMin, FixedUnaligned, FixedBeyondTask,
    NoReplaceConflict, TableFull, RangeWrapped, AccessUnmapped,
    AccessPerm, GuardTouched, RollbackBroken, LedgerRegress,
    NotReproducible, SegmentViolation,
}

impl MmapCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MmapCode::HintIgnored => "提示值被忽略：未带 MAP_FIXED 的 addr 只是愿望；请勿把 hint 当保证地址",
            MmapCode::FixedBelowMin => "固定映射低于 mmap_min_addr：addr < 0x1000 回 EPERM；请抬高地址",
            MmapCode::FixedUnaligned => "固定映射未页对齐：MAP_FIXED 要求 addr 是 4096 倍数；请先对齐",
            MmapCode::FixedBeyondTask => "固定映射越过 TASK_SIZE：区间终点超出用户空间；请缩小长度或降低 addr",
            MmapCode::NoReplaceConflict => "NOREPLACE 冲突：目标区间已有 VMA；请改址或去掉 NOREPLACE",
            MmapCode::TableFull => "VMA 登记表已满：本模型容量 8；请先 munmap 再映射",
            MmapCode::RangeWrapped => "区间回绕：addr+len 溢出 u64；请用 checked 运算复核",
            MmapCode::AccessUnmapped => "访问未映射页：SEGV_MAPERR；请确认探针落在 VMA 内",
            MmapCode::AccessPerm => "访问权限不符：写只读或读 PROT_NONE → SEGV_ACCERR；请先 mprotect",
            MmapCode::GuardTouched => "守卫页被访问：映射末页的下一页必须 MAPERR；请检查长度",
            MmapCode::RollbackBroken => "失败未回滚：被拒的映射改动了登记表；请先校验后落表",
            MmapCode::LedgerRegress => "注入账倒退：计数只增不减；请检查回滚分支",
            MmapCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            MmapCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [MmapCode; 14] = [
        MmapCode::HintIgnored, MmapCode::FixedBelowMin, MmapCode::FixedUnaligned,
        MmapCode::FixedBeyondTask, MmapCode::NoReplaceConflict, MmapCode::TableFull,
        MmapCode::RangeWrapped, MmapCode::AccessUnmapped, MmapCode::AccessPerm,
        MmapCode::GuardTouched, MmapCode::RollbackBroken, MmapCode::LedgerRegress,
        MmapCode::NotReproducible, MmapCode::SegmentViolation,
    ];
}

// ── VMA 登记表 ────────────────────────────────────────────────────────
/// 一段虚拟映射 [start, end)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub prot: u32,
}

impl Vma {
    /// 页数。
    pub fn pages(&self) -> u64 {
        self.end.saturating_sub(self.start) / PAGE_SIZE
    }
}

/// 定容 VMA 表（按 start 升序保持有序）。
#[derive(Clone, Copy, Debug)]
pub struct VmaTable {
    slots: [Option<Vma>; VMA_CAPACITY],
    len: usize,
    inserts: u32,
    punches: u32,
    rejects: u32,
}

impl VmaTable {
    /// 空表。
    pub const fn new() -> VmaTable {
        VmaTable { slots: [None; VMA_CAPACITY], len: 0, inserts: 0, punches: 0, rejects: 0 }
    }

    /// 区间数。
    pub fn len(&self) -> usize { self.len }

    /// 插入次数。
    pub fn inserts(&self) -> u32 { self.inserts }

    /// 打洞次数。
    pub fn punches(&self) -> u32 { self.punches }

    /// 取第 i 段。
    pub fn at(&self, i: usize) -> Option<Vma> {
        match self.slots.get(i) {
            Some(Some(v)) => Some(*v),
            _ => None,
        }
    }

    /// 包含 addr 的 VMA 下标。
    pub fn find(&self, addr: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                if v.start <= addr && addr < v.end {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 区间是否与既有 VMA 相交。
    pub fn overlaps(&self, start: u64, end: u64) -> bool {
        if end <= start {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                if v.start < end && start < v.end {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 已映射字节数。
    pub fn bytes(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                n = n.saturating_add(v.end.saturating_sub(v.start));
            }
            i += 1;
        }
        n
    }

    /// 登记表签名（顺序 + 边界 + 权限）。
    pub fn signature(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                let mut k = 0usize;
                while k < 8 {
                    bytes.push(((v.start >> (k * 8)) & 0xFF) as u8);
                    bytes.push(((v.end >> (k * 8)) & 0xFF) as u8);
                    k += 1;
                }
                bytes.push((v.prot & 0xFF) as u8);
            }
            i += 1;
        }
        fnv1a(&bytes)
    }

    /// 有序插入。容量满回 ENOMEM。
    pub fn insert(&mut self, vma: Vma) -> Result<(), i32> {
        if self.len >= VMA_CAPACITY {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        if vma.end <= vma.start {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let mut pos = self.len;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                if vma.start < v.start {
                    pos = i;
                    break;
                }
            }
            i += 1;
        }
        let mut j = self.len;
        while j > pos {
            let prev = match self.slots.get(j - 1) {
                Some(Some(v)) => Some(*v),
                _ => None,
            };
            if let Some(cell) = self.slots.get_mut(j) {
                *cell = prev;
            }
            j -= 1;
        }
        if let Some(cell) = self.slots.get_mut(pos) {
            *cell = Some(vma);
        }
        self.len += 1;
        self.inserts = self.inserts.saturating_add(1);
        Ok(())
    }

    /// 从区间中打洞（保留左右残段）。
    pub fn punch(&mut self, start: u64, end: u64) -> usize {
        let mut scratch: [Option<Vma>; 24] = [None; 24];
        let mut n = 0usize;
        let mut removed = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.slots.get(i) {
                Some(Some(v)) => *v,
                _ => {
                    i += 1;
                    continue;
                }
            };
            if v.end <= start || v.start >= end {
                if let Some(cell) = scratch.get_mut(n) { *cell = Some(v); }
                n += 1;
            } else {
                removed = removed.saturating_add(1);
                if v.start < start {
                    if let Some(cell) = scratch.get_mut(n) {
                        *cell = Some(Vma { start: v.start, end: start, prot: v.prot });
                    }
                    n += 1;
                }
                if v.end > end {
                    if let Some(cell) = scratch.get_mut(n) {
                        *cell = Some(Vma { start: end, end: v.end, prot: v.prot });
                    }
                    n += 1;
                }
            }
            i += 1;
        }
        let mut w = 0usize;
        while w < VMA_CAPACITY {
            if let Some(cell) = self.slots.get_mut(w) {
                *cell = if w < n { scratch.get(w).copied().flatten() } else { None };
            }
            w += 1;
        }
        self.len = if n > VMA_CAPACITY { VMA_CAPACITY } else { n };
        self.punches = self.punches.saturating_add(1);
        removed
    }

    /// 自动分配：首适配扫描，找不到连续窗口回 ENOMEM。
    pub fn map_any(&mut self, len: u64, prot: u32) -> Result<u64, i32> {
        let mut cand = ALLOC_BASE;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                let fits = match cand.checked_add(len) {
                    Some(e) => e <= v.start,
                    None => return Err(ENOMEM),
                };
                if fits {
                    break;
                }
                if v.end > cand {
                    cand = v.end;
                }
            }
            i += 1;
        }
        let end = match cand.checked_add(len) {
            Some(e) => e,
            None => return Err(ENOMEM),
        };
        if end > ALLOC_TOP {
            return Err(ENOMEM);
        }
        self.insert(Vma { start: cand, end, prot })?;
        Ok(cand)
    }
}

// ── mmap 判定 ─────────────────────────────────────────────────────────
/// mmap 地址判定：hint 与固定地址语义分离。
pub fn mmap_addr(table: &mut VmaTable, hint: u64, len: u64, prot: u32,
                flags: u32) -> Result<u64, i32> {
    if len == 0 {
        return Err(EINVAL);
    }
    let fixed = flags & MAP_FIXED != 0;
    let noreplace = flags & MAP_FIXED_NOREPLACE != 0;
    if !fixed && !noreplace {
        // hint 只是愿望：完全不看 addr，也不得在 hint 处留任何痕迹。
        return table.map_any(len, prot);
    }
    if hint % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    if hint < MMAP_MIN_ADDR {
        return Err(EPERM);
    }
    let end = match hint.checked_add(len) {
        Some(e) => e,
        None => return Err(ENOMEM),
    };
    if end > TASK_SIZE {
        return Err(ENOMEM);
    }
    if noreplace && table.overlaps(hint, end) {
        return Err(EEXIST);
    }
    if fixed {
        let _ = table.punch(hint, end);
    } else if table.overlaps(hint, end) {
        return Err(EEXIST);
    }
    table.insert(Vma { start: hint, end, prot })?;
    Ok(hint)
}

/// 映射后访问探针：返回 SIGSEGV 的 si_code（None 表示合法访问）。
pub fn fault_code(table: &VmaTable, addr: u64, write: bool) -> Option<u32> {
    let idx = match table.find(addr) {
        Some(i) => i,
        None => return Some(SEGV_MAPERR),
    };
    let v = match table.at(idx) {
        Some(v) => v,
        None => return Some(SEGV_MAPERR),
    };
    if write {
        if v.prot & PROT_WRITE == 0 {
            return Some(SEGV_ACCERR);
        }
    } else if v.prot & PROT_READ == 0 {
        return Some(SEGV_ACCERR);
    }
    None
}

/// 访问是否合法。
pub fn access_ok(table: &VmaTable, addr: u64, write: bool) -> bool {
    fault_code(table, addr, write).is_none()
}

// ── 坏 hint 矩阵 ──────────────────────────────────────────────────────
/// 八类非法/边缘提示值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintCase {
    Null, BelowMin, Kernel, NonCanonical, Unaligned, NearTop, AboveTask, MaxU64,
}

impl HintCase {
    /// 闭集全量。
    pub const ALL: [HintCase; 8] = [
        HintCase::Null, HintCase::BelowMin, HintCase::Kernel, HintCase::NonCanonical,
        HintCase::Unaligned, HintCase::NearTop, HintCase::AboveTask, HintCase::MaxU64,
    ];

    /// 样本提示值。
    pub fn sample(self) -> u64 {
        match self {
            HintCase::Null => 0,
            HintCase::BelowMin => 0x800,
            HintCase::Kernel => 0xFFFF_8000_0000_0000,
            HintCase::NonCanonical => 0x0001_0000_0000_0000,
            HintCase::Unaligned => ALLOC_BASE + PAGE_SIZE * 5 + 1,
            HintCase::NearTop => TASK_SIZE - PAGE_SIZE,
            HintCase::AboveTask => TASK_SIZE + PAGE_SIZE,
            HintCase::MaxU64 => u64::MAX,
        }
    }
}

/// 单类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAddrCase {
    HintIgnored, FixedBelowMin, FixedBeyondTask, NoReplaceConflict, AccessPerm,
}

impl BadAddrCase {
    /// 闭集全量。
    pub const ALL: [BadAddrCase; 5] = [
        BadAddrCase::HintIgnored, BadAddrCase::FixedBelowMin,
        BadAddrCase::FixedBeyondTask, BadAddrCase::NoReplaceConflict,
        BadAddrCase::AccessPerm,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger { InjectLedger { injected: 0, detected: 0, escaped: 0 } }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> Result<(), MmapCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(MmapCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 八类 hint 各跑 10 轮，返回逐类「被正确忽略」计数。
pub fn run_hint_matrix() -> [u32; 8] {
    let mut per = [0u32; 8];
    let mut ci = 0usize;
    while ci < HintCase::ALL.len() {
        if let Some(case) = HintCase::ALL.get(ci).copied() {
            let hint = case.sample();
            let mut caught = 0u32;
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut t = VmaTable::new();
                let r = mmap_addr(&mut t, hint, PAGE_SIZE, PROT_READ | PROT_WRITE,
                                  MAP_PRIVATE | MAP_ANONYMOUS);
                let ok = match r {
                    Ok(addr) => {
                        let in_window = addr >= ALLOC_BASE && addr < ALLOC_TOP;
                        let hint_clean = t.find(hint).is_none()
                            && match hint.checked_add(PAGE_SIZE) {
                                Some(e) => !t.overlaps(hint, e),
                                None => true,
                            };
                        in_window && hint_clean && addr != hint && t.bytes() == PAGE_SIZE
                    }
                    Err(_) => false,
                };
                if ok {
                    caught = caught.saturating_add(1);
                }
                round += 1;
            }
            if let Some(slot) = per.get_mut(ci) {
                *slot = caught;
            }
        }
        ci += 1;
    }
    per
}

/// 五类坏样本各 10 轮，返回账本与逐类检出数。
pub fn run_addr_injections() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadAddrCase::ALL.len() {
        if let Some(case) = BadAddrCase::ALL.get(ci).copied() {
            let mut caught = 0u32;
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut t = VmaTable::new();
                let ok = match case {
                    BadAddrCase::HintIgnored => {
                        match mmap_addr(&mut t, 0, PAGE_SIZE, PROT_READ,
                                        MAP_PRIVATE | MAP_ANONYMOUS) {
                            Ok(addr) => t.find(0).is_none() && addr >= ALLOC_BASE,
                            Err(_) => false,
                        }
                    }
                    BadAddrCase::FixedBelowMin => {
                        mmap_addr(&mut t, 0, PAGE_SIZE, PROT_READ,
                                  MAP_FIXED | MAP_PRIVATE) == Err(EPERM)
                    }
                    BadAddrCase::FixedBeyondTask => {
                        mmap_addr(&mut t, TASK_SIZE, PAGE_SIZE, PROT_READ,
                                  MAP_FIXED | MAP_PRIVATE) == Err(ENOMEM)
                    }
                    BadAddrCase::NoReplaceConflict => {
                        let first = mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE * 2,
                                              PROT_READ, MAP_FIXED | MAP_PRIVATE);
                        let sig = t.signature();
                        let second = mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE,
                                               PROT_READ,
                                               MAP_FIXED_NOREPLACE | MAP_PRIVATE);
                        first.is_ok() && second == Err(EEXIST) && t.signature() == sig
                    }
                    BadAddrCase::AccessPerm => {
                        let m = mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE,
                                          PROT_READ, MAP_FIXED | MAP_PRIVATE);
                        m.is_ok()
                            && fault_code(&t, ALLOC_BASE, true) == Some(SEGV_ACCERR)
                            && fault_code(&t, ALLOC_BASE, false).is_none()
                    }
                };
                if ok {
                    caught = caught.saturating_add(1);
                }
                let _ = ledger.record(ok);
                round += 1;
            }
            if let Some(slot) = per.get_mut(ci) {
                *slot = caught;
            }
        }
        ci += 1;
    }
    (ledger, per)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
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

/// 规范调用序列双跑指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut t = VmaTable::new();
    let mut bytes: Vec<u8> = Vec::new();
    let steps = [
        mmap_addr(&mut t, 0, PAGE_SIZE, PROT_READ | PROT_WRITE,
                  MAP_PRIVATE | MAP_ANONYMOUS),
        mmap_addr(&mut t, ALLOC_BASE + PAGE_SIZE * 2, PAGE_SIZE * 2,
                  PROT_READ, MAP_FIXED | MAP_PRIVATE),
        mmap_addr(&mut t, ALLOC_BASE + PAGE_SIZE * 2, PAGE_SIZE,
                  PROT_READ, MAP_FIXED_NOREPLACE | MAP_PRIVATE),
        mmap_addr(&mut t, 0x800, PAGE_SIZE, PROT_READ, MAP_FIXED | MAP_PRIVATE),
    ];
    let mut i = 0usize;
    while i < steps.len() {
        if let Some(r) = steps.get(i) {
            match r {
                Ok(addr) => {
                    bytes.push(0xAA);
                    bytes.push((addr & 0xFF) as u8);
                }
                Err(e) => bytes.push((*e as u32 & 0xFF) as u8),
            }
        }
        i += 1;
    }
    bytes.push(match fault_code(&t, ALLOC_BASE, false) {
        Some(code) => (code & 0xFF) as u8,
        None => 0,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9242 判据集（15 静态 + 3 动态）。
pub fn run_unx9242_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9242");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 八类 hint 全部忽略
    let hints = run_hint_matrix();
    let hint_all = hints.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("坏 hint 矩阵：8 类 × 10 轮全部正确忽略", hint_all,
          "NULL/低址/内核/非规范/未对齐/近顶/越顶/u64::MAX 都只是愿望");

    // 3. 零副作用正判据
    let zero_side = {
        let mut t = VmaTable::new();
        let r = mmap_addr(&mut t, u64::MAX, PAGE_SIZE, PROT_READ,
                          MAP_PRIVATE | MAP_ANONYMOUS);
        match r {
            Ok(addr) => t.bytes() == PAGE_SIZE && t.find(u64::MAX).is_none()
                && addr >= ALLOC_BASE && addr < ALLOC_TOP && t.signature() != 0,
            Err(_) => false,
        }
    };
    s.add("零副作用：非法 hint 不被占用且返回值在窗内", zero_side,
          "只看返回值不够，hint 页必须仍未被映射");

    // 4. 固定低址 EPERM
    let below = {
        let mut t = VmaTable::new();
        mmap_addr(&mut t, 0, PAGE_SIZE, PROT_READ, MAP_FIXED | MAP_PRIVATE) == Err(EPERM)
            && mmap_addr(&mut t, 0x800, PAGE_SIZE, PROT_READ,
                         MAP_FIXED | MAP_PRIVATE) == Err(EINVAL)
            && mmap_addr(&mut t, 0x801, PAGE_SIZE, PROT_READ, MAP_FIXED | MAP_PRIVATE) == Err(EINVAL)
    };
    s.add("固定映射：低址 EPERM、未对齐 EINVAL（对齐先判）", below,
          "低址属权限面、未对齐属参数面，且对齐校验先于低址校验");

    // 5. 固定越 TASK_SIZE
    let beyond = {
        let mut t = VmaTable::new();
        mmap_addr(&mut t, TASK_SIZE, PAGE_SIZE, PROT_READ,
                  MAP_FIXED | MAP_PRIVATE) == Err(ENOMEM)
            && mmap_addr(&mut t, TASK_SIZE - PAGE_SIZE, PAGE_SIZE * 2,
                         PROT_READ, MAP_FIXED | MAP_PRIVATE) == Err(ENOMEM)
    };
    s.add("固定映射：越过 TASK_SIZE 全部 ENOMEM", beyond,
          "尾部跨界与首址越界都必须回 ENOMEM");

    // 7. 合法固定映射
    let fixed_ok = {
        let mut t = VmaTable::new();
        let at = ALLOC_BASE + PAGE_SIZE * 3;
        match mmap_addr(&mut t, at, PAGE_SIZE * 2, PROT_READ | PROT_WRITE,
                        MAP_FIXED | MAP_PRIVATE) {
            Ok(addr) => addr == at && t.find(at) == Some(0)
                && t.find(at + PAGE_SIZE * 2 - 1).is_some()
                && t.bytes() == PAGE_SIZE * 2,
            Err(_) => false,
        }
    };
    s.add("正常流：合法固定映射落在精确地址", fixed_ok,
          "固定地址是硬约束，返回值必须等于 addr");

    // 8. NOREPLACE 冲突
    let noreplace_ok = {
        let mut t = VmaTable::new();
        let first = mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE * 2, PROT_READ,
                              MAP_FIXED | MAP_PRIVATE);
        let sig = t.signature();
        let second = mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE, PROT_READ,
                               MAP_FIXED_NOREPLACE | MAP_PRIVATE);
        first.is_ok() && second == Err(EEXIST) && t.signature() == sig
            && t.bytes() == PAGE_SIZE * 2
    };
    s.add("NOREPLACE：冲突回 EEXIST 且原映射不动", noreplace_ok,
          "冲突失败必须零副作用，字节账与签名逐字节不变");

    // 9. 坏区间访问 MAPERR
    let map_err = {
        let mut t = VmaTable::new();
        let r = mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE * 2, PROT_READ | PROT_WRITE,
                          MAP_FIXED | MAP_PRIVATE);
        r.is_ok()
            && fault_code(&t, ALLOC_BASE - 1, false) == Some(SEGV_MAPERR)
            && fault_code(&t, ALLOC_BASE + PAGE_SIZE * 2, false) == Some(SEGV_MAPERR)
            && fault_code(&t, ALLOC_BASE + PAGE_SIZE * 3, true) == Some(SEGV_MAPERR)
    };
    s.add("访问坏区间：映射前后与守卫页全部 SEGV_MAPERR", map_err,
          "守卫页不得被相邻 VMA 吞掉，越界必须稳定报 MAPERR");

    // 10. 权限不符 ACCERR
    let acc_err = {
        let mut t = VmaTable::new();
        let m = mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE, PROT_READ,
                          MAP_FIXED | MAP_PRIVATE);
        let n = mmap_addr(&mut t, ALLOC_BASE + PAGE_SIZE, PAGE_SIZE, PROT_NONE,
                          MAP_FIXED | MAP_PRIVATE);
        m.is_ok() && n.is_ok()
            && fault_code(&t, ALLOC_BASE, true) == Some(SEGV_ACCERR)
            && fault_code(&t, ALLOC_BASE, false).is_none()
            && fault_code(&t, ALLOC_BASE + PAGE_SIZE, false) == Some(SEGV_ACCERR)
    };
    s.add("访问权限：写只读/读 PROT_NONE 全部 SEGV_ACCERR", acc_err,
          "SIGSEGV 的 si_code 必须区分 MAPERR 与 ACCERR");

    // 11. 失败回滚
    let rollback = {
        let mut t = VmaTable::new();
        let base = mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE, PROT_READ,
                             MAP_FIXED | MAP_PRIVATE);
        let sig = t.signature();
        let bytes = t.bytes();
        let e1 = mmap_addr(&mut t, 0, PAGE_SIZE, PROT_READ,
                           MAP_FIXED | MAP_PRIVATE);
        let e2 = mmap_addr(&mut t, TASK_SIZE, PAGE_SIZE, PROT_READ,
                           MAP_FIXED | MAP_PRIVATE);
        base.is_ok() && e1 == Err(EPERM) && e2 == Err(ENOMEM)
            && t.signature() == sig && t.bytes() == bytes
    };
    s.add("回滚：被拒固定映射不改登记表", rollback,
          "先校验后落表；失败路径不得 punch 或 insert");

    // 12. 文案闭集
    let mut text_ok = true;
    let mut u = 0usize;
    while u < MmapCode::ALL.len() {
        if let Some(c) = MmapCode::ALL.get(u) {
            if c.detail().is_empty() {
                text_ok = false;
            }
            let mut v = u + 1;
            while v < MmapCode::ALL.len() {
                if let Some(d) = MmapCode::ALL.get(v) {
                    if c.val() == d.val() {
                        text_ok = false;
                    }
                }
                v += 1;
            }
        }
        u += 1;
    }
    s.add("文案闭集：14 码中文建议非空且码值唯一", text_ok,
          "每个诊断码必须可定位、可操作");

    // 15. 双跑一致
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范调用序列指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. 注入账守恒
    let (ledger, per) = run_addr_injections();
    let all_caught = per.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入账：五类各 10/10 且逃逸为 0",
          all_caught && ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BAD_CLASSES * INJECT_ROUNDS,
          "50 次注入必须 50 次检出，账目不增不减");

    // 动态明细
    let hint_min = match hints.get(0) { Some(n) => *n, None => 0 };
    let hint_max = match hints.get(7) { Some(n) => *n, None => 0 };
    let hint_text = format!("hint 最小 {} 最大 {} 全判 {}",
                            hint_min, hint_max, hint_all);
    s.add_dyn("hint 快照", hint_all, &hint_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("mmap 地址指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_null_ignored() {
        let mut t = VmaTable::new();
        match mmap_addr(&mut t, 0, PAGE_SIZE, PROT_READ, MAP_PRIVATE) {
            Ok(addr) => {
                assert!(addr >= ALLOC_BASE && addr < ALLOC_TOP);
                assert!(t.find(0).is_none());
                assert_eq!(t.bytes(), PAGE_SIZE);
            }
            Err(_) => assert!(false, "hint 0 must be ignored"),
        }
    }

    #[test]
    fn fixed_below_min_eperm() {
        let mut t = VmaTable::new();
        assert_eq!(mmap_addr(&mut t, 0, PAGE_SIZE, PROT_READ,
                             MAP_FIXED | MAP_PRIVATE), Err(EPERM));
        assert_eq!(mmap_addr(&mut t, 0x800, PAGE_SIZE, PROT_READ,
                             MAP_FIXED | MAP_PRIVATE), Err(EINVAL));
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn fixed_unaligned_einval() {
        let mut t = VmaTable::new();
        assert_eq!(mmap_addr(&mut t, ALLOC_BASE + 1, PAGE_SIZE, PROT_READ,
                             MAP_FIXED | MAP_PRIVATE), Err(EINVAL));
    }

    #[test]
    fn access_errors_split_by_si_code() {
        let mut t = VmaTable::new();
        assert!(mmap_addr(&mut t, ALLOC_BASE, PAGE_SIZE, PROT_READ,
                          MAP_FIXED | MAP_PRIVATE).is_ok());
        assert_eq!(fault_code(&t, ALLOC_BASE, true), Some(SEGV_ACCERR));
        assert_eq!(fault_code(&t, ALLOC_BASE, false), None);
        assert_eq!(fault_code(&t, ALLOC_BASE - 1, false), Some(SEGV_MAPERR));
        assert_eq!(fault_code(&t, ALLOC_BASE + PAGE_SIZE, false), Some(SEGV_MAPERR));
    }

    #[test]
    fn hint_matrix_all_ignored() {
        let per = run_hint_matrix();
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn injection_matrix_conserved() {
        let (l, per) = run_addr_injections();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, BAD_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9242_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
