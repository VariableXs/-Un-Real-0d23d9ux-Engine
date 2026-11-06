//! UNX-F9285 fork/clone ENOMEM 矩阵（地址空间耗尽） · 0x7B4C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9285 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9285-J1 地址空间耗尽注入判据过，
//!   全 ENOMEM 格过，COW 挂点依赖声明格过；③ 完成时 API/软件写回：状态=已完成、
//!   结果摘要非空。
//!
//! 设计要点：
//!   ① ENOMEM 只出自「资源面」：页预算跌破水位、地址空间越界、地址算术溢出。
//!      额度面（nproc/线程表/pid）是 EAGAIN，属 F9284；本模块整表扫描拒绝
//!      EAGAIN，边界写死。
//!   ② 复刻 fork 的十步挂点（任务结构→内核栈→信号→fd 表→PGD→PMD→anon_vma→
//!      COW 页→swap 槽→PTE 锁），顺序冻结。注入方式是「让第 k 个挂点恰好成为
//!      第一个失败点」：预算 = 水位 + 前 k 步页数，于是前 k 步成功、第 k 步跌破
//!      水位。这样每个挂点都被单独打中一次，而不是只打第一个。
//!   ③ 失败必须原子：任何一步失败都归还本次已扣的全部页，free 回到初值、
//!      live_pages 归零、va.used 不前进。半扣（扣了不还）会让下一次 fork 假性
//!      缺页，属最难查的一类预算泄漏。
//!   ④ COW 挂点依赖声明：凡触碰 mm 的挂点（PGD 之后六步）必须显式声明一个
//!      COW 依赖（页表钉住/anon_vma 链/页引用计数/swap 槽钉住/PTE 写锁）；
//!      不碰 mm 的四步（任务结构/内核栈/信号/fd 表）必须声明 None。两向都要
//!      精确——全声明成 COW 与全不声明同样不可接受。声明检查先于任何扣页。
//!   ⑤ 地址空间两档耗尽：`used + need > limit` 回 ENOMEM；`used + need` 在 u64
//!      上溢出也回 ENOMEM（checked_add 不 panic、不回绕）。
//!   ⑥ 注入账：12 格 × 10 轮 = 120 次全检出，escaped 恒 0；指纹双跑相等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9285 独占 0x7B4C0..=0x7B4FF）。
pub const SEG_BASE: u32 = 0x7B4C0;
/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7B400;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// 页大小。
pub const PAGE: u64 = 4096;
/// 低水位：free 低于它即拒绝分配（保留应急页）。
pub const WATERMARK: u64 = 64;
/// 挂点行数（fork 的十步）。
pub const HOOK_ROWS: usize = 10;
/// 矩阵格数（十挂点 + VA 越界 + VA 溢出）。
pub const ENOMEM_CELLS: usize = 12;
/// 单格注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 默认地址空间上限（1 MiB）。
pub const VA_LIMIT: u64 = 1024 * 1024;

/// ENOMEM：资源面耗尽。
pub const ENOMEM: i32 = 12;
/// EAGAIN：额度面（本模块禁止返回）。
pub const EAGAIN: i32 = 11;
/// EINVAL：flags 非法或 COW 声明缺失/多余。
pub const EINVAL: i32 = 22;

/// CLONE_VM：共享地址空间（跳过全部 mm 挂点）。
pub const CLONE_VM: u64 = 0x0000_0100;
/// CLONE_VFORK：父进程挂起直到子 exec。
pub const CLONE_VFORK: u64 = 0x0000_4000;
/// CLONE_THREAD：线程克隆。
pub const CLONE_THREAD: u64 = 0x0001_0000;
/// 合法 clone 位全集。
pub const VALID_FLAGS: u64 = CLONE_VM | CLONE_VFORK | CLONE_THREAD;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemCode {
    PageShortfall,
    WatermarkBreach,
    VaExhausted,
    VaOverflow,
    UndeclaredCowHook,
    OverDeclaredHook,
    OrderViolation,
    RefundLeak,
    WrongErrno,
    Escaped,
    NotReproducible,
    SegmentViolation,
}

impl MemCode {
    /// 闭集下标。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self.code() as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            MemCode::PageShortfall => "页预算不足（ENOMEM）：扣页后低于低水位；请先回收页或降低 fork 的内存需求",
            MemCode::WatermarkBreach => "水位保护触发（ENOMEM）：free 跌破保留水位；不要把应急页算进可分配额度",
            MemCode::VaExhausted => "地址空间耗尽（ENOMEM）：used + need 超过 limit；请扩大地址空间或先 munmap",
            MemCode::VaOverflow => "地址算术溢出（ENOMEM）：used + need 在 u64 上溢出；请用 checked_add 先验算，禁止回绕",
            MemCode::UndeclaredCowHook => "COW 依赖缺声明（EINVAL）：mm 挂点必须声明页表/anon_vma/引用计数/swap/PTE 之一；请补齐声明",
            MemCode::OverDeclaredHook => "COW 依赖多声明（EINVAL）：不碰 mm 的挂点必须声明 None；全声明成 COW 等于没有声明",
            MemCode::OrderViolation => "挂点次序错误：十步必须按 任务结构→内核栈→信号→fd→PGD→PMD→anon_vma→COW 页→swap→PTE 冻结",
            MemCode::RefundLeak => "失败未原子（页泄漏）：失败路径必须归还本次全部扣页；请检查 refund 覆盖所有出口",
            MemCode::WrongErrno => "错误码不符：资源面只许 ENOMEM，额度面才是 EAGAIN；两码混用会误导调用方的重试策略",
            MemCode::Escaped => "注入逃逸：挂点失败未被检出；请确认 budget.charge 的失败分支可达",
            MemCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            MemCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [MemCode] = &[
        MemCode::PageShortfall,
        MemCode::WatermarkBreach,
        MemCode::VaExhausted,
        MemCode::VaOverflow,
        MemCode::UndeclaredCowHook,
        MemCode::OverDeclaredHook,
        MemCode::OrderViolation,
        MemCode::RefundLeak,
        MemCode::WrongErrno,
        MemCode::Escaped,
        MemCode::NotReproducible,
        MemCode::SegmentViolation,
    ];
}

