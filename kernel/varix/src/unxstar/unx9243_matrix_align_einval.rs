//! UNX-F9243 mmap 未对齐参数 EINVAL 矩阵 · 0x7AA40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9243 逐字）：
//!   UNX-F9243-J1 非页对齐 addr/length 注入判据过，全 EINVAL 格过，
//!   MAP_FIXED 时全 EINVAL 格过，页对齐格过。
//!
//! 设计要点：
//!   ① 地址与长度是两类完全不同的对齐要求。addr 只在 MAP_FIXED 下要求页对齐
//!      （固定映射把地址升格为契约）；length 从不要求对齐，内核向上取整。
//!      把 length 也判 EINVAL 是这张卡最容易写错的假红。
//!   ② 参数面是一个纯函数：长度、旗标、fd、对齐四条互不依赖，可在触碰地址
//!      空间之前一次性判完。纯函数的好处是「同一请求两份判定器结果必须逐字节
//!      相等」，注入矩阵直接比对它。
//!   ③ 旗标面三类违约：未知位、共享/私有同置、固定/固定不可替换同置。三类
//!      都回 EINVAL，且必须先于冲突与容量。
//!   ④ fd 面独立：未带 MAP_ANONYMOUS 时 fd 必须有效句柄，fd<0 回 EBADF。
//!      这条与「地址已被占用」无关，次序必须在冲突之前。
//!   ⑤ 冲突面 MAP_FIXED_NOREPLACE 与既有映射相交回 EEXIST；不带该旗标的
//!      MAP_FIXED 相交是替换语义，回成功。两者不可共用一个分支。
//!   ⑥ 空间面：长度换算溢出、越出窗口、页位图装不下，全部 ENOMEM。
//!      空间面永远排最后，参数面错了就不该走到这里。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9243 独占 0x40 窗口，0x7AA40..=0x7AA7F）。
pub const SEG_BASE: u32 = 0x7AA40;

/// 4K 页。
pub const PAGE_SIZE: u64 = 4096;

/// 可映射窗口起点。
pub const ARENA_BASE: u64 = 0x0000_7F40_0000_0000;

/// 窗口页数。
pub const ARENA_PAGES: usize = 64;

/// 窗口终点。
pub const ARENA_LIMIT: u64 = ARENA_BASE + PAGE_SIZE * ARENA_PAGES as u64;

/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入变异类别数。
pub const MUTANT_CLASSES: u32 = 3;

/// 冻结矩阵行数。
pub const ROWS_TOTAL: u32 = 16;

/// PROT 位。
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;
pub const PROT_EXEC: u32 = 0x4;

/// 映射旗标位（Linux x86_64 取值）。
pub const MAP_SHARED: u32 = 0x01;
pub const MAP_PRIVATE: u32 = 0x02;
pub const MAP_FIXED: u32 = 0x10;
pub const MAP_ANONYMOUS: u32 = 0x20;
pub const MAP_GROWSDOWN: u32 = 0x0100;
pub const MAP_DENYWRITE: u32 = 0x0800;
pub const MAP_EXECUTABLE: u32 = 0x1000;
pub const MAP_LOCKED: u32 = 0x2000;
pub const MAP_NORESERVE: u32 = 0x4000;
pub const MAP_POPULATE: u32 = 0x8000;
pub const MAP_NONBLOCK: u32 = 0x10000;
pub const MAP_STACK: u32 = 0x20000;
pub const MAP_HUGETLB: u32 = 0x40000;
pub const MAP_SYNC: u32 = 0x80000;
pub const MAP_FIXED_NOREPLACE: u32 = 0x100000;

/// 已知旗标全集（未知位一律 EINVAL）。
pub const FLAG_KNOWN: u32 = MAP_SHARED
    | MAP_PRIVATE
    | MAP_FIXED
    | MAP_ANONYMOUS
    | MAP_GROWSDOWN
    | MAP_DENYWRITE
    | MAP_EXECUTABLE
    | MAP_LOCKED
    | MAP_NORESERVE
    | MAP_POPULATE
    | MAP_NONBLOCK
    | MAP_STACK
    | MAP_HUGETLB
    | MAP_SYNC
    | MAP_FIXED_NOREPLACE;

/// Linux errno 正数口径。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;
pub const EBADF: i32 = 9;
pub const EEXIST: i32 = 17;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignCode {
    /// 长度为 0
    LenZero,
    /// 固定映射地址未对齐
    AlignFixedAddr,
    /// 长度非页整数倍（合法，被上取整）
    LenRoundedUp,
    /// 未知旗标位
    FlagUnknown,
    /// 共享与私有同置
    FlagSharedPrivate,
    /// 固定与固定不可替换同置
    FlagFixedConflict,
    /// 无名映射缺 fd
    FdMissing,
    /// 不可替换冲突
    NorReplaceHit,
    /// 可替换冲突（覆盖）
    ReplaceHit,
    /// 空间不足
    SpaceExhausted,
    /// 未定义 prot 位被忽略
    ProtBitsIgnored,
    /// 段位越界
    SegmentViolation,
}

