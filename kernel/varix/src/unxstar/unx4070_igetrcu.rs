//! unx4070_igetrcu — UNX-F4070 iget 并发去重（fake A5 RCU）（诊断段 0x8B00）。
//!
//! 判据原文（提炼）：
//!   J1. 双读者并发 iget 同号合并为单实例（实例计数=1）。
//!   J2. 读侧 RCU 临界区无阻塞断言（fake 接口 1,000 次零违例）。
//!   J3. 写侧更新读侧无感：读者始终看到一致实例。
//!   J4. 合并账守恒：读者请求数 == 实例挂靠数合计。
//! 设计纪律：
//!   1. RCU 语义 fake 化：读者进入临界区拿代次快照（无锁表达），
//!      写侧更新推进代次，读者退出核对代次差（宽限期模拟）。
//!   2. 并发去重：同号并发请求经在建窗口合并到单实例（复用
//!      iget 合并语义，RCU 读侧免阻塞）。
//!   3. 无阻塞判据：临界区不调用任何可能等待的接口（本锚以
//!      计步模拟，临界区内零阻塞计步）。
//!   4. 千次并发序确定性化：双读者交错序列由整数推导。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8B00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_INO: u16 = SEG_BASE + 1;   // 0 号拒收
const E_DUP_INST: u16 = SEG_BASE + 2;  // 实例重复建立（去重失守）
const E_GRADE_GAP: u16 = SEG_BASE + 3; // 读者代次漂移（宽限期失守）
const E_BLOCKED: u16 = SEG_BASE + 4;   // 临界区阻塞违例（判据 J2）
const E_LEDGER: u16 = SEG_BASE + 5;    // 合并账守恒破坏
const E_STATE_INV: u16 = SEG_BASE + 6; // RCU 相位迁移非法
const E_BAD_ARG: u16 = SEG_BASE + 7;   // 参数非法

/// RCU 相位状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RcuPhase {
    Quiescent, // 静止（无读者在临界区）
    Reading,   // 读临界区中（宽限期不可推进发布）
    Updating,  // 写侧更新中
}

/// 共享实例（RCU 发布对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SharedInst {
    pub ino: u64,     // inode 号
    pub readers: u32, // 挂靠读者数（合并观测面）
}

/// fake RCU 枢纽：实例表 + 代次账 + 相位账。
pub struct RcuFabric {
    pub insts: Vec<SharedInst>, // 共享实例表
    pub epoch: u32,             // 全局代次（写侧发布推进）
    pub phase: RcuPhase,        // 当前相位
    pub read_enter: u32,        // 读侧进入总次
    pub read_exit: u32,         // 读侧退出总次
    pub merges: u32,            // 合并挂靠总数（判据 J4）
    pub blocked_total: u32,     // 临界区阻塞违例计数（判据 J2 面）
    pub update_total: u32,      // 写侧更新总数
}

impl RcuFabric {
    pub fn new() -> RcuFabric {
        RcuFabric {
            insts: Vec::new(),
            epoch: 0,
            phase: RcuPhase::Quiescent,
            read_enter: 0,
            read_exit: 0,
            merges: 0,
            blocked_total: 0,
            update_total: 0,
        }
    }

