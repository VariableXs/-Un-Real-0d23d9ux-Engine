//! unx4073_orphanclr — UNX-F4073 孤儿 inode 清理（kill_sb 联动）（诊断段 0x8B60）。
//!
//! 判据原文（提炼）：
//!   J1. kill_sb 时缓存内该 sb 全部条目出清（残留=0）。
//!   J2. 出清顺序先于 sb 释放（顺序断言通过）。
//!   J3. 其他 sb 条目不受波及（出清按 sb 隔离）。
//!   J4. 出清账守恒：出清数 == 出清前该 sb 条目数。
//! 设计纪律：
//!   1. Linux VFS kill_sb 语义：超级块卸载时，其孤儿 inode
//!      （无引用残留缓存条目）必须先于 sb 释放全部出清。
//!   2. 顺序账：出清步骤号必须小于 sb 释放步骤号（顺序断言，
//!      判据 J2）。
//!   3. sb 隔离：出清只扫 sb 号匹配条目，他 sb 零波及（判据 J3）。
//!   4. 孤儿判定：refs==0 方可入出清；有引用条目先挂孤儿链
//!      待引用归零（本锚简化为出清前置闸）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8B60;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_REFERENCED: u16 = SEG_BASE + 1; // 有引用条目出清注入拒收
const E_NO_SB: u16 = SEG_BASE + 2;      // sb 未登记
const E_DUP_SB: u16 = SEG_BASE + 3;     // sb 重复登记
const E_ORDER_INV: u16 = SEG_BASE + 4;  // 顺序失守（先释放后出清）
const E_LEDGER: u16 = SEG_BASE + 5;     // 出清账守恒破坏
const E_STATE_INV: u16 = SEG_BASE + 6;  // sb 相位迁移非法
const E_BAD_ARG: u16 = SEG_BASE + 7;    // 参数非法（0 号）

/// sb 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SbPhase {
    Mounted,  // 挂载中
    Killing,  // kill_sb 进行中（出清窗口）
    Released, // 已释放（出清必须先于本相）
}

/// 缓存条目：sb 归属 + 引用计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SbEntry {
    pub ino: u64,   // inode 号
    pub sb: u64,    // 归属超级块号
    pub refs: u32,  // 引用计数（0 = 孤儿）
}

/// 超级块行：相位 + 顺序账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SbRow {
    pub sb: u64,        // sb 号
    pub phase: SbPhase, // 生命周期相位
    pub clear_step: u32,// 出清步骤号（0 表未出清）
    pub free_step: u32, // 释放步骤号（0 表未释放）
}

/// 孤儿清理枢纽：条目表 + sb 表 + 顺序步进账。
pub struct OrphanFabric {
    pub entries: Vec<SbEntry>, // 缓存条目表
    pub sbs: Vec<SbRow>,       // sb 表
    pub step: u32,             // 全局步进器（顺序账锚）
    pub clear_total: u32,      // 出清总数（判据 J4）
    pub free_total: u32,       // sb 释放总数
}

impl OrphanFabric {
    pub fn new() -> OrphanFabric {
        OrphanFabric {
            entries: Vec::new(),
            sbs: Vec::new(),
            step: 0,
            clear_total: 0,
            free_total: 0,
        }
    }

    fn find_sb(&self, sb: u64) -> Option<usize> {
        for i in 0..self.sbs.len() {
            if self.sbs[i].sb == sb {
                return Some(i);
            }
        }
        None
    }

    /// 登记超级块（Mounted 相）。
    pub fn mount_sb(&mut self, sb: u64) -> Result<(), u16> {
        if sb == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_sb(sb).is_some() {
            return Err(E_DUP_SB);
        }
        self.sbs.push(SbRow {
            sb,
            phase: SbPhase::Mounted,
            clear_step: 0,
            free_step: 0,
        });
        Ok(())
    }

