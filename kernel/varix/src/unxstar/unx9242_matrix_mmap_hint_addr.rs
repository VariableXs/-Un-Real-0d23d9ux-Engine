//! UNX-F9242 mmap addr 提示与坏映射探针矩阵 · 0x7AA00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9242 逐字）：
//!   UNX-F9242-J1 mmap addr 非对齐提示值注入判据过，提示值≠副产物判据过，
//!   坏映射访问 SIGSEGV 路径判据过。
//!
//! 设计要点：
//!   ① 提示不是契约。非 MAP_FIXED 时 addr 只是「我希望能放这里」，内核可以
//!      整体忽略（越界、回绕、低地址）、可以下取整（未对齐），也可以在占用时
//!      改投别处；这三种偏离都不产生错误码，只产生「返回值≠提示值」这一可观测
//!      事实。把提示当契约判 EINVAL 是这类测试最常见的假红。
//!   ② MAP_FIXED 才把 addr 升格为契约：未对齐一律 EINVAL，落在可映射窗口外
//!      一律 ENOMEM，占用则先拆旧再装新（替换语义，不是 EEXIST）。
//!   ③ 判定次序固定为 长度(EINVAL) → 对齐(EINVAL) → 窗口(ENOMEM) → 冲突(替换)。
//!      固定映射同时未对齐且越界时必须回 EINVAL：对齐是参数面，越界是空间面，
//!      参数面的错误先出。
//!   ④ 访问面只判「完全落在单个可读区间内」：区外、跨界、区间尾后一字节、
//!      PROT_NONE、只写不可写，全部走 SIGSEGV；零长访问不触碰任何页，不算违约。
//!   ⑤ 替换后旧权限必须随之消失：同址重映射成 PROT_NONE 之后，旧读权限不得
//!      残留。这是「拆旧」与「装新」是否原子成对的唯一可观测判据。
//!   ⑥ 账目四面：honored/relocated/replaced/rejects 只增不减；替换不改区间数，
//!      拆分才改；容量 16 个区间满后第 17 个回 ENOMEM 且既有区间一个不丢。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9242 独占 0x40 窗口，0x7AA00..=0x7AA3F）。
pub const SEG_BASE: u32 = 0x7AA00;

/// 4K 基础页。
pub const PAGE_SIZE: u64 = 4096;

/// 可映射窗口起点（页对齐，窗口内地址才被本模型接受为固定映射目标）。
pub const ARENA_BASE: u64 = 0x0000_7F00_0000_0000;

/// 可映射页数（窗口大小 64 页）。
pub const ARENA_PAGES: u64 = 64;

/// 可映射窗口终点（开区间上界）。
pub const ARENA_LIMIT: u64 = ARENA_BASE + PAGE_SIZE * ARENA_PAGES;

/// 用户地址空间上界（x86_64 规范 47 位用户态顶）。
pub const TASK_SIZE: u64 = 0x0000_7FFF_FFFF_F000;

/// 区间表容量。
pub const MAX_MAPS: usize = 16;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入变异类别数。
pub const MUTANT_CLASSES: u32 = 3;

/// PROT 位。
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;
pub const PROT_EXEC: u32 = 0x4;

/// Linux errno 正数口径。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapCode {
    /// 提示页空闲，内核原样采纳
    HintHonored,
    /// 提示页被占，内核改投他页
    HintRelocated,
    /// 提示越出窗口，内核整体忽略
    HintIgnored,
    /// 未对齐提示被下取整
    UnalignedRounded,
    /// 固定映射未对齐
    FixedUnaligned,
    /// 固定映射越出窗口
    FixedOutside,
    /// 固定映射替换既有区间
    FixedReplaced,
    /// 解引用落在映射之外
    SegvUnmapped,
    /// 解引用跨过区间尾（尾后一字节）
    SegvGuard,
    /// 权限不足（只读页写 / PROT_NONE）
    ProtDenied,
    /// 零长度请求
    LenZero,
    /// 段位越界
    SegmentViolation,
}

impl MapCode {
    /// 码值 = 段基址 + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MapCode::HintHonored => "提示被采纳：该页空闲，内核可以按提示放置；\
                但采纳是结果不是承诺，不得据此断言内核必须采纳",
            MapCode::HintRelocated => "提示被改投：该页已占用，内核换一处空白放置；\
                返回地址与提示不同是合法行为，不是缺陷",
            MapCode::HintIgnored => "提示被整体忽略：提示越出可映射窗口或回绕；\
                非固定映射下这类提示只当没提，不得回 EINVAL",
            MapCode::UnalignedRounded => "提示被下取整：非固定映射允许任意对齐的提示；\
                内核取整到页首后再放置，返回值必须页对齐",
            MapCode::FixedUnaligned => "固定映射未对齐：MAP_FIXED 把地址升格为契约，\
                未对齐必须 EINVAL；请按 4K 对齐后重试",
            MapCode::FixedOutside => "固定映射越界：目标区间伸出可映射窗口，\
                空间面错误必须 ENOMEM，不得与对齐错误混报",
            MapCode::FixedReplaced => "固定映射替换：同址再映射会先拆旧再装新；\
                区间数不变、旧权限消失，替换不是 EEXIST",
            MapCode::SegvUnmapped => "访问未映射页：完全落在区间之外的解引用必须 \
                SIGSEGV；请先 mmap 再触碰",
            MapCode::SegvGuard => "访问跨越区间尾：起始在区内但长度伸出区外，\
                整段必须 SIGSEGV，不得按首字节判定放行",
            MapCode::ProtDenied => "权限不足：向不可读页读或向不可写页写必须 \
                SIGSEGV；PROT_NONE 与只读页两条路径都要覆盖",
            MapCode::LenZero => "零长度请求：mmap 长度为 0 必须 EINVAL；\
                零长度访问本身不算越界，两者不可混淆",
            MapCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [MapCode; 12] = [
        MapCode::HintHonored,
        MapCode::HintRelocated,
        MapCode::HintIgnored,
        MapCode::UnalignedRounded,
        MapCode::FixedUnaligned,
        MapCode::FixedOutside,
        MapCode::FixedReplaced,
        MapCode::SegvUnmapped,
        MapCode::SegvGuard,
        MapCode::ProtDenied,
        MapCode::LenZero,
        MapCode::SegmentViolation,
    ];
}

