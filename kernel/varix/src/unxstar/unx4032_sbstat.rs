//! unx4032_sbstat — UNX-F4032 sb 统计账（对象计数/缓存水位）（诊断段 0x8680）。
//!
//! 判据原文：
//!   J1 sb 级对象计数（inode/dentry 池占用）与全局账交叉一致（两账 diff 零差）；
//!   J2 统计快照可导出。
//! 设计纪律：
//!   1. 双账模型：每 sb 持本地对象账（inode/dentry 占用计数），
//!      全局持汇总账；对象分配/回收双记账，两账 diff 必须恒为零。
//!   2. 水位模型：池容量定编（inode 64 / dentry 128），占用/容量
//!      即水位（千分比整数化，无浮点）；越线（>1000‰）拒分配。
//!   3. 快照导出：统计快照按值导出（容量/占用/峰值/水位），
//!      快照不携带可变引用，导出后账面继续演化不影响快照。
//!   4. 守恒式：alloc_total - free_total = 占用 = 全局 diff 分量；
//!      峰值 ≥ 当前占用 ≥ 0；free 不得超 alloc（下溢拒收）。
//!   5. 边界：池满分配、空池回收、重复回收、越界句柄——全部
//!      显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8680;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_INO_FULL: u16 = SEG_BASE + 1;    // inode 池满（水位越线）
const E_DENT_FULL: u16 = SEG_BASE + 2;   // dentry 池满（水位越线）
const E_FREE_UNDER: u16 = SEG_BASE + 3;  // 回收下溢（空池/重复回收）
const E_BAD_SLOT: u16 = SEG_BASE + 4;    // 越界槽位句柄
const E_DIFF_NZ: u16 = SEG_BASE + 5;     // 两账 diff 非零（交叉不一致）
const E_BAL: u16 = SEG_BASE + 6;         // 守恒式失守（峰值/占用矛盾）

/// 池容量定编（内核典型静态配额，写死）。
pub const INO_POOL_CAP: u32 = 64;
pub const DENT_POOL_CAP: u32 = 128;
/// 水位千分比基准。
pub const WATERMARK_BASE: u32 = 1000;

/// 单池账：容量 + 占用 + 峰值 + 累计分配/回收。
#[derive(Clone, Copy)]
pub struct PoolStat {
    pub cap: u32,        // 池容量
    pub used: u32,       // 当前占用
    pub peak: u32,       // 历史峰值
    pub alloc_total: u32, // 累计分配
    pub free_total: u32,  // 累计回收
}

impl PoolStat {
    fn new(cap: u32) -> PoolStat {
        PoolStat { cap, used: 0, peak: 0, alloc_total: 0, free_total: 0 }
    }

    /// 分配：容量校验 + 峰值推进。
    fn alloc(&mut self) -> Result<(), u16> {
        if self.used >= self.cap {
            return Err(E_INO_FULL);
        }
        self.used += 1;
        self.alloc_total += 1;
        if self.used > self.peak {
            self.peak = self.used;
        }
        Ok(())
    }

    /// 回收：下溢拒收。
    fn release(&mut self) -> Result<(), u16> {
        if self.used == 0 {
            return Err(E_FREE_UNDER);
        }
        self.used -= 1;
        self.free_total += 1;
        Ok(())
    }

    /// 水位（千分比整数）：占用 * 1000 / 容量，无浮点。
    fn watermark(&self) -> u32 {
        if self.cap == 0 {
            return 0;
        }
        self.used * WATERMARK_BASE / self.cap
    }

    /// 守恒核对：alloc - free = used；峰值 ≥ used。
    fn check(&self) -> Result<(), u16> {
        if self.alloc_total < self.free_total {
            return Err(E_BAL);
        }
        if self.alloc_total - self.free_total != self.used {
            return Err(E_BAL);
        }
        if self.peak < self.used {
            return Err(E_BAL);
        }
        Ok(())
    }
}

/// sb 级统计账：inode/dentry 双池 + 槽位占用位图（线性表）。
pub struct SbStat {
    pub ino_pool: PoolStat,
    pub dent_pool: PoolStat,
    pub ino_slots: Vec<bool>,  // inode 槽占用位图
    pub dent_slots: Vec<bool>, // dentry 槽占用位图
}

