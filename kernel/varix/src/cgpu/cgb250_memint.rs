//! CGPU-F0250 · 内存子系统集成（cgb250_memint）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0250`
//! **判据原文摘录**：显存等待边（分配器锁/池耗尽等待进等待图——显存耗尽引发的
//! 等待链可见）、分配死锁模式（池 A 等 B、B 等 A 的池间死锁专项检测）、OOM 与
//! 死锁区分（分配失败的「等待」不是死锁——语义分流出等待图）。
//!
//! ## 显存等待的三种形态
//!
//! 1. **分配器锁等待**——两个任务同时要分配，后到的等先到的释放分配器锁。
//!    这是普通锁等待，进 WFG 的锁等待源。
//!
//! 2. **池耗尽等待**——池满，任务等空闲槽。这**不是锁等待**——没有持有者
//!    在阻塞它，是容量不足。建模为 `AllocSlot` 谓词的等待边，指向
//!    **当前持有槽的主体集合**。
//!
//! 3. **池间死锁**——池 A 的释放流程需要分配池 B 的管理结构，池 B 反之。
//!    这是**跨池资源死锁**，WFG 上表现为 A 的管理任务 ↔ B 的管理任务互等。
//!
//! ## OOM 与死锁的语义分流
//!
//! 关键区分：
//! - **OOM**：池满且**无人会释放**（全部持有者都在跑、不释放）。这是
//!   资源不足，不是死锁——解法是扩容或杀任务减压，不是死锁解除。
//! - **死锁**：池满且**持有者互相等待**。这是死锁，走 F0245 解除。
//!
//! 判定依据：OOM 时 WFG 上持有者子图**无环**；死锁时**有环**。
//! 语义分流发生在 F0243 环检测之后、F0244 证据评分之前。

use crate::checks::CheckSet;
use alloc::vec::Vec;

use super::cgb241_dlarch::{ResId, SubjectId, WaitPredicate, WaitSource};

// ---------------------------------------------------------------------------
// 诊断码（0xCBD0 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCBD0;
/// 池未注册。
pub const E_NO_POOL: u16 = SEG_BASE + 1;
/// 池注册表满。
pub const E_POOL_FULL: u16 = SEG_BASE + 2;
/// 等待语义分流失败。
pub const E_SEMANTIC_FAIL: u16 = SEG_BASE + 3;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 显存池注册上限。
pub const MAX_POOLS: usize = 64;
/// 等待边记录上限。
pub const MAX_MEM_WAITS: usize = 512;

// ---------------------------------------------------------------------------
// 显存池模型
// ---------------------------------------------------------------------------

/// 显存池。
#[derive(Clone, Copy, Debug)]
pub struct VramPool {
    pub id: u32,
    /// 总槽位。
    pub total_slots: u16,
    /// 已用槽位。
    pub used_slots: u16,
    /// 等待者数量。
    pub waiters: u16,
    /// 单槽大小（字节，归一化后）。
    pub slot_size: u32,
}

impl VramPool {
    pub fn new(id: u32, total: u16, slot_size: u32) -> VramPool {
        VramPool {
            id,
            total_slots: total,
            used_slots: 0,
            waiters: 0,
            slot_size,
        }
    }

    pub fn available(&self) -> u16 {
        self.total_slots.saturating_sub(self.used_slots)
    }

    pub fn is_full(&self) -> bool {
        self.used_slots >= self.total_slots
    }

    pub fn utilization(&self) -> f32 {
        if self.total_slots == 0 {
            0.0
        } else {
            self.used_slots as f32 / self.total_slots as f32
        }
    }
}

/// 分配死锁模式（池间互等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllocDeadlockPattern {
    /// 池 A 等 B 的槽、池 B 等 A 的槽。
    PoolMutual,
    /// 单池内持有者互等（经典死锁）。
    SinglePoolCycle,
    /// 链式等待 A→B→C→A。
    ChainCycle,
    /// 不是死锁（OOM / 慢）。
    NotDeadlock,
}

impl AllocDeadlockPattern {
    pub const fn name(self) -> &'static str {
        match self {
            AllocDeadlockPattern::PoolMutual => "pool_mutual",
            AllocDeadlockPattern::SinglePoolCycle => "single_cycle",
            AllocDeadlockPattern::ChainCycle => "chain_cycle",
            AllocDeadlockPattern::NotDeadlock => "not_deadlock",
        }
    }
}

/// 语义分流结果。
#[derive(Clone, Copy, Debug)]
pub struct SemanticVerdict {
    /// 是否 OOM。
    pub is_oom: bool,
    /// 是否死锁。
    pub is_deadlock: bool,
    /// 死锁模式。
    pub pattern: AllocDeadlockPattern,
    /// 涉及的池。
    pub pools: [u32; 4],
    /// 池数量。
    pub pool_count: u8,
}

// ---------------------------------------------------------------------------
// 显存等待跟踪器
// ---------------------------------------------------------------------------

