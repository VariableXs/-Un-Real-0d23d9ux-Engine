//! UNX-F12050 PEB FastPebLock 快锁语义
//! 分段: 0x19860 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：FastPebLock 是环境块上的自旋保护，规程为
//!   Acquire → 临界操作（受限集合：读写环境字段）→ Release。
//! 关键规则：
//!   1) 同一线程可重入（计数），跨线程重入视为死锁候选；
//!   2) 持锁期间只允许执行白名单操作；
//!   3) Release 必须与 Acquire 配对，未持锁 Release 视为损坏；
//!   4) 未持锁执行受限写操作 → 直接判违规。

use crate::alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x19860;
pub const SEG_FPL_DOUBLE: u32 = SEG_BASE + 0x01;
pub const SEG_FPL_STRAY: u32 = SEG_BASE + 0x02;
pub const SEG_FPL_FORBID: u32 = SEG_BASE + 0x03;
pub const SEG_FPL_NOLOCK: u32 = SEG_BASE + 0x04;
pub const SEG_FPL_SEALED: u32 = SEG_BASE + 0x05;

/// 持锁期间允许的操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PebOp {
    ReadEnv,
    WriteEnv,
    ReadVer,
    AllocPool,
    FreePool,
}

impl PebOp {
    /// 是否为受限写操作（必须持锁）
    pub fn needs_lock(self) -> bool {
        matches!(self, PebOp::WriteEnv | PebOp::FreePool)
    }

    /// 是否在白名单（持锁期间可执行）
    pub fn in_whitelist(self) -> bool {
        match self {
            PebOp::ReadEnv | PebOp::WriteEnv | PebOp::ReadVer => true,
            PebOp::AllocPool | PebOp::FreePool => false,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PebOp::ReadEnv => "RdEnv",
            PebOp::WriteEnv => "WrEnv",
            PebOp::ReadVer => "RdVer",
            PebOp::AllocPool => "Alloc",
            PebOp::FreePool => "Free",
        }
    }
}

/// 锁记录：一条 Acquire/Release 事件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEv {
    pub owner: u64,
    pub acquired: bool,
    pub depth_after: u32,
    pub sealed: bool,
}

/// 快锁状态机
#[derive(Debug, Clone)]
pub struct FastPebLock {
    pub holder: u64,
    pub depth: u32,
    pub sealed: bool,
    pub trace: Vec<LockEv>,
}

impl FastPebLock {
    pub fn new() -> FastPebLock {
        FastPebLock { holder: 0, depth: 0, sealed: false, trace: Vec::new() }
    }

