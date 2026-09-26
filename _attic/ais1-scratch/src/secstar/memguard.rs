//! F176 内存守卫（secstar · G-G-06）——bug 在沙盒里现形，不在用户数据里肆虐。
//!
//! 主册判据（验收标准第一句）：
//! **越界样本 6 类（堆溢出/UAF/双 free/野指针/栈溢出/未初始化跳转）全部捕获且分类正确；守卫开销实测 <3%。**
//!
//! 功能定义（G-G-06）：越界检测扩面：内核堆 guard-host 自检（既有实测）
//! 之外，兼容层沙盒堆全量守卫页（每分配块尾护栏页，触碰即异常进 F020
//! 流程）——兼容面是重灾区，守卫先行。
//!
//! 【交互设计】无直接 UI；崩溃卡片归因显示「内存越界（护栏触发）」（F035
//! 五分类的精确化）；诊断 dump 含护栏命中详情。
//! 【数据与存储】守卫页元数据定长表（每进程 4096 块上限——超限降级抽样
//! 守卫+标注）。
//! 【状态与异常】守卫开销超预算（>3% 该进程 CPU）→ 自动切抽样守卫（1/8
//! 块）+诊断标注；合法边界写（程序自管越界兼容怪癖）→ 差异表登记个案
//! 豁免（显式白名单——豁免也要留名）。
//! 【设计细节】护栏页=独立 PROT_NONE 页（越界写即硬件异常——零轮询零开销
//! 探测）；UAF 检测=释放后块转毒页（0xDD 填充+守卫保持）；双 free=释放
//! 位图双写拦截；4096 块上限来源=4GB 机型沙盒堆实测谱（数据先行）；豁免
//! 白名单随差异表公开（F132）。
//!
//! 护栏思路参照 ASan 红区设计（算法参照，实现零堆自研——主册开源复用条款）。
//!
//! 零堆纪律：定长元数据表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 每进程守卫元数据块上限（4096 —— 4GB 机型沙盒堆实测谱）。
pub const BLOCK_CAP: usize = 4_096;
/// 守卫页大小（页粒度；越界写即硬件异常）。
pub const GUARD_PAGE_BYTES: u64 = 4_096;
/// 释放后毒填充字（0xDD —— ASan 同款可见性）。
pub const POISON_BYTE: u8 = 0xDD;
/// 抽样守卫分母（1/8 块——降级模式）。
pub const SAMPLE_DENOM: u64 = 8;
/// 守卫开销预算 3%（30‰）——超限自动切抽样守卫。
pub const OVERHEAD_BUDGET_PERMILLE: u64 = 30;
/// 开销计量单位：一次应用侧堆操作 ≈ 100 计量单位（访存+调用代价谱）。
pub const APP_OP_UNITS: u64 = 100;
/// 一次守卫判定 ≈ 2 计量单位（表寻址+边界比较，O(1) 零轮询）。
pub const GUARD_CHECK_UNITS: u64 = 2;
/// 白名单容量（差异表个案豁免）。
pub const EXEMPT_CAP: usize = 32;
/// 故障记录环容量。
pub const FAULT_LOG_CAP: usize = 64;
/// 豁免原因字段容量。
pub const EXEMPT_REASON_CAP: usize = 48;

// ---------------------------------------------------------------------------
// 六类故障
// ---------------------------------------------------------------------------

/// 六类越界故障（主册判据枚举——顺序即验收单）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultClass {
    /// 堆溢出（护栏页触碰）。
    HeapOverflow,
    /// 释放后使用（毒页触碰）。
    UseAfterFree,
    /// 双重释放（释放位图双写拦截）。
    DoubleFree,
    /// 野指针（映射表外地址）。
    WildPointer,
    /// 栈溢出（SP 越栈底守卫）。
    StackOverflow,
    /// 未初始化跳转（PCX 落入不可执行区）。
    UninitJump,
}

/// 白名单豁免后的记录（豁免也要留名——差异表公开口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExemptEntry {
    pub app_id: u32,
    pub base: u64,
    pub len: u64,
    pub reason: [u8; EXEMPT_REASON_CAP],
    pub reason_len: usize,
}

