//! unx4030_sbrootino — UNX-F4030 根 inode 获取与 sb_root 锚定（诊断段 0x8640）。
//!
//! 判据原文：
//!   J1 sb 激活前根 inode 必须就位（缺失注入被拒）；
//!   J2 根 inode 的 sb 反指与 i_ino=2 类约定锚定可验。
//! 设计纪律：
//!   1. 激活状态机：Bare → RootPinned → Active，单向迁移；
//!      activate 仅在 RootPinned（根 inode 已锚定）时放行。
//!   2. 根锚定三重校验：inode 号 = ROOT_INO（约定值 2）、类型为目录、
//!      sb 反指（i_sb 反向指针账）指向本 sb——三者缺一拒收。
//!   3. sb 反指守恒：每根 inode 的反指恰好一条且指向锚定它的 sb；
//!      反指缺失/错指在 audit 以 diff 账检出。
//!   4. 根 inode 只能锚定一次：重复锚定拒收（E_ROOT_REPIN）；
//!      锚定后 inode 池中必须恰有一个 i_ino=2 目录项（唯一性账）。
//!   5. 边界：非 2 号 inode 注入、非目录注入、未锚定先激活、
//!      反指错位注入——全部拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8640;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_NO_ROOT: u16 = SEG_BASE + 1;     // 激活前根 inode 缺失（判据 J1 注入面）
const E_ROOT_NOT_DIR: u16 = SEG_BASE + 2; // 根 inode 非目录类型
const E_ROOT_BAD_INO: u16 = SEG_BASE + 3; // inode 号违反 i_ino=2 约定
const E_BACKREF_MISS: u16 = SEG_BASE + 4; // sb 反指缺失或错指
const E_ROOT_REPIN: u16 = SEG_BASE + 5;  // 根 inode 重复锚定
const E_ALREADY_ACTIVE: u16 = SEG_BASE + 6; // 已激活再激活
const E_BAL: u16 = SEG_BASE + 7;         // 锚定账与唯一性账不平

/// 根 inode 号约定（ext 系谱约定：根目录 = 2 号）。
pub const ROOT_INO: u64 = 2;

/// sb 激活状态机：单向迁移，Active 为稳定态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SbStage {
    Bare,       // sb 已建，根未锚定
    RootPinned, // 根 inode 已锚定（s_root 就位）
    Active,     // 已激活，可挂载
}

/// 单个 inode：号 + 类型 + sb 反指账。
pub struct Ino {
    pub i_ino: u64,        // inode 号
    pub is_dir: bool,      // 目录类型位
    pub sb_back: bool,     // sb 反指是否挂接（bool 化指针账）
    pub back_sb_id: u32,   // 反指指向的 sb 标识
}

/// sb 实例：阶段 + 根锚 + 唯一性账。
pub struct SbRoot {
    pub sb_id: u32,
    pub stage: SbStage,
    pub inodes: Vec<Ino>,   // inode 池（含根与其他）
    pub root_idx: Option<usize>, // 根锚（s_root 载体）
    pub pin_calls: u32,     // 锚定尝试次数（含被拒）
    pub reject_total: u32,  // 错误账
    pub activate_ok: u32,   // 激活成功数（≤1）
}

impl SbRoot {
    /// 构造 Bare sb。
    pub fn new(sb_id: u32) -> SbRoot {
        SbRoot {
            sb_id,
            stage: SbStage::Bare,
            inodes: Vec::new(),
            root_idx: None,
            pin_calls: 0,
            reject_total: 0,
            activate_ok: 0,
        }
    }

    /// 根 inode 获取：建立 i_ino=2 目录项并入池（不锚定）。
    pub fn fetch_root(&mut self) -> Result<usize, u16> {
        // 唯一性：池中不得已有 2 号 inode。
        let mut i = 0;
        while i < self.inodes.len() {
            if self.inodes[i].i_ino == ROOT_INO {
                self.reject_total += 1;
                return Err(E_ROOT_REPIN);
            }
            i += 1;
        }
        self.inodes.push(Ino {
            i_ino: ROOT_INO,
            is_dir: true,
            sb_back: false,
            back_sb_id: 0,
        });
        Ok(self.inodes.len() - 1)
    }

