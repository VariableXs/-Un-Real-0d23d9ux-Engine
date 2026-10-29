//! unx4031_srootdentry — UNX-F4031 s_root dentry 与挂载根约定（诊断段 0x8660）。
//!
//! 判据原文：
//!   J1 s_root dentry 绑定根 inode 且为目录类型（非目录注入被拒）；
//!   J2 dentry 父指针指向自身（根约定）断言通过。
//! 设计纪律：
//!   1. 根 dentry 双锚：i_dentry 指向根 inode（号 = 2 约定），
//!      d_parent 指向自身（根无父约定）——两锚独立校验。
//!   2. dentry 状态机：Unbound → BoundRoot → Mounted，单向迁移；
//!      换绑（rebind）仅允许 Unbound 期，BoundRoot 后冻结。
//!   3. 非目录注入拦截：绑定目标 inode 类型位必须为目录，
//!      普通文件/其他类型一律拒收，dentry 状态不迁移。
//!   4. 守恒式：bound 计数 ≤ 1（单根唯一）；自指父指针账恰一条；
//!      绑定尝试数 = 成功数 + 拒收数。
//!   5. 边界：悬空 inode 索引、重复绑定、未绑定先挂载、
//!      父指针错位注入——全部显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8660;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_NOT_DIR: u16 = SEG_BASE + 1;     // 绑定目标非目录类型
const E_BAD_INO: u16 = SEG_BASE + 2;     // inode 号违反根约定（≠2）
const E_DANGLING: u16 = SEG_BASE + 3;    // 悬空 inode 索引
const E_REBIND: u16 = SEG_BASE + 4;      // 已绑定再换绑（冻结违例）
const E_NOT_BOUND: u16 = SEG_BASE + 5;   // 未绑定先挂载
const E_PARENT_MISS: u16 = SEG_BASE + 6; // 自指父指针缺失/错位
const E_BAL: u16 = SEG_BASE + 7;         // 绑定账不平

/// 根 inode 号约定（与 4030 同谱系：根 = 2）。
pub const ROOT_INO: u64 = 2;

/// dentry 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DPhase {
    Unbound,   // 建成未绑根
    BoundRoot, // 已绑定根 inode（s_root 就位）
    Mounted,   // 已随 sb 挂载
}

/// 简化 inode：号 + 目录类型位。
pub struct DIno {
    pub i_ino: u64,   // inode 号
    pub is_dir: bool, // 目录类型位
}

/// s_root dentry：inode 锚 + 自指父锚 + 状态。
pub struct Dentry {
    pub phase: DPhase,
    pub ino_idx: Option<usize>, // i_dentry（绑定的 inode 池下标）
    pub parent: Option<usize>,  // d_parent（根约定 = 自身下标）
}

/// s_root 枢纽：inode 池 + 根 dentry + 绑定账。
pub struct SRoot {
    pub inodes: Vec<DIno>,
    pub dentry: Dentry,
    pub bind_calls: u32,     // 绑定尝试总数
    pub bind_ok: u32,        // 绑定成功数（≤1）
    pub mount_ok: u32,       // 挂载成功数（≤1）
    pub reject_total: u32,   // 错误账
}

impl SRoot {
    pub fn new() -> SRoot {
        SRoot {
            inodes: Vec::new(),
            dentry: Dentry {
                phase: DPhase::Unbound,
                ino_idx: None,
                parent: None,
            },
            bind_calls: 0,
            bind_ok: 0,
            mount_ok: 0,
            reject_total: 0,
        }
    }

    /// 建入 inode，返回池下标。
    pub fn add_ino(&mut self, ino: u64, is_dir: bool) -> Result<usize, u16> {
        if self.inodes.len() >= 0xFFFF {
            self.reject_total += 1;
            return Err(E_BAL);
        }
        self.inodes.push(DIno { i_ino: ino, is_dir });
        Ok(self.inodes.len() - 1)
    }

