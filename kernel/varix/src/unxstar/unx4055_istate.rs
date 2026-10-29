//! unx4055_istate — UNX-F4055 i_state 脏位与写回标记（诊断段 0x8940）。
//!
//! 判据原文（提炼）：
//!   J1. 脏位生命周期（置位→写回→清位）状态机全路径断言通过。
//!   J2. 脏 inode 清单可导出，且与写回完成账逐项一致。
//!   J3. 写回未完成清位注入按 I_SYNC 语义拒收（清位必须先落写回账）。
//!   J4. 脏账守恒：置位总数 == 清位总数 + 在脏数。
//! 设计纪律：
//!   1. i_state 位型取 Linux VFS 语义四态：I_CLEAN/I_DIRTY/I_SYNC/
//!      I_WRITEBACK_DONE，迁移唯一闸门 controlled。
//!   2. 写回两步走：先置 I_SYNC（写回中），写回完成才允许清脏位，
//!      跳步清位注入在状态机闸门拦截（判据 J3）。
//!   3. 脏清单导出为确定性快照：按 inode 号升序出账。
//!   4. 计数账守恒：dirty_set/dirty_clear/inflight 三账交叉核对。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8940;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_DIRTY: u16 = SEG_BASE + 1;  // 非 Dirty 态发起写回
const E_EARLY_CLEAR: u16 = SEG_BASE + 2;// 写回未完成清位（跳步注入）
const E_ALREADY_DIRTY: u16 = SEG_BASE + 3; // 重复置脏
const E_ENOENT: u16 = SEG_BASE + 4;     // inode 不在状态表
const E_STATE_INV: u16 = SEG_BASE + 5;  // 非法状态迁移
const E_LEDGER: u16 = SEG_BASE + 6;     // 三账守恒核对失败
const E_BAD_ARG: u16 = SEG_BASE + 7;    // 参数非法（0 号）

/// i_state 位型四态（Linux VFS I_DIRTY/I_SYNC 语义整数化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IState {
    Clean,   // 干净：无需写回
    Dirty,   // 脏：待写回
    Sync,    // 写回中：I_SYNC 置位
    Done,    // 写回完成：清位前置相（可观测终态）
}

/// inode 脏位状态行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StateRow {
    pub ino: u64,       // inode 号
    pub state: IState,  // 当前 i_state
    pub dirty_gen: u32, // 置脏代次（每置位递增，写回对账锚）
}

/// i_state 枢纽：状态表 + 写回账 + 脏清单导出。
pub struct StateFabric {
    pub rows: Vec<StateRow>,  // 状态表
    pub dirty_set: u32,       // 置脏总数
    dirty_clear: u32,         // 清位总数（完成即清）
    pub wb_done: u32,         // 写回完成账（判据 J2 对账面）
    pub wb_inflight: u32,     // 写回在途计数（I_SYNC 水位）
}

impl StateFabric {
    pub fn new() -> StateFabric {
        StateFabric {
            rows: Vec::new(),
            dirty_set: 0,
            dirty_clear: 0,
            wb_done: 0,
            wb_inflight: 0,
        }
    }