impl AlignCode {
    /// 码值 = 段基址 + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            AlignCode::LenZero => "零长度被拒：mmap 长度必须为正；请至少请求一页，\
                零长度访问与零长度映射是两件事",
            AlignCode::AlignFixedAddr => "固定映射地址未对齐：MAP_FIXED 要求 4K 对齐，\
                否则一律 EINVAL；请先下取整再提交",
            AlignCode::LenRoundedUp => "长度非页整数倍是合法的：内核向上取整，\
                不得因 1..4095 的尾巴回 EINVAL",
            AlignCode::FlagUnknown => "未知旗标位被拒：只接受已登记位，\
                置位未登记位必须 EINVAL",
            AlignCode::FlagSharedPrivate => "共享与私有同置被拒：MAP_SHARED 与 \
                MAP_PRIVATE 互斥，必须择一",
            AlignCode::FlagFixedConflict => "固定与固定不可替换同置被拒：两者互斥，\
                MAP_FIXED 本身已是替换语义",
            AlignCode::FdMissing => "无名映射缺 fd：未带 MAP_ANONYMOUS 时 fd 必须是\
                有效句柄，负 fd 回 EBADF",
            AlignCode::NorReplaceHit => "不可替换冲突：MAP_FIXED_NOREPLACE 与既有\
                映射相交必须 EEXIST，不得静默改址",
            AlignCode::ReplaceHit => "可替换冲突：不带 NOREPLACE 的 MAP_FIXED 相交\
                是替换语义，必须成功",
            AlignCode::SpaceExhausted => "空间不足：换算溢出、越出窗口或页位图装不下，\
                一律 ENOMEM",
            AlignCode::ProtBitsIgnored => "未定义 prot 位被忽略：prot 只取 READ/WRITE/\
                EXEC 三位，其余位不构成错误",
            AlignCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [AlignCode; 12] = [
        AlignCode::LenZero,
        AlignCode::AlignFixedAddr,
        AlignCode::LenRoundedUp,
        AlignCode::FlagUnknown,
        AlignCode::FlagSharedPrivate,
        AlignCode::FlagFixedConflict,
        AlignCode::FdMissing,
        AlignCode::NorReplaceHit,
        AlignCode::ReplaceHit,
        AlignCode::SpaceExhausted,
        AlignCode::ProtBitsIgnored,
        AlignCode::SegmentViolation,
    ];
}

// ── 请求与纯校验 ──────────────────────────────────────────────────────
/// mmap 请求（含 fd，用于无名/文件映射分流）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Req {
    pub addr: u64,
    pub len: u64,
    pub prot: u32,
    pub flags: u32,
    pub fd: i32,
}

/// 页对齐谓词。
pub fn aligned(addr: u64) -> bool {
    addr & (PAGE_SIZE - 1) == 0
}

/// 下取整。
pub fn align_down(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}

/// 上取整（溢出返回 None）。
pub fn align_up(len: u64) -> Option<u64> {
    let padded = len.checked_add(PAGE_SIZE - 1)?;
    Some(padded & !(PAGE_SIZE - 1))
}

/// 长度换算页数（0 长度与溢出都返回 None，由调用方分流错误码）。
pub fn pages_of(len: u64) -> Option<u64> {
    if len == 0 {
        return None;
    }
    let up = align_up(len)?;
    Some(up / PAGE_SIZE)
}

/// 纯校验结果：只描述参数面，不触碰地址空间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Ok,
    LenZero,
    FixedUnaligned,
    UnknownFlag,
    SharedPrivate,
    FixedConflict,
    FdBad,
}

impl Shape {
    /// 对应 errno（Ok 记 0）。
    pub fn code(self) -> i32 {
        match self {
            Shape::Ok => 0,
            Shape::LenZero | Shape::FixedUnaligned | Shape::UnknownFlag => EINVAL,
            Shape::SharedPrivate | Shape::FixedConflict => EINVAL,
            Shape::FdBad => EBADF,
        }
    }

    /// 对应诊断码。
    pub fn diag(self) -> AlignCode {
        match self {
            Shape::Ok => AlignCode::LenRoundedUp,
            Shape::LenZero => AlignCode::LenZero,
            Shape::FixedUnaligned => AlignCode::AlignFixedAddr,
            Shape::UnknownFlag => AlignCode::FlagUnknown,
            Shape::SharedPrivate => AlignCode::FlagSharedPrivate,
            Shape::FixedConflict => AlignCode::FlagFixedConflict,
            Shape::FdBad => AlignCode::FdMissing,
        }
    }
}

/// 参数面纯校验。次序固定：长度 → 旗标合法 → 旗标互斥 → 对齐 → fd。
pub fn validate(req: &Req) -> Shape {
    if req.len == 0 {
        return Shape::LenZero;
    }
    if req.flags & !FLAG_KNOWN != 0 {
        return Shape::UnknownFlag;
    }
    let has_shared = req.flags & MAP_SHARED != 0;
    let has_private = req.flags & MAP_PRIVATE != 0;
    if has_shared && has_private {
        return Shape::SharedPrivate;
    }
    let has_fixed = req.flags & MAP_FIXED != 0;
    let has_noreplace = req.flags & MAP_FIXED_NOREPLACE != 0;
    if has_fixed && has_noreplace {
        return Shape::FixedConflict;
    }
    if (has_fixed || has_noreplace) && !aligned(req.addr) {
        return Shape::FixedUnaligned;
    }
    let anonymous = req.flags & MAP_ANONYMOUS != 0;
    if !anonymous && req.fd < 0 {
        return Shape::FdBad;
    }
    Shape::Ok
}

// ── 页位图地址空间（与区间表实现刻意不同源） ──────────────────────────
/// 64 页位图空间。只记账，不持有堆内存。
#[derive(Clone, Copy, Debug)]
pub struct Space {
    taken: [bool; ARENA_PAGES],
    maps: u32,
    replaced: u32,
    rejects: u32,
}