// ── 挂点与 COW 依赖 ───────────────────────────────────────────────────
/// fork 的十个挂点（顺序即冻结次序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HangPoint {
    TaskStruct,
    KernelStack,
    SignalStruct,
    FdTable,
    Pgd,
    Pmd,
    AnonVma,
    CowPage,
    SwapSlot,
    PteLock,
    /// 伪挂点：页扣完后做地址预留（不在 HOOK_ROWS 内，仅用于诊断定位）。
    VaReserve,
}

impl HangPoint {
    /// 闭集下标（VaReserve 排最后，不参与十行表）。
    pub fn code(self) -> u16 {
        match self {
            HangPoint::TaskStruct => 0,
            HangPoint::KernelStack => 1,
            HangPoint::SignalStruct => 2,
            HangPoint::FdTable => 3,
            HangPoint::Pgd => 4,
            HangPoint::Pmd => 5,
            HangPoint::AnonVma => 6,
            HangPoint::CowPage => 7,
            HangPoint::SwapSlot => 8,
            HangPoint::PteLock => 9,
            HangPoint::VaReserve => 10,
        }
    }

    /// 是否触碰 mm（PGD 及其后的 mm 挂点 + 伪 VA 预留）。
    pub fn is_mm(self) -> bool {
        matches!(
            self,
            HangPoint::Pgd
                | HangPoint::Pmd
                | HangPoint::AnonVma
                | HangPoint::CowPage
                | HangPoint::SwapSlot
                | HangPoint::PteLock
                | HangPoint::VaReserve
        )
    }

    /// 名册。
    pub fn name(self) -> &'static str {
        match self {
            HangPoint::TaskStruct => "task_struct",
            HangPoint::KernelStack => "kernel_stack",
            HangPoint::SignalStruct => "signal_struct",
            HangPoint::FdTable => "fd_table",
            HangPoint::Pgd => "pgd",
            HangPoint::Pmd => "pmd",
            HangPoint::AnonVma => "anon_vma",
            HangPoint::CowPage => "cow_page",
            HangPoint::SwapSlot => "swap_slot",
            HangPoint::PteLock => "pte_lock",
            HangPoint::VaReserve => "va_reserve",
        }
    }
}

/// COW 依赖种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CowDep {
    /// 不碰 mm，必须显式声明无依赖。
    None,
    /// 页表页钉住。
    PageTablePin,
    /// anon_vma 链。
    AnonVmaChain,
    /// 页引用计数（写时复制）。
    PageRefcount,
    /// swap 槽钉住。
    SwapSlotPin,
    /// PTE 写锁。
    PteWriteLock,
}

impl CowDep {
    /// 是否是一个真实的 mm 依赖。
    pub fn is_real(self) -> bool {
        !matches!(self, CowDep::None)
    }

    /// 名册。
    pub fn name(self) -> &'static str {
        match self {
            CowDep::None => "none",
            CowDep::PageTablePin => "page_table_pin",
            CowDep::AnonVmaChain => "anon_vma_chain",
            CowDep::PageRefcount => "page_refcount",
            CowDep::SwapSlotPin => "swap_slot_pin",
            CowDep::PteWriteLock => "pte_write_lock",
        }
    }
}

/// 一行挂点声明：页成本、COW 依赖、是否已声明、预留地址字节。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hook {
    pub point: HangPoint,
    pub pages: u64,
    pub dep: CowDep,
    pub declared: bool,
    pub va_bytes: u64,
}

/// 十行冻结挂点表。
pub const HOOKS: [Hook; HOOK_ROWS] = [
    Hook { point: HangPoint::TaskStruct, pages: 2, dep: CowDep::None, declared: true, va_bytes: 0 },
    Hook { point: HangPoint::KernelStack, pages: 4, dep: CowDep::None, declared: true, va_bytes: 0 },
    Hook { point: HangPoint::SignalStruct, pages: 1, dep: CowDep::None, declared: true, va_bytes: 0 },
    Hook { point: HangPoint::FdTable, pages: 1, dep: CowDep::None, declared: true, va_bytes: 0 },
    Hook { point: HangPoint::Pgd, pages: 1, dep: CowDep::PageTablePin, declared: true, va_bytes: PAGE },
    Hook { point: HangPoint::Pmd, pages: 1, dep: CowDep::PageTablePin, declared: true, va_bytes: PAGE },
    Hook { point: HangPoint::AnonVma, pages: 1, dep: CowDep::AnonVmaChain, declared: true, va_bytes: 2 * PAGE },
    Hook { point: HangPoint::CowPage, pages: 3, dep: CowDep::PageRefcount, declared: true, va_bytes: 3 * PAGE },
    Hook { point: HangPoint::SwapSlot, pages: 1, dep: CowDep::SwapSlotPin, declared: true, va_bytes: PAGE },
    Hook { point: HangPoint::PteLock, pages: 1, dep: CowDep::PteWriteLock, declared: true, va_bytes: 0 },
];

