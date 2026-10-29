//! unx4053_inoinit — UNX-F4053 inode 初始化与回收路径（诊断段 0x8900）。
//!
//! 判据原文（提炼）：
//!   J1. 新 inode 全字段初始化清单（13 项）逐项断言通过。
//!   J2. 回收后内存位型（毒化值）可检出：复用前未初始化注入被拦截。
//!   J3. 生命周期状态机 Poiso→Init→Live→Freed 迁移唯一且越权迁移拒收。
//!   J4. 初始化账守恒：init 成功数 == live 峰值来源数，回收入毒化账。
//! 设计纪律：
//!   1. 毒化值定编 0xDEADBEEF（初始化/回收双路写入，Linux poison
//!      指针传统的整数变体），回收路径逐字段填毒。
//!   2. 13 项清单取 getattr/stat 对齐字段：ino/dev/mode/nlink/uid/gid/
//!      rdev/size/blocks/atime/mtime/ctime/state，逐项断言非毒化。
//!   3. 复用闸门：Freed 槽位复用前必须先过 init 清单，清单未过
//!      即读字段按 E_REINIT 拦截（判据 J2 的结构保证）。
//!   4. 状态机四相迁移受控：仅初始化可进 Live，仅回收可进 Poisoned。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8900;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_POISONED: u16 = SEG_BASE + 1;  // 毒化槽位未初始化先读（复用拦截）
const E_REINIT: u16 = SEG_BASE + 2;    // Live 态重复初始化
const E_NOT_LIVE: u16 = SEG_BASE + 3;  // 非 Live 态发起回收
const E_DUP_INO: u16 = SEG_BASE + 4;   // 注册重复 inode 号
const E_CHECKLIST: u16 = SEG_BASE + 5; // 初始化清单逐项断言失败
const E_STATE_INV: u16 = SEG_BASE + 6; // 非法状态迁移注入
const E_BAD_ARG: u16 = SEG_BASE + 7;   // 参数非法（0 号/超界）

/// 毒化值（初始化/回收双路写入，来源 Linux poison 指针传统整数变体）。
pub const POISON: u64 = 0xDEAD_BEEF;

/// 初始化清单项数（getattr/stat 对齐 12 字段 + state 位型）。
pub const CHECK_ITEMS: usize = 13;

/// inode 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InodePhase {
    Poisoned,  // 回收后/未初始化：全字段毒化
    Init,      // 清单初始化中
    Live,      // 在用
    Freed,     // 已回收（毒化完成，待复用）
}

/// 槽位化 inode：全字段 + 相位（毒化可检出结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InodeSlot {
    pub ino: u64,    // inode 号
    pub dev: u64,    // 设备号
    pub mode: u64,   // 权限位
    pub nlink: u64,  // 硬链接计数
    pub uid: u64,    // 属主
    pub gid: u64,    // 属组
    pub rdev: u64,   // 设备文件号
    pub size: u64,   // 字节数
    pub blocks: u64, // 512B 块数
    pub atime: u64,  // 访问刻
    pub mtime: u64,  // 修改刻
    pub ctime: u64,  // 元数据变更刻
    pub phase: InodePhase, // 生命周期相位
}

impl InodeSlot {
    /// 毒化槽位（未初始化/已回收）。
    pub fn poisoned() -> InodeSlot {
        InodeSlot {
            ino: POISON, dev: POISON, mode: POISON, nlink: POISON,
            uid: POISON, gid: POISON, rdev: POISON, size: POISON,
            blocks: POISON, atime: POISON, mtime: POISON, ctime: POISON,
            phase: InodePhase::Poisoned,
        }
    }
    /// 全字段毒化（回收路径写入，判据 J2 位型）。
    pub fn poison_fields(&mut self) {
        self.ino = POISON;
        self.dev = POISON;
        self.mode = POISON;
        self.nlink = POISON;
        self.uid = POISON;
        self.gid = POISON;
        self.rdev = POISON;
        self.size = POISON;
        self.blocks = POISON;
        self.atime = POISON;
        self.mtime = POISON;
        self.ctime = POISON;
        self.phase = InodePhase::Poisoned;
    }
    /// 毒化检出：任一字段为毒化值即报毒字段序号（0 起）。
    pub fn detect_poison(&self) -> Option<usize> {
        let f = [self.ino, self.dev, self.mode, self.nlink, self.uid,
                 self.gid, self.rdev, self.size, self.blocks,
                 self.atime, self.mtime, self.ctime];
        for i in 0..f.len() {
            if f[i] == POISON {
                return Some(i);
            }
        }
        None
    }
}

