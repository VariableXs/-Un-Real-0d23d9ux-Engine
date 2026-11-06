//! UNX-F9243 mmap 未对齐 EINVAL 矩阵 · 0x7AA40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（判据 UNX-F9243-J1 逐字摘录）：
//!   非页对齐 addr/length 注入矩阵判据过，全 EINVAL 逐格检出（MAP_FIXED
//!   时全 EINVAL），页掩码对账格过。
//!
//! 设计要点：
//!   ① 页掩码是唯一判据：`addr & 0xFFF` 与 `len & 0xFFF` 任一非零即参数
//!      非法，MAP_FIXED 下逐格回 EINVAL；只有 (0,0) 格允许成功。
//!   ② 注入矩阵取 4×4：addr 偏移 {0,1,2,0xFFF} × len 偏移 {0,1,2,0xFFF}，
//!      每格复测 10 轮；非零行优先报 AddrUnaligned，零行非零列报
//!      LenUnaligned，判定次序在格内被逐格锁定。
//!   ③ 判定次序固定为：len==0 → addr 对齐 → len 对齐 → 加法溢出；零长度
//!      先于未对齐、未对齐先于溢出，任何一格错序都判失败。
//!   ④ 页掩码对账：成功跨度必须满足 start/end 双对齐、pages×4096==bytes、
//!      fixed 时 start==addr、hint 时 start==align_down(addr)，四项缺一不可。
//!   ⑤ 提示地址（hint）与固定地址分离：hint 未对齐只向下取整、不报错；
//!      但长度未对齐仍按本卡契约回 EINVAL，且失败路径零落表。
//!   ⑥ 状态机 Grid 单次登记、全格探毕才可封印，二次封印回 GridSealed；
//!      注入账 16×10=160 次，注入 = 检出 + 逃逸且逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9243 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AA40;

/// 页大小与页掩码。
pub const PAGE_SIZE: u64 = 4096;
pub const PAGE_MASK: u64 = !(PAGE_SIZE - 1);

/// 注入矩阵边长、格数与单格复测轮数。
pub const GRID_SIDE: usize = 4;
pub const GRID_CELLS: usize = 16;
pub const INJECT_ROUNDS: u32 = 10;

/// 映射表定容。
pub const SPAN_CAPACITY: usize = 8;

/// 自动分配窗下界。
pub const ALLOC_BASE: u64 = 0x1000_0000;

/// mmap 标志与权限位。
pub const MAP_PRIVATE: u32 = 0x02;
pub const MAP_FIXED: u32 = 0x10;
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;

/// Linux errno 正数码。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;

/// 本模块诊断码（段内 +0x01..+0x0E 闭集，含 errno 归档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignCode {
    LenZero = 1,
    AddrUnaligned = 2,
    LenUnaligned = 3,
    LenWrapped = 4,
    TableFull = 5,
    Overlap = 6,
    MaskBroken = 7,
    OrderBroken = 8,
    RollbackBroken = 9,
    GridUnsealed = 10,
    GridSealed = 11,
    LedgerRegress = 12,
    NotReproducible = 13,
    SegmentViolation = 14,
}

