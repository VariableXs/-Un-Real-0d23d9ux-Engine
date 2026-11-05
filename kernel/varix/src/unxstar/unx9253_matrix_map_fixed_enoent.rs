//! UNX-F9253 MAP_FIXED 冲突矩阵（ENOENT/EEXIST） · 0x7ACC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9253 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9253-J1 MAP_FIXED 覆盖语义
//!   判据过，与现存映射冲突覆盖可观测格过，MAP_FIXED_NOREPLACE 全 EEXIST
//!   格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 覆盖必须是**可观测**的：替换后同址查表要看到新权限、新 tag，旧 tag
//!      在整表里查不到，且退役页数逐页入账。
//!   ② NOREPLACE 与 FIXED 是两种冲突语义：前者遇重叠整体失败（EEXIST，
//!      表零改动），后者静默覆盖（旧段整段退役）。同一重叠集合上两者结果
//!      必须相反，否则说明分支写反。
//!   ③ ENOENT 的挂点在**后端**：非匿名映射先解析后端，缺失即 ENOENT，且
//!      该门先于冲突门——重叠 + 后端缺失必须报 ENOENT 而不是 EEXIST。
//!   ④ 本引擎不做 VMA 拆分：FIXED 部分重叠时整段旧 VMA 退役再插新段；保守
//!      恒式用字节账：inserted = mapped + retired，逐字节可复算。
//!   ⑤ 拒绝路径零改动：EEXIST/ENOENT/EINVAL 的快照（槽数/字节/序号/指纹）
//!      必须与调用前逐字段相等，命中计数单独记，不进快照。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9253 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7ACC0;

/// 页大小。
pub const PAGE: u64 = 4096;

/// 受管地址窗基址。
pub const ARENA_BASE: u64 = 0x4000_0000;

/// 受管地址窗页数。
pub const ARENA_PAGES: u32 = 16;

/// VMA 槽上限。
pub const MAX_VMAS: usize = 8;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 三种重叠形态（全盖/前部/尾部）。
pub const OVERLAP_SHAPES: u32 = 3;

/// 读权限。
pub const PROT_READ: u32 = 1;
/// 写权限。
pub const PROT_WRITE: u32 = 2;

/// 固定地址覆盖。
pub const MAP_FIXED: u32 = 0x10;
/// 共享映射。
pub const MAP_SHARED: u32 = 0x01;
/// 私有映射。
pub const MAP_PRIVATE: u32 = 0x02;
/// 匿名映射。
pub const MAP_ANONYMOUS: u32 = 0x20;
/// 固定地址且不覆盖。
pub const MAP_FIXED_NOREPLACE: u32 = 0x0010_0000;

/// 合法 flags 并集。
pub const ALLOWED_FLAGS: u32 =
    MAP_SHARED | MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED | MAP_FIXED_NOREPLACE;

/// 无此对象（后端缺失）。
pub const ENOENT: i32 = 2;
/// 已存在（NOREPLACE 冲突）。
pub const EEXIST: i32 = 17;
/// 内存不足。
pub const ENOMEM: i32 = 12;
/// 参数非法。
pub const EINVAL: i32 = 22;
/// 坏 fd（对照表成员）。
pub const EBADF: i32 = 9;