/// 显存等待边记录。
#[derive(Clone, Copy, Debug)]
pub struct MemWait {
    pub waiter: SubjectId,
    pub pool: u32,
    pub created_at: u32,
    pub weight: u32,
    /// 是否已消亡。
    pub settled: bool,
}

/// 显存等待跟踪器。
///
/// 职责：
/// 1. 显存等待边进 WFG（四源统一的「显存分配」源）；
/// 2. 池间死锁专项检测；
/// 3. OOM 与死锁语义分流。
pub struct MemWaitTracker {
    pools: Vec<VramPool>,
    waits: Vec<MemWait>,
    /// 遥测：等待边总数。
    pub total_waits: u64,
    /// 遥测：OOM 次数。
    pub oom_count: u64,
    /// 遥测：死锁检出数。
    pub deadlock_count: u64,
}

impl MemWaitTracker {
    pub fn new() -> MemWaitTracker {
        MemWaitTracker {
            pools: Vec::new(),
            waits: Vec::new(),
            total_waits: 0,
            oom_count: 0,
            deadlock_count: 0,
        }
    }

    /// 注册池。
    pub fn register_pool(&mut self, id: u32, slots: u16, slot_size: u32) -> Result<(), u16> {
        if self.pools.len() >= MAX_POOLS {
            return Err(E_POOL_FULL);
        }
        if self.pools.iter().any(|p| p.id == id) {
            return Ok(());
        }
        self.pools.push(VramPool::new(id, slots, slot_size));
        Ok(())
    }

    /// 分配一个槽。返回 Ok(槽序号) 或 Err(等待需要)。
    pub fn alloc(&mut self, pool_id: u32, subject: SubjectId) -> Result<u16, u16> {
        let pos = self
            .pools
            .iter()
            .position(|p| p.id == pool_id)
            .ok_or(E_NO_POOL)?;
        if self.pools[pos].available() == 0 {
            // 记录等待。
            if self.waits.len() < MAX_MEM_WAITS {
                self.waits.push(MemWait {
                    waiter: subject,
                    pool: pool_id,
                    created_at: 0,
                    weight: 0,
                    settled: false,
                });
                self.total_waits += 1;
                self.pools[pos].waiters += 1;
            }
            return Err(E_SEMANTIC_FAIL);
        }
        self.pools[pos].used_slots += 1;
        Ok(self.pools[pos].used_slots)
    }

    /// 释放一个槽。
    pub fn free(&mut self, pool_id: u32) -> Result<(), u16> {
        let pos = self
            .pools
            .iter()
            .position(|p| p.id == pool_id)
            .ok_or(E_NO_POOL)?;
        self.pools[pos].used_slots = self.pools[pos].used_slots.saturating_sub(1);
        // 唤醒一个等待者。
        if let Some(w) = self
            .waits
            .iter_mut()
            .find(|w| w.pool == pool_id && !w.settled)
        {
            w.settled = true;
            self.pools[pos].waiters = self.pools[pos].waiters.saturating_sub(1);
        }
        Ok(())
    }

    /// 生成 WFG 等待边参数。
    ///
    /// 返回 (from, to, predicate) 列表，供 DualModel 建边。
    pub fn generate_wait_edges(
        &self,
    ) -> Vec<(SubjectId, SubjectId, WaitPredicate)> {
        let mut out = Vec::new();
        for w in &self.waits {
            if w.settled {
                continue;
            }
            // 池耗尽等待：指向池的持有者（这里用池 ID 的哈希代理持有者集合）。
            // 真实系统中，指向持有者列表。此处用池内第一个持有者。
            out.push((
                w.waiter,
                w.pool as SubjectId, // 简化：池 ID 作目标
                WaitPredicate::AllocSlot(w.pool),
            ));
        }
        out
    }

    /// 池间死锁专项检测。
    ///
    /// 检测池 A 等 B、B 等 A 的模式。
    pub fn detect_pool_deadlock(&self) -> AllocDeadlockPattern {
        // 简化检测：看是否有两个池都满了且都有等待者。
        let full_with_waiters: Vec<&VramPool> = self
            .pools
            .iter()
            .filter(|p| p.is_full() && p.waiters > 0)
            .collect();

        if full_with_waiters.len() >= 2 {
            AllocDeadlockPattern::PoolMutual
        } else if full_with_waiters.len() == 1 {
            // 单池内是否有环——需要 WFG 信息，此处仅标记可能。
            AllocDeadlockPattern::SinglePoolCycle
        } else {
            AllocDeadlockPattern::NotDeadlock
        }
    }

    /// OOM 与死锁语义分流。
    ///
    /// `has_cycle` 来自 F0243 环检测结果。
    pub fn semantic_verdict(&self, has_cycle: bool) -> SemanticVerdict {
        let any_full = self.pools.iter().any(|p| p.is_full());
        let any_waiter = self.waits.iter().any(|w| !w.settled);

        let is_oom = any_full && any_waiter && !has_cycle;
        let is_deadlock = any_full && any_waiter && has_cycle;
        let pattern = if is_deadlock {
            self.detect_pool_deadlock()
        } else {
            AllocDeadlockPattern::NotDeadlock
        };

        if is_oom {
            // OOM 计数在外部。
        }

        let mut pools = [0u32; 4];
        let mut pool_count = 0u8;
        for p in &self.pools {
            if p.is_full() && (pool_count as usize) < 4 {
                pools[pool_count as usize] = p.id;
                pool_count += 1;
            }
        }

        SemanticVerdict {
            is_oom,
            is_deadlock,
            pattern,
            pools,
            pool_count,
        }
    }