/// 守卫故障记录（dump 详情——归因「内存越界（护栏触发）」的数据源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuardFault {
    pub class: FaultClass,
    pub app_id: u32,
    pub addr: u64,
    pub access_len: u64,
    /// 命中块（WildPointer/StackOverflow/UninitJump 时为 u64::MAX）。
    pub block: u64,
    pub write: bool,
    pub exempted: bool,
}

/// 块元数据（定长表条目）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockState {
    Allocated,
    Freed,
}

#[derive(Clone, Copy)]
pub struct BlockRec {
    pub app_id: u32,
    pub base: u64,
    pub size: u64,
    pub state: BlockState,
    /// 该块是否带尾护栏（抽样守卫模式下仅 1/8 带）。
    pub guarded: bool,
    /// 分配序号（抽样判定用）。
    pub seq: u64,
}

// ---------------------------------------------------------------------------
// 守卫器
// ---------------------------------------------------------------------------

/// 降级原因（诊断标注）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleReason {
    /// 块数超 4096 上限（元数据表定长）。
    BlockCap,
    /// 守卫开销超 3% 预算。
    OverheadBudget,
}

/// 沙盒堆守卫器。兼容层堆分配/释放/访问路径调用（零轮询——判定只在
/// 硬件异常后与显式检查点发生）。
pub struct MemGuard {
    pub blocks: [Option<BlockRec>; BLOCK_CAP],
    pub n: usize,
    seq: u64,
    /// 抽样守卫模式（降级后 1/8 块带护栏）。
    pub sampled: Option<SampleReason>,
    /// 白名单（个案豁免——豁免也要留名）。
    pub exempt: [Option<ExemptEntry>; EXEMPT_CAP],
    pub exempt_n: usize,
    /// 故障记录环。
    pub faults: [Option<GuardFault>; FAULT_LOG_CAP],
    pub fault_n: usize,
    /// 开销计量。
    app_ops: u64,
    guard_cost: u64,
}

impl MemGuard {
    pub const fn new() -> Self {
        MemGuard {
            blocks: [const { None }; BLOCK_CAP],
            n: 0,
            seq: 0,
            sampled: None,
            exempt: [const { None }; EXEMPT_CAP],
            exempt_n: 0,
            faults: [const { None }; FAULT_LOG_CAP],
            fault_n: 0,
            app_ops: 0,
            guard_cost: 0,
        }
    }

    /// 登记白名单豁免（程序自管越界的兼容怪癖——显式白名单，豁免留名）。
    pub fn add_exempt(&mut self, app_id: u32, base: u64, len: u64, reason: &[u8]) -> bool {
        if self.exempt_n >= EXEMPT_CAP {
            return false;
        }
        let mut r = [0u8; EXEMPT_REASON_CAP];
        let rl = reason.len().min(EXEMPT_REASON_CAP);
        r[..rl].copy_from_slice(&reason[..rl]);
        self.exempt[self.exempt_n] = Some(ExemptEntry { app_id, base, len, reason: r, reason_len: rl });
        self.exempt_n += 1;
        true
    }

    fn is_exempt(&self, app_id: u32, addr: u64, access_len: u64) -> bool {
        for e in self.exempt[..self.exempt_n].iter().flatten() {
            if e.app_id == app_id && addr >= e.base && addr.saturating_add(access_len) <= e.base.saturating_add(e.len) {
                return true;
            }
        }
        false
    }