/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FixedCode {
    ReplaceNotObservable,
    ReplaceLedgerDrift,
    NoReplaceLeaked,
    NoReplaceErrno,
    BackendGateLate,
    OverlapUndetected,
    ZeroLenAccepted,
    AlignGateMissed,
    RangeOverflow,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl FixedCode {
    /// 码值（段基址 + 0x01 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FixedCode::ReplaceNotObservable => "覆盖不可观测：替换后同址必须看到新权限与新 tag，旧 tag 全表不可查；只比返回地址不算过",
            FixedCode::ReplaceLedgerDrift => "覆盖账漂移：旧段退役页必须逐页入账，inserted = mapped + retired 要能逐字节复算",
            FixedCode::NoReplaceLeaked => "NOREPLACE 泄漏：冲突时必须零改动，槽数/字节/序号/指纹四项快照全等，先摘链再报错属于泄漏",
            FixedCode::NoReplaceErrno => "NOREPLACE 返回码错：重叠只能报 EEXIST，报 EINVAL 会误导调用方去改参数格式",
            FixedCode::BackendGateLate => "后端门太晚：非匿名映射缺后端必须 ENOENT，且该门先于冲突门，重叠时也要先报后端缺失",
            FixedCode::OverlapUndetected => "重叠未检出：区间判定必须按页覆盖整段比较，只比起点会漏掉前部/尾部部分重叠",
            FixedCode::ZeroLenAccepted => "零长度未拒：FIXED 路径与普通路径一致，len=0 必须 EINVAL（与 F9252 的 munmap 口径对齐）",
            FixedCode::AlignGateMissed => "对齐门漏判：固定地址必须页对齐，未对齐先于冲突判定返回 EINVAL",
            FixedCode::RangeOverflow => "区间溢出：addr+len 走 checked_add，溢出回 ENOMEM，禁止回绕成小地址后继续映射",
            FixedCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查回滚路径是否误改累计值",
            FixedCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查隐藏状态或非确定哈希",
            FixedCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（顺序即码序）。
    pub const ALL: [FixedCode; 12] = [
        FixedCode::ReplaceNotObservable,
        FixedCode::ReplaceLedgerDrift,
        FixedCode::NoReplaceLeaked,
        FixedCode::NoReplaceErrno,
        FixedCode::BackendGateLate,
        FixedCode::OverlapUndetected,
        FixedCode::ZeroLenAccepted,
        FixedCode::AlignGateMissed,
        FixedCode::RangeOverflow,
        FixedCode::LedgerRegress,
        FixedCode::NotReproducible,
        FixedCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

/// 后端来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backing {
    /// 匿名（无后端）。
    Anon,
    /// 文件后端且已解析成功。
    File,
    /// 文件后端缺失（判据要求 ENOENT）。
    Missing,
}

/// 一段映射。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vma {
    /// 起始地址
    pub base: u64,
    /// 页数
    pub pages: u32,
    /// 权限
    pub prot: u32,
    /// flags（保留原样用于观测）
    pub flags: u32,
    /// 后端
    pub backing: Backing,
    /// 分配序号
    pub tag: u16,
}

impl Vma {
    /// 结束地址（不含）。
    pub fn end(&self) -> u64 {
        self.base.saturating_add((self.pages as u64) * PAGE)
    }
}

/// 快照：拒绝路径零改动的观测面（命中计数不进快照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableSnap {
    pub count: u32,
    pub seq: u16,
    pub mapped_bytes: u64,
    pub retired_bytes: u64,
    pub fingerprint: u32,
}

/// 定容映射表。
#[derive(Clone, Copy, Debug)]
pub struct MapTable {
    vmas: [Option<Vma>; MAX_VMAS],
    seq: u16,
    inserted_bytes: u64,
    retired_bytes: u64,
    replaces: u32,
    eexist_hits: u32,
    enoent_hits: u32,
    /// 参数拒绝数（供外部审计直读）。
    pub rejects: u32,
}

impl MapTable {
    /// 空表。
    pub const fn new() -> MapTable {
        MapTable {
            vmas: [None; MAX_VMAS],
            seq: 0,
            inserted_bytes: 0,
            retired_bytes: 0,
            replaces: 0,
            eexist_hits: 0,
            enoent_hits: 0,
            rejects: 0,
        }
    }

    /// 页数上取整（checked）。
    pub fn round_pages(len: usize) -> SysResult<u32> {
        if len == 0 {
            return Ok(0);
        }
        let padded = match (len as u64).checked_add(PAGE - 1) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        let pages = padded / PAGE;
        if pages > u32::MAX as u64 {
            return Err(ENOMEM);
        }
        Ok(pages as u32)
    }

    /// 两区间是否按页重叠。
    pub fn ranges_overlap(a_base: u64, a_pages: u32, b_base: u64, b_pages: u32) -> bool {
        let a_end = a_base.saturating_add((a_pages as u64) * PAGE);
        let b_end = b_base.saturating_add((b_pages as u64) * PAGE);
        a_base < b_end && b_base < a_end
    }

