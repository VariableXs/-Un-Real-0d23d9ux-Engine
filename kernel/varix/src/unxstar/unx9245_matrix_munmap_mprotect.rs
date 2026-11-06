//! UNX-F9245 munmap/mprotect 坏区间矩阵 · 0x7AAC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（判据 UNX-F9245-J1 逐字摘录）：
//!   未对齐区间注入判据过，munmap 全 EINVAL 格过，mprotect 坏 prot
//!   全 EINVAL 格过，区间不存在全 ENOMEM 格过。
//!
//! 设计要点：
//!   ① 两个系统调用共用区间校验，但错误面不同：munmap 对不存在的区间
//!      容忍（成功且零副作用），mprotect 要求整段被 VMA 覆盖，缺口一律
//!      ENOMEM；把 munmap 的宽容误套到 mprotect 是本卡首要陷阱。
//!   ② 判定次序固定：len==0 → addr 对齐 → len 对齐 → 加法回绕 →
//!      prot 位合法 → 覆盖完整；EINVAL 必须先于任何 ENOMEM。
//!   ③ munmap 按页打洞：中段挖除保留左右残段、整段删净、部分覆盖只裁
//!      边；mprotect 同理切三段，中段换权限，字节账与段数必须对账。
//!   ④ 注入矩阵分三组：munmap 4 类、坏 prot 4 类、覆盖缺口 6 类，各
//!      10 轮；14×10=140 次注入，注入=检出+逃逸且逃逸恒 0。
//!   ⑤ 覆盖状态机 None→Partial→Full→Partial→None 随映射/打洞迁移；
//!      失败路径零改表，签名与字节账逐字节不变；双跑指纹一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9245 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AAC0;

/// 页大小、映射基准与容量。
pub const PAGE_SIZE: u64 = 4096;
pub const MAP_BASE: u64 = 0x2000_0000;
pub const VMA_CAPACITY: usize = 8;

/// PROT 位与合法掩码。
pub const PROT_NONE: u32 = 0x0;
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;
pub const PROT_EXEC: u32 = 0x4;
pub const PROT_MASK: u32 = PROT_READ | PROT_WRITE | PROT_EXEC;

/// 注入类别数与单类轮数。
pub const MUNMAP_CLASSES: u32 = 4;
pub const PROT_CLASSES: u32 = 4;
pub const COVER_CLASSES: u32 = 6;
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;

/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeCode {
    LenZero = 1,
    AddrUnaligned = 2,
    LenUnaligned = 3,
    RangeWrapped = 4,
    BadProt = 5,
    NoCoverage = 6,
    PartialCoverage = 7,
    TableFull = 8,
    Overlap = 9,
    StateBroken = 10,
    LedgerRegress = 11,
    NotReproducible = 12,
    SegmentViolation = 13,
}

impl RangeCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 映射到 Linux errno。
    pub fn errno(self) -> i32 {
        match self {
            RangeCode::NoCoverage | RangeCode::PartialCoverage
            | RangeCode::TableFull | RangeCode::Overlap => ENOMEM,
            RangeCode::LenZero | RangeCode::AddrUnaligned | RangeCode::LenUnaligned
            | RangeCode::RangeWrapped | RangeCode::BadProt | RangeCode::StateBroken
            | RangeCode::LedgerRegress | RangeCode::NotReproducible
            | RangeCode::SegmentViolation => EINVAL,
        }
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RangeCode::LenZero => "长度为零：len==0 回 EINVAL；请给出至少一页",
            RangeCode::AddrUnaligned => "地址未对齐：munmap/mprotect 要求 addr 是 4096 倍数",
            RangeCode::LenUnaligned => "长度未对齐：本卡契约要求 len 是 4096 倍数",
            RangeCode::RangeWrapped => "区间回绕：addr+len 溢出 u64；请用 checked 运算复核",
            RangeCode::BadProt => "权限位非法：prot 只允许 READ|WRITE|EXEC；请先掩码",
            RangeCode::NoCoverage => "区间不存在：整段无 VMA 覆盖回 ENOMEM；请先映射",
            RangeCode::PartialCoverage => "区间部分覆盖：存在空洞回 ENOMEM；请补齐或缩段",
            RangeCode::TableFull => "VMA 表已满：容量 8；请先合并或解除旧映射",
            RangeCode::Overlap => "区间重叠：新 VMA 与既有段相交；请改址或先打洞",
            RangeCode::StateBroken => "覆盖状态非法：None/Partial/Full 迁移不可跳跃",
            RangeCode::LedgerRegress => "注入账倒退：计数只增不减；请检查逃逸分支",
            RangeCode::NotReproducible => "双跑不一致：同输入两次结果不同；请查隐藏状态",
            RangeCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [RangeCode; 13] = [
        RangeCode::LenZero, RangeCode::AddrUnaligned, RangeCode::LenUnaligned,
        RangeCode::RangeWrapped, RangeCode::BadProt, RangeCode::NoCoverage,
        RangeCode::PartialCoverage, RangeCode::TableFull, RangeCode::Overlap,
        RangeCode::StateBroken, RangeCode::LedgerRegress, RangeCode::NotReproducible,
        RangeCode::SegmentViolation,
    ];
}

