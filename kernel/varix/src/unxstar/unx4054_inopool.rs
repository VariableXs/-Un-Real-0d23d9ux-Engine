//! unx4054_inopool — UNX-F4054 inode 池与静态配额（no_alloc 约束）（诊断段 0x8920）。
//!
//! 判据原文（提炼）：
//!   J1. 池静态配额耗尽注入返回 ENOSPC 且池结构零破坏。
//!   J2. 池水位账与占用账交叉一致（free+used==容量恒成立）。
//!   J3. 释放后槽位可复用，占位账随取随还精确归零能力可验。
//!   J4. 空池取号、重复释放均按闭集错误拒收。
//! 设计纪律：
//!   1. 池本体为固定长度槽位数组（本锚定编 64 槽），分配期一次
//!      建账，运行期零动态扩容（no_alloc 约束的池面表达）。
//!   2. 空闲账用槽位号环账承载：freed 槽位号 FIFO 复用，确定性。
//!   3. 每槽位状态机 Free→Used→Free，越权迁移（重复释放/双取）
//!      按闭集错误拒收且账面不动。
//!   4. 水位账 = used 计数，占用账逐槽枚举复核，交叉一致性入账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8920;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENOSPC: u16 = SEG_BASE + 1;  // 静态配额耗尽（POSIX ENOSPC）
const E_SLOT_BUSY: u16 = SEG_BASE + 2; // 槽位双取（Free 态才可取）
const E_DUP_FREE: u16 = SEG_BASE + 3;  // 重复释放（Used 态才可还）
const E_BAD_SLOT: u16 = SEG_BASE + 4;  // 槽位号超界
const E_WATER_INV: u16 = SEG_BASE + 5; // 水位账与占用账交叉不平
const E_POOL_INV: u16 = SEG_BASE + 6;  // 池结构破坏（槽位账自检失败）
const E_BAD_ARG: u16 = SEG_BASE + 7;   // 参数非法（0 配额/0 号 inode）

/// 池容量定编：64 槽（本锚静态配额）。
pub const POOL_CAP: usize = 64;

/// 槽位状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Free, // 空闲可取
    Used, // 已占用（承载 inode）
}

/// 静态池槽位：状态 + 持有 inode 号（0 表无持有）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolSlot {
    pub state: SlotState,
    pub ino: u64, // 持有的 inode 号（Free 态恒 0）
}

impl PoolSlot {
    fn fresh() -> PoolSlot {
        PoolSlot { state: SlotState::Free, ino: 0 }
    }
}

/// 静态 inode 池：槽位表 + 空闲环账 + 水位账。
pub struct InodePool {
    pub slots: Vec<PoolSlot>, // 固定容量槽位表（建账期一次成型）
    pub free_ring: Vec<usize>,// 空闲槽位号环账（FIFO 复用序）
    pub cap: usize,           // 静态配额
    pub used: u32,            // 水位账（占用计数）
    pub alloc_total: u32,     // 历史取号总数
    pub free_total: u32,      // 历史还号总数
    pub enospc_total: u32,    // ENOSPC 注入命中数（判据 J1 面）
}

impl InodePool {
    /// 建池：一次性开出 cap 槽位与全量空闲环账（no_alloc 面）。
    pub fn new(cap: usize) -> Result<InodePool, u16> {
        if cap == 0 || cap > POOL_CAP {
            return Err(E_BAD_ARG);
        }
        let mut slots = Vec::new();
        let mut ring = Vec::new();
        for i in 0..cap {
            slots.push(PoolSlot::fresh());
            ring.push(i);
        }
        Ok(InodePool {
            slots,
            free_ring: ring,
            cap,
            used: 0,
            alloc_total: 0,
            free_total: 0,
            enospc_total: 0,
        })
    }

    /// 取号：环账头部弹出；空池或耗尽 → ENOSPC（结构零破坏）。
    pub fn take(&mut self, ino: u64) -> Result<usize, u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.free_ring.is_empty() {
            self.enospc_total += 1;
            return Err(E_ENOSPC);
        }
        // FIFO：环账头部弹出（先还先复用，确定性）
        let slot = self.free_ring.remove(0);
        if slot >= self.cap || self.slots[slot].state != SlotState::Free {
            // 池结构自检失败：环账与槽位态不一致
            return Err(E_POOL_INV);
        }
        self.slots[slot].state = SlotState::Used;
        self.slots[slot].ino = ino;
        self.used += 1;
        self.alloc_total += 1;
        Ok(slot)
    }

    /// 还号：槽位置 Free 并入环账尾部；重复释放拒收。
    pub fn give_back(&mut self, slot: usize) -> Result<(), u16> {
        if slot >= self.cap {
            return Err(E_BAD_SLOT);
        }
        if self.slots[slot].state != SlotState::Used {
            return Err(E_DUP_FREE);
        }
        self.slots[slot].state = SlotState::Free;
        self.slots[slot].ino = 0;
        if self.used == 0 {
            return Err(E_WATER_INV);
        }
        self.used -= 1;
        self.free_total += 1;
        self.free_ring.push(slot);
        Ok(())
    }

    /// 按 inode 号还号（持号归还便捷面）。
    pub fn give_back_ino(&mut self, ino: u64) -> Result<(), u16> {
        for i in 0..self.cap {
            if self.slots[i].state == SlotState::Used && self.slots[i].ino == ino {
                return self.give_back(i);
            }
        }
        Err(E_BAD_SLOT)
    }

    /// 占用账：逐槽枚举 Used 计数（与水位账交叉复核）。
    pub fn occupancy_count(&self) -> u32 {
        let mut c = 0;
        for s in &self.slots {
            if s.state == SlotState::Used {
                c += 1;
            }
        }
        c
    }

    /// 空闲账自检：环账长度 + 占用数 == 容量。
    pub fn structure_ok(&self) -> bool {
        if self.free_ring.len() as u32 + self.used != self.cap as u32 {
            return false;
        }
        // 环账内不得出现 Used 槽位号
        for &s in &self.free_ring {
            if s >= self.cap || self.slots[s].state != SlotState::Free {
                return false;
            }
        }
        true
    }

    /// 交叉一致复核（判据 J2）：水位账 == 占用账 且结构自检通过。
    pub fn crosscheck(&self) -> Result<(), u16> {
        if !self.structure_ok() {
            return Err(E_POOL_INV);
        }
        if self.used != self.occupancy_count() {
            return Err(E_WATER_INV);
        }
        Ok(())
    }
}

