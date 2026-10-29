//! unx4410_setfdbg — UNX-F4410 F_SETFD 非法位注入（诊断段 0xB700）。
//!
//! 判据原文：J1 F_SETFD 只认 FD_CLOEXEC 一位：合法位集 = {0x1}，
//!           其余位（0x2~0x8000 与高位）全部拒收 EINVAL 语义并入注入账；
//!           J2 注入零副作用：拒收后 fd 的 fdflags 与关前逐位一致
//!           （位账快照比对），文件内容账不动；
//!           J3 合法写路径：FD_CLOEXEC 置 1/清 0 各一次成功，位账
//!           恰好变更两次且方向账（set/clr）各记一笔；
//!           J4 注入账守恒：injected = rejected + accepted，且
//!           per-fd 注入分账与全局账闭合。
//! 设计纪律：
//!   1. Linux 语义：F_SETFD 只接受 FD_CLOEXEC（0x1）；F_GETFD 读回
//!      同一位。fd 旗标与文件旗标（F_SETFL）双轨隔离。
//!   2. 位账模型：fdflags: u16，有效位掩码 VALID = 0x1，越掩码写入
//!      即注入（EINVAL 账）。
//!   3. 快照比对：每次注入前取 fdflags 快照，拒收后比对一致。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0xB700 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB700;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_BIT: u16 = SEG_BASE + 1;  // 非法位注入（EINVAL 账）
const E_FD_BAD: u16 = SEG_BASE + 2;   // fd 越表
const E_BAL: u16 = SEG_BASE + 3;      // 注入账不守恒
const E_SNAP_DIFF: u16 = SEG_BASE + 4;// 注入后位账漂移（零副作用失守）
const E_DIR_MISS: u16 = SEG_BASE + 5; // 方向账缺笔

/// FD_CLOEXEC = 0x1（fcntl.h）。
pub const FD_CLOEXEC: u16 = 0x1;
/// 有效位掩码：F_SETFD 全集。
pub const VALID_MASK: u16 = 0x1;

/// fd 记录。
pub struct FdRec {
    pub fdflags: u16,
    pub set_ops: u32,
    pub clr_ops: u32,
    pub inj_hits: u32,
}

/// F_SETFD 注入闸枢纽。
pub struct SetFdGate {
    fds: Vec<FdRec>,
    pub injected: u32,
    pub rejected: u32,
    pub accepted: u32,
}

impl SetFdGate {
    pub fn new(nfds: usize) -> SetFdGate {
        let mut fds = Vec::new();
        for _ in 0..nfds {
            fds.push(FdRec {
                fdflags: 0,
                set_ops: 0,
                clr_ops: 0,
                inj_hits: 0,
            });
        }
        SetFdGate {
            fds,
            injected: 0,
            rejected: 0,
            accepted: 0,
        }
    }

    /// F_SETFD：掩码外位注入即拒（判据 J1），拒收零副作用（判据 J2）。
    pub fn setfd(&mut self, fd: usize, flags: u16) -> Result<(), u16> {
        if fd >= self.fds.len() {
            return Err(E_FD_BAD);
        }
        self.injected += 1;
        if flags & !VALID_MASK != 0 {
            // 快照比对：拒收前后位账一致（判据 J2）
            let before = self.fds[fd].fdflags;
            self.fds[fd].inj_hits += 1;
            self.rejected += 1;
            if self.fds[fd].fdflags != before {
                return Err(E_SNAP_DIFF);
            }
            return Err(E_BAD_BIT);
        }
        // 合法写：方向账（判据 J3）
        let on = flags & FD_CLOEXEC != 0;
        self.fds[fd].fdflags = flags;
        if on {
            self.fds[fd].set_ops += 1;
        } else {
            self.fds[fd].clr_ops += 1;
        }
        self.accepted += 1;
        Ok(())
    }

    /// F_GETFD。
    pub fn getfd(&self, fd: usize) -> Result<u16, u16> {
        if fd >= self.fds.len() {
            return Err(E_FD_BAD);
        }
        Ok(self.fds[fd].fdflags)
    }

    /// per-fd 注入账。
    pub fn fd_inj(&self, fd: usize) -> Result<u32, u16> {
        if fd >= self.fds.len() {
            return Err(E_FD_BAD);
        }
        Ok(self.fds[fd].inj_hits)
    }

    /// 方向账闭合（判据 J3）：set_ops == clr_ops + 当前置位数（可逆推）。
    pub fn dir_balanced(&self) -> bool {
        let mut cur = 0u32;
        for f in self.fds.iter() {
            if f.fdflags & FD_CLOEXEC != 0 {
                cur += 1;
            }
        }
        let mut setsum = 0u32;
        let mut clrsum = 0u32;
        for f in self.fds.iter() {
            setsum += f.set_ops;
            clrsum += f.clr_ops;
        }
        // 每 fd：set_ops - clr_ops ∈ {0,1} 且与当前位一致
        for f in self.fds.iter() {
            let delta = f.set_ops as i64 - f.clr_ops as i64;
            let want = if f.fdflags & FD_CLOEXEC != 0 { 1i64 } else { 0i64 };
            if delta != want {
                return false;
            }
        }
        setsum >= clrsum || cur == 0
    }

    /// 注入账守恒（判据 J4）。
    pub fn inj_balanced(&self) -> bool {
        self.injected == self.rejected + self.accepted
    }
}

/// 自例程：R1 合法位 set/clr；R2 非法位注入全拒 + 零副作用；
/// R3 per-fd 分账与全局账闭合；R4 越表拦截；R5 双账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：合法位 set→1、clr→0 各一笔，方向账两笔（判据 J3）。
    let mut g = SetFdGate::new(4);
    g.setfd(1, FD_CLOEXEC)?;
    if g.getfd(1)? != FD_CLOEXEC {
        return Err(E_DIR_MISS);
    }
    g.setfd(1, 0)?;
    if g.getfd(1)? != 0 {
        return Err(E_DIR_MISS);
    }
    if g.accepted != 2 || !g.dir_balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：非法位注入——0x2/0x4/0x8000/0xFFFF 全拒且位账零漂移。
    for bad in [0x2u16, 0x4, 0x8000, 0xFFFF, FD_CLOEXEC | 0x2].iter() {
        let before = g.getfd(2)?;
        if g.setfd(2, *bad) != Err(E_BAD_BIT) {
            return Err(E_BAD_BIT);
        }
        if g.getfd(2)? != before {
            return Err(E_SNAP_DIFF);
        }
    }
    if g.rejected != 5 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：per-fd 分账——fd2 注入 5 笔；全局闭合（判据 J4）。
    if g.fd_inj(2)? != 5 {
        return Err(E_BAL);
    }
    if g.fd_inj(1)? != 0 {
        return Err(E_BAL);
    }
    if !g.inj_balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：越表拦截。
    if g.setfd(99, FD_CLOEXEC) != Err(E_FD_BAD) {
        return Err(E_FD_BAD);
    }
    if g.getfd(99) != Err(E_FD_BAD) {
        return Err(E_FD_BAD);
    }
    if g.fd_inj(99) != Err(E_FD_BAD) {
        return Err(E_FD_BAD);
    }
    hits += 1;

    // R5：守恒终验——账三槽闭合 + 方向账再核。
    g.setfd(0, FD_CLOEXEC)?;
    g.setfd(3, FD_CLOEXEC)?;
    g.setfd(3, 0)?;
    if !g.dir_balanced() || !g.inj_balanced() {
        return Err(E_BAL);
    }
    // fd3 方向：set1 + clr1 → delta 0 与位 0 一致
    if g.accepted + g.rejected != g.injected {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
