//! unx4411_fcntlcmd — UNX-F4411 fcntl 命令字全集裁断（诊断段 0xB720）。
//!
//! 判据原文：J1 命令字闭集：本锚登记 fcntl 全集 12 命令（F_DUPFD/
//!           F_GETFD/F_SETFD/F_GETFL/F_SETFL/F_GETLK/F_SETLK/
//!           F_SETLKW/F_GETOWN/F_SETOWN/F_DUPFD_CLOEXEC/未知），未
//!           知命令字裁断 EINVAL；J2 每命令带参数形态闭集
//!           （int/ptr/none）与返回形态闭集，形态与命令一一对应，
//!           错配裁断；J3 裁断账守恒：dispatched = ok + rejected，
//!           per-command 分账之和等于全局账；J4 未知命令注入多次
//!           全拒且不污染已登记命令的账目。
//! 设计纪律：
//!   1. Linux fcntl 语义：命令字决定 arg 解释方式——int 类
//!      (F_GETFD/F_SETFD/F_GETFL/F_SETFL/F_GETOWN/F_SETOWN)、指针
//!      类（lock 族、dupfd 族返回 int）。裁断在入口层。
//!   2. 命令表：静态闭集表（cmd → (arg形态, ret形态, 是否实现)），
//!      未实现命令按矩阵返回缺省错误。
//!   3. 账目：per-command 计数槽 + 全局 dispatched/rejected。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0xB720 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB720;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_UNKNOWN: u16 = SEG_BASE + 1;   // 未知命令字（EINVAL 账）
const E_ARGFORM: u16 = SEG_BASE + 2;   // 参数形态错配
const E_RETFORM: u16 = SEG_BASE + 3;   // 返回形态错配
const E_UNIMPL: u16 = SEG_BASE + 4;    // 已登记未实现（缺省矩阵）
const E_BAL: u16 = SEG_BASE + 5;       // 裁断账不守恒
const E_CMD_RANGE: u16 = SEG_BASE + 6; // 命令号越表

/// fcntl 命令字闭集（Linux 值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    DupFd = 1,        // F_DUPFD (0)
    GetFd = 2,        // F_GETFD (1)
    SetFd = 3,        // F_SETFD (2)
    GetFl = 4,        // F_GETFL (3)
    SetFl = 5,        // F_SETFL (4)
    GetLk = 6,        // F_GETLK (5)
    SetLk = 7,        // F_SETLK (6)
    SetLkw = 8,       // F_SETLKW (7)
    GetOwn = 9,       // F_GETOWN (9)
    SetOwn = 10,      // F_SETOWN (8)
    DupFdClo = 11,    // F_DUPFD_CLOEXEC (1030)
    Unknown = 12,     // 未知命令兜底槽
}

pub const NCMD: usize = 12;

/// 参数形态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgForm {
    Int = 1,
    Ptr,
    None,
}

/// 返回形态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetForm {
    NewFd = 1,
    Flags,
    Status,
    LockInfo,
    OwnerPid,
}

/// 命令表项：形态三元组 + 实现位。
#[derive(Clone, Copy)]
pub struct CmdEnt {
    pub cmd: Cmd,
    pub arg: ArgForm,
    pub ret: RetForm,
    pub implemented: bool,
}

/// fcntl 裁断枢纽。
pub struct FcntlGate {
    table: [CmdEnt; NCMD],
    per_cmd: [u32; NCMD],
    pub dispatched: u32,
    pub ok: u32,
    pub rejected: u32,
}

impl FcntlGate {
    /// 静态命令表（判据 J1/J2 的承载主体）。
    pub fn new() -> FcntlGate {
        let e = |cmd: Cmd, arg: ArgForm, ret: RetForm, imp: bool| CmdEnt {
            cmd, arg, ret, implemented: imp,
        };
        FcntlGate {
            table: [
                e(Cmd::DupFd, ArgForm::Int, RetForm::NewFd, true),
                e(Cmd::GetFd, ArgForm::None, RetForm::Flags, true),
                e(Cmd::SetFd, ArgForm::Int, RetForm::Status, true),
                e(Cmd::GetFl, ArgForm::None, RetForm::Flags, true),
                e(Cmd::SetFl, ArgForm::Int, RetForm::Flags, true),
                e(Cmd::GetLk, ArgForm::Ptr, RetForm::LockInfo, false),
                e(Cmd::SetLk, ArgForm::Ptr, RetForm::Status, false),
                e(Cmd::SetLkw, ArgForm::Ptr, RetForm::Status, false),
                e(Cmd::GetOwn, ArgForm::None, RetForm::OwnerPid, true),
                e(Cmd::SetOwn, ArgForm::Int, RetForm::Status, true),
                e(Cmd::DupFdClo, ArgForm::Int, RetForm::NewFd, true),
                e(Cmd::Unknown, ArgForm::None, RetForm::Status, false),
            ],
            per_cmd: [0; NCMD],
            dispatched: 0,
            ok: 0,
            rejected: 0,
        }
    }

    fn slot(&self, cmd: Cmd) -> Result<usize, u16> {
        let s = cmd as usize - 1;
        if s >= NCMD {
            return Err(E_CMD_RANGE);
        }
        Ok(s)
    }