/// 是否页对齐。
pub const fn is_page_aligned(v: u64) -> bool { v & (PAGE_SIZE - 1) == 0 }

/// 区间参数判定：零长度→地址→长度→回绕。
fn judge_range(addr: u64, len: u64) -> Result<u64, RangeCode> {
    if len == 0 {
        return Err(RangeCode::LenZero);
    }
    if !is_page_aligned(addr) {
        return Err(RangeCode::AddrUnaligned);
    }
    if !is_page_aligned(len) {
        return Err(RangeCode::LenUnaligned);
    }
    match addr.checked_add(len) {
        Some(end) => Ok(end),
        None => Err(RangeCode::RangeWrapped),
    }
}

/// 一段虚拟映射 [start, end)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub prot: u32,
}

impl Vma {
    /// 字节数。
    pub fn bytes(&self) -> u64 { self.end.saturating_sub(self.start) }

    /// 是否与 [start, end) 相交。
    pub fn overlaps(&self, start: u64, end: u64) -> bool {
        self.start < end && start < self.end
    }
}

/// 区间覆盖状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Coverage {
    None,
    Partial,
    Full,
}

/// 定容有序 VMA 表。
#[derive(Clone, Copy, Debug)]
pub struct VmaTable {
    slots: [Option<Vma>; VMA_CAPACITY],
    len: usize,
}

impl VmaTable {
    /// 空表。
    pub const fn new() -> VmaTable {
        VmaTable { slots: [None; VMA_CAPACITY], len: 0 }
    }

    /// 段数。
    pub fn len(&self) -> usize { self.len }

    /// 第 i 段。
    pub fn at(&self, i: usize) -> Option<Vma> {
        match self.slots.get(i) {
            Some(Some(v)) => Some(*v),
            _ => None,
        }
    }

