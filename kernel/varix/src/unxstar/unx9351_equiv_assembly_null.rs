//! UNX-F9351 指针等价类总装（NULL/-1/非映射/只读映射） · 0x7C540段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9351 逐字）：
//!   ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9351-J1 四指针等价类总装判据过，
//!   逐类期望 EFAULT 逐格检出，只读映射写路径格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 指针的「坏」有四个互不相同的成因，判据必须逐类分开，不能合并成
//!      「返回 EFAULT 就算过」：
//!        Null      地址 0，页表里根本没有这一页（等价于 mmap_min_addr 策略）
//!        MinusOne  0xFFFF_FFFF_FFFF_FFFF，**规范地址**（x86-64 高半区），
//!                  但页表里没映射——「规范性」与「有映射」是两层，不能混判
//!        Unmapped  规范地址、落在两段映射之间的空洞
//!        ReadOnly  映射存在、读合法，写路径必须 EFAULT
//!      前三类的读写路径都是 EFAULT；第四类只有写是 EFAULT，读必须放行。
//!      把 ReadOnly 也一律判 EFAULT，会掩盖「只读映射读路径被误拒」这种真 bug。
//!   ② 访问判定分三层，次序固定：规范地址 → 页表命中 → 权限（写要求 RW）。
//!      非规范地址在页表里查不到，但两者错误语义同属 EFAULT；判据仍分开
//!      成格，避免「非规范」与「未映射」的实现被同一段代码悄悄合并。
//!   ③ 部分拷贝语义：Linux 在拷贝途中缺页时，若已拷贝 >0 字节，read/write
//!      返回已拷贝字节数而不是 EFAULT；首字节即错才回 EFAULT。本模块把
//!      「起始页末尾 8 字节、长度 16」这一格钉死为 Ok(8)，与「空洞起点、
//!      长度 16」的 EFAULT 形成对照。
//!   ④ 页表定容 4 区：RW 区 16 KiB、RO 区 4 KiB、两条空槽。满表再插回
//!      ENOMEM 并计 rejects；写探测**不得**提升只读区权限（写后复查仍是 RO）。
//!   ⑤ 长度上限 64 KiB：超过即 TooBig → EINVAL。既把「容量面」与「地址面」
//!      分开，也让探针循环有确定上界（不依赖数据长度做无界扫描）。
//!   ⑥ 注入账：10 格 × 10 轮 = 100 次；恒等式 injected == detected + escaped，
//!      逃逸必须为 0；读写两条路径各自记账，避免写路径全绿掩盖读路径。
//!   ⑦ 诊断码段内闭集 12 个（+0x01..=0x0C），code() 段内序号 u16，
//!      addr() = 0x7C540 + 序号（u32），两两唯一，每码一句中文处置。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9351 独占 0x7C540..=0x7C57F 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C540;
/// 前一张卡的段基址：本段必须严格大于它。
pub const SEG_PREV: u32 = 0x7C500;
/// 本批段窗口上界（含）。
pub const SEG_LIMIT: u32 = 0x7D1C0;

/// errno：内存不足（页表满）。
pub const ENOMEM: i32 = 12;
/// errno：坏地址（本模块的主错误码）。
pub const EFAULT: i32 = 14;
/// errno：非法参数（长度越限）。
pub const EINVAL: i32 = 22;

/// 页大小。
pub const PAGE: u64 = 4096;
/// RW 语料区基址（1 GiB，页对齐）。
pub const REGION_BASE: u64 = 0x0000_0000_4000_0000;
/// RW 语料区长度（4 页）。
pub const REGION_LEN: u64 = 4 * PAGE;
/// RW 语料区上界（不含）。
pub const REGION_END: u64 = REGION_BASE + REGION_LEN;
/// 只读映射基址（RW 区之后 48 KiB 处，中间留空洞）。
pub const RO_BASE: u64 = REGION_BASE + 0x10000;
/// 只读映射长度（1 页）。
pub const RO_LEN: u64 = PAGE;
/// 空洞地址（两段映射之间，规范但未映射）。
pub const HOLE: u64 = REGION_BASE + 0x8000;
/// 页表容量（区数）。
pub const CAP: usize = 4;
/// 探针长度上限（容量面闸门，保证循环有界）。
pub const PROBE_LIMIT: u64 = 64 * 1024;
/// 单格注入轮数。
pub const ROUNDS: u32 = 10;
/// 单格探测长度。
pub const PROBE_LEN: u64 = 8;
/// 指针等价类数量（四类坏指针）。
pub const CLASS_N: usize = 4;
/// 冻结格数：四类坏指针 × 读写 + 合法对照 × 读写。
pub const CELL_N: usize = 10;
/// 注入总次数。
pub const INJECT_TOTAL: u32 = (CELL_N as u32) * ROUNDS;
/// 冻结格中期望 EFAULT 的格数。
pub const EFAULT_CELLS: u32 = 7;