/// 前 k 步累计页数（k 取 0..=HOOK_ROWS）。
pub fn prefix_pages(k: usize) -> u64 {
    let mut sum = 0u64;
    let mut i = 0usize;
    while i < k {
        if let Some(h) = HOOKS.get(i) {
            sum = sum.saturating_add(h.pages);
        }
        i += 1;
    }
    sum
}

/// 前 k 步累计地址字节数。
pub fn prefix_bytes(k: usize) -> u64 {
    let mut sum = 0u64;
    let mut i = 0usize;
    while i < k {
        if let Some(h) = HOOKS.get(i) {
            sum = sum.saturating_add(h.va_bytes);
        }
        i += 1;
    }
    sum
}

/// 声明面判定：返回违规行数（0 = 通过）。
pub fn cow_decl_gaps(hooks: &[Hook]) -> u32 {
    let mut gaps = 0u32;
    let mut i = 0usize;
    while i < hooks.len() {
        if let Some(h) = hooks.get(i) {
            let dep_ok = if h.point.is_mm() {
                h.dep.is_real()
            } else {
                h.dep == CowDep::None
            };
            if !h.declared || !dep_ok {
                gaps = gaps.saturating_add(1);
            }
        }
        i += 1;
    }
    gaps
}

/// 声明面是否完备。
pub fn cow_decl_complete(hooks: &[Hook]) -> bool {
    cow_decl_gaps(hooks) == 0
}

// ── 页预算 ────────────────────────────────────────────────────────────
/// 页预算：free / 低水位 / 累计扣 / 累计还 / 初值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageBudget {
    free: u64,
    watermark: u64,
    charged: u64,
    released: u64,
    initial: u64,
}

impl PageBudget {
    /// 建预算（初值 = free）。
    pub const fn new(free: u64, watermark: u64) -> PageBudget {
        PageBudget {
            free,
            watermark,
            charged: 0,
            released: 0,
            initial: free,
        }
    }

    /// 当前空闲页。
    pub fn free(&self) -> u64 {
        self.free
    }

    /// 低水位。
    pub fn watermark(&self) -> u64 {
        self.watermark
    }

    /// 累计扣页。
    pub fn charged_pages(&self) -> u64 {
        self.charged
    }

    /// 累计还页。
    pub fn refunded_pages(&self) -> u64 {
        self.released
    }

    /// 在途页（charged - released）。
    pub fn live_pages(&self) -> u64 {
        self.charged.saturating_sub(self.released)
    }

    /// 守恒：在途页 == 初值 - 当前空闲。
    pub fn conserved(&self) -> bool {
        self.live_pages() == self.initial.saturating_sub(self.free)
    }

    /// 扣页：扣完必须 >= 低水位；下溢/跌破一律 ENOMEM 且不改状态。
    pub fn charge(&mut self, pages: u64) -> Result<u64, i32> {
        let after = match self.free.checked_sub(pages) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        if after < self.watermark {
            return Err(ENOMEM);
        }
        self.free = after;
        self.charged = self.charged.saturating_add(pages);
        Ok(after)
    }

    /// 还页（失败回滚用）。只增 released，不回头改 charged 的历史峰值。
    pub fn refund(&mut self, pages: u64) {
        self.free = self.free.saturating_add(pages);
        self.released = self.released.saturating_add(pages);
    }

    /// 失败是否已完全回滚。
    pub fn rolled_back(&self, initial: u64) -> bool {
        self.free == initial && self.live_pages() == 0 && self.conserved()
    }
}

// ── 地址空间 ──────────────────────────────────────────────────────────
/// 地址空间账：已用字节 + 上限。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VaSpace {
    used: u64,
    limit: u64,
}

impl VaSpace {
    /// 建空间。
    pub const fn new(limit: u64, used: u64) -> VaSpace {
        VaSpace { used, limit }
    }

    /// 已用字节。
    pub fn used(&self) -> u64 {
        self.used
    }

    /// 上限。
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// 预留：溢出与越界都回 ENOMEM，成功才前进。
    pub fn reserve(&mut self, bytes: u64) -> Result<u64, i32> {
        let end = match self.used.checked_add(bytes) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        if end > self.limit {
            return Err(ENOMEM);
        }
        self.used = end;
        Ok(end)
    }
}

// ── fork 计划 ─────────────────────────────────────────────────────────
/// 一次 fork 成功的轨迹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ForkTrace {
    pub pages: u64,
    pub va_end: u64,
    pub cow_hooks: u32,
    pub points: u32,
}

/// 一次 fork 失败的结构化错误（比裸 errno 多带「失败前已扣页」与挂点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ForkErr {
    pub errno: i32,
    pub pages_before: u64,
    pub point: HangPoint,
}

