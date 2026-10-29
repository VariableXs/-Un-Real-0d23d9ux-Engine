//! unx4067_dirtykeep — UNX-F4067 脏 inode 不淘汰不变式（诊断段 0x8AA0）。
//!
//! 判据原文（提炼）：
//!   J1. 脏位存活期间淘汰注入 1,000 次零命中。
//!   J2. 脏位清零后立即回到可淘汰集（状态迁移账可见）。
//!   J3. 不变式断言器：脏候选一律拒绝，净候选放行。
//!   J4. 迁移账守恒：入列数 == 出列数 + 在列数（按脏/净分流）。
//! 设计纪律：
//!   1. Linux VFS 语义：脏 inode 挂脏链不挂 LRU 可淘汰集，
//!      写回清脏后方可被淘汰候选受理。
//!   2. 淘汰闸门双检：refs==0 与 !dirty 双条件，脏候选注入
//!      一律拒收并入拦截账（判据 J1/J3）。
//!   3. 状态机：CleanEligible ↔ DirtyKept 双相，迁移记账可见。
//!   4. 千次注入用确定性序列表达（无随机源）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8AA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DIRTY_KEEP: u16 = SEG_BASE + 1; // 脏候选淘汰注入被拒（判据 J1）
const E_REFERENCED: u16 = SEG_BASE + 2; // 引用非零候选拒收
const E_ENOENT: u16 = SEG_BASE + 3;     // inode 未登记
const E_STATE_INV: u16 = SEG_BASE + 4;  // 相位迁移非法
const E_LEDGER: u16 = SEG_BASE + 5;     // 迁移账守恒破坏
const E_NOT_ELIGIBLE: u16 = SEG_BASE + 6; // 净候选未入可淘汰集
const E_BAD_ARG: u16 = SEG_BASE + 7;    // 参数非法（0 号）

/// 双相状态机：可淘汰 / 脏保留。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeepPhase {
    CleanEligible, // 干净且可淘汰
    DirtyKept,     // 脏保留（不淘汰不变式作用中）
}

/// 候选行：引用 + 脏位 + 相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeepRow {
    pub ino: u64,     // inode 号
    pub refs: u32,    // 引用计数
    pub dirty: bool,  // 脏位
    pub phase: KeepPhase,
}

/// 不变式枢纽：候选表 + 淘汰拦截账 + 迁移账。
pub struct DirtyKeepFabric {
    pub rows: Vec<KeepRow>,     // 候选表
    pub eligible: Vec<u64>,     // 可淘汰集（净且零引用）
    pub keep_block_total: u32,  // 脏候选淘汰拦截数（判据 J1 面）
    pub ref_block_total: u32,   // 引用非零拦截数
    pub to_dirty_total: u32,    // 净→脏迁移数
    pub to_clean_total: u32,    // 脏→净迁移数（判据 J2 可见性）
    pub evict_total: u32,       // 实际淘汰数
}

impl DirtyKeepFabric {
    pub fn new() -> DirtyKeepFabric {
        DirtyKeepFabric {
            rows: Vec::new(),
            eligible: Vec::new(),
            keep_block_total: 0,
            ref_block_total: 0,
            to_dirty_total: 0,
            to_clean_total: 0,
            evict_total: 0,
        }
    }