impl AlignCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 映射到 Linux errno（说明码不对应错误）。
    pub fn errno(self) -> i32 {
        match self {
            AlignCode::LenWrapped | AlignCode::TableFull | AlignCode::Overlap => ENOMEM,
            AlignCode::LenZero | AlignCode::AddrUnaligned | AlignCode::LenUnaligned
            | AlignCode::MaskBroken | AlignCode::OrderBroken | AlignCode::RollbackBroken
            | AlignCode::GridUnsealed | AlignCode::GridSealed | AlignCode::LedgerRegress
            | AlignCode::NotReproducible | AlignCode::SegmentViolation => EINVAL,
        }
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AlignCode::LenZero => "长度为零：len==0 回 EINVAL；请给出至少一页的长度",
            AlignCode::AddrUnaligned => "地址未对齐：MAP_FIXED 要求 addr 是 4096 倍数；请先对齐",
            AlignCode::LenUnaligned => "长度未对齐：本卡契约要求 len 是 4096 倍数；请向上取整后重试",
            AlignCode::LenWrapped => "长度回绕：addr+len 溢出 u64 回 ENOMEM；请用 checked 运算复核",
            AlignCode::TableFull => "映射表已满：容量 8 段；请先释放再映射",
            AlignCode::Overlap => "区间重叠：目标范围已有映射；请改址或先解除旧映射",
            AlignCode::MaskBroken => "页掩码对账失败：start/end 未双对齐或页数×4096 不等于字节数",
            AlignCode::OrderBroken => "判定次序被破坏：零长度→地址→长度→溢出的顺序不可交换",
            AlignCode::RollbackBroken => "失败未回滚：被拒参数不得改动映射表；请先判定后落表",
            AlignCode::GridUnsealed => "矩阵未探毕：仍有格子未登记；请跑满 16 格再封印",
            AlignCode::GridSealed => "矩阵已封印：状态机只允许封印一次；请新建矩阵",
            AlignCode::LedgerRegress => "注入账倒退：计数只增不减；请检查回滚与逃逸分支",
            AlignCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            AlignCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [AlignCode; 14] = [
        AlignCode::LenZero, AlignCode::AddrUnaligned, AlignCode::LenUnaligned,
        AlignCode::LenWrapped, AlignCode::TableFull, AlignCode::Overlap,
        AlignCode::MaskBroken, AlignCode::OrderBroken, AlignCode::RollbackBroken,
        AlignCode::GridUnsealed, AlignCode::GridSealed, AlignCode::LedgerRegress,
        AlignCode::NotReproducible, AlignCode::SegmentViolation,
    ];
}

/// 是否页对齐。
pub const fn is_page_aligned(v: u64) -> bool { v & (PAGE_SIZE - 1) == 0 }

/// 向下取整到页边界。
pub const fn align_down(v: u64) -> u64 { v & PAGE_MASK }

/// 一段页对齐映射 [start, end)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

impl Span {
    /// 页数。
    pub fn pages(&self) -> u64 { self.end.saturating_sub(self.start) / PAGE_SIZE }

    /// 字节数。
    pub fn bytes(&self) -> u64 { self.end.saturating_sub(self.start) }

    /// 双边界对齐且非空。
    pub fn aligned(&self) -> bool {
        self.end > self.start && is_page_aligned(self.start) && is_page_aligned(self.end)
    }

    /// 是否包含 addr。
    pub fn contains(&self, addr: u64) -> bool { self.start <= addr && addr < self.end }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MmapRequest {
    pub addr: u64,
    pub len: u64,
    pub prot: u32,
    pub flags: u32,
}

impl MmapRequest {
    /// 固定地址请求。
    pub const fn fixed(addr: u64, len: u64) -> MmapRequest {
        MmapRequest { addr, len, prot: PROT_READ | PROT_WRITE, flags: MAP_PRIVATE | MAP_FIXED }
    }