/// 带声明检查与原子回滚的 fork 判定。
///
/// 次序（冻结）：flags EINVAL → COW 声明 EINVAL → 逐挂点扣页 ENOMEM →
/// 地址预留 ENOMEM。任何失败都归还本次已扣页，va 不前进。
pub fn fork_with(
    budget: &mut PageBudget,
    va: &mut VaSpace,
    flags: u64,
    hooks: &[Hook],
) -> Result<ForkTrace, ForkErr> {
    // ① 参数面。
    if flags & !VALID_FLAGS != 0 {
        return Err(ForkErr {
            errno: EINVAL,
            pages_before: 0,
            point: HangPoint::TaskStruct,
        });
    }
    let skip_mm = (flags & CLONE_VM) != 0;

    // ② 声明面：先于任何扣页，缺声明时预算必须纹丝不动。
    let mut i = 0usize;
    while i < hooks.len() {
        if let Some(h) = hooks.get(i) {
            let touching = !h.point.is_mm() || !skip_mm;
            if touching {
                let dep_ok = if h.point.is_mm() {
                    h.dep.is_real()
                } else {
                    h.dep == CowDep::None
                };
                if !h.declared || !dep_ok {
                    return Err(ForkErr {
                        errno: EINVAL,
                        pages_before: 0,
                        point: h.point,
                    });
                }
            }
        }
        i += 1;
    }

    // ③ 逐挂点扣页。
    let mut charged = 0u64;
    let mut bytes = 0u64;
    let mut cow = 0u32;
    let mut points = 0u32;
    i = 0usize;
    while i < hooks.len() {
        if let Some(h) = hooks.get(i) {
            let touching = !h.point.is_mm() || !skip_mm;
            if touching {
                match budget.charge(h.pages) {
                    Ok(_) => {
                        charged = charged.saturating_add(h.pages);
                        bytes = bytes.saturating_add(h.va_bytes);
                        if h.point.is_mm() {
                            cow = cow.saturating_add(1);
                        }
                        points = points.saturating_add(1);
                    }
                    Err(_) => {
                        budget.refund(charged);
                        return Err(ForkErr {
                            errno: ENOMEM,
                            pages_before: charged,
                            point: h.point,
                        });
                    }
                }
            }
        }
        i += 1;
    }

    // ④ 地址预留（页已全部扣好；失败时整批归还）。
    match va.reserve(bytes) {
        Ok(end) => Ok(ForkTrace {
            pages: charged,
            va_end: end,
            cow_hooks: cow,
            points,
        }),
        Err(_) => {
            budget.refund(charged);
            Err(ForkErr {
                errno: ENOMEM,
                pages_before: charged,
                point: HangPoint::VaReserve,
            })
        }
    }
}

/// 标准 fork（十行冻结表 + 默认地址空间）。
pub fn fork_clone(budget: &mut PageBudget, va: &mut VaSpace, flags: u64) -> Result<ForkTrace, ForkErr> {
    fork_with(budget, va, flags, &HOOKS)
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 一格：`fail_at` 0..=9 指定挂点，10 = VA 越界，11 = VA 溢出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnomemCell {
    pub fail_at: u8,
}

/// 十二格冻结矩阵。
pub const ENOMEM_GRID: [EnomemCell; ENOMEM_CELLS] = [
    EnomemCell { fail_at: 0 },
    EnomemCell { fail_at: 1 },
    EnomemCell { fail_at: 2 },
    EnomemCell { fail_at: 3 },
    EnomemCell { fail_at: 4 },
    EnomemCell { fail_at: 5 },
    EnomemCell { fail_at: 6 },
    EnomemCell { fail_at: 7 },
    EnomemCell { fail_at: 8 },
    EnomemCell { fail_at: 9 },
    EnomemCell { fail_at: 10 },
    EnomemCell { fail_at: 11 },
];

/// 该格的期望「失败前已扣页」。
pub fn expected_pages_before(cell: &EnomemCell) -> u64 {
    if (cell.fail_at as usize) < HOOK_ROWS {
        prefix_pages(cell.fail_at as usize)
    } else {
        prefix_pages(HOOK_ROWS)
    }
}

/// 按格构造预算与地址空间。
pub fn cell_setup(cell: &EnomemCell) -> (PageBudget, VaSpace) {
    if (cell.fail_at as usize) < HOOK_ROWS {
        let free = WATERMARK.saturating_add(prefix_pages(cell.fail_at as usize));
        (PageBudget::new(free, WATERMARK), VaSpace::new(VA_LIMIT, 0))
    } else if cell.fail_at == 10 {
        // 页充足，地址空间只给 16 KiB（计划需 32 KiB）。
        (
            PageBudget::new(WATERMARK.saturating_add(64), WATERMARK),
            VaSpace::new(16 * 1024, 0),
        )
    } else {
        // 页充足，地址空间已逼近 u64 上界 → checked_add 溢出。
        (
            PageBudget::new(WATERMARK.saturating_add(64), WATERMARK),
            VaSpace::new(u64::MAX, u64::MAX.saturating_sub(1024)),
        )
    }
}

/// 复算单格：必须恰好 ENOMEM、失败前页数对、预算与 va 全回滚。
pub fn cell_matches(cell: &EnomemCell) -> bool {
    let (mut budget, mut va) = cell_setup(cell);
    let initial = budget.free();
    let va_initial = va.used();
    let expect = expected_pages_before(cell);
    match fork_clone(&mut budget, &mut va, 0) {
        Ok(_) => false,
        Err(e) => {
            e.errno == ENOMEM
                && e.pages_before == expect
                && budget.rolled_back(initial)
                && va.used() == va_initial
        }
    }
}

/// 资源面注入账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub pages_charged: u64,
    pub pages_refunded: u64,
}