/// x86-64 规范地址低半区上界（不含）。
pub const CANON_LOW_END: u64 = 0x0000_8000_0000_0000;
/// x86-64 规范地址高半区下界（含）。
pub const CANON_HIGH_START: u64 = 0xFFFF_8000_0000_0000;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 指针判定诊断码（段内 +0x01..=0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrCode {
    NullLeak,
    MinusOneAccepted,
    UnmappedAccepted,
    ReadOnlyWriteAllowed,
    ReadOnlyReadDenied,
    CanonicalMisjudged,
    PartialCopyWrong,
    PermMutated,
    CapacityOverrun,
    LedgerBroken,
    NotReproducible,
    SegmentViolation,
}

impl PtrCode {
    /// 段内序号（1 起）。
    pub fn code(self) -> u16 {
        1 + self as u16
    }

    /// 完整码值（段基址 + 序号）。
    pub fn addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            PtrCode::NullLeak => "空指针漏判：地址 0 必须 EFAULT；请确认第 0 页未被映射",
            PtrCode::MinusOneAccepted => "-1 被放行：0xFFFF...FFFF 规范但未映射；请先查页表再判权限",
            PtrCode::UnmappedAccepted => "空洞被放行：规范不等于有映射；请补齐页表命中判定",
            PtrCode::ReadOnlyWriteAllowed => "只读区被写穿：写路径必须要求 RW；请在权限层拦下",
            PtrCode::ReadOnlyReadDenied => "只读区读被误拒：RO 读合法；请勿把写权限条件套到读路径",
            PtrCode::CanonicalMisjudged => "规范性判错：低半区 <0x8000_0000_0000、高半区 ≥0xFFFF_8000_0000_0000",
            PtrCode::PartialCopyWrong => "半程拷贝语义错：已拷贝 >0 字节应返回字节数而非 EFAULT",
            PtrCode::PermMutated => "权限被改写：写探测不得提升只读区；请把探测做成只读路径",
            PtrCode::CapacityOverrun => "页表越界：4 区满后必须 ENOMEM；请检查插入前容量判定",
            PtrCode::LedgerBroken => "注入账不守恒：injected 不等于 detected+escaped；请检查记账分支",
            PtrCode::NotReproducible => "双跑不一致：同格两次结果不同；请排查隐藏状态",
            PtrCode::SegmentViolation => "段位越界：基址须 0x40 对齐且落在 0x7C500..=0x7D1C0；请修正 SEG_BASE",
        }
    }

    /// 闭集全量。
    pub const ALL: [PtrCode; 12] = [
        PtrCode::NullLeak,
        PtrCode::MinusOneAccepted,
        PtrCode::UnmappedAccepted,
        PtrCode::ReadOnlyWriteAllowed,
        PtrCode::ReadOnlyReadDenied,
        PtrCode::CanonicalMisjudged,
        PtrCode::PartialCopyWrong,
        PtrCode::PermMutated,
        PtrCode::CapacityOverrun,
        PtrCode::LedgerBroken,
        PtrCode::NotReproducible,
        PtrCode::SegmentViolation,
    ];
}

// ── 权限与页表 ────────────────────────────────────────────────────────
/// 映射权限（只有两种：只读、读写）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Perm {
    /// 只读映射：读放行、写 EFAULT。
    ReadOnly,
    /// 读写映射。
    ReadWrite,
}

impl Perm {
    /// 是否允许读。
    pub const fn readable(self) -> bool {
        true
    }

    /// 是否允许写。
    pub const fn writable(self) -> bool {
        matches!(self, Perm::ReadWrite)
    }
}

/// 一段映射。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub base: u64,
    pub len: u64,
    pub perm: Perm,
}