    /// 已映射字节数。
    pub fn bytes(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                n = n.saturating_add(v.bytes());
            }
            i += 1;
        }
        n
    }

    /// 有序 + 边界 + 权限签名。
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

    /// addr 所在段的权限。
    pub fn prot_of(&self, addr: u64) -> Option<u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                if v.start <= addr && addr < v.end {
                    return Some(v.prot);
                }
            }
            i += 1;
        }
        None
    }

    /// 覆盖状态：整段被 VMA 无缝覆盖才算 Full。
    pub fn coverage(&self, addr: u64, end: u64) -> Coverage {
        if end <= addr {
            return Coverage::None;
        }
        let mut cur = addr;
        let mut overlaps = false;
        while cur < end {
            let mut next = cur;
            let mut found = false;
            let mut i = 0usize;
            while i < self.len {
                if let Some(Some(v)) = self.slots.get(i) {
                    if v.start < end && addr < v.end {
                        overlaps = true;
                    }
                    if v.start <= cur && cur < v.end {
                        found = true;
                        if v.end > next {
                            next = v.end;
                        }
                    }
                }
                i += 1;
            }
            if !found {
                break;
            }
            cur = next;
        }
        if cur >= end {
            Coverage::Full
        } else if overlaps {
            Coverage::Partial
        } else {
            Coverage::None
        }
    }

    /// 有序插入。
    pub fn insert(&mut self, vma: Vma) -> Result<(), RangeCode> {
        if !is_page_aligned(vma.start) {
            return Err(RangeCode::AddrUnaligned);
        }
        if !is_page_aligned(vma.end) || vma.end <= vma.start {
            return Err(RangeCode::LenUnaligned);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                if v.overlaps(vma.start, vma.end) {
                    return Err(RangeCode::Overlap);
                }
            }
            i += 1;
        }
        if self.len >= VMA_CAPACITY {
            return Err(RangeCode::TableFull);
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
        Ok(())
    }

    fn store(&mut self, scratch: &[Option<Vma>; 24], n: usize) {
        let mut w = 0usize;
        while w < VMA_CAPACITY {
            if let Some(cell) = self.slots.get_mut(w) {
                *cell = if w < n { scratch.get(w).copied().flatten() } else { None };
            }
            w += 1;
        }
        self.len = if n > VMA_CAPACITY { VMA_CAPACITY } else { n };
    }

    /// munmap：按页打洞，区间不存在时成功且零副作用。
    pub fn munmap(&mut self, addr: u64, len: u64) -> Result<(), RangeCode> {
        let end = judge_range(addr, len)?;
        let mut scratch: [Option<Vma>; 24] = [None; 24];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                if v.end <= addr || v.start >= end {
                    if let Some(cell) = scratch.get_mut(n) { *cell = Some(*v); }
                    n += 1;
                } else {
                    if v.start < addr {
                        if let Some(cell) = scratch.get_mut(n) {
                            *cell = Some(Vma { start: v.start, end: addr, prot: v.prot });
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
            }
            i += 1;
        }
        self.store(&scratch, n);
        Ok(())
    }

    /// mprotect：整段覆盖后切三段，中段换权限。
    pub fn mprotect(&mut self, addr: u64, len: u64, prot: u32) -> Result<(), RangeCode> {
        let end = judge_range(addr, len)?;
        if prot & !PROT_MASK != 0 {
            return Err(RangeCode::BadProt);
        }
        match self.coverage(addr, end) {
            Coverage::Full => {}
            Coverage::Partial => return Err(RangeCode::PartialCoverage),
            Coverage::None => return Err(RangeCode::NoCoverage),
        }
        let mut scratch: [Option<Vma>; 24] = [None; 24];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(v)) = self.slots.get(i) {
                if v.end <= addr || v.start >= end {
                    if let Some(cell) = scratch.get_mut(n) { *cell = Some(*v); }
                    n += 1;
                } else {
                    let mid_start = if v.start < addr { addr } else { v.start };
                    let mid_end = if v.end > end { end } else { v.end };
                    if v.start < addr {
                        if let Some(cell) = scratch.get_mut(n) {
                            *cell = Some(Vma { start: v.start, end: addr, prot: v.prot });
                        }
                        n += 1;
                    }
                    if let Some(cell) = scratch.get_mut(n) {
                        *cell = Some(Vma { start: mid_start, end: mid_end, prot });
                    }
                    n += 1;
                    if v.end > end {
                        if let Some(cell) = scratch.get_mut(n) {
                            *cell = Some(Vma { start: end, end: v.end, prot: v.prot });
                        }
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        self.store(&scratch, n);
        Ok(())
    }
}

/// 种子表：两段，中间留 4 页空洞。
pub fn seed_table() -> VmaTable {
    let mut t = VmaTable::new();
    let _ = t.insert(Vma { start: MAP_BASE, end: MAP_BASE + PAGE_SIZE * 4, prot: PROT_READ });
    let _ = t.insert(Vma {
        start: MAP_BASE + PAGE_SIZE * 8,
        end: MAP_BASE + PAGE_SIZE * 12,
        prot: PROT_READ | PROT_WRITE,
    });
    t
}

/// munmap 坏参数四类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MunmapCase {
    AddrOff,
    LenOff,
    LenZero,
    Wrap,
}

impl MunmapCase {
    /// 闭集全量。
    pub const ALL: [MunmapCase; 4] = [
        MunmapCase::AddrOff, MunmapCase::LenOff, MunmapCase::LenZero, MunmapCase::Wrap,
    ];

    /// 样本 (addr, len)。
    pub fn sample(self) -> (u64, u64) {
        match self {
            MunmapCase::AddrOff => (MAP_BASE + 1, PAGE_SIZE),
            MunmapCase::LenOff => (MAP_BASE, PAGE_SIZE + 1),
            MunmapCase::LenZero => (MAP_BASE + 1, 0),
            MunmapCase::Wrap => (
                u64::MAX & !(PAGE_SIZE - 1),
                PAGE_SIZE * 2,
            ),
        }
    }

    /// 期望诊断码。
    pub fn want_code(self) -> RangeCode {
        match self {
            MunmapCase::AddrOff => RangeCode::AddrUnaligned,
            MunmapCase::LenOff => RangeCode::LenUnaligned,
            MunmapCase::LenZero => RangeCode::LenZero,
            MunmapCase::Wrap => RangeCode::RangeWrapped,
        }
    }
}

/// mprotect 坏 prot 四类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtCase {
    Bit8,
    Bit16,
    HighBit,
    AllBits,
}