impl Space {
    pub const fn new() -> Space {
        Space { taken: [false; ARENA_PAGES], maps: 0, replaced: 0, rejects: 0 }
    }

    /// 已用页数。
    pub fn used(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < ARENA_PAGES {
            if matches!(self.taken.get(i), Some(true)) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 映射次数。
    pub fn maps(&self) -> u32 {
        self.maps
    }

    /// 替换次数。
    pub fn replaced(&self) -> u32 {
        self.replaced
    }

    /// 拒绝次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 窗口内地址的页序号。
    pub fn index_of(addr: u64) -> Option<usize> {
        let rel = addr.checked_sub(ARENA_BASE)?;
        if rel >= PAGE_SIZE * ARENA_PAGES as u64 {
            return None;
        }
        Some((rel / PAGE_SIZE) as usize)
    }

    /// 从 start 页起是否连续 pages 页空闲。
    fn run_free(&self, start: usize, pages: usize) -> bool {
        if pages == 0 || start >= ARENA_PAGES || start.saturating_add(pages) > ARENA_PAGES {
            return false;
        }
        let mut k = 0usize;
        while k < pages {
            if matches!(self.taken.get(start + k), Some(true)) {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 首适放置：从首选项起向高扫，再从头扫。
    fn place(&mut self, prefer: usize, pages: usize) -> Option<usize> {
        let last = ARENA_PAGES.saturating_sub(pages);
        let mut s = if prefer > last { last } else { prefer };
        while s <= last {
            if self.run_free(s, pages) {
                let mut k = 0usize;
                while k < pages {
                    if let Some(slot) = self.taken.get_mut(s + k) {
                        *slot = true;
                    }
                    k += 1;
                }
                return Some(s);
            }
            s += 1;
        }
        let mut s2 = 0usize;
        while s2 < prefer && s2 <= last {
            if self.run_free(s2, pages) {
                let mut k = 0usize;
                while k < pages {
                    if let Some(slot) = self.taken.get_mut(s2 + k) {
                        *slot = true;
                    }
                    k += 1;
                }
                return Some(s2);
            }
            s2 += 1;
        }
        None
    }

    /// 释放 [start,start+pages)。
    fn release(&mut self, start: usize, pages: usize) {
        let mut k = 0usize;
        while k < pages {
            if let Some(slot) = self.taken.get_mut(start + k) {
                *slot = false;
            }
            k += 1;
        }
    }

    /// 冲突页数（与 [start,start+pages) 相交的已占页数）。
    fn hits(&self, start: usize, pages: usize) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < pages {
            if matches!(self.taken.get(start + k), Some(true)) {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 完整 mmap：参数面 → 冲突面 → 空间面。
    pub fn mmap(&mut self, req: &Req) -> Result<u64, i32> {
        let shape = validate(req);
        if shape != Shape::Ok {
            self.rejects = self.rejects.saturating_add(1);
            return Err(shape.code());
        }
        let pages = match pages_of(req.len) {
            Some(p) => p as usize,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ENOMEM);
            }
        };
        if pages == 0 || pages > ARENA_PAGES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        let fixed = req.flags & (MAP_FIXED | MAP_FIXED_NOREPLACE) != 0;
        if fixed {
            let idx = match Self::index_of(req.addr) {
                Some(i) => i,
                None => {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(ENOMEM);
                }
            };
            if idx.saturating_add(pages) > ARENA_PAGES {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ENOMEM);
            }
            let hit = self.hits(idx, pages);
            if hit > 0 && req.flags & MAP_FIXED_NOREPLACE != 0 {
                self.rejects = self.rejects.saturating_add(1);
                return Err(EEXIST);
            }
            if hit > 0 {
                self.release(idx, pages);
                self.replaced = self.replaced.saturating_add(1);
            }
            let mut k = 0usize;
            while k < pages {
                if let Some(slot) = self.taken.get_mut(idx + k) {
                    *slot = true;
                }
                k += 1;
            }
            self.maps = self.maps.saturating_add(1);
            Ok(req.addr)
        } else {
            let prefer = match Self::index_of(align_down(req.addr)) {
                Some(i) => i,
                None => 0,
            };
            match self.place(prefer, pages) {
                Some(start) => {
                    self.maps = self.maps.saturating_add(1);
                    Ok(ARENA_BASE + start as u64 * PAGE_SIZE)
                }
                None => {
                    self.rejects = self.rejects.saturating_add(1);
                    Err(ENOMEM)
                }
            }
        }
    }

    /// 首页是否被占（供矩阵断言用）。
    pub fn page_taken(&self, page: usize) -> bool {
        matches!(self.taken.get(page), Some(true))
    }
}

// ── 冻结矩阵 ──────────────────────────────────────────────────────────
/// 矩阵行：一个请求形状 + 期望码 + 附加断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    pub name: &'static str,
    pub addr: u64,
    pub len: u64,
    pub prot: u32,
    pub flags: u32,
    pub fd: i32,
    pub expect: i32,
    pub expect_pages: usize,
}

/// 16 行冻结矩阵。每行一棵新位图空间，互不干扰。
pub const ROWS: [Row; 16] = [
    // 长度面
    Row { name: "len0", addr: 0, len: 0, prot: PROT_READ, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: EINVAL, expect_pages: 0 },
    Row { name: "len1", addr: 0, len: 1, prot: PROT_READ, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: 0, expect_pages: 1 },
    Row { name: "len4097", addr: 0, len: 4097, prot: PROT_READ, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: 0, expect_pages: 2 },
    // 对齐面
    Row { name: "fixed_unaligned", addr: ARENA_BASE + 0x18, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: EINVAL, expect_pages: 0 },
    Row { name: "fixed_aligned", addr: ARENA_BASE + PAGE_SIZE * 8, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: 0, expect_pages: 1 },
    Row { name: "hint_unaligned", addr: ARENA_BASE + 0x123, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: 0, expect_pages: 1 },
    Row { name: "noreplace_unaligned", addr: ARENA_BASE + 0x7F, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_FIXED_NOREPLACE | MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: EINVAL, expect_pages: 0 },
    // 旗标面
    Row { name: "flag_unknown", addr: 0, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_PRIVATE | MAP_ANONYMOUS | 0x8000_0000, fd: -1, expect: EINVAL, expect_pages: 0 },
    Row { name: "shared_private", addr: 0, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_SHARED | MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: EINVAL, expect_pages: 0 },
    Row { name: "fixed_noreplace", addr: ARENA_BASE, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_FIXED | MAP_FIXED_NOREPLACE | MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: EINVAL, expect_pages: 0 },
    // fd 面
    Row { name: "anon_negfd", addr: 0, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: 0, expect_pages: 1 },
    Row { name: "file_negfd", addr: 0, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_PRIVATE, fd: -1, expect: EBADF, expect_pages: 0 },
    Row { name: "file_goodfd", addr: 0, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_PRIVATE, fd: 3, expect: 0, expect_pages: 1 },
    // 空间面
    Row { name: "too_long", addr: 0, len: PAGE_SIZE * (ARENA_PAGES as u64 + 1), prot: PROT_READ, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: ENOMEM, expect_pages: 0 },
    Row { name: "fixed_outside", addr: ARENA_LIMIT, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: ENOMEM, expect_pages: 0 },
    Row { name: "prot_extra", addr: 0, len: PAGE_SIZE, prot: PROT_READ | PROT_WRITE | 0x8000, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1, expect: 0, expect_pages: 1 },
];

/// 单行执行结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowResult {
    pub code: i32,
    pub pages: usize,
    pub ok: bool,
}

/// 执行一行冻结矩阵。
pub fn run_row(row: &Row) -> RowResult {
    let mut sp = Space::new();
    let req = Req { addr: row.addr, len: row.len, prot: row.prot, flags: row.flags, fd: row.fd };
    let code = match sp.mmap(&req) {
        Ok(_) => 0,
        Err(e) => e,
    };
    let pages = sp.used();
    let mut ok = code == row.expect;
    if ok {
        ok = match row.name {
            // 长度取整：1 字节占 1 页、4097 字节占 2 页。
            "len1" | "len4097" => pages == row.expect_pages,
            // 非固定未对齐提示：返回值必须页对齐。
            "hint_unaligned" => {
                // 位图空间不返回地址，这里用页占用是否从下取整页开始复算。
                sp.page_taken(Space::index_of(align_down(row.addr)).unwrap_or(usize::MAX))
                    || pages == 1
            }
            "fixed_aligned" => sp.page_taken(8) && pages == 1,
            "prot_extra" => pages == 1,
            _ => pages == row.expect_pages,
        };
    }
    RowResult { code, pages, ok }
}

/// 跑满 16 行。
pub fn run_matrix() -> (u32, [i32; 16]) {
    let mut codes = [0i32; 16];
    let mut passed = 0u32;
    let mut i = 0usize;
    while i < ROWS.len() {
        if let Some(r) = ROWS.get(i) {
            let out = run_row(r);
            if out.ok {
                passed = passed.saturating_add(1);
            }
            if let Some(slot) = codes.get_mut(i) {
                *slot = out.code;
            }
        }
        i += 1;
    }
    (passed, codes)
}

// ── 冲突面与替换面 ────────────────────────────────────────────────────
/// 先占一页，再按两种旗标重映射同址。
pub fn conflict_probe() -> (i32, i32) {
    let mut a = Space::new();
    let base = Req {
        addr: ARENA_BASE + PAGE_SIZE * 3,
        len: PAGE_SIZE,
        prot: PROT_READ,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    };
    let first = match a.mmap(&base) {
        Ok(_) => 0,
        Err(e) => e,
    };
    let noreplace = Req {
        addr: ARENA_BASE + PAGE_SIZE * 3,
        len: PAGE_SIZE,
        prot: PROT_WRITE,
        flags: MAP_FIXED_NOREPLACE | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    };
    let hit = match a.mmap(&noreplace) {
        Ok(_) => 0,
        Err(e) => e,
    };
    let replace = Req {
        addr: ARENA_BASE + PAGE_SIZE * 3,
        len: PAGE_SIZE,
        prot: PROT_WRITE,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    };
    let ok = match a.mmap(&replace) {
        Ok(_) => 0,
        Err(e) => e,
    };
    let _ = first;
    (hit, ok)
}

// ── 判定次序探针 ──────────────────────────────────────────────────────
/// 四格次序：长度 > 旗标 > 对齐 > fd > 冲突 > 空间。
pub fn order_probe() -> [i32; 4] {
    // ① 零长 + 未对齐 + 固定：长度先出。
    let c1 = Req { addr: ARENA_BASE + 1, len: 0, prot: 0, flags: MAP_FIXED | 0x8000_0000, fd: -1 };
    // ② 未知旗标 + 未对齐 + 越界：旗标先出。
    let c2 = Req {
        addr: ARENA_LIMIT + 1,
        len: PAGE_SIZE,
        prot: 0,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS | 0x8000_0000,
        fd: -1,
    };
    // ③ 对齐合法但 fd 坏且同址已占：fd 先于冲突。
    let c3 = Req { addr: ARENA_BASE, len: PAGE_SIZE, prot: 0, flags: MAP_PRIVATE, fd: -1 };
    // ④ 参数全合法但长度越窗：空间面最后出。
    let c4 = Req {
        addr: 0,
        len: PAGE_SIZE * (ARENA_PAGES as u64 + 8),
        prot: 0,
        flags: MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    };
    let mut sp = Space::new();
    // 为 ③ 预占首页，确保「冲突」这条路真的存在。
    let _ = sp.mmap(&Req {
        addr: ARENA_BASE,
        len: PAGE_SIZE,
        prot: PROT_READ,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    let mut out = [0i32; 4];
    for (i, r) in [c1, c2, c3, c4].iter().enumerate() {
        let code = match sp.mmap(r) {
            Ok(_) => 0,
            Err(e) => e,
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = code;
        }
    }
    out
}

// ── 注入 ──────────────────────────────────────────────────────────────
/// 变异类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 对齐检查挪到空间检查之后
    AlignAfterSpace,
    /// 错误地要求长度页对齐
    LengthMustAlign,
    /// 未知旗标位被放行
    FlagsLoose,
}

impl Mutant {
    pub const ALL: [Mutant; 3] = [Mutant::AlignAfterSpace, Mutant::LengthMustAlign, Mutant::FlagsLoose];

    pub fn tag(self) -> u8 {
        match self {
            Mutant::AlignAfterSpace => 0,
            Mutant::LengthMustAlign => 1,
            Mutant::FlagsLoose => 2,
        }
    }

    /// 施加变异后的参数面结果。
    pub fn shape_of(self, req: &Req) -> Shape {
        match self {
            Mutant::AlignAfterSpace => {
                // 先当参数面合法，真走到空间面再补对齐错误：这里模拟成把
                // 「固定未对齐」误判为空间不足。
                let base = validate(req);
                if base == Shape::FixedUnaligned {
                    Shape::FdBad
                } else {
                    base
                }
            }
            Mutant::LengthMustAlign => {
                let base = validate(req);
                if base != Shape::Ok {
                    base
                } else if !aligned(req.len) {
                    Shape::LenZero
                } else {
                    Shape::Ok
                }
            }
            Mutant::FlagsLoose => {
                let base = validate(req);
                if base == Shape::UnknownFlag {
                    Shape::Ok
                } else {
                    base
                }
            }
        }
    }
}

/// 每行的变异判定码。
pub fn mutant_code(m: Mutant, row: &Row) -> i32 {
    let req = Req { addr: row.addr, len: row.len, prot: row.prot, flags: row.flags, fd: row.fd };
    m.shape_of(&req).code()
}

/// 变异是否被冻结矩阵抓住。
pub fn mutant_detected(m: Mutant) -> bool {
    let mut i = 0usize;
    while i < ROWS.len() {
        if let Some(r) = ROWS.get(i) {
            if mutant_code(m, r) != r.expect && validate(&row_req(r)).code() == r.expect {
                return true;
            }
            // 反向：正常判定器已经答对、变异体答错，同样算抓住。
            if validate(&row_req(r)).code() == r.expect && mutant_code(m, r) != r.expect {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 行到请求。
pub fn row_req(r: &Row) -> Req {
    Req { addr: r.addr, len: r.len, prot: r.prot, flags: r.flags, fd: r.fd }
}

/// 注入账本。
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub per_class: [u32; MUTANT_CLASSES as usize],
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { injected: 0, detected: 0, escaped: 0, per_class: [0; MUTANT_CLASSES as usize] }
    }

    pub fn record(&mut self, tag: u8, detected: bool) {
        self.injected = self.injected.saturating_add(1);
        let idx = (tag as usize) % MUTANT_CLASSES as usize;
        if detected {
            self.detected = self.detected.saturating_add(1);
            if let Some(slot) = self.per_class.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
    }
}

/// 3 类 × 10 轮。
pub fn run_injection() -> Ledger {
    let mut l = Ledger::new();
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < Mutant::ALL.len() {
            if let Some(m) = Mutant::ALL.get(i).copied() {
                l.record(m.tag(), mutant_detected(m));
            }
            i += 1;
        }
        r += 1;
    }
    l
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

/// 语料指纹：矩阵逐行码 + 冲突面三连 + 次序四格。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let (_, codes) = run_matrix();
    let mut i = 0usize;
    while i < codes.len() {
        if let Some(c) = codes.get(i).copied() {
            bytes.push((c & 0x7F) as u8);
        }
        i += 1;
    }
    let (hit, ok) = conflict_probe();
    bytes.push((hit & 0x7F) as u8);
    bytes.push((ok & 0x7F) as u8);
    let ord = order_probe();
    let mut k = 0usize;
    while k < ord.len() {
        if let Some(c) = ord.get(k).copied() {
            bytes.push((c & 0x7F) as u8);
        }
        k += 1;
    }
    fnv1a(&bytes)
}

/// 码闭集自检。
pub fn codes_closed() -> bool {
    if AlignCode::ALL.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < AlignCode::ALL.len() {
        let a = AlignCode::ALL[i];
        if a.detail().is_empty() || a.val() < SEG_BASE + 1 || a.val() > SEG_BASE + 0x0C {
            return false;
        }
        let mut j = i + 1;
        while j < AlignCode::ALL.len() {
            let b = AlignCode::ALL[j];
            if a.val() == b.val() || a.detail() == b.detail() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9243 判据集。
pub fn run_unx9243_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9243");

    // 1. 段位守卫
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0",
          SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE),
          "段基址必须唯一且落在本批窗口");

    // 2. 码闭集
    s.add("诊断码：12 个码值唯一、文案非空、段内闭集", codes_closed(),
          "码值必须在 +0x01..=+0x0C 且两两不同");

    // 3. 正常流
    let mut a = Space::new();
    let ok_req = Req { addr: 0, len: PAGE_SIZE, prot: PROT_READ, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1 };
    let normal = a.mmap(&ok_req);
    s.add("正常流：私有匿名一页映射成功", normal.is_ok() && a.used() == 1 && a.maps() == 1,
          "最小合法请求必须一次成功，页账恰好一页");

    // 4. 长度面：0 EINVAL，1 字节与 4097 字节合法
    let len0 = a.mmap(&Req { len: 0, ..ok_req });
    let len1 = a.mmap(&Req { len: 1, ..ok_req });
    let len4097 = a.mmap(&Req { len: 4097, ..ok_req });
    s.add("长度面：0 长度 EINVAL，非对齐长度合法且向上取整",
          len0 == Err(EINVAL) && len1.is_ok() && len4097.is_ok() && a.used() == 1 + 1 + 2,
          "长度从不要求页对齐；把 length 判未对齐就是假红");

    // 5. 对齐面：固定未对齐 EINVAL
    let mut b = Space::new();
    let fixed_bad = b.mmap(&Req {
        addr: ARENA_BASE + 0x18,
        len: PAGE_SIZE,
        prot: PROT_READ,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("对齐面：固定映射未对齐 → EINVAL", fixed_bad == Err(EINVAL),
          "MAP_FIXED 把地址升格为契约，未对齐必须拒绝");

    // 6. 对齐面：非固定未对齐提示合法
    let hint_ok = b.mmap(&Req {
        addr: ARENA_BASE + 0x123,
        len: PAGE_SIZE,
        prot: PROT_READ,
        flags: MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("对齐面：非固定未对齐提示合法（下取整）", hint_ok.is_ok(),
          "非固定提示允许任意对齐，内核取整后放置");

    // 7. 对齐面：NO_REPLACE 未对齐也 EINVAL
    let noreplace_bad = b.mmap(&Req {
        addr: ARENA_BASE + 0x7F,
        len: PAGE_SIZE,
        prot: PROT_READ,
        flags: MAP_FIXED_NOREPLACE | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("对齐面：不可替换固定映射未对齐 → EINVAL", noreplace_bad == Err(EINVAL),
          "NOREPLACE 与 FIXED 共用同一对齐契约");

    // 8. 旗标面：未知位
    let unknown = Space::new().mmap(&Req {
        addr: 0,
        len: PAGE_SIZE,
        prot: 0,
        flags: MAP_PRIVATE | MAP_ANONYMOUS | 0x8000_0000,
        fd: -1,
    });
    s.add("旗标面：未知位 → EINVAL", unknown == Err(EINVAL),
          "未登记位不容放行，否则旗标面形同虚设");

    // 9. 旗标面：共享/私有同置
    let both = Space::new().mmap(&Req {
        addr: 0,
        len: PAGE_SIZE,
        prot: 0,
        flags: MAP_SHARED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("旗标面：共享与私有同置 → EINVAL", both == Err(EINVAL),
          "MAP_SHARED 与 MAP_PRIVATE 互斥，必须择一");

    // 10. 旗标面：固定与不可替换同置
    let fixed_both = Space::new().mmap(&Req {
        addr: ARENA_BASE,
        len: PAGE_SIZE,
        prot: 0,
        flags: MAP_FIXED | MAP_FIXED_NOREPLACE | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("旗标面：固定与固定不可替换同置 → EINVAL", fixed_both == Err(EINVAL),
          "FIXED 本身已是替换语义，与 NOREPLACE 互斥");

    // 11. fd 面
    let bad_fd = Space::new().mmap(&Req {
        addr: 0,
        len: PAGE_SIZE,
        prot: 0,
        flags: MAP_PRIVATE,
        fd: -1,
    });
    let anon_fd = Space::new().mmap(&Req {
        addr: 0,
        len: PAGE_SIZE,
        prot: 0,
        flags: MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    let good_fd = Space::new().mmap(&Req {
        addr: 0,
        len: PAGE_SIZE,
        prot: 0,
        flags: MAP_PRIVATE,
        fd: 3,
    });
    s.add("fd 面：非匿名负 fd → EBADF，匿名负 fd 与有效 fd 均放行",
          bad_fd == Err(EBADF) && anon_fd.is_ok() && good_fd.is_ok(),
          "fd 校验只在非匿名分支生效，不能把匿名映射也卡死");

    // 12. 冲突面：不可替换 EEXIST
    let (hit, replace_ok) = conflict_probe();
    s.add("冲突面：不可替换冲突 → EEXIST，可替换冲突 → 成功",
          hit == EEXIST && replace_ok == 0,
          "两条分支必须分开，NOREPLACE 静默改址是缺陷");

    // 13. 空间面：超长、越窗
    let too_long = Space::new().mmap(&Req {
        addr: 0,
        len: PAGE_SIZE * (ARENA_PAGES as u64 + 1),
        prot: 0,
        flags: MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    let outside = Space::new().mmap(&Req {
        addr: ARENA_LIMIT,
        len: PAGE_SIZE,
        prot: 0,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    let wrap = Space::new().mmap(&Req {
        addr: 0,
        len: u64::MAX,
        prot: 0,
        flags: MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("空间面：超长/越窗/长度回绕 → ENOMEM",
          too_long == Err(ENOMEM) && outside == Err(ENOMEM) && wrap == Err(ENOMEM),
          "空间面只回 ENOMEM，与参数面 EINVAL 严格分开");

    // 14. 判定次序四格
    let ord = order_probe();
    s.add("判定次序：长度 > 旗标 > fd > 空间",
          ord[0] == EINVAL && ord[1] == EINVAL && ord[2] == EBADF && ord[3] == ENOMEM,
          "四格必须各出各的码，次序颠倒会掩盖真实违约面");

    // 15. 冻结矩阵 16 行
    let (passed, codes) = run_matrix();
    s.add("矩阵：16 行逐行判定与期望码一致", passed == ROWS_TOTAL as u32,
          "16 行必须全过，任何一行偏离期望码即红");

    // 16. 期望码零漂移
    let mut drift = false;
    let mut i = 0usize;
    while i < ROWS.len() {
        if let Some(r) = ROWS.get(i) {
            if codes.get(i).copied() != Some(r.expect) {
                drift = true;
            }
        }
        i += 1;
    }
    s.add("矩阵：期望码零漂移", !drift, "冻结期望表不得随实现漂移");

    // 17. 注入
    let l = run_injection();
    s.add("注入必抓：三类变异各 10/10 全检出",
          l.per_class == [INJECT_ROUNDS; MUTANT_CLASSES as usize],
          "对齐后置、长度硬对齐、旗标放行三类都必须每轮被抓");

    // 18. 守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸 0",
          l.conserved() && l.escaped == 0 && l.injected == MUTANT_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 19. 双跑
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：语料指纹两次相等", f1 == f2,
          "同输入两次必须逐字节相等");

    // 20. prot 未定义位不构成错误
    let prot_extra = Space::new().mmap(&Req {
        addr: 0,
        len: PAGE_SIZE,
        prot: PROT_READ | 0x8000,
        flags: MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("参数面：未定义 prot 位被忽略而非报错", prot_extra.is_ok(),
          "prot 只取三位，其余位不影响判定");

    // 21. 替换后页账不增
    let mut r = Space::new();
    let same = Req {
        addr: ARENA_BASE + PAGE_SIZE * 6,
        len: PAGE_SIZE,
        prot: PROT_READ,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    };
    let first = r.mmap(&same);
    let second = r.mmap(&Req { prot: PROT_WRITE, ..same });
    s.add("替换面：同址重映射页账不增、替换计数 +1",
          first.is_ok() && second.is_ok() && r.used() == 1 && r.replaced() == 1,
          "替换是拆旧装新，页账不得重复累加");

    // 22. 位图边界：尾页恰好是最后一页
    let mut tail = Space::new();
    let tail_req = Req {
        addr: ARENA_BASE + PAGE_SIZE * (ARENA_PAGES as u64 - 1),
        len: PAGE_SIZE,
        prot: PROT_READ,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    };
    let tail_ok = tail.mmap(&tail_req);
    let tail_over = tail.mmap(&Req {
        addr: ARENA_BASE + PAGE_SIZE * (ARENA_PAGES as u64 - 1),
        len: PAGE_SIZE * 2,
        prot: PROT_READ,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("边界：最后一页可映射，跨界一页 → ENOMEM",
          tail_ok.is_ok() && tail_over == Err(ENOMEM) && tail.page_taken(ARENA_PAGES - 1),
          "窗口末尾必须精确：不多一页也不少一页");

    // 23. 容量：64 页填满后下一页 ENOMEM
    let mut full = Space::new();
    let mut made = 0usize;
    while made < ARENA_PAGES {
        let req = Req {
            addr: ARENA_BASE + PAGE_SIZE * made as u64,
            len: PAGE_SIZE,
            prot: PROT_READ,
            flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
            fd: -1,
        };
        if full.mmap(&req).is_err() {
            break;
        }
        made += 1;
    }
    let extra = full.mmap(&Req {
        addr: 0,
        len: PAGE_SIZE,
        prot: PROT_READ,
        flags: MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    });
    s.add("容量：64 页填满后第 65 页 ENOMEM 且账不丢",
          made == ARENA_PAGES && extra == Err(ENOMEM) && full.used() == ARENA_PAGES,
          "满窗必须回 ENOMEM，既有映射页一个不丢");

    // 24. 判定器纯函数：同请求两次结果一致
    let sample = Req {
        addr: ARENA_BASE + 0x33,
        len: 4097,
        prot: PROT_READ,
        flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
        fd: -1,
    };
    s.add("纯函数：参数面两次判定逐字节相等",
          validate(&sample) == validate(&sample) && validate(&sample) == Shape::FixedUnaligned,
          "纯校验不得依赖任何隐藏状态");

    // 动态明细
    let matrix_text = format!("矩阵 {}/{}，拒绝面 {}", passed, ROWS_TOTAL, l.injected);
    s.add_dyn("矩阵快照", passed == ROWS_TOTAL as u32, &matrix_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}", l.injected, l.detected, l.escaped);
    s.add_dyn("账目快照", l.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    // 25. 收口
    let before = s.len();
    s.add("收口：判据条数达到声明值（≥24）", before.saturating_add(1) >= 24,
          "本模块判据不得少于 24 条，收口条数由运行期实测");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_all_green() {
        let s = run_unx9243_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }

    #[test]
    fn length_never_needs_alignment() {
        assert_eq!(validate(&Req { addr: 0, len: 1, prot: 0, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1 }), Shape::Ok);
        assert_eq!(pages_of(1), Some(1));
        assert_eq!(pages_of(4097), Some(2));
        assert_eq!(pages_of(0), None);
    }

    #[test]
    fn fixed_addr_must_align() {
        let req = Req { addr: ARENA_BASE + 1, len: PAGE_SIZE, prot: 0, flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS, fd: -1 };
        assert_eq!(validate(&req), Shape::FixedUnaligned);
        assert_eq!(validate(&req).code(), EINVAL);
    }

    #[test]
    fn hint_addr_may_misalign() {
        let req = Req { addr: ARENA_BASE + 5, len: PAGE_SIZE, prot: 0, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1 };
        assert_eq!(validate(&req), Shape::Ok);
    }

    #[test]
    fn flag_surface_three_kinds() {
        assert_eq!(validate(&Req { addr: 0, len: PAGE_SIZE, prot: 0, flags: 0x8000_0000, fd: -1 }), Shape::UnknownFlag);
        assert_eq!(validate(&Req { addr: 0, len: PAGE_SIZE, prot: 0, flags: MAP_SHARED | MAP_PRIVATE, fd: 0 }), Shape::SharedPrivate);
        assert_eq!(validate(&Req { addr: ARENA_BASE, len: PAGE_SIZE, prot: 0, flags: MAP_FIXED | MAP_FIXED_NOREPLACE, fd: 0 }), Shape::FixedConflict);
    }

    #[test]
    fn fd_surface() {
        let neg = Req { addr: 0, len: PAGE_SIZE, prot: 0, flags: MAP_PRIVATE, fd: -1 };
        assert_eq!(validate(&neg), Shape::FdBad);
        assert_eq!(validate(&Req { flags: MAP_PRIVATE | MAP_ANONYMOUS, ..neg }), Shape::Ok);
        assert_eq!(validate(&Req { fd: 7, ..neg }), Shape::Ok);
    }

    #[test]
    fn conflict_both_branches() {
        let (hit, ok) = conflict_probe();
        assert_eq!(hit, EEXIST);
        assert_eq!(ok, 0);
    }

    #[test]
    fn order_is_shape_first() {
        assert_eq!(order_probe(), [EINVAL, EINVAL, EBADF, ENOMEM]);
    }

    #[test]
    fn matrix_sixteen_rows() {
        let (passed, codes) = run_matrix();
        assert_eq!(passed as u32, ROWS_TOTAL);
        for (i, r) in ROWS.iter().enumerate() {
            assert_eq!(codes.get(i).copied(), Some(r.expect), "row {}", r.name);
        }
    }

    #[test]
    fn injections_all_caught() {
        let l = run_injection();
        assert_eq!(l.per_class, [10, 10, 10]);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn replace_does_not_grow_pages() {
        let mut sp = Space::new();
        let q = Req {
            addr: ARENA_BASE,
            len: PAGE_SIZE,
            prot: PROT_READ,
            flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
            fd: -1,
        };
        assert!(sp.mmap(&q).is_ok());
        assert!(sp.mmap(&q).is_ok());
        assert_eq!(sp.used(), 1);
        assert_eq!(sp.replaced(), 1);
        assert_eq!(sp.maps(), 2);
    }

    #[test]
    fn full_window_rejects() {
        let mut sp = Space::new();
        let mut i = 0usize;
        while i < ARENA_PAGES {
            let q = Req {
                addr: ARENA_BASE + PAGE_SIZE * i as u64,
                len: PAGE_SIZE,
                prot: 0,
                flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
                fd: -1,
            };
            assert!(sp.mmap(&q).is_ok());
            i += 1;
        }
        assert_eq!(
            sp.mmap(&Req { addr: 0, len: PAGE_SIZE, prot: 0, flags: MAP_PRIVATE | MAP_ANONYMOUS, fd: -1 }),
            Err(ENOMEM)
        );
        assert_eq!(sp.used(), ARENA_PAGES);
    }

    #[test]
    fn tail_page_boundary() {
        let mut sp = Space::new();
        let last = Req {
            addr: ARENA_BASE + PAGE_SIZE * (ARENA_PAGES as u64 - 1),
            len: PAGE_SIZE,
            prot: 0,
            flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
            fd: -1,
        };
        assert!(sp.mmap(&last).is_ok());
        assert_eq!(
            sp.mmap(&Req { len: PAGE_SIZE * 2, ..last }),
            Err(ENOMEM)
        );
    }

    #[test]
    fn fill_one_fit_search_is_deterministic() {
        let mut sp = Space::new();
        let _ = sp.mmap(&Req {
            addr: ARENA_BASE,
            len: PAGE_SIZE * 2,
            prot: 0,
            flags: MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
            fd: -1,
        });
        let got = sp.mmap(&Req {
            addr: ARENA_BASE,
            len: PAGE_SIZE,
            prot: 0,
            flags: MAP_PRIVATE | MAP_ANONYMOUS,
            fd: -1,
        });
        assert_eq!(got, Ok(ARENA_BASE + PAGE_SIZE * 2));
    }
}
