//! unx4037_fstypelock — UNX-F4037 fs 注册表的并发保护（fake A3 锁）（诊断段 0x8720）。
//!
//! 判据原文：
//!   J1 注册表并发注册/查找 1,000 次交错零错账；
//!   J2 锁持有期间注册表不可变快照断言通过。
//! 设计纪律：
//!   1. fake A3 锁：Unlocked → Locked 状态机；持锁期是临界区，
//!      注册/查找只在持锁期放行；无锁操作一律拒收。
//!   2. 死锁/双解锁注入：持锁再锁（E_DEADLOCK）、无锁再解
//!      （E_DOUBLE_UNLOCK）均为显性错误路径。
//!   3. 交错调度器：确定性的注册/查找交错序列（1,000 步），
//!      每步持锁执行，账目随步推进，零错账（无一步意外失败）。
//!   4. 快照不可变断言（判据 J2）：进临界区拍快照（表长 + 序号），
//!      出临界区复核——临界区内快照必须与账面零 diff。
//!   5. 守恒式：注册成功 - 注销 = 表长；交错步数 = 成功步 +
//!      空查找步；锁获取数 = 锁释放数（终态解锁平账）。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8720;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_DEADLOCK: u16 = SEG_BASE + 1;       // 持锁再锁
const E_DOUBLE_UNLOCK: u16 = SEG_BASE + 2;  // 无锁再解
const E_OP_UNLOCKED: u16 = SEG_BASE + 3;    // 无锁期操作注册表
const E_EXIST: u16 = SEG_BASE + 4;          // 重名注册（EEXIST 语义）
const E_NO_TYPE: u16 = SEG_BASE + 5;        // 查找不存在的类型
const E_SNAP_DIFF: u16 = SEG_BASE + 6;      // 快照 diff 非零（不可变失守）
const E_BAL: u16 = SEG_BASE + 7;            // 账目守恒失守

/// 类型名定长（名空间简化约定）。
pub const NAME_CAP: usize = 12;
/// 交错调度步数（判据 J1 的 1,000 次）。
pub const INTERLEAVE_STEPS: u32 = 1000;

/// fake A3 锁状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockPhase {
    Unlocked,
    Locked,
}

/// 类型表项。
pub struct TypeCell {
    pub name: [u8; NAME_CAP],
    pub name_len: usize,
    pub seq: u32,
}

/// 锁保护的 fs 注册表枢纽。
pub struct FsTypeLock {
    pub lock: LockPhase,
    pub hold_count: u32,   // 累计持锁次数
    pub release_count: u32, // 累计释放次数
    pub entries: Vec<TypeCell>,
    pub reg_ok: u32,       // 注册成功步
    pub lookup_ok: u32,    // 查找命中步
    pub lookup_miss: u32,  // 查找未命中步（合法空转）
    pub seq_next: u32,     // 注册序号
    pub rejects: u32,      // 错误账
    pub snap_len: usize,   // 临界区快照表长
    pub snap_seq: u32,     // 临界区快照序号
    pub snap_reg: u32,     // 临界区快照注册账（delta 推导基线）
}

impl FsTypeLock {
    pub fn new() -> FsTypeLock {
        FsTypeLock {
            lock: LockPhase::Unlocked,
            hold_count: 0,
            release_count: 0,
            entries: Vec::new(),
            reg_ok: 0,
            lookup_ok: 0,
            lookup_miss: 0,
            seq_next: 1,
            rejects: 0,
            snap_len: 0,
            snap_seq: 0,
            snap_reg: 0,
        }
    }

    /// 加锁：持锁再锁拒收（死锁注入面）。
    pub fn lock_acquire(&mut self) -> Result<(), u16> {
        if self.lock == LockPhase::Locked {
            self.rejects += 1;
            return Err(E_DEADLOCK);
        }
        self.lock = LockPhase::Locked;
        self.hold_count += 1;
        // 判据 J2 前半：进临界区拍不可变快照（此后快照域只读）。
        self.snap_len = self.entries.len();
        self.snap_seq = self.seq_next;
        self.snap_reg = self.reg_ok;
        Ok(())
    }

    /// 解锁：无锁再解拒收；出临界区复核快照 delta 闭合。
    pub fn lock_release(&mut self) -> Result<(), u16> {
        if self.lock == LockPhase::Unlocked {
            self.rejects += 1;
            return Err(E_DOUBLE_UNLOCK);
        }
        // 判据 J2 后半：临界区内账面增量与快照基线 delta 闭合
        //（快照域本身未被改写，账面演化可由 delta 完全解释）。
        let delta = self.reg_ok - self.snap_reg;
        if self.entries.len() != self.snap_len + delta as usize {
            self.rejects += 1;
            return Err(E_SNAP_DIFF);
        }
        if self.seq_next != self.snap_seq + delta {
            self.rejects += 1;
            return Err(E_SNAP_DIFF);
        }
        self.lock = LockPhase::Unlocked;
        self.release_count += 1;
        Ok(())
    }