    /// 入口裁断（判据 J1/J2/J4）。
    pub fn dispatch(&mut self, cmd: Cmd, arg_form: ArgForm, ret_want: RetForm) -> Result<RetForm, u16> {
        self.dispatched += 1;
        let s = self.slot(cmd)?;
        self.per_cmd[s] += 1;
        let ent = self.table[s];
        if cmd == Cmd::Unknown {
            self.rejected += 1;
            return Err(E_UNKNOWN);
        }
        if ent.arg != arg_form {
            self.rejected += 1;
            return Err(E_ARGFORM);
        }
        if ent.ret != ret_want {
            self.rejected += 1;
            return Err(E_RETFORM);
        }
        if !ent.implemented {
            self.rejected += 1;
            return Err(E_UNIMPL);
        }
        self.ok += 1;
        Ok(ent.ret)
    }

    /// per-command 账。
    pub fn count(&self, cmd: Cmd) -> Result<u32, u16> {
        let s = self.slot(cmd)?;
        Ok(self.per_cmd[s])
    }

    /// 分账闭合（判据 J3）。
    pub fn balanced(&self) -> bool {
        let mut sum = 0u32;
        for c in self.per_cmd.iter() {
            sum += *c;
        }
        sum == self.dispatched && self.dispatched == self.ok + self.rejected
    }

    /// 实现数（已登记且 implemented）。
    pub fn impl_count(&self) -> usize {
        let mut n = 0;
        for e in self.table.iter() {
            if e.implemented && e.cmd != Cmd::Unknown {
                n += 1;
            }
        }
        n
    }
}

/// 自例程：R1 已实现命令全放行；R2 lock 族缺省矩阵拒收；
/// R3 形态错配裁断；R4 未知命令注入；R5 分账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：7 个已实现命令各 dispatch 一笔正确形态。
    let mut g = FcntlGate::new();
    g.dispatch(Cmd::DupFd, ArgForm::Int, RetForm::NewFd)?;
    g.dispatch(Cmd::GetFd, ArgForm::None, RetForm::Flags)?;
    g.dispatch(Cmd::SetFd, ArgForm::Int, RetForm::Status)?;
    g.dispatch(Cmd::GetFl, ArgForm::None, RetForm::Flags)?;
    g.dispatch(Cmd::SetFl, ArgForm::Int, RetForm::Flags)?;
    g.dispatch(Cmd::GetOwn, ArgForm::None, RetForm::OwnerPid)?;
    g.dispatch(Cmd::DupFdClo, ArgForm::Int, RetForm::NewFd)?;
    if g.ok != 7 {
        return Err(E_BAL);
    }
    if g.impl_count() != 8 {
        return Err(E_BAL); // 7 + SetOwn
    }
    hits += 1;

    // R2：lock 族已登记未实现 → 缺省矩阵拒收（判据 J2/J1）。
    if g.dispatch(Cmd::GetLk, ArgForm::Ptr, RetForm::LockInfo) != Err(E_UNIMPL) {
        return Err(E_UNIMPL);
    }
    if g.dispatch(Cmd::SetLk, ArgForm::Ptr, RetForm::Status) != Err(E_UNIMPL) {
        return Err(E_UNIMPL);
    }
    if g.dispatch(Cmd::SetLkw, ArgForm::Ptr, RetForm::Status) != Err(E_UNIMPL) {
        return Err(E_UNIMPL);
    }
    hits += 1;

    // R3：形态错配——int 命令传 Ptr、ptr 命令传 None 全拒。
    if g.dispatch(Cmd::DupFd, ArgForm::Ptr, RetForm::NewFd) != Err(E_ARGFORM) {
        return Err(E_ARGFORM);
    }
    if g.dispatch(Cmd::GetLk, ArgForm::None, RetForm::LockInfo) != Err(E_ARGFORM) {
        return Err(E_ARGFORM);
    }
    if g.dispatch(Cmd::GetFd, ArgForm::None, RetForm::Status) != Err(E_RETFORM) {
        return Err(E_RETFORM);
    }
    hits += 1;

    // R4：未知命令注入多次全拒（判据 J4）。
    for _ in 0..4 {
        if g.dispatch(Cmd::Unknown, ArgForm::None, RetForm::Status) != Err(E_UNKNOWN) {
            return Err(E_UNKNOWN);
        }
    }
    if g.count(Cmd::Unknown)? != 4 {
        return Err(E_BAL);
    }
    // 已登记命令账不被污染
    if g.count(Cmd::DupFd)? != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：分账守恒终验（判据 J3）。
    if !g.balanced() {
        return Err(E_BAL);
    }
    let mut sum = 0u32;
    for s in 1..=NCMD {
        let c = match s {
            1 => Cmd::DupFd,
            2 => Cmd::GetFd,
            3 => Cmd::SetFd,
            4 => Cmd::GetFl,
            5 => Cmd::SetFl,
            6 => Cmd::GetLk,
            7 => Cmd::SetLk,
            8 => Cmd::SetLkw,
            9 => Cmd::GetOwn,
            10 => Cmd::SetOwn,
            11 => Cmd::DupFdClo,
            _ => Cmd::Unknown,
        };
        sum += g.count(c)?;
    }
    if sum != g.dispatched {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