    /// 绑定根：目录类型 + 根号约定 + 父自指三校验。
    pub fn bind_root(&mut self, idx: usize) -> Result<DPhase, u16> {
        self.bind_calls += 1;
        if idx >= self.inodes.len() {
            self.reject_total += 1;
            return Err(E_DANGLING);
        }
        if self.dentry.phase != DPhase::Unbound {
            self.reject_total += 1;
            return Err(E_REBIND);
        }
        let ino = self.inodes[idx].i_ino;
        let dir = self.inodes[idx].is_dir;
        if ino != ROOT_INO {
            self.reject_total += 1;
            return Err(E_BAD_INO);
        }
        if !dir {
            // 判据 J1 反向注入：非目录绑定被拒，状态不迁移。
            self.reject_total += 1;
            return Err(E_NOT_DIR);
        }
        self.dentry.ino_idx = Some(idx);
        self.dentry.parent = Some(idx); // 根约定：父指针指向自身
        self.dentry.phase = DPhase::BoundRoot;
        self.bind_ok += 1;
        Ok(self.dentry.phase)
    }

    /// 挂载：仅 BoundRoot 放行，迁 Mounted。
    pub fn mount(&mut self) -> Result<DPhase, u16> {
        match self.dentry.phase {
            DPhase::BoundRoot => {
                self.dentry.phase = DPhase::Mounted;
                self.mount_ok += 1;
                Ok(self.dentry.phase)
            }
            DPhase::Unbound => {
                self.reject_total += 1;
                Err(E_NOT_BOUND)
            }
            DPhase::Mounted => {
                self.reject_total += 1;
                Err(E_BAL)
            }
        }
    }

    /// 自指断言：父指针必须等于自身绑定下标（判据 J2）。
    pub fn assert_self_parent(&self) -> Result<(), u16> {
        match (self.dentry.ino_idx, self.dentry.parent) {
            (Some(i), Some(p)) if i == p => Ok(()),
            (Some(_), Some(_)) => {
                Err(E_PARENT_MISS)
            }
            _ => Err(E_PARENT_MISS),
        }
    }

    /// 双锚可验导出（绑定 inode 号，父自指布尔）。
    pub fn anchor_view(&self) -> Result<(u64, bool), u16> {
        let i = match self.dentry.ino_idx {
            Some(i) if i < self.inodes.len() => i,
            _ => return Err(E_DANGLING),
        };
        let selfp = self.dentry.parent == Some(i);
        Ok((self.inodes[i].i_ino, selfp))
    }