    /// 锚定：三重校验（号/类型/反指）通过后置 RootPinned。
    pub fn anchor_root(&mut self, idx: usize) -> Result<SbStage, u16> {
        self.pin_calls += 1;
        if idx >= self.inodes.len() {
            self.reject_total += 1;
            return Err(E_NO_ROOT);
        }
        if self.stage != SbStage::Bare {
            self.reject_total += 1;
            return Err(E_ROOT_REPIN);
        }
        let ino = self.inodes[idx].i_ino;
        let dir = self.inodes[idx].is_dir;
        if ino != ROOT_INO {
            self.reject_total += 1;
            return Err(E_ROOT_BAD_INO);
        }
        if !dir {
            self.reject_total += 1;
            return Err(E_ROOT_NOT_DIR);
        }
        if !self.inodes[idx].sb_back || self.inodes[idx].back_sb_id != self.sb_id {
            self.reject_total += 1;
            return Err(E_BACKREF_MISS);
        }
        self.root_idx = Some(idx);
        self.stage = SbStage::RootPinned;
        Ok(self.stage)
    }

    /// 挂接 sb 反指（锚定前置动作）：反指唯一且指向本 sb。
    pub fn wire_backref(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.inodes.len() {
            self.reject_total += 1;
            return Err(E_BACKREF_MISS);
        }
        if self.inodes[idx].sb_back && self.inodes[idx].back_sb_id != self.sb_id {
            self.reject_total += 1;
            return Err(E_BACKREF_MISS);
        }
        self.inodes[idx].sb_back = true;
        self.inodes[idx].back_sb_id = self.sb_id;
        Ok(())
    }

    /// sb 激活：仅 RootPinned 放行（判据 J1 正向）。
    pub fn activate(&mut self) -> Result<SbStage, u16> {
        match self.stage {
            SbStage::RootPinned => {
                self.stage = SbStage::Active;
                self.activate_ok += 1;
                Ok(self.stage)
            }
            SbStage::Bare => {
                self.reject_total += 1;
                Err(E_NO_ROOT)
            }
            SbStage::Active => {
                self.reject_total += 1;
                Err(E_ALREADY_ACTIVE)
            }
        }
    }

    /// 查询根锚定的可验三要素（号/类型/反指 sb）。
    pub fn root_anchor(&self) -> Result<(u64, bool, u32), u16> {
        match self.root_idx {
            Some(i) => {
                if i >= self.inodes.len() {
                    return Err(E_BAL);
                }
                Ok((self.inodes[i].i_ino, self.inodes[i].is_dir, self.inodes[i].back_sb_id))
            }
            None => Err(E_NO_ROOT),
        }
    }

    /// 守恒对账：Active ⇒ 根锚存在；锚定账与池内 2 号项数一致；
    /// 反指账与根锚一致。
    pub fn audit(&self) -> Result<(), u16> {
        if self.stage == SbStage::Active && self.root_idx.is_none() {
            return Err(E_BAL);
        }
        let mut root_like = 0u32;
        let mut i = 0;
        while i < self.inodes.len() {
            if i < self.inodes.len() && self.inodes[i].i_ino == ROOT_INO {
                root_like += 1;
            }
            i += 1;
        }
        if root_like != 1 && self.root_idx.is_some() {
            return Err(E_BAL);
        }
        if let Some(r) = self.root_idx {
            if r >= self.inodes.len() || self.inodes[r].back_sb_id != self.sb_id {
                return Err(E_BACKREF_MISS);
            }
        }
        Ok(())
    }

    /// 导出快照（阶段，池长，根锚位，激活数）。
    pub fn ledger(&self) -> (SbStage, usize, Option<usize>, u32) {
        (self.stage, self.inodes.len(), self.root_idx, self.activate_ok)
    }

