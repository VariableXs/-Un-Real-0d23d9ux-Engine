//! unx4009_inodeobj — UNX-F4009 Inode 对象结构锚（诊断段 0x83E0）。
//!
//! 判据原文：
//!   J1 Inode 字段集（元数据核/i_op 指针/sb 反指/引用计数/页缓存锚/i_state）
//!      实例化断言通过。
//!   J2 「i_op 指针非空」不变式在 10,000 次随机创建中零违例。
//!   J3 引用计数账：iget/iput 成对，归零进 Free 态，重复释放被拒。
//!   J4 i_state 位段语义：Dirty/Writeback/Clean 三态互斥推进，跳跃被拒。
//! 设计纪律：
//!   1. i_op 操作表以槽位号（u16）承载，0 表示空表；实例化强制非空，
//!      这是 VFS 调用面「结构体方法表不可为 NULL」的纯功能投影。
//!   2. i_state 以位段 + 状态机双表达：枚举三态为主轴，位段仅作镜像，
//!      两者任何时刻必须一致（i_state 与 state 互校）。
//!   3. 引用计数上限 INODE_REF_MAX=1<<20，纯 u32 计数；归零即 Free，
//!      不再接受 iget（须走「重建」路径）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x83E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_IOP_NULL: u16 = SEG_BASE + 1;   // i_op 指针为空（不变式失守）
const E_SB_NULL: u16 = SEG_BASE + 2;    // sb 反指为空
const E_REF_OVER: u16 = SEG_BASE + 3;   // 引用计数超上限
const E_REF_ZERO: u16 = SEG_BASE + 4;   // 归零后再 iget / 重复 iput
const E_STATE_JUMP: u16 = SEG_BASE + 5; // i_state 迁移跳跃（非法序）
const E_META_BAD: u16 = SEG_BASE + 6;   // 元数据核字段非法

/// 引用计数上限（1<<20，VFS 活跃 inode 上限量级锚）。
pub const INODE_REF_MAX: u32 = 1 << 20;

/// i_state 位段镜像（Linux I_DIRTY_SYNC/I_DIRTY_DATASYNC/I_DIRTY_PAGES 简化为三态）。
pub const I_DIRTY: u32 = 0x01;     // 脏（待回写）
pub const I_WRITEBACK: u32 = 0x02; // 回写中
pub const I_CLEAN: u32 = 0x04;     // 干净
pub const I_STATE_MASK: u32 = I_DIRTY | I_WRITEBACK | I_CLEAN;

/// i_state 状态机：New → Dirty → Writeback → Clean → Dirty…
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IState {
    New = 0,       // 新建未落盘
    Dirty = 1,     // 脏页待回写
    Writeback = 2, // 回写中
    Clean = 3,     // 干净
    Free = 4,      // 已释放（终态）
}

impl IState {
    /// 位段镜像导出（与状态一一对应）。
    pub fn mirror(&self) -> u32 {
        match self {
            IState::New => I_CLEAN,
            IState::Dirty => I_DIRTY,
            IState::Writeback => I_WRITEBACK,
            IState::Clean => I_CLEAN,
            IState::Free => 0,
        }
    }
}

/// 页缓存锚：以页号区间承载（纯功能线不持有真实页）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageAnchor {
    pub first_page: u64, // 缓存区间首页号
    pub npages: u32,     // 缓存页数
    pub mapped: bool,    // 是否已建立映射
}

impl PageAnchor {
    pub fn empty() -> PageAnchor {
        PageAnchor { first_page: 0, npages: 0, mapped: false }
    }

    /// 建立映射：页数上限 1<<16，越界拒收。
    pub fn map(&mut self, first: u64, n: u32) -> Result<(), u16> {
        if n > (1 << 16) {
            return Err(E_META_BAD);
        }
        self.first_page = first;
        self.npages = n;
        self.mapped = true;
        Ok(())
    }
}

/// Inode 对象结构锚。
pub struct Inode {
    pub i_mode: u32,      // 类型 + 权限（元数据核）
    pub i_size: u64,      // 数据字节数
    pub i_nlink: u32,     // 硬链接数
    pub i_op: u16,        // 操作表指针（槽号，0=空）
    pub i_sb: u16,        // sb 反指（槽号，0=空）
    pub i_count: u32,     // 引用计数
    pub i_pages: PageAnchor, // 页缓存锚
    pub i_state: IState,  // 生命周期状态
    pub state_bits: u32,  // 位段镜像（须与 i_state 互校）
    pub igets: u32,       // iget 累计账
    pub iputs: u32,       // iput 累计账
}

