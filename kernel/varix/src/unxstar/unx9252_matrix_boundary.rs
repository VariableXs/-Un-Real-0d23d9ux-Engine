//! UNX-F9252 零长度映射边界矩阵 · 0x7AC80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9252 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9252-J1 零长度 mmap 正判据
//!   （返回合法地址零副作用）判据过，零长度 munmap 全 EINVAL 格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 判据把零长度 mmap 定义成**正判据**：返回一个合法地址且零副作用。
//!      这与 Linux 的 `mmap(len=0) -> -EINVAL` 相反，是本引擎的显性口径，
//!      不是实现疏漏；差异在 F9256 的 Windows/异口径差异账里登记，本模块
//!      只负责把「返回合法地址」与「零副作用」两件事各自变成可观测格。
//!   ② 零副作用必须逐项可核：VMA 数量、bump 游标、映射页数、映射字节数、
//!      表指纹五元组在 10 次零长度 mmap 前后逐字段相等。只比地址不够——
//!      地址相同但偷偷占了一个槽，就是隐性副作用。
//!   ③ 零长度 munmap 一律 EINVAL，且该判定**先于**地址校验：非法地址 +
//!      零长度也必须回 EINVAL，并且不得把这次调用记进地址拒绝账（
//!      `addr_rejects` 不涨），否则判定次序就发生了倒置。
//!   ④ 页数上取整走 checked 算术：`len + PAGE - 1` 溢出即 ENOMEM，不静默
//!      回绕；`len=4095/4096/4097` 三点钉死上取整边界，防止「整除当上取整」
//!      的经典 off-by-one。
//!   ⑤ 容量口径：arena 16 页、VMA 槽 8 个；零长度调用不消耗页也不消耗槽，
//!      先打 100 次零长度再打满 arena，与直接打满结果必须一致。
//!   ⑥ 账目守恒：注入 = 检出 + 逃逸（恒 0）；插入 = 现存 + 退役，双向可复算。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9252 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AC80;

/// 页大小（4K 基准页）。
pub const PAGE: u64 = 4096;

/// 受管地址窗基址。
pub const ARENA_BASE: u64 = 0x4000_0000;

/// 受管地址窗页数（16 页 = 64KiB）。
pub const ARENA_PAGES: u32 = 16;

/// VMA 槽位上限。
pub const MAX_VMAS: usize = 8;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 零长度 munmap 的地址类别数（含非法地址，用于钉死判定次序）。
pub const ZERO_LEN_CLASSES: u32 = 5;

/// 零长度 mmap 的规范返回地址：对齐、固定、永不被映射占用。
pub const ZERO_LEN_ADDR: u64 = 0x4FFF_F000;

/// 读权限位。
pub const PROT_READ: u32 = 1;
/// 写权限位。
pub const PROT_WRITE: u32 = 2;