impl BudgetLedger {
    /// 空账。
    pub const fn zero() -> BudgetLedger {
        BudgetLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            pages_charged: 0,
            pages_refunded: 0,
        }
    }

    /// 记一笔注入（含该次扣/还页数）。
    pub fn record(&mut self, caught: bool, charged: u64, refunded: u64) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.pages_charged = self.pages_charged.saturating_add(charged);
        self.pages_refunded = self.pages_refunded.saturating_add(refunded);
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }

    /// 失败路径的页必须全数归还（charged 只记成功扣页，故此处应相等）。
    pub fn balanced(&self) -> bool {
        self.pages_charged == self.pages_refunded
    }
}

/// 十二格 × 十轮。每轮独立现场，失败后预算归零。
pub fn run_enomem_matrix() -> ([u32; ENOMEM_CELLS], BudgetLedger) {
    let mut per_cell = [0u32; ENOMEM_CELLS];
    let mut ledger = BudgetLedger::zero();
    let mut ci = 0usize;
    while ci < ENOMEM_CELLS {
        if let Some(cell) = ENOMEM_GRID.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let (mut budget, mut va) = cell_setup(&cell);
                let initial = budget.free();
                let va_initial = va.used();
                let expect = expected_pages_before(&cell);
                let caught = match fork_clone(&mut budget, &mut va, 0) {
                    Ok(_) => false,
                    Err(e) => {
                        e.errno == ENOMEM
                            && e.pages_before == expect
                            && budget.rolled_back(initial)
                            && va.used() == va_initial
                    }
                };
                ledger.record(caught, budget.charged_pages(), budget.refunded_pages());
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

/// 编码一个 fork 结果。
pub fn encode_fork(out: &mut Vec<u8>, r: Result<ForkTrace, ForkErr>) {
    match r {
        Ok(t) => {
            out.push(1);
            out.push((t.pages & 0xFF) as u8);
            out.push((t.va_end & 0xFF) as u8);
            out.push((t.cow_hooks & 0xFF) as u8);
        }
        Err(e) => {
            out.push(2);
            out.push((e.errno & 0x7F) as u8);
            out.push((e.pages_before & 0xFF) as u8);
            out.push(e.point.code() as u8);
        }
    }
}

/// 规范语料双跑指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut b1 = PageBudget::new(1024, WATERMARK);
    let mut v1 = VaSpace::new(VA_LIMIT, 0);
    encode_fork(&mut bytes, fork_clone(&mut b1, &mut v1, 0));
    encode_fork(&mut bytes, fork_clone(&mut b1, &mut v1, CLONE_VM));
    encode_fork(&mut bytes, fork_clone(&mut b1, &mut v1, 0x8000_0000));
    let mut b2 = PageBudget::new(WATERMARK, WATERMARK);
    let mut v2 = VaSpace::new(VA_LIMIT, 0);
    encode_fork(&mut bytes, fork_clone(&mut b2, &mut v2, 0));
    let mut b3 = PageBudget::new(1024, WATERMARK);
    let mut v3 = VaSpace::new(16 * 1024, 0);
    encode_fork(&mut bytes, fork_clone(&mut b3, &mut v3, 0));
    let mut b4 = PageBudget::new(1024, WATERMARK);
    let mut v4 = VaSpace::new(u64::MAX, u64::MAX - 4096);
    encode_fork(&mut bytes, fork_clone(&mut b4, &mut v4, 0));
    fnv1a(&bytes)
}

// ── errno 对照 ────────────────────────────────────────────────────────
/// (名称, Linux 正值, 归属面)。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EAGAIN", 11, "额度面：nproc/线程表/pid，F9284 独占"),
    ("ENOMEM", 12, "资源面：页预算/地址空间/算术溢出"),
    ("EINVAL", 22, "参数面：flags 非法或 COW 声明不精确"),
];