impl ProtCase {
    /// 闭集全量。
    pub const ALL: [ProtCase; 4] = [
        ProtCase::Bit8, ProtCase::Bit16, ProtCase::HighBit, ProtCase::AllBits,
    ];

    /// 坏权限样本。
    pub fn prot(self) -> u32 {
        match self {
            ProtCase::Bit8 => 0x8,
            ProtCase::Bit16 => 0x10,
            ProtCase::HighBit => 0x8000_0000,
            ProtCase::AllBits => u32::MAX,
        }
    }
}

/// mprotect 覆盖缺口六类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoverCase {
    Before,
    After,
    PartialTail,
    PartialHead,
    Gap,
    Wide,
}

impl CoverCase {
    /// 闭集全量。
    pub const ALL: [CoverCase; 6] = [
        CoverCase::Before, CoverCase::After, CoverCase::PartialTail,
        CoverCase::PartialHead, CoverCase::Gap, CoverCase::Wide,
    ];

    /// 样本 (addr, len)。
    pub fn sample(self) -> (u64, u64) {
        match self {
            CoverCase::Before => (MAP_BASE - PAGE_SIZE * 2, PAGE_SIZE),
            CoverCase::After => (MAP_BASE + PAGE_SIZE * 16, PAGE_SIZE),
            CoverCase::PartialTail => (MAP_BASE + PAGE_SIZE * 3, PAGE_SIZE * 2),
            CoverCase::PartialHead => (MAP_BASE - PAGE_SIZE, PAGE_SIZE * 2),
            CoverCase::Gap => (MAP_BASE + PAGE_SIZE * 5, PAGE_SIZE),
            CoverCase::Wide => (MAP_BASE - PAGE_SIZE * 2, PAGE_SIZE * 8),
        }
    }

    /// 期望诊断码。
    pub fn want_code(self) -> RangeCode {
        match self {
            CoverCase::Before | CoverCase::After | CoverCase::Gap => RangeCode::NoCoverage,
            CoverCase::PartialTail | CoverCase::PartialHead | CoverCase::Wide => {
                RangeCode::PartialCoverage
            }
        }
    }
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
    pub fn record(&mut self, caught: bool) -> Result<(), RangeCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RangeCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

fn munmap_hit(case: MunmapCase) -> bool {
    let mut t = seed_table();
    let sig = t.signature();
    let (addr, len) = case.sample();
    t.munmap(addr, len) == Err(case.want_code()) && t.signature() == sig
}

fn prot_hit(case: ProtCase) -> bool {
    let mut t = seed_table();
    let sig = t.signature();
    t.mprotect(MAP_BASE, PAGE_SIZE, case.prot()) == Err(RangeCode::BadProt)
        && t.signature() == sig
}

fn cover_hit(case: CoverCase) -> bool {
    let mut t = seed_table();
    let sig = t.signature();
    let (addr, len) = case.sample();
    t.mprotect(addr, len, PROT_READ) == Err(case.want_code()) && t.signature() == sig
}

fn record_class(ledger: &mut InjectLedger, caught: u32) {
    let mut k = 0u32;
    while k < INJECT_ROUNDS {
        let _ = ledger.record(k < caught);
        k += 1;
    }
}

fn rounds_munmap(case: MunmapCase) -> u32 {
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if munmap_hit(case) {
            caught = caught.saturating_add(1);
        }
        r += 1;
    }
    caught
}

fn rounds_prot(case: ProtCase) -> u32 {
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if prot_hit(case) {
            caught = caught.saturating_add(1);
        }
        r += 1;
    }
    caught
}

fn rounds_cover(case: CoverCase) -> u32 {
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if cover_hit(case) {
            caught = caught.saturating_add(1);
        }
        r += 1;
    }
    caught
}