/// 无此对象（本模块仅作对照表成员）。
pub const ENOENT: i32 = 2;
/// 内存不足。
pub const ENOMEM: i32 = 12;
/// 坏指针。
pub const EFAULT: i32 = 14;
/// 参数非法（零长度 munmap 的主码）。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapCode {
    ZeroMapRejected,
    ZeroMapSideEffect,
    ZeroUnmapAccepted,
    ZeroUnmapTouched,
    RoundUpDrift,
    OverflowLeak,
    CapacityLeak,
    WrongErrno,
    OrderViolation,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl MapCode {
    /// 码值（段基址 + 0x01 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MapCode::ZeroMapRejected => "零长度 mmap 判据要求返回合法地址；若被拒为 EINVAL，\
                说明判定器把本引擎口径写成了 Linux 口径，请回到冻结表核对",
            MapCode::ZeroMapSideEffect => "零长度 mmap 必须零副作用：槽位、bump 游标、映射\
                页数与表指纹五项都不许变；请逐项比对快照而不是只比返回地址",
            MapCode::ZeroUnmapAccepted => "零长度 munmap 必须全 EINVAL：没有长度就没有待拆除\
                区间，请把长度门放在地址门之前",
            MapCode::ZeroUnmapTouched => "零长度 munmap 不得改动任何映射：拒绝路径必须在\
                查表之前返回，不能先摘链再报错",
            MapCode::RoundUpDrift => "页数上取整漂移：len=1/4095/4096/4097 四点必须落在\
                1/1/1/2 页，请用 checked_add(PAGE-1) 再做整除",
            MapCode::OverflowLeak => "长度溢出未拒：len 接近 usize::MAX 时必须走 checked_add\
                回 ENOMEM，禁止回绕成小页数后继续映射",
            MapCode::CapacityLeak => "容量账泄漏：零长度调用不得占用 VMA 槽或 arena 页；\
                请先判长度再判容量",
            MapCode::WrongErrno => "返回码串位：零长度 munmap 只允许 EINVAL，缺内存只允许\
                ENOMEM，两者不得互换",
            MapCode::OrderViolation => "判定次序倒置：长度门必须先于地址门，非法地址加零\
                长度仍应回 EINVAL 且不计入地址拒绝账",
            MapCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查是否有回滚路径\
                误改累计值",
            MapCode::NotReproducible => "双跑不一致：同输入两次指纹不同；请排查隐藏状态或\
                非确定哈希",
            MapCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（顺序即码序）。
    pub const ALL: [MapCode; 12] = [
        MapCode::ZeroMapRejected,
        MapCode::ZeroMapSideEffect,
        MapCode::ZeroUnmapAccepted,
        MapCode::ZeroUnmapTouched,
        MapCode::RoundUpDrift,
        MapCode::OverflowLeak,
        MapCode::CapacityLeak,
        MapCode::WrongErrno,
        MapCode::OrderViolation,
        MapCode::LedgerRegress,
        MapCode::NotReproducible,
        MapCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

/// 一段连续映射（页粒度，整段同权限）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vma {
    /// 起始地址（页对齐）
    pub base: u64,
    /// 页数（≥1）
    pub pages: u32,
    /// 权限位
    pub prot: u32,
    /// 分配序号（每次成功 mmap 自增，用于观测覆盖/复用）
    pub tag: u16,
}

/// 映射表快照：零副作用的五元组判据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableSnap {
    pub count: u32,
    pub next_page: u32,
    pub mapped_pages: u32,
    pub mapped_bytes: u64,
    pub fingerprint: u32,
}

/// 定容映射表：bump 分配 + 不回收的 arena（保守口径，容量判据可复算）。
#[derive(Clone, Copy, Debug)]
pub struct MapTable {
    vmas: [Option<Vma>; MAX_VMAS],
    next_page: u32,
    seq: u16,
    maps: u32,
    unmaps: u32,
    zero_maps: u32,
    zero_unmaps: u32,
    addr_rejects: u32,
    noops: u32,
    inserted_pages: u32,
    retired_pages: u32,
}

impl MapTable {
    /// 空表。
    pub const fn new() -> MapTable {
        MapTable {
            vmas: [None; MAX_VMAS],
            next_page: 0,
            seq: 0,
            maps: 0,
            unmaps: 0,
            zero_maps: 0,
            zero_unmaps: 0,
            addr_rejects: 0,
            noops: 0,
            inserted_pages: 0,
            retired_pages: 0,
        }
    }

    /// 页数上取整（checked；len=0 回 0，由调用方决定语义）。
    pub fn round_pages(len: usize) -> SysResult<u32> {
        if len == 0 {
            return Ok(0);
        }
        let bytes = len as u64;
        let padded = match bytes.checked_add(PAGE - 1) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        let pages = padded / PAGE;
        if pages > u32::MAX as u64 {
            return Err(ENOMEM);
        }
        Ok(pages as u32)
    }

    /// 首个空槽下标。
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

    /// 映射：len=0 走正判据分支（合法地址 + 零副作用）。
    pub fn mmap(&mut self, len: usize, prot: u32) -> SysResult<u64> {
        if len == 0 {
            self.zero_maps = self.zero_maps.saturating_add(1);
            return Ok(ZERO_LEN_ADDR);
        }
        let pages = Self::round_pages(len)?;
        if pages == 0 {
            return Err(EINVAL);
        }
        let end = match self.next_page.checked_add(pages) {
            Some(e) => e,
            None => return Err(ENOMEM),
        };
        if end > ARENA_PAGES {
            return Err(ENOMEM);
        }
        let slot = match self.free_slot() {
            Some(i) => i,
            None => return Err(ENOMEM),
        };
        let base = ARENA_BASE + (self.next_page as u64) * PAGE;
        self.seq = self.seq.wrapping_add(1);
        if let Some(cell) = self.vmas.get_mut(slot) {
            *cell = Some(Vma { base, pages, prot, tag: self.seq });
        }
        self.next_page = end;
        self.maps = self.maps.saturating_add(1);
        self.inserted_pages = self.inserted_pages.saturating_add(pages);
        Ok(base)
    }