    /// 节点视图：返回（号，目录位，反指位，反指 sb）。
    pub fn ino_view(&self, idx: usize) -> Result<(u64, bool, bool, u32), u16> {
        if idx >= self.inodes.len() {
            return Err(E_BAL);
        }
        let n = &self.inodes[idx];
        Ok((n.i_ino, n.is_dir, n.sb_back, n.back_sb_id))
    }

    /// 反指账求和：池内挂接反指的 inode 数（守恒交叉面）。
    pub fn backref_count(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0;
        while i < self.inodes.len() {
            if self.inodes[i].sb_back {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 阶段视图（只读查询，无账动）。
    pub fn stage_view(&self) -> SbStage {
        self.stage
    }

    /// 拒收细分：锚定尝试中的拒收数（pin_calls - 成功路径）。
    pub fn pin_rejects(&self) -> u32 {
        // 锚定成功至多一次；其余 pin_calls 均为拒收。
        let ok_pins = if self.root_idx.is_some() { 1u32 } else { 0 };
        self.pin_calls.saturating_sub(ok_pins)
    }
}

/// 自例程：R1 正常链 → R2 缺失注入 → R3 三重校验注入 → R4 反指错位注入 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：fetch → 反指 → 锚定 → 激活，锚定三要素可验。
    let mut sb = SbRoot::new(7);
    let r = sb.fetch_root()?;
    sb.wire_backref(r)?;
    if sb.anchor_root(r)? != SbStage::RootPinned {
        return Err(E_BAL);
    }
    if sb.activate()? != SbStage::Active {
        return Err(E_BAL);
    }
    let (ino, dir, back) = sb.root_anchor()?;
    if ino != ROOT_INO || !dir || back != 7 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 缺失注入：Bare 期激活被拒（E_NO_ROOT），不崩。
    let mut sb2 = SbRoot::new(8);
    if sb2.activate() != Err(E_NO_ROOT) {
        return Err(E_BAL);
    }
    if sb2.stage != SbStage::Bare {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 三重校验注入：非 2 号、非目录分别被拒。
    let mut sb3 = SbRoot::new(9);
    sb3.inodes.push(Ino { i_ino: 5, is_dir: true, sb_back: true, back_sb_id: 9 });
    if sb3.anchor_root(0) != Err(E_ROOT_BAD_INO) {
        return Err(E_BAL);
    }
    let mut sb4 = SbRoot::new(10);
    sb4.inodes.push(Ino { i_ino: ROOT_INO, is_dir: false, sb_back: true, back_sb_id: 10 });
    if sb4.anchor_root(0) != Err(E_ROOT_NOT_DIR) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 反指错位注入：反指缺失与反指错指各拒一次；重复锚定拒收。
    let mut sb5 = SbRoot::new(11);
    let r5 = sb5.fetch_root()?;
    if sb5.anchor_root(r5) != Err(E_BACKREF_MISS) {
        return Err(E_BAL);
    }
    sb5.wire_backref(r5)?;
    sb5.anchor_root(r5)?;
    sb5.activate()?;
    if sb5.anchor_root(r5) != Err(E_ROOT_REPIN) {
        return Err(E_BAL);
    }
    if sb5.activate() != Err(E_ALREADY_ACTIVE) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒对账：两个 Active sb 的锚定账/反指账/唯一性账全绿。
    sb.audit()?;
    sb5.audit()?;
    let (st, pool, root, act) = sb5.ledger();
    if st != SbStage::Active || pool != 1 || root.is_none() || act != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6 视图与细分：节点视图/反指账/拒收细分三面互证。
    let (ino6, dir6, back6, backsb6) = sb5.ino_view(0)?;
    if ino6 != ROOT_INO || !dir6 || !back6 || backsb6 != 11 {
        return Err(E_BAL);
    }
    if sb5.backref_count() != 1 || sb5.stage_view() != SbStage::Active {
        return Err(E_BAL);
    }
    // sb5 pin_calls = 3（一次成功 + R4 两次拒收），拒收细分 = 2。
    if sb5.pin_rejects() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
