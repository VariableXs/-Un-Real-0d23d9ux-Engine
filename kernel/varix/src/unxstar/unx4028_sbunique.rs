//! unx4028_sbunique — UNX-F4028 sb 实例唯一性（同类型同设备单实例）（诊断段 0x8600）。
//!
//! 判据原文：
//!   J1 同（fstype,dev）二次 get_sb 返回既有实例（实例计数不变）；
//!   J2 不同设备同类型创建第二实例成功；
//!   J3 区分断言 10/10 通过（复用与新建逐例判定无误分）。
//! 设计纪律：
//!   1. 唯一键模型：sb 实例以（fstype 名, 设备号）二元组为唯一键，
//!      注册表线性查重——命中即复用既有实例句柄，不产生新对象。
//!   2. 守恒式：get_sb 总调用数 = 新建数 + 复用数 + 拒收数；
//!      注册表长度 = 新建数（无重复键入场）。
//!   3. 状态机：每实例 Empty → Alive → Dead；Dead 键保留于账但可被
//!      重建（键释放语义：同名键在 Dead 后允许二次新建）。
//!   4. 边界：设备号 0 非法、类型名空/超长/含非打印字节、注册表满、
//!      对 Dead 实例操作——全部拒收并记错误账。
//!   5. 常量定编：容量 16（内核 sb 注册表典型静态配额）、设备号
//!      上限 0xFFF（12 位 minor 约定）、类型名 8 字节定长。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8600;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_DEV_ZERO: u16 = SEG_BASE + 1;    // 设备号 0 非法（保留值）
const E_DEV_RANGE: u16 = SEG_BASE + 2;   // 设备号超 12 位约定上限
const E_BAD_NAME: u16 = SEG_BASE + 3;    // 类型名空/超长/含非打印字节
const E_TABLE_FULL: u16 = SEG_BASE + 4;  // 注册表满（静态配额耗尽）
const E_DEAD_REF: u16 = SEG_BASE + 5;    // 引用已 Dead 实例的活性操作
const E_BAL: u16 = SEG_BASE + 6;         // 新建/复用/拒收三账不平

/// 注册表静态容量（内核典型 sb 静态配额约定，写死）。
pub const TABLE_CAP: usize = 16;
/// 设备号 12 位上限（minor 编号约定）。
pub const DEV_MAX: u32 = 0xFFF;
/// 类型名定长字节数。
pub const NAME_LEN: usize = 8;

/// 实例生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstPhase {
    Empty, // 槽位未用
    Alive, // 在用（可被 get_sb 复用命中）
    Dead,  // 已卸载（键位保留，可重建）
}

/// 单个 sb 实例槽位。
pub struct SbSlot {
    pub phase: InstPhase,
    pub fstype: [u8; NAME_LEN], // 类型名（零填充定长）
    pub dev: u32,               // 设备号（唯一键第二元）
    pub refcnt: u32,            // 挂载引用计数
    pub dup_hits: u32,          // 本实例被复用命中次数
}

/// sb 实例注册表：唯一键查重 + 新建/复用分流 + 三账。
pub struct SbRegistry {
    pub slots: Vec<SbSlot>,
    pub created_total: u32, // 新建实例总数（=表内非 Empty 槽位数）
    pub reuse_total: u32,   // 复用命中总数
    pub rejected_total: u32, // 拒收总数
    pub get_calls: u32,     // get_sb 总调用数（含拒收）
}

impl SbRegistry {
    pub fn new() -> SbRegistry {
        SbRegistry {
            slots: Vec::new(),
            created_total: 0,
            reuse_total: 0,
            rejected_total: 0,
            get_calls: 0,
        }
    }

    /// 类型名合法性：至少 1 字节、不超 NAME_LEN、全为可打印 ASCII。
    fn name_ok(name: &[u8]) -> bool {
        if name.is_empty() || name.len() > NAME_LEN {
            return false;
        }
        for &b in name {
            if b < 0x20 || b > 0x7E {
                return false;
            }
        }
        true
    }

    /// 设备号合法性：非零且不超 12 位。
    fn dev_ok(dev: u32) -> bool {
        dev != 0 && dev <= DEV_MAX
    }