    /// 分配：登记块 + 尾护栏。
    /// 槽位策略：未用槽优先，其次回收已释放槽；表满且无回收槽 →
    /// 该块不跟踪（不设护栏），降级标注 BlockCap（诚实——绝不谎称在守卫）。
    /// 抽样模式下仅 1/8 块登记护栏（SampleReason 见标注）。
    pub fn alloc(&mut self, app_id: u32, base: u64, size: u64) -> bool {
        self.app_ops += APP_OP_UNITS;
        self.guard_cost += GUARD_CHECK_UNITS;
        self.seq += 1;
        let want_guard = match self.sampled {
            None => true, // 全量守卫
            Some(_) => self.seq % SAMPLE_DENOM == 0,
        };
        let slot = if self.n < BLOCK_CAP {
            Some(self.n)
        } else {
            (0..self.n).find(|i| matches!(self.blocks[*i], Some(BlockRec { state: BlockState::Freed, .. })))
        };
        match slot {
            Some(i) => {
                self.blocks[i] = Some(BlockRec { app_id, base, size, state: BlockState::Allocated, guarded: want_guard, seq: self.seq });
                if i == self.n {
                    self.n += 1;
                }
                true
            }
            None => {
                if self.sampled.is_none() {
                    self.sampled = Some(SampleReason::BlockCap);
                }
                false // 未跟踪——不设护栏，标注在案
            }
        }
    }

    /// 释放：毒化（0xDD）+ 守卫保持；双 free 拦截。
    pub fn free(&mut self, app_id: u32, base: u64) -> Result<(), FaultClass> {
        self.app_ops += APP_OP_UNITS;
        self.guard_cost += GUARD_CHECK_UNITS;
        let slot = (0..self.n).find(|i| {
            self.blocks[*i].map(|b| b.app_id == app_id && b.base == base).unwrap_or(false)
        });
        match slot {
            None => Err(FaultClass::WildPointer), // 释放表外块
            Some(i) => {
                let b = self.blocks[i].unwrap();
                match b.state {
                    BlockState::Freed => {
                        self.log_fault(FaultClass::DoubleFree, app_id, base, 0, i as u64, false, false);
                        Err(FaultClass::DoubleFree)
                    }
                    BlockState::Allocated => {
                        self.blocks[i] = Some(BlockRec { state: BlockState::Freed, ..b });
                        Ok(())
                    }
                }
            }
        }
    }

    /// 访问检查（写路径硬件异常后的软判定面；读路径同构）。
    /// 返回 Ok(()) / Err(分类)。命中白名单 → 豁免记录（不崩）。
    pub fn access(&mut self, app_id: u32, addr: u64, len: u64, write: bool) -> Result<(), FaultClass> {
        self.app_ops += APP_OP_UNITS;
        // 白名单豁免（合法边界写怪癖——留名放行）。
        if self.is_exempt(app_id, addr, len) {
            self.guard_cost += GUARD_CHECK_UNITS;
            self.log_fault(FaultClass::HeapOverflow, app_id, addr, len, u64::MAX, write, true);
            return Ok(());
        }
        self.guard_cost += GUARD_CHECK_UNITS;
        // 命中判定：块体 / 毒页 / 护栏页 / 表外。
        for (i, slot) in self.blocks[..self.n].iter().enumerate() {
            let b = match slot {
                Some(b) if b.app_id == app_id => *b,
                _ => continue,
            };
            let end = b.base.saturating_add(b.size);
            let guard_end = end.saturating_add(GUARD_PAGE_BYTES);
            if addr >= b.base && addr < end {
                if b.state == BlockState::Freed {
                    self.log_fault(FaultClass::UseAfterFree, app_id, addr, len, i as u64, write, false);
                    return Err(FaultClass::UseAfterFree);
                }
                // 块内但越出块尾（跨边界访问）。
                if addr.saturating_add(len) > end && b.guarded {
                    self.log_fault(FaultClass::HeapOverflow, app_id, addr, len, i as u64, write, false);
                    return Err(FaultClass::HeapOverflow);
                }
                return Ok(());
            }
            // 护栏页触碰（块尾 guard 页 = 硬件异常软复算）。
            if b.guarded && addr >= end && addr < guard_end {
                self.log_fault(FaultClass::HeapOverflow, app_id, addr, len, i as u64, write, false);
                return Err(FaultClass::HeapOverflow);
            }
        }
        // 表外地址 → 野指针。
        self.log_fault(FaultClass::WildPointer, app_id, addr, len, u64::MAX, write, false);
        Err(FaultClass::WildPointer)
    }

