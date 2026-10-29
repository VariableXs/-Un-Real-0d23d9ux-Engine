//! unx4014_mntns — UNX-F4014 mnt_namespace 对象结构锚（诊断段 0x8480）。
//!
//! 判据原文：
//!   J1 命名空间对象（root mnt/mount 表/引用计数）实例化断言通过。
//!   J2 初始命名空间全局唯一不变式成立：创建第二个初始 ns 的注入被拒绝。
//!   J3 mount 表语义：加入/移出成对，表内槽号唯一，同一 mount 不得重复加入。
//!   J4 引用计数：ns 被进程组引用时 +1，全部退出后归零进 Free，重复释放拒收。
//! 设计纪律：
//!   1. 初始命名空间以「全局单例标志」建模：MntNs::new_initial() 只允许在
//!      注册表内首次调用时成功，第二次返回 E_INIT_DUP（判据 J2）。
//!   2. mount 表以 Vec<u16> 承载 mount 槽号，插入保证唯一（查重后入表），
//!      移出按值查找；表长度上限 NS_MNT_MAX=4096。
//!   3. 引用计数与 ns 生命周期绑定：refs 归零即 Free，Free 后任何操作拒收。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8480;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_INIT_DUP: u16 = SEG_BASE + 1;  // 第二个初始 ns 注入被拒
const E_ROOT_NULL: u16 = SEG_BASE + 2; // root mount 槽为空
const E_MNT_DUP: u16 = SEG_BASE + 3;   // mount 重复加入
const E_MNT_MISS: u16 = SEG_BASE + 4;  // 移出不在表的 mount
const E_NS_FULL: u16 = SEG_BASE + 5;   // mount 表满（超 4096）
const E_REF_ZERO: u16 = SEG_BASE + 6;  // 引用归零后操作/重复释放
const E_BAL: u16 = SEG_BASE + 7;       // 表账与引用账不平

/// mount 表容量上限（Linux MNT_NS 量级锚，本模块取 4096 便于校验）。
pub const NS_MNT_MAX: usize = 4096;

/// ns 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsState {
    Active = 0, // 活跃，可增删 mount
    Frozen = 1, // 冻结（挂起态），只读
    Free = 2,   // 引用归零，终态
}

/// 命名空间对象结构锚。
pub struct MntNs {
    pub root_mnt: u16,      // 根 mount 槽号（0=空）
    pub mounts: Vec<u16>,   // mount 表（槽号集合）
    pub refs: u32,          // 进程组引用计数
    pub state: NsState,     // 生命周期
    pub initial: bool,      // 是否初始命名空间
    pub adds: u32,          // 加入累计账
    pub dels: u32,          // 移出累计账
}

impl MntNs {
    /// 创建普通命名空间：root 必须非空，初值引用 1（创建者）。
    pub fn new_regular(root_mnt: u16) -> Result<MntNs, u16> {
        if root_mnt == 0 {
            return Err(E_ROOT_NULL);
        }
        let mut mounts = Vec::new();
        mounts.push(root_mnt); // 根 mount 恒在表内
        Ok(MntNs {
            root_mnt,
            mounts,
            refs: 1,
            state: NsState::Active,
            initial: false,
            adds: 1,
            dels: 0,
        })
    }

    /// 加入 mount：查重 → 容量校验 → 入表（判据 J3）。
    pub fn add_mount(&mut self, slot: u16) -> Result<usize, u16> {
        if self.state != NsState::Active {
            return Err(E_REF_ZERO);
        }
        if slot == 0 {
            return Err(E_ROOT_NULL);
        }
        for m in self.mounts.iter() {
            if *m == slot {
                return Err(E_MNT_DUP);
            }
        }
        if self.mounts.len() >= NS_MNT_MAX {
            return Err(E_NS_FULL);
        }
        self.mounts.push(slot);
        self.adds += 1;
        Ok(self.mounts.len())
    }