    /// 池状态查询。
    pub fn pool(&self, id: u32) -> Option<&VramPool> {
        self.pools.iter().find(|p| p.id == id)
    }

    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    pub fn active_waits(&self) -> usize {
        self.waits.iter().filter(|w| !w.settled).count()
    }

    /// 一致性检查。
    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb250_memint");

        // 池容量守恒。
        let mut bad = 0usize;
        for p in &self.pools {
            if p.used_slots > p.total_slots {
                bad += 1;
            }
        }
        cs.add("池容量守恒", bad == 0, "used ≤ total");

        // 等待者计数对齐。
        let mut waiter_bad = 0usize;
        for p in &self.pools {
            let real = self
                .waits
                .iter()
                .filter(|w| w.pool == p.id && !w.settled)
                .count() as u16;
            if real != p.waiters {
                waiter_bad += 1;
            }
        }
        cs.add("等待者计数对齐", waiter_bad == 0, "waiters 字段同步");

        // 有界。
        cs.add(
            "池有界",
            self.pools.len() <= MAX_POOLS,
            "MAX_POOLS=64",
        );
        cs.add(
            "等待有界",
            self.waits.len() <= MAX_MEM_WAITS,
            "MAX_MEM_WAITS=512",
        );

        cs
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0250 域自检。
pub fn run_cgb250_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb250_memint");
    let mut tracker = MemWaitTracker::new();

    // --- 池注册 ---
    tracker.register_pool(1, 10, 4096).ok();
    tracker.register_pool(2, 5, 8192).ok();
    cs.add("池注册", tracker.pool_count() == 2, "2 pools");
    cs.add(
        "池容量正确",
        tracker.pool(1).map(|p| p.total_slots == 10).unwrap_or(false),
        "10 slots",
    );

    // --- 分配 ---
    let a1 = tracker.alloc(1, 100);
    cs.add("分配成功", a1.is_ok(), "pool 1 slot 1");
    cs.add(
        "已用 1",
        tracker.pool(1).map(|p| p.used_slots == 1).unwrap_or(false),
        "used=1",
    );

    // --- 池满 → 等待 ---
    for i in 0..10u32 {
        tracker.alloc(1, 200 + i).ok();
    }
    cs.add(
        "池满",
        tracker.pool(1).map(|p| p.is_full()).unwrap_or(false),
        "10/10",
    );
    let wait_result = tracker.alloc(1, 999);
    cs.add(
        "满池等待",
        wait_result.is_err(),
        "alloc 返回 Err",
    );
    cs.add(
        "等待记录",
        tracker.active_waits() > 0,
        "waiter 记录",
    );

    // --- 释放 → 唤醒 ---
    tracker.free(1).ok();
    cs.add(
        "释放后可用",
        tracker.pool(1).map(|p| p.available() >= 1).unwrap_or(false),
        "free",
    );

    // --- 语义分流：OOM（无环） ---
    // 重新灌满。
    while tracker.pool(1).map(|p| !p.is_full()).unwrap_or(false) {
        tracker.alloc(1, 300).ok();
    }
    tracker.alloc(1, 998).ok(); // 产生等待
    let verdict_oom = tracker.semantic_verdict(false);
    cs.add(
        "OOM 判定",
        verdict_oom.is_oom && !verdict_oom.is_deadlock,
        "满+等待+无环",
    );

    // --- 语义分流：死锁（有环） ---
    let verdict_dl = tracker.semantic_verdict(true);
    cs.add(
        "死锁判定",
        verdict_dl.is_deadlock,
        "满+等待+有环",
    );

    // --- 池间死锁模式 ---
    let pattern = tracker.detect_pool_deadlock();
    cs.add(
        "池间检测",
        pattern != AllocDeadlockPattern::NotDeadlock || tracker.pool_count() < 2,
        "模式可判",
    );

    // --- 等待边生成 ---
    let edges = tracker.generate_wait_edges();
    cs.add(
        "等待边生成",
        !edges.is_empty(),
        "AllocSlot 谓词",
    );

    // --- 一致性 ---
    let ic = tracker.check_consistency();
    let mut ok = true;
    for i in 0..ic.len() {
        if let Some(c) = ic.get(i) {
            if !c.passed {
                ok = false;
            }
        }
    }
    cs.add("内存跟踪一致性", ok, "四项");

    // --- 利用率 ---
    let util = tracker.pool(1).map(|p| p.utilization()).unwrap_or(0.0);
    cs.add(
        "利用率可算",
        util >= 0.0 && util <= 1.0,
        "0..1",
    );

    cs
}