/// 按名查码；表外 0。
pub fn errno_value(name: &str) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.0 == name {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 本模块允许返回的错误码（EAGAIN 明确排除）。
pub fn mem_errno_allowed(code: i32) -> bool {
    code == errno_value("ENOMEM") || code == errno_value("EINVAL")
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9285 判据集（add ≥12）。
pub fn run_unx9285_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9285");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE >= WINDOW_LO
        && SEG_BASE <= WINDOW_HI
        && SEG_BASE >= 0x7B4C0
        && SEG_BASE + 0x3F < 0x7B500;
    s.add(
        "段位守卫：0x7B4C0 窗口且与 F9284/F9286 不重叠",
        seg_ok,
        "段基址必须 0x40 对齐并落在 0x7B4C0..=0x7B4FF",
    );

    // 水位边界：free=watermark 时再扣 1 页必失败；free=watermark+1 成功。
    let mut exact = PageBudget::new(WATERMARK, WATERMARK);
    let fail_at_wm = exact.charge(1);
    let mut plus1 = PageBudget::new(WATERMARK.saturating_add(1), WATERMARK);
    let ok_at_wm = plus1.charge(1);
    let wm_ok = fail_at_wm == Err(ENOMEM)
        && ok_at_wm == Ok(WATERMARK)
        && plus1.free() == WATERMARK
        && exact.free() == WATERMARK;
    s.add(
        "水位边界：跌破水位 ENOMEM、恰好保留水位放行",
        wm_ok,
        "低水位是保留页，扣完必须 >= watermark，不能让应急页被 fork 吃掉",
    );

    // checked 扣页：u64::MAX 不得下溢 panic，也不得回绕。
    let mut huge = PageBudget::new(128, WATERMARK);
    let huge_err = huge.charge(u64::MAX);
    s.add(
        "算术面：超大扣页 ENOMEM 且状态不变",
        huge_err == Err(ENOMEM) && huge.free() == 128 && huge.live_pages() == 0,
        "checked_sub 失败必须直接返回，绝不回绕成天文数字的空闲页",
    );

    // 成功流：16 页 + 32 KiB 地址 + 6 个 COW 挂点。
    let mut b_ok = PageBudget::new(1024, WATERMARK);
    let mut v_ok = VaSpace::new(VA_LIMIT, 0);
    let trace = fork_clone(&mut b_ok, &mut v_ok, 0);
    let success_ok = (match trace {
        Ok(t) => {
            t.pages == 16
                && t.va_end == 32 * 1024
                && t.cow_hooks == 6
                && t.points == HOOK_ROWS as u32
        }
        Err(_) => false,
    }) && b_ok.live_pages() == 16
        && v_ok.used() == 32 * 1024;
    s.add(
        "成功流：16 页、32 KiB、6 个 COW 挂点",
        success_ok,
        "十步挂点全过 + 地址预留成功，页与地址必须同步前进",
    );

    // CLONE_VM：跳过全部 mm 挂点（8 页、0 COW、0 地址）。
    let mut b_vm = PageBudget::new(1024, WATERMARK);
    let mut v_vm = VaSpace::new(VA_LIMIT, 0);
    let vm_trace = fork_clone(&mut b_vm, &mut v_vm, CLONE_VM);
    let vm_ok = match vm_trace {
        Ok(t) => t.pages == 8 && t.cow_hooks == 0 && t.points == 4 && t.va_end == 0,
        Err(_) => false,
    };
    s.add(
        "CLONE_VM：mm 挂点整体跳过",
        vm_ok,
        "共享地址空间的克隆不复制页表，只扣 4 个非 mm 步",
    );

    // 全 ENOMEM 格：12 格 × 10 轮。
    let (per_cell, ledger) = run_enomem_matrix();
    let mut cells_all = true;
    let mut ci = 0usize;
    while ci < ENOMEM_CELLS {
        match per_cell.get(ci) {
            Some(n) => {
                if *n != INJECT_ROUNDS {
                    cells_all = false;
                }
            }
            None => cells_all = false,
        }
        ci += 1;
    }
    s.add(
        "全 ENOMEM 格：12 格各 10/10 命中",
        cells_all,
        "十挂点逐个打中 + VA 越界 + VA 溢出，格格都要失败在对的位置",
    );
    s.add(
        "注入账：120 次全检出且逃逸为 0",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.injected == ENOMEM_CELLS as u32 * INJECT_ROUNDS
            && ledger.balanced(),
        "injected == detected + escaped，且失败路径扣页与还页必须相等",
    );

    // 失败原子性：每格失败后 free 回初值、在途 0、va 未前进。
    let mut atomic_ok = true;
    let mut ai = 0usize;
    while ai < ENOMEM_GRID.len() {
        if let Some(cell) = ENOMEM_GRID.get(ai).copied() {
            let (mut b, mut v) = cell_setup(&cell);
            let initial = b.free();
            let va_initial = v.used();
            if let Err(e) = fork_clone(&mut b, &mut v, 0) {
                if e.errno != ENOMEM || !b.rolled_back(initial) || v.used() != va_initial {
                    atomic_ok = false;
                }
            } else {
                atomic_ok = false;
            }
        }
        ai += 1;
    }
    s.add(
        "失败原子性：回滚后 free 回初值且 va 不前进",
        atomic_ok,
        "半扣是最隐蔽的预算泄漏，失败出口必须整批 refund",
    );

    // 地址空间越界：16 KiB 上限挡住 32 KiB 计划。
    let mut b_va = PageBudget::new(1024, WATERMARK);
    let mut v_va = VaSpace::new(16 * 1024, 0);
    let va_err = fork_clone(&mut b_va, &mut v_va, 0);
    let va_ok = matches!(va_err, Err(e) if e.errno == ENOMEM && e.point == HangPoint::VaReserve)
        && b_va.live_pages() == 0
        && v_va.used() == 0;
    s.add(
        "地址空间：limit 越界回 ENOMEM 且整批还页",
        va_ok,
        "地址预留是最后一步，失败时前面的页必须全部归还",
    );

    // 地址算术溢出：used 逼近 u64::MAX。
    let mut b_of = PageBudget::new(1024, WATERMARK);
    let mut v_of = VaSpace::new(u64::MAX, u64::MAX.saturating_sub(1024));
    let of_err = fork_clone(&mut b_of, &mut v_of, 0);
    let of_ok = matches!(of_err, Err(e) if e.errno == ENOMEM && e.point == HangPoint::VaReserve)
        && b_of.live_pages() == 0;
    s.add(
        "地址溢出：checked_add 失败回 ENOMEM",
        of_ok,
        "u64 上溢不得回绕到低地址，必须按资源耗尽处理",
    );

    // COW 声明：默认十行零缺口，mm 六行各自声明一种依赖。
    let gaps = cow_decl_gaps(&HOOKS);
    let mut mm_rows = 0u32;
    let mut dep_ok = true;
    let mut hi = 0usize;
    while hi < HOOKS.len() {
        if let Some(h) = HOOKS.get(hi) {
            if h.point.is_mm() {
                mm_rows = mm_rows.saturating_add(1);
                if !h.dep.is_real() {
                    dep_ok = false;
                }
            } else if h.dep != CowDep::None {
                dep_ok = false;
            }
        }
        hi += 1;
    }
    s.add(
        "COW 声明格：十行零缺口、六行 mm 各有实依赖",
        gaps == 0 && dep_ok && mm_rows == 6 && cow_decl_complete(&HOOKS),
        "mm 挂点缺声明与不碰 mm 的挂点多声明都必须为 0",
    );

    // 缺声明负例：CowPage 的 dep 被清空 → EINVAL 且不扣页。
    let mut bad_missing: Vec<Hook> = Vec::new();
    let mut k = 0usize;
    while k < HOOKS.len() {
        if let Some(h) = HOOKS.get(k) {
            let mut copy = *h;
            if copy.point == HangPoint::CowPage {
                copy.dep = CowDep::None;
            }
            bad_missing.push(copy);
        }
        k += 1;
    }
    let mut b_miss = PageBudget::new(1024, WATERMARK);
    let mut v_miss = VaSpace::new(VA_LIMIT, 0);
    let miss_err = fork_with(&mut b_miss, &mut v_miss, 0, &bad_missing);
    s.add(
        "缺声明负例：COW 页挂点无依赖回 EINVAL 且零扣页",
        matches!(miss_err, Err(e) if e.errno == EINVAL && e.point == HangPoint::CowPage)
            && b_miss.live_pages() == 0
            && b_miss.free() == 1024,
        "声明检查必须先于分配，否则坏声明会先扣页再报错",
    );

    // 多声明负例：TaskStruct 谎报 COW 依赖 → EINVAL。
    let mut bad_extra: Vec<Hook> = Vec::new();
    let mut k2 = 0usize;
    while k2 < HOOKS.len() {
        if let Some(h) = HOOKS.get(k2) {
            let mut copy = *h;
            if copy.point == HangPoint::TaskStruct {
                copy.dep = CowDep::PageRefcount;
            }
            bad_extra.push(copy);
        }
        k2 += 1;
    }
    let mut b_extra = PageBudget::new(1024, WATERMARK);
    let mut v_extra = VaSpace::new(VA_LIMIT, 0);
    let extra_err = fork_with(&mut b_extra, &mut v_extra, 0, &bad_extra);
    s.add(
        "多声明负例：非 mm 挂点谎报依赖回 EINVAL",
        matches!(extra_err, Err(e) if e.errno == EINVAL && e.point == HangPoint::TaskStruct)
            && cow_decl_gaps(&bad_extra) == 1,
        "全声明成 COW 等于没有声明，声明必须精确到行",
    );

    // 挂点次序：前缀页数严格递增，总页 16。
    let mut order_ok = true;
    let mut prev = 0u64;
    let mut oi = 0usize;
    while oi <= HOOK_ROWS {
        let cur = prefix_pages(oi);
        if cur < prev {
            order_ok = false;
        }
        prev = cur;
        oi += 1;
    }
    order_ok = order_ok && prefix_pages(HOOK_ROWS) == 16 && prefix_bytes(HOOK_ROWS) == 32 * 1024;
    s.add(
        "冻结次序：前缀页数单调、总计 16 页 / 32 KiB",
        order_ok,
        "十步次序是注入坐标，任何重排都会让「第 k 步失败」失去意义",
    );

    // 资源面边界：整表不出现 EAGAIN 与 ENOSYS。
    let mut no_eagain = true;
    let mut gi = 0usize;
    while gi < ENOMEM_GRID.len() {
        if let Some(cell) = ENOMEM_GRID.get(gi).copied() {
            let (mut b, mut v) = cell_setup(&cell);
            if let Err(e) = fork_clone(&mut b, &mut v, 0) {
                if !mem_errno_allowed(e.errno) || e.errno == EAGAIN {
                    no_eagain = false;
                }
            }
        }
        gi += 1;
    }
    s.add(
        "边界：整表扫描不出现 EAGAIN",
        no_eagain,
        "额度面与资源面必须分码，ENOMEM 表里混进 EAGAIN 会让重试策略全错",
    );

    // errno 数值。
    let errno_ok = errno_value("ENOMEM") == 12
        && errno_value("EAGAIN") == 11
        && errno_value("EINVAL") == 22
        && mem_errno_allowed(ENOMEM)
        && mem_errno_allowed(EINVAL)
        && !mem_errno_allowed(EAGAIN);
    s.add(
        "errno：ENOMEM=12 / EINVAL=22，EAGAIN 禁入",
        errno_ok,
        "三码分属三个面，数值与 Linux 一致",
    );

    // 页预算守恒。
    let mut cons = PageBudget::new(256, WATERMARK);
    let charged_ok = cons.charge(4).is_ok();
    cons.refund(4);
    let cons_ok = charged_ok && cons.conserved() && cons.live_pages() == 0 && cons.free() == 256;
    s.add(
        "预算守恒：扣 4 还 4 后在途归零",
        cons_ok,
        "live_pages == 初值 - free 是贯穿全部路径的不变量",
    );

    // 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：规范语料指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等",
    );

    // 诊断码唯一性与文案。
    let mut uniq = true;
    let mut i = 0usize;
    while i < MemCode::ALL.len() {
        let mut j = i + 1;
        while j < MemCode::ALL.len() {
            if let (Some(a), Some(b)) = (MemCode::ALL.get(i), MemCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = MemCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add(
        "诊断码：12 个码值唯一且文案非空",
        uniq && text_ok && MemCode::ALL.len() == 12,
        "闭集码不得重复，每码必须有可操作建议",
    );

    // 账目单调。
    let mut mono = ledger;
    let base = mono.injected;
    let mut m = 0u32;
    while m < 3 {
        mono.record(true, 0, 0);
        m += 1;
    }
    s.add(
        "账目单调：追加 3 笔只增不减",
        mono.injected == base.saturating_add(3)
            && mono.escaped == 0
            && mono.conserved()
            && mono.balanced(),
        "累计计数禁止回退，扣/还页两本账保持平衡",
    );

    // 动态快照。
    let matrix_text = format!(
        "ENOMEM 矩阵 {} / 检出 {} / 逃逸 {} / 扣还 {}:{}",
        ledger.injected, ledger.detected, ledger.escaped, ledger.pages_charged, ledger.pages_refunded
    );
    s.add_dyn("矩阵账目", ledger.conserved() && ledger.balanced(), &matrix_text);

    let decl_text = format!("COW 声明缺口 {} / mm 行 {}", gaps, mm_rows);
    s.add_dyn("声明快照", gaps == 0, &decl_text);

    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watermark_boundary() {
        let mut b = PageBudget::new(WATERMARK, WATERMARK);
        assert_eq!(b.charge(1), Err(ENOMEM));
        assert_eq!(b.free(), WATERMARK);
        let mut b2 = PageBudget::new(WATERMARK + 1, WATERMARK);
        assert_eq!(b2.charge(1), Ok(WATERMARK));
        assert_eq!(b2.live_pages(), 1);
    }

    #[test]
    fn charge_never_underflows() {
        let mut b = PageBudget::new(16, 0);
        assert_eq!(b.charge(u64::MAX), Err(ENOMEM));
        assert_eq!(b.free(), 16);
        assert_eq!(b.conserved(), true);
    }

    #[test]
    fn success_charges_all_hooks() {
        let mut b = PageBudget::new(1024, WATERMARK);
        let mut v = VaSpace::new(VA_LIMIT, 0);
        match fork_clone(&mut b, &mut v, 0) {
            Ok(t) => {
                assert_eq!(t.pages, 16);
                assert_eq!(t.va_end, 32 * 1024);
                assert_eq!(t.cow_hooks, 6);
                assert_eq!(t.points, 10);
            }
            Err(_) => assert_eq!(true, false),
        }
        assert_eq!(b.live_pages(), 16);
        assert_eq!(v.used(), 32 * 1024);
    }

    #[test]
    fn clone_vm_skips_mm() {
        let mut b = PageBudget::new(1024, WATERMARK);
        let mut v = VaSpace::new(VA_LIMIT, 0);
        match fork_clone(&mut b, &mut v, CLONE_VM) {
            Ok(t) => {
                assert_eq!(t.pages, 8);
                assert_eq!(t.cow_hooks, 0);
                assert_eq!(t.points, 4);
                assert_eq!(t.va_end, 0);
            }
            Err(_) => assert_eq!(true, false),
        }
    }

    #[test]
    fn each_hook_can_be_the_first_failure() {
        let mut k = 0usize;
        while k < HOOK_ROWS {
            let cell = EnomemCell { fail_at: k as u8 };
            assert_eq!(cell_matches(&cell), true);
            k += 1;
        }
    }

    #[test]
    fn va_limit_and_overflow() {
        let cell_limit = EnomemCell { fail_at: 10 };
        let cell_overflow = EnomemCell { fail_at: 11 };
        assert_eq!(cell_matches(&cell_limit), true);
        assert_eq!(cell_matches(&cell_overflow), true);
    }

    #[test]
    fn declaration_gaps_are_detected() {
        assert_eq!(cow_decl_gaps(&HOOKS), 0);
        assert_eq!(cow_decl_complete(&HOOKS), true);
        let mut bad: Vec<Hook> = Vec::new();
        let mut i = 0usize;
        while i < HOOKS.len() {
            if let Some(h) = HOOKS.get(i) {
                let mut c = *h;
                if c.point == HangPoint::AnonVma {
                    c.dep = CowDep::None;
                }
                bad.push(c);
            }
            i += 1;
        }
        assert_eq!(cow_decl_gaps(&bad), 1);
    }

    #[test]
    fn failure_refunds_everything() {
        let mut b = PageBudget::new(1024, WATERMARK);
        let mut v = VaSpace::new(16 * 1024, 0);
        let r = fork_clone(&mut b, &mut v, 0);
        assert_eq!(matches!(r, Err(e) if e.errno == ENOMEM), true);
        assert_eq!(b.free(), 1024);
        assert_eq!(b.live_pages(), 0);
        assert_eq!(v.used(), 0);
    }

    #[test]
    fn prefix_is_monotone_and_total() {
        let mut prev = 0u64;
        let mut k = 0usize;
        while k <= HOOK_ROWS {
            let cur = prefix_pages(k);
            assert_eq!(cur >= prev, true);
            prev = cur;
            k += 1;
        }
        assert_eq!(prefix_pages(HOOK_ROWS), 16);
        assert_eq!(prefix_bytes(HOOK_ROWS), 32 * 1024);
    }

    #[test]
    fn matrix_conserved() {
        let (per, l) = run_enomem_matrix();
        assert_eq!(l.conserved(), true);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 120);
        assert_eq!(l.balanced(), true);
        let mut i = 0usize;
        while i < per.len() {
            match per.get(i) {
                Some(n) => assert_eq!(*n, INJECT_ROUNDS),
                None => assert_eq!(true, false),
            }
            i += 1;
        }
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9285_checks();
        assert_eq!(s.all_passed(), true);
        assert_eq!(s.dyn_dropped(), 0);
    }
}