    fn find_row(&self, ino: u64) -> Option<usize> {
        for i in 0..self.rows.len() {
            if self.rows[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    fn in_eligible(&self, ino: u64) -> bool {
        for &x in &self.eligible {
            if x == ino {
                return true;
            }
        }
        false
    }

    /// 登记候选（refs=1 初持，CleanEligible 相但未入可淘汰集）。
    pub fn admit(&mut self, ino: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_row(ino).is_some() {
            return Err(E_BAD_ARG);
        }
        self.rows.push(KeepRow {
            ino,
            refs: 1,
            dirty: false,
            phase: KeepPhase::CleanEligible,
        });
        Ok(())
    }

    /// 置脏：CleanEligible→DirtyKept（迁移记账，判据 J2 可见性）。
    pub fn mark_dirty(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_row(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.rows[i].phase != KeepPhase::CleanEligible {
            return Err(E_STATE_INV);
        }
        self.rows[i].dirty = true;
        self.rows[i].phase = KeepPhase::DirtyKept;
        // 若在可淘汰集则同步移出（脏不淘汰不变式）
        let mut k = 0;
        while k < self.eligible.len() {
            if self.eligible[k] == ino {
                self.eligible.remove(k);
            } else {
                k += 1;
            }
        }
        self.to_dirty_total += 1;
        Ok(())
    }

    /// 清脏：DirtyKept→CleanEligible；零引用则立即回可淘汰集（判据 J2）。
    pub fn clean(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_row(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.rows[i].phase != KeepPhase::DirtyKept {
            return Err(E_STATE_INV);
        }
        self.rows[i].dirty = false;
        self.rows[i].phase = KeepPhase::CleanEligible;
        self.to_clean_total += 1;
        if self.rows[i].refs == 0 && !self.in_eligible(ino) {
            self.eligible.push(ino);
        }
        Ok(())
    }

    /// 释放引用：refs 归零，净者入可淘汰集。
    pub fn put_unused(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_row(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.rows[i].refs == 0 {
            self.ref_block_total += 1;
            return Err(E_REFERENCED);
        }
        self.rows[i].refs -= 1;
        if self.rows[i].refs == 0 && !self.rows[i].dirty && !self.in_eligible(ino) {
            self.eligible.push(ino);
        }
        Ok(())
    }

    /// 淘汰尝试：脏候选一律拒收（不变式，判据 J1/J3）。
    pub fn try_evict(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_row(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.rows[i].dirty || self.rows[i].phase == KeepPhase::DirtyKept {
            self.keep_block_total += 1;
            return Err(E_DIRTY_KEEP);
        }
        if self.rows[i].refs != 0 {
            self.ref_block_total += 1;
            return Err(E_REFERENCED);
        }
        // 净候选：必须在可淘汰集内才可淘汰
        if !self.in_eligible(ino) {
            return Err(E_NOT_ELIGIBLE);
        }
        let mut k = 0;
        while k < self.eligible.len() {
            if self.eligible[k] == ino {
                self.eligible.remove(k);
                break;
            }
            k += 1;
        }
        self.rows.remove(i);
        self.evict_total += 1;
        Ok(())
    }

    /// 千次脏候选淘汰注入（判据 J1：零命中拦截）。
    pub fn inject_thousand(&mut self, ino: u64) -> Result<u32, u16> {
        let mut blocked = 0;
        for _ in 0..1000u32 {
            match self.try_evict(ino) {
                Err(E_DIRTY_KEEP) => blocked += 1,
                Ok(()) => return Err(E_DIRTY_KEEP), // 不变式失守
                Err(e) => return Err(e),
            }
        }
        Ok(blocked)
    }

    /// 可淘汰集深度。
    pub fn eligible_len(&self) -> u32 {
        self.eligible.len() as u32
    }

    /// 迁移账守恒（判据 J4）：可淘汰集 + 淘汰数 + 脏保留数 == 累计入列。
    pub fn ledger_ok(&self) -> bool {
        // 在集行必须净且零引用
        for &ino in &self.eligible {
            match self.find_row(ino) {
                Some(i) => {
                    if self.rows[i].dirty || self.rows[i].refs != 0 {
                        return false;
                    }
                }
                None => return false,
            }
        }
        // 脏→净迁移数 == 净→脏迁移数 - 当前在脏数
        let mut dirty_now = 0;
        for r in &self.rows {
            if r.dirty {
                dirty_now += 1;
            }
        }
        self.to_clean_total + dirty_now == self.to_dirty_total
    }
}

// ── 扩展面：相位分布与拦截账导出 ─────────────────────────────

impl DirtyKeepFabric {
    /// 脏保留相计数（不变式作用中条目数，判据 J1 观测面）。
    pub fn kept_count(&self) -> u32 {
        let mut c = 0;
        for r in &self.rows {
            if r.phase == KeepPhase::DirtyKept {
                c += 1;
            }
        }
        c
    }

    /// 拦截双账快照：返回（脏候选拦截数，引用拦截数）（判据 J1/J3 面）。
    pub fn block_ledger(&self) -> (u32, u32) {
        (self.keep_block_total, self.ref_block_total)
    }

    /// 迁移账快照：返回（净→脏数，脏→净数）（判据 J2 可见性面）。
    pub fn transition_ledger(&self) -> (u32, u32) {
        (self.to_dirty_total, self.to_clean_total)
    }

    /// 实际淘汰数只读面。
    pub fn evict_count(&self) -> u32 {
        self.evict_total
    }

    /// 单行相位只读面（逐条目观测）。
    pub fn phase_of(&self, ino: u64) -> Result<KeepPhase, u16> {
        match self.find_row(ino) {
            Some(i) => Ok(self.rows[i].phase),
            None => Err(E_ENOENT),
        }
    }
}

/// 自例程：脏保留注入千次→清脏回归→迁移账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = DirtyKeepFabric::new();

    // R1 净候选入可淘汰集
    f.admit(21)?;
    f.put_unused(21)?;
    if f.eligible_len() != 1 {
        return Err(E_NOT_ELIGIBLE);
    }
    hits += 1;

    // R2 置脏：立即移出可淘汰集（迁移账 +1）
    f.mark_dirty(21)?;
    if f.eligible_len() != 0 || f.to_dirty_total != 1 {
        return Err(E_STATE_INV);
    }
    hits += 1;

    // R3 千次脏候选淘汰注入零命中（判据 J1）
    let blocked = f.inject_thousand(21)?;
    if blocked != 1000 || f.keep_block_total != 1000 {
        return Err(E_DIRTY_KEEP);
    }
    hits += 1;

    // R4 清脏：立即回到可淘汰集（判据 J2，迁移账可见）
    f.clean(21)?;
    if f.eligible_len() != 1 || f.to_clean_total != 1 {
        return Err(E_STATE_INV);
    }
    hits += 1;

    // R5 净候选正常淘汰 + 迁移账守恒（判据 J4）
    f.try_evict(21)?;
    if f.evict_total != 1 || f.eligible_len() != 0 {
        return Err(E_NOT_ELIGIBLE);
    }
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