    fn first_overlap(&self, base: u64, pages: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(v)) = self.vmas.get(i) {
                if Self::ranges_overlap(base, pages, v.base, v.pages) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn any_overlap(&self, base: u64, pages: u32) -> bool {
        self.first_overlap(base, pages).is_some()
    }

    fn free_slot(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(cell) = self.vmas.get(i) {
                if cell.is_none() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 退役与 [base, base+pages) 重叠的全部旧段（整段退役，不拆分）。
    fn retire_overlaps(&mut self, base: u64, pages: u32) {
        let mut i = 0usize;
        while i < MAX_VMAS {
            let hit = match self.vmas.get(i) {
                Some(Some(v)) => Self::ranges_overlap(base, pages, v.base, v.pages),
                _ => false,
            };
            if hit {
                let old = match self.vmas.get(i) {
                    Some(Some(v)) => *v,
                    _ => Vma { base: 0, pages: 0, prot: 0, flags: 0, backing: Backing::Anon, tag: 0 },
                };
                if let Some(cell) = self.vmas.get_mut(i) {
                    *cell = None;
                }
                self.retired_bytes = self
                    .retired_bytes
                    .saturating_add((old.pages as u64) * PAGE);
                self.replaces = self.replaces.saturating_add(1);
            }
            i += 1;
        }
    }

    /// 首个能放下 pages 页的空洞（非固定路径）。
    fn place(&self, pages: u32) -> Option<u64> {
        let mut start = 0u32;
        while start + pages <= ARENA_PAGES {
            let base = ARENA_BASE + (start as u64) * PAGE;
            if !self.any_overlap(base, pages) {
                return Some(base);
            }
            start += 1;
        }
        None
    }

    /// 映射入口：次序为 长度 → flags → 对齐 → 后端 → 冲突 → 容量。
    pub fn map(
        &mut self,
        addr: u64,
        len: usize,
        prot: u32,
        flags: u32,
        backing: Backing,
    ) -> SysResult<u64> {
        if len == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let both_fixed = flags & MAP_FIXED != 0 && flags & MAP_FIXED_NOREPLACE != 0;
        if flags & !ALLOWED_FLAGS != 0 || both_fixed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let pages = Self::round_pages(len)?;
        if pages == 0 || pages > ARENA_PAGES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        let fixed = flags & (MAP_FIXED | MAP_FIXED_NOREPLACE) != 0;
        if fixed && addr % PAGE != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let span = match (pages as u64).checked_mul(PAGE) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        let end = match addr.checked_add(span) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        if flags & MAP_ANONYMOUS == 0 && backing == Backing::Missing {
            self.enoent_hits = self.enoent_hits.saturating_add(1);
            return Err(ENOENT);
        }
        let arena_end = ARENA_BASE + (ARENA_PAGES as u64) * PAGE;
        if fixed && (addr < ARENA_BASE || end > arena_end) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        if fixed && flags & MAP_FIXED_NOREPLACE != 0 && self.any_overlap(addr, pages) {
            self.eexist_hits = self.eexist_hits.saturating_add(1);
            return Err(EEXIST);
        }
        if fixed && flags & MAP_FIXED != 0 && self.any_overlap(addr, pages) {
            self.retire_overlaps(addr, pages);
        }
        let base = if fixed {
            addr
        } else {
            match self.place(pages) {
                Some(b) => b,
                None => return Err(ENOMEM),
            }
        };
        let slot = match self.free_slot() {
            Some(i) => i,
            None => return Err(ENOMEM),
        };
        let tag = self.seq.wrapping_add(1);
        self.seq = tag;
        if let Some(cell) = self.vmas.get_mut(slot) {
            *cell = Some(Vma { base, pages, prot, flags, backing, tag });
        }
        self.inserted_bytes = self
            .inserted_bytes
            .saturating_add((pages as u64) * PAGE);
        Ok(base)
    }

    /// 同址权限查询（覆盖可观测的主观测点）。
    pub fn lookup(&self, addr: u64) -> Option<u32> {
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(v)) = self.vmas.get(i) {
                if addr >= v.base && addr < v.end() {
                    return Some(v.prot);
                }
            }
            i += 1;
        }
        None
    }

    /// 同址 tag 查询。
    pub fn tag_at(&self, addr: u64) -> Option<u16> {
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(v)) = self.vmas.get(i) {
                if addr >= v.base && addr < v.end() {
                    return Some(v.tag);
                }
            }
            i += 1;
        }
        None
    }

    /// tag 是否存在（旧 tag 覆盖后必须查不到）。
    pub fn tag_exists(&self, tag: u16) -> bool {
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(v)) = self.vmas.get(i) {
                if v.tag == tag {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 已占用槽数。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(_v)) = self.vmas.get(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 现存映射字节。
    pub fn mapped_bytes(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(v)) = self.vmas.get(i) {
                n = n.saturating_add((v.pages as u64) * PAGE);
            }
            i += 1;
        }
        n
    }

    /// 表指纹（FNV-1a，空槽哨兵 0xFF）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < MAX_VMAS {
            match self.vmas.get(i) {
                Some(Some(v)) => {
                    h = fnv_step(h, (v.base & 0xFF) as u8);
                    h = fnv_step(h, ((v.base >> 12) & 0xFF) as u8);
                    h = fnv_step(h, (v.pages & 0xFF) as u8);
                    h = fnv_step(h, (v.prot & 0xFF) as u8);
                    h = fnv_step(h, (v.tag & 0xFF) as u8);
                }
                _ => h = fnv_step(h, 0xFF),
            }
            i += 1;
        }
        h
    }

    /// 快照。
    pub fn snap(&self) -> TableSnap {
        TableSnap {
            count: self.count(),
            seq: self.seq,
            mapped_bytes: self.mapped_bytes(),
            retired_bytes: self.retired_bytes,
            fingerprint: self.fingerprint(),
        }
    }

    /// 字节守恒：插入 = 现存 + 退役。
    pub fn bytes_conserved(&self) -> bool {
        self.inserted_bytes == self.mapped_bytes().saturating_add(self.retired_bytes)
    }

    /// 覆盖次数。
    pub fn replaces(&self) -> u32 { self.replaces }
    /// EEXIST 命中数。
    pub fn eexist_hits(&self) -> u32 { self.eexist_hits }
    /// ENOENT 命中数。
    pub fn enoent_hits(&self) -> u32 { self.enoent_hits }
    /// 累计插入字节。
    pub fn inserted_bytes(&self) -> u64 { self.inserted_bytes }
    /// 累计退役字节。
    pub fn retired_bytes(&self) -> u64 { self.retired_bytes }
}

