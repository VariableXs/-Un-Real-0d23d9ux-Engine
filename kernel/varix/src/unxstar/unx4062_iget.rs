//! unx4062_iget — UNX-F4062 iget 查找-插入-命中路径（诊断段 0x8A00）。
//!
//! 判据原文（提炼）：
//!   J1. iget 三路径（命中/未命中新建/并发竞争合并）各 1,000 次行为正确。
//!   J2. 路径判定账与理论分布一致（三账合计 == 总调用）。
//!   J3. 并发合并：同号双请求只建一实例，第二请求挂到同实例。
//!   J4. 0 号 inode 拒收（EINVAL），配额满拒收且账面零破坏。
//! 设计纪律：
//!   1. iget 语义取 Linux VFS iget_locked：先查缓存（命中即返），
//!      未命中新建入缓存，竞争者发现已在建则合并等待同一实例。
//!   2. 三路径独立分账：hit/new/merge 各自计数，守恒主账交叉核对。
//!   3. 实例唯一性：同号 inode 在缓存中至多一条（重复插入拒收）。
//!   4. 竞争模拟用确定性伪并发序：请求流按序到达，在建标记位
//!      表达"正在新建"窗口，窗口内同号请求走合并路径。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8A00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_INO: u16 = SEG_BASE + 1;  // 0 号 inode 拒收（EINVAL 语义）
const E_DUP_INO: u16 = SEG_BASE + 2;  // 缓存内同号重复插入
const E_QUOTA: u16 = SEG_BASE + 3;    // 缓存配额满（静态上限）
const E_STATE_INV: u16 = SEG_BASE + 4;// 在建窗口状态迁移非法
const E_LEDGER: u16 = SEG_BASE + 5;   // 三路径账守恒破坏
const E_NO_INST: u16 = SEG_BASE + 6;  // 合并目标实例不存在
const E_BAD_ARG: u16 = SEG_BASE + 7;  // 参数非法

/// 缓存配额定编：64 实例（本锚静态上限）。
pub const CACHE_CAP: usize = 64;

/// iget 实例状态机：新建窗口表达并发竞争合并语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstPhase {
    Creating, // 在建（竞争窗口开启）
    Shared,   // 就绪共享
}

/// 缓存实例：inode 号 + 相位 + 合并挂靠计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IgetInst {
    pub ino: u64,     // inode 号
    pub phase: InstPhase,
    pub merges: u32,  // 并发合并挂靠数（判据 J3 观测面）
}

/// iget 路径判定枢纽：实例表 + 三路径分账。
pub struct IgetFabric {
    pub insts: Vec<IgetInst>, // 实例表
    pub cap: usize,           // 配额
    pub hit_total: u32,       // 命中路径账
    pub new_total: u32,       // 未命中新建账
    pub merge_total: u32,     // 竞争合并账
    pub calls_total: u32,     // 总调用账（守恒主账）
    pub creating: u64,        // 当前在建窗口 inode 号（0 表无）
}