impl Inode {
    /// 实例化：六字段全量校验，i_op/i_sb 非空为硬前置（判据 J1/J2）。
    pub fn new(i_mode: u32, i_op: u16, i_sb: u16) -> Result<Inode, u16> {
        if i_op == 0 {
            return Err(E_IOP_NULL);
        }
        if i_sb == 0 {
            return Err(E_SB_NULL);
        }
        if i_mode & !(0o170000 | 0o7777) != 0 {
            return Err(E_META_BAD);
        }
        Ok(Inode {
            i_mode,
            i_size: 0,
            i_nlink: 1,
            i_op,
            i_sb,
            i_count: 1,
            i_pages: PageAnchor::empty(),
            i_state: IState::New,
            state_bits: I_CLEAN,
            igets: 1,
            iputs: 0,
        })
    }

    /// iget：引用计数 +1，Free 态拒收，超上限拒收。
    pub fn iget(&mut self) -> Result<u32, u16> {
        if self.i_state == IState::Free {
            return Err(E_REF_ZERO);
        }
        if self.i_count >= INODE_REF_MAX {
            return Err(E_REF_OVER);
        }
        self.i_count += 1;
        self.igets += 1;
        Ok(self.i_count)
    }

    /// iput：引用计数 -1，归零进 Free；零态重复释放拒收。
    pub fn iput(&mut self) -> Result<u32, u16> {
        if self.i_count == 0 {
            return Err(E_REF_ZERO);
        }
        self.i_count -= 1;
        self.iputs += 1;
        if self.i_count == 0 {
            self.i_state = IState::Free;
            self.state_bits = 0;
            self.i_pages = PageAnchor::empty();
        }
        Ok(self.i_count)
    }

    /// 回写推进：Dirty → Writeback → Clean，New→Dirty 由 mark_dirty 承担。
    pub fn mark_dirty(&mut self) -> Result<IState, u16> {
        match self.i_state {
            IState::New | IState::Clean => {
                self.i_state = IState::Dirty;
                self.state_bits = I_DIRTY;
                Ok(self.i_state)
            }
            IState::Dirty => Ok(IState::Dirty), // 幂等
            _ => Err(E_STATE_JUMP),
        }
    }

    pub fn start_writeback(&mut self) -> Result<IState, u16> {
        if self.i_state != IState::Dirty {
            return Err(E_STATE_JUMP);
        }
        self.i_state = IState::Writeback;
        self.state_bits = I_WRITEBACK;
        Ok(self.i_state)
    }

    pub fn finish_writeback(&mut self) -> Result<IState, u16> {
        if self.i_state != IState::Writeback {
            return Err(E_STATE_JUMP);
        }
        self.i_state = IState::Clean;
        self.state_bits = I_CLEAN;
        Ok(self.i_state)
    }

    /// 位段与状态互校（判据 J4 一致性面）。
    pub fn state_consistent(&self) -> bool {
        if self.state_bits & !I_STATE_MASK != 0 {
            return false;
        }
        self.state_bits == self.i_state.mirror()
    }

    /// 引用账守恒：igets - iputs == i_count。
    pub fn refs_conserved(&self) -> bool {
        self.igets as i64 - self.iputs as i64 == self.i_count as i64
    }

    /// i_op 非空校验（判据 J2 不变式本体）。
    pub fn iop_nonnull(&self) -> bool {
        self.i_op != 0
    }
}

/// Inode 缓存：万次创建不变式检查 + 槽账。
pub struct InodeCache {
    pub slots: Vec<Inode>, // inode 槽表
    pub created: u32,      // 创建总数
    pub iop_viol: u32,     // i_op 空指针违例账（恒应为 0）
}

impl InodeCache {
    pub fn new() -> InodeCache {
        InodeCache { slots: Vec::new(), created: 0, iop_viol: 0 }
    }

    /// 创建并入槽：每次创建后复核 i_op 非空，违例入账（判据 J2）。
    pub fn create(&mut self, i_mode: u32, i_op: u16, i_sb: u16) -> Result<usize, u16> {
        let ino = Inode::new(i_mode, i_op, i_sb)?;
        if !ino.iop_nonnull() {
            self.iop_viol += 1;
            return Err(E_IOP_NULL);
        }
        self.slots.push(ino);
        self.created += 1;
        Ok(self.slots.len() - 1)
    }

    /// 释放：iput 到零后从槽账移除（保留槽位仅置 Free）。
    pub fn release(&mut self, idx: usize) -> Result<u32, u16> {
        if idx >= self.slots.len() {
            return Err(E_META_BAD);
        }
        let c = self.slots[idx].iput()?;
        Ok(c)
    }

    /// 违例账导出：判据 J2 要求 10,000 次创建后为 0。
    pub fn violations(&self) -> u32 {
        self.iop_viol
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_IOP_NULL => "i_op 指针为空（操作表不变式失守）",
        E_SB_NULL => "sb 反指为空",
        E_REF_OVER => "引用计数超上限 1<<20",
        E_REF_ZERO => "归零后再 iget 或重复 iput",
        E_STATE_JUMP => "i_state 迁移跳跃（非法序）",
        E_META_BAD => "元数据核字段非法",
        _ => "未知诊断码",
    }
}

