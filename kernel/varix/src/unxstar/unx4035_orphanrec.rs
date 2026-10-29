//! unx4035_orphanrec — UNX-F4035 死亡 sb 的孤儿 inode 回收语义（诊断段 0x86E0）。
//!
//! 判据原文：
//!   J1 kill 时仍被引用的 inode 进入孤儿账；
//!   J2 引用归零后回收（回收计数 = 孤儿计数）；
//!   J3 孤儿滞留注入被周期断言检出。
//! 设计纪律：
//!   1. 孤儿账模型：sb 死亡（kill）时扫描 inode 池，凡引用计数
//!      仍 > 0 的 inode 逐个迁入孤儿账（orphan 列表）。
//!   2. 回收语义：孤儿条目在引用归零（最后一次 put）时从孤儿账
//!      摘除并计入回收账——回收计数最终必须等于孤儿入账计数。
//!   3. 滞留注入：周期断言器以「孤儿账龄超阈值」检出滞留孤儿
//!      并报 E_ORPHAN_STALL（判据 J3 注入面）。
//!   4. 状态机：InUse（在池在用）→ Orphaned（sb 死后入孤儿账）
//!      → Reclaimed（引用归零已回收），单向迁移；Reclaimed 终态。
//!   5. 边界：二次 kill、put 已回收条目、对未孤儿化条目走孤儿
//!      回收路径、越界句柄——全部显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x86E0;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_ORPHAN_FULL: u16 = SEG_BASE + 1;  // 孤儿账满（静态配额）
const E_PUT_LIVE: u16 = SEG_BASE + 2;     // 强制回收时引用仍 > 1
const E_NOT_ORPHAN: u16 = SEG_BASE + 3;   // 对未孤儿化条目走孤儿回收路径
const E_RECLAIMED: u16 = SEG_BASE + 4;    // 已回收条目再操作（终态违例）
const E_ORPHAN_STALL: u16 = SEG_BASE + 5; // 孤儿滞留（账龄超阈值）
const E_BAL: u16 = SEG_BASE + 6;          // 孤儿/回收账不平（含二次 kill）
const E_BAD_SLOT: u16 = SEG_BASE + 7;     // 越界条目句柄

/// 孤儿账静态容量。
pub const ORPHAN_CAP: usize = 16;
/// 滞留阈值：账龄超过此扫描代数判定滞留。
pub const STALL_GEN: u32 = 3;

/// 孤儿条目状态机：单向迁移，Reclaimed 终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrphanPhase {
    InUse,     // 在池在用
    Orphaned,  // sb 死亡后入孤儿账（引用未归零）
    Reclaimed, // 引用归零已回收
}

/// 单个受管 inode：引用计数 + 阶段 + 账龄。
pub struct TrackedIno {
    pub i_ino: u64,       // inode 号
    pub refcnt: u32,      // 引用计数
    pub phase: OrphanPhase,
    pub orphan_gen: u32,  // 入孤儿账时的扫描代数（滞留判定用）
}

/// 孤儿回收账枢纽。
pub struct OrphanLedger {
    pub inos: Vec<TrackedIno>,
    pub orphan_list: Vec<usize>, // 孤儿账（池下标列表）
    pub sb_alive: bool,          // sb 生存位
    pub scan_gen: u32,           // 周期断言扫描代数
    pub orphan_total: u32,       // 孤儿入账总数（判据 J1 账面）
    pub reclaim_total: u32,      // 回收总数（判据 J2 账面）
    pub rejects: u32,            // 错误账
}

impl OrphanLedger {
    pub fn new() -> OrphanLedger {
        OrphanLedger {
            inos: Vec::new(),
            orphan_list: Vec::new(),
            sb_alive: true,
            scan_gen: 0,
            orphan_total: 0,
            reclaim_total: 0,
            rejects: 0,
        }
    }

    /// 建 inode（InUse，引用计数 1 起步）。
    pub fn create_ino(&mut self, i_ino: u64) -> Result<usize, u16> {
        self.inos.push(TrackedIno {
            i_ino,
            refcnt: 1,
            phase: OrphanPhase::InUse,
            orphan_gen: 0,
        });
        Ok(self.inos.len() - 1)
    }