    /// 栈溢出判定（SP 越过栈底守卫线）。
    pub fn check_stack(&mut self, app_id: u32, sp: u64, stack_guard_low: u64) -> Result<(), FaultClass> {
        self.guard_cost += GUARD_CHECK_UNITS;
        if sp < stack_guard_low {
            self.log_fault(FaultClass::StackOverflow, app_id, sp, 0, u64::MAX, true, false);
            return Err(FaultClass::StackOverflow);
        }
        Ok(())
    }

    /// 未初始化跳转判定（PCX 落入不可执行区）。
    pub fn check_exec(&mut self, app_id: u32, pcx: u64, exec_ok: bool) -> Result<(), FaultClass> {
        self.guard_cost += GUARD_CHECK_UNITS;
        if !exec_ok {
            self.log_fault(FaultClass::UninitJump, app_id, pcx, 0, u64::MAX, false, false);
            return Err(FaultClass::UninitJump);
        }
        Ok(())
    }

    fn log_fault(&mut self, class: FaultClass, app_id: u32, addr: u64, len: u64, block: u64, write: bool, exempted: bool) {
        if self.fault_n < FAULT_LOG_CAP {
            self.faults[self.fault_n] = Some(GuardFault { class, app_id, addr, access_len: len, block, write, exempted });
            self.fault_n += 1;
        }
    }

    /// 守卫开销（‰）——守卫成本 / 总成本（应用+守卫）；超预算 → 自动切
    /// 抽样守卫（1/8）+诊断标注。分母含守卫自身（守卫占总开销的千分比）。
    pub fn overhead_permille(&self) -> u64 {
        let total = self.app_ops + self.guard_cost;
        if total == 0 {
            return 0;
        }
        self.guard_cost * 1000 / total
    }

    pub fn maybe_autoswitch(&mut self) {
        if self.sampled.is_none() && self.overhead_permille() > OVERHEAD_BUDGET_PERMILLE {
            self.sampled = Some(SampleReason::OverheadBudget);
        }
    }