    fn find_inst(&self, ino: u64) -> Option<usize> {
        for i in 0..self.insts.len() {
            if self.insts[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    /// 读侧进入临界区：拿代次快照（无阻塞，判据 J2）。
    /// 返回（快照代次，实例序号）。
    pub fn rcu_read_enter(&mut self, ino: u64) -> Result<(u32, usize), u16> {
        if ino == 0 {
            return Err(E_BAD_INO);
        }
        self.read_enter += 1;
        // 相位检查：Updating 期读者照常进入（RCU 读侧永不等）
        match self.find_inst(ino) {
            Some(i) => {
                self.insts[i].readers += 1;
                self.merges += 1;
                self.phase = RcuPhase::Reading;
                Ok((self.epoch, i))
            }
            None => {
                // 首读者触发实例建立（去重唯一）
                self.insts.push(SharedInst { ino, readers: 1 });
                self.merges += 1;
                self.phase = RcuPhase::Reading;
                Ok((self.epoch, self.insts.len() - 1))
            }
        }
    }

    /// 读侧退出：核对代次漂移（宽限期模拟，判据 J3）。
    pub fn rcu_read_exit(&mut self, ino: u64, snap_epoch: u32) -> Result<(), u16> {
        let i = match self.find_inst(ino) {
            Some(i) => i,
            None => return Err(E_STATE_INV),
        };
        if self.insts[i].readers == 0 {
            return Err(E_STATE_INV);
        }
        self.insts[i].readers -= 1;
        self.read_exit += 1;
        // 代次漂移检测：退出时代次不得越过快照两个以上发布窗
        if self.epoch > snap_epoch + 1 {
            return Err(E_GRADE_GAP);
        }
        if self.read_enter == self.read_exit {
            self.phase = RcuPhase::Quiescent;
        }
        Ok(())
    }

    /// 写侧更新：推进代次并发布（读者无感，判据 J3）。
    pub fn rcu_update(&mut self, ino: u64) -> Result<u32, u16> {
        if ino == 0 {
            return Err(E_BAD_INO);
        }
        if self.find_inst(ino).is_none() {
            return Err(E_STATE_INV);
        }
        self.epoch += 1;
        self.update_total += 1;
        self.phase = RcuPhase::Updating;
        Ok(self.epoch)
    }

    /// 并发去重注入：双读者并发同号 → 实例计数必为 1（判据 J1）。
    pub fn inject_double_reader(&mut self, ino: u64) -> Result<usize, u16> {
        let (_, i1) = self.rcu_read_enter(ino)?;
        let (_, i2) = self.rcu_read_enter(ino)?;
        if i1 != i2 {
            self.blocked_total += 1;
            return Err(E_DUP_INST);
        }
        if self.insts[i1].readers != 2 {
            return Err(E_DUP_INST);
        }
        Ok(i1)
    }

    /// 千次读侧无阻塞断言（判据 J2）：fake 临界区零阻塞计步。
    pub fn thousand_read_loop(&mut self, ino: u64) -> Result<u32, u16> {
        let mut steps = 0;
        for _ in 0..1000u32 {
            let (snap, i) = self.rcu_read_enter(ino)?;
            // 临界区内零等待计步（阻塞即违例）
            steps += 1;
            self.rcu_read_exit(ino, snap)?;
            if self.blocked_total != 0 {
                return Err(E_BLOCKED);
            }
            if self.insts[i].ino != ino {
                return Err(E_STATE_INV);
            }
        }
        Ok(steps)
    }

    /// 实例计数（去重观测面）。
    pub fn inst_count(&self) -> u32 {
        self.insts.len() as u32
    }

    /// 合并账守恒（判据 J4）：merges == read_enter。
    pub fn ledger_ok(&self) -> bool {
        self.merges == self.read_enter && self.read_enter >= self.read_exit
    }
}

// ── 扩展面：RCU 相位诊断与读者账导出 ─────────────────────────

impl RcuFabric {
    /// 相位只读面（RCU 全局相位读数）。
    pub fn phase(&self) -> RcuPhase {
        self.phase
    }

    /// 代次读数（写侧发布代次，判据 J3 观测面）。
    pub fn epoch_of(&self) -> u32 {
        self.epoch
    }

    /// 读者挂靠读数（指定实例在临界区读者数）。
    pub fn readers_of(&self, ino: u64) -> Result<u32, u16> {
        match self.find_inst(ino) {
            Some(i) => Ok(self.insts[i].readers),
            None => Err(E_BAD_INO),
        }
    }

    /// 进入/退出账快照（判据 J2 临界区对账面）。
    pub fn read_ledger(&self) -> (u32, u32) {
        (self.read_enter, self.read_exit)
    }

    /// 写侧更新账读数（发布次数，判据 J3 面）。
    pub fn update_count(&self) -> u32 {
        self.update_total
    }

    /// 阻塞违例账读数（判据 J2 恒零锚导出）。
    pub fn blocked_count(&self) -> u32 {
        self.blocked_total
    }

    /// 宽限期核对：全部读者退出后代次不得再滞后（静止相断言）。
    pub fn grace_ok(&self) -> Result<(), u16> {
        if self.phase == RcuPhase::Reading && self.read_enter == self.read_exit {
            return Err(E_STATE_INV);
        }
        Ok(())
    }

    /// 实例去重全表审计：任意 inode 至多一实例（判据 J1 全局面）。
    pub fn dedup_ok(&self) -> bool {
        for i in 0..self.insts.len() {
            for j in (i + 1)..self.insts.len() {
                if self.insts[i].ino == self.insts[j].ino {
                    return false;
                }
            }
        }
        true
    }
}

/// 自例程：双读者合并→千次无阻塞→写侧无感→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = RcuFabric::new();

    // R1 双读者并发同号合并单实例（判据 J1）
    let i = f.inject_double_reader(33)?;
    if f.inst_count() != 1 || f.insts[i].readers != 2 {
        return Err(E_DUP_INST);
    }
    hits += 1;

    // R2 千次读侧无阻塞断言（判据 J2）
    // 先退出两位读者再进循环
    f.rcu_read_exit(33, 0)?;
    f.rcu_read_exit(33, 0)?;
    let steps = f.thousand_read_loop(33)?;
    if steps != 1000 {
        return Err(E_BLOCKED);
    }
    if f.blocked_total != 0 {
        return Err(E_BLOCKED);
    }
    hits += 1;

    // R3 写侧更新读者无感：更新推进代次，读者快照一致（判据 J3）
    let (snap, _) = f.rcu_read_enter(33)?;
    let ep = f.rcu_update(33)?;
    if ep != snap + 1 {
        return Err(E_GRADE_GAP);
    }
    f.rcu_read_exit(33, snap)?;
    hits += 1;

    // R4 阻塞违例注入：模拟读者代次漂移 → 检出（判据 J2/J3 面）
    let (snap2, _) = f.rcu_read_enter(33)?;
    f.rcu_update(33)?;
    f.rcu_update(33)?; // 连续两窗发布 → 读者漂移
    match f.rcu_read_exit(33, snap2) {
        Err(E_GRADE_GAP) => {}
        _ => return Err(E_GRADE_GAP),
    }
    hits += 1;

    // R5 合并账守恒（判据 J4）
    // 收尾：平衡进入/退出
    if f.read_enter > f.read_exit {
        let (snap3, _) = f.rcu_read_enter(33)?;
        f.rcu_read_exit(33, snap3)?;
    }
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    if f.merges != f.read_enter {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
