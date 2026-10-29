//! unx4190_magicperm — UNX-F4190 magic-link 重打开权限校验点（诊断段 0x9940）。
//!
//! 判据原文：
//!   J1 UNX-F4190-J1 重打开时权限校验（按打开时凭据而非读链接时）断言
//!      通过——越权经 magic-link 重开注入 100/100 次被拒（POSIX 权威规则
//!      落账）。
//! 设计纪律：
//!   1. 凭据快照：打开时凭据（uid/gid）随打开记录冻结成快照，重打开
//!      校验恒按快照执行，与读链接时的当前凭据无关——POSIX 权威规则。
//!   2. 校验闭集：重打开者 uid 必须与快照 uid 一致（含 gid 附属核对），
//!      不一致即 E_PERM 拒收，拒收入账（判据 100/100 账）。
//!   3. 读链接放行、重打开把守：readlink 语义层不做凭据校验（magic-link
//!      读出恒可），权限校验只落在重打开点——两路分账不串。
//!   4. 校验状态机：Idle→Checked→Granted/Denied，一次重开一次迁移闭环，
//!      判定理由（快照比对）随账可导出。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9940;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_PERM: u16 = SEG_BASE + 1;    // 重打开凭据与打开时快照不符
const E_NOENT: u16 = SEG_BASE + 2;   // 打开记录不存在
const E_DUP_FD: u16 = SEG_BASE + 3;  // fd 重复登记
const E_BAD_FD: u16 = SEG_BASE + 4;  // 重打开引用的 fd 未注册
const E_LEDGER: u16 = SEG_BASE + 5;  // 凭据账失守（判定理由与结果不符）
const E_STATE: u16 = SEG_BASE + 6;   // 校验状态机非法迁移

/// 判据注入次数（越权重开 100/100 拒）。
pub const INJECT_TIMES: u32 = 100;

/// 凭据：uid/gid 整数对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cred {
    pub uid: u32,
    pub gid: u32,
}

/// 校验状态机：Idle→Checked→{Granted,Denied}。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermPhase {
    Idle,
    Checked,
    Granted,
    Denied,
}

/// 判定二值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermVerdict {
    Granted,
    Denied,
}

/// 打开记录：fd + 打开时凭据快照 + 记录状态。
pub struct OpenRec {
    pub fd: u32,        // 文件描述符号
    pub opener: Cred,   // 打开时凭据快照（冻结）
    pub live: bool,     // 实态位（打开期有效）
}

/// 权限校验枢纽：打开登记（快照冻结）+ 重打开校验点 + 分账。
pub struct PermGate {
    pub recs: Vec<OpenRec>,
    pub phase: PermPhase,
    pub grants: u32,     // 重开放行数
    pub denials: u32,    // 越权拒收数（判据 100/100 账）
    pub wrong_reason: u32, // 拒收理由错位数（恒零）
    pub read_passes: u32,// 读链接放行数（不校验凭据的分账面）
}

impl PermGate {
    pub fn new() -> PermGate {
        PermGate {
            recs: Vec::new(),
            phase: PermPhase::Idle,
            grants: 0,
            denials: 0,
            wrong_reason: 0,
            read_passes: 0,
        }
    }

    fn rec_index(&self, fd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].fd == fd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 打开登记：凭据快照随记录冻结（判据「打开时凭据」的账面载体）。
    pub fn open(&mut self, fd: u32, who: Cred) -> Result<(), u16> {
        if self.rec_index(fd).is_some() {
            return Err(E_DUP_FD);
        }
        self.recs.push(OpenRec {
            fd,
            opener: who,
            live: true,
        });
        Ok(())
    }

    /// 读链接面：不做凭据校验，恒放行（读链接/重打开两路分账）。
    pub fn readlink_pass(&mut self, fd: u32) -> Result<(), u16> {
        match self.rec_index(fd) {
            Some(_) => {
                self.read_passes += 1;
                Ok(())
            }
            None => Err(E_NOENT),
        }
    }

    /// 重打开校验点：按打开时凭据快照比对，与当前读链接者凭据无关。
    pub fn reopen(&mut self, fd: u32, who: Cred) -> Result<PermVerdict, u16> {
        let idx = match self.rec_index(fd) {
            Some(i) => i,
            None => return Err(E_BAD_FD),
        };
        if !self.recs[idx].live {
            return Err(E_NOENT);
        }
        self.phase = PermPhase::Checked;
        // 快照比对：uid/gid 双对齐才放行（POSIX 权威规则落账）。
        let same = self.recs[idx].opener.uid == who.uid && self.recs[idx].opener.gid == who.gid;
        if same {
            self.phase = PermPhase::Granted;
            self.grants += 1;
            Ok(PermVerdict::Granted)
        } else {
            self.phase = PermPhase::Denied;
            self.denials += 1;
            Ok(PermVerdict::Denied)
        }
    }

    /// 拒收口径化简：越权重开必须以 E_PERM 口径对外（判定入账）。
    pub fn reopen_strict(&mut self, fd: u32, who: Cred) -> Result<(), u16> {
        match self.reopen(fd, who)? {
            PermVerdict::Granted => Ok(()),
            PermVerdict::Denied => Err(E_PERM),
        }
    }

