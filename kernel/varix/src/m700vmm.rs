//! m700vmm — VARIX-M700 AI-02 虚拟内存域 (F026~F050)
//!
//! 地址空间账本/缺页分类官/大页阶梯/页表走路车/写时复制稽核/映射原子律/
//! 交换槽账房/驻留页水位/反向映射谱/页迁移走廊/内存压力谱线/守护映射区/
//! 栈自动扩容/共享映射仲裁/页错误风暴阀/地址空间隔离舱/TLB 击落协议/
//! 零页共享池/映射血缘图/内存对账器/访问位考古/巨页透明化/页表自愈/
//! 内存沙漏仪表/内存域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。
//! 类型与常量统一加 `Vmm` 前缀，避免与 crate 内 vmm/mem 既有符号撞名。

use crate::checks::CheckSet;

// ===========================================================================
// F026 — 地址空间账本：mapped + free = total 恒等式
// ===========================================================================

pub const VMM_PAGE_SIZE: u64 = 4096;
/// 账本样本上限，超出拒绝入账。
pub const VMM_LEDGER_CAP: u32 = 4096;

/// 地址空间账本：total = mapped + free 恒等式由 `consistent()` 守护。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmmAddrLedger {
    pub total: u32,
    pub mapped: u32,
    pub free: u32,
}

impl VmmAddrLedger {
    pub const fn new() -> VmmAddrLedger {
        VmmAddrLedger { total: 0, mapped: 0, free: 0 }
    }

    /// 入账一页映射。账满拒绝并返回 false。
    pub fn add_mapped(&mut self) -> bool {
        if self.total >= VMM_LEDGER_CAP {
            return false;
        }
        self.total += 1;
        self.mapped += 1;
        true
    }

    /// 入账一页空闲。
    pub fn add_free(&mut self) -> bool {
        if self.total >= VMM_LEDGER_CAP {
            return false;
        }
        self.total += 1;
        self.free += 1;
        true
    }

    /// 注销 n 页映射（空闲页数不变，总数随之收缩）。
    pub fn unmap(&mut self, n: u32) -> bool {
        if n > self.mapped {
            return false;
        }
        self.mapped -= n;
        self.total -= n;
        true
    }

    pub fn consistent(&self) -> bool {
        self.mapped + self.free == self.total
    }

    /// 驻留比 permille。
    pub fn mapped_permille(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            self.mapped * 1000 / self.total
        }
    }
}

// ===========================================================================
// F027 — 缺页分类官：三分缺页，识别 COW 候选
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmmFaultKind {
    /// 页不在场，需要分配。
    NotPresent,
    /// 页在场但权限不足（典型：写只读页）。
    Protection,
    /// 页在场且权限足够，属于伪缺页。
    Spurious,
}

pub fn classify_fault(present: bool, write: bool, writable: bool) -> VmmFaultKind {
    if !present {
        VmmFaultKind::NotPresent
    } else if write && !writable {
        VmmFaultKind::Protection
    } else {
        VmmFaultKind::Spurious
    }
}

/// COW 候选：在场 + 写 + 只读映射。
pub fn cow_candidate(present: bool, write: bool, writable: bool) -> bool {
    present && write && !writable
}

// ===========================================================================
// F028 — 大页阶梯：区域够大且对齐才升级页粒度
// ===========================================================================

pub const VMM_HUGE_2M: u64 = 1 << 21;
pub const VMM_HUGE_1G: u64 = 1 << 30;

pub fn huge_step_bytes(len: u64, align_1g: bool, align_2m: bool) -> u64 {
    if len >= VMM_HUGE_1G && align_1g {
        VMM_HUGE_1G
    } else if len >= VMM_HUGE_2M && align_2m {
        VMM_HUGE_2M
    } else {
        VMM_PAGE_SIZE
    }
}

// ===========================================================================
// F029 — 页表走路车：四级索引拆解 + 叶子物理地址拼装
// ===========================================================================

/// x86-64 四级页表索引：PML4/PDPT/PD/PT。
pub fn walk_indices(vaddr: u64) -> [usize; 4] {
    [
        ((vaddr >> 39) as usize) & 0x1FF,
        ((vaddr >> 30) as usize) & 0x1FF,
        ((vaddr >> 21) as usize) & 0x1FF,
        ((vaddr >> 12) as usize) & 0x1FF,
    ]
}

/// 叶子物理地址 = 物理帧基址（低 12 位清零）| 虚拟地址页内偏移。
pub fn leaf_phys(vaddr: u64, frame: u64) -> u64 {
    (frame & !0xFFF) | (vaddr & 0xFFF)
}

// ===========================================================================
// F030 — 写时复制稽核：共享引用 ≥ 2 才拆，独占直接写
// ===========================================================================

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmmCowAudit {
    pub breaks: u32,
    pub refuses: u32,
}