    /// 提示地址请求。
    pub const fn hint(addr: u64, len: u64) -> MmapRequest {
        MmapRequest { addr, len, prot: PROT_READ, flags: MAP_PRIVATE }
    }
}

/// 固定地址判定：零长度→地址对齐→长度对齐→溢出。
pub fn judge_fixed(addr: u64, len: u64) -> Result<Span, AlignCode> {
    if len == 0 {
        return Err(AlignCode::LenZero);
    }
    if !is_page_aligned(addr) {
        return Err(AlignCode::AddrUnaligned);
    }
    if !is_page_aligned(len) {
        return Err(AlignCode::LenUnaligned);
    }
    let end = match addr.checked_add(len) {
        Some(e) => e,
        None => return Err(AlignCode::LenWrapped),
    };
    if !is_page_aligned(end) {
        return Err(AlignCode::MaskBroken);
    }
    Ok(Span { start: addr, end })
}

/// 提示地址判定：地址向下取整，长度仍须对齐。
pub fn judge_hint(addr: u64, len: u64) -> Result<Span, AlignCode> {
    if len == 0 {
        return Err(AlignCode::LenZero);
    }
    if !is_page_aligned(len) {
        return Err(AlignCode::LenUnaligned);
    }
    let start = align_down(addr);
    let end = match start.checked_add(len) {
        Some(e) => e,
        None => return Err(AlignCode::LenWrapped),
    };
    Ok(Span { start, end })
}

/// 统一判定入口。
pub fn judge(req: &MmapRequest) -> Result<Span, AlignCode> {
    if req.flags & MAP_FIXED != 0 {
        judge_fixed(req.addr, req.len)
    } else {
        judge_hint(req.addr, req.len)
    }
}

/// 页掩码对账：双对齐、页×4096==字节、起点符合模式。
pub fn reconcile(addr: u64, len: u64, fixed: bool) -> Result<Span, AlignCode> {
    let span = if fixed { judge_fixed(addr, len)? } else { judge_hint(addr, len)? };
    if !span.aligned() {
        return Err(AlignCode::MaskBroken);
    }
    if span.pages().saturating_mul(PAGE_SIZE) != span.bytes() {
        return Err(AlignCode::MaskBroken);
    }
    let want = if fixed { addr } else { align_down(addr) };
    if span.start != want || span.bytes() != len {
        return Err(AlignCode::MaskBroken);
    }
    Ok(span)
}

/// 定容有序映射表（容量 8，超出回 ENOMEM）。
#[derive(Clone, Copy, Debug)]
pub struct SpanTable {
    slots: [Option<Span>; SPAN_CAPACITY],
    len: usize,
    rejects: u32,
}

impl SpanTable {
    /// 空表。
    pub const fn new() -> SpanTable {
        SpanTable { slots: [None; SPAN_CAPACITY], len: 0, rejects: 0 }
    }

    /// 段数。
    pub fn len(&self) -> usize { self.len }

    /// 拒绝次数。
    pub fn rejects(&self) -> u32 { self.rejects }

    /// 包含 addr 的段下标。
    pub fn find(&self, addr: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(s)) = self.slots.get(i) {
                if s.contains(addr) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 与 [start, end) 是否相交。
    pub fn overlaps(&self, start: u64, end: u64) -> bool {
        if end <= start {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(s)) = self.slots.get(i) {
                if s.start < end && start < s.end {
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
            if let Some(Some(s)) = self.slots.get(i) {
                n = n.saturating_add(s.bytes());
            }
            i += 1;
        }
        n
    }

    /// 顺序 + 边界签名（FNV-1a）。
    pub fn signature(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(s)) = self.slots.get(i) {
                let mut k = 0usize;
                while k < 8 {
                    bytes.push(((s.start >> (k * 8)) & 0xFF) as u8);
                    bytes.push(((s.end >> (k * 8)) & 0xFF) as u8);
                    k += 1;
                }
            }
            i += 1;
        }
        fnv1a(&bytes)
    }

    /// 有序插入：对齐→重叠→容量。
    pub fn insert(&mut self, span: Span) -> Result<(), AlignCode> {
        if !span.aligned() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AlignCode::MaskBroken);
        }
        if self.overlaps(span.start, span.end) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AlignCode::Overlap);
        }
        if self.len >= SPAN_CAPACITY {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AlignCode::TableFull);
        }
        let mut pos = self.len;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(s)) = self.slots.get(i) {
                if span.start < s.start {
                    pos = i;
                    break;
                }
            }
            i += 1;
        }
        let mut j = self.len;
        while j > pos {
            let prev = match self.slots.get(j - 1) {
                Some(Some(s)) => Some(*s),
                _ => None,
            };
            if let Some(cell) = self.slots.get_mut(j) {
                *cell = prev;
            }
            j -= 1;
        }
        if let Some(cell) = self.slots.get_mut(pos) {
            *cell = Some(span);
        }
        self.len += 1;
        Ok(())
    }
}