    /// 移出 mount：按值查找，根 mount 不可移出。
    pub fn del_mount(&mut self, slot: u16) -> Result<usize, u16> {
        if self.state != NsState::Active {
            return Err(E_REF_ZERO);
        }
        if slot == self.root_mnt {
            return Err(E_MNT_MISS); // 根不得移出
        }
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        for m in self.mounts.iter() {
            if *m == slot {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        let k = match idx {
            Some(v) => v,
            None => return Err(E_MNT_MISS),
        };
        self.mounts.remove(k);
        self.dels += 1;
        Ok(self.mounts.len())
    }

    /// 引用 +1（进程组加入）。
    pub fn get(&mut self) -> Result<u32, u16> {
        if self.state == NsState::Free {
            return Err(E_REF_ZERO);
        }
        self.refs += 1;
        Ok(self.refs)
    }

    /// 引用 -1；归零进 Free 并清表（判据 J4）。
    pub fn put(&mut self) -> Result<u32, u16> {
        if self.refs == 0 {
            return Err(E_REF_ZERO);
        }
        self.refs -= 1;
        if self.refs == 0 {
            self.state = NsState::Free;
            self.mounts.clear();
        }
        Ok(self.refs)
    }

    /// 冻结/解冻：Active ↔ Frozen。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.state != NsState::Active {
            return Err(E_REF_ZERO);
        }
        self.state = NsState::Frozen;
        Ok(())
    }

    pub fn thaw(&mut self) -> Result<(), u16> {
        if self.state != NsState::Frozen {
            return Err(E_REF_ZERO);
        }
        self.state = NsState::Active;
        Ok(())
    }

    /// 表账守恒：表长 == adds - dels。
    pub fn table_conserved(&self) -> bool {
        self.adds as i64 - self.dels as i64 == self.mounts.len() as i64
    }

    /// 根在位：root 必在表内（未释放时）。
    pub fn root_present(&self) -> bool {
        if self.state == NsState::Free {
            return self.mounts.is_empty();
        }
        for m in self.mounts.iter() {
            if *m == self.root_mnt {
                return true;
            }
        }
        false
    }
}

/// 命名空间注册表：初始 ns 唯一性守门（判据 J2）。
pub struct NsRegistry {
    pub namespaces: Vec<MntNs>, // ns 槽表
    pub initial_idx: Option<usize>, // 初始 ns 槽位（唯一）
    pub init_attempts: u32,     // 初始 ns 创建尝试账
    pub init_rej: u32,          // 重复创建拒绝账
}

impl NsRegistry {
    pub fn new() -> NsRegistry {
        NsRegistry {
            namespaces: Vec::new(),
            initial_idx: None,
            init_attempts: 0,
            init_rej: 0,
        }
    }

    /// 创建初始 ns：仅首次成功，后续一律 E_INIT_DUP（判据 J2 原文）。
    pub fn new_initial(&mut self, root_mnt: u16) -> Result<usize, u16> {
        self.init_attempts += 1;
        if self.initial_idx.is_some() {
            self.init_rej += 1;
            return Err(E_INIT_DUP);
        }
        let mut ns = MntNs::new_regular(root_mnt)?;
        ns.initial = true;
        self.namespaces.push(ns);
        let idx = self.namespaces.len() - 1;
        self.initial_idx = Some(idx);
        Ok(idx)
    }

    /// 创建子 ns（clone CLONE_NEWNS）：root 继承父 ns 的 root mount。
    pub fn clone_ns(&mut self, parent: usize) -> Result<usize, u16> {
        if parent >= self.namespaces.len() {
            return Err(E_ROOT_NULL);
        }
        if self.namespaces[parent].state == NsState::Free {
            return Err(E_REF_ZERO);
        }
        let root = self.namespaces[parent].root_mnt;
        let ns = MntNs::new_regular(root)?;
        self.namespaces.push(ns);
        Ok(self.namespaces.len() - 1)
    }