/// 对引用数为 refs 的帧发起写：≥ 2 必须复制（记账 breaks），独占放行（记账 refuses）。
pub fn attempt_cow_break(refs: u32, audit: &mut VmmCowAudit) -> bool {
    if refs >= 2 {
        audit.breaks += 1;
        true
    } else {
        audit.refuses += 1;
        false
    }
}

// ===========================================================================
// F031 — 映射原子律：新映射区间不得与既有区间半点交叠
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmmRange {
    pub start: u64,
    pub len: u64,
}

impl VmmRange {
    pub const fn end(&self) -> u64 {
        self.start.saturating_add(self.len)
    }
}

pub fn ranges_overlap(a: VmmRange, b: VmmRange) -> bool {
    a.len > 0 && b.len > 0 && a.start < b.end() && b.start < a.end()
}

/// 原子映射：区间非空且与所有既有区间无交叠才放行。
pub fn atomic_map_ok(existing: &[VmmRange], req: VmmRange) -> bool {
    if req.len == 0 {
        return false;
    }
    for &r in existing {
        if ranges_overlap(r, req) {
            return false;
        }
    }
    true
}

// ===========================================================================
// F032 — 交换槽账房：16 槽位分配/释放，双释放必须拒绝
// ===========================================================================

pub const VMM_SWAP_SLOTS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct VmmSwapSlots {
    used: [bool; VMM_SWAP_SLOTS],
    pub count: u32,
}

impl VmmSwapSlots {
    pub const fn new() -> VmmSwapSlots {
        VmmSwapSlots { used: [false; VMM_SWAP_SLOTS], count: 0 }
    }