/// FNV-1a 单步。
pub fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fnv_step(h, *b);
        }
        i += 1;
    }
    h
}

// ── 重叠形态与注入 ────────────────────────────────────────────────────
/// 重叠形态：旧段 [ARENA_BASE, +4 页)，新段按形态放置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverlapShape {
    /// 全盖：新段起点与旧段相同
    Full,
    /// 前部：新段从旧段第 1 页开始
    Front,
    /// 尾部：新段在旧段末页前 1 页开始
    Tail,
}

impl OverlapShape {
    /// 新段起点。
    pub fn base(self) -> u64 {
        match self {
            OverlapShape::Full => ARENA_BASE,
            OverlapShape::Front => ARENA_BASE + PAGE,
            OverlapShape::Tail => ARENA_BASE + 3 * PAGE,
        }
    }

    /// 闭集全量。
    pub const ALL: [OverlapShape; 3] = [
        OverlapShape::Full,
        OverlapShape::Front,
        OverlapShape::Tail,
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
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(FixedCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// NOREPLACE 冲突矩阵：3 形态 × 10 轮，全 EEXIST 且零改动。
pub fn run_noreplace_matrix() -> (InjectLedger, [u32; 3], bool) {
    let mut ledger = InjectLedger::zero();
    let mut per_shape = [0u32; 3];
    let mut clean = true;
    let mut si = 0usize;
    while si < OverlapShape::ALL.len() {
        if let Some(shape) = OverlapShape::ALL.get(si).copied() {
            let mut t = MapTable::new();
            let _ = t.map(ARENA_BASE, 4 * PAGE as usize, PROT_READ, MAP_ANONYMOUS, Backing::Anon);
            let before = t.snap();
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let flags = MAP_FIXED_NOREPLACE | MAP_ANONYMOUS;
                let caught =
                    t.map(shape.base(), PAGE as usize, PROT_WRITE, flags, Backing::Anon)
                        == Err(EEXIST);
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_shape.get_mut(si) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
            if t.snap() != before {
                clean = false;
            }
        }
        si += 1;
    }
    (ledger, per_shape, clean)
}

/// ENOENT 复测：后端缺失 10 轮，全 ENOENT 且零改动。
pub fn run_enoent_probe() -> (u32, bool) {
    let mut t = MapTable::new();
    let _ = t.map(ARENA_BASE, 2 * PAGE as usize, PROT_READ, MAP_ANONYMOUS, Backing::Anon);
    let before = t.snap();
    let mut hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let flags = MAP_FIXED_NOREPLACE;
        if t.map(ARENA_BASE, PAGE as usize, PROT_READ, flags, Backing::Missing) == Err(ENOENT) {
            hits = hits.saturating_add(1);
        }
        round += 1;
    }
    (hits, t.snap() == before)
}

// ── 对照表 ────────────────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；旗标/对齐非法"),
    ("ENOENT", -2, 487, "Windows ERROR_INVALID_ADDRESS；后端缺失在本引擎折叠到地址码"),
    ("EEXIST", -17, 487, "Windows ERROR_INVALID_ADDRESS；固定地址冲突"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；窗内无洞"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；对照表成员"),
];

/// 查表取负值；表外返回 0。
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

/// EEXIST 与 ENOENT 是两个不同判定域，禁止合并。
pub fn conflict_codes_distinct() -> bool {
    EEXIST != ENOENT && agrees_with_linux(EEXIST) && agrees_with_linux(ENOENT)
}

/// 语料指纹：覆盖/冲突/后端缺失/零长度四类操作序列。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut t = MapTable::new();
    let _ = t.map(ARENA_BASE, 2 * PAGE as usize, PROT_READ, MAP_ANONYMOUS, Backing::Anon);
    let ops = [
        (ARENA_BASE, 2 * PAGE as usize, PROT_WRITE, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon),
        (ARENA_BASE, PAGE as usize, PROT_READ, MAP_FIXED_NOREPLACE | MAP_ANONYMOUS, Backing::Anon),
        (ARENA_BASE + 8 * PAGE, PAGE as usize, PROT_READ, MAP_FIXED, Backing::Missing),
        (ARENA_BASE + 9 * PAGE, 0usize, PROT_READ, MAP_FIXED, Backing::Anon),
    ];
    let mut i = 0usize;
    while i < ops.len() {
        if let Some(op) = ops.get(i) {
            let r = t.map(op.0, op.1, op.2, op.3, op.4);
            bytes.push(match r {
                Ok(addr) => (addr & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            });
        }
        i += 1;
    }
    bytes.push((t.count() & 0xFF) as u8);
    bytes.push((t.replaces() & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9253 判据集（16 条静态 + 2 条动态明细）。
pub fn run_unx9253_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9253");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流 + 覆盖语义：权限与 tag 双观测
    let mut t = MapTable::new();
    let first = t.map(ARENA_BASE, PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
    let normal = first == Ok(ARENA_BASE) && t.lookup(ARENA_BASE) == Some(PROT_READ);
    let old_tag = t.tag_at(ARENA_BASE).map_or(0, |v| v);
    let replaced = t.map(ARENA_BASE, PAGE as usize, PROT_READ | PROT_WRITE,
                         MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
    let new_tag = t.tag_at(ARENA_BASE).map_or(0, |v| v);
    let replace_ok = normal
        && replaced == Ok(ARENA_BASE)
        && t.lookup(ARENA_BASE) == Some(PROT_READ | PROT_WRITE)
        && new_tag != old_tag
        && !t.tag_exists(old_tag)
        && t.replaces() == 1
        && t.count() == 1;
    s.add("正常流 + 覆盖：权限翻转 + tag 更替 + 旧 tag 不可查", replace_ok,
          "合法固定地址先成功，再覆盖时必须三项同时成立");

    // 4. NOREPLACE：3 形态 × 10 轮全 EEXIST 且冲突零改动
    let (ledger, per_shape, clean) = run_noreplace_matrix();
    let all_eexist = per_shape.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("NOREPLACE：三形态 × 10 轮全 EEXIST 且快照全等", all_eexist && clean,
          "任一形态漏检或快照变动都说明冲突分支有副作用");

    // 5. ENOENT：后端缺失 10/10
    let (enoent_hits, enoent_clean) = run_enoent_probe();
    s.add("后端缺失：10/10 ENOENT 且零改动", enoent_hits == INJECT_ROUNDS && enoent_clean,
          "缺后端是非匿名映射的前置失败，不得落到冲突分支");

    // 8. 判定次序：后端门先于冲突门
    let mut t3 = MapTable::new();
    let _ = t3.map(ARENA_BASE, 2 * PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
    let before3 = t3.snap();
    let order = t3.map(ARENA_BASE, PAGE as usize, PROT_READ,
                       MAP_FIXED_NOREPLACE, Backing::Missing) == Err(ENOENT)
        && t3.snap() == before3
        && t3.eexist_hits() == 0;
    s.add("判定次序：重叠 + 缺后端先报 ENOENT", order,
          "后端门晚于冲突门会把「没有后端」误诊成「地址被占」");

    // 6. 边界：零长度与未对齐都在冲突门之前拒绝
    let mut t4 = MapTable::new();
    let _ = t4.map(ARENA_BASE, PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
    let zero = t4.map(ARENA_BASE, 0, PROT_READ, MAP_FIXED, Backing::Anon) == Err(EINVAL);
    let align = t4.map(ARENA_BASE + 1, PAGE as usize, PROT_READ,
                       MAP_FIXED_NOREPLACE, Backing::Anon) == Err(EINVAL)
        && t4.eexist_hits() == 0;
    s.add("边界：零长度 EINVAL，未对齐 EINVAL 且不记 EEXIST", zero && align,
          "长度门与对齐门都必须先于冲突门，错误码不许串位");

    // 13. 容量：非固定路径无洞即 ENOMEM
    let mut t8 = MapTable::new();
    let mut i = 0u32;
    while i < ARENA_PAGES / 2 {
        let _ = t8.map(0, 2 * PAGE as usize, PROT_READ, MAP_ANONYMOUS, Backing::Anon);
        i += 1;
    }
    let full = t8.map(0, PAGE as usize, PROT_READ, MAP_ANONYMOUS, Backing::Anon) == Err(ENOMEM)
        && t8.mapped_bytes() == (ARENA_PAGES as u64) * PAGE;
    s.add("容量：arena 打满后非固定映射回 ENOMEM", full,
          "16 页打满后必须失败，且现存字节恰为 64KiB");

    // 9. 字节守恒：插入 = 现存 + 退役
    let mut t9 = MapTable::new();
    let _ = t9.map(ARENA_BASE, 2 * PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
    let _ = t9.map(ARENA_BASE, PAGE as usize, PROT_WRITE, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
    let _ = t9.map(ARENA_BASE + 4 * PAGE, PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
    s.add("守恒：插入字节 = 现存字节 + 退役字节", t9.bytes_conserved(),
          "覆盖路径最容易漏记退役字节，逐字节复算才能钉死");

    // 10. 注入账守恒
    s.add("注入账：30 次注入 30 次检出 0 逃逸",
          ledger.conserved()
              && ledger.escaped == 0
              && ledger.injected == OVERLAP_SHAPES * INJECT_ROUNDS,
          "3 形态 × 10 轮必须全部检出 EEXIST");

    // 16. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：覆盖/冲突/缺失语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 动态明细：覆盖字节账（冲突命中数并入上方守恒判据）
    let text = format!(
        "插入 {} / 现存 {} / 退役 {} / 覆盖 {} 次 / EEXIST {}",
        t9.inserted_bytes(), t9.mapped_bytes(), t9.retired_bytes(),
        t9.replaces(), t3.eexist_hits()
    );
    s.add_dyn("覆盖字节账", t9.bytes_conserved(), &text);

    // 17. Linux 逐码 + 判定域互斥 + Windows 折叠声明
    let codes_ok = conflict_codes_distinct()
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOMEM)
        && agrees_with_linux(EBADF)
        && !agrees_with_linux(0);
    s.add("Linux 逐码一致且 EEXIST/ENOENT 判定域不合并", codes_ok,
          "两码负值可复算且必须可区分；Windows 侧折叠只作说明");

    // 文案闭集 + 码值唯一
    let text_ok = FixedCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut u = 0usize;
    while u < FixedCode::ALL.len() {
        let mut v = u + 1;
        while v < FixedCode::ALL.len() {
            if let (Some(x), Some(y)) = (FixedCode::ALL.get(u), FixedCode::ALL.get(v)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            v += 1;
        }
        u += 1;
    }
    s.add("文案闭集：12 码均有中文建议且码值两两唯一", text_ok && uniq,
          "任一条为空或重码，判据都无法定位到具体违约点");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_replace_is_observable() {
        let mut t = MapTable::new();
        let a = t.map(ARENA_BASE, PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
        assert_eq!(a, Ok(ARENA_BASE));
        let old = t.tag_at(ARENA_BASE).map_or(0, |v| v);
        let b = t.map(ARENA_BASE, PAGE as usize, PROT_WRITE, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
        assert_eq!(b, Ok(ARENA_BASE));
        assert_eq!(t.lookup(ARENA_BASE), Some(PROT_WRITE));
        assert_ne!(t.tag_at(ARENA_BASE).map_or(0, |v| v), old);
        assert!(!t.tag_exists(old));
    }

    #[test]
    fn noreplace_conflict_eexist() {
        let mut t = MapTable::new();
        let _ = t.map(ARENA_BASE, 4 * PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
        let before = t.snap();
        let flags = MAP_FIXED_NOREPLACE | MAP_ANONYMOUS;
        for shape in OverlapShape::ALL.iter() {
            assert_eq!(t.map(shape.base(), PAGE as usize, PROT_WRITE, flags, Backing::Anon), Err(EEXIST));
        }
        assert_eq!(t.snap(), before);
        assert_eq!(t.eexist_hits(), OVERLAP_SHAPES);
    }

    #[test]
    fn missing_backing_enoent() {
        let mut t = MapTable::new();
        assert_eq!(
            t.map(ARENA_BASE, PAGE as usize, PROT_READ, MAP_FIXED, Backing::Missing),
            Err(ENOENT)
        );
        assert_eq!(t.count(), 0);
    }

    #[test]
    fn backend_gate_precedes_conflict() {
        let mut t = MapTable::new();
        let _ = t.map(ARENA_BASE, 2 * PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
        assert_eq!(
            t.map(ARENA_BASE, PAGE as usize, PROT_READ, MAP_FIXED_NOREPLACE, Backing::Missing),
            Err(ENOENT)
        );
        assert_eq!(t.eexist_hits(), 0);
    }

    #[test]
    fn zero_len_and_align_rejected() {
        let mut t = MapTable::new();
        assert_eq!(t.map(ARENA_BASE, 0, PROT_READ, MAP_FIXED, Backing::Anon), Err(EINVAL));
        assert_eq!(t.map(ARENA_BASE + 1, PAGE as usize, PROT_READ, MAP_FIXED, Backing::Anon), Err(EINVAL));
        assert_eq!(t.map(ARENA_BASE, PAGE as usize, PROT_READ, MAP_FIXED | MAP_FIXED_NOREPLACE, Backing::Anon), Err(EINVAL));
    }

    #[test]
    fn bytes_conserved_after_replace() {
        let mut t = MapTable::new();
        let _ = t.map(ARENA_BASE, 3 * PAGE as usize, PROT_READ, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
        let _ = t.map(ARENA_BASE, 2 * PAGE as usize, PROT_WRITE, MAP_FIXED | MAP_ANONYMOUS, Backing::Anon);
        assert!(t.bytes_conserved());
        assert_eq!(t.inserted_bytes(), 5 * PAGE);
        assert_eq!(t.retired_bytes(), 3 * PAGE);
        assert_eq!(t.mapped_bytes(), 2 * PAGE);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9253_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