    /// 引用递增（仅 InUse 期）。
    pub fn get_ref(&mut self, idx: usize) -> Result<u32, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_BAD_SLOT);
        }
        if self.inos[idx].phase != OrphanPhase::InUse {
            self.rejects += 1;
            return Err(E_RECLAIMED);
        }
        self.inos[idx].refcnt += 1;
        Ok(self.inos[idx].refcnt)
    }

    /// 孤儿账摘除辅助：删除指定下标的首个账项。
    fn drop_from_orphan(&mut self, idx: usize) {
        let mut j = 0;
        while j < self.orphan_list.len() {
            if self.orphan_list[j] == idx {
                self.orphan_list.remove(j);
                return;
            }
            j += 1;
        }
    }

    /// 引用递减：归零时——sb 存活则池内回收；sb 已死则孤儿摘除回收。
    pub fn put_ref(&mut self, idx: usize) -> Result<OrphanPhase, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_BAD_SLOT);
        }
        if self.inos[idx].phase == OrphanPhase::Reclaimed {
            self.rejects += 1;
            return Err(E_RECLAIMED);
        }
        if self.inos[idx].refcnt == 0 {
            self.rejects += 1;
            return Err(E_PUT_LIVE);
        }
        self.inos[idx].refcnt -= 1;
        if self.inos[idx].refcnt > 0 {
            // 判据 J2 反向：引用未归零，不回收。
            return Ok(self.inos[idx].phase);
        }
        if self.sb_alive {
            self.inos[idx].phase = OrphanPhase::Reclaimed;
            return Ok(OrphanPhase::Reclaimed);
        }
        // sb 已死：必须经孤儿账摘除路径回收（判据 J2）。
        if self.inos[idx].phase != OrphanPhase::Orphaned {
            self.rejects += 1;
            return Err(E_NOT_ORPHAN);
        }
        self.inos[idx].phase = OrphanPhase::Reclaimed;
        self.reclaim_total += 1;
        self.drop_from_orphan(idx);
        Ok(OrphanPhase::Reclaimed)
    }

    /// sb kill：扫描池，仍被引用者入孤儿账，引用归零者直回收。
    pub fn kill_sb(&mut self) -> Result<u32, u16> {
        if !self.sb_alive {
            self.rejects += 1;
            return Err(E_BAL);
        }
        self.sb_alive = false;
        let mut orphans = 0u32;
        let mut i = 0;
        while i < self.inos.len() {
            if i < self.inos.len() && self.inos[i].phase == OrphanPhase::InUse {
                if self.inos[i].refcnt > 0 {
                    // 判据 J1：仍被引用 → 入孤儿账。
                    if self.orphan_list.len() >= ORPHAN_CAP {
                        self.rejects += 1;
                        return Err(E_ORPHAN_FULL);
                    }
                    self.inos[i].phase = OrphanPhase::Orphaned;
                    self.inos[i].orphan_gen = self.scan_gen;
                    self.orphan_list.push(i);
                    self.orphan_total += 1;
                    orphans += 1;
                } else {
                    // 无引用：直回收。
                    self.inos[i].phase = OrphanPhase::Reclaimed;
                }
            }
            i += 1;
        }
        Ok(orphans)
    }

    /// 孤儿强制回收路径：引用必须已归零至 1（持有者最后一手）。
    pub fn reclaim_orphan(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_BAD_SLOT);
        }
        if self.inos[idx].phase != OrphanPhase::Orphaned {
            self.rejects += 1;
            return Err(E_NOT_ORPHAN);
        }
        if self.inos[idx].refcnt > 1 {
            self.inos[idx].refcnt -= 1;
            return Err(E_PUT_LIVE);
        }
        self.inos[idx].refcnt = 0;
        self.inos[idx].phase = OrphanPhase::Reclaimed;
        self.reclaim_total += 1;
        self.drop_from_orphan(idx);
        Ok(())
    }

    /// 周期断言：孤儿账龄超阈值判滞留（判据 J3）。
    pub fn periodic_assert(&mut self) -> Result<u32, u16> {
        self.scan_gen += 1;
        let mut stalled = 0u32;
        let mut j = 0;
        while j < self.orphan_list.len() {
            let idx = self.orphan_list[j];
            if idx >= self.inos.len() {
                return Err(E_BAD_SLOT);
            }
            if self.scan_gen.saturating_sub(self.inos[idx].orphan_gen) > STALL_GEN {
                stalled += 1;
            }
            j += 1;
        }
        if stalled > 0 {
            self.rejects += 1;
            return Err(E_ORPHAN_STALL);
        }
        Ok(stalled)
    }

    /// 守恒对账：孤儿账长与 Orphaned 条目数一致；
    /// 死 sb 且孤儿清空时回收计数必须追平孤儿入账（判据 J2 终局）。
    pub fn audit(&self) -> Result<(), u16> {
        let mut orphaned = 0u32;
        let mut i = 0;
        while i < self.inos.len() {
            if i < self.inos.len() && self.inos[i].phase == OrphanPhase::Orphaned {
                orphaned += 1;
            }
            i += 1;
        }
        if orphaned as usize != self.orphan_list.len() {
            return Err(E_BAL);
        }
        if !self.sb_alive && self.orphan_list.is_empty() {
            if self.reclaim_total != self.orphan_total {
                return Err(E_BAL);
            }
        }
        Ok(())
    }

    /// 导出快照（孤儿账长，孤儿入账，回收账，扫描代数）。
    pub fn ledger(&self) -> (usize, u32, u32, u32) {
        (self.orphan_list.len(), self.orphan_total, self.reclaim_total, self.scan_gen)
    }
}