/// 三组矩阵：4+4+6 类，各 10 轮。
pub fn run_range_injections() -> (InjectLedger, [u32; 4], [u32; 4], [u32; 6]) {
    let mut ledger = InjectLedger::zero();
    let mut mun = [0u32; 4];
    let mut pro = [0u32; 4];
    let mut cov = [0u32; 6];
    let mut i = 0usize;
    while i < MunmapCase::ALL.len() {
        if let Some(case) = MunmapCase::ALL.get(i).copied() {
            let n = rounds_munmap(case);
            record_class(&mut ledger, n);
            if let Some(slot) = mun.get_mut(i) {
                *slot = n;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < ProtCase::ALL.len() {
        if let Some(case) = ProtCase::ALL.get(i).copied() {
            let n = rounds_prot(case);
            record_class(&mut ledger, n);
            if let Some(slot) = pro.get_mut(i) {
                *slot = n;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < CoverCase::ALL.len() {
        if let Some(case) = CoverCase::ALL.get(i).copied() {
            let n = rounds_cover(case);
            record_class(&mut ledger, n);
            if let Some(slot) = cov.get_mut(i) {
                *slot = n;
            }
        }
        i += 1;
    }
    (ledger, mun, pro, cov)
}

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

/// 规范序列双跑指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut t = seed_table();
    let r1 = t.munmap(MAP_BASE + PAGE_SIZE, PAGE_SIZE);
    let r2 = t.mprotect(MAP_BASE, PAGE_SIZE, PROT_READ | PROT_WRITE);
    let r3 = t.mprotect(MAP_BASE + PAGE_SIZE, PAGE_SIZE, 0x8);
    let r4 = t.mprotect(MAP_BASE + PAGE_SIZE * 16, PAGE_SIZE, PROT_READ);
    let mut bytes: Vec<u8> = Vec::new();
    let steps = [r1, r2, r3, r4];
    let mut i = 0usize;
    while i < steps.len() {
        match steps.get(i) {
            Some(Ok(())) => bytes.push(0xA5),
            Some(Err(code)) => bytes.push((*code as u32 & 0xFF) as u8),
            None => {}
        }
        i += 1;
    }
    bytes.push(t.len() as u8);
    bytes.push((t.bytes() & 0xFF) as u8);
    fnv1a(&bytes)
}

/// F9245 判据集（15 静态 + 3 动态）。
pub fn run_unx9245_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9245");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. munmap 裁边
    let trim = {
        let mut t = seed_table();
        let ok = t.munmap(MAP_BASE, PAGE_SIZE).is_ok();
        ok && t.len() == 2 && t.bytes() == PAGE_SIZE * 7
            && t.prot_of(MAP_BASE).is_none()
            && t.prot_of(MAP_BASE + PAGE_SIZE) == Some(PROT_READ)
    };
    s.add("munmap 裁边：首段前移一页，段数不变", trim,
          "部分覆盖只裁剪边界，字节账必须精确减少一页");

    // 3. munmap 中段打洞
    let split = {
        let mut t = VmaTable::new();
        let _ = t.insert(Vma {
            start: MAP_BASE, end: MAP_BASE + PAGE_SIZE * 4, prot: PROT_READ,
        });
        let ok = t.munmap(MAP_BASE + PAGE_SIZE, PAGE_SIZE * 2).is_ok();
        ok && t.len() == 2 && t.bytes() == PAGE_SIZE * 2
            && t.prot_of(MAP_BASE) == Some(PROT_READ)
            && t.prot_of(MAP_BASE + PAGE_SIZE).is_none()
            && t.prot_of(MAP_BASE + PAGE_SIZE * 3) == Some(PROT_READ)
    };
    s.add("munmap 打洞：中段挖除保留左右残段", split,
          "左残段与右残段都必须保留原权限与精确边界");

    // 4. munmap 整段删净与不存在容忍
    let full = {
        let mut t = VmaTable::new();
        let _ = t.insert(Vma {
            start: MAP_BASE, end: MAP_BASE + PAGE_SIZE * 4, prot: PROT_READ,
        });
        let removed = t.munmap(MAP_BASE, PAGE_SIZE * 4).is_ok()
            && t.len() == 0 && t.bytes() == 0;
        let mut e = VmaTable::new();
        let tolerant = e.munmap(MAP_BASE, PAGE_SIZE).is_ok() && e.len() == 0;
        removed && tolerant
    };
    s.add("munmap 边界：整段删净，区间不存在仍成功", full,
          "munmap 对未映射区间宽容；这一点不得套用到 mprotect");

    // 5. munmap 坏参数矩阵
    let (ledger, mun, pro, cov) = run_range_injections();
    let mun_all = mun.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("munmap 矩阵：未对齐/零长/回绕 4 类各 10/10 EINVAL", mun_all,
          "地址、长度、零长、回绕四类必须逐格命中 EINVAL");

    // 6. 坏 prot 矩阵
    let prot_all = pro.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("mprotect 坏 prot：4 类越界位各 10/10 EINVAL", prot_all,
          "0x8/0x10/高位/全位必须全拒，且失败零改表");

    // 7. 覆盖缺口矩阵
    let cover_all = cov.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("覆盖缺口：6 类各 10/10 ENOMEM", cover_all,
          "整段不存在报 NoCoverage，部分覆盖报 PartialCoverage");

    // 8. mprotect 成功换权
    let update = {
        let mut t = VmaTable::new();
        let _ = t.insert(Vma {
            start: MAP_BASE, end: MAP_BASE + PAGE_SIZE * 2, prot: PROT_READ,
        });
        let ok = t.mprotect(MAP_BASE, PAGE_SIZE, PROT_READ | PROT_WRITE).is_ok();
        ok && t.len() == 2 && t.bytes() == PAGE_SIZE * 2
            && t.prot_of(MAP_BASE) == Some(PROT_READ | PROT_WRITE)
            && t.prot_of(MAP_BASE + PAGE_SIZE) == Some(PROT_READ)
    };
    s.add("mprotect 成功：前页换权，后页保留", update,
          "权限更新不得改变字节账与未覆盖页的原权限");

    // 9. mprotect 切三段
    let three = {
        let mut t = VmaTable::new();
        let _ = t.insert(Vma {
            start: MAP_BASE, end: MAP_BASE + PAGE_SIZE * 4, prot: PROT_READ,
        });
        let ok = t.mprotect(MAP_BASE + PAGE_SIZE, PAGE_SIZE * 2,
                            PROT_READ | PROT_WRITE | PROT_EXEC).is_ok();
        ok && t.len() == 3 && t.bytes() == PAGE_SIZE * 4
            && t.prot_of(MAP_BASE) == Some(PROT_READ)
            && t.prot_of(MAP_BASE + PAGE_SIZE)
                == Some(PROT_READ | PROT_WRITE | PROT_EXEC)
            && t.prot_of(MAP_BASE + PAGE_SIZE * 3) == Some(PROT_READ)
    };
    s.add("mprotect 切三段：中段换权限，两侧原样", three,
          "部分覆盖必须切段，段数与字节账同时对账");

    // 10. 判定次序
    let order = {
        let mut t = seed_table();
        let align_first = t.mprotect(MAP_BASE + 1, PAGE_SIZE, 0x8)
            == Err(RangeCode::AddrUnaligned);
        let prot_first = t.mprotect(MAP_BASE + PAGE_SIZE * 16, PAGE_SIZE, 0x8)
            == Err(RangeCode::BadProt);
        let cover_after = t.mprotect(MAP_BASE + PAGE_SIZE * 16, PAGE_SIZE, PROT_READ)
            == Err(RangeCode::NoCoverage);
        let zero_first = t.munmap(MAP_BASE + 1, 0) == Err(RangeCode::LenZero);
        align_first && prot_first && cover_after && zero_first
    };
    s.add("判定次序：对齐→回绕→prot→覆盖，EINVAL 先于 ENOMEM", order,
          "未对齐+坏 prot 先报对齐；对齐+坏 prot 先报 prot");

    // 11. 失败回滚
    let rollback = {
        let mut t = seed_table();
        let sig = t.signature();
        let bytes = t.bytes();
        let len = t.len();
        let bad_prot = t.mprotect(MAP_BASE, PAGE_SIZE, 0x10);
        let bad_addr = t.munmap(MAP_BASE + 1, PAGE_SIZE);
        let no_cover = t.mprotect(MAP_BASE + PAGE_SIZE * 16, PAGE_SIZE, PROT_READ);
        bad_prot == Err(RangeCode::BadProt)
            && bad_addr == Err(RangeCode::AddrUnaligned)
            && no_cover == Err(RangeCode::NoCoverage)
            && t.signature() == sig && t.bytes() == bytes && t.len() == len
    };
    s.add("回滚：EINVAL/ENOMEM 路径零改表", rollback,
          "失败分支不得动 slots、不得改 len 或权限");

    // 12. 容量与覆盖状态机
    let machine = {
        let mut t = VmaTable::new();
        let start = t.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2);
        let mut filled = true;
        let mut i = 0u64;
        while i < VMA_CAPACITY as u64 {
            let a = MAP_BASE + i * PAGE_SIZE * 2;
            if t.insert(Vma { start: a, end: a + PAGE_SIZE, prot: PROT_READ }).is_err() {
                filled = false;
            }
            i += 1;
        }
        let full = t.insert(Vma {
            start: MAP_BASE + PAGE_SIZE * 100,
            end: MAP_BASE + PAGE_SIZE * 101,
            prot: PROT_READ,
        }) == Err(RangeCode::TableFull);
        let mut c = VmaTable::new();
        let none = c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2) == Coverage::None;
        let _ = c.insert(Vma { start: MAP_BASE, end: MAP_BASE + PAGE_SIZE, prot: PROT_READ });
        let partial = c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2) == Coverage::Partial;
        let _ = c.insert(Vma {
            start: MAP_BASE + PAGE_SIZE, end: MAP_BASE + PAGE_SIZE * 2, prot: PROT_READ,
        });
        let complete = c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2) == Coverage::Full;
        let _ = c.munmap(MAP_BASE, PAGE_SIZE);
        let back = c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2) == Coverage::Partial;
        start == Coverage::None && filled && full && none && partial && complete && back
    };
    s.add("容量与状态机：8 段满拒，None→Partial→Full→Partial", machine,
          "容量与覆盖迁移同时成立；状态迁移不可跳跃");

    // 13. 文案闭集
    let mut text_ok = true;
    let mut u = 0usize;
    while u < RangeCode::ALL.len() {
        if let Some(c) = RangeCode::ALL.get(u) {
            if c.detail().is_empty() || !matches!(c.errno(), ENOMEM | EINVAL) {
                text_ok = false;
            }
            let mut w = u + 1;
            while w < RangeCode::ALL.len() {
                if let Some(d) = RangeCode::ALL.get(w) {
                    if c.val() == d.val() {
                        text_ok = false;
                    }
                }
                w += 1;
            }
        }
        u += 1;
    }
    s.add("文案闭集：13 码中文建议非空、码值唯一、errno 归档", text_ok,
          "每个诊断码必须可定位、可操作，且只允许 ENOMEM/EINVAL");

    // 14. 双跑一致
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范调用序列指纹逐字节相等", fp_a == fp_b,
          "同输入两次必须相等，隐藏状态或计数泄漏即失败");

    // 15. 注入账
    let total_classes = MUNMAP_CLASSES + PROT_CLASSES + COVER_CLASSES;
    s.add("注入账：14 类各 10/10，注入=检出且逃逸为 0",
          mun_all && prot_all && cover_all
              && ledger.conserved() && ledger.escaped == 0
              && ledger.injected == total_classes * INJECT_ROUNDS,
          "140 次注入必须 140 次检出，三组矩阵全部守恒");

    // 动态明细
    let min = match mun.iter().copied().min() { Some(n) => n, None => 0 };
    let max = match mun.iter().copied().max() { Some(n) => n, None => 0 };
    let matrix_text = format!("munmap 最小 {} 最大 {}，注入 {}", min, max, ledger.injected);
    s.add_dyn("矩阵快照", mun_all && prot_all && cover_all, &matrix_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("区间矩阵指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_vma(start: u64, pages: u64, prot: u32) -> VmaTable {
        let mut t = VmaTable::new();
        let _ = t.insert(Vma {
            start,
            end: start + pages * PAGE_SIZE,
            prot,
        });
        t
    }

    #[test]
    fn munmap_trim_and_full() {
        let mut t = seed_table();
        assert!(t.munmap(MAP_BASE, PAGE_SIZE).is_ok());
        assert_eq!(t.len(), 2);
        assert_eq!(t.bytes(), PAGE_SIZE * 7);
        let mut f = one_vma(MAP_BASE, 4, PROT_READ);
        assert!(f.munmap(MAP_BASE, PAGE_SIZE * 4).is_ok());
        assert_eq!(f.len(), 0);
        assert_eq!(f.bytes(), 0);
    }

    #[test]
    fn munmap_split_middle() {
        let mut t = one_vma(MAP_BASE, 4, PROT_READ);
        assert!(t.munmap(MAP_BASE + PAGE_SIZE, PAGE_SIZE * 2).is_ok());
        assert_eq!(t.len(), 2);
        assert_eq!(t.bytes(), PAGE_SIZE * 2);
        assert_eq!(t.prot_of(MAP_BASE), Some(PROT_READ));
        assert_eq!(t.prot_of(MAP_BASE + PAGE_SIZE * 3), Some(PROT_READ));
    }

    #[test]
    fn munmap_nonexistent_tolerated() {
        let mut t = VmaTable::new();
        assert!(t.munmap(MAP_BASE, PAGE_SIZE).is_ok());
        assert_eq!(t.len(), 0);
        let mut e = seed_table();
        assert!(e.munmap(MAP_BASE + PAGE_SIZE * 20, PAGE_SIZE).is_ok());
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn munmap_matrix_einval() {
        let (_, mun, _, _) = run_range_injections();
        for n in mun.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn bad_prot_matrix_einval() {
        let (_, _, pro, _) = run_range_injections();
        for n in pro.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn coverage_matrix_enomem() {
        let (_, _, _, cov) = run_range_injections();
        for n in cov.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        for c in CoverCase::ALL.iter() {
            assert_eq!(c.want_code().errno(), ENOMEM);
        }
    }

    #[test]
    fn mprotect_split_three() {
        let mut t = one_vma(MAP_BASE, 4, PROT_READ);
        assert!(t.mprotect(MAP_BASE + PAGE_SIZE, PAGE_SIZE * 2,
                           PROT_READ | PROT_WRITE).is_ok());
        assert_eq!(t.len(), 3);
        assert_eq!(t.bytes(), PAGE_SIZE * 4);
        assert_eq!(t.prot_of(MAP_BASE), Some(PROT_READ));
        assert_eq!(t.prot_of(MAP_BASE + PAGE_SIZE), Some(PROT_READ | PROT_WRITE));
        assert_eq!(t.prot_of(MAP_BASE + PAGE_SIZE * 3), Some(PROT_READ));
    }

    #[test]
    fn order_and_rollback() {
        let mut t = seed_table();
        let sig = t.signature();
        assert_eq!(t.mprotect(MAP_BASE + 1, PAGE_SIZE, 0x8),
                   Err(RangeCode::AddrUnaligned));
        assert_eq!(t.mprotect(MAP_BASE + PAGE_SIZE * 16, PAGE_SIZE, 0x8),
                   Err(RangeCode::BadProt));
        assert_eq!(t.mprotect(MAP_BASE + PAGE_SIZE * 16, PAGE_SIZE, PROT_READ),
                   Err(RangeCode::NoCoverage));
        assert_eq!(t.munmap(MAP_BASE + 1, 0), Err(RangeCode::LenZero));
        assert_eq!(t.signature(), sig);
    }

    #[test]
    fn capacity_and_coverage_state() {
        let mut t = VmaTable::new();
        let mut i = 0u64;
        while i < VMA_CAPACITY as u64 {
            let a = MAP_BASE + i * PAGE_SIZE * 2;
            assert!(t.insert(Vma {
                start: a, end: a + PAGE_SIZE, prot: PROT_READ,
            }).is_ok());
            i += 1;
        }
        assert_eq!(t.insert(Vma {
            start: MAP_BASE + PAGE_SIZE * 100,
            end: MAP_BASE + PAGE_SIZE * 101,
            prot: PROT_READ,
        }), Err(RangeCode::TableFull));
        let mut c = VmaTable::new();
        assert_eq!(c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2), Coverage::None);
        assert!(c.insert(Vma {
            start: MAP_BASE, end: MAP_BASE + PAGE_SIZE, prot: PROT_READ,
        }).is_ok());
        assert_eq!(c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2), Coverage::Partial);
        assert!(c.insert(Vma {
            start: MAP_BASE + PAGE_SIZE,
            end: MAP_BASE + PAGE_SIZE * 2,
            prot: PROT_READ,
        }).is_ok());
        assert_eq!(c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2), Coverage::Full);
    }

    #[test]
    fn machine_terms_stepwise() {
        let mut t = VmaTable::new();
        assert_eq!(t.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2), Coverage::None);
        let mut i = 0u64;
        while i < VMA_CAPACITY as u64 {
            let a = MAP_BASE + i * PAGE_SIZE * 2;
            assert!(t.insert(Vma { start: a, end: a + PAGE_SIZE, prot: PROT_READ }).is_ok());
            i += 1;
        }
        assert_eq!(t.len(), VMA_CAPACITY);
        assert_eq!(
            t.insert(Vma {
                start: MAP_BASE + PAGE_SIZE * 100,
                end: MAP_BASE + PAGE_SIZE * 101,
                prot: PROT_READ,
            }),
            Err(RangeCode::TableFull)
        );
        let mut c = VmaTable::new();
        assert!(c
            .insert(Vma { start: MAP_BASE, end: MAP_BASE + PAGE_SIZE, prot: PROT_READ })
            .is_ok());
        assert_eq!(c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2), Coverage::Partial);
        assert!(c
            .insert(Vma {
                start: MAP_BASE + PAGE_SIZE,
                end: MAP_BASE + PAGE_SIZE * 2,
                prot: PROT_READ,
            })
            .is_ok());
        assert_eq!(c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2), Coverage::Full);
        assert!(c.munmap(MAP_BASE, PAGE_SIZE).is_ok());
        assert_eq!(c.coverage(MAP_BASE, MAP_BASE + PAGE_SIZE * 2), Coverage::Partial);
    }

    #[test]
    fn fingerprint_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9245_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