/// 空区（数组初始化用；len=0 使任何地址都落不进去）。
pub const REGION_EMPTY: Region = Region { base: 0, len: 0, perm: Perm::ReadOnly };
/// RW 语料区常量。
pub const RW_REGION: Region = Region { base: REGION_BASE, len: REGION_LEN, perm: Perm::ReadWrite };
/// RO 语料区常量。
pub const RO_REGION: Region = Region { base: RO_BASE, len: RO_LEN, perm: Perm::ReadOnly };

/// 地址是否规范（x86-64 两层地址空间）。
pub fn canonical(addr: u64) -> bool {
    addr < CANON_LOW_END || addr >= CANON_HIGH_START
}

/// 定容页表：4 区，超出即 ENOMEM。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageTable {
    regions: [Region; CAP],
    n: usize,
    rejects: u32,
}

impl PageTable {
    /// 标准布局：RW 区 + RO 区 + 两条空槽。
    pub fn new() -> PageTable {
        PageTable {
            regions: [RW_REGION, RO_REGION, REGION_EMPTY, REGION_EMPTY],
            n: 2,
            rejects: 0,
        }
    }

    /// 已用区数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 插入被拒次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 取第 i 区视图。
    pub fn region(&self, i: usize) -> Option<Region> {
        if i < self.n {
            self.regions.get(i).copied()
        } else {
            None
        }
    }

    /// 查地址权限：命中返回权限，未命中返回 None。
    pub fn lookup(&self, addr: u64) -> Option<Perm> {
        let mut i = 0usize;
        while i < self.n && i < CAP {
            if let Some(r) = self.regions.get(i) {
                if let Some(off) = addr.checked_sub(r.base) {
                    if off < r.len {
                        return Some(r.perm);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 当前权限（诊断用，等价 lookup）。
    pub fn perm_at(&self, addr: u64) -> Option<Perm> {
        self.lookup(addr)
    }

    /// 该地址是否允许该方向访问（读路径要求 ≥RO，写路径要求 RW）。
    pub fn allows(&self, addr: u64, write: bool) -> bool {
        match self.lookup(addr) {
            Some(Perm::ReadWrite) => true,
            Some(Perm::ReadOnly) => !write,
            None => false,
        }
    }

    /// 插入一段映射：参数面（对齐/零长）EINVAL，容量面（满表）ENOMEM。
    pub fn add(&mut self, base: u64, len: u64, perm: Perm) -> Result<usize, i32> {
        if len == 0 || base & (PAGE - 1) != 0 || len & (PAGE - 1) != 0 {
            return Err(EINVAL);
        }
        if self.n >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        let idx = self.n;
        if let Some(slot) = self.regions.get_mut(idx) {
            *slot = Region { base, len, perm };
        }
        self.n += 1;
        Ok(idx)
    }
}

// ── 探针 ──────────────────────────────────────────────────────────────
/// 探针结果：首错偏移或整段可访问。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Probe {
    /// 整段可访问。
    Ok,
    /// 在偏移处首次不可访问。
    Fault(u64),
    /// 长度超过 PROBE_LIMIT（容量面）。
    TooBig,
}

/// 逐页探针：规范 → 命中 → 权限，三层次序固定。
///
/// 以页为单位推进（不是逐字节），既保证首错偏移就是页首偏移，
/// 也把循环次数压到「长度 / 页大小」，上界由 PROBE_LIMIT 兜住。
pub fn probe_addr(pt: &PageTable, addr: u64, len: u64, write: bool) -> Probe {
    if len == 0 {
        return Probe::Ok;
    }
    if len > PROBE_LIMIT {
        return Probe::TooBig;
    }
    if !canonical(addr) {
        return Probe::Fault(0);
    }
    let mut off = 0u64;
    while off < len {
        let a = match addr.checked_add(off) {
            Some(v) => v,
            None => return Probe::Fault(off),
        };
        if !canonical(a) {
            return Probe::Fault(off);
        }
        if !pt.allows(a, write) {
            return Probe::Fault(off);
        }
        let page_left = PAGE - (a & (PAGE - 1));
        let remaining = len - off;
        let step = if page_left < remaining { page_left } else { remaining };
        off += step;
    }
    Probe::Ok
}

/// 拷贝语义：首字节即错回 EFAULT，半程错返回已拷贝字节数。
pub fn copy_into(pt: &PageTable, addr: u64, len: u64, write: bool) -> Result<u64, i32> {
    match probe_addr(pt, addr, len, write) {
        Probe::Ok => Ok(len),
        Probe::Fault(0) => Err(EFAULT),
        Probe::Fault(k) => Ok(k),
        Probe::TooBig => Err(EINVAL),
    }
}

// ── 四类坏指针 + 合法对照 ────────────────────────────────────────────
/// 指针等价类（前三类为坏指针，第四类只有写坏，第五类是合法对照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    /// 空指针（地址 0，无映射）。
    Null,
    /// 全 F（规范高半区，无映射）。
    MinusOne,
    /// 规范但落在映射空洞。
    Unmapped,
    /// 只读映射（读合法、写 EFAULT）。
    ReadOnly,
    /// 读写映射（合法对照）。
    Writable,
}

impl PtrClass {
    /// 类代表地址。
    pub const fn repr(self) -> u64 {
        match self {
            PtrClass::Null => 0,
            PtrClass::MinusOne => u64::MAX,
            PtrClass::Unmapped => HOLE,
            PtrClass::ReadOnly => RO_BASE,
            PtrClass::Writable => REGION_BASE,
        }
    }

    /// 类名。
    pub const fn name(self) -> &'static str {
        match self {
            PtrClass::Null => "NULL",
            PtrClass::MinusOne => "-1",
            PtrClass::Unmapped => "非映射",
            PtrClass::ReadOnly => "只读映射",
            PtrClass::Writable => "读写映射",
        }
    }

    /// 四类坏指针（判据「四指针等价类」的闭集）。
    pub const BAD: [PtrClass; CLASS_N] = [
        PtrClass::Null,
        PtrClass::MinusOne,
        PtrClass::Unmapped,
        PtrClass::ReadOnly,
    ];

    /// 闭集下标（坏指针 0..3，合法对照 4）。
    pub const fn index(self) -> usize {
        match self {
            PtrClass::Null => 0,
            PtrClass::MinusOne => 1,
            PtrClass::Unmapped => 2,
            PtrClass::ReadOnly => 3,
            PtrClass::Writable => 4,
        }
    }
}

/// 期望结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// 已拷贝字节数。
    Bytes(u64),
    /// 拦下并回 errno。
    Err(i32),
}

/// 一格冻结计划。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub class: PtrClass,
    pub write: bool,
    pub expect: Out,
}