    /// 分配最低空闲槽位；满则 None。
    pub fn alloc(&mut self) -> Option<usize> {
        let mut i = 0usize;
        while i < VMM_SWAP_SLOTS {
            if !self.used[i] {
                self.used[i] = true;
                self.count += 1;
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 释放槽位；重复释放/越界返回 false。
    pub fn release(&mut self, slot: usize) -> bool {
        if slot < VMM_SWAP_SLOTS && self.used[slot] {
            self.used[slot] = false;
            self.count -= 1;
            true
        } else {
            false
        }
    }

    pub fn is_used(&self, slot: usize) -> bool {
        slot < VMM_SWAP_SLOTS && self.used[slot]
    }
}

// ===========================================================================
// F033 — 驻留页水位：三态水位 + 回收目标页数
// ===========================================================================

pub const VMM_WM_HIGH_PERMILLE: u32 = 600;
pub const VMM_WM_OOM_PERMILLE: u32 = 900;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmmResidentState {
    Normal,
    Reclaim,
    Oom,
}

pub fn resident_state(used_permille: u32) -> VmmResidentState {
    if used_permille >= VMM_WM_OOM_PERMILLE {
        VmmResidentState::Oom
    } else if used_permille >= VMM_WM_HIGH_PERMILLE {
        VmmResidentState::Reclaim
    } else {
        VmmResidentState::Normal
    }
}

/// 回收到高水位所需的页数：resident 超出 total*high‰ 的部分。
pub fn reclaim_target_pages(resident: u32, total: u32, high_permille: u32) -> u32 {
    let high = total * high_permille / 1000;
    if resident > high {
        resident - high
    } else {
        0
    }
}

// ===========================================================================
// F034 — 反向映射谱：一页多归属，去重 + 定容
// ===========================================================================

pub const VMM_RMAP_CAP: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct VmmRmap {
    owners: [u32; VMM_RMAP_CAP],
    n: usize,
}

impl VmmRmap {
    pub const fn new() -> VmmRmap {
        VmmRmap { owners: [0; VMM_RMAP_CAP], n: 0 }
    }

    /// 登记归属：重复进程直接拒绝，容量满也拒绝。
    pub fn add_owner(&mut self, pid: u32) -> bool {
        if self.n >= VMM_RMAP_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            if self.owners[i] == pid {
                return false;
            }
            i += 1;
        }
        self.owners[self.n] = pid;
        self.n += 1;
        true
    }

    /// 撤销归属（swap-remove）；不存在返回 false。
    pub fn remove_owner(&mut self, pid: u32) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.owners[i] == pid {
                self.n -= 1;
                self.owners[i] = self.owners[self.n];
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn shares(&self) -> u32 {
        self.n as u32
    }
}

// ===========================================================================
// F035 — 页迁移走廊：有钉住引用禁迁，目标必须页对齐
// ===========================================================================

pub fn migrate_allowed(pinned_refs: u32, mappings: u32) -> bool {
    pinned_refs == 0 && mappings >= 1
}

pub fn migrate_target_ok(dst: u64, page_bytes: u64) -> bool {
    page_bytes > 0 && dst % page_bytes == 0
}

// ===========================================================================
// F036 — 内存压力谱线：可用度四档 + 对应 stall
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmmPressure {
    Low,
    Medium,
    High,
    Critical,
}

pub fn pressure_class(avail_permille: u32) -> VmmPressure {
    if avail_permille >= 700 {
        VmmPressure::Low
    } else if avail_permille >= 400 {
        VmmPressure::Medium
    } else if avail_permille >= 150 {
        VmmPressure::High
    } else {
        VmmPressure::Critical
    }
}

pub fn pressure_stall_us(p: VmmPressure) -> u32 {
    match p {
        VmmPressure::Low => 0,
        VmmPressure::Medium => 100,
        VmmPressure::High => 1000,
        VmmPressure::Critical => 10000,
    }
}

// ===========================================================================
// F037 — 守护映射区：相邻映射必须留出守护间隙
// ===========================================================================

pub const VMM_GUARD_GAP_BYTES: u64 = 8 * VMM_PAGE_SIZE;

pub fn guard_gap_ok(map_end: u64, next_start: u64) -> bool {
    next_start >= map_end.saturating_add(VMM_GUARD_GAP_BYTES)
}

// ===========================================================================
// F038 — 栈自动扩容：触页只允许贴近栈顶下方，且有扩容上限
// ===========================================================================

pub const VMM_STACK_GROW_PROXIMITY: u64 = 2 * VMM_PAGE_SIZE;

#[derive(Clone, Copy, Debug)]
pub struct VmmStackGrow {
    /// 当前栈顶（最低已映射地址，向下生长）。
    pub top: u64,
    pub grown: u32,
    pub max_grow: u32,
}

impl VmmStackGrow {
    pub const fn new(top: u64, max_grow: u32) -> VmmStackGrow {
        VmmStackGrow { top, grown: 0, max_grow }
    }

    /// 栈顶下方近距离缺页触发扩容；太远/超限/方向不对均拒绝。
    pub fn fault_grow(&mut self, fault_addr: u64) -> bool {
        let below = self.top.saturating_sub(fault_addr);
        if fault_addr < self.top
            && below <= VMM_STACK_GROW_PROXIMITY
            && self.grown < self.max_grow
        {
            self.top = fault_addr;
            self.grown += 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F039 — 共享映射仲裁：MAP_SHARED 写要求双边可写；fork 对端定容
// ===========================================================================

pub const VMM_SHARED_PEER_CAP: u32 = 8;

pub fn shared_write_ok(shared: bool, a_writable: bool, b_writable: bool) -> bool {
    !shared || (a_writable && b_writable)
}

#[derive(Clone, Copy, Debug)]
pub struct VmmSharedPeers {
    pub peers: u32,
}

impl VmmSharedPeers {
    pub const fn new() -> VmmSharedPeers {
        VmmSharedPeers { peers: 0 }
    }

    /// fork 增加一个共享对端；满 8 拒绝。
    pub fn fork_peer(&mut self) -> bool {
        if self.peers >= VMM_SHARED_PEER_CAP {
            return false;
        }
        self.peers += 1;
        true
    }
}

// ===========================================================================
// F040 — 页错误风暴阀：缺页速率超限线性限速
// ===========================================================================

pub const VMM_FAULT_STORM_LOW: u32 = 100;
pub const VMM_FAULT_STORM_HIGH: u32 = 1000;
pub const VMM_FAULT_STORM_MAX_DELAY_US: u32 = 5000;

/// ≤100/s 放行；≥1000/s 顶格；中间线性爬升（550/s → 2500us）。
pub fn fault_storm_delay_us(faults_per_sec: u32) -> u32 {
    if faults_per_sec <= VMM_FAULT_STORM_LOW {
        0
    } else if faults_per_sec >= VMM_FAULT_STORM_HIGH {
        VMM_FAULT_STORM_MAX_DELAY_US
    } else {
        (faults_per_sec - VMM_FAULT_STORM_LOW) * VMM_FAULT_STORM_MAX_DELAY_US
            / (VMM_FAULT_STORM_HIGH - VMM_FAULT_STORM_LOW)
    }
}

// ===========================================================================
// F041 — 地址空间隔离舱：user/kernel 半空间硬分界
// ===========================================================================

pub const VMM_USER_TOP: u64 = 0x0000_8000_0000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmmAddrDomain {
    User,
    Kernel,
}

pub fn addr_domain(a: u64) -> VmmAddrDomain {
    if a < VMM_USER_TOP {
        VmmAddrDomain::User
    } else {
        VmmAddrDomain::Kernel
    }
}

/// 隔离检查：user 地址必须落在用户半空间，kernel 地址必须落在内核半空间。
pub fn isolation_ok(user_addr: u64, kernel_addr: u64) -> bool {
    addr_domain(user_addr) == VmmAddrDomain::User
        && addr_domain(kernel_addr) == VmmAddrDomain::Kernel
}

// ===========================================================================
// F042 — TLB 击落协议：全部目标核 ACK 才算击落完成
// ===========================================================================

pub const VMM_SHOOTDOWN_MAX_CPUS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct VmmShootdown {
    target_count: usize,
    acked: [bool; VMM_SHOOTDOWN_MAX_CPUS],
}

impl VmmShootdown {
    pub const fn new() -> VmmShootdown {
        VmmShootdown { target_count: 0, acked: [false; VMM_SHOOTDOWN_MAX_CPUS] }
    }

    /// 布防：目标核数 1~8。
    pub fn arm(&mut self, cpus: usize) -> bool {
        if cpus >= 1 && cpus <= VMM_SHOOTDOWN_MAX_CPUS {
            self.target_count = cpus;
            self.acked = [false; VMM_SHOOTDOWN_MAX_CPUS];
            true
        } else {
            false
        }
    }

    /// 目标核回 ACK；重复 ACK/越界拒绝。
    pub fn ack(&mut self, cpu: usize) -> bool {
        if cpu < self.target_count && !self.acked[cpu] {
            self.acked[cpu] = true;
            true
        } else {
            false
        }
    }

    /// 全部目标核已 ACK。
    pub fn complete(&self) -> bool {
        let mut i = 0usize;
        while i < self.target_count {
            if !self.acked[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn ipi_needed(&self) -> u32 {
        self.target_count as u32
    }
}

// ===========================================================================
// F043 — 零页共享池：读共享、写拆私有，池容量封顶
// ===========================================================================

pub const VMM_ZERO_POOL_CAP: u32 = 16;

#[derive(Clone, Copy, Debug)]
pub struct VmmZeroPool {
    pub refs: u32,
}

impl VmmZeroPool {
    pub const fn new() -> VmmZeroPool {
        VmmZeroPool { refs: 0 }
    }

    /// 挂一页共享零映射；池满拒绝。
    pub fn map_zero(&mut self) -> bool {
        if self.refs >= VMM_ZERO_POOL_CAP {
            return false;
        }
        self.refs += 1;
        true
    }

    /// 写命中：共享引用 ≥ 2 拆一份私有副本；最后一个引用原地写。
    pub fn write_fault(&mut self) -> bool {
        if self.refs >= 2 {
            self.refs -= 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F044 — 映射血缘图：fork 子嗣登记，去重 + 定容 + 深度上限
// ===========================================================================

pub const VMM_LINEAGE_MAX_DEPTH: u32 = 8;
pub const VMM_LINEAGE_CHILD_CAP: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct VmmLineage {
    children: [u32; VMM_LINEAGE_CHILD_CAP],
    n: usize,
}

impl VmmLineage {
    pub const fn new() -> VmmLineage {
        VmmLineage { children: [0; VMM_LINEAGE_CHILD_CAP], n: 0 }
    }

    /// 登记子嗣：重复/满容拒绝。
    pub fn add_child(&mut self, pid: u32) -> bool {
        if self.n >= VMM_LINEAGE_CHILD_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            if self.children[i] == pid {
                return false;
            }
            i += 1;
        }
        self.children[self.n] = pid;
        self.n += 1;
        true
    }

    pub fn children(&self) -> u32 {
        self.n as u32
    }
}

pub fn lineage_depth_ok(depth: u32) -> bool {
    depth <= VMM_LINEAGE_MAX_DEPTH
}

// ===========================================================================
// F045 — 内存对账器：resident + swapped + free = total
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmmReconciler {
    pub total: u32,
    pub resident: u32,
    pub swapped: u32,
    pub free: u32,
}

impl VmmReconciler {
    pub const fn new(total: u32, resident: u32, swapped: u32, free: u32) -> VmmReconciler {
        VmmReconciler { total, resident, swapped, free }
    }

    pub fn consistent(&self) -> bool {
        self.resident + self.swapped + self.free == self.total
    }

    /// 换出 n 页；超额拒绝（不许透支）。
    pub fn swap_out(&mut self, n: u32) -> bool {
        if n > self.resident {
            return false;
        }
        self.resident -= n;
        self.swapped += n;
        true
    }
}

// ===========================================================================
// F046 — 访问位考古：按闲置轮数分温层，冷/冰层可逐出
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmmAgeClass {
    Hot,
    Warm,
    Cold,
    Ice,
}

pub fn age_class(idle_rounds: u32) -> VmmAgeClass {
    if idle_rounds == 0 {
        VmmAgeClass::Hot
    } else if idle_rounds <= 4 {
        VmmAgeClass::Warm
    } else if idle_rounds <= 16 {
        VmmAgeClass::Cold
    } else {
        VmmAgeClass::Ice
    }
}

pub fn evict_worthy(c: VmmAgeClass) -> bool {
    matches!(c, VmmAgeClass::Cold | VmmAgeClass::Ice)
}

// ===========================================================================
// F047 — 巨页透明化：512 页全覆盖 + 2M 对齐才晋升
// ===========================================================================

pub const VMM_THP_PAGES_PER_HUGE: u32 = 512;

pub fn thp_promotable(covered_pages: u32, align_2m: bool) -> bool {
    covered_pages == VMM_THP_PAGES_PER_HUGE && align_2m
}

/// 巨页模式下所需的页表项数（512 普通 PTE 合 1 个巨 PTE）。
pub fn pte_entries_for(pages: u32, huge: bool) -> u32 {
    if huge {
        (pages + VMM_THP_PAGES_PER_HUGE - 1) / VMM_THP_PAGES_PER_HUGE
    } else {
        pages
    }
}

// ===========================================================================
// F048 — 页表自愈：非规范地址按 bit47 符号扩展矫正
// ===========================================================================

pub fn canonicalize(a: u64) -> u64 {
    let low = a & 0x0000_FFFF_FFFF_FFFF;
    if low & (1u64 << 47) != 0 {
        low | 0xFFFF_0000_0000_0000
    } else {
        low
    }
}

pub fn is_canonical(a: u64) -> bool {
    a == canonicalize(a)
}

// ===========================================================================
// F049 — 内存沙漏仪表：余量分档 + 燃尽刻度
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmmGauge {
    Full,
    Half,
    Low,
    Empty,
}

pub fn gauge_class(remaining_permille: u32) -> VmmGauge {
    if remaining_permille >= 700 {
        VmmGauge::Full
    } else if remaining_permille >= 300 {
        VmmGauge::Half
    } else if remaining_permille >= 100 {
        VmmGauge::Low
    } else {
        VmmGauge::Empty
    }
}

/// 剩余量按每 tick 消耗还能撑几 tick；零消耗视为无限。
pub fn ticks_to_empty(remaining: u32, burn_per_tick: u32) -> u32 {
    if burn_per_tick == 0 {
        u32::MAX
    } else {
        remaining / burn_per_tick
    }
}

// ===========================================================================
// F050 — 内存域年报：年报章节完备性
// ===========================================================================

pub const VMM_REPORT_SECTIONS: [&str; 5] =
    ["ledger", "faults", "watermark", "rmap", "reconcile"];

pub fn vmm_report_complete(sections_filled: u32) -> bool {
    sections_filled >= VMM_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700vmm_checks() -> CheckSet {
    let mut set = CheckSet::new("m700vmm");

    // F026 地址空间账本
    let mut ledger = VmmAddrLedger::new();
    let mut i = 0;
    while i < 5 {
        ledger.add_mapped();
        i += 1;
    }
    let mut i = 0;
    while i < 3 {
        ledger.add_free();
        i += 1;
    }
    let permille_before = ledger.mapped_permille();
    ledger.unmap(1);
    let permille_after = ledger.mapped_permille();
    set.add(
        "F026 ledger identity",
        ledger.consistent() && ledger.total == 7 && ledger.mapped == 4 && ledger.free == 3,
        "mapped+free=total",
    );
    set.add(
        "F026 ledger permille",
        permille_before == 625 && permille_after == 571,
        "ratio tracked across unmap",
    );
    set.add("F026 ledger over-unmap", !ledger.unmap(9), "no overdraft");
    let mut full = VmmAddrLedger::new();
    let mut added = 0u32;
    while full.add_mapped() {
        added += 1;
    }
    set.add("F026 ledger capacity", added == VMM_LEDGER_CAP && full.total == 4096, "cap 4096");

    // F027 缺页分类官
    set.add(
        "F027 fault classes",
        classify_fault(false, true, false) == VmmFaultKind::NotPresent
            && classify_fault(true, true, false) == VmmFaultKind::Protection
            && classify_fault(true, false, false) == VmmFaultKind::Spurious
            && classify_fault(true, true, true) == VmmFaultKind::Spurious,
        "three-way split",
    );
    set.add(
        "F027 cow candidate",
        cow_candidate(true, true, false) && !cow_candidate(true, true, true)
            && !cow_candidate(false, true, false),
        "present+write+readonly",
    );

    // F028 大页阶梯
    set.add(
        "F028 ladder 1g",
        huge_step_bytes(4 * VMM_HUGE_1G, true, true) == VMM_HUGE_1G
            && huge_step_bytes(VMM_HUGE_1G, true, true) == VMM_HUGE_1G,
        "giant region",
    );
    set.add(
        "F028 ladder fallback",
        huge_step_bytes(64 * 1024 * 1024, false, true) == VMM_HUGE_2M
            && huge_step_bytes(64 * 1024 * 1024, false, false) == VMM_PAGE_SIZE
            && huge_step_bytes(8192, false, true) == VMM_PAGE_SIZE,
        "align or size gates",
    );

    // F029 页表走路车
    let walk_v = 0x0000_0080_8060_4000u64;
    set.add(
        "F029 walk indices",
        walk_indices(walk_v) == [1, 2, 3, 4],
        "four-level decode",
    );
    set.add(
        "F029 leaf phys",
        leaf_phys(0x0000_0088_0060_4ABC, 0x0000_0000_1234_5000) == 0x0000_0000_1234_5ABC
            && (leaf_phys(walk_v, 0x1234_0000) & 0xFFF) == (walk_v & 0xFFF),
        "frame base + offset",
    );

    // F030 写时复制稽核
    let mut cow = VmmCowAudit::default();
    let broke = attempt_cow_break(3, &mut cow);
    let breaks_after_shared = cow.breaks;
    let refused = attempt_cow_break(1, &mut cow);
    set.add("F030 cow break shared", broke && breaks_after_shared == 1, "copy taken");
    set.add(
        "F030 cow refuse exclusive",
        !refused && cow.breaks == 1 && cow.refuses == 1,
        "exclusive write in place",
    );

    // F031 映射原子律
    let existing = [VmmRange { start: 0x0000, len: 0x1000 }];
    set.add(
        "F031 atomic map adjacent",
        atomic_map_ok(&existing, VmmRange { start: 0x1000, len: 0x1000 }),
        "adjacent allowed",
    );
    set.add(
        "F031 atomic map rejects",
        !atomic_map_ok(&existing, VmmRange { start: 0x0800, len: 0x1000 })
            && !atomic_map_ok(&existing, VmmRange { start: 0x2000, len: 0 }),
        "overlap and empty rejected",
    );

    // F032 交换槽账房
    let mut slots = VmmSwapSlots::new();
    let mut taken = 0usize;
    while slots.alloc().is_some() {
        taken += 1;
    }
    set.add("F032 swap full", taken == VMM_SWAP_SLOTS && slots.count == 16, "16 slots");
    let rel1 = slots.release(3);
    let count_after_release = slots.count;
    let rel2 = slots.release(3);
    set.add(
        "F032 swap release",
        rel1 && count_after_release == 15 && !rel2,
        "double free rejected",
    );
    let refill = slots.alloc();
    set.add("F032 swap refill", refill == Some(3) && slots.count == 16, "lowest slot reused");

    // F033 驻留页水位
    set.add(
        "F033 watermark tiers",
        resident_state(500) == VmmResidentState::Normal
            && resident_state(600) == VmmResidentState::Reclaim
            && resident_state(950) == VmmResidentState::Oom,
        "three tiers",
    );
    set.add(
        "F033 reclaim target",
        reclaim_target_pages(900, 1000, VMM_WM_HIGH_PERMILLE) == 300
            && reclaim_target_pages(500, 1000, VMM_WM_HIGH_PERMILLE) == 0,
        "resident minus high water",
    );

    // F034 反向映射谱
    let mut rmap = VmmRmap::new();
    let a1 = rmap.add_owner(7);
    let dup = rmap.add_owner(7);
    set.add("F034 rmap dedup", a1 && !dup && rmap.shares() == 1, "same pid once");
    let a2 = rmap.add_owner(8);
    let a3 = rmap.add_owner(9);
    let a4 = rmap.add_owner(10);
    let cap = rmap.add_owner(11);
    set.add(
        "F034 rmap capacity",
        a2 && a3 && a4 && !cap && rmap.shares() == 4,
        "cap 4",
    );
    let rm1 = rmap.remove_owner(8);
    let shares_after = rmap.shares();
    let rm2 = rmap.remove_owner(8);
    set.add(
        "F034 rmap remove",
        rm1 && shares_after == 3 && !rm2,
        "swap-remove, idempotent reject",
    );

    // F035 页迁移走廊
    set.add(
        "F035 migrate gate",
        migrate_allowed(0, 1) && !migrate_allowed(2, 1) && !migrate_allowed(0, 0),
        "pinned and empty blocked",
    );
    set.add(
        "F035 migrate align",
        migrate_target_ok(0x20_0000, VMM_HUGE_2M)
            && !migrate_target_ok(0x20_0001, VMM_HUGE_2M)
            && migrate_target_ok(0x1000, VMM_PAGE_SIZE),
        "page aligned only",
    );

    // F036 内存压力谱线
    set.add(
        "F036 pressure tiers",
        pressure_class(700) == VmmPressure::Low
            && pressure_class(699) == VmmPressure::Medium
            && pressure_class(400) == VmmPressure::Medium
            && pressure_class(399) == VmmPressure::High
            && pressure_class(150) == VmmPressure::High
            && pressure_class(149) == VmmPressure::Critical,
        "boundary exact",
    );
    set.add(
        "F036 pressure stalls",
        pressure_stall_us(VmmPressure::Low) == 0
            && pressure_stall_us(VmmPressure::Medium) == 100
            && pressure_stall_us(VmmPressure::High) == 1000
            && pressure_stall_us(VmmPressure::Critical) == 10000,
        "escalating stall",
    );

    // F037 守护映射区
    set.add(
        "F037 guard exact gap",
        guard_gap_ok(0x10_0000, 0x10_0000 + VMM_GUARD_GAP_BYTES),
        "gap equals requirement",
    );
    set.add(
        "F037 guard too close",
        !guard_gap_ok(0x10_0000, 0x10_8000 - 1) && !guard_gap_ok(0x10_0000, 0x10_0000),
        "short gap rejected",
    );

    // F038 栈自动扩容
    let mut stack = VmmStackGrow::new(0x1_0000, 2);
    let g1 = stack.fault_grow(0xF800);
    let top_after_g1 = stack.top;
    let g_far = stack.fault_grow(0x8000);
    set.add(
        "F038 stack proximity grow",
        g1 && top_after_g1 == 0xF800 && !g_far && stack.top == 0xF800,
        "near grows, far rejects",
    );
    let g2 = stack.fault_grow(0xF000);
    let grown_after_g2 = stack.grown;
    let g3 = stack.fault_grow(0xE800);
    set.add(
        "F038 stack grow cap",
        g2 && grown_after_g2 == 2 && !g3 && stack.grown == 2,
        "max_grow enforced",
    );

    // F039 共享映射仲裁
    set.add(
        "F039 shared write gate",
        !shared_write_ok(true, true, false)
            && shared_write_ok(true, true, true)
            && shared_write_ok(false, false, false),
        "both sides writable for shared",
    );
    let mut peers = VmmSharedPeers::new();
    let mut forked = 0u32;
    while peers.fork_peer() {
        forked += 1;
    }
    set.add(
        "F039 peer cap",
        forked == VMM_SHARED_PEER_CAP && peers.peers == 8,
        "cap 8",
    );

    // F040 页错误风暴阀
    set.add(
        "F040 storm pass",
        fault_storm_delay_us(50) == 0 && fault_storm_delay_us(VMM_FAULT_STORM_LOW) == 0,
        "low rate free",
    );
    set.add(
        "F040 storm curve",
        fault_storm_delay_us(550) == 2500
            && fault_storm_delay_us(VMM_FAULT_STORM_HIGH) == VMM_FAULT_STORM_MAX_DELAY_US
            && fault_storm_delay_us(5000) == VMM_FAULT_STORM_MAX_DELAY_US,
        "linear then clamp",
    );

    // F041 地址空间隔离舱
    set.add(
        "F041 split boundary",
        addr_domain(VMM_USER_TOP - 1) == VmmAddrDomain::User
            && addr_domain(VMM_USER_TOP) == VmmAddrDomain::Kernel,
        "half-space split",
    );
    set.add(
        "F041 isolation check",
        isolation_ok(0x1000, 0xFFFF_8000_0000_0000)
            && !isolation_ok(0xFFFF_8000_0000_0000, 0x1000),
        "crossed halves caught",
    );

    // F042 TLB 击落协议
    let mut sd = VmmShootdown::new();
    let arm1 = sd.arm(3);
    let arm0 = sd.arm(0);
    set.add(
        "F042 shootdown arm",
        arm1 && !arm0 && sd.ipi_needed() == 3,
        "1..=8 cpus",
    );
    let ack0 = sd.ack(0);
    let ack1 = sd.ack(1);
    let complete_mid = sd.complete();
    set.add(
        "F042 shootdown partial",
        ack0 && ack1 && !complete_mid,
        "pending until all ack",
    );
    let dup_ack = sd.ack(0);
    let ack2 = sd.ack(2);
    let complete_end = sd.complete();
    set.add(
        "F042 shootdown complete",
        !dup_ack && ack2 && complete_end,
        "duplicate ack ignored, all ack done",
    );

    // F043 零页共享池
    let mut pool = VmmZeroPool::new();
    let m1 = pool.map_zero();
    let m2 = pool.map_zero();
    let m3 = pool.map_zero();
    set.add("F043 pool share", m1 && m2 && m3 && pool.refs == 3, "three mappings");
    let mut extra = 0u32;
    while pool.map_zero() {
        extra += 1;
    }
    set.add(
        "F043 pool cap",
        extra == 13 && pool.refs == VMM_ZERO_POOL_CAP,
        "cap 16",
    );
    let w1 = pool.write_fault();
    let refs_after_w1 = pool.refs;
    set.add("F043 pool cow", w1 && refs_after_w1 == 15, "copy taken");
    while pool.refs > 1 {
        pool.write_fault();
    }
    let w_last = pool.write_fault();
    set.add(
        "F043 pool last mapping",
        !w_last && pool.refs == 1,
        "exclusive write in place",
    );

    // F044 映射血缘图
    let mut lin = VmmLineage::new();
    let c1 = lin.add_child(1);
    let c2 = lin.add_child(2);
    let cdup = lin.add_child(1);
    let c3 = lin.add_child(3);
    let c4 = lin.add_child(4);
    let c5 = lin.add_child(5);
    set.add(
        "F044 lineage registry",
        c1 && c2 && !cdup && c3 && c4 && !c5 && lin.children() == 4,
        "dedup + cap 4",
    );
    set.add(
        "F044 lineage depth",
        lineage_depth_ok(VMM_LINEAGE_MAX_DEPTH) && !lineage_depth_ok(9),
        "depth <= 8",
    );

    // F045 内存对账器
    let mut rec = VmmReconciler::new(10, 6, 2, 2);
    set.add("F045 reconcile open", rec.consistent() && rec.total == 10, "books balanced");
    let out1 = rec.swap_out(2);
    set.add(
        "F045 reconcile swap",
        out1 && rec.consistent() && rec.resident == 4 && rec.swapped == 4,
        "resident flows to swap",
    );
    let out2 = rec.swap_out(5);
    set.add(
        "F045 reconcile overdraft",
        !out2 && rec.resident == 4 && rec.consistent(),
        "no overdraft",
    );

    // F046 访问位考古
    set.add(
        "F046 age tiers",
        age_class(0) == VmmAgeClass::Hot
            && age_class(4) == VmmAgeClass::Warm
            && age_class(16) == VmmAgeClass::Cold
            && age_class(17) == VmmAgeClass::Ice,
        "boundary exact",
    );
    set.add(
        "F046 age evict",
        !evict_worthy(VmmAgeClass::Hot)
            && !evict_worthy(VmmAgeClass::Warm)
            && evict_worthy(VmmAgeClass::Cold)
            && evict_worthy(VmmAgeClass::Ice),
        "cold and ice evicted",
    );

    // F047 巨页透明化
    set.add(
        "F047 thp gate",
        thp_promotable(512, true) && !thp_promotable(511, true) && !thp_promotable(512, false),
        "full coverage + aligned",
    );
    set.add(
        "F047 thp pte count",
        pte_entries_for(512, true) == 1
            && pte_entries_for(1024, true) == 2
            && pte_entries_for(512, false) == 512,
        "512 ptes collapse to 1",
    );

    // F048 页表自愈
    set.add(
        "F048 canonical detect",
        is_canonical(0x1234) && !is_canonical(0x0000_8000_0000_0000),
        "bit47 sign rule",
    );
    let healed = canonicalize(0x0000_8000_0000_0000);
    set.add(
        "F048 canonical heal",
        healed == 0xFFFF_8000_0000_0000 && is_canonical(healed),
        "sign-extended and stable",
    );

    // F049 内存沙漏仪表
    set.add(
        "F049 gauge tiers",
        gauge_class(700) == VmmGauge::Full
            && gauge_class(699) == VmmGauge::Half
            && gauge_class(300) == VmmGauge::Half
            && gauge_class(299) == VmmGauge::Low
            && gauge_class(100) == VmmGauge::Low
            && gauge_class(99) == VmmGauge::Empty,
        "boundary exact",
    );
    set.add(
        "F049 gauge burn",
        ticks_to_empty(500, 100) == 5
            && ticks_to_empty(500, 0) == u32::MAX
            && ticks_to_empty(99, 100) == 0,
        "burn math incl. zero-burn",
    );

    // F050 内存域年报
    set.add(
        "F050 vmm report",
        VMM_REPORT_SECTIONS.len() == 5 && vmm_report_complete(5) && !vmm_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f026_ledger_math() {
        let mut l = VmmAddrLedger::new();
        let mut i = 0;
        while i < 3 {
            l.add_mapped();
            i += 1;
        }
        l.add_free();
        assert!(l.consistent());
        assert_eq!(l.mapped_permille(), 750);
        assert!(l.unmap(1));
        assert_eq!(l.total, 3);
        assert_eq!(l.mapped_permille(), 666);
        assert!(!l.unmap(9));
    }

    #[test]
    fn f029_walk_decode() {
        assert_eq!(walk_indices(0x0000_0080_8060_4000), [1, 2, 3, 4]);
        assert_eq!(walk_indices(0), [0, 0, 0, 0]);
        assert_eq!(leaf_phys(0x1234_ABCD, 0x9999_9000), 0x9999_9BCD);
    }

    #[test]
    fn f032_swap_slot_lifecycle() {
        let mut s = VmmSwapSlots::new();
        assert_eq!(s.alloc(), Some(0));
        assert_eq!(s.alloc(), Some(1));
        assert!(s.release(0));
        assert!(!s.release(0));
        assert!(!s.release(99));
        assert_eq!(s.alloc(), Some(0));
        assert_eq!(s.count, 2);
    }

    #[test]
    fn f038_stack_growth_cap() {
        let mut st = VmmStackGrow::new(0x1_0000, 1);
        assert!(st.fault_grow(0xF800));
        assert_eq!(st.top, 0xF800);
        assert!(!st.fault_grow(0xF000)); // 扩容额度用尽
        assert!(!st.fault_grow(0x1000)); // 远超邻近窗口
        assert_eq!(st.grown, 1);
    }

    #[test]
    fn f048_canonical_heal() {
        assert!(is_canonical(0));
        assert!(is_canonical(0x0000_7FFF_FFFF_FFFF));
        assert!(!is_canonical(0x0000_8000_0000_0000));
        assert_eq!(canonicalize(0x0000_9000_0000_0000), 0xFFFF_9000_0000_0000);
        assert!(is_canonical(0xFFFF_9000_0000_0000));
    }

    #[test]
    fn m700vmm_selfcheck_all_pass() {
        let set = run_m700vmm_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