/// 全局汇总账：跨 sb 汇总对象计数（交叉对账用）。
pub struct GlobalStat {
    pub g_ino_used: u32,
    pub g_dent_used: u32,
    pub sb_count: u32,
}

/// 模块枢纽：注册 sb 统计账，分配/回收对象，交叉对账与快照导出。
pub struct StatHub {
    pub sbs: Vec<SbStat>,
    pub global: GlobalStat,
    pub reject_total: u32, // 错误账
}

impl StatHub {
    pub fn new() -> StatHub {
        StatHub {
            sbs: Vec::new(),
            global: GlobalStat { g_ino_used: 0, g_dent_used: 0, sb_count: 0 },
            reject_total: 0,
        }
    }

    /// 注册新 sb（双池容量按常量定编）。
    pub fn add_sb(&mut self) -> Result<usize, u16> {
        self.sbs.push(SbStat {
            ino_pool: PoolStat::new(INO_POOL_CAP),
            dent_pool: PoolStat::new(DENT_POOL_CAP),
            ino_slots: Vec::new(),
            dent_slots: Vec::new(),
        });
        self.global.sb_count += 1;
        Ok(self.sbs.len() - 1)
    }

    /// 分配 inode 槽：sb 池满拒收，双账同步。
    pub fn alloc_ino(&mut self, sb: usize) -> Result<usize, u16> {
        if sb >= self.sbs.len() {
            self.reject_total += 1;
            return Err(E_BAD_SLOT);
        }
        if self.sbs[sb].ino_pool.used >= self.sbs[sb].ino_pool.cap {
            self.reject_total += 1;
            return Err(E_INO_FULL);
        }
        // 找空槽（线性位图，先比长度再索引）。
        let mut slot = None;
        let mut i = 0;
        while i < self.sbs[sb].ino_slots.len() {
            if !self.sbs[sb].ino_slots[i] {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let s = match slot {
            Some(s) => s,
            None => {
                if self.sbs[sb].ino_slots.len() >= self.sbs[sb].ino_pool.cap as usize {
                    self.reject_total += 1;
                    return Err(E_INO_FULL);
                }
                self.sbs[sb].ino_slots.push(true);
                self.sbs[sb].ino_slots.len() - 1
            }
        };
        self.sbs[sb].ino_slots[s] = true;
        let _ = self.sbs[sb].ino_pool.alloc();
        self.global.g_ino_used += 1;
        Ok(s)
    }

    /// 分配 dentry 槽：同上，双账同步。
    pub fn alloc_dent(&mut self, sb: usize) -> Result<usize, u16> {
        if sb >= self.sbs.len() {
            self.reject_total += 1;
            return Err(E_BAD_SLOT);
        }
        if self.sbs[sb].dent_pool.used >= self.sbs[sb].dent_pool.cap {
            self.reject_total += 1;
            return Err(E_DENT_FULL);
        }
        let mut slot = None;
        let mut i = 0;
        while i < self.sbs[sb].dent_slots.len() {
            if !self.sbs[sb].dent_slots[i] {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let s = match slot {
            Some(s) => s,
            None => {
                if self.sbs[sb].dent_slots.len() >= self.sbs[sb].dent_pool.cap as usize {
                    self.reject_total += 1;
                    return Err(E_DENT_FULL);
                }
                self.sbs[sb].dent_slots.push(true);
                self.sbs[sb].dent_slots.len() - 1
            }
        };
        self.sbs[sb].dent_slots[s] = true;
        let _ = self.sbs[sb].dent_pool.alloc();
        self.global.g_dent_used += 1;
        Ok(s)
    }

    /// 回收 inode 槽：下溢/越界拒收，双账同步。
    pub fn free_ino(&mut self, sb: usize, slot: usize) -> Result<(), u16> {
        if sb >= self.sbs.len() || slot >= self.sbs[sb].ino_slots.len() {
            self.reject_total += 1;
            return Err(E_BAD_SLOT);
        }
        if !self.sbs[sb].ino_slots[slot] {
            self.reject_total += 1;
            return Err(E_FREE_UNDER);
        }
        self.sbs[sb].ino_slots[slot] = false;
        self.sbs[sb].ino_pool.release()?;
        self.global.g_ino_used -= 1;
        Ok(())
    }

    /// 交叉对账：sb 本地占用求和 = 全局账，diff 必须为零（判据 J1）。
    pub fn cross_audit(&self) -> Result<(), u16> {
        let mut sum_ino = 0u32;
        let mut sum_dent = 0u32;
        let mut i = 0;
        while i < self.sbs.len() {
            if i < self.sbs.len() {
                sum_ino += self.sbs[i].ino_pool.used;
                sum_dent += self.sbs[i].dent_pool.used;
            }
            i += 1;
        }
        if sum_ino != self.global.g_ino_used || sum_dent != self.global.g_dent_used {
            return Err(E_DIFF_NZ);
        }
        // 逐池守恒核对。
        let mut j = 0;
        while j < self.sbs.len() {
            self.sbs[j].ino_pool.check()?;
            self.sbs[j].dent_pool.check()?;
            j += 1;
        }
        Ok(())
    }

    /// 统计快照导出（判据 J2）：按值拷贝，不含可变引用。
    pub fn snapshot(&mut self, sb: usize) -> Result<(PoolStat, PoolStat), u16> {
        if sb >= self.sbs.len() {
            self.reject_total += 1;
            return Err(E_BAD_SLOT);
        }
        Ok((self.sbs[sb].ino_pool, self.sbs[sb].dent_pool))
    }

    /// 水位查询（千分比）。
    pub fn watermark(&self, sb: usize) -> Result<(u32, u32), u16> {
        if sb >= self.sbs.len() {
            return Err(E_BAD_SLOT);
        }
        Ok((self.sbs[sb].ino_pool.watermark(), self.sbs[sb].dent_pool.watermark()))
    }
}

/// 自例程：R1 分配回收 → R2 水位/快照 → R3 池满注入 → R4 回收滥用注入 → R5 交叉对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut hub = StatHub::new();
    let s0 = hub.add_sb()?;

    // R1 正常路径：分配 3 inode + 2 dentry，占用账同步。
    let i1 = hub.alloc_ino(s0)?;
    let _i2 = hub.alloc_ino(s0)?;
    let _i3 = hub.alloc_ino(s0)?;
    let _d1 = hub.alloc_dent(s0)?;
    let _d2 = hub.alloc_dent(s0)?;
    hub.free_ino(s0, i1)?;
    if hub.sbs[s0].ino_pool.used != 2 || hub.global.g_ino_used != 2 {
        return Err(E_DIFF_NZ);
    }
    hits += 1;

    // R2 快照与水位：快照按值导出，水位千分比无浮点。
    let (ip, dp) = hub.snapshot(s0)?;
    if ip.used != 2 || dp.used != 2 || ip.cap != INO_POOL_CAP {
        return Err(E_BAL);
    }
    let (wi, wd) = hub.watermark(s0)?;
    if wi != 2 * WATERMARK_BASE / INO_POOL_CAP || wd != 2 * WATERMARK_BASE / DENT_POOL_CAP {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 池满注入：压满 inode 池后继续分配被拒。
    let mut k = hub.sbs[s0].ino_pool.used;
    while k < INO_POOL_CAP {
        hub.alloc_ino(s0)?;
        k += 1;
    }
    if hub.alloc_ino(s0) != Err(E_INO_FULL) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 回收滥用注入：重复回收与越界句柄分别被拒。
    let last = INO_POOL_CAP as usize - 1;
    hub.free_ino(s0, last)?;
    if hub.free_ino(s0, last) != Err(E_FREE_UNDER) {
        return Err(E_BAL);
    }
    if hub.free_ino(s0, 0xFFFF) != Err(E_BAD_SLOT) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 交叉对账：sb 本地求和 = 全局账，diff 零差。
    hub.cross_audit()?;
    hits += 1;

    Ok(hits)
}