/// 自例程：R1 死亡扫描 → R2 孤儿回收 → R3 滞留检出 → R4 注入拒收 → R5 守恒终局。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ol = OrphanLedger::new();

    // R1 死亡扫描（判据 J1）：a 引用 2、b 引用 1，kill 后双双入孤儿账。
    let a = ol.create_ino(10)?;
    let b = ol.create_ino(11)?;
    ol.get_ref(a)?;
    let orphans = ol.kill_sb()?;
    if orphans != 2 || ol.orphan_list.len() != 2 || ol.orphan_total != 2 {
        return Err(E_BAL);
    }
    if ol.inos[a].phase != OrphanPhase::Orphaned
        || ol.inos[b].phase != OrphanPhase::Orphaned
    {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 孤儿回收（判据 J2）：b 引用归零走孤儿路径回收；a 经强制路径回收。
    if ol.put_ref(b)? != OrphanPhase::Reclaimed {
        return Err(E_BAL);
    }
    if ol.orphan_list.len() != 1 || ol.reclaim_total != 1 {
        return Err(E_BAL);
    }
    if ol.put_ref(a)? != OrphanPhase::Orphaned {
        // a 引用 2→1，未归零，仍在孤儿账。
        return Err(E_BAL);
    }
    ol.reclaim_orphan(a)?;
    if ol.orphan_list.len() != 0 || ol.reclaim_total != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 滞留检出（判据 J3）：滞留孤儿账龄超过 3 代被周期断言捕获。
    let mut ol2 = OrphanLedger::new();
    let c = ol2.create_ino(20)?;
    ol2.kill_sb()?;
    ol2.periodic_assert()?; // 代 1：未滞留
    ol2.periodic_assert()?; // 代 2：未滞留
    ol2.periodic_assert()?; // 代 3：未滞留（恰在阈值内）
    if ol2.periodic_assert() != Err(E_ORPHAN_STALL) {
        // 代 4：账龄 4 > 阈值 3，滞留注入被检出。
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注入拒收：二次 kill、已回收再 put、非孤儿走孤儿路径、越界句柄。
    if ol.kill_sb() != Err(E_BAL) {
        return Err(E_BAL);
    }
    if ol.put_ref(b) != Err(E_RECLAIMED) {
        return Err(E_BAL);
    }
    if ol.reclaim_orphan(b) != Err(E_NOT_ORPHAN) {
        return Err(E_BAL);
    }
    if ol.put_ref(0xFFFF) != Err(E_BAD_SLOT) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒终局：回收计数 = 孤儿入账计数，两账闭合。
    ol.audit()?;
    ol2.audit()?;
    let (ol_len, ot, rt, _g) = ol.ledger();
    if ol_len != 0 || ot != 2 || rt != 2 || ot != rt {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