    /// 初始 ns 查询：唯一性读面。
    pub fn initial(&self) -> Option<usize> {
        self.initial_idx
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_INIT_DUP => "第二个初始命名空间注入被拒（全局唯一）",
        E_ROOT_NULL => "root mount 槽为空或父 ns 越界",
        E_MNT_DUP => "同一 mount 重复加入表",
        E_MNT_MISS => "移出不在表的 mount 或移出根",
        E_NS_FULL => "mount 表超容量 4096",
        E_REF_ZERO => "引用归零/冻结态操作或重复释放",
        E_BAL => "mount 表账与引用账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：初始唯一 → mount 增删 → 引用账 → 冻结 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut reg = NsRegistry::new();

    // R1：初始 ns 首次创建成功（判据 J1/J2 正向）。
    let i0 = reg.new_initial(1)?;
    if i0 != 0 || reg.initial() != Some(0) {
        return Err(E_INIT_DUP);
    }
    let ns = &reg.namespaces[i0];
    if ns.root_mnt != 1 || ns.refs != 1 || !ns.initial || ns.state != NsState::Active {
        return Err(E_ROOT_NULL);
    }
    if ns.mounts.len() != 1 || ns.mounts[0] != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：加入 mount 三次 → 表长 4，账守恒（判据 J3）。
    reg.namespaces[i0].add_mount(2)?;
    reg.namespaces[i0].add_mount(3)?;
    reg.namespaces[i0].add_mount(4)?;
    if reg.namespaces[i0].mounts.len() != 4 {
        return Err(E_BAL);
    }
    if !reg.namespaces[i0].table_conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：移出一个 mount → 表长 3；再移非法槽被拒。
    reg.namespaces[i0].del_mount(3)?;
    if reg.namespaces[i0].mounts.len() != 3 {
        return Err(E_BAL);
    }
    if !reg.namespaces[i0].table_conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：根 mount 不可移出（根在位不变式）。
    if reg.namespaces[i0].del_mount(1) != Err(E_MNT_MISS) {
        return Err(E_MNT_MISS);
    }
    if !reg.namespaces[i0].root_present() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：引用账——get 两次 refs=3，put 两次回 1（判据 J4）。
    reg.namespaces[i0].get()?;
    reg.namespaces[i0].get()?;
    if reg.namespaces[i0].refs != 3 {
        return Err(E_REF_ZERO);
    }
    reg.namespaces[i0].put()?;
    reg.namespaces[i0].put()?;
    if reg.namespaces[i0].refs != 1 || reg.namespaces[i0].state != NsState::Active {
        return Err(E_REF_ZERO);
    }
    hits += 1;

    // R6：子命名空间 clone——继承 root=1，独立表。
    let i1 = reg.clone_ns(i0)?;
    if reg.namespaces[i1].root_mnt != 1 || reg.namespaces[i1].initial {
        return Err(E_ROOT_NULL);
    }
    // 子 ns 加 mount 不影响初始 ns（表隔离）。
    reg.namespaces[i1].add_mount(9)?;
    if reg.namespaces[i0].mounts.len() != 3 || reg.namespaces[i1].mounts.len() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：冻结/解冻——Frozen 期增删被拒，解冻恢复。
    reg.namespaces[i1].freeze()?;
    if reg.namespaces[i1].add_mount(10) != Err(E_REF_ZERO) {
        return Err(E_REF_ZERO);
    }
    reg.namespaces[i1].thaw()?;
    reg.namespaces[i1].add_mount(10)?;
    if reg.namespaces[i1].mounts.len() != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R8：引用归零进 Free、表清空（判据 J4 终局）。
    reg.namespaces[i1].put()?;
    if reg.namespaces[i1].refs != 0 || reg.namespaces[i1].state != NsState::Free {
        return Err(E_REF_ZERO);
    }
    if !reg.namespaces[i1].mounts.is_empty() {
        return Err(E_BAL);
    }
    if !reg.namespaces[i1].root_present() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：第二个初始 ns 注入被拒（判据 J2 原文，3/3 重复验）。
    if reg.new_initial(2) != Err(E_INIT_DUP) {
        return Err(E_INIT_DUP);
    }
    if reg.new_initial(2) != Err(E_INIT_DUP) {
        return Err(E_INIT_DUP);
    }
    if reg.init_rej != 2 || reg.init_attempts != 3 {
        return Err(E_INIT_DUP);
    }
    hits += 1;

    // I2：重复加入同一 mount 注入被拒。
    if reg.namespaces[i0].add_mount(2) != Err(E_MNT_DUP) {
        return Err(E_MNT_DUP);
    }
    hits += 1;

    // I3：移出不在表的 mount 注入被拒。
    if reg.namespaces[i0].del_mount(77) != Err(E_MNT_MISS) {
        return Err(E_MNT_MISS);
    }
    hits += 1;

    // I4：Free 后 get/add 双注入被拒。
    if reg.namespaces[i1].get() != Err(E_REF_ZERO) {
        return Err(E_REF_ZERO);
    }
    if reg.namespaces[i1].add_mount(11) != Err(E_REF_ZERO) {
        return Err(E_REF_ZERO);
    }
    hits += 1;

    // I5：重复 put 到零后再 put 被拒。
    if reg.namespaces[i1].put() != Err(E_REF_ZERO) {
        return Err(E_REF_ZERO);
    }
    hits += 1;

    // I6：root=0 的 ns 构造注入被拒。
    if MntNs::new_regular(0).err() != Some(E_ROOT_NULL) {
        return Err(E_ROOT_NULL);
    }
    hits += 1;

    Ok(hits)
}