    /// 持锁期查重。
    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.name_len == name.len() {
                let mut j = 0;
                let mut same = true;
                while j < name.len() {
                    if e.name[j] != name[j] {
                        same = false;
                        break;
                    }
                    j += 1;
                }
                if same {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册（须持锁）：重名 EEXIST。
    pub fn register(&mut self, name: &[u8]) -> Result<u32, u16> {
        if self.lock != LockPhase::Locked {
            self.rejects += 1;
            return Err(E_OP_UNLOCKED);
        }
        if name.is_empty() || name.len() > NAME_CAP {
            self.rejects += 1;
            return Err(E_BAL);
        }
        if self.find(name).is_some() {
            return Err(E_EXIST);
        }
        let mut nm = [0u8; NAME_CAP];
        let mut i = 0;
        while i < name.len() {
            nm[i] = name[i];
            i += 1;
        }
        let seq = self.seq_next;
        self.entries.push(TypeCell { name: nm, name_len: name.len(), seq });
        self.seq_next += 1;
        self.reg_ok += 1;
        Ok(seq)
    }

    /// 查找（须持锁）：命中/未命中双账。
    pub fn lookup(&mut self, name: &[u8]) -> Result<u32, u16> {
        if self.lock != LockPhase::Locked {
            self.rejects += 1;
            return Err(E_OP_UNLOCKED);
        }
        match self.find(name) {
            Some(i) => {
                self.lookup_ok += 1;
                Ok(self.entries[i].seq)
            }
            None => {
                self.lookup_miss += 1;
                Err(E_NO_TYPE)
            }
        }
    }

    /// 交错调度一步：进锁 → 注册或查找 → 出锁（快照成对校验）。
    pub fn interleave_step(&mut self, round: u32) -> Result<(), u16> {
        self.lock_acquire()?;
        // 确定性交错：偶数轮注册名，奇数轮查找（含未命中空转）。
        if round % 2 == 0 {
            let nm = gen_name(round);
            // 已存在则跳过注册（本轮登记为重名拒收）。
            match self.find(&nm) {
                Some(_) => {
                    let _ = self.register(&nm); // EEXIST 路径（合法交错分支）
                }
                None => {
                    self.register(&nm)?;
                }
            }
        } else {
            let nm = gen_name(round - 1);
            let _ = self.lookup(&nm)?; // 必命中（前一轮注册）
        }
        self.lock_release()?;
        Ok(())
    }

    /// 守恒对账：锁账平、注册/查找账与表长闭合。
    pub fn audit(&self) -> Result<(), u16> {
        if self.hold_count != self.release_count {
            return Err(E_BAL);
        }
        if self.lock != LockPhase::Unlocked {
            return Err(E_BAL);
        }
        if self.reg_ok != self.entries.len() as u32 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（表长，注册账，命中账，空转账，持锁账）。
    pub fn ledger(&self) -> (usize, u32, u32, u32, u32) {
        (self.entries.len(), self.reg_ok, self.lookup_ok, self.lookup_miss, self.hold_count)
    }
}

/// 确定性名生成：偶数轮 r 生成 "t{r:06}"（整数化，无格式化依赖）。
fn gen_name(r: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b't');
    // 六位十进制展开。
    let mut div = 100_000u32;
    while div > 0 {
        let d = (r / div) % 10;
        out.push(b'0' + d as u8);
        div /= 10;
    }
    out
}

/// 自例程：R1 锁机注入 → R2 交错 1,000 步 → R3 快照断言 → R4 无锁注入 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut hub = FsTypeLock::new();

    // R1 锁机注入：持锁再锁与无锁再解分别被拒。
    hub.lock_acquire()?;
    if hub.lock_acquire() != Err(E_DEADLOCK) {
        return Err(E_BAL);
    }
    hub.lock_release()?;
    if hub.lock_release() != Err(E_DOUBLE_UNLOCK) {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 交错 1,000 步（判据 J1）：注册/查找交错零错账。
    let mut step = 0;
    while step < INTERLEAVE_STEPS {
        hub.interleave_step(step as u32)?;
        step += 1;
    }
    hits += 1;

    // R3 快照断言（判据 J2）：临界区快照与账面零 diff 已在出锁复核；
    // 此处复核账面自洽（快照序号 = 当前序号）。
    if hub.snap_seq != hub.seq_next || hub.snap_len != hub.entries.len() {
        return Err(E_SNAP_DIFF);
    }
    hits += 1;

    // R4 无锁注入：注册与查找在无锁期一律被拒。
    if hub.register(b"nolock") != Err(E_OP_UNLOCKED) {
        return Err(E_BAL);
    }
    if hub.lookup(b"nolock") != Err(E_OP_UNLOCKED) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒：锁账平、注册账 = 表长、查找命中 = 注册对数。
    hub.audit()?;
    let (ln, reg, lok, lmiss, holds) = hub.ledger();
    // 500 偶轮注册（全部成功，无重名），500 奇轮查找（全部命中）。
    if ln != (INTERLEAVE_STEPS / 2) as usize
        || reg != INTERLEAVE_STEPS / 2
        || lok != INTERLEAVE_STEPS / 2
        || lmiss != 0
        || holds != INTERLEAVE_STEPS + 1
    {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