// ── 区间与请求 ────────────────────────────────────────────────────────
/// 一段映射区间。start 恒为页对齐，len 恒为正页的整数倍。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub start: u64,
    pub len: u64,
    pub prot: u32,
}

impl Region {
    /// 区间结束地址（开区间）。构造时已保证不溢出。
    pub fn end(&self) -> u64 {
        self.start.saturating_add(self.len)
    }

    /// 是否覆盖地址 addr。
    pub fn covers(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end()
    }
}

/// 映射请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapReq {
    pub hint: u64,
    pub len: u64,
    pub prot: u32,
    pub fixed: bool,
}

/// 页对齐谓词。
pub fn is_page_aligned(addr: u64) -> bool {
    addr & (PAGE_SIZE - 1) == 0
}

/// 下取整到页首。
pub fn page_align_down(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}

/// 长度换算页数：长度 0 或换算溢出返回 None（调用方分流 EINVAL / ENOMEM）。
pub fn pages_of(len: u64) -> Option<u64> {
    if len == 0 {
        return None;
    }
    let padded = len.checked_add(PAGE_SIZE - 1)?;
    Some(padded / PAGE_SIZE)
}

/// 窗口内的页序号；窗口外返回 None。
pub fn page_index(addr: u64) -> Option<u64> {
    let rel = addr.checked_sub(ARENA_BASE)?;
    if rel >= PAGE_SIZE * ARENA_PAGES {
        return None;
    }
    Some(rel / PAGE_SIZE)
}

// ── 变异模式（仅内部判定器使用） ──────────────────────────────────────
/// 判定器模式。Correct 是唯一上线路径；其余三种只用于注入检出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Correct,
    /// 把非固定提示当契约：越界/占用即回 EINVAL
    LiteralHint,
    /// 把 MAP_FIXED 降级成提示
    DropFixed,
    /// 跳过去对齐/取整
    DropRounding,
}

// ── 地址空间模型 ──────────────────────────────────────────────────────
/// 定容映射空间。16 个区间槽 + 四面账目，不持有堆内存。
#[derive(Clone, Copy, Debug)]
pub struct Arena {
    regions: [Option<Region>; MAX_MAPS],
    honored: u32,
    relocated: u32,
    replaced: u32,
    rejects: u32,
}

impl Arena {
    /// 空地址空间。
    pub const fn new() -> Arena {
        Arena {
            regions: [None; MAX_MAPS],
            honored: 0,
            relocated: 0,
            replaced: 0,
            rejects: 0,
        }
    }