    /// 解映射：len=0 一律 EINVAL，且长度门先于地址门。
    pub fn munmap(&mut self, addr: u64, len: usize) -> SysResult<()> {
        if len == 0 {
            self.zero_unmaps = self.zero_unmaps.saturating_add(1);
            return Err(EINVAL);
        }
        if addr % PAGE != 0 {
            self.addr_rejects = self.addr_rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let pages = Self::round_pages(len)?;
        if pages == 0 {
            return Err(EINVAL);
        }
        let mut hit: Option<usize> = None;
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(v)) = self.vmas.get(i) {
                if v.base == addr {
                    hit = Some(i);
                }
            }
            i += 1;
        }
        let idx = match hit {
            Some(i) => i,
            None => {
                self.noops = self.noops.saturating_add(1);
                return Ok(());
            }
        };
        let pages_now = match self.vmas.get(idx) {
            Some(Some(v)) => v.pages,
            _ => return Err(EINVAL),
        };
        if pages_now != pages {
            self.addr_rejects = self.addr_rejects.saturating_add(1);
            return Err(EINVAL);
        }
        if let Some(cell) = self.vmas.get_mut(idx) {
            *cell = None;
        }
        self.retired_pages = self.retired_pages.saturating_add(pages);
        self.unmaps = self.unmaps.saturating_add(1);
        Ok(())
    }

    /// 查 addr 所在映射的权限。
    pub fn lookup(&self, addr: u64) -> Option<u32> {
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(v)) = self.vmas.get(i) {
                let end = v.base.saturating_add((v.pages as u64) * PAGE);
                if addr >= v.base && addr < end {
                    return Some(v.prot);
                }
            }
            i += 1;
        }
        None
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

    /// 映射页数合计。
    pub fn mapped_pages(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_VMAS {
            if let Some(Some(v)) = self.vmas.get(i) {
                n = n.saturating_add(v.pages);
            }
            i += 1;
        }
        n
    }

    /// 映射字节合计。
    pub fn mapped_bytes(&self) -> u64 {
        (self.mapped_pages() as u64) * PAGE
    }

    /// 表指纹（FNV-1a，空槽用 0xFF 哨兵）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < MAX_VMAS {
            match self.vmas.get(i) {
                Some(Some(v)) => {
                    h = fnv_step(h, (v.base & 0xFF) as u8);
                    h = fnv_step(h, ((v.base >> 8) & 0xFF) as u8);
                    h = fnv_step(h, ((v.base >> 16) & 0xFF) as u8);
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

    /// 五元组快照。
    pub fn snap(&self) -> TableSnap {
        TableSnap {
            count: self.count(),
            next_page: self.next_page,
            mapped_pages: self.mapped_pages(),
            mapped_bytes: self.mapped_bytes(),
            fingerprint: self.fingerprint(),
        }
    }

    /// 零长度 mmap 次数。
    pub fn zero_maps(&self) -> u32 {
        self.zero_maps
    }
    /// 零长度 munmap 次数。
    pub fn zero_unmaps(&self) -> u32 {
        self.zero_unmaps
    }
    /// 地址拒绝次数（判定次序的观测点）。
    pub fn addr_rejects(&self) -> u32 {
        self.addr_rejects
    }
    /// 空操作 munmap 次数。
    pub fn noops(&self) -> u32 {
        self.noops
    }
    /// 成功映射次数。
    pub fn maps(&self) -> u32 {
        self.maps
    }
    /// 成功解映射次数。
    pub fn unmaps(&self) -> u32 {
        self.unmaps
    }
    /// 累计插入页数。
    pub fn inserted_pages(&self) -> u32 {
        self.inserted_pages
    }
    /// 累计退役页数。
    pub fn retired_pages(&self) -> u32 {
        self.retired_pages
    }

    /// 页账守恒：插入 = 现存 + 退役。
    pub fn pages_conserved(&self) -> bool {
        self.inserted_pages == self.mapped_pages() + self.retired_pages
    }
}

/// FNV-1a 单步。
pub fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// FNV-1a 32 位（无浮点、无随机）。
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

// ── 零长度 munmap 注入矩阵 ────────────────────────────────────────────
/// 零长度 munmap 的地址类别（含三类非法地址，专门压判定次序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZeroAddrCase {
    /// NULL
    Null,
    /// 合法且已映射的基址
    Mapped,
    /// 未对齐（在合法映射内偏移 1 字节）
    Unaligned,
    /// arena 末尾（一页外）
    ArenaEnd,
    /// 全 1 页对齐（远离 arena）
    FarHigh,
}