// ── 扩展面：池审计与水位诊断 ─────────────────────────────────

impl InodePool {
    /// 空闲槽位计数（环账长度只读面，水位账互补量）。
    pub fn free_count(&self) -> u32 {
        self.free_ring.len() as u32
    }

    /// 水位千分比读数：used*1000/cap（整数化水位，无浮点纪律）。
    pub fn water_permille(&self) -> Result<u32, u16> {
        if self.cap == 0 {
            return Err(E_BAD_ARG);
        }
        self.crosscheck()?;
        Ok(self.used * 1000 / self.cap as u32)
    }

    /// 槽位态双计数审计：返回（free, used），供交叉复核（判据 J2 面）。
    pub fn audit_slots(&self) -> (u32, u32) {
        let mut free = 0;
        let mut used = 0;
        for s in &self.slots {
            match s.state {
                SlotState::Free => free += 1,
                SlotState::Used => used += 1,
            }
        }
        (free, used)
    }

    /// 环账定序审计：空闲环账内槽位号不得重复（结构自检增强面）。
    pub fn ring_unique(&self) -> bool {
        for i in 0..self.free_ring.len() {
            for j in (i + 1)..self.free_ring.len() {
                if self.free_ring[i] == self.free_ring[j] {
                    return false;
                }
            }
        }
        true
    }

    /// 持有查问：按槽位号读持有 inode 号（Free 态返回 0）。
    pub fn holder_of(&self, slot: usize) -> Result<u64, u16> {
        if slot >= self.cap {
            return Err(E_BAD_SLOT);
        }
        Ok(self.slots[slot].ino)
    }

    /// 诊断账复位：ENOSPC 命中账与取还历史账清零（槽位态不动，
    /// 供回归账重跑；结构性自检先行）。
    pub fn reset_ledgers(&mut self) -> Result<(), u16> {
        self.crosscheck()?;
        self.enospc_total = 0;
        self.alloc_total = 0;
        self.free_total = 0;
        Ok(())
    }
}

/// 自例程：满池取号→ENOSPC 注入→释放复用→交叉对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut p = InodePool::new(8)?;

    // R1 正常路径：取满 8 槽，水位与占用账同步
    for k in 1..=8u64 {
        p.take(k)?;
    }
    p.crosscheck()?;
    if p.used != 8 {
        return Err(E_WATER_INV);
    }
    hits += 1;

    // R2 ENOSPC 注入：第 9 取 → ENOSPC 且池结构零破坏
    match p.take(9) {
        Err(E_ENOSPC) => {}
        _ => return Err(E_ENOSPC),
    }
    if p.enospc_total != 1 {
        return Err(E_ENOSPC);
    }
    p.crosscheck()?;
    hits += 1;

    // R3 释放复用：还 2 槽后复取，环账 FIFO 序复用先还槽
    p.give_back_ino(2)?;
    p.give_back_ino(4)?;
    let s1 = p.take(100)?;
    if p.slots[s1].ino != 100 {
        return Err(E_POOL_INV);
    }
    if s1 != 1 { // 槽位 2（ino=2）先还，FIFO 应先复用
        return Err(E_WATER_INV);
    }
    p.crosscheck()?;
    hits += 1;

    // R4 重复释放注入：已 Free 槽位再还 → 拒收且账面不动
    match p.give_back(s1) {
        Ok(_) => {}
        Err(E_DUP_FREE) => {}
        _ => return Err(E_DUP_FREE),
    }
    let used_before = p.used;
    match p.give_back(s1) {
        Err(E_DUP_FREE) => {}
        _ => return Err(E_DUP_FREE),
    }
    if p.used != used_before {
        return Err(E_WATER_INV);
    }
    hits += 1;

    // R5 终态对账：alloc/free 历史账与终态水位闭环
    p.crosscheck()?;
    // 取 9 次（8+1），还 2 次；终态 used 应为 7
    if p.alloc_total != 9 || p.free_total != 2 || p.used != 7 {
        return Err(E_WATER_INV);
    }
    hits += 1;

    Ok(hits)
}