    /// 当前区间数。
    pub fn region_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_MAPS {
            if self.regions.get(i).and_then(|r| r.as_ref()).is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 已映射页数。
    pub fn mapped_pages(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < MAX_MAPS {
            if let Some(Some(r)) = self.regions.get(i) {
                n = n.saturating_add(r.len / PAGE_SIZE);
            }
            i += 1;
        }
        n
    }

    /// 采纳提示次数。
    pub fn honored(&self) -> u32 {
        self.honored
    }

    /// 改投次数。
    pub fn relocated(&self) -> u32 {
        self.relocated
    }

    /// 替换次数。
    pub fn replaced(&self) -> u32 {
        self.replaced
    }

    /// 拒绝次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 取第 idx 个区间。
    pub fn region_at(&self, idx: usize) -> Option<Region> {
        self.regions.get(idx).and_then(|r| r.as_ref()).copied()
    }

    /// 页是否空闲（窗口外的页一律视为不空闲）。
    pub fn page_free(&self, page: u64) -> bool {
        if page >= ARENA_PAGES {
            return false;
        }
        let addr = ARENA_BASE + page * PAGE_SIZE;
        let mut i = 0usize;
        while i < MAX_MAPS {
            if let Some(Some(r)) = self.regions.get(i) {
                if r.covers(addr) {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 从 start_page 起找连续 pages 页，找不满返回 None。只向高地址扫一遍。
    pub fn find_run(&self, start_page: u64, pages: u64) -> Option<u64> {
        if pages == 0 || pages > ARENA_PAGES {
            return None;
        }
        let last_start = ARENA_PAGES.saturating_sub(pages);
        let mut base = if start_page > last_start { last_start } else { start_page };
        while base <= last_start {
            let mut k = 0u64;
            let mut ok = true;
            while k < pages {
                if !self.page_free(base + k) {
                    ok = false;
                    break;
                }
                k += 1;
            }
            if ok {
                return Some(ARENA_BASE + base * PAGE_SIZE);
            }
            base += 1;
        }
        None
    }

    /// 找第一个空槽。
    fn free_slot(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_MAPS {
            match self.regions.get(i) {
                Some(None) => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 统计与 [start,end) 相交的区间数。
    fn overlap_count(&self, start: u64, end: u64) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_MAPS {
            if let Some(Some(r)) = self.regions.get(i) {
                if r.start < end && r.end() > start {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 拆掉与 [start,end) 相交的部分。相交区间可能被拆成左右两片。
    /// 容量不足时整段操作失败且原状态一字不动（先在临时表里推演）。
    pub fn unmap_range(&mut self, start: u64, end: u64) -> Result<u64, i32> {
        if end <= start {
            return Ok(0);
        }
        let mut next: [Option<Region>; MAX_MAPS] = [None; MAX_MAPS];
        let mut n = 0usize;
        let mut removed_pages = 0u64;
        let mut i = 0usize;
        while i < MAX_MAPS {
            if let Some(Some(r)) = self.regions.get(i) {
                let r_end = r.end();
                if r_end <= start || r.start >= end {
                    if n >= MAX_MAPS {
                        return Err(ENOMEM);
                    }
                    if let Some(slot) = next.get_mut(n) {
                        *slot = Some(*r);
                    }
                    n += 1;
                } else {
                    let lo = if start > r.start { start } else { r.start };
                    let hi = if end < r_end { end } else { r_end };
                    removed_pages = removed_pages.saturating_add((hi - lo) / PAGE_SIZE);
                    if r.start < start {
                        if n >= MAX_MAPS {
                            return Err(ENOMEM);
                        }
                        if let Some(slot) = next.get_mut(n) {
                            *slot = Some(Region { start: r.start, len: start - r.start, prot: r.prot });
                        }
                        n += 1;
                    }
                    if r_end > end {
                        if n >= MAX_MAPS {
                            return Err(ENOMEM);
                        }
                        if let Some(slot) = next.get_mut(n) {
                            *slot = Some(Region { start: end, len: r_end - end, prot: r.prot });
                        }
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        self.regions = next;
        Ok(removed_pages)
    }

    /// 装新区间（不查冲突，冲突由调用方先拆）。
    fn install(&mut self, region: Region) -> Result<(), i32> {
        match self.free_slot() {
            Some(idx) => {
                if let Some(slot) = self.regions.get_mut(idx) {
                    *slot = Some(region);
                    Ok(())
                } else {
                    Err(ENOMEM)
                }
            }
            None => Err(ENOMEM),
        }
    }

    /// 上线路径：正常判定器。
    pub fn map(&mut self, req: &MapReq) -> Result<u64, i32> {
        self.map_mode(req, Mode::Correct)
    }

    /// 参数化判定器。Mode 只被注入矩阵使用，Correct 与 map 等价。
    pub fn map_mode(&mut self, req: &MapReq, mode: Mode) -> Result<u64, i32> {
        // ① 长度面：0 长度 EINVAL；换算溢出归空间面 ENOMEM。
        if req.len == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let pages = match pages_of(req.len) {
            Some(p) => p,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ENOMEM);
            }
        };
        if pages > ARENA_PAGES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ENOMEM);
        }
        let span = match pages.checked_mul(PAGE_SIZE) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ENOMEM);
            }
        };
        let fixed = if mode == Mode::DropFixed { false } else { req.fixed };

        if fixed {
            // ② 参数面：未对齐先于空间面。
            if is_page_aligned(req.hint) || mode == Mode::DropRounding {
                let end = match req.hint.checked_add(span) {
                    Some(v) => v,
                    None => {
                        self.rejects = self.rejects.saturating_add(1);
                        return Err(ENOMEM);
                    }
                };
                if req.hint < ARENA_BASE || end > ARENA_LIMIT {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(ENOMEM);
                }
                let overlaps = self.overlap_count(req.hint, end);
                if overlaps > 0 {
                    if self.unmap_range(req.hint, end).is_err() {
                        self.rejects = self.rejects.saturating_add(1);
                        return Err(ENOMEM);
                    }
                    self.replaced = self.replaced.saturating_add(overlaps);
                }
                match self.install(Region { start: req.hint, len: span, prot: req.prot }) {
                    Ok(()) => {
                        self.honored = self.honored.saturating_add(1);
                        Ok(req.hint)
                    }
                    Err(e) => {
                        self.rejects = self.rejects.saturating_add(1);
                        Err(e)
                    }
                }
            } else {
                self.rejects = self.rejects.saturating_add(1);
                Err(EINVAL)
            }
        } else {
            // ③ 提示面：非固定映射的提示永不产生错误码。
            if mode == Mode::LiteralHint {
                let inside = req.hint >= ARENA_BASE && req.hint < ARENA_LIMIT;
                let rounded = page_align_down(req.hint);
                let busy = match page_index(rounded) {
                    Some(p) => !self.page_free(p),
                    None => false,
                };
                if req.hint != 0 && (!inside || busy) {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(EINVAL);
                }
            }
            let prefer = if mode == Mode::DropRounding {
                match page_index(req.hint) {
                    Some(p) => p,
                    None => 0,
                }
            } else {
                match page_index(page_align_down(req.hint)) {
                    Some(p) => p,
                    None => 0,
                }
            };
            let start = match self.find_run(prefer, pages).or_else(|| self.find_run(0, pages)) {
                Some(v) => v,
                None => {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(ENOMEM);
                }
            };
            let placed = if mode == Mode::DropRounding && req.hint != 0 && req.hint < ARENA_LIMIT {
                // 变异体：不取整，直接把未对齐提示当起始地址。
                if self.page_free(page_index(page_align_down(req.hint)).unwrap_or(0)) {
                    req.hint
                } else {
                    start
                }
            } else {
                start
            };
            match self.install(Region { start: placed, len: span, prot: req.prot }) {
                Ok(()) => {
                    if req.hint != 0 && placed == page_align_down(req.hint) {
                        self.honored = self.honored.saturating_add(1);
                    } else if req.hint != 0 {
                        self.relocated = self.relocated.saturating_add(1);
                    }
                    Ok(placed)
                }
                Err(e) => {
                    self.rejects = self.rejects.saturating_add(1);
                    Err(e)
                }
            }
        }
    }

    /// 地址所属区间。
    pub fn region_of(&self, addr: u64) -> Option<Region> {
        let mut i = 0usize;
        while i < MAX_MAPS {
            if let Some(Some(r)) = self.regions.get(i) {
                if r.covers(addr) {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 读访问判定：true = 通过，false = SIGSEGV。
    pub fn read_ok(&self, addr: u64, len: u64) -> bool {
        self.access_ok(addr, len, false)
    }

    /// 写访问判定：true = 通过，false = SIGSEGV。
    pub fn write_ok(&self, addr: u64, len: u64) -> bool {
        self.access_ok(addr, len, true)
    }

    /// 访问判定：整段必须落在单个区间内且权限成立。
    /// 零长度不触碰任何页，直接通过（与 mmap 的 0 长度 EINVAL 是两回事）。
    pub fn access_ok(&self, addr: u64, len: u64, write: bool) -> bool {
        if len == 0 {
            return true;
        }
        let end = match addr.checked_add(len) {
            Some(v) => v,
            None => return false,
        };
        let r = match self.region_of(addr) {
            Some(r) => r,
            None => return false,
        };
        if end > r.end() {
            return false;
        }
        if write {
            // x86 语义：W 隐含 R；只读页写必 SIGSEGV。
            r.prot & PROT_WRITE != 0
        } else {
            r.prot & (PROT_READ | PROT_WRITE) != 0
        }
    }

    /// 访问失败归类（供诊断与矩阵断言）。
    pub fn access_code(&self, addr: u64, len: u64, write: bool) -> MapCode {
        if len == 0 {
            return MapCode::HintIgnored;
        }
        match self.region_of(addr) {
            None => MapCode::SegvUnmapped,
            Some(r) => match addr.checked_add(len) {
                Some(end) if end <= r.end() => {
                    if self.access_ok(addr, len, write) {
                        MapCode::HintHonored
                    } else {
                        MapCode::ProtDenied
                    }
                }
                _ => MapCode::SegvGuard,
            },
        }
    }
}

// ── 提示类别与探针矩阵 ────────────────────────────────────────────────
/// 提示类别（矩阵的行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintClass {
    /// 不给提示
    Auto,
    /// 给窗口内空闲页
    FreeHint,
    /// 给窗口内已占用页
    BusyHint,
    /// 给未对齐地址
    UnalignedHint,
    /// 给窗口下沿之外的低地址
    BelowHint,
    /// 给窗口上沿之外的高地址
    AboveHint,
    /// 给接近 u64::MAX 的回绕地址
    WrapHint,
    /// 固定映射到空闲页
    FixedFree,
    /// 固定映射未对齐
    FixedUnaligned,
    /// 固定映射越出窗口
    FixedOutside,
    /// 固定映射覆盖既有区间
    FixedOverlap,
    /// 零长度（固定与否都 EINVAL）
    ZeroLen,
}

impl HintClass {
    /// 矩阵全量（12 行）。
    pub const ALL: [HintClass; 12] = [
        HintClass::Auto,
        HintClass::FreeHint,
        HintClass::BusyHint,
        HintClass::UnalignedHint,
        HintClass::BelowHint,
        HintClass::AboveHint,
        HintClass::WrapHint,
        HintClass::FixedFree,
        HintClass::FixedUnaligned,
        HintClass::FixedOutside,
        HintClass::FixedOverlap,
        HintClass::ZeroLen,
    ];

    /// 期望错误码（0 = 成功）。
    pub fn expect(self) -> i32 {
        match self {
            HintClass::FixedUnaligned | HintClass::ZeroLen => EINVAL,
            HintClass::FixedOutside => ENOMEM,
            _ => 0,
        }
    }

    /// 是否要求返回值与提示一致（页对齐后）。
    pub fn demands_hint(self) -> bool {
        matches!(self, HintClass::FreeHint | HintClass::FixedFree)
    }
}

/// 单格结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellResult {
    pub code: i32,
    pub va: u64,
    pub ok: bool,
}

/// 单格执行：每格一棵全新地址空间，杜绝跨格污染。
pub fn run_cell(class: HintClass, mode: Mode) -> CellResult {
    let mut a = Arena::new();
    let req = match class {
        HintClass::Auto => MapReq { hint: 0, len: PAGE_SIZE, prot: PROT_READ, fixed: false },
        HintClass::FreeHint => MapReq {
            hint: ARENA_BASE + PAGE_SIZE * 4,
            len: PAGE_SIZE,
            prot: PROT_READ,
            fixed: false,
        },
        HintClass::BusyHint => {
            // 先把首页占掉，再用首页做提示。
            let _ = a.map(&MapReq { hint: 0, len: PAGE_SIZE, prot: PROT_READ, fixed: false });
            MapReq { hint: ARENA_BASE, len: PAGE_SIZE, prot: PROT_READ, fixed: false }
        }
        HintClass::UnalignedHint => MapReq {
            hint: ARENA_BASE + PAGE_SIZE * 2 + 0x123,
            len: PAGE_SIZE,
            prot: PROT_READ,
            fixed: false,
        },
        HintClass::BelowHint => MapReq { hint: PAGE_SIZE, len: PAGE_SIZE, prot: PROT_READ, fixed: false },
        HintClass::AboveHint => MapReq {
            hint: TASK_SIZE.saturating_add(PAGE_SIZE),
            len: PAGE_SIZE,
            prot: PROT_READ,
            fixed: false,
        },
        HintClass::WrapHint => MapReq { hint: u64::MAX - 8, len: PAGE_SIZE, prot: PROT_READ, fixed: false },
        HintClass::FixedFree => MapReq {
            hint: ARENA_BASE + PAGE_SIZE * 8,
            len: PAGE_SIZE,
            prot: PROT_READ,
            fixed: true,
        },
        HintClass::FixedUnaligned => MapReq {
            hint: ARENA_BASE + PAGE_SIZE * 8 + 0x10,
            len: PAGE_SIZE,
            prot: PROT_READ,
            fixed: true,
        },
        HintClass::FixedOutside => MapReq {
            hint: ARENA_LIMIT + PAGE_SIZE * 8,
            len: PAGE_SIZE,
            prot: PROT_READ,
            fixed: true,
        },
        HintClass::FixedOverlap => {
            // 先放一个只读区间，再用 PROT_NONE 固定覆盖同址。
            let _ = a.map(&MapReq {
                hint: ARENA_BASE + PAGE_SIZE * 12,
                len: PAGE_SIZE,
                prot: PROT_READ,
                fixed: true,
            });
            MapReq {
                hint: ARENA_BASE + PAGE_SIZE * 12,
                len: PAGE_SIZE,
                prot: 0,
                fixed: true,
            }
        }
        HintClass::ZeroLen => MapReq { hint: ARENA_BASE, len: 0, prot: PROT_READ, fixed: true },
    };

    let before_count = a.region_count();
    let r = a.map_mode(&req, mode);
    let (code, va) = match r {
        Ok(v) => (0, v),
        Err(e) => (e, 0),
    };
    let mut ok = code == class.expect();
    if ok {
        ok = match class {
            HintClass::FreeHint | HintClass::FixedFree => {
                va == req.hint && is_page_aligned(va) && a.region_of(va).is_some()
            }
            HintClass::BusyHint => va != req.hint && a.region_of(va).is_some(),
            HintClass::UnalignedHint => va == page_align_down(req.hint) && is_page_aligned(va),
            HintClass::BelowHint | HintClass::AboveHint | HintClass::WrapHint => {
                va >= ARENA_BASE && va < ARENA_LIMIT && a.region_of(va).is_some()
            }
            HintClass::FixedOverlap => {
                va == req.hint
                    && a.region_count() == before_count
                    && a.replaced() == 1
                    && !a.read_ok(req.hint, 1)
            }
            HintClass::Auto => va >= ARENA_BASE && va < ARENA_LIMIT && is_page_aligned(va),
            HintClass::FixedUnaligned | HintClass::FixedOutside | HintClass::ZeroLen => true,
        };
    }
    CellResult { code, va, ok }
}

/// 矩阵汇总。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    pub codes: [i32; 12],
    pub passed: u32,
    pub segv_cells: u32,
}

/// 跑满 12 格（Correct 模式）。
pub fn run_matrix() -> Matrix {
    let mut codes = [0i32; 12];
    let mut passed = 0u32;
    let mut segv = 0u32;
    let mut i = 0usize;
    while i < HintClass::ALL.len() {
        if let Some(c) = HintClass::ALL.get(i).copied() {
            let r = run_cell(c, Mode::Correct);
            if r.ok {
                passed = passed.saturating_add(1);
            }
            if r.code != 0 {
                segv = segv.saturating_add(1);
            }
            if let Some(slot) = codes.get_mut(i) {
                *slot = r.code;
            }
        }
        i += 1;
    }
    Matrix { codes, passed, segv_cells: segv }
}

// ── 变异检出与账目 ────────────────────────────────────────────────────
/// 注入变异类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    LiteralHint,
    DropFixed,
    DropRounding,
}

impl Mutant {
    pub const ALL: [Mutant; 3] = [Mutant::LiteralHint, Mutant::DropFixed, Mutant::DropRounding];

    /// 稳定标签 0..2。
    pub fn tag(self) -> u8 {
        match self {
            Mutant::LiteralHint => 0,
            Mutant::DropFixed => 1,
            Mutant::DropRounding => 2,
        }
    }

    /// 对应判定器模式。
    pub fn mode(self) -> Mode {
        match self {
            Mutant::LiteralHint => Mode::LiteralHint,
            Mutant::DropFixed => Mode::DropFixed,
            Mutant::DropRounding => Mode::DropRounding,
        }
    }
}

/// 矩阵指纹：逐格编码 (码值, 是否成功, va 页对齐, va 是否落在窗口内)。
pub fn matrix_digest(mode: Mode) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < HintClass::ALL.len() {
        if let Some(c) = HintClass::ALL.get(i).copied() {
            let r = run_cell(c, mode);
            let flags = (if r.code == 0 { 1u8 } else { 0 })
                | (if is_page_aligned(r.va) { 2 } else { 0 })
                | (if r.va >= ARENA_BASE && r.va < ARENA_LIMIT { 4 } else { 0 });
            for b in [i as u8, flags, (r.code & 0x7F) as u8, (r.va & 0xFF) as u8].iter() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        i += 1;
    }
    h
}

/// 变异是否被矩阵指纹抓住。
pub fn mutant_detected(m: Mutant) -> bool {
    matrix_digest(m.mode()) != matrix_digest(Mode::Correct)
}

/// 注入账本。
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub per_class: [u32; 3],
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { injected: 0, detected: 0, escaped: 0, per_class: [0; 3] }
    }

    /// 记一轮。
    pub fn record(&mut self, tag: u8, detected: bool) {
        self.injected = self.injected.saturating_add(1);
        let idx = (tag as usize) % 3;
        if detected {
            self.detected = self.detected.saturating_add(1);
            if let Some(slot) = self.per_class.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒：注入 = 检出 + 逃逸。
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

// ── 指纹与闭集自检 ────────────────────────────────────────────────────
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

/// 语料指纹：覆盖提示采纳/改投/忽略/取整/固定/替换六条路径。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let m = run_matrix();
    let mut i = 0usize;
    while i < m.codes.len() {
        if let Some(c) = m.codes.get(i).copied() {
            bytes.push((c & 0x7F) as u8);
        }
        i += 1;
    }
    let seq = [
        MapReq { hint: 0, len: PAGE_SIZE, prot: PROT_READ, fixed: false },
        MapReq { hint: ARENA_BASE + 0x10, len: PAGE_SIZE, prot: PROT_WRITE, fixed: false },
        MapReq { hint: ARENA_LIMIT + PAGE_SIZE, len: PAGE_SIZE, prot: PROT_READ, fixed: false },
        MapReq { hint: ARENA_BASE + PAGE_SIZE * 20, len: PAGE_SIZE * 2, prot: PROT_READ, fixed: true },
        MapReq { hint: ARENA_BASE + PAGE_SIZE * 20, len: PAGE_SIZE, prot: 0, fixed: true },
    ];
    let mut a = Arena::new();
    let mut k = 0usize;
    while k < seq.len() {
        if let Some(req) = seq.get(k).copied() {
            let r = a.map(&req);
            bytes.push(if r.is_ok() { 1 } else { 2 });
            if let Ok(v) = r {
                bytes.push((v & 0xFF) as u8);
            }
        }
        k += 1;
    }
    bytes.push(a.region_count() as u8);
    bytes.push(a.replaced() as u8);
    fnv1a(&bytes)
}

/// 诊断码文案闭集且码值唯一。
pub fn codes_closed() -> bool {
    if MapCode::ALL.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < MapCode::ALL.len() {
        let a = MapCode::ALL[i];
        if a.detail().is_empty() {
            return false;
        }
        if a.val() < SEG_BASE + 1 || a.val() > SEG_BASE + 0x0C {
            return false;
        }
        let mut j = i + 1;
        while j < MapCode::ALL.len() {
            let b = MapCode::ALL[j];
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
/// F9242 判据集。
pub fn run_unx9242_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9242");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 诊断码闭集
    s.add("诊断码：12 个码值唯一、文案非空、全部落在段内", codes_closed(),
          "码值必须在 +0x01..=+0x0C 且两两不同，文案不得为空");

    // 3. 正常流：不给提示也能落在窗口内
    let auto = run_cell(HintClass::Auto, Mode::Correct);
    s.add("正常流：无提示映射落在窗口内且页对齐",
          auto.code == 0 && auto.ok && auto.va >= ARENA_BASE && auto.va < ARENA_LIMIT,
          "不提示时内核自选地址，结果必须合法");

    // 4. 提示空闲页被采纳
    let free = run_cell(HintClass::FreeHint, Mode::Correct);
    s.add("提示面：空闲页提示被原样采纳", free.code == 0 && free.va == ARENA_BASE + PAGE_SIZE * 4,
          "提示页空闲时返回值应等于提示值，这是「提示可被采纳」的正判据");

    // 5. 提示占用页被改投
    let busy = run_cell(HintClass::BusyHint, Mode::Correct);
    s.add("提示面：占用页提示被改投且不报错",
          busy.code == 0 && busy.va != ARENA_BASE && busy.va >= ARENA_BASE,
          "提示被占用时内核换址是合法行为，回 EINVAL 才是缺陷");

    // 6. 非对齐提示下取整
    let unaligned = run_cell(HintClass::UnalignedHint, Mode::Correct);
    s.add("提示面：非对齐提示被下取整",
          unaligned.code == 0
              && unaligned.va == page_align_down(ARENA_BASE + PAGE_SIZE * 2 + 0x123)
              && is_page_aligned(unaligned.va),
          "非固定提示允许任意对齐，内核取整到页首后返回");

    // 7. 越界提示被整体忽略
    let below = run_cell(HintClass::BelowHint, Mode::Correct);
    let above = run_cell(HintClass::AboveHint, Mode::Correct);
    let wrap = run_cell(HintClass::WrapHint, Mode::Correct);
    s.add("提示面：低地址/高地址/回绕提示一律被忽略",
          below.code == 0 && above.code == 0 && wrap.code == 0
              && below.ok && above.ok && wrap.ok,
          "提示越出窗口或回绕时只当没提；回错误码即把提示当成了契约");

    // 8. 固定映射命中
    let fixed = run_cell(HintClass::FixedFree, Mode::Correct);
    s.add("契约面：固定映射精确命中提示地址",
          fixed.code == 0 && fixed.va == ARENA_BASE + PAGE_SIZE * 8,
          "MAP_FIXED 下返回值必须等于请求地址，差一页即违约");

    // 9. 固定未对齐 → EINVAL
    let fixed_unaligned = run_cell(HintClass::FixedUnaligned, Mode::Correct);
    s.add("契约面：固定映射未对齐 → EINVAL", fixed_unaligned.code == EINVAL,
          "固定映射把对齐升格为契约，未对齐必须拒绝");

    // 10. 固定越界 → ENOMEM
    let fixed_out = run_cell(HintClass::FixedOutside, Mode::Correct);
    s.add("契约面：固定映射越出窗口 → ENOMEM", fixed_out.code == ENOMEM,
          "空间面错误必须是 ENOMEM，不得与参数面 EINVAL 混报");

    // 11. 判定次序：未对齐先于越界
    let mut order = Arena::new();
    let both_bad = order.map(&MapReq {
        hint: ARENA_LIMIT + 0x10,
        len: PAGE_SIZE,
        prot: PROT_READ,
        fixed: true,
    });
    s.add("判定次序：未对齐先于越界（EINVAL 优先）", both_bad == Err(EINVAL),
          "同一请求同时未对齐又越界时，参数面错误必须先出");

    // 12. 固定覆盖：替换不是 EEXIST
    let overlap = run_cell(HintClass::FixedOverlap, Mode::Correct);
    s.add("替换面：同址固定重映射拆旧装新",
          overlap.code == 0 && overlap.ok,
          "覆盖必须成功、区间数不变、旧权限消失；回 EEXIST 即语义错误");

    // 13. 访问面：区内可读
    let mut acc = Arena::new();
    let base = match acc.map(&MapReq {
        hint: ARENA_BASE + PAGE_SIZE * 3,
        len: PAGE_SIZE,
        prot: PROT_READ,
        fixed: true,
    }) {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add("访问面：区内整页可读", acc.read_ok(base, PAGE_SIZE) && acc.read_ok(base + 8, 4),
          "完全落在可读区间内的访问必须放行");

    // 14. 访问面：区外与尾后一字节
    let outside = !acc.read_ok(ARENA_BASE + PAGE_SIZE * 30, 8);
    let guard = !acc.read_ok(base + PAGE_SIZE - 4, 8);
    s.add("访问面：区外与跨区间尾都走 SIGSEGV", outside && guard,
          "首字节在区内但整段伸出区外必须 SIGSEGV，不能只看首字节");

    // 15. 权限面：只读页写、PROT_NONE 读
    let ro_write = !acc.write_ok(base, 1);
    let mut none_arena = Arena::new();
    let none_base = match none_arena.map(&MapReq {
        hint: ARENA_BASE + PAGE_SIZE * 5,
        len: PAGE_SIZE,
        prot: 0,
        fixed: true,
    }) {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add("权限面：只读页写与 PROT_NONE 读都走 SIGSEGV",
          ro_write && !none_arena.read_ok(none_base, 1) && !none_arena.write_ok(none_base, 1),
          "权限判定必须按访问方向分别判，不能一把梭");

    // 16. 零长度访问不越界，零长度映射 EINVAL
    let zero_len = run_cell(HintClass::ZeroLen, Mode::Correct);
    s.add("零长度：访问放行、映射 EINVAL", acc.read_ok(base, 0) && zero_len.code == EINVAL,
          "零长度访问不触碰页，零长度映射是参数错误，两者不可混为一谈");

    // 17. 满矩阵逐格
    let m = run_matrix();
    s.add("矩阵：12 格逐格判定与期望码一致", m.passed == 12,
          "12 格必须全过，任何一格偏离期望码即红");

    // 18. 矩阵码快照与期望码逐格相等
    let mut codes_match = true;
    let mut i = 0usize;
    while i < HintClass::ALL.len() {
        if let Some(c) = HintClass::ALL.get(i).copied() {
            if m.codes.get(i).copied() != Some(c.expect()) {
                codes_match = false;
            }
        }
        i += 1;
    }
    s.add("矩阵：期望码零漂移", codes_match, "冻结期望表不得随实现漂移");

    // 19. 注入 3 类各 10/10
    let l = run_injection();
    s.add("注入必抓：三类变异各 10/10 全检出",
          l.per_class == [INJECT_ROUNDS; 3],
          "提示当契约、固定降级、跳过取整三类都必须每轮被抓");

    // 20. 守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸 0",
          l.conserved() && l.escaped == 0 && l.injected == MUTANT_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 21. 双跑指纹
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：语料指纹两次相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 22. 状态机：拆掉之后旧指针立即 SIGSEGV
    let mut life = Arena::new();
    let va = match life.map(&MapReq { hint: 0, len: PAGE_SIZE * 2, prot: PROT_READ | PROT_WRITE, fixed: false }) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let alive = life.write_ok(va, 8);
    let removed = life.unmap_range(va, va.saturating_add(PAGE_SIZE));
    let dead = !life.read_ok(va, 8);
    let tail_alive = life.read_ok(va.saturating_add(PAGE_SIZE), 8);
    s.add("状态机：拆掉前半页后旧首地址立即 SIGSEGV、后半页仍可读",
          alive && removed == Ok(1) && dead && tail_alive,
          "拆映射必须精确到页，拆多拆少都能被这条判据抓到");

    // 23. 容量：16 区间满后 ENOMEM 且既有区间一个不丢
    let mut cap = Arena::new();
    let mut made = 0usize;
    while made < MAX_MAPS {
        let r = cap.map(&MapReq {
            hint: ARENA_BASE + PAGE_SIZE * (made as u64),
            len: PAGE_SIZE,
            prot: PROT_READ,
            fixed: true,
        });
        if r.is_err() {
            break;
        }
        made += 1;
    }
    let overflow = cap.map(&MapReq {
        hint: ARENA_BASE + PAGE_SIZE * 20,
        len: PAGE_SIZE,
        prot: PROT_READ,
        fixed: true,
    });
    s.add("容量：16 个区间满后第 17 个 ENOMEM 且账不丢",
          made == MAX_MAPS && overflow == Err(ENOMEM) && cap.region_count() == MAX_MAPS
              && cap.mapped_pages() == MAX_MAPS as u64,
          "满槽必须回 ENOMEM；拒收不得丢既有区间，也不得漏记账");

    // 24. 拆分/合并账：替换不增区间，拆分只增一片
    let mut split = Arena::new();
    let whole = MapReq { hint: ARENA_BASE, len: PAGE_SIZE * 4, prot: PROT_READ, fixed: true };
    let mapped = split.map(&whole).is_ok();
    let after_map = split.region_count();
    let holed = split.unmap_range(ARENA_BASE + PAGE_SIZE, ARENA_BASE + PAGE_SIZE * 2);
    let after_hole = split.region_count();
    s.add("容量：整段拆洞后区间数 +1 且页账精确",
          mapped && after_map == 1 && holed == Ok(1) && after_hole == 2
              && split.mapped_pages() == 3,
          "挖掉中间一页应得两片、剩三页；页账与区间数必须同时对上");

    // 动态明细（≤8）
    let matrix_text = format!("矩阵通过 {}/12，拒绝格 {}", m.passed, m.segv_cells);
    s.add_dyn("矩阵快照", m.passed == 12, &matrix_text);
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
        let s = run_unx9242_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }

    #[test]
    fn hint_is_advisory_never_error() {
        for c in [HintClass::BelowHint, HintClass::AboveHint, HintClass::WrapHint].iter() {
            let r = run_cell(*c, Mode::Correct);
            assert_eq!(r.code, 0);
            assert!(r.va >= ARENA_BASE && r.va < ARENA_LIMIT);
        }
    }

    #[test]
    fn busy_hint_relocates() {
        let r = run_cell(HintClass::BusyHint, Mode::Correct);
        assert_eq!(r.code, 0);
        assert_ne!(r.va, ARENA_BASE);
    }

    #[test]
    fn unaligned_hint_rounds_down() {
        let r = run_cell(HintClass::UnalignedHint, Mode::Correct);
        assert_eq!(r.va, page_align_down(ARENA_BASE + PAGE_SIZE * 2 + 0x123));
        assert!(is_page_aligned(r.va));
    }

    #[test]
    fn fixed_requires_alignment_and_window() {
        assert_eq!(run_cell(HintClass::FixedUnaligned, Mode::Correct).code, EINVAL);
        assert_eq!(run_cell(HintClass::FixedOutside, Mode::Correct).code, ENOMEM);
        let mut a = Arena::new();
        let both = MapReq { hint: ARENA_LIMIT + 0x10, len: PAGE_SIZE, prot: 0, fixed: true };
        assert_eq!(a.map(&both), Err(EINVAL));
    }

    #[test]
    fn fixed_overlap_replaces_not_eeexist() {
        let r = run_cell(HintClass::FixedOverlap, Mode::Correct);
        assert_eq!(r.code, 0);
        assert!(r.ok);
    }

    #[test]
    fn access_needs_whole_range() {
        let mut a = Arena::new();
        let base = a
            .map(&MapReq { hint: ARENA_BASE, len: PAGE_SIZE, prot: PROT_READ, fixed: true })
            .unwrap_or(0);
        assert!(a.read_ok(base, PAGE_SIZE));
        assert!(!a.read_ok(base + PAGE_SIZE - 1, 2));
        assert!(!a.read_ok(base - 1, 2));
        assert!(a.read_ok(base, 0));
    }

    #[test]
    fn write_implies_read_on_x86() {
        let mut a = Arena::new();
        let base = a
            .map(&MapReq {
                hint: ARENA_BASE,
                len: PAGE_SIZE,
                prot: PROT_WRITE,
                fixed: true,
            })
            .unwrap_or(0);
        assert!(a.read_ok(base, 4));
        assert!(a.write_ok(base, 4));
        assert_eq!(a.access_code(base, 4, true), MapCode::HintHonored);
    }

    #[test]
    fn prot_none_denies_all() {
        let mut a = Arena::new();
        let base = a
            .map(&MapReq { hint: ARENA_BASE, len: PAGE_SIZE, prot: 0, fixed: true })
            .unwrap_or(0);
        assert!(!a.read_ok(base, 1));
        assert!(!a.write_ok(base, 1));
        assert_eq!(a.access_code(base, 1, false), MapCode::ProtDenied);
    }

    #[test]
    fn access_classification() {
        let mut a = Arena::new();
        assert_eq!(a.access_code(ARENA_BASE, 1, false), MapCode::SegvUnmapped);
        let base = a
            .map(&MapReq { hint: ARENA_BASE, len: PAGE_SIZE, prot: PROT_READ, fixed: true })
            .unwrap_or(0);
        assert_eq!(a.access_code(base + PAGE_SIZE - 1, 2, false), MapCode::SegvGuard);
    }

    #[test]
    fn matrix_all_twelve() {
        let m = run_matrix();
        assert_eq!(m.passed, 12);
        for (i, c) in HintClass::ALL.iter().enumerate() {
            assert_eq!(m.codes.get(i).copied(), Some(c.expect()));
        }
    }

    #[test]
    fn mutants_detected_ten_of_ten() {
        let l = run_injection();
        assert_eq!(l.per_class, [10, 10, 10]);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
        assert_eq!(l.injected, 30);
    }

    #[test]
    fn finger_print_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_ne!(matrix_digest(Mode::Correct), matrix_digest(Mode::DropRounding));
    }

    #[test]
    fn capacity_and_split_accounting() {
        let mut cap = Arena::new();
        let mut made = 0u64;
        while (made as usize) < MAX_MAPS {
            let r = cap.map(&MapReq {
                hint: ARENA_BASE + PAGE_SIZE * made,
                len: PAGE_SIZE,
                prot: PROT_READ,
                fixed: true,
            });
            if r.is_err() {
                break;
            }
            made += 1;
        }
        assert_eq!(made as usize, MAX_MAPS);
        assert_eq!(
            cap.map(&MapReq { hint: ARENA_BASE + PAGE_SIZE * 20, len: PAGE_SIZE, prot: 0, fixed: true }),
            Err(ENOMEM)
        );

        let mut split = Arena::new();
        assert!(split
            .map(&MapReq { hint: ARENA_BASE, len: PAGE_SIZE * 4, prot: PROT_READ, fixed: true })
            .is_ok());
        assert_eq!(split.unmap_range(ARENA_BASE + PAGE_SIZE, ARENA_BASE + PAGE_SIZE * 2), Ok(1));
        assert_eq!(split.region_count(), 2);
        assert_eq!(split.mapped_pages(), 3);
        assert!(!split.read_ok(ARENA_BASE + PAGE_SIZE, 8));
    }

    #[test]
    fn unmap_keeps_disjoint_regions() {
        let mut a = Arena::new();
        let _ = a.map(&MapReq { hint: ARENA_BASE, len: PAGE_SIZE, prot: PROT_READ, fixed: true });
        let _ = a.map(&MapReq {
            hint: ARENA_BASE + PAGE_SIZE * 4,
            len: PAGE_SIZE,
            prot: PROT_READ,
            fixed: true,
        });
        assert_eq!(a.unmap_range(ARENA_BASE, ARENA_BASE + PAGE_SIZE), Ok(1));
        assert_eq!(a.region_count(), 1);
        assert!(a.read_ok(ARENA_BASE + PAGE_SIZE * 4, 1));
    }

    #[test]
    fn zero_length_request_is_einval() {
        let mut a = Arena::new();
        assert_eq!(
            a.map(&MapReq { hint: 0, len: 0, prot: 0, fixed: false }),
            Err(EINVAL)
        );
        assert_eq!(a.rejects(), 1);
    }
}