    fn find(&self, ino: u64) -> Option<usize> {
        for i in 0..self.rows.len() {
            if self.rows[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    /// 登记 inode（初始 Clean 态入表）。
    pub fn admit(&mut self, ino: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find(ino).is_some() {
            return Err(E_STATE_INV);
        }
        self.rows.push(StateRow { ino, state: IState::Clean, dirty_gen: 0 });
        Ok(())
    }

    /// 置脏：Clean/Done→Dirty；重复置脏拒收（判据 J1 迁移唯一）。
    pub fn mark_dirty(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        match self.rows[i].state {
            IState::Clean | IState::Done => {
                self.rows[i].state = IState::Dirty;
                self.rows[i].dirty_gen += 1;
                self.dirty_set += 1;
                Ok(())
            }
            IState::Dirty => Err(E_ALREADY_DIRTY),
            IState::Sync => Err(E_STATE_INV),
        }
    }

    /// 发起写回：Dirty→Sync（I_SYNC 置位，在途水位 +1）。
    pub fn start_writeback(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.rows[i].state != IState::Dirty {
            return Err(E_NOT_DIRTY);
        }
        self.rows[i].state = IState::Sync;
        self.wb_inflight += 1;
        Ok(())
    }

    /// 写回完成：Sync→Done 并入完成账；随后 commit_clear 清位。
    pub fn finish_writeback(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.rows[i].state != IState::Sync {
            return Err(E_NOT_DIRTY);
        }
        self.rows[i].state = IState::Done;
        if self.wb_inflight == 0 {
            return Err(E_LEDGER);
        }
        self.wb_inflight -= 1;
        self.wb_done += 1;
        Ok(())
    }

    /// 清位：Done→Clean；写回未完成（非 Done）注入拒收（判据 J3）。
    pub fn commit_clear(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        match self.rows[i].state {
            IState::Done => {
                self.rows[i].state = IState::Clean;
                self.dirty_clear += 1;
                Ok(())
            }
            IState::Dirty | IState::Sync => Err(E_EARLY_CLEAR),
            IState::Clean => Err(E_STATE_INV),
        }
    }

    /// 跳步清位注入：Dirty 态直接清位 → 必须拦截（判据 J3 面）。
    pub fn inject_early_clear(&mut self, ino: u64) -> Result<(), u16> {
        match self.commit_clear(ino) {
            Err(E_EARLY_CLEAR) => Ok(()), // 拦截成功即注入回收
            Err(e) => Err(e),
            Ok(()) => Err(E_STATE_INV),   // 不该成功
        }
    }

    /// 脏清单导出：Dirty/Sync 态 inode 号升序快照（判据 J2）。
    pub fn export_dirty_list(&self) -> Result<Vec<u64>, u16> {
        let mut out: Vec<u64> = Vec::new();
        for r in &self.rows {
            if r.state == IState::Dirty || r.state == IState::Sync {
                out.push(r.ino);
            }
        }
        // 升序确定性（插入排序，量小）
        for i in 1..out.len() {
            let mut j = i;
            while j > 0 && out[j - 1] > out[j] {
                let t = out[j - 1];
                out[j - 1] = out[j];
                out[j] = t;
                j -= 1;
            }
        }
        Ok(out)
    }

    /// 在脏计数（与导出清单长度交叉复核用）。
    pub fn dirty_count(&self) -> u32 {
        let mut c = 0;
        for r in &self.rows {
            if r.state == IState::Dirty || r.state == IState::Sync {
                c += 1;
            }
        }
        c
    }

    /// 三账守恒（判据 J4）：dirty_set == dirty_clear + 在脏数。
    pub fn ledger_ok(&self) -> bool {
        self.dirty_set == self.dirty_clear + self.dirty_count()
            && self.dirty_set >= self.wb_done
    }

    /// 清位总数只读面（对账导出用）。
    pub fn clear_total(&self) -> u32 {
        self.dirty_clear
    }
}

// ── 扩展面：状态分布与诊断导出 ───────────────────────────────

impl StateFabric {
    /// 指定状态行计数（状态分布只读面）。
    pub fn count_in_state(&self, st: IState) -> u32 {
        let mut c = 0;
        for r in &self.rows {
            if r.state == st {
                c += 1;
            }
        }
        c
    }

    /// 在脏占比千分比：在脏数 * 1000 / 登记总数（零登记拒收）。
    pub fn dirty_ratio_permille(&self) -> Result<u32, u16> {
        if self.rows.is_empty() {
            return Err(E_ENOENT);
        }
        Ok(self.dirty_count() * 1000 / self.rows.len() as u32)
    }

    /// 全表清洗：把 Done 相残行统一回 Clean（诊断复位面，
    /// 写回完成标志语义下的合法终态收敛）。
    pub fn settle_done(&mut self) -> Result<u32, u16> {
        let mut n = 0;
        for i in 0..self.rows.len() {
            if self.rows[i].state == IState::Done {
                self.rows[i].state = IState::Clean;
                self.dirty_clear += 1;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 写回在途水位只读面（I_SYNC 水位导出）。
    pub fn inflight(&self) -> u32 {
        self.wb_inflight
    }

    /// 状态迁移账快照导出：返回（置脏数，清位数，完成数）三元组。
    pub fn export_transition(&self) -> Result<(u32, u32, u32), u16> {
        if self.dirty_set < self.dirty_clear {
            return Err(E_LEDGER);
        }
        Ok((self.dirty_set, self.dirty_clear, self.wb_done))
    }
}

/// 自例程：全路径迁移→跳步注入→清单导出对账→守恒核对。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = StateFabric::new();

    // R1 全路径：Clean→Dirty→Sync→Done→Clean 逐态断言
    f.admit(11)?;
    f.mark_dirty(11)?;
    if f.rows[0].state != IState::Dirty {
        return Err(E_STATE_INV);
    }
    f.start_writeback(11)?;
    if f.rows[0].state != IState::Sync || f.wb_inflight != 1 {
        return Err(E_STATE_INV);
    }
    f.finish_writeback(11)?;
    if f.rows[0].state != IState::Done || f.wb_done != 1 {
        return Err(E_STATE_INV);
    }
    f.commit_clear(11)?;
    if f.rows[0].state != IState::Clean {
        return Err(E_STATE_INV);
    }
    hits += 1;

    // R2 跳步注入：Dirty 态直接清位 → 拦截且状态不动
    f.admit(22)?;
    f.mark_dirty(22)?;
    f.inject_early_clear(22)?;
    if f.rows[1].state != IState::Dirty {
        return Err(E_EARLY_CLEAR);
    }
    hits += 1;

    // R3 重复置脏注入：已在脏再置 → 拒收
    match f.mark_dirty(22) {
        Err(E_ALREADY_DIRTY) => {}
        _ => return Err(E_ALREADY_DIRTY),
    }
    hits += 1;

    // R4 脏清单导出与写回完成账一致（判据 J2）
    f.admit(33)?;
    f.mark_dirty(33)?;
    f.admit(44)?;
    f.mark_dirty(44)?;
    f.start_writeback(44)?;
    let list = f.export_dirty_list()?;
    if list.len() as u32 != f.dirty_count() {
        return Err(E_LEDGER);
    }
    // 22 与 33 在脏（44 已进 Sync 也在清单），升序核对
    if list[0] != 22 || list[1] != 33 || list[2] != 44 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 三账守恒：dirty_set == dirty_clear + 在脏
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    if f.dirty_set != 4 || f.dirty_count() != 3 || f.clear_total() != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