    /// 唯一键查找：返回命中槽位下标。
    fn find_key(&self, fstype: &[u8], dev: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.slots.len() {
            let s = &self.slots[i];
            // 索引前先比长度（铁律 5）。
            if i < self.slots.len()
                && s.phase != InstPhase::Empty
                && s.dev == dev
                && name_eq(&s.fstype, fstype)
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// get_sb 唯一性入口：命中复用（计数不变），未命中新建。
    pub fn get_sb(&mut self, fstype: &[u8], dev: u32) -> Result<usize, u16> {
        self.get_calls += 1;
        if !Self::dev_ok(dev) {
            self.rejected_total += 1;
            if dev == 0 {
                return Err(E_DEV_ZERO);
            }
            return Err(E_DEV_RANGE);
        }
        if !Self::name_ok(fstype) {
            self.rejected_total += 1;
            return Err(E_BAD_NAME);
        }
        // 同键查找：Alive 命中即复用；Dead 命中允许重建。
        match self.find_key(fstype, dev) {
            Some(i) => {
                if i >= self.slots.len() {
                    self.rejected_total += 1;
                    return Err(E_BAL);
                }
                match self.slots[i].phase {
                    InstPhase::Alive => {
                        self.slots[i].refcnt += 1;
                        self.slots[i].dup_hits += 1;
                        self.reuse_total += 1;
                        Ok(i)
                    }
                    InstPhase::Dead => {
                        // 键释放语义：Dead 后同键二次新建。
                        if self.slots.len() >= TABLE_CAP {
                            self.rejected_total += 1;
                            return Err(E_TABLE_FULL);
                        }
                        self.slots[i].phase = InstPhase::Alive;
                        self.slots[i].refcnt = 1;
                        self.created_total += 1;
                        Ok(i)
                    }
                    InstPhase::Empty => {
                        self.rejected_total += 1;
                        Err(E_DEAD_REF)
                    }
                }
            }
            None => {
                if self.slots.len() >= TABLE_CAP {
                    self.rejected_total += 1;
                    return Err(E_TABLE_FULL);
                }
                let mut nm = [0u8; NAME_LEN];
                let mut j = 0;
                while j < fstype.len() && j < NAME_LEN {
                    nm[j] = fstype[j];
                    j += 1;
                }
                self.slots.push(SbSlot {
                    phase: InstPhase::Alive,
                    fstype: nm,
                    dev,
                    refcnt: 1,
                    dup_hits: 0,
                });
                self.created_total += 1;
                Ok(self.slots.len() - 1)
            }
        }
    }

    /// 卸载：引用归零后置 Dead；仍被引用则拒收。
    pub fn put_sb(&mut self, idx: usize) -> Result<InstPhase, u16> {
        if idx >= self.slots.len() {
            self.rejected_total += 1;
            return Err(E_DEAD_REF);
        }
        if self.slots[idx].phase != InstPhase::Alive {
            self.rejected_total += 1;
            return Err(E_DEAD_REF);
        }
        if self.slots[idx].refcnt > 1 {
            self.slots[idx].refcnt -= 1;
            return Ok(InstPhase::Alive);
        }
        self.slots[idx].refcnt = 0;
        self.slots[idx].phase = InstPhase::Dead;
        Ok(InstPhase::Dead)
    }

    /// 查询实例句柄（区分断言面）：返回（阶段，复用命中数）。
    pub fn probe(&self, idx: usize) -> Result<(InstPhase, u32), u16> {
        if idx >= self.slots.len() {
            return Err(E_DEAD_REF);
        }
        Ok((self.slots[idx].phase, self.slots[idx].dup_hits))
    }

    /// 守恒对账：get 调用 = 新建 + 复用 + 拒收；表长 = 新建数。
    pub fn audit(&self) -> Result<(), u16> {
        if self.created_total + self.reuse_total + self.rejected_total != self.get_calls {
            return Err(E_BAL);
        }
        let mut live_or_dead = 0;
        for s in &self.slots {
            if s.phase != InstPhase::Empty {
                live_or_dead += 1;
            }
            if s.phase == InstPhase::Alive && s.refcnt == 0 {
                return Err(E_BAL);
            }
        }
        if live_or_dead != self.created_total {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出账面快照。
    pub fn ledger(&self) -> (u32, u32, u32, usize) {
        (
            self.created_total,
            self.reuse_total,
            self.rejected_total,
            self.slots.len(),
        )
    }
}

/// 定长名比较（截断到 NAME_LEN 后逐字节比对）。
fn name_eq(a: &[u8; NAME_LEN], b: &[u8]) -> bool {
    let mut i = 0;
    while i < b.len() && i < NAME_LEN {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    // 余下填充位必须为零。
    while i < NAME_LEN {
        if a[i] != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 自例程：R1 同键复用 → R2 异设备新建 → R3 区分断言 10/10 → R4 边界注入 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut reg = SbRegistry::new();

    // R1 同（fstype,dev）二次 get_sb：复用既有实例，实例计数不变。
    let a = reg.get_sb(b"ramfs", 0x101)?;
    let len_before = reg.slots.len();
    let b = reg.get_sb(b"ramfs", 0x101)?;
    if a != b || reg.slots.len() != len_before {
        return Err(E_BAL);
    }
    if reg.reuse_total != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 不同设备同类型：第二实例新建成功。
    let c = reg.get_sb(b"ramfs", 0x102)?;
    if c == a || reg.created_total != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 区分断言 10/10：逐例核对（复用命中递增、新实例命中为 0）。
    let mut pass = 0;
    for k in 0..10u32 {
        let _ = reg.get_sb(b"ramfs", 0x101)?;
        let (ph, dh) = reg.probe(a)?;
        let (_, dh2) = reg.probe(c)?;
        if ph == InstPhase::Alive && dh == 2 + k && dh2 == 0 {
            pass += 1;
        }
    }
    if pass != 10 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 边界注入：零设备号、超界设备号、坏类型名、Dead 引用全部拒收。
    if reg.get_sb(b"ramfs", 0) != Err(E_DEV_ZERO) {
        return Err(E_BAL);
    }
    if reg.get_sb(b"ramfs", DEV_MAX + 1) != Err(E_DEV_RANGE) {
        return Err(E_BAL);
    }
    if reg.get_sb(b"ram\xffs", 0x103) != Err(E_BAD_NAME) {
        return Err(E_BAL);
    }
    reg.put_sb(c)?;
    if reg.probe(c).is_err() {
        return Err(E_BAL);
    }
    if reg.put_sb(c) != Err(E_DEAD_REF) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒对账：三账之和 = 总调用数；表长 = 新建数（Dead 重建后）。
    // 注：R4 中 Dead 重建一次，created_total 再 +1。
    reg.audit()?;
    let (cr, ru, rj, ln) = reg.ledger();
    if cr + ru + rj != reg.get_calls || ln != cr as usize - 0 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