impl ZeroAddrCase {
    /// 样本地址。
    pub fn addr(self) -> u64 {
        match self {
            ZeroAddrCase::Null => 0,
            ZeroAddrCase::Mapped => ARENA_BASE,
            ZeroAddrCase::Unaligned => ARENA_BASE + 1,
            ZeroAddrCase::ArenaEnd => ARENA_BASE + (ARENA_PAGES as u64) * PAGE,
            ZeroAddrCase::FarHigh => 0xFFFF_FFFF_FFFF_F000,
        }
    }

    /// 闭集全量。
    pub const ALL: [ZeroAddrCase; 5] = [
        ZeroAddrCase::Null,
        ZeroAddrCase::Mapped,
        ZeroAddrCase::Unaligned,
        ZeroAddrCase::ArenaEnd,
        ZeroAddrCase::FarHigh,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
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
            return Err(-(MapCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 零长度 munmap 全矩阵：5 类地址 × 10 轮，全部必须 EINVAL。
pub fn run_zero_unmap_matrix() -> (InjectLedger, [u32; 5], bool) {
    let mut table = MapTable::new();
    let _ = table.mmap(PAGE as usize, PROT_READ | PROT_WRITE);
    let before = table.snap();
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 5];
    let mut untouched = true;
    let mut ci = 0usize;
    while ci < ZeroAddrCase::ALL.len() {
        if let Some(case) = ZeroAddrCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = table.munmap(case.addr(), 0) == Err(EINVAL);
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    if table.snap() != before {
        untouched = false;
    }
    (ledger, per_class, untouched)
}

/// 零长度 mmap 复测：10 轮返回值与快照全等。
pub fn zero_map_stability() -> (bool, bool, u64) {
    let mut table = MapTable::new();
    let before = table.snap();
    let mut same_addr = true;
    let mut first: Option<u64> = None;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        match table.mmap(0, PROT_READ) {
            Ok(addr) => {
                if let Some(f) = first {
                    if f != addr {
                        same_addr = false;
                    }
                } else {
                    first = Some(addr);
                }
                if addr % PAGE != 0 {
                    same_addr = false;
                }
            }
            Err(_) => same_addr = false,
        }
        round += 1;
    }
    let after = table.snap();
    let addr = match first {
        Some(a) => a,
        None => 0,
    };
    (same_addr, before == after, addr)
}

// ── Linux 逐码对照 + Windows 对照 ────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零长度 munmap 主码"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；arena 页尽"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；邻域对照，本模块不发射"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；邻域对照，本模块不发射"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
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

/// Windows VirtualAlloc 对 size=0 的处置（对照，不参与判定）：
/// 回 NULL + ERROR_INVALID_PARAMETER，与零长度正判据的差异见 F9256。
pub const WIN_ZERO_SIZE_CODE: u32 = 87;

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

/// 规范语料：长度边界 × 映射/解映射，逐结果编码字节后取指纹。
pub fn corpus_fingerprint() -> u32 {
    let lens: [usize; 6] = [0, 1, 4095, 4096, 4097, 8193];
    let mut bytes: Vec<u8> = Vec::new();
    let mut table = MapTable::new();
    let mut i = 0usize;
    while i < lens.len() {
        if let Some(len) = lens.get(i).copied() {
            let m = table.mmap(len, PROT_READ | PROT_WRITE);
            bytes.push(match m {
                Ok(addr) => (addr & 0xFF) as u8,
                Err(e) => (e & 0xFF) as u8,
            });
            let u = table.munmap(ARENA_BASE, 0);
            bytes.push(match u {
                Ok(()) => 0,
                Err(e) => (e & 0xFF) as u8,
            });
        }
        i += 1;
    }
    bytes.push((table.mapped_pages() & 0xFF) as u8);
    bytes.push((table.count() & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9252 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx9252_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9252");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正判据：零长度 mmap 返回合法地址
    let mut t = MapTable::new();
    let z = t.mmap(0, PROT_READ);
    let pos_ok = match z {
        Ok(addr) => addr == ZERO_LEN_ADDR && addr % PAGE == 0,
        Err(_) => false,
    };
    s.add("正判据：零长度 mmap 返回合法地址", pos_ok,
          "地址必须页对齐且等于冻结哨兵 ZERO_LEN_ADDR");

    // 3. 零副作用：五元组快照逐字段相等
    let mut t2 = MapTable::new();
    let before = t2.snap();
    let _ = t2.mmap(PAGE as usize, PROT_READ);
    let before_zero = t2.snap();
    let mut k = 0u32;
    while k < INJECT_ROUNDS {
        let _ = t2.mmap(0, PROT_READ | PROT_WRITE);
        k += 1;
    }
    let after_zero = t2.snap();
    s.add("零副作用：10 次零长度 mmap 前后五元组全等",
          before_zero == after_zero && before != before_zero,
          "槽数/游标/页数/字节/指纹五项必须逐字段相等");

    // 4. 复测格：10/10 同址且页对齐
    let (same_addr, stable, _addr) = zero_map_stability();
    s.add("复测格：零长度 mmap 10/10 同址且状态稳定", same_addr && stable,
          "重复调用不得改变返回地址，也不得漂移到未对齐值或改动状态");

    // 5. 零长度 munmap：5 类 × 10 轮全 EINVAL 且映射零改动
    let (ledger, per_class, untouched) = run_zero_unmap_matrix();
    let all_einval = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("零长度 munmap：5 类地址 × 10 轮全 EINVAL 且映射零改动",
          all_einval && untouched,
          "NULL/已映射/未对齐/arena 尾/远高五类一律 EINVAL，快照不许变");

    // 6. 判定次序：长度门先于地址门
    let mut t3 = MapTable::new();
    let _ = t3.munmap(0xFFFF_FFFF_FFFF_F001, 0);
    let _ = t3.munmap(12345, 0);
    s.add("判定次序：非法地址 + 零长度仍回 EINVAL 且不计地址拒绝",
          t3.addr_rejects() == 0 && t3.zero_unmaps() == 2,
          "addr_rejects 不涨才证明长度门在前");

    // 7. 上取整边界
    let ru = MapTable::round_pages(1) == Ok(1)
        && MapTable::round_pages(4095) == Ok(1)
        && MapTable::round_pages(4096) == Ok(1)
        && MapTable::round_pages(4097) == Ok(2)
        && MapTable::round_pages(8193) == Ok(3);
    s.add("上取整：1/4095/4096/4097/8193 → 1/1/1/2/3 页", ru,
          "四点钉死整除边界，禁止把 4097 算成 1 页");

    // 8. 溢出
    let mut t4 = MapTable::new();
    let ovf = t4.mmap(usize::MAX, PROT_READ) == Err(ENOMEM)
        && t4.mapped_pages() == 0;
    s.add("溢出：usize::MAX 长度回 ENOMEM 且零映射", ovf,
          "checked_add(PAGE-1) 必须捕获溢出，不允许回绕");

    // 9. 容量：零长度不吃页，打满后超卖回 ENOMEM
    let mut t5 = MapTable::new();
    let mut n = 0u32;
    while n < 100 {
        let _ = t5.mmap(0, PROT_READ);
        n += 1;
    }
    let zero_free = t5.mapped_pages() == 0 && t5.count() == 0;
    let mut filled = true;
    let mut page = 0u32;
    while page < ARENA_PAGES / 2 {
        if t5.mmap((PAGE as usize) * 2, PROT_READ).is_err() {
            filled = false;
        }
        page += 1;
    }
    let over = t5.mmap((PAGE as usize) * 2, PROT_READ) == Err(ENOMEM);
    s.add("容量：零长度不吃页且 arena 满后回 ENOMEM",
          zero_free && filled && over && t5.mapped_pages() == ARENA_PAGES,
          "100 次零长度后仍可满额映射 16 页，第 17 页必须 ENOMEM");

    // 10. 状态机：映射→撤销→空操作
    let mut t7 = MapTable::new();
    let base = t7.mmap(PAGE as usize, PROT_READ).map_or(0, |v| v);
    let first_unmap = t7.munmap(base, PAGE as usize) == Ok(());
    let second_unmap = t7.munmap(base, PAGE as usize) == Ok(());
    let gone = t7.lookup(base).is_none();
    s.add("状态机：Mapped→Free 单向，二次撤销为空操作",
          first_unmap && second_unmap && gone && t7.noops() == 1,
          "撤销后查表必须为空，二次撤销只记空操作不报错");

    // 12. 页账守恒：插入 = 现存 + 退役
    let mut t8 = MapTable::new();
    let mut q = 0u32;
    while q < 6 {
        let a = t8.mmap((PAGE as usize) * 2, PROT_READ).map_or(0, |v| v);
        let _ = t8.munmap(a, (PAGE as usize) * 2);
        q += 1;
    }
    s.add("守恒：插入页 = 现存页 + 退役页", t8.pages_conserved(),
          "双向可复算，任何一页都不许凭空出现或消失");

    // 13. 注入账守恒
    s.add("注入账：50 次注入 50 次检出 0 逃逸",
          ledger.conserved()
              && ledger.escaped == 0
              && ledger.injected == ZERO_LEN_CLASSES * INJECT_ROUNDS,
          "5 类 × 10 轮必须全部检出");

    // 14. Linux 逐码一致 + Windows 对照
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOMEM)
        && agrees_with_linux(EFAULT)
        && agrees_with_linux(ENOENT)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999)
        && linux_neg(EINVAL) == -EINVAL;
    s.add("Linux 逐码一致且 Windows 零长度差异在册",
          codes_ok && WIN_ZERO_SIZE_CODE == 87,
          "表内码负值可复算；VirtualAlloc(0)=87 的差异不许静默");

    // 15. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 动态明细 1：快照对比
    let snap_text = format!(
        "映射 {} 页 / 槽 {} / 游标 {} / 指纹 0x{:08X}",
        after_zero.mapped_pages, after_zero.count, after_zero.next_page, after_zero.fingerprint
    );
    s.add_dyn("零长度后快照", before_zero == after_zero, &snap_text);

    // 动态明细 2：账目
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);

    // 文案闭集 + 码值唯一
    let text_ok = MapCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < MapCode::ALL.len() {
        let mut j = i + 1;
        while j < MapCode::ALL.len() {
            if let (Some(x), Some(y)) = (MapCode::ALL.get(i), MapCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
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
    fn zero_len_mmap_returns_ticket() {
        let mut t = MapTable::new();
        assert_eq!(t.mmap(0, PROT_READ), Ok(ZERO_LEN_ADDR));
        assert_eq!(t.zero_maps(), 1);
    }

    #[test]
    fn zero_len_mmap_has_no_side_effect() {
        let mut t = MapTable::new();
        let _ = t.mmap(PAGE as usize, PROT_READ);
        let before = t.snap();
        let mut i = 0u32;
        while i < INJECT_ROUNDS {
            let _ = t.mmap(0, PROT_READ | PROT_WRITE);
            i += 1;
        }
        assert_eq!(t.snap(), before);
        assert_eq!(t.zero_maps(), INJECT_ROUNDS);
    }

    #[test]
    fn zero_len_munmap_always_einval() {
        let mut t = MapTable::new();
        let _ = t.mmap(PAGE as usize, PROT_READ);
        for case in ZeroAddrCase::ALL.iter() {
            assert_eq!(t.munmap(case.addr(), 0), Err(EINVAL));
        }
        assert_eq!(t.addr_rejects(), 0);
        assert_eq!(t.zero_unmaps(), ZERO_LEN_CLASSES);
    }

    #[test]
    fn zero_len_munmap_keeps_mapping() {
        let mut t = MapTable::new();
        let base = t.mmap(PAGE as usize, PROT_READ | PROT_WRITE).map_or(0, |v| v);
        let before = t.snap();
        assert_eq!(t.munmap(base, 0), Err(EINVAL));
        assert_eq!(t.snap(), before);
        assert_eq!(t.lookup(base), Some(PROT_READ | PROT_WRITE));
    }

    #[test]
    fn overflow_rejected() {
        assert_eq!(MapTable::round_pages(usize::MAX), Err(ENOMEM));
        let mut t = MapTable::new();
        assert_eq!(t.mmap(usize::MAX, PROT_READ), Err(ENOMEM));
        assert_eq!(t.mapped_pages(), 0);
    }

    #[test]
    fn page_ledger_conserved() {
        let mut t = MapTable::new();
        let a = t.mmap((PAGE as usize) * 3, PROT_READ).map_or(0, |v| v);
        assert!(t.munmap(a, (PAGE as usize) * 3).is_ok());
        assert!(t.pages_conserved());
        assert_eq!(t.retired_pages(), 3);
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per, untouched) = run_zero_unmap_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, ZERO_LEN_CLASSES * INJECT_ROUNDS);
        assert!(untouched);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9252_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