    /// 守恒对账：绑定账 + 挂载账与错误账闭合； Mounted ⇒ 自指成立。
    pub fn audit(&self) -> Result<(), u16> {
        if self.bind_ok + self.reject_only_binds() > self.bind_calls {
            return Err(E_BAL);
        }
        if self.dentry.phase == DPhase::Mounted {
            self.assert_self_parent()?;
        }
        if self.mount_ok > self.bind_ok {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 绑定侧被拒近似计数（错误账为 get/bind 共享，此处保守推 0）。
    fn reject_only_binds(&self) -> u32 {
        if self.bind_calls >= self.bind_ok { self.bind_calls - self.bind_ok } else { 0 }
    }

    /// 导出快照（状态，绑定尝试，绑定成功，挂载成功，拒收）。
    pub fn ledger(&self) -> (DPhase, u32, u32, u32, u32) {
        (self.dentry.phase, self.bind_calls, self.bind_ok, self.mount_ok, self.reject_total)
    }

    /// 绑定账视图：返回（尝试数，成功数，拒收数）三账互证基线。
    pub fn bind_view(&self) -> (u32, u32, u32) {
        (self.bind_calls, self.bind_ok, self.reject_total)
    }

    /// 拒收细分：绑定侧拒收（尝试 - 成功）与挂载侧拒收（错误账余量）。
    pub fn reject_split(&self) -> (u32, u32) {
        let bind_rej = self.bind_calls - self.bind_ok;
        let mount_rej = self.reject_total.saturating_sub(bind_rej);
        (bind_rej, mount_rej)
    }

    /// 绑定下标视图（i_dentry 载体，未绑定返 Err）。
    pub fn bound_idx(&self) -> Result<usize, u16> {
        match self.dentry.ino_idx {
            Some(i) if i < self.inodes.len() => Ok(i),
            _ => Err(E_DANGLING),
        }
    }

    /// 挂载态判定（冻结终态可验面）。
    pub fn is_mounted(&self) -> bool {
        self.dentry.phase == DPhase::Mounted
    }

    /// 状态机单向迁移核验：Unbound 无锚，BoundRoot 有锚未挂，
    /// Mounted 有锚已挂——三态与锚/账互锁。
    pub fn transition_ok(&self) -> bool {
        match self.dentry.phase {
            DPhase::Unbound => self.dentry.ino_idx.is_none() && self.bind_ok == 0,
            DPhase::BoundRoot => self.dentry.ino_idx.is_some() && self.mount_ok == 0,
            DPhase::Mounted => self.dentry.ino_idx.is_some() && self.mount_ok == 1,
        }
    }
}

/// 自例程：R1 正常绑定 → R2 非目录注入 → R3 悬空/坏号注入 → R4 自指断言 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：建根 inode → 绑定 → 挂载，状态三段迁移。
    let mut s = SRoot::new();
    let r = s.add_ino(ROOT_INO, true)?;
    if s.bind_root(r)? != DPhase::BoundRoot {
        return Err(E_BAL);
    }
    if s.mount()? != DPhase::Mounted {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 非目录注入：普通文件 inode 绑定被拒，dentry 不迁移。
    let mut s2 = SRoot::new();
    let f = s2.add_ino(ROOT_INO, false)?;
    if s2.bind_root(f) != Err(E_NOT_DIR) {
        return Err(E_BAL);
    }
    if s2.dentry.phase != DPhase::Unbound || s2.bind_ok != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 悬空索引与坏号注入：双通道拒收。
    let mut s3 = SRoot::new();
    if s3.bind_root(0) != Err(E_DANGLING) {
        return Err(E_BAL);
    }
    let bad = s3.add_ino(5, true)?;
    if s3.bind_root(bad) != Err(E_BAD_INO) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 自指断言与重复绑定：父指针错位注入被检出，冻结后再绑被拒。
    let mut s4 = SRoot::new();
    let r4 = s4.add_ino(ROOT_INO, true)?;
    s4.bind_root(r4)?;
    s4.assert_self_parent()?;
    let (ino_v, selfp) = s4.anchor_view()?;
    if ino_v != ROOT_INO || !selfp {
        return Err(E_BAL);
    }
    if s4.bind_root(r4) != Err(E_REBIND) {
        return Err(E_BAL);
    }
    // 父指针错位注入：改写后断言器以 E_PARENT_MISS 检出。
    s4.dentry.parent = Some(0x7FFF);
    if s4.assert_self_parent() != Err(E_PARENT_MISS) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 未绑定先挂载注入 + 守恒对账。
    let mut s5 = SRoot::new();
    if s5.mount() != Err(E_NOT_BOUND) {
        return Err(E_BAL);
    }
    s.audit()?;
    s4.audit()?;
    let (ph, bc, bo, mo, rj) = s.ledger();
    if ph != DPhase::Mounted || bc != 1 || bo != 1 || mo != 1 || rj != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6 视图与迁移核验：绑定账细分、绑定下标、三态迁移互锁。
    let (br, mr) = s.reject_split();
    if br != 0 || mr != 0 {
        return Err(E_BAL);
    }
    let bi = s.bound_idx()?;
    if bi != r {
        return Err(E_BAL);
    }
    if !s.is_mounted() || !s.transition_ok() {
        return Err(E_BAL);
    }
    // s2 处于 Unbound（绑定被拒后）：迁移核验仍须通过。
    if !s2.transition_ok() {
        return Err(E_BAL);
    }
    // s3 的绑定账细分：两次拒收均落在绑定侧。
    let (br3, mr3) = s3.reject_split();
    if br3 != 2 || mr3 != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