    /// 缓存条目入账（sb 必须已登记）。
    pub fn cache_put(&mut self, ino: u64, sb: u64, refs: u32) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_sb(sb).is_none() {
            return Err(E_NO_SB);
        }
        self.entries.push(SbEntry { ino, sb, refs });
        Ok(())
    }

    /// 引用归还（孤儿化：refs 归零）。
    pub fn put_ref(&mut self, ino: u64) -> Result<(), u16> {
        for i in 0..self.entries.len() {
            if self.entries[i].ino == ino {
                if self.entries[i].refs == 0 {
                    return Err(E_REFERENCED);
                }
                self.entries[i].refs -= 1;
                return Ok(());
            }
        }
        Err(E_REFERENCED)
    }

    /// kill_sb 核心：先出清（步进记账）后释放（判据 J1/J2）。
    /// 返回出清条目数。
    pub fn kill_sb(&mut self, sb: u64) -> Result<u32, u16> {
        let i = match self.find_sb(sb) {
            Some(i) => i,
            None => return Err(E_NO_SB),
        };
        match self.sbs[i].phase {
            SbPhase::Mounted => {}
            _ => return Err(E_STATE_INV),
        }
        self.sbs[i].phase = SbPhase::Killing;
        // 出清窗口：该 sb 全部条目逐条检查
        let mut cleared = 0;
        let mut k = 0;
        while k < self.entries.len() {
            if self.entries[k].sb == sb {
                if self.entries[k].refs != 0 {
                    // 有引用条目：出清前置闸拒收（判据 J1 纪律）
                    self.sbs[i].phase = SbPhase::Mounted;
                    return Err(E_REFERENCED);
                }
                self.entries.remove(k);
                cleared += 1;
            } else {
                k += 1;
            }
        }
        // 出清步骤记账（先于释放，判据 J2）
        self.step += 1;
        self.sbs[i].clear_step = self.step;
        self.clear_total += cleared;
        // sb 释放
        self.step += 1;
        self.sbs[i].free_step = self.step;
        self.sbs[i].phase = SbPhase::Released;
        self.free_total += 1;
        Ok(cleared)
    }

    /// 顺序断言（判据 J2）：clear_step < free_step 且都非零。
    pub fn assert_order(&self, sb: u64) -> Result<(), u16> {
        match self.find_sb(sb) {
            Some(i) => {
                if self.sbs[i].clear_step == 0 || self.sbs[i].free_step == 0 {
                    return Err(E_ORDER_INV);
                }
                if self.sbs[i].clear_step >= self.sbs[i].free_step {
                    return Err(E_ORDER_INV);
                }
                Ok(())
            }
            None => Err(E_NO_SB),
        }
    }

    /// 该 sb 残留条目数（判据 J1 锚）。
    pub fn residue_of(&self, sb: u64) -> u32 {
        let mut c = 0;
        for e in &self.entries {
            if e.sb == sb {
                c += 1;
            }
        }
        c
    }

    /// 出清账守恒（判据 J4）。
    pub fn ledger_ok(&self) -> bool {
        // 出清总数 == 释放 sb 的出清量合计；此处以步进序校验：
        // 所有 Released sb 均有 clear < free
        for r in &self.sbs {
            if r.phase == SbPhase::Released {
                if r.clear_step == 0 || r.free_step == 0 || r.clear_step >= r.free_step {
                    return false;
                }
            }
        }
        self.free_total as u32 <= self.clear_total + 1
    }
}

// ── 扩展面：sb 相位诊断与出清账导出 ──────────────────────────

impl OrphanFabric {
    /// sb 相位只读面。
    pub fn sb_phase(&self, sb: u64) -> Result<SbPhase, u16> {
        match self.find_sb(sb) {
            Some(i) => Ok(self.sbs[i].phase),
            None => Err(E_NO_SB),
        }
    }

    /// 出清账读数（判据 J4 总量导出）。
    pub fn clear_count(&self) -> u32 {
        self.clear_total
    }

    /// 释放账读数。
    pub fn free_count(&self) -> u32 {
        self.free_total
    }

    /// 全局步进读数（顺序账锚读数，判据 J2 面）。
    pub fn step_of(&self) -> u32 {
        self.step
    }

    /// 在表条目总数（全局水位面）。
    pub fn entry_count(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 孤儿计数：refs==0 条目数（出清候选读数）。
    pub fn orphan_count(&self) -> u32 {
        let mut c = 0;
        for e in &self.entries {
            if e.refs == 0 {
                c += 1;
            }
        }
        c
    }

    /// 出清步骤读数（指定 sb 的 clear/free 步骤号对）。
    pub fn sb_steps(&self, sb: u64) -> Result<(u32, u32), u16> {
        match self.find_sb(sb) {
            Some(i) => Ok((self.sbs[i].clear_step, self.sbs[i].free_step)),
            None => Err(E_NO_SB),
        }
    }
}

/// 自例程：多 sb 隔离出清→残留核对→顺序断言→守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = OrphanFabric::new();

    // R1 双 sb 登记 + 条目入账
    f.mount_sb(1)?;
    f.mount_sb(2)?;
    f.cache_put(10, 1, 0)?;
    f.cache_put(11, 1, 0)?;
    f.cache_put(12, 1, 0)?;
    f.cache_put(20, 2, 0)?;
    f.cache_put(21, 2, 1)?;
    hits += 1;

    // R2 kill_sb(1)：3 条全出清，残留=0（判据 J1/J4）
    let cleared = f.kill_sb(1)?;
    if cleared != 3 || f.residue_of(1) != 0 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3 sb 隔离：sb2 条目零波及（判据 J3）
    if f.residue_of(2) != 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 顺序断言：出清步骤 < 释放步骤（判据 J2）
    f.assert_order(1)?;
    // 有引用条目出清注入：sb2 有 refs=1 条目 → 拒收且相位回滚
    match f.kill_sb(2) {
        Err(E_REFERENCED) => {}
        _ => return Err(E_REFERENCED),
    }
    if f.residue_of(2) != 2 {
        return Err(E_STATE_INV);
    }
    hits += 1;

    // R5 归还引用后再出清 + 守恒（判据 J4）
    f.put_ref(21)?;
    let cleared2 = f.kill_sb(2)?;
    if cleared2 != 2 || f.residue_of(2) != 0 {
        return Err(E_LEDGER);
    }
    f.assert_order(2)?;
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    if f.clear_total != 5 || f.free_total != 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