/// 固定映射落表：先判参数（EINVAL），再判容量（ENOMEM）。
pub fn map_fixed(table: &mut SpanTable, addr: u64, len: u64) -> Result<Span, AlignCode> {
    let span = judge_fixed(addr, len)?;
    table.insert(span)?;
    Ok(span)
}

/// 四档偏移：对齐、+1、+2、末字节。
pub const GRID_OFFSETS: [u64; GRID_SIDE] = [0, 1, 2, PAGE_SIZE - 1];

/// 行 → 地址。
pub fn grid_addr(row: usize) -> u64 {
    match GRID_OFFSETS.get(row) {
        Some(off) => ALLOC_BASE.saturating_add(*off),
        None => ALLOC_BASE,
    }
}

/// 列 → 长度。
pub fn grid_len(col: usize) -> u64 {
    match GRID_OFFSETS.get(col) {
        Some(off) => PAGE_SIZE.saturating_add(*off),
        None => PAGE_SIZE,
    }
}

/// 格子期望：对角线 (0,0) 成功，非零行先报地址，零行非零列报长度。
pub fn grid_want(row: usize, col: usize) -> Option<AlignCode> {
    if row == 0 && col == 0 {
        None
    } else if row > 0 {
        Some(AlignCode::AddrUnaligned)
    } else {
        Some(AlignCode::LenUnaligned)
    }
}

/// 单格 10 轮判定。
fn cell_caught(row: usize, col: usize) -> u32 {
    let addr = grid_addr(row);
    let len = grid_len(col);
    let want = grid_want(row, col);
    let mut caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut t = SpanTable::new();
        let ok = match (judge(&MmapRequest::fixed(addr, len)), want) {
            (Ok(span), None) => {
                let good = span.start == addr
                    && span.end == addr.saturating_add(len)
                    && span.aligned()
                    && span.pages() == len / PAGE_SIZE;
                if good {
                    let _ = t.insert(span);
                }
                good && t.len() == 1 && t.bytes() == len
            }
            (Err(code), Some(expected)) => code == expected && t.len() == 0,
            _ => false,
        };
        if ok {
            caught = caught.saturating_add(1);
        }
        round += 1;
    }
    caught
}

/// 格子状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellState {
    Unprobed,
    Passed,
    Failed,
}

/// 矩阵状态机：Unprobed → Passed/Failed → Sealed。
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    cells: [CellState; GRID_CELLS],
    probed: u32,
    passed: u32,
    sealed: bool,
}

impl Grid {
    /// 空矩阵。
    pub const fn new() -> Grid {
        Grid { cells: [CellState::Unprobed; GRID_CELLS], probed: 0, passed: 0, sealed: false }
    }

    /// 已登记格数。
    pub fn probed(&self) -> u32 { self.probed }

    /// 通过格数。
    pub fn passed(&self) -> u32 { self.passed }

    /// 是否已封印。
    pub fn is_sealed(&self) -> bool { self.sealed }

    fn record_cell(&mut self, i: usize, ok: bool) -> Result<(), AlignCode> {
        let cur = match self.cells.get(i) {
            Some(s) => *s,
            None => return Err(AlignCode::MaskBroken),
        };
        if cur != CellState::Unprobed {
            return Err(AlignCode::OrderBroken);
        }
        if let Some(cell) = self.cells.get_mut(i) {
            *cell = if ok { CellState::Passed } else { CellState::Failed };
        }
        self.probed = self.probed.saturating_add(1);
        if ok {
            self.passed = self.passed.saturating_add(1);
        }
        Ok(())
    }

    /// 16 格各跑 10 轮并登记（单次）。
    pub fn probe_all(&mut self) -> Result<(), AlignCode> {
        if self.sealed {
            return Err(AlignCode::GridSealed);
        }
        let (ledger, per) = run_grid_injections();
        if !ledger.conserved() || ledger.escaped != 0 {
            return Err(AlignCode::LedgerRegress);
        }
        let mut i = 0usize;
        while i < GRID_CELLS {
            let ok = match per.get(i) {
                Some(n) => *n == INJECT_ROUNDS,
                None => false,
            };
            self.record_cell(i, ok)?;
            i += 1;
        }
        Ok(())
    }