    /// Acquire：同线程重入 → 计数；异线程持有 → DOUBLE（死锁候选）
    pub fn acquire(&mut self, tid: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(SEG_FPL_SEALED);
        }
        if self.depth > 0 && self.holder != tid {
            return Err(SEG_FPL_DOUBLE);
        }
        self.holder = tid;
        self.depth = self.depth + 1;
        self.trace.push(LockEv { owner: tid, acquired: true, depth_after: self.depth, sealed: self.sealed });
        Ok(self.depth)
    }

    /// Release：未持锁 → STRAY；配对成功 → 深度-1
    pub fn release(&mut self, tid: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(SEG_FPL_SEALED);
        }
        if self.depth == 0 || self.holder != tid {
            return Err(SEG_FPL_STRAY);
        }
        // 深度必然 >= 1，无下溢风险
        self.depth = self.depth - 1;
        if self.depth == 0 {
            self.holder = 0;
        }
        self.trace.push(LockEv { owner: tid, acquired: false, depth_after: self.depth, sealed: self.sealed });
        Ok(self.depth)
    }

    /// 在持锁前提下执行操作；白名单外 → FORBID；未持锁需锁操作 → NOLOCK
    pub fn exec(&mut self, tid: u64, op: PebOp) -> Result<&'static str, u32> {
        let holding = self.depth > 0 && self.holder == tid;
        if op.needs_lock() && !holding {
            return Err(SEG_FPL_NOLOCK);
        }
        if holding && !op.in_whitelist() {
            return Err(SEG_FPL_FORBID);
        }
        let tag = if holding { "locked" } else { "free" };
        Ok(tag)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(lock: &FastPebLock) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    h = (h.wrapping_mul(211).wrapping_add(lock.holder & 0xFF)) & 0xFFFF_FFFF;
    h = (h.wrapping_mul(211).wrapping_add(lock.depth as u64)) & 0xFFFF_FFFF;
    h = (h.wrapping_mul(211).wrapping_add(lock.sealed as u64)) & 0xFFFF_FFFF;
    for e in &lock.trace {
        h = (h.wrapping_mul(211).wrapping_add(e.owner & 0xFF)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(e.acquired as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(e.depth_after as u64)) & 0xFFFF_FFFF;
    }
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: 异线程重入 → DOUBLE
    let mut l = FastPebLock::new();
    let _ = l.acquire(1);
    if l.acquire(2) == Err(SEG_FPL_DOUBLE) {
        ok[0] = true;
    }
    // 样2: 未持锁 Release → STRAY
    let mut l2 = FastPebLock::new();
    if l2.release(7) == Err(SEG_FPL_STRAY) {
        ok[1] = true;
    }
    // 样3: 持锁执行 AllocPool → FORBID（白名单外）
    let mut l3 = FastPebLock::new();
    let _ = l3.acquire(1);
    if l3.exec(1, PebOp::AllocPool) == Err(SEG_FPL_FORBID) {
        ok[2] = true;
    }
    // 样4: 未持锁 WriteEnv → NOLOCK
    let mut l4 = FastPebLock::new();
    if l4.exec(1, PebOp::WriteEnv) == Err(SEG_FPL_NOLOCK) {
        ok[3] = true;
    }
    // 样5: 封存后 acquire → SEALED
    let mut l5 = FastPebLock::new();
    l5.seal();
    if l5.acquire(1) == Err(SEG_FPL_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12050_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12050");
    // J1 首次 acquire 深度 1
    let mut l = FastPebLock::new();
    cs.add("J1", l.acquire(1) == Ok(1), "acquire depth1");
    // J2 同线程重入深度 2
    cs.add("J2", l.acquire(1) == Ok(2), "reentrant depth2");
    // J3 异线程 → DOUBLE
    cs.add("J3", l.acquire(2).err() == Some(SEG_FPL_DOUBLE), "cross double");
    // J4 配对 release 深度回 1
    cs.add("J4", l.release(1) == Ok(1), "release to 1");
    // J5 完全释放 holder 清零
    cs.add("J5", l.release(1) == Ok(0) && l.holder == 0, "fully released");
    // J6 多释放 → STRAY
    cs.add("J6", l.release(1).err() == Some(SEG_FPL_STRAY), "stray release");
    // J7 白名单内操作持锁 OK
    let mut l2 = FastPebLock::new();
    let _ = l2.acquire(3);
    cs.add("J7", l2.exec(3, PebOp::ReadEnv) == Ok("locked"), "rd env locked");
    // J8 未持锁读也允许（读无锁）
    cs.add("J8", l2.exec(3, PebOp::ReadEnv).is_ok() && l2.exec(9, PebOp::ReadVer).is_ok(), "free read ok");
    // J9 持锁 AllocPool → FORBID
    cs.add("J9", l2.exec(3, PebOp::AllocPool).err() == Some(SEG_FPL_FORBID), "alloc forbid");
    // J10 未持锁 WriteEnv → NOLOCK
    cs.add("J10", l2.exec(9, PebOp::WriteEnv).err() == Some(SEG_FPL_NOLOCK), "nolock write");
    // J11 持锁 WriteEnv OK
    cs.add("J11", l2.exec(3, PebOp::WriteEnv) == Ok("locked"), "wr env locked");
    // J12 FreePool 需锁
    cs.add("J12", l2.exec(9, PebOp::FreePool).err() == Some(SEG_FPL_NOLOCK), "free needs lock");
    // J13 trace 记录数
    let mut l3 = FastPebLock::new();
    let _ = l3.acquire(1);
    let _ = l3.release(1);
    cs.add("J13", l3.trace.len() == 2 && l3.trace[0].acquired, "trace len 2");
    // J14 注入五样
    let inj = inject_five();
    cs.add("J14", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reentrant_pair() {
        let mut l = FastPebLock::new();
        assert!(l.acquire(5).is_ok());
        assert!(l.acquire(5).is_ok());
        assert_eq!(l.release(5).ok(), Some(1));
        assert_eq!(l.release(5).ok(), Some(0));
        assert!(l.release(5).is_err());
    }

    #[test]
    fn test_cross_thread_deadlock() {
        let mut l = FastPebLock::new();
        let _ = l.acquire(1);
        assert_eq!(l.acquire(2).err(), Some(SEG_FPL_DOUBLE));
        assert!(l.release(2).is_err());
        assert!(l.release(1).is_ok());
    }

    #[test]
    fn test_fold_stable() {
        let mut l = FastPebLock::new();
        let _ = l.acquire(1);
        let _ = l.release(1);
        let a = fold(&l);
        let b = fold(&l);
        assert_eq!(a, b);
        assert_ne!(a, SEG_BASE);
    }
}