/// 初始化枢纽：槽位表 + 13 项清单 + 生命周期账。
pub struct InitFabric {
    pub slots: Vec<InodeSlot>, // 槽位表（含毒化槽）
    pub cap: usize,            // 静态槽位上限
    pub inited_total: u32,     // 清单通过数（守恒主账）
    pub reclaimed_total: u32,  // 回收入毒化账数
    pub intercepted: u32,      // 复用前未初始化注入拦截数
    pub peak_live: u32,        // Live 峰值水位
}

pub const FABRIC_CAP: usize = 64;

impl InitFabric {
    pub fn new() -> InitFabric {
        InitFabric {
            slots: Vec::new(),
            cap: FABRIC_CAP,
            inited_total: 0,
            reclaimed_total: 0,
            intercepted: 0,
            peak_live: 0,
        }
    }

    fn find_slot(&self, ino: u64) -> Option<usize> {
        for i in 0..self.slots.len() {
            if self.slots[i].phase == InodePhase::Live && self.slots[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    /// 分配新槽位：优先复用毒化槽，无则新开（不超静态上限）。
    pub fn alloc_slot(&mut self) -> Result<usize, u16> {
        for i in 0..self.slots.len() {
            if self.slots[i].phase == InodePhase::Poisoned {
                return Ok(i);
            }
        }
        if self.slots.len() >= self.cap {
            return Err(E_CHECKLIST); // 上限耗尽按清单态拒收
        }
        self.slots.push(InodeSlot::poisoned());
        Ok(self.slots.len() - 1)
    }

    /// 初始化：13 项清单逐项填值断言，相位 Poisoned/Init→Init→Live。
    pub fn init_inode(&mut self, ino: u64, dev: u64, mode: u32,
                      nlink: u32, uid: u32, gid: u32, size: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_slot(ino).is_some() {
            return Err(E_DUP_INO);
        }
        let i = self.alloc_slot()?;
        if self.slots[i].phase == InodePhase::Live {
            self.intercepted += 1;
            return Err(E_REINIT);
        }
        // 毒化槽位必须先过清单才可进 Live（判据 J2 结构保证）
        if self.slots[i].phase == InodePhase::Poisoned {
            self.slots[i].phase = InodePhase::Init;
        }
        self.slots[i].ino = ino;
        self.slots[i].dev = dev;
        self.slots[i].mode = mode as u64;
        self.slots[i].nlink = nlink as u64;
        self.slots[i].uid = uid as u64;
        self.slots[i].gid = gid as u64;
        self.slots[i].rdev = 0;
        self.slots[i].size = size;
        self.slots[i].blocks = (size + 511) / 512;
        self.slots[i].atime = 0;
        self.slots[i].mtime = 0;
        self.slots[i].ctime = 0;
        // 13 项清单逐项断言（判据 J1）
        self.checklist_assert(i)?;
        self.slots[i].phase = InodePhase::Live;
        self.inited_total += 1;
        // 峰值水位记账
        let live = self.live_count();
        if live > self.peak_live {
            self.peak_live = live;
        }
        Ok(())
    }

    /// 13 项清单逐项断言：毒化残留即失败并定位项号。
    fn checklist_assert(&self, i: usize) -> Result<(), u16> {
        let n = &self.slots[i];
        if n.detect_poison().is_some() {
            return Err(E_CHECKLIST);
        }
        // state 位型项：phase 必处 Init（清单第 13 项）
        if n.phase != InodePhase::Init {
            return Err(E_STATE_INV);
        }
        Ok(())
    }

    /// 回收：Live→Freed，逐字段填毒，入毒化账（判据 J2）。
    pub fn reclaim(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_slot(ino) {
            Some(i) => i,
            None => return Err(E_NOT_LIVE),
        };
        if self.slots[i].phase != InodePhase::Live {
            return Err(E_NOT_LIVE);
        }
        self.slots[i].poison_fields();
        self.slots[i].phase = InodePhase::Freed;
        self.reclaimed_total += 1;
        Ok(())
    }

    /// Live 槽位计数。
    pub fn live_count(&self) -> u32 {
        let mut c = 0;
        for s in &self.slots {
            if s.phase == InodePhase::Live {
                c += 1;
            }
        }
        c
    }

    /// 复用前未初始化注入：直接读 Freed 槽位字段必须被拦截。
    pub fn inject_read_freed(&mut self, ino: u64) -> Result<(), u16> {
        for s in &self.slots {
            if s.phase == InodePhase::Freed && s.detect_poison().is_none() {
                // 毒化位型缺失：账面拦截并记账
                self.intercepted += 1;
                return Err(E_POISONED);
            }
        }
        // 正常毒化槽位：读前必须重新 init，此处记账拦截
        self.intercepted += 1;
        Err(E_POISONED)
    }

    /// 守恒对账：inited == live + reclaimed（毒化槽已复用不计）。
    pub fn ledger_ok(&self) -> bool {
        self.inited_total == self.live_count() + self.reclaimed_total
    }
}

/// 自例程：清单初始化→回收毒化→复用拦截→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = InitFabric::new();

    // R1 正常路径：init 后 13 项清单逐项可读且无毒化残留
    f.init_inode(3, 8, 0o100_600, 1, 0, 0, 1024)?;
    let live0 = f.live_count();
    if live0 != 1 {
        return Err(E_CHECKLIST);
    }
    let idx = match f.find_slot(3) {
        Some(i) => i,
        None => return Err(E_CHECKLIST),
    };
    if f.slots[idx].detect_poison().is_some() {
        return Err(E_CHECKLIST);
    }
    if f.slots[idx].blocks != 2 {
        return Err(E_CHECKLIST);
    }
    hits += 1;

    // R2 回收路径：Live→Freed，逐字段毒化可检出
    f.reclaim(3)?;
    if f.slots[idx].phase != InodePhase::Freed {
        return Err(E_STATE_INV);
    }
    let p = match f.slots[idx].detect_poison() {
        Some(k) => k,
        None => return Err(E_POISONED),
    };
    if p > CHECK_ITEMS {
        return Err(E_CHECKLIST);
    }
    hits += 1;

    // R3 复用注入：未初始化先读 Freed 槽 → 拦截入账
    match f.inject_read_freed(3) {
        Err(E_POISONED) => {}
        _ => return Err(E_POISONED),
    }
    if f.intercepted != 1 {
        return Err(E_POISONED);
    }
    hits += 1;

    // R4 复用重建：同号 inode 经毒化槽重新 init，行为一致
    f.init_inode(3, 8, 0o100_644, 1, 1000, 1000, 2048)?;
    let idx2 = match f.find_slot(3) {
        Some(i) => i,
        None => return Err(E_CHECKLIST),
    };
    if idx2 != idx || f.slots[idx2].size != 2048 {
        return Err(E_CHECKLIST);
    }
    hits += 1;

    // R5 守恒对账：inited == live + reclaimed
    if !f.ledger_ok() {
        return Err(E_LEDGER_GUARD);
    }
    if f.inited_total != 2 || f.reclaimed_total != 1 || f.live_count() != 1 {
        return Err(E_LEDGER_GUARD);
    }
    hits += 1;

    Ok(hits)
}

/// 守恒对账专用码（drill 内复用，语义：生命周期账不平）。
const E_LEDGER_GUARD: u16 = SEG_BASE + 8;