    /// 封印：全格探毕才可封一次，返回通过格数。
    pub fn seal(&mut self) -> Result<u32, AlignCode> {
        if self.sealed {
            return Err(AlignCode::GridSealed);
        }
        if self.probed as usize != GRID_CELLS {
            return Err(AlignCode::GridUnsealed);
        }
        self.sealed = true;
        Ok(self.passed)
    }
}

/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger { InjectLedger { injected: 0, detected: 0, escaped: 0 } }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> Result<(), AlignCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(AlignCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 16 格 × 10 轮，返回账本与逐格检出数。
pub fn run_grid_injections() -> (InjectLedger, [u32; GRID_CELLS]) {
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; GRID_CELLS];
    let mut i = 0usize;
    while i < GRID_CELLS {
        let row = i / GRID_SIDE;
        let col = i % GRID_SIDE;
        let caught = cell_caught(row, col);
        let mut r = 0u32;
        while r < INJECT_ROUNDS {
            let _ = ledger.record(caught == INJECT_ROUNDS);
            r += 1;
        }
        if let Some(slot) = per.get_mut(i) {
            *slot = caught;
        }
        i += 1;
    }
    (ledger, per)
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

/// 规范序列双跑指纹（含矩阵汇总）。
pub fn corpus_fingerprint() -> u32 {
    let fixed_ok = MmapRequest::fixed(ALLOC_BASE, PAGE_SIZE);
    let fixed_addr = MmapRequest::fixed(ALLOC_BASE + 1, PAGE_SIZE);
    let fixed_len = MmapRequest::fixed(ALLOC_BASE, PAGE_SIZE + 1);
    let hint = MmapRequest::hint(ALLOC_BASE + 7, PAGE_SIZE * 2);
    let wrap = MmapRequest::fixed(u64::MAX & PAGE_MASK, PAGE_SIZE * 2);
    let steps = [
        judge(&fixed_ok), judge(&fixed_addr), judge(&fixed_len), judge(&hint), judge(&wrap),
    ];
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < steps.len() {
        match steps.get(i) {
            Some(Ok(span)) => {
                bytes.push(0xA5);
                bytes.push((span.start & 0xFF) as u8);
                bytes.push(span.pages() as u8);
            }
            Some(Err(code)) => bytes.push((*code as u32 & 0xFF) as u8),
            None => {}
        }
        i += 1;
    }
    let (ledger, _per) = run_grid_injections();
    bytes.push(ledger.injected as u8);
    bytes.push(ledger.detected as u8);
    fnv1a(&bytes)
}

/// F9243 判据集（14 静态 + 3 动态）。
pub fn run_unx9243_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9243");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：对齐固定映射
    let normal = {
        let mut t = SpanTable::new();
        let req = MmapRequest::fixed(ALLOC_BASE, PAGE_SIZE * 2);
        let prot_ok = req.prot & PROT_READ != 0 && req.prot & PROT_WRITE != 0;
        let mapped = map_fixed(&mut t, req.addr, req.len);
        match mapped {
            Ok(span) => prot_ok && span.start == ALLOC_BASE
                && span.end == ALLOC_BASE + PAGE_SIZE * 2
                && t.len() == 1 && t.bytes() == PAGE_SIZE * 2
                && t.find(ALLOC_BASE).is_some()
                && t.find(ALLOC_BASE + PAGE_SIZE * 2 - 1).is_some()
                && t.find(ALLOC_BASE + PAGE_SIZE * 2).is_none(),
            Err(_) => false,
        }
    };
    s.add("正常流：对齐 addr/len 固定映射精确落位", normal,
          "成功格必须返回 addr，且 start/end 双对齐、字节账等于 len");

    // 3. 零长度边界
    let zero = judge_fixed(ALLOC_BASE, 0) == Err(AlignCode::LenZero)
        && judge_fixed(ALLOC_BASE + 1, 0) == Err(AlignCode::LenZero)
        && matches!(judge_fixed(ALLOC_BASE, 0), Err(e) if e.errno() == EINVAL);
    s.add("边界：len==0 一律 EINVAL，且先于地址校验", zero,
          "零长度是第一批判据，即使 addr 未对齐也必须报 LenZero");

    // 4. 未对齐地址边界
    let addr_bad = judge_fixed(ALLOC_BASE + 1, PAGE_SIZE) == Err(AlignCode::AddrUnaligned)
        && judge_fixed(ALLOC_BASE + 2, PAGE_SIZE) == Err(AlignCode::AddrUnaligned)
        && judge_fixed(ALLOC_BASE + PAGE_SIZE - 1, PAGE_SIZE) == Err(AlignCode::AddrUnaligned)
        && judge_fixed(ALLOC_BASE, PAGE_SIZE).is_ok();
    s.add("边界：addr 偏移 1/2/0xFFF 全 EINVAL，0 通过", addr_bad,
          "页掩码 addr & 0xFFF 是唯一判据，不许把偏移 1 当合法");

    // 5. 未对齐长度边界
    let len_bad = judge_fixed(ALLOC_BASE, PAGE_SIZE + 1) == Err(AlignCode::LenUnaligned)
        && judge_fixed(ALLOC_BASE, PAGE_SIZE + 2) == Err(AlignCode::LenUnaligned)
        && judge_fixed(ALLOC_BASE, PAGE_SIZE * 2 - 1) == Err(AlignCode::LenUnaligned)
        && judge_fixed(ALLOC_BASE, PAGE_SIZE * 2).is_ok();
    s.add("边界：len 偏移 1/2/0xFFF 全 EINVAL，整页通过", len_bad,
          "本卡契约对长度从严：len & 0xFFF != 0 即拒绝");

    // 6. 判定次序
    let order = judge_fixed(ALLOC_BASE + 1, 0) == Err(AlignCode::LenZero)
        && judge_fixed(ALLOC_BASE + 1, PAGE_SIZE + 1) == Err(AlignCode::AddrUnaligned)
        && judge_fixed(u64::MAX, PAGE_SIZE) == Err(AlignCode::AddrUnaligned)
        && judge_fixed(u64::MAX & PAGE_MASK, PAGE_SIZE * 2) == Err(AlignCode::LenWrapped)
        && matches!(judge_fixed(u64::MAX & PAGE_MASK, PAGE_SIZE * 2),
                    Err(e) if e.errno() == ENOMEM);
    s.add("判定次序：零长度→地址→长度→溢出，不可交换", order,
          "未对齐必须在溢出之前报 EINVAL，溢出才轮到 ENOMEM");

    // 7. 4×4 注入矩阵
    let (ledger, per) = run_grid_injections();
    let grid_all = per.iter().all(|n| *n == INJECT_ROUNDS);
    let grid_ok = grid_all && ledger.conserved() && ledger.escaped == 0
        && ledger.injected == (GRID_CELLS as u32) * INJECT_ROUNDS;
    s.add("4×4 注入矩阵：16 格 × 10 轮逐格全中", grid_ok,
          "非零行报 AddrUnaligned、零行非零列报 LenUnaligned，对角成功");

    // 8. 状态机
    let machine = {
        let mut g = Grid::new();
        let early = g.seal() == Err(AlignCode::GridUnsealed);
        let probed = g.probe_all().is_ok();
        let first = g.seal();
        let second = g.seal();
        let passed = match first {
            Ok(n) => n,
            Err(_) => 0,
        };
        early && probed && passed as usize == GRID_CELLS
            && second == Err(AlignCode::GridSealed)
            && g.is_sealed() && g.probed() as usize == GRID_CELLS
            && g.passed() as usize == GRID_CELLS
    };
    s.add("状态机：未探毕不可封印，封印后二次封印被拒", machine,
          "Unprobed→Passed/Failed→Sealed 三步，单次封印返回通过格数");

    // 9. 容量
    let capacity = {
        let mut t = SpanTable::new();
        let mut i = 0u64;
        let mut filled = true;
        while i < SPAN_CAPACITY as u64 {
            let a = ALLOC_BASE.saturating_add(i.saturating_mul(PAGE_SIZE));
            if map_fixed(&mut t, a, PAGE_SIZE).is_err() {
                filled = false;
            }
            i += 1;
        }
        let ninth = ALLOC_BASE.saturating_add((SPAN_CAPACITY as u64) * PAGE_SIZE);
        let full = map_fixed(&mut t, ninth, PAGE_SIZE) == Err(AlignCode::TableFull);
        let align_first = map_fixed(&mut t, ALLOC_BASE + 1, PAGE_SIZE)
            == Err(AlignCode::AddrUnaligned);
        filled && full && align_first && t.len() == SPAN_CAPACITY
    };
    s.add("容量：8 段后第 9 段 ENOMEM，未对齐仍先 EINVAL", capacity,
          "容量是落表阶段，参数判定必须先于容量判定");

    // 10. 提示地址取整
    let hint = {
        let span = match judge_hint(ALLOC_BASE + 123, PAGE_SIZE * 2) {
            Ok(sp) => sp,
            Err(_) => Span { start: 0, end: 0 },
        };
        let bad_len = judge_hint(ALLOC_BASE + 123, PAGE_SIZE + 1)
            == Err(AlignCode::LenUnaligned);
        span.start == align_down(ALLOC_BASE + 123)
            && span.end == span.start + PAGE_SIZE * 2
            && span.aligned() && span.pages() == 2 && bad_len
    };
    s.add("hint 语义：未对齐地址向下取整，长度仍从严", hint,
          "hint 不是硬约束；但长度未对齐仍按本卡契约回 EINVAL");

    // 11. 页掩码对账
    let reconcile_ok = {
        let fixed = matches!(reconcile(ALLOC_BASE + PAGE_SIZE * 2,
                                       PAGE_SIZE * 3, true),
                             Ok(sp) if sp.pages() == 3 && sp.bytes() == PAGE_SIZE * 3);
        let rounded = matches!(reconcile(ALLOC_BASE + 0xFFF, PAGE_SIZE, false),
                               Ok(sp) if sp.start == ALLOC_BASE && sp.pages() == 1);
        let broken = reconcile(ALLOC_BASE + 1, PAGE_SIZE, true)
            == Err(AlignCode::AddrUnaligned);
        fixed && rounded && broken
    };
    s.add("页掩码对账：双对齐、页×4096==字节、起点符合模式", reconcile_ok,
          "四项对账缺一不可，失败格不得进入成功分支");

    // 12. 失败回滚
    let rollback = {
        let mut t = SpanTable::new();
        let base = map_fixed(&mut t, ALLOC_BASE, PAGE_SIZE);
        let sig = t.signature();
        let bytes = t.bytes();
        let e1 = map_fixed(&mut t, ALLOC_BASE + 1, PAGE_SIZE);
        let e2 = map_fixed(&mut t, ALLOC_BASE, 0);
        base.is_ok() && e1 == Err(AlignCode::AddrUnaligned)
            && e2 == Err(AlignCode::LenZero)
            && t.signature() == sig && t.bytes() == bytes && t.len() == 1
    };
    s.add("回滚：EINVAL 路径零落表、签名与字节账不变", rollback,
          "先判定后落表；失败分支不得 insert，也不得推进计数");

    // 13. 文案闭集
    let mut text_ok = true;
    let mut u = 0usize;
    while u < AlignCode::ALL.len() {
        if let Some(c) = AlignCode::ALL.get(u) {
            if c.detail().is_empty() || !matches!(c.errno(), EINVAL | ENOMEM | 0) {
                text_ok = false;
            }
            let mut v = u + 1;
            while v < AlignCode::ALL.len() {
                if let Some(d) = AlignCode::ALL.get(v) {
                    if c.val() == d.val() {
                        text_ok = false;
                    }
                }
                v += 1;
            }
        }
        u += 1;
    }
    s.add("文案闭集：14 码中文建议非空、码值唯一、errno 归档", text_ok,
          "每个诊断码必须可定位、可操作，且只允许 EINVAL/ENOMEM/说明码");

    // 14. 双跑一致
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范调用序列指纹逐字节相等", fp_a == fp_b,
          "同输入两次必须相等，隐藏状态或计数泄漏即失败");

    // 动态明细
    let cell_min = match per.iter().copied().min() {
        Some(n) => n,
        None => 0,
    };
    let cell_max = match per.iter().copied().max() {
        Some(n) => n,
        None => 0,
    };
    let grid_text = format!("16 格 × {} 轮，最小 {} 最大 {}，注入 {}",
                            INJECT_ROUNDS, cell_min, cell_max, ledger.injected);
    s.add_dyn("矩阵快照", grid_all, &grid_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("对齐矩阵指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_mask_math() {
        assert!(is_page_aligned(PAGE_SIZE * 3));
        assert!(!is_page_aligned(PAGE_SIZE + 1));
        assert_eq!(align_down(PAGE_SIZE + 1), PAGE_SIZE);
    }

    #[test]
    fn fixed_aligned_ok() {
        assert_eq!(judge_fixed(ALLOC_BASE, PAGE_SIZE),
                   Ok(Span { start: ALLOC_BASE, end: ALLOC_BASE + PAGE_SIZE }));
        let mut t = SpanTable::new();
        assert!(map_fixed(&mut t, ALLOC_BASE, PAGE_SIZE).is_ok());
        assert_eq!(t.len(), 1);
        assert_eq!(t.bytes(), PAGE_SIZE);
    }

    #[test]
    fn fixed_unaligned_addr_and_len_einval() {
        let offsets = [1u64, 2, PAGE_SIZE - 1];
        for off in offsets.iter() {
            assert_eq!(judge_fixed(ALLOC_BASE + *off, PAGE_SIZE),
                       Err(AlignCode::AddrUnaligned));
        }
        let lens = [PAGE_SIZE + 1, PAGE_SIZE + 2, PAGE_SIZE * 2 - 1];
        for len in lens.iter() {
            assert_eq!(judge_fixed(ALLOC_BASE, *len), Err(AlignCode::LenUnaligned));
        }
    }

    #[test]
    fn zero_len_wins_over_unaligned_addr() {
        assert_eq!(judge_fixed(ALLOC_BASE + 1, 0), Err(AlignCode::LenZero));
        assert_eq!(judge_fixed(ALLOC_BASE + 1, PAGE_SIZE + 1),
                   Err(AlignCode::AddrUnaligned));
    }

    #[test]
    fn grid_cells_ten_of_ten() {
        let (l, per) = run_grid_injections();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, (GRID_CELLS as u32) * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn grid_state_machine_seals_once() {
        let mut g = Grid::new();
        assert_eq!(g.seal(), Err(AlignCode::GridUnsealed));
        assert!(g.probe_all().is_ok());
        assert_eq!(g.seal(), Ok(GRID_CELLS as u32));
        assert_eq!(g.seal(), Err(AlignCode::GridSealed));
        assert_eq!(g.probe_all(), Err(AlignCode::GridSealed));
        assert!(g.is_sealed());
    }

    #[test]
    fn capacity_enomem_after_eight() {
        let mut t = SpanTable::new();
        let mut i = 0usize;
        while i < SPAN_CAPACITY {
            let a = ALLOC_BASE + (i as u64) * PAGE_SIZE;
            assert!(map_fixed(&mut t, a, PAGE_SIZE).is_ok());
            i += 1;
        }
        let ninth = ALLOC_BASE + (SPAN_CAPACITY as u64) * PAGE_SIZE;
        assert_eq!(map_fixed(&mut t, ninth, PAGE_SIZE), Err(AlignCode::TableFull));
        assert_eq!(map_fixed(&mut t, ALLOC_BASE + 1, PAGE_SIZE),
                   Err(AlignCode::AddrUnaligned));
        assert_eq!(t.rejects(), 1);
    }

    #[test]
    fn fingerprint_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9243_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