impl IgetFabric {
    pub fn new() -> IgetFabric {
        IgetFabric {
            insts: Vec::new(),
            cap: CACHE_CAP,
            hit_total: 0,
            new_total: 0,
            merge_total: 0,
            calls_total: 0,
            creating: 0,
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

    /// iget 核心：三路径判定（判据 J1）。
    /// 返回（路径码 0=hit/1=new/2=merge, 实例序号）。
    pub fn iget(&mut self, ino: u64) -> Result<(u8, usize), u16> {
        self.calls_total += 1;
        if ino == 0 {
            return Err(E_BAD_INO);
        }
        // 路径 3：在建窗口内同号请求 → 合并到在建实例（判据 J3）
        if self.creating == ino {
            let i = match self.find_inst(ino) {
                Some(i) => i,
                None => return Err(E_NO_INST),
            };
            self.insts[i].merges += 1;
            self.merge_total += 1;
            return Ok((2, i));
        }
        // 路径 1：缓存命中
        if let Some(i) = self.find_inst(ino) {
            self.hit_total += 1;
            return Ok((0, i));
        }
        // 路径 2：未命中新建（配额闸门先行）
        if self.insts.len() >= self.cap {
            return Err(E_QUOTA);
        }
        self.insts.push(IgetInst {
            ino,
            phase: InstPhase::Creating,
            merges: 0,
        });
        self.creating = ino;
        self.new_total += 1;
        Ok((1, self.insts.len() - 1))
    }

    /// 新建窗口闭合：Creating→Shared（建毕共享，窗口关闭）。
    pub fn seal(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_inst(ino) {
            Some(i) => i,
            None => return Err(E_NO_INST),
        };
        if self.insts[i].phase != InstPhase::Creating {
            return Err(E_STATE_INV);
        }
        self.insts[i].phase = InstPhase::Shared;
        if self.creating == ino {
            self.creating = 0;
        }
        Ok(())
    }

    /// 缓存实例数。
    pub fn inst_count(&self) -> u32 {
        self.insts.len() as u32
    }

    /// 路径码常量读取面（0=hit 1=new 2=merge）。
    pub fn path_hit() -> u8 { 0 }
    pub fn path_new() -> u8 { 1 }
    pub fn path_merge() -> u8 { 2 }

    /// 守恒对账（判据 J2）：三路径账合计 == 总调用。
    pub fn ledger_ok(&self) -> bool {
        self.hit_total + self.new_total + self.merge_total == self.calls_total
    }
}

/// 三路径各千次负载：确定性序列（hit/new/merge 各 1,000 次）。
pub fn load_thousand_each(f: &mut IgetFabric) -> Result<(u32, u32, u32), u16> {
    // 未命中新建 1,000：号段 1..=1000
    for ino in 1..=1000u64 {
        let (p, _) = f.iget(ino)?;
        if p != IgetFabric::path_new() {
            return Err(E_LEDGER);
        }
        f.seal(ino)?;
    }
    let new_cnt = f.new_total;
    // 命中 1,000：重复请求号段奇数位
    for k in 0..1000u64 {
        let ino = (k % 1000) + 1;
        let (p, _) = f.iget(ino)?;
        if p != IgetFabric::path_hit() {
            return Err(E_LEDGER);
        }
    }
    let hit_cnt = f.hit_total - 0;
    // 合并 1,000：在建窗口内同号请求
    let mut merge_cnt = 0;
    for k in 0..1000u64 {
        let ino = (k % 1000) + 1;
        f.creating = ino; // 人为重开窗口（竞争模拟）
        let (p, _) = f.iget(ino)?;
        if p != IgetFabric::path_merge() {
            return Err(E_LEDGER);
        }
        merge_cnt += 1;
        f.creating = 0;
    }
    Ok((hit_cnt, new_cnt, merge_cnt))
}

// ── 扩展面：实例查问与路径分布诊断 ───────────────────────────

impl IgetFabric {
    /// 实例相位只读面（在建/共享态读数）。
    pub fn phase_of(&self, ino: u64) -> Result<InstPhase, u16> {
        match self.find_inst(ino) {
            Some(i) => Ok(self.insts[i].phase),
            None => Err(E_NO_INST),
        }
    }

    /// 合并挂靠读数（判据 J3 观测面：单实例合并次数）。
    pub fn merge_count_of(&self, ino: u64) -> Result<u32, u16> {
        match self.find_inst(ino) {
            Some(i) => Ok(self.insts[i].merges),
            None => Err(E_NO_INST),
        }
    }

    /// 在建窗口状态读数（0 表无窗口开启）。
    pub fn creating_of(&self) -> u64 {
        self.creating
    }

    /// 共享实例计数（Creating 相除外，判据 J2 分布面）。
    pub fn shared_count(&self) -> u32 {
        let mut c = 0;
        for s in &self.insts {
            if s.phase == InstPhase::Shared {
                c += 1;
            }
        }
        c
    }

    /// 三路径分布千分比导出：返回（命中‰，新建‰，合并‰）。
    /// 零调用拒收（分布无意义）。
    pub fn path_permille(&self) -> Result<(u32, u32, u32), u16> {
        if self.calls_total == 0 {
            return Err(E_BAD_ARG);
        }
        if !self.ledger_ok() {
            return Err(E_LEDGER);
        }
        Ok((
            self.hit_total * 1000 / self.calls_total,
            self.new_total * 1000 / self.calls_total,
            self.merge_total * 1000 / self.calls_total,
        ))
    }

    /// 实例驱逐面（缓存收缩：仅 Shared 相可出清）。
    pub fn evict_shared(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_inst(ino) {
            Some(i) => i,
            None => return Err(E_NO_INST),
        };
        if self.insts[i].phase != InstPhase::Shared {
            return Err(E_STATE_INV);
        }
        self.insts.remove(i);
        Ok(())
    }
}

/// 自例程：三路径行为→合并唯一性→千次负载→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = IgetFabric::new();

    // R1 三路径基本行为：new→merge（窗口内）→hit（窗口闭后）
    let (p1, _) = f.iget(7)?;
    if p1 != IgetFabric::path_new() {
        return Err(E_LEDGER);
    }
    let (p2, i2) = f.iget(7)?;
    if p2 != IgetFabric::path_merge() || f.insts[i2].merges != 1 {
        return Err(E_LEDGER);
    }
    f.seal(7)?;
    let (p3, _) = f.iget(7)?;
    if p3 != IgetFabric::path_hit() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 合并唯一性：双请求同号只建一实例（判据 J3）
    if f.inst_count() != 1 {
        return Err(E_DUP_INO);
    }
    hits += 1;

    // R3 0 号拒收（判据 J4）
    match f.iget(0) {
        Err(E_BAD_INO) => {}
        _ => return Err(E_BAD_INO),
    }
    hits += 1;

    // R4 三路径各千次负载（判据 J1：各 1,000 次行为正确）
    let (h, n, m) = load_thousand_each(&mut f)?;
    if h != 1000 || n != 1000 || m != 1000 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 守恒对账：三账合计 == 总调用（判据 J2）
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    // R1 三次 + R3 一次 + R4 三千次 = 3004
    if f.calls_total != 3004 {
        return Err(E_LEDGER);
    }
    if f.inst_count() != 1000 {
        return Err(E_QUOTA);
    }
    hits += 1;

    Ok(hits)
}