/// 确定性演练：字段实例化 → 万次不变式 → 引用账 → 状态机 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut cache = InodeCache::new();

    // R1：实例化六字段并逐项断言（判据 J1）——REG 0o100644、i_op=7、sb=1。
    let mut ino = Inode::new(0o100644, 7, 1)?;
    if ino.i_mode != 0o100644 || ino.i_op != 7 || ino.i_sb != 1 {
        return Err(E_META_BAD);
    }
    if ino.i_count != 1 || ino.i_state != IState::New || ino.i_nlink != 1 {
        return Err(E_META_BAD);
    }
    if !ino.iop_nonnull() || !ino.state_consistent() {
        return Err(E_IOP_NULL);
    }
    hits += 1;

    // R2：页缓存锚建立映射（4 页）。
    ino.i_pages.map(0, 4)?;
    if !ino.i_pages.mapped || ino.i_pages.npages != 4 {
        return Err(E_META_BAD);
    }
    hits += 1;

    // R3：万次随机创建不变式零违例（判据 J2 原文）——确定性参数轮转。
    for n in 0..10_000u32 {
        let mode = match n % 3 {
            0 => 0o100644u32, // REG
            1 => 0o040755u32, // DIR
            _ => 0o120777u32, // LNK
        };
        let iop = (n % 60 + 1) as u16; // 1..=60，恒非空
        let sb = (n % 4 + 1) as u16;   // 1..=4
        cache.create(mode, iop, sb)?;
    }
    if cache.created != 10_000 || cache.violations() != 0 {
        return Err(E_IOP_NULL);
    }
    if cache.slots.len() != 10_000 {
        return Err(E_META_BAD);
    }
    hits += 1;

    // R4：引用计数账——iget 三次后 i_count=4，iput 三次回 1，账守恒。
    ino.iget()?;
    ino.iget()?;
    ino.iget()?;
    if ino.i_count != 4 || !ino.refs_conserved() {
        return Err(E_REF_OVER);
    }
    ino.iput()?;
    ino.iput()?;
    ino.iput()?;
    if ino.i_count != 1 || !ino.refs_conserved() {
        return Err(E_REF_OVER);
    }
    hits += 1;

    // R5：i_state 合法序 New→Dirty→Writeback→Clean，每步位段一致（判据 J4）。
    ino.mark_dirty()?;
    if ino.i_state != IState::Dirty || !ino.state_consistent() {
        return Err(E_STATE_JUMP);
    }
    ino.start_writeback()?;
    if ino.i_state != IState::Writeback || !ino.state_consistent() {
        return Err(E_STATE_JUMP);
    }
    ino.finish_writeback()?;
    if ino.i_state != IState::Clean || !ino.state_consistent() {
        return Err(E_STATE_JUMP);
    }
    hits += 1;

    // R6：Clean→Dirty 再回写循环一次（账面可续）。
    ino.mark_dirty()?;
    ino.start_writeback()?;
    ino.finish_writeback()?;
    if !ino.state_consistent() {
        return Err(E_STATE_JUMP);
    }
    hits += 1;

    // R7：归零进 Free——iput 到 0，状态 Free、页缓存清空。
    let _ = ino.iput();
    if ino.i_count != 0 || ino.i_state != IState::Free {
        return Err(E_REF_ZERO);
    }
    if ino.i_pages.mapped {
        return Err(E_META_BAD);
    }
    hits += 1;

    // I1：i_op=0 注入被拒（判据 J2 的构造面）。
    if Inode::new(0o100644, 0, 1).err() != Some(E_IOP_NULL) {
        return Err(E_IOP_NULL);
    }
    hits += 1;

    // I2：sb=0 注入被拒。
    if Inode::new(0o100644, 1, 0).err() != Some(E_SB_NULL) {
        return Err(E_SB_NULL);
    }
    hits += 1;

    // I3：Free 后再 iget / 重复 iput 双拒。
    if ino.iget() != Err(E_REF_ZERO) {
        return Err(E_REF_ZERO);
    }
    if ino.iput() != Err(E_REF_ZERO) {
        return Err(E_REF_ZERO);
    }
    hits += 1;

    // I4：状态跳跃注入——Clean 直跳 finish_writeback 被拒。
    let mut s = Inode::new(0o100644, 2, 1)?;
    if s.finish_writeback() != Err(E_STATE_JUMP) {
        return Err(E_STATE_JUMP);
    }
    if s.start_writeback() != Err(E_STATE_JUMP) {
        return Err(E_STATE_JUMP);
    }
    hits += 1;

    // R8：槽账末态复核——引用账在万次创建路径全程守恒。
    let mut ok = true;
    for ino2 in &cache.slots {
        if !ino2.refs_conserved() || !ino2.iop_nonnull() {
            ok = false;
            break;
        }
    }
    if !ok {
        return Err(E_REF_OVER);
    }
    hits += 1;

    Ok(hits)
}