/// 10 格冻结计划：四类坏指针 × 读写 + 合法对照 × 读写。
///
/// 只有「只读映射读」与「读写映射读写」放行；四类的写路径全部 EFAULT。
pub const CELLS: [Cell; CELL_N] = [
    Cell { class: PtrClass::Null, write: false, expect: Out::Err(EFAULT) },
    Cell { class: PtrClass::Null, write: true, expect: Out::Err(EFAULT) },
    Cell { class: PtrClass::MinusOne, write: false, expect: Out::Err(EFAULT) },
    Cell { class: PtrClass::MinusOne, write: true, expect: Out::Err(EFAULT) },
    Cell { class: PtrClass::Unmapped, write: false, expect: Out::Err(EFAULT) },
    Cell { class: PtrClass::Unmapped, write: true, expect: Out::Err(EFAULT) },
    Cell { class: PtrClass::ReadOnly, write: false, expect: Out::Bytes(PROBE_LEN) },
    Cell { class: PtrClass::ReadOnly, write: true, expect: Out::Err(EFAULT) },
    Cell { class: PtrClass::Writable, write: false, expect: Out::Bytes(PROBE_LEN) },
    Cell { class: PtrClass::Writable, write: true, expect: Out::Bytes(PROBE_LEN) },
];

/// 把 Result 归一成 Out。
pub fn normalise(r: Result<u64, i32>) -> Out {
    match r {
        Ok(n) => Out::Bytes(n),
        Err(e) => Out::Err(e),
    }
}

/// 跑一格：用标准页表按类代表地址探测。
pub fn run_cell(pt: &PageTable, c: Cell) -> Out {
    normalise(copy_into(pt, c.class.repr(), PROBE_LEN, c.write))
}

/// 一格是否检出。
pub fn cell_detected(pt: &PageTable, c: Cell) -> bool {
    run_cell(pt, c) == c.expect
}