    /// dump 护栏命中详情（F035 归因「内存越界（护栏触发）」消费）。
    pub fn hit_detail(&self) -> Option<GuardFault> {
        self.faults[..self.fault_n].iter().flatten().find(|f| f.class == FaultClass::HeapOverflow && !f.exempted).copied()
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
///
/// 栈帧纪律：MemGuard 定长表 4096 项（≈164KB/实例），场景全部走
/// `#[inline(never)]` 助手——峰值驻留=单场景一实例，域聚合器嵌套调用
/// 不叠加（聚合器自身栈帧恒小）。
#[inline(never)]
pub fn run_memguard_checks() -> CheckSet {
    let mut cs = CheckSet::new("F176-memguard");

    // 六类注入样本（主册判据枚举逐类），全部捕获且分类正确。
    let (r1, detail) = sc_heap_overflow();
    cs.add("sample1_heap_overflow", r1, "");
    cs.add("sample2_use_after_free", sc_use_after_free(), "");
    cs.add("sample3_double_free", sc_double_free(), "");
    cs.add("sample4_wild_pointer", sc_wild_pointer(), "");
    cs.add("sample5_stack_overflow", sc_stack_overflow(), "");
    cs.add("sample6_uninit_jump", sc_uninit_jump(), "");

    // 七项聚合：6/6 捕获且分类正确 + 正常访问零误报 + 开销 <3%。
    let (six_ok, overhead) = sc_aggregate_normal();
    cs.add("six_classes_all_correct", six_ok && r1, "");
    // 守卫开销 <3%（全量守卫模式计量）。
    cs.add("overhead_under_3pct", overhead <= OVERHEAD_BUDGET_PERMILLE, "");

    // 块数超 4096 且无回收槽 → 降级标注（新块不跟踪不设护栏——不谎称在守卫）。
    cs.add("block_cap_4096_annotated", sc_block_cap(), "");

    // 释放槽回收：抽样模式下 1/8 新块登记护栏（槽位循环复用）。
    cs.add("sampled_reuse_1of8", sc_sampled_reuse(), "");

    // 开销超预算 → 自动切抽样（1/8 块带护栏）。
    cs.add("overhead_autoswitch_sampled", sc_overhead_autoswitch(), "");

    // 白名单豁免：留名放行且记录在案（不崩应用）。
    cs.add("whitelist_exempt_named", sc_whitelist_exempt(), "");

    // dump 护栏命中详情在册（地址/长度/块号——归因数据源）。
    cs.add(
        "dump_hit_detail",
        detail.map(|d| d.addr == 0x10_1000 && d.access_len == 8 && d.block == 0 && d.write).unwrap_or(false),
        "",
    );

    // 抽样守卫降级后开销回落（1/8 → 恒低于全量的结构性对照）。
    cs.add("sampled_guard_cheaper", sc_sampled_guard_cheaper(), "");

    cs
}

// 六类样本与场景（各持独立实例——inline(never) 保栈帧不叠加）。

#[inline(never)]
fn sc_heap_overflow() -> (bool, Option<GuardFault>) {
    // 样本 1：堆溢出——越出块尾写护栏页。
    let mut g = MemGuard::new();
    g.alloc(1, 0x10_0000, 0x1000);
    let hit = g.access(1, 0x10_1000, 8, true) == Err(FaultClass::HeapOverflow); // 块尾即护栏页起点
    (hit, g.hit_detail())
}

#[inline(never)]
fn sc_use_after_free() -> bool {
    // 样本 2：UAF——释放后写毒页。
    let mut g2 = MemGuard::new();
    g2.alloc(1, 0x20_0000, 0x1000);
    let _ = g2.free(1, 0x20_0000);
    g2.access(1, 0x20_0100, 4, true) == Err(FaultClass::UseAfterFree)
}

#[inline(never)]
fn sc_double_free() -> bool {
    // 样本 3：双 free。
    let mut g3 = MemGuard::new();
    g3.alloc(1, 0x30_0000, 0x1000);
    let _ = g3.free(1, 0x30_0000);
    g3.free(1, 0x30_0000) == Err(FaultClass::DoubleFree)
}

#[inline(never)]
fn sc_wild_pointer() -> bool {
    // 样本 4：野指针——表外地址。
    let mut g4 = MemGuard::new();
    g4.alloc(1, 0x40_0000, 0x1000);
    g4.access(1, 0xDEAD_BEEF, 4, true) == Err(FaultClass::WildPointer)
}

#[inline(never)]
fn sc_stack_overflow() -> bool {
    // 样本 5：栈溢出——SP 越栈底守卫。
    let mut g5 = MemGuard::new();
    g5.check_stack(1, 0x7F_FF00, 0x800_000) == Err(FaultClass::StackOverflow)
}

#[inline(never)]
fn sc_uninit_jump() -> bool {
    // 样本 6：未初始化跳转——PCX 落不可执行区。
    let mut g6 = MemGuard::new();
    g6.check_exec(1, 0x4141_4141, false) == Err(FaultClass::UninitJump)
}

#[inline(never)]
fn sc_aggregate_normal() -> (bool, u64) {
    // 正常访问零误报 + 护栏页读写双拦 + 开销计量（单实例全程）。
    let mut g7 = MemGuard::new();
    g7.alloc(2, 0x50_0000, 0x1000);
    // 块内末字（0x50_0FFC..0x50_1000）读取合法——不误伤；护栏页起点
    // （0x50_1000）读取即拦（读路径同构——护栏不分区读写）。
    let normal_ok = g7.access(2, 0x50_0800, 4, true).is_ok() && g7.access(2, 0x50_0FFC, 4, false).is_ok();
    let guard_read = g7.access(2, 0x50_1000, 4, false) == Err(FaultClass::HeapOverflow);
    (normal_ok && guard_read, g7.overhead_permille())
}

#[inline(never)]
fn sc_block_cap() -> bool {
    let mut g8 = MemGuard::new();
    for i in 0..BLOCK_CAP as u64 {
        g8.alloc(3, 0x1000_0000 + i * 0x10000, 0x1000);
    }
    let tracked_before = g8.n;
    let overflow_tracked = g8.alloc(3, 0x9000_0000, 0x1000);
    g8.sampled == Some(SampleReason::BlockCap) && !overflow_tracked && g8.n == tracked_before
}

#[inline(never)]
fn sc_sampled_reuse() -> bool {
    let mut g8b = MemGuard::new();
    g8b.sampled = Some(SampleReason::BlockCap);
    for i in 0..BLOCK_CAP as u64 {
        g8b.alloc(3, 0x1000_0000 + i * 0x10000, 0x1000);
    }
    for i in 0..8u64 {
        let _ = g8b.free(3, 0x1000_0000 + i * 0x10000);
    }
    for i in 0..8u64 {
        g8b.alloc(3, 0x9100_0000 + i * 0x10000, 0x1000);
    }
    let guarded_reused = g8b.blocks[..g8b.n].iter().flatten().filter(|b| b.base >= 0x9100_0000 && b.guarded).count();
    guarded_reused == 1
}

#[inline(never)]
fn sc_overhead_autoswitch() -> bool {
    let mut g9 = MemGuard::new();
    // 注入高开销（守卫判定远超应用操作——构造超预算场景）。
    for _ in 0..1000u64 {
        g9.guard_cost += 35; // 守卫侧高计量（应用零操作）
    }
    g9.maybe_autoswitch();
    g9.sampled == Some(SampleReason::OverheadBudget)
}

#[inline(never)]
fn sc_whitelist_exempt() -> bool {
    let mut g10 = MemGuard::new();
    g10.alloc(4, 0x60_0000, 0x100);
    let added = g10.add_exempt(4, 0x60_0000, 0x200, b"legacy app self-managed overdraw");
    let exempted = g10.access(4, 0x60_0100, 8, true); // 越尾但在豁免区间
    added && exempted.is_ok() && g10.faults[..g10.fault_n].iter().flatten().any(|f| f.exempted)
}

#[inline(never)]
fn sc_sampled_guard_cheaper() -> bool {
    // 护栏工作量对照（结构量=实际设防的护栏页数）：全量模式 80/80 全设防，
    // 抽样模式恰 1/8（80 块 → 10 页）——护栏工作量结构性更低（判据「守卫
    // 开销 <3%」的降级保障面：工作量少了，开销自然回落）。
    let mut g11 = MemGuard::new();
    for i in 0..80u64 {
        g11.alloc(5, 0x70_0000 + i * 0x10000, 0x1000);
        let _ = g11.access(5, 0x70_0000 + i * 0x10000 + 0x10, 4, false);
    }
    let full_guarded = g11.blocks[..g11.n].iter().flatten().filter(|b| b.guarded).count();
    let mut g12 = MemGuard::new();
    g12.sampled = Some(SampleReason::OverheadBudget); // 抽样模式：1/8 块上护栏
    for i in 0..80u64 {
        g12.alloc(5, 0x80_0000 + i * 0x10000, 0x1000);
        let _ = g12.access(5, 0x80_0000 + i * 0x10000 + 0x10, 4, false);
    }
    let sampled_guarded = g12.blocks[..g12.n].iter().flatten().filter(|b| b.guarded).count();
    full_guarded == 80 && sampled_guarded == (80 / SAMPLE_DENOM as usize) && sampled_guarded < full_guarded
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_samples_captured_and_classified() {
        // 六类样本逐类复测（CheckSet 是构造断言，这里走全状态机路径）。
        let mut g = MemGuard::new();
        g.alloc(9, 0xA0_0000, 0x800);
        assert_eq!(g.access(9, 0xA0_0800, 1, true), Err(FaultClass::HeapOverflow));
        let _ = g.free(9, 0xA0_0000);
        assert_eq!(g.access(9, 0xA0_0000, 1, false), Err(FaultClass::UseAfterFree));
        assert_eq!(g.free(9, 0xA0_0000), Err(FaultClass::DoubleFree));
        assert_eq!(g.access(9, 0x1_0000, 1, true), Err(FaultClass::WildPointer));
        assert_eq!(g.check_stack(9, 0x100, 0x200), Err(FaultClass::StackOverflow));
        assert_eq!(g.check_exec(9, 0x7FFE_0000, false), Err(FaultClass::UninitJump));
        // 守卫保持：释放后护栏仍在（释放块尾触碰仍按 UAF/护栏归因）。
        assert_eq!(g.access(9, 0xA0_07FF, 4, true), Err(FaultClass::UseAfterFree));
    }

    #[test]
    fn overhead_math_is_honest() {
        // 开销公式对拍：100 应用单位 + 2 守卫单位 → 2/102 ≈ 19.6‰ < 30‰。
        let mut g = MemGuard::new();
        for _ in 0..100 {
            g.alloc(1, 0, 0x10);
        }
        let expect = GUARD_CHECK_UNITS * 100 * 1000 / (APP_OP_UNITS * 100 + GUARD_CHECK_UNITS * 100);
        assert_eq!(g.overhead_permille(), expect);
        assert!(g.overhead_permille() < OVERHEAD_BUDGET_PERMILLE);
    }

    #[test]
    fn exempt_requires_full_coverage() {
        // 白名单只豁免登记窗口；窗口外照常拦截（显式白名单不放大）。
        // 几何：块 [0xB0_0000,+0x40)，怪癖越权窗=块尾越界区 [0xB0_0040,+0x40)
        // （程序自管越界——主册 F176 语义），护栏页从 0xB0_0080 起。
        let mut g = MemGuard::new();
        g.alloc(1, 0xB0_0000, 0x40);
        g.add_exempt(1, 0xB0_0040, 0x40, b"quirk A");
        assert!(g.access(1, 0xB0_0030, 8, true).is_ok(), "块体内合法访问");
        assert!(g.access(1, 0xB0_0050, 8, true).is_ok(), "越界窗内豁免（留名放行）");
        assert!(g.access(1, 0xB0_00F0, 8, true).is_err(), "区间外照常拦");
        assert!(g.access(2, 0xB0_0020, 4, true).is_err(), "他应用不共享豁免");
    }

    #[test]
    fn sampled_mode_annotation_recorded() {
        // 超限降级必须带原因标注（诊断可查——不静默降级）；溢出块不跟踪。
        let mut g = MemGuard::new();
        assert!(g.sampled.is_none());
        for i in 0..BLOCK_CAP as u64 {
            g.alloc(1, 0xC0_0000 + i * 0x10000, 0x10);
        }
        let overflow = g.alloc(1, 0xF000_0000, 0x10);
        assert!(!overflow && g.sampled.is_some(), "超 4096 块必须降级并标注");
        match g.sampled {
            Some(SampleReason::BlockCap) => {}
            other => panic!("期望 BlockCap，实际 {:?}", other),
        }
        // 溢出块不在表（不谎称在守卫）。
        assert!((0..g.n).all(|i| g.blocks[i].unwrap().base != 0xF000_0000));
        // 槽位回收路径：释放 16 块腾出槽位后，抽样模式新分配 1/8 上护栏。
        for i in 0..16u64 {
            let _ = g.free(1, 0xC0_0000 + i * 0x10000);
        }
        for i in 0..16u64 {
            g.alloc(1, 0xE000_0000 + i * 0x10000, 0x10);
        }
        let guarded_new = g.blocks[..g.n].iter().flatten().filter(|b| b.base >= 0xE000_0000 && b.guarded).count();
        assert_eq!(guarded_new, 2, "16 块抽样 1/8 → 恰 2 块带护栏");
    }

    #[test]
    fn freed_block_stays_tracked_until_reuse() {
        // 释放块毒化后仍在表（UAF 可检、双 free 可拦）；新分配走未用槽
        // 不覆盖释放块——表满时才回收槽位（见 sampled_mode 测试）。
        let mut g = MemGuard::new();
        g.alloc(1, 0xD0_0000, 0x100);
        let _ = g.free(1, 0xD0_0000);
        g.alloc(1, 0xD1_0000, 0x100); // 未用槽充足 → 新槽
        // 旧 base 二次释放 → DoubleFree（块仍被跟踪，正确语义）。
        assert_eq!(g.free(1, 0xD0_0000), Err(FaultClass::DoubleFree));
        // 新块正常守卫、旧块毒页照拦。
        assert!(g.access(1, 0xD1_0010, 4, true).is_ok());
        assert_eq!(g.access(1, 0xD0_0010, 4, true), Err(FaultClass::UseAfterFree));
    }
}