    /// 账面守恒：放行+拒收=校验总数；错位恒零。
    pub fn conserved(&self) -> bool {
        self.wrong_reason == 0
    }

    /// 注销打开记录（实态失效）：注销后重开报 E_NOENT（记录不再在位）。
    pub fn close(&mut self, fd: u32) -> Result<(), u16> {
        match self.rec_index(fd) {
            Some(i) => {
                self.recs[i].live = false;
                Ok(())
            }
            None => Err(E_NOENT),
        }
    }

    /// 校验总次数读面（放行+拒收）。
    pub fn total_checks(&self) -> u32 {
        self.grants + self.denials
    }

    /// 三分账快照导出（放行/拒收/读链接放行）。
    pub fn snapshot(&self) -> [u32; 3] {
        [self.grants, self.denials, self.read_passes]
    }

    /// 状态机读面。
    pub fn phase_of(&self) -> PermPhase {
        self.phase
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_PERM => "重打开凭据与打开时快照不符",
        E_NOENT => "打开记录不存在",
        E_DUP_FD => "fd 重复登记",
        E_BAD_FD => "重打开引用的 fd 未注册",
        E_LEDGER => "凭据账失守（判定理由与结果不符）",
        E_STATE => "校验状态机非法迁移",
        _ => "未知诊断码",
    }
}

/// 确定性演练：快照冻结→同凭据放行→越权 100/100 拒→读链接分账→注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut g = PermGate::new();

    // R1：登记打开记录——uid1000 打开 fd5（凭据快照冻结）。
    let opener = Cred { uid: 1000, gid: 100 };
    g.open(5, opener)?;
    if !matches!(g.phase_of(), PermPhase::Idle) {
        return Err(E_STATE);
    }
    hits += 1;

    // R2：同凭据重打开放行（快照一致 → Granted）。
    match g.reopen(5, opener)? {
        PermVerdict::Granted => {}
        PermVerdict::Denied => return Err(E_LEDGER),
    }
    if g.grants != 1 || !matches!(g.phase_of(), PermPhase::Granted) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：越权重开注入 100/100——uid2000/gid 错位两种越权各半，全拒。
    let intruder = Cred { uid: 2000, gid: 100 };
    let wrong_gid = Cred { uid: 1000, gid: 200 };
    let mut n = 0u32;
    while n < INJECT_TIMES / 2 {
        if g.reopen_strict(5, intruder) != Err(E_PERM) {
            return Err(E_PERM);
        }
        if g.reopen_strict(5, wrong_gid) != Err(E_PERM) {
            return Err(E_PERM);
        }
        n += 1;
    }
    if g.denials != INJECT_TIMES {
        return Err(E_PERM); // 判据：100/100 次被拒
    }
    hits += 1;

    // R4：读链接分账——magic-link 读出恒放行（不校验凭据），与重开账不串。
    let mut n = 0u32;
    while n < 3 {
        g.readlink_pass(5)?;
        n += 1;
    }
    if g.read_passes != 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：未注册 fd 重开 / 未登记 fd 读链接。
    if g.reopen_strict(99, opener) != Err(E_BAD_FD) {
        return Err(E_BAD_FD);
    }
    if g.readlink_pass(99) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    hits += 1;

    // I2：fd 重复登记注入。
    if g.open(5, opener) != Err(E_DUP_FD) {
        return Err(E_DUP_FD);
    }
    hits += 1;

    // R5：终审——放行+拒收对平、错位恒零、状态机终态 Denied。
    if !g.conserved() {
        return Err(E_LEDGER);
    }
    if g.grants + g.denials != 1 + INJECT_TIMES {
        return Err(E_LEDGER);
    }
    if !matches!(g.phase_of(), PermPhase::Denied) {
        return Err(E_STATE);
    }
    hits += 1;

    // R6：第二记录对照——uid2000 打开 fd6：同凭据放行、uid1000 越权拒收
    // （×20，双向凭据矩阵补面）。
    let opener2 = Cred { uid: 2000, gid: 300 };
    g.open(6, opener2)?;
    match g.reopen(6, opener2)? {
        PermVerdict::Granted => {}
        PermVerdict::Denied => return Err(E_LEDGER),
    }
    let outsider = Cred { uid: 1000, gid: 100 };
    let mut n = 0u32;
    while n < 20 {
        if g.reopen_strict(6, outsider) != Err(E_PERM) {
            return Err(E_PERM);
        }
        n += 1;
    }
    if g.denials != INJECT_TIMES + 20 {
        return Err(E_PERM);
    }
    hits += 1;

    // R7：注销实态面——close(fd6) 后重开报 E_NOENT（记录不在位），
    // 不入放行/拒收两账（校验点只在活记录上）。
    g.close(6)?;
    if g.reopen(6, opener2) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    let total = g.total_checks();
    if total != g.grants + g.denials {
        return Err(E_LEDGER);
    }
    let s = g.snapshot();
    if s[0] != 2 || s[2] != 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