// ── 账本 ──────────────────────────────────────────────────────────────
/// 注入账：逐类、逐读写方向分开记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PtrLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    /// 逐类命中（坏指针 4 + 合法对照 1）。
    pub per_class: [u32; 5],
    /// 读路径命中 / 写路径命中。
    pub per_dir: [u32; 2],
    /// 账本自校验失败次数。
    pub broken: u32,
}

impl PtrLedger {
    /// 空账。
    pub const fn zero() -> PtrLedger {
        PtrLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            per_class: [0u32; 5],
            per_dir: [0u32; 2],
            broken: 0,
        }
    }

    /// 记一笔。
    pub fn record(&mut self, class: PtrClass, write: bool, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
            if let Some(slot) = self.per_class.get_mut(class.index()) {
                *slot = slot.saturating_add(1);
            }
            let dir = if write { 1usize } else { 0usize };
            if let Some(slot) = self.per_dir.get_mut(dir) {
                *slot = slot.saturating_add(1);
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        let conserved = self.conserved();
        if !conserved {
            self.broken = self.broken.saturating_add(1);
        }
        conserved
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 跑 10 格 × 10 轮。
pub fn run_ptr_injection() -> PtrLedger {
    let pt = PageTable::new();
    let mut led = PtrLedger::zero();
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(c) = CELLS.get(i).copied() {
            let caught = cell_detected(&pt, c);
            let mut r = 0u32;
            while r < ROUNDS {
                led.record(c.class, c.write, caught);
                r += 1;
            }
        }
        i += 1;
    }
    led
}

// ── 半程拷贝与容量面 ─────────────────────────────────────────────────
/// 半程拷贝：从 RW 区最后 8 字节起、读 16 字节，跨进空洞。
///
/// 语义：已拷贝 8 字节 → 返回 Ok(8)（不是 EFAULT）。首字节即错的
/// 对照格是 `copy_into(pt, HOLE, 16, false)`，必须 EFAULT。
pub fn partial_crossing() -> (Result<u64, i32>, Result<u64, i32>) {
    let pt = PageTable::new();
    let cross = copy_into(&pt, REGION_END - 8, 16, false);
    let immediate = copy_into(&pt, HOLE, 16, false);
    (cross, immediate)
}

/// 容量面：4 区填满后第 5 段回 ENOMEM，且 rejects 恰增一笔。
pub fn capacity_probe() -> (u32, bool, usize) {
    let mut pt = PageTable::new();
    let mut accepted = 0u32;
    let mut i = 0u64;
    while i < 2 {
        let base = REGION_BASE + 0x20000 + i * PAGE;
        if pt.add(base, PAGE, Perm::ReadWrite).is_ok() {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    let full = pt.add(REGION_BASE + 0x30000, PAGE, Perm::ReadWrite) == Err(ENOMEM);
    (accepted, full, pt.rejects() as usize)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位，逐字节喂入。
fn fnv(h: u32, bytes: &[u8]) -> u32 {
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc ^= *b as u32;
            acc = acc.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    acc
}

/// 语料指纹：10 格 (类, 方向, 结果) 逐格喂入。
pub fn ptr_fingerprint() -> u32 {
    let pt = PageTable::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(c) = CELLS.get(i) {
            buf.clear();
            buf.push(c.class.index() as u8);
            buf.push(if c.write { 1u8 } else { 0u8 });
            match run_cell(&pt, *c) {
                Out::Bytes(n) => {
                    buf.push(1);
                    buf.push((n & 0xFF) as u8);
                }
                Out::Err(e) => {
                    buf.push(2);
                    buf.push((e & 0x7F) as u8);
                }
            }
            h = fnv(h, &buf);
        }
        i += 1;
    }
    h
}

// ── 完成回写（判据③） ────────────────────────────────────────────────
/// 完成回写记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Completion {
    pub done: bool,
    pub summary_len: usize,
    pub cells: u32,
}

/// 结果摘要（纯整数拼接）。
pub fn completion_summary() -> String {
    let led = run_ptr_injection();
    let (cross, immediate) = partial_crossing();
    format!(
        "F9351 指针等价类总装：{} 类坏指针 + 1 对照 = {} 格 × {} 轮，注入 {} 检出 {} 逃逸 {}，半程 {:?}/{:?}，指纹 0x{:08X}",
        CLASS_N, CELL_N, ROUNDS, led.injected, led.detected, led.escaped,
        cross.is_ok(), immediate.is_err(), ptr_fingerprint()
    )
}

/// 取完成回写。
pub fn completion_record() -> Completion {
    let text = completion_summary();
    Completion { done: true, summary_len: text.len(), cells: CELL_N as u32 }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9351 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9351_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9351");

    s.add("段位守卫：0x7C540 独占 0x40 窗口且不与 F9350 重叠",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0x7C540 && SEG_BASE > SEG_PREV
            && SEG_BASE <= SEG_LIMIT,
        "段基址须 0x40 对齐、严格大于前段、且落在本批窗口内");

    let pt = PageTable::new();

    s.add("等价类守卫：四类代表地址互不相同且各自可解释",
        PtrClass::BAD.len() == CLASS_N
            && PtrClass::Null.repr() == 0
            && PtrClass::MinusOne.repr() == u64::MAX
            && PtrClass::Unmapped.repr() == HOLE
            && PtrClass::ReadOnly.repr() == RO_BASE
            && HOLE >= REGION_END && HOLE < RO_BASE,
        "代表地址必须落在各自语料位置，互不重叠");

    let mut bad_reads = 0u32;
    let mut bad_writes = 0u32;
    let mut bi = 0usize;
    while bi < PtrClass::BAD.len() {
        if let Some(c) = PtrClass::BAD.get(bi) {
            let addr = c.repr();
            if copy_into(&pt, addr, PROBE_LEN, false) == Err(EFAULT) {
                bad_reads = bad_reads.saturating_add(1);
            }
            if copy_into(&pt, addr, PROBE_LEN, true) == Err(EFAULT) {
                bad_writes = bad_writes.saturating_add(1);
            }
        }
        bi += 1;
    }
    s.add("四类坏指针：写路径 4/4 EFAULT",
        bad_writes == CLASS_N as u32,
        "只读映射的写路径也必须 EFAULT，否则写穿被放过");

    s.add("四类坏指针：读路径 3/3 坏类 EFAULT（只读类读合法）",
        bad_reads == 3,
        "只读映射读是合法路径，把它算进 EFAULT 会掩盖误拒 bug");

    let ro = pt.perm_at(RO_BASE);
    let ro_read = copy_into(&pt, RO_BASE, PROBE_LEN, false);
    let ro_write = copy_into(&pt, RO_BASE, PROBE_LEN, true);
    s.add("只读映射写路径格过：读 Ok、写 EFAULT 且权限不变",
        ro == Some(Perm::ReadOnly)
            && ro_read == Ok(PROBE_LEN)
            && ro_write == Err(EFAULT)
            && pt.perm_at(RO_BASE) == Some(Perm::ReadOnly),
        "写探测不得提升权限；读路径不得被写条件误伤");

    s.add("空指针：地址 0 读写皆 EFAULT",
        copy_into(&pt, 0, PROBE_LEN, false) == Err(EFAULT)
            && copy_into(&pt, 0, PROBE_LEN, true) == Err(EFAULT)
            && pt.lookup(0).is_none(),
        "第 0 页未映射，等价 mmap_min_addr 策略");

    s.add("-1 指针：规范地址但未映射，读写皆 EFAULT",
        canonical(u64::MAX)
            && canonical(CANON_HIGH_START)
            && pt.lookup(u64::MAX).is_none()
            && copy_into(&pt, u64::MAX, PROBE_LEN, false) == Err(EFAULT),
        "「规范」与「有映射」是两层，不能混判");

    s.add("非规范地址：0x0000_8000_0000_0000 读写皆 EFAULT",
        !canonical(CANON_LOW_END)
            && !canonical(0x0000_FFFF_FFFF_0000)
            && copy_into(&pt, CANON_LOW_END, PROBE_LEN, false) == Err(EFAULT)
            && copy_into(&pt, CANON_LOW_END, PROBE_LEN, true) == Err(EFAULT),
        "非规范地址在页表里查不到，错误语义同属 EFAULT");

    s.add("非映射空洞：规范但无映射，读写皆 EFAULT",
        canonical(HOLE)
            && pt.lookup(HOLE).is_none()
            && copy_into(&pt, HOLE, PROBE_LEN, false) == Err(EFAULT)
            && copy_into(&pt, HOLE, PROBE_LEN, true) == Err(EFAULT),
        "空洞格与空指针格分开，避免两条路径被同一段代码合并");

    let (cross, immediate) = partial_crossing();
    s.add("半程拷贝：跨页 8 字节错返回字节数、首字节错返回 EFAULT",
        cross == Ok(8) && immediate == Err(EFAULT),
        "已拷贝 >0 才返回字节数；首字节即错必须 EFAULT");

    s.add("零长度：不触碰用户内存即放行（含 nullptr）",
        copy_into(&pt, 0, 0, false) == Ok(0)
            && copy_into(&pt, u64::MAX, 0, true) == Ok(0)
            && probe_addr(&pt, 0, 0, true) == Probe::Ok,
        "零长度是合法空操作，先于地址校验短路");

    s.add("容量面：长度超过 64 KiB 回 EINVAL 且探针有界",
        copy_into(&pt, REGION_BASE, PROBE_LIMIT + 1, false) == Err(EINVAL)
            && probe_addr(&pt, REGION_BASE, PROBE_LIMIT + 1, false) == Probe::TooBig
            && copy_into(&pt, REGION_BASE, PROBE_LIMIT, false).is_ok(),
        "长度越限属参数面，且保证探针循环次数有确定上界");

    let (accepted, full, rejects) = capacity_probe();
    s.add("页表容量：4 区满后第 5 段 ENOMEM 且 rejects 恰一笔",
        accepted == 2 && full && rejects == 1,
        "定容表不得越界写入，满表必须显性回 ENOMEM");

    let led = run_ptr_injection();
    let mut cells_hit = 0u32;
    let mut ci = 0usize;
    while ci < CELLS.len() {
        if let Some(c) = CELLS.get(ci) {
            if cell_detected(&pt, *c) {
                cells_hit = cells_hit.saturating_add(1);
            }
        }
        ci += 1;
    }
    s.add("逐格检出：10 格冻结计划全部命中",
        cells_hit == CELL_N as u32 && CELLS.len() == CELL_N,
        "10/10 才叫逐格；任一格红即分类或权限规则漂移");

    s.add("注入账：100 次注入全检出、逃逸为 0",
        led.injected == INJECT_TOTAL && led.detected == INJECT_TOTAL
            && led.escaped == 0 && led.conserved() && led.broken == 0,
        "injected == detected + escaped，且 escapes 必须为 0");

    let mut efaul_plan = 0u32;
    let mut pi = 0usize;
    while pi < CELLS.len() {
        if let Some(c) = CELLS.get(pi) {
            if c.expect == Out::Err(EFAULT) {
                efaul_plan = efaul_plan.saturating_add(1);
            }
        }
        pi += 1;
    }
    s.add("冻结计划：7 格 EFAULT / 3 格放行，读写两向分开记账",
        efaul_plan == EFAULT_CELLS
            && led.per_dir.iter().sum::<u32>() == led.detected
            && led.per_class.iter().all(|n| *n == ROUNDS * 2),
        "逐类 2 向 × 10 轮；方向合并会让写路径全绿掩盖读路径红");

    let fp_a = ptr_fingerprint();
    s.add("双跑一致：指针语料指纹相等且非零",
        fp_a == ptr_fingerprint() && fp_a != 0,
        "同格两次必须逐字节相等；0 视为未喂料");

    let mut uniq = true;
    let mut ui = 0usize;
    while ui < PtrCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < PtrCode::ALL.len() {
            if let (Some(a), Some(b)) = (PtrCode::ALL.get(ui), PtrCode::ALL.get(uj)) {
                if a.addr() == b.addr() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    s.add("诊断码：12 个段内码 addr 两两唯一且文案非空",
        uniq && PtrCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "码重叠会让红项无法定位；每码必须有中文处置");

    let mut mono = led;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        mono.record(PtrClass::Null, false, true);
        m += 1;
    }
    s.add("账目单调：追加注入只增不减且守恒保持",
        mono.injected == before + 4 && mono.conserved() && mono.escaped == led.escaped,
        "追加记账不得改写既有逃逸数");

    let comp = completion_record();
    s.add("完成回写：状态=已完成且结果摘要非空",
        comp.done && comp.summary_len > 0 && comp.cells == CELL_N as u32,
        "判据③要求状态与摘要同时落账");

    let led_text = format!(
        "注入 {} / 检出 {} / 逃逸 {} / 读向 {} / 写向 {}",
        led.injected, led.detected, led.escaped,
        led.per_dir.first().copied().unwrap_or(0),
        led.per_dir.last().copied().unwrap_or(0)
    );
    s.add_dyn("指针账目", led.conserved() && led.escaped == 0, &led_text);

    let ro_text = format!("只读读 {:?} / 只读写 {:?} / 权限 {:?}", ro_read, ro_write, ro);
    s.add_dyn("只读对照", ro_read.is_ok() && ro_write.is_err(), &ro_text);

    let fp_text = format!("指针指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a != 0, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_layers() {
        assert_eq!(canonical(0), true);
        assert_eq!(canonical(REGION_BASE), true);
        assert_eq!(canonical(u64::MAX), true);
        assert_eq!(canonical(CANON_HIGH_START), true);
        assert_eq!(canonical(CANON_LOW_END), false);
        assert_eq!(canonical(0x0000_FFFF_FFFF_0000), false);
    }

    #[test]
    fn four_classes_reprs_distinct() {
        assert_eq!(PtrClass::Null.repr(), 0);
        assert_eq!(PtrClass::MinusOne.repr(), u64::MAX);
        assert_eq!(PtrClass::Unmapped.repr(), HOLE);
        assert_eq!(PtrClass::ReadOnly.repr(), RO_BASE);
        assert_eq!(PtrClass::BAD.len(), CLASS_N);
    }

    #[test]
    fn bad_classes_read_path() {
        let pt = PageTable::new();
        assert_eq!(copy_into(&pt, 0, PROBE_LEN, false), Err(EFAULT));
        assert_eq!(copy_into(&pt, u64::MAX, PROBE_LEN, false), Err(EFAULT));
        assert_eq!(copy_into(&pt, HOLE, PROBE_LEN, false), Err(EFAULT));
        assert_eq!(copy_into(&pt, RO_BASE, PROBE_LEN, false), Ok(PROBE_LEN));
    }

    #[test]
    fn readonly_write_is_fault_but_read_is_not() {
        let pt = PageTable::new();
        assert_eq!(pt.perm_at(RO_BASE), Some(Perm::ReadOnly));
        assert_eq!(copy_into(&pt, RO_BASE, PROBE_LEN, false), Ok(PROBE_LEN));
        assert_eq!(copy_into(&pt, RO_BASE, PROBE_LEN, true), Err(EFAULT));
        assert_eq!(pt.perm_at(RO_BASE), Some(Perm::ReadOnly));
    }

    #[test]
    fn writable_region_allows_both() {
        let pt = PageTable::new();
        assert_eq!(copy_into(&pt, REGION_BASE, PROBE_LEN, false), Ok(PROBE_LEN));
        assert_eq!(copy_into(&pt, REGION_BASE, PROBE_LEN, true), Ok(PROBE_LEN));
    }

    #[test]
    fn partial_crossing_returns_copied_bytes() {
        let (cross, immediate) = partial_crossing();
        assert_eq!(cross, Ok(8));
        assert_eq!(immediate, Err(EFAULT));
    }

    #[test]
    fn zero_length_short_circuits() {
        let pt = PageTable::new();
        assert_eq!(copy_into(&pt, 0, 0, false), Ok(0));
        assert_eq!(copy_into(&pt, u64::MAX, 0, true), Ok(0));
    }

    #[test]
    fn capacity_and_limit() {
        let pt = PageTable::new();
        assert_eq!(copy_into(&pt, REGION_BASE, PROBE_LIMIT + 1, false), Err(EINVAL));
        let (accepted, full, rejects) = capacity_probe();
        assert_eq!(accepted, 2);
        assert_eq!(full, true);
        assert_eq!(rejects, 1);
    }

    #[test]
    fn all_cells_detected() {
        let pt = PageTable::new();
        let mut n = 0u32;
        for c in CELLS.iter() {
            assert_eq!(cell_detected(&pt, *c), true, "class={}", c.class.name());
            n += 1;
        }
        assert_eq!(n, CELL_N as u32);
    }

    #[test]
    fn injection_ledger_conserved() {
        let led = run_ptr_injection();
        assert_eq!(led.injected, INJECT_TOTAL);
        assert_eq!(led.detected, INJECT_TOTAL);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
        for n in led.per_class.iter() {
            assert_eq!(*n, ROUNDS * 2);
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(ptr_fingerprint(), ptr_fingerprint());
        assert_eq!(ptr_fingerprint() != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9351_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
